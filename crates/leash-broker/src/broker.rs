//! The broker: proposals, a person's decision, and one execution per approval.
//!
//! A request becomes a *proposal* only after every check that does not need a person has passed: the command is
//! bound, the arguments validate, the binary still has its pinned hash. The proposal carries a *digest* of exactly what
//! would run. Approving means approving **that digest**: if anything differs, nothing runs. An approval is used once;
//! a second one, concurrent or late, finds the proposal already resolved and runs nothing.
//!
//! The ledger entry `started` is written **before** the process exists. If the broker dies between `started` and
//! `finished`, the record says the outcome is unknown; it never says "did not run".

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::time::Instant;

use crate::exec::{self, Execution, PinError, PinnedBinary};
use crate::ledger::{Kind, Ledger, LedgerError, Record};
use crate::manifest::{Approval, Manifest};
use crate::validate::{ValidateError, build_argv};

/// Who is deciding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    /// A person, identified by whoever authenticated them.
    Human(String),
    /// The broker's own policy: allowed only for commands whose manifest says `approval: never`.
    Policy,
}

impl Actor {
    fn label(&self) -> String {
        match self {
            Self::Human(name) => format!("human:{name}"),
            Self::Policy => "policy".to_owned(),
        }
    }
}

/// What the decider chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Approve the proposal whose digest is this. The digest is what the person was shown.
    Approve {
        digest: String,
    },
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    Pending,
    /// Approved and claimed; the process is being started or is running.
    Dispatching,
    Done {
        exit_code: Option<i32>,
        timed_out: bool,
        output_limit_hit: bool,
    },
    Denied,
    Expired,
    Failed {
        reason: String,
    },
}

/// What a person is shown, and what the API returns.
#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    pub id: String,
    pub command_id: String,
    pub description: String,
    /// The argument vector exactly as it will be passed (after `--`, positionals follow).
    pub argv: Vec<String>,
    pub binary: String,
    pub binary_sha256: String,
    /// SHA-256 over (command, version, binary hash, argv). Approving means approving this.
    pub digest: String,
    pub approval: Approval,
    #[serde(flatten)]
    pub state: State,
}

/// A request turned down before it became a proposal.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Refusal {
    #[error("no command {0:?} is bound")]
    UnknownCommand(String),
    #[error("{0}")]
    Invalid(ValidateError),
    #[error("the binary no longer matches its pinned hash")]
    BinaryChanged,
    #[error("too many proposals are waiting; resolve or let some expire")]
    TooManyPending,
    #[error("the ledger could not be written; nothing was accepted")]
    LedgerUnavailable,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ResolveError {
    #[error("no such proposal")]
    NotFound,
    #[error("already resolved; nothing ran this time")]
    AlreadyResolved(State),
    #[error("the digest does not match this proposal: it may have changed since it was shown")]
    DigestMismatch,
    #[error("this command needs a person's approval; policy cannot give it")]
    HumanRequired,
    #[error("the proposal expired")]
    Expired,
    #[error("the ledger could not be written; nothing ran")]
    LedgerUnavailable,
}

/// Result of resolving a proposal.
#[derive(Debug)]
pub struct Resolution {
    pub proposal: Proposal,
    /// Present when the command ran.
    pub execution: Option<Execution>,
}

struct Entry {
    proposal: Proposal,
    created: Instant,
    manifest: Manifest,
}

struct Inner {
    ledger: Ledger,
    proposals: BTreeMap<String, Entry>,
}

/// Tunables.
#[derive(Debug, Clone, Copy)]
pub struct BrokerConfig {
    /// A proposal not decided within this long can no longer be approved.
    pub proposal_ttl: Duration,
    /// Proposals waiting for a decision, at most.
    pub max_pending: usize,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            proposal_ttl: Duration::from_secs(15 * 60),
            max_pending: 64,
        }
    }
}

