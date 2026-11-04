//! The lab: endpoints that attack the system on purpose, so a visitor can watch each defence hold.
//! They exist only with `LEASH_LAB=1` and the server refuses to start with it in production.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use leash_broker::{
    Actor, AnchorMismatch, Decision, Ledger, LedgerEntry, LedgerRecord, check_anchor, verify_ledger,
};
use leash_gateway::Behaviour;
use serde::Deserialize;
use serde_json::{Value, json};

use super::broker::{ledger_key, refusal_error};
use super::require_lab;
use crate::error::{ApiError, ApiJson};
use crate::state::Shared;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviourIn {
    Healthy,
    Failing,
    Slow,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderIn {
    pub behaviour: BehaviourIn,
}

/// Make one of the two stand-in models healthy, failing, or slower than the call timeout.
pub async fn provider(
    State(state): State<Shared>,
    UrlPath(id): UrlPath<String>,
    ApiJson(input): ApiJson<ProviderIn>,
) -> Result<Json<Value>, ApiError> {
    require_lab(&state)?;
    let target = match id.as_str() {
        "local-stand-in" => &state.local,
        "remote-stand-in" => &state.remote,
        _ => {
            return Err(ApiError::not_found(
                "no_such_provider",
                "only local-stand-in and remote-stand-in can be controlled",
            ));
        }
    };
    let behaviour = match input.behaviour {
        BehaviourIn::Healthy => Behaviour::Healthy,
        BehaviourIn::Failing => Behaviour::Failing,
        BehaviourIn::Slow => Behaviour::Slow(Duration::from_secs(5)),
    };
    target.set_behaviour(behaviour);
    Ok(Json(
        json!({"provider": id, "behaviour": super::gateway::behaviour_name(target.behaviour()), "calls": target.calls(), "providers": state.gateway.status()}),
    ))
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TamperWhen {
    /// The file changes before anyone asks to run it.
    BeforeProposal,
    /// The file changes after a person was shown the proposal and approved it.
    AfterApproval,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TamperIn {
    pub when: TamperWhen,
}

fn one_arg() -> BTreeMap<String, String> {
    BTreeMap::from([("word".to_owned(), "hello".to_owned())])
}

/// Change the bound binary by one byte and try to run it. The original is put back afterwards.
pub async fn tamper_binary(
    State(state): State<Shared>,
    ApiJson(input): ApiJson<TamperIn>,
) -> Result<Json<Value>, ApiError> {
    require_lab(&state)?;
    let outcome = tamper(&state, input.when).await;
    // Whatever happened, the demo binary is restored so the lab can be run again.
    state
        .scratch
        .restore()
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    outcome
}

async fn tamper(state: &Shared, when: TamperWhen) -> Result<Json<Value>, ApiError> {
    match when {
        TamperWhen::BeforeProposal => {
            state
                .scratch
                .tamper()
                .map_err(|e| ApiError::internal(&e.to_string()))?;
            let attempt = state.broker.propose("scratch", &one_arg(), &[]);
            let refused = attempt
                .as_ref()
                .err()
                .map(|r| json!({"code": refusal_error(r).code(), "message": r.to_string()}));
            Ok(Json(json!({
                "when": "before_proposal",
                "binary_changed": true,
                "proposal_created": attempt.is_ok(),
                "refusal": refused,
                "held": attempt.is_err(),
            })))
        }
        TamperWhen::AfterApproval => {
            let proposal = state
                .broker
                .propose("scratch", &one_arg(), &[])
                .map_err(|r| refusal_error(&r))?;
            state
                .scratch
                .tamper()
                .map_err(|e| ApiError::internal(&e.to_string()))?;
            let resolution = state
                .broker
                .resolve(
                    &proposal.id,
                    Decision::Approve {
                        digest: proposal.digest.clone(),
                    },
                    Actor::Human("lab".to_owned()),
                )
                .await
                .map_err(|e| ApiError::conflict("resolve_failed", e.to_string()))?;
            let ran = resolution.execution.is_some();
            Ok(Json(json!({
                "when": "after_approval",
                "binary_changed": true,
                "approved_by_a_person": true,
                "ran": ran,
                "proposal": resolution.proposal,
                "held": !ran,
            })))
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForgeMode {
    /// Change a field of an entry in the middle.
    Edit,
    /// Swap two entries.
    Reorder,
    /// Cut the end off.
    Truncate,
    /// Rebuild the chain without one entry, the way someone who does not have the key would.
    Rewrite,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgeIn {
    pub mode: ForgeMode,
}

const MIN_HISTORY: u64 = 6;

/// Make sure there is something to forge: run the read-only commands until the ledger has a few entries.
async fn ensure_history(state: &Shared) -> Result<(), ApiError> {
    let mut guard = 0;
    while state.broker.ledger_anchor().0 < MIN_HISTORY && guard < 6 {
        guard += 1;
        let proposal = state
            .broker
            .propose("kernel", &BTreeMap::new(), &[])
            .map_err(|r| refusal_error(&r))?;
        state
            .broker
            .resolve(
                &proposal.id,
                Decision::Approve {
                    digest: proposal.digest.clone(),
                },
                Actor::Policy,
            )
            .await
            .map_err(|e| ApiError::conflict("resolve_failed", e.to_string()))?;
    }
    Ok(())
}

fn anchor_name(result: Option<&Result<(), AnchorMismatch>>) -> Value {
    match result {
        None => json!({"result": "not_checked", "why": "the chain is already broken"}),
        Some(Ok(())) => json!({"result": "matches"}),
        Some(Err(AnchorMismatch::Truncated { expected, found })) => {
            json!({"result": "truncated", "expected": expected, "found": found})
        }
        Some(Err(AnchorMismatch::Rewritten)) => json!({"result": "rewritten"}),
    }
}

/// Forge a copy of the ledger and ask the verifier whether it notices. The real ledger is never touched.
pub async fn forge_ledger(
    State(state): State<Shared>,
    ApiJson(input): ApiJson<ForgeIn>,
) -> Result<Json<Value>, ApiError> {
    require_lab(&state)?;
    ensure_history(&state).await?;
    let key = ledger_key(&state);
    let (kept_length, kept_head) = state.broker.ledger_anchor();
    let original = tokio::fs::read_to_string(state.broker.ledger_path())
        .await
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();
    let middle = lines.len() / 2;
    let copy = state.work_dir.join("forged.jsonl");
    let _ = std::fs::remove_file(&copy);

    let explanation = match input.mode {
        ForgeMode::Edit => {
            let mut entry: Value = serde_json::from_str(&lines[middle])
                .map_err(|e| ApiError::internal(&e.to_string()))?;
            entry["command_id"] = json!("forged");
            lines[middle] = entry.to_string();
            "one field of an entry in the middle was changed"
        }
        ForgeMode::Reorder => {
            lines.swap(1, 2);
            "two entries were swapped"
        }
        ForgeMode::Truncate => {
            lines.truncate(lines.len() - 2);
            "the last two entries were cut off"
        }
        ForgeMode::Rewrite => {
            // The forger replays the records into a fresh chain, leaving one out. They do not have the key.
            let mut forged =
                Ledger::open(&copy, None).map_err(|e| ApiError::internal(&e.to_string()))?;
            for (index, line) in lines.iter().enumerate() {
                if index == middle {
                    continue;
                }
                let entry: LedgerEntry =
                    serde_json::from_str(line).map_err(|e| ApiError::internal(&e.to_string()))?;
                forged
                    .append(LedgerRecord {
                        kind: entry.kind,
                        command_id: entry.command_id,
                        proposal_id: entry.proposal_id,
                        binary_sha256: entry.binary_sha256,
                        detail: entry.detail,
                    })
                    .map_err(|e| ApiError::internal(&e.to_string()))?;
            }
            drop(forged);
            "the chain was rebuilt without one entry, by someone who does not have the key"
        }
    };
    if !matches!(input.mode, ForgeMode::Rewrite) {
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&copy, text).map_err(|e| ApiError::internal(&e.to_string()))?;
    }

    let before = verify_ledger(&state.broker.ledger_path(), key.as_ref());
    let after = verify_ledger(&copy, key.as_ref());
    // A broken chain is already caught; the anchor only matters when the chain still verifies.
    let anchor = after
        .is_valid()
        .then(|| check_anchor(&copy, key.as_ref(), kept_length, &kept_head));
    let _ = std::fs::remove_file(&copy);
    let detected = !after.is_valid() || anchor.as_ref().is_some_and(Result::is_err);
    Ok(Json(json!({
        "mode": format!("{:?}", input.mode).to_lowercase(),
        "explanation": explanation,
        "keyed": key.is_some(),
        "original": {"entries": before.entries, "valid": before.is_valid()},
        "forged": {"entries": after.entries, "valid": after.is_valid(), "broken": after.broken},
        "anchor": {"kept": {"length": kept_length, "head": kept_head}, "check": anchor_name(anchor.as_ref())},
        "detected": detected,
        "detected_by": if !after.is_valid() { "the chain" } else if anchor.as_ref().is_some_and(Result::is_err) { "the anchor" } else { "nothing" },
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpostorIn {
    pub kind: String,
}

/// Try to take control of a fresh agent with a credential that must not work.
pub async fn impostor(
    State(state): State<Shared>,
    ApiJson(input): ApiJson<ImpostorIn>,
) -> Result<Json<Value>, ApiError> {
    require_lab(&state)?;
    state
        .bridge
        .impostor(&input.kind)
        .await
        .map(Json)
        .ok_or_else(|| {
            ApiError::unprocessable(
                "unknown_kind",
                "kind must be wrong_role, foreign_ca, expired, no_certificate or not_tls",
            )
        })
}
