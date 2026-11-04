//! The broker end to end: propose, decide, run once, and what the ledger says afterwards.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{bind_copy, manifest, system_program, token};
use leash_broker::{
    Actor, Approval, Broker, BrokerConfig, Decision, Ledger, LedgerError, Refusal, ResolveError,
    State, ValidateError, verify_ledger,
};

struct Fixture {
    dir: tempfile::TempDir,
    broker: Arc<Broker>,
    say_path: String,
}

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// Opens the ledger again once whoever held it has let go.
///
/// The lock is `flock(2)`, which belongs to the open file description, not to the process. Tests in this file run on
/// parallel threads of one process and some of them fork a child; in the instant between its `fork` and `exec` that
/// child holds a copy of every descriptor, so a lock this test has just released can look held for a few
/// microseconds. Waiting a bounded time for that is correct; waiting forever, or not at all, is not.
fn reopen(path: &Path) -> Ledger {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match Ledger::open(path, None) {
            Ok(ledger) => return ledger,
            Err(LedgerError::Locked) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("the ledger could not be reopened: {e}"),
        }
    }
}

fn fixture_with(config: BrokerConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let (say, say_hash) = bind_copy(dir.path(), "echo", "say");
    let (name, name_hash) = bind_copy(dir.path(), "uname", "kernel");
    let work = dir.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.jsonl"), None).unwrap();
    let broker = Broker::new(
        vec![
            manifest(
                "say",
                &say,
                &say_hash,
                vec![token("word", 0)],
                &[],
                Approval::Always,
            ),
            manifest(
                "kernel",
                &name,
                &name_hash,
                vec![],
                &["-s"],
                Approval::Never,
            ),
        ],
        ledger,
        work,
        config,
    )
    .unwrap();
    Fixture {
        dir,
        broker: Arc::new(broker),
        say_path: say,
    }
}

fn fixture() -> Fixture {
    fixture_with(BrokerConfig::default())
}

fn human() -> Actor {
    Actor::Human("tester".into())
}