pub struct Broker {
    manifests: BTreeMap<String, Manifest>,
    config: BrokerConfig,
    workdir: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl std::fmt::Debug for Broker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Broker")
            .field("commands", &self.manifests.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn digest_of(manifest: &Manifest, argv: &[String]) -> String {
    let mut hasher = Sha256::new();
    for part in [
        manifest.id.as_str(),
        manifest.version.as_str(),
        manifest.binary_sha256.as_str(),
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hasher.update((argv.len() as u64).to_be_bytes());
    for arg in argv {
        hasher.update((arg.len() as u64).to_be_bytes());
        hasher.update(arg.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn random_id() -> Option<String> {
    let mut bytes = [0u8; 12];
    // An unavailable system RNG is not recoverable here, and a predictable id would make approvals guessable.
    getrandom::fill(&mut bytes).ok()?;
    Some(hex::encode(bytes))
}

impl Broker {
    /// # Errors
    /// A manifest repeats an id.
    pub fn new(
        manifests: Vec<Manifest>,
        ledger: Ledger,
        workdir: PathBuf,
        config: BrokerConfig,
    ) -> Result<Self, crate::manifest::ManifestError> {
        let mut map = BTreeMap::new();
        for manifest in manifests {
            manifest.check()?;
            let id = manifest.id.clone();
            if map.insert(id.clone(), manifest).is_some() {
                return Err(crate::manifest::ManifestError::DuplicateId(id));
            }
        }
        Ok(Self {
            manifests: map,
            config,
            workdir,
            inner: Arc::new(Mutex::new(Inner {
                ledger,
                proposals: BTreeMap::new(),
            })),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[must_use]
    pub fn commands(&self) -> Vec<&Manifest> {
        self.manifests.values().collect()
    }

    fn record(inner: &mut Inner, record: Record) -> Result<(), LedgerError> {
        inner.ledger.append(record).map(|_| ())
    }

    /// Ask to run a bound command. Nothing runs yet.
    ///
    /// # Errors
    /// See [`Refusal`]. Every refusal is written to the ledger.
    pub fn propose(
        &self,
        command_id: &str,
        args: &BTreeMap<String, String>,
        flags: &[String],
    ) -> Result<Proposal, Refusal> {
        let refuse = |inner: &mut Inner, why: &str, detail: String| {
            let _ = Self::record(
                inner,
                Record {
                    kind: Kind::Refused,
                    command_id: command_id.chars().take(64).collect(),
                    proposal_id: None,
                    binary_sha256: None,
                    detail: json!({"reason": why, "detail": detail}),
                },
            );
        };

        let Some(manifest) = self.manifests.get(command_id) else {
            refuse(&mut self.lock(), "unknown command", String::new());
            return Err(Refusal::UnknownCommand(
                command_id.chars().take(64).collect(),
            ));
        };
        let argv = match build_argv(manifest, args, flags) {
            Ok(argv) => argv,
            Err(error) => {
                refuse(&mut self.lock(), "invalid arguments", error.to_string());
                return Err(Refusal::Invalid(error));
            }
        };
        if PinnedBinary::open(&manifest.binary, &manifest.binary_sha256).is_err() {
            refuse(
                &mut self.lock(),
                "binary does not match its pinned hash",
                String::new(),
            );
            return Err(Refusal::BinaryChanged);
        }

        let Some(id) = random_id() else {
            return Err(Refusal::LedgerUnavailable);
        };
        let digest = digest_of(manifest, &argv);
        let proposal = Proposal {
            id: id.clone(),
            command_id: manifest.id.clone(),
            description: manifest.description.clone(),
            argv: argv.clone(),
            binary: manifest.binary.clone(),
            binary_sha256: manifest.binary_sha256.clone(),
            digest: digest.clone(),
            approval: manifest.approval,
            state: State::Pending,
        };

        let mut inner = self.lock();
        let pending = inner
            .proposals
            .values()
            .filter(|e| e.proposal.state == State::Pending)
            .count();
        if pending >= self.config.max_pending {
            refuse(&mut inner, "too many pending proposals", String::new());
            return Err(Refusal::TooManyPending);
        }
        Self::record(
            &mut inner,
            Record {
                kind: Kind::Proposed,
                command_id: manifest.id.clone(),
                proposal_id: Some(id.clone()),
                binary_sha256: Some(manifest.binary_sha256.clone()),
                detail: json!({"digest": digest, "argv": argv, "approval": manifest.approval}),
            },
        )
        .map_err(|_| Refusal::LedgerUnavailable)?;
        inner.proposals.insert(
            id,
            Entry {
                proposal: proposal.clone(),
                created: Instant::now(),
                manifest: manifest.clone(),
            },
        );
        Ok(proposal)
    }

    /// A proposal as it is now.
    #[must_use]
    pub fn proposal(&self, id: &str) -> Option<Proposal> {
        self.lock().proposals.get(id).map(|e| e.proposal.clone())
    }

    /// All proposals, newest last.
    #[must_use]
    pub fn proposals(&self) -> Vec<Proposal> {
        let inner = self.lock();
        let mut all: Vec<&Entry> = inner.proposals.values().collect();
        all.sort_by_key(|e| e.created);
        all.into_iter().map(|e| e.proposal.clone()).collect()
    }

    /// Decide a proposal; if approved, run it, once.
    ///
    /// # Errors
    /// See [`ResolveError`]. In every error case nothing was started.
    pub async fn resolve(
        &self,
        id: &str,
        decision: Decision,
        actor: Actor,
    ) -> Result<Resolution, ResolveError> {
        // Phase 1, under the lock: every check, then the claim. The claim is what makes an approval single-use.
        let (manifest, argv, proposal_id) = {
            let mut inner = self.lock();
            let Some(entry) = inner.proposals.get_mut(id) else {
                return Err(ResolveError::NotFound);
            };
            if entry.proposal.state != State::Pending {
                return Err(ResolveError::AlreadyResolved(entry.proposal.state.clone()));
            }
            let manifest = entry.manifest.clone();
            let command_id = manifest.id.clone();
            let binary_sha = Some(manifest.binary_sha256.clone());

            if entry.created.elapsed() > self.config.proposal_ttl {
                entry.proposal.state = State::Expired;
                let _ = Self::record(
                    &mut inner,
                    Record {
                        kind: Kind::Expired,
                        command_id,
                        proposal_id: Some(id.to_owned()),
                        binary_sha256: binary_sha,
                        detail: json!({}),
                    },
                );
                return Err(ResolveError::Expired);
            }
            let rec = |kind, detail| Record {
                kind,
                command_id: command_id.clone(),
                proposal_id: Some(id.to_owned()),
                binary_sha256: binary_sha.clone(),
                detail,
            };

            match decision {
                Decision::Deny => {
                    entry.proposal.state = State::Denied;
                    let proposal = entry.proposal.clone();
                    Self::record(&mut inner, rec(Kind::Denied, json!({"by": actor.label()})))
                        .map_err(|_| ResolveError::LedgerUnavailable)?;
                    return Ok(Resolution {
                        proposal,
                        execution: None,
                    });
                }
                Decision::Approve { digest } => {
                    if digest != entry.proposal.digest {
                        let _ = Self::record(
                            &mut inner,
                            rec(
                                Kind::Refused,
                                json!({"reason": "approval digest mismatch", "by": actor.label()}),
                            ),
                        );
                        return Err(ResolveError::DigestMismatch);
                    }
                    if manifest.approval == Approval::Always && !matches!(actor, Actor::Human(_)) {
                        let _ = Self::record(
                            &mut inner,
                            rec(
                                Kind::Refused,
                                json!({"reason": "human approval required", "by": actor.label()}),
                            ),
                        );
                        return Err(ResolveError::HumanRequired);
                    }
                    entry.proposal.state = State::Dispatching;
                    let argv = entry.proposal.argv.clone();
                    // `approved` and `started` before the process exists. If either cannot be written, nothing runs.
                    let written = Self::record(
                        &mut inner,
                        rec(Kind::Approved, json!({"by": actor.label()})),
                    )
                    .and_then(|()| {
                        Self::record(&mut inner, rec(Kind::Started, json!({"argv": argv})))
                    });
                    if written.is_err() {
                        if let Some(entry) = inner.proposals.get_mut(id) {
                            entry.proposal.state = State::Failed {
                                reason: "ledger unavailable".into(),
                            };
                        }
                        return Err(ResolveError::LedgerUnavailable);
                    }
                    (manifest, argv, id.to_owned())
                }
            }
        };

        // Phase 2, no lock held: verify the pin again, on the descriptor that will be executed, then run.
        let outcome = match PinnedBinary::open(&manifest.binary, &manifest.binary_sha256) {
            Err(error) => Err(match error {
                PinError::HashMismatch { .. } => {
                    "the binary changed after the proposal was made".to_owned()
                }
                other => other.to_string(),
            }),
            Ok(pinned) => exec::run(
                &pinned,
                &argv,
                manifest.limits,
                Duration::from_secs(manifest.timeout_seconds),
                &self.workdir,
            )
            .await
            .map_err(|e| e.to_string()),
        };

        // Phase 3: record the outcome.
        let mut inner = self.lock();
        let (state, detail, execution) = match outcome {
            Ok(execution) => (
                State::Done {
                    exit_code: execution.exit_code,
                    timed_out: execution.timed_out,
                    output_limit_hit: execution.output_limit_hit,
                },
                json!({
                    "exit_code": execution.exit_code, "signal": execution.signal, "timed_out": execution.timed_out,
                    "output_limit_hit": execution.output_limit_hit, "duration_ms": execution.duration_ms,
                    "stdout_bytes": execution.stdout.len(), "stderr_bytes": execution.stderr.len(),
                }),
                Some(execution),
            ),
            Err(reason) => (
                State::Failed {
                    reason: reason.clone(),
                },
                json!({"error": reason}),
                None,
            ),
        };
        let _ = Self::record(
            &mut inner,
            Record {
                kind: Kind::Finished,
                command_id: manifest.id.clone(),
                proposal_id: Some(proposal_id.clone()),
                binary_sha256: Some(manifest.binary_sha256.clone()),
                detail,
            },
        );
        let proposal = match inner.proposals.get_mut(&proposal_id) {
            Some(entry) => {
                entry.proposal.state = state;
                entry.proposal.clone()
            }
            None => return Err(ResolveError::NotFound),
        };
        Ok(Resolution {
            proposal,
            execution,
        })
    }

    /// Head hash and length of the ledger, to record somewhere the broker cannot write.
    #[must_use]
    pub fn ledger_anchor(&self) -> (u64, String) {
        let inner = self.lock();
        (inner.ledger.len(), inner.ledger.head().to_owned())
    }

    #[must_use]
    pub fn ledger_path(&self) -> PathBuf {
        self.lock().ledger.path().to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ArgKind, ArgSpec, Limits};

    fn broker_with_failing_ledger() -> (Broker, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("dir");
        let echo = dir.path().join("say");
        std::fs::copy("/usr/bin/echo", &echo).expect("copy echo");
        let hash = exec::sha256_of(&echo).expect("hash");
        let manifest = Manifest {
            id: "say".into(),
            version: "1".into(),
            description: "say".into(),
            binary: echo.display().to_string(),
            binary_sha256: hash,
            args: vec![ArgSpec {
                name: "word".into(),
                kind: ArgKind::Token,
                required: true,
                position: 0,
            }],
            allowed_flags: vec![],
            timeout_seconds: 5,
            limits: Limits::default(),
            approval: Approval::Always,
            end_of_options: false,
        };
        let ledger = Ledger::open(&dir.path().join("l.jsonl"), None).expect("ledger");
        let broker = Broker::new(
            vec![manifest],
            ledger,
            dir.path().to_path_buf(),
            BrokerConfig::default(),
        )
        .expect("broker");
        (broker, dir)
    }

    #[tokio::test]
    async fn if_the_ledger_cannot_record_the_start_the_process_is_never_created() {
        let (broker, dir) = broker_with_failing_ledger();
        let args = BTreeMap::from([("word".to_owned(), "hello".to_owned())]);
        let proposal = broker.propose("say", &args, &[]).expect("proposal");

        // The marker would be created by a run only if the process started; echo cannot create it, so instead the
        // assertion is on the outcome and on the state: no execution, and the proposal is marked failed.
        broker.lock().ledger.failing = true;
        let result = broker
            .resolve(
                &proposal.id,
                Decision::Approve {
                    digest: proposal.digest.clone(),
                },
                Actor::Human("t".into()),
            )
            .await;
        assert_eq!(
            result.expect_err("must not run"),
            ResolveError::LedgerUnavailable
        );
        assert!(matches!(
            broker.proposal(&proposal.id).expect("exists").state,
            State::Failed { .. }
        ));
        drop(dir);
    }

    #[tokio::test]
    async fn if_the_ledger_cannot_record_a_proposal_there_is_no_proposal() {
        let (broker, _dir) = broker_with_failing_ledger();
        broker.lock().ledger.failing = true;
        let args = BTreeMap::from([("word".to_owned(), "hello".to_owned())]);
        assert_eq!(
            broker.propose("say", &args, &[]).expect_err("refused"),
            Refusal::LedgerUnavailable
        );
        assert!(broker.proposals().is_empty());
    }
}