fn kinds(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn ledger_path(f: &Fixture) -> std::path::PathBuf {
    f.dir.path().join("ledger.jsonl")
}

#[tokio::test]
async fn a_proposal_runs_only_after_a_person_approves_its_digest_and_leaves_a_complete_trail() {
    let f = fixture();
    let proposal = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    assert_eq!(proposal.state, State::Pending);
    assert_eq!(proposal.argv, vec!["hello"]);

    let done = f
        .broker
        .resolve(
            &proposal.id,
            Decision::Approve {
                digest: proposal.digest.clone(),
            },
            human(),
        )
        .await
        .unwrap();
    assert!(matches!(
        done.proposal.state,
        State::Done {
            exit_code: Some(0),
            ..
        }
    ));
    assert_eq!(
        String::from_utf8_lossy(&done.execution.unwrap().stdout),
        "hello\n"
    );

    let report = verify_ledger(&ledger_path(&f), None);
    assert!(report.is_valid());
    assert!(report.unfinished.is_empty());
    assert_eq!(
        kinds(&ledger_path(&f)),
        ["proposed", "approved", "started", "finished"]
    );
}

#[tokio::test]
async fn nothing_runs_without_a_decision() {
    let f = fixture();
    f.broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    assert_eq!(
        kinds(&ledger_path(&f)),
        ["proposed"],
        "a proposal alone never starts a process"
    );
}

#[tokio::test]
async fn hostile_requests_are_refused_before_a_proposal_exists_and_are_recorded() {
    let f = fixture();
    let attempts: Vec<(&str, BTreeMap<String, String>, Vec<String>)> = vec![
        ("say", args(&[("word", "-n")]), vec![]),
        ("say", args(&[("word", "hello; rm -rf /")]), vec![]),
        ("say", args(&[("word", "$(id)")]), vec![]),
        ("say", args(&[("word", "x"), ("extra", "y")]), vec![]),
        ("say", args(&[]), vec![]),
        ("say", args(&[("word", "ok")]), vec!["-e".into()]),
        ("kernel", args(&[]), vec!["-a".into()]),
        ("kernel", args(&[]), vec!["-s".into(), "-s".into()]),
        ("sh", args(&[("word", "ok")]), vec![]),
        ("", args(&[]), vec![]),
    ];
    let count = attempts.len();
    for (command, arguments, flags) in attempts {
        assert!(
            f.broker.propose(command, &arguments, &flags).is_err(),
            "accepted {command:?} {arguments:?} {flags:?}"
        );
    }
    assert!(f.broker.proposals().is_empty());
    let recorded = kinds(&ledger_path(&f));
    assert_eq!(recorded.len(), count);
    assert!(recorded.iter().all(|k| k == "refused"));
    assert!(verify_ledger(&ledger_path(&f), None).is_valid());
}

#[tokio::test]
async fn a_hostile_value_is_not_copied_into_the_ledger_verbatim() {
    let f = fixture();
    let hostile = format!("a\nFORGED LINE {}", "x".repeat(5000));
    let error = f
        .broker
        .propose("say", &args(&[("word", &hostile)]), &[])
        .unwrap_err();
    assert!(matches!(
        error,
        Refusal::Invalid(ValidateError::BadValue { .. })
    ));
    let text = std::fs::read_to_string(ledger_path(&f)).unwrap();
    assert!(
        !text.contains("FORGED LINE xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
        "the value was cut"
    );
    assert_eq!(
        text.lines().count(),
        1,
        "and its newline did not start a second ledger line"
    );
}

#[tokio::test]
async fn a_binary_changed_before_the_proposal_is_refused() {
    let f = fixture();
    std::fs::remove_file(&f.say_path).unwrap();
    std::fs::copy(system_program("true"), &f.say_path).unwrap();
    assert_eq!(
        f.broker
            .propose("say", &args(&[("word", "hello")]), &[])
            .unwrap_err(),
        Refusal::BinaryChanged
    );
}

#[tokio::test]
async fn a_binary_changed_between_proposal_and_approval_never_runs() {
    let f = fixture();
    let proposal = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    std::fs::remove_file(&f.say_path).unwrap();
    std::fs::copy(system_program("true"), &f.say_path).unwrap();

    let done = f
        .broker
        .resolve(
            &proposal.id,
            Decision::Approve {
                digest: proposal.digest.clone(),
            },
            human(),
        )
        .await
        .unwrap();
    assert!(matches!(done.proposal.state, State::Failed { .. }));
    assert!(done.execution.is_none());
    let report = verify_ledger(&ledger_path(&f), None);
    assert!(
        report.unfinished.is_empty(),
        "the run was closed with a finished entry that records the failure"
    );
}

#[tokio::test]
async fn policy_cannot_approve_what_needs_a_person_but_can_approve_what_does_not() {
    let f = fixture();
    let needs_person = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    let denied = f
        .broker
        .resolve(
            &needs_person.id,
            Decision::Approve {
                digest: needs_person.digest.clone(),
            },
            Actor::Policy,
        )
        .await;
    assert_eq!(denied.unwrap_err(), ResolveError::HumanRequired);
    assert_eq!(
        f.broker.proposal(&needs_person.id).unwrap().state,
        State::Pending,
        "still waiting for a person"
    );
    assert!(!kinds(&ledger_path(&f)).contains(&"started".to_owned()));

    let harmless = f
        .broker
        .propose("kernel", &args(&[]), &["-s".into()])
        .unwrap();
    let done = f
        .broker
        .resolve(
            &harmless.id,
            Decision::Approve {
                digest: harmless.digest.clone(),
            },
            Actor::Policy,
        )
        .await
        .unwrap();
    assert!(matches!(
        done.proposal.state,
        State::Done {
            exit_code: Some(0),
            ..
        }
    ));
    assert_eq!(
        String::from_utf8_lossy(&done.execution.unwrap().stdout).trim(),
        "Linux"
    );
}

#[tokio::test]
async fn approving_a_different_digest_runs_nothing() {
    let f = fixture();
    let first = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    let other = f
        .broker
        .propose("say", &args(&[("word", "goodbye")]), &[])
        .unwrap();
    assert_ne!(first.digest, other.digest);

    let wrong = f
        .broker
        .resolve(
            &first.id,
            Decision::Approve {
                digest: other.digest.clone(),
            },
            human(),
        )
        .await;
    assert_eq!(wrong.unwrap_err(), ResolveError::DigestMismatch);
    let garbage = f
        .broker
        .resolve(
            &first.id,
            Decision::Approve {
                digest: "0".repeat(64),
            },
            human(),
        )
        .await;
    assert_eq!(garbage.unwrap_err(), ResolveError::DigestMismatch);
    assert_eq!(f.broker.proposal(&first.id).unwrap().state, State::Pending);
    assert!(!kinds(&ledger_path(&f)).contains(&"started".to_owned()));
}

#[tokio::test]
async fn the_digest_covers_the_command_the_binary_and_every_argument() {
    let f = fixture();
    let a = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    let b = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    let c = f
        .broker
        .propose("say", &args(&[("word", "hellp")]), &[])
        .unwrap();
    assert_eq!(a.digest, b.digest, "the same request has the same digest");
    assert_ne!(a.id, b.id, "but not the same id");
    assert_ne!(a.digest, c.digest);
}

#[tokio::test]
async fn an_approval_is_used_once() {
    let f = fixture();
    let proposal = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    let approve = || Decision::Approve {
        digest: proposal.digest.clone(),
    };
    f.broker
        .resolve(&proposal.id, approve(), human())
        .await
        .unwrap();
    let again = f.broker.resolve(&proposal.id, approve(), human()).await;
    assert!(matches!(
        again.unwrap_err(),
        ResolveError::AlreadyResolved(State::Done { .. })
    ));
    assert_eq!(
        kinds(&ledger_path(&f))
            .iter()
            .filter(|k| *k == "started")
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_approvals_start_exactly_one_process() {
    let f = fixture();
    let proposal = f
        .broker
        .propose("say", &args(&[("word", "once")]), &[])
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..24 {
        let broker = Arc::clone(&f.broker);
        let (id, digest) = (proposal.id.clone(), proposal.digest.clone());
        tasks.spawn(async move {
            broker
                .resolve(
                    &id,
                    Decision::Approve { digest },
                    Actor::Human("racer".into()),
                )
                .await
        });
    }
    let (mut ran, mut refused) = (0, 0);
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            Ok(resolution) => {
                assert!(resolution.execution.is_some());
                ran += 1;
            }
            Err(ResolveError::AlreadyResolved(_)) => refused += 1,
            Err(other) => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!((ran, refused), (1, 23));
    assert_eq!(
        kinds(&ledger_path(&f))
            .iter()
            .filter(|k| *k == "started")
            .count(),
        1
    );
    assert!(verify_ledger(&ledger_path(&f), None).is_valid());
}

#[tokio::test]
async fn a_denied_proposal_cannot_be_approved_later() {
    let f = fixture();
    let proposal = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    f.broker
        .resolve(&proposal.id, Decision::Deny, human())
        .await
        .unwrap();
    let late = f
        .broker
        .resolve(
            &proposal.id,
            Decision::Approve {
                digest: proposal.digest.clone(),
            },
            human(),
        )
        .await;
    assert_eq!(
        late.unwrap_err(),
        ResolveError::AlreadyResolved(State::Denied)
    );
    assert!(!kinds(&ledger_path(&f)).contains(&"started".to_owned()));
}

#[tokio::test]
async fn an_unknown_proposal_is_not_found() {
    let f = fixture();
    assert_eq!(
        f.broker
            .resolve("nope", Decision::Deny, human())
            .await
            .unwrap_err(),
        ResolveError::NotFound
    );
}

#[tokio::test(start_paused = true)]
async fn an_old_proposal_expires_instead_of_running() {
    let f = fixture_with(BrokerConfig {
        proposal_ttl: std::time::Duration::from_secs(60),
        ..BrokerConfig::default()
    });
    let proposal = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    let late = f
        .broker
        .resolve(
            &proposal.id,
            Decision::Approve {
                digest: proposal.digest.clone(),
            },
            human(),
        )
        .await;
    assert_eq!(late.unwrap_err(), ResolveError::Expired);
    assert_eq!(
        f.broker.proposal(&proposal.id).unwrap().state,
        State::Expired
    );
    assert!(kinds(&ledger_path(&f)).contains(&"expired".to_owned()));
}

#[tokio::test]
async fn the_queue_of_waiting_proposals_is_bounded() {
    let f = fixture_with(BrokerConfig {
        max_pending: 3,
        ..BrokerConfig::default()
    });
    for i in 0..3 {
        f.broker
            .propose("say", &args(&[("word", &format!("w{i}"))]), &[])
            .unwrap();
    }
    assert_eq!(
        f.broker
            .propose("say", &args(&[("word", "w4")]), &[])
            .unwrap_err(),
        Refusal::TooManyPending
    );
}

#[tokio::test]
async fn the_ledger_keeps_working_across_a_restart_of_the_broker() {
    let f = fixture();
    let path = ledger_path(&f);
    let p = f
        .broker
        .propose("say", &args(&[("word", "hello")]), &[])
        .unwrap();
    f.broker
        .resolve(&p.id, Decision::Approve { digest: p.digest }, human())
        .await
        .unwrap();
    let (length, head) = f.broker.ledger_anchor();
    assert_eq!(length, 4);
    drop(f.broker); // releases the ledger's lock, as a restart would

    let reopened = reopen(&path);
    assert_eq!((reopened.len(), reopened.head()), (4, head.as_str()));
    assert_eq!(
        leash_broker::check_anchor(&path, None, length, &head),
        Ok(())
    );
}
