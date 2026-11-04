use std::collections::BTreeMap;
use std::path::Path;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use leash_broker::{
    Actor, Approval, Decision, Execution, LedgerKey, Refusal, ResolveError, verify_ledger,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{ApiError, ApiJson};
use crate::state::Shared;

/// The commands that are bound, with the binary shown by name only.
pub async fn commands(State(state): State<Shared>) -> Json<Value> {
    let list: Vec<Value> = state
        .broker
        .commands()
        .into_iter()
        .map(|m| {
            let mut value = serde_json::to_value(m).unwrap_or(Value::Null);
            value["binary"] = json!(
                Path::new(&m.binary)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
            );
            value
        })
        .collect();
    Json(json!({"commands": list}))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeIn {
    pub command: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    #[serde(default)]
    pub flags: Vec<String>,
}

pub(crate) fn refusal_error(refusal: &Refusal) -> ApiError {
    let message = refusal.to_string();
    match refusal {
        Refusal::UnknownCommand(_) => ApiError::unprocessable("unknown_command", message),
        Refusal::Invalid(_) => ApiError::unprocessable("invalid_arguments", message),
        Refusal::BinaryChanged => ApiError::new(StatusCode::CONFLICT, "binary_changed", message),
        Refusal::TooManyPending => {
            ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too_many_pending", message)
        }
        Refusal::LedgerUnavailable => ApiError::unavailable("ledger_unavailable", message),
    }
}

fn resolve_error(error: &ResolveError) -> ApiError {
    let message = error.to_string();
    match error {
        ResolveError::NotFound => ApiError::not_found("no_such_proposal", message),
        ResolveError::AlreadyResolved(_) => ApiError::conflict("already_resolved", message),
        ResolveError::DigestMismatch => ApiError::conflict("digest_mismatch", message),
        ResolveError::HumanRequired => ApiError::forbidden("human_required", message),
        ResolveError::Expired => ApiError::new(StatusCode::GONE, "expired", message),
        ResolveError::LedgerUnavailable => ApiError::unavailable("ledger_unavailable", message),
    }
}

/// What a run produced, with the output sealed as the model would receive it.
pub(crate) fn execution_json(execution: &Execution) -> Value {
    let sealed = execution.sealed_output();
    json!({
        "exit_code": execution.exit_code,
        "signal": execution.signal,
        "timed_out": execution.timed_out,
        "output_limit_hit": execution.output_limit_hit,
        "duration_ms": execution.duration_ms,
        "output": sealed.text(),
        "block": sealed.render_block(),
        "report": sealed.report(),
    })
}

/// Ask to run a command. Read-only commands whose manifest says `approval: never` are approved by policy and run now;
/// every other command waits for a person.
pub async fn propose(
    State(state): State<Shared>,
    ApiJson(input): ApiJson<ProposeIn>,
) -> Result<Json<Value>, ApiError> {
    let proposal = state
        .broker
        .propose(&input.command, &input.args, &input.flags)
        .map_err(|r| refusal_error(&r))?;
    if proposal.approval == Approval::Never {
        let resolution = state
            .broker
            .resolve(
                &proposal.id,
                Decision::Approve {
                    digest: proposal.digest.clone(),
                },
                Actor::Policy,
            )
            .await
            .map_err(|e| resolve_error(&e))?;
        return Ok(Json(json!({
            "proposal": resolution.proposal,
            "ran": resolution.execution.as_ref().map(execution_json),
        })));
    }
    Ok(Json(json!({"proposal": proposal, "ran": null})))
}

pub async fn proposals(State(state): State<Shared>) -> Json<Value> {
    Json(json!({"proposals": state.broker.proposals()}))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproveIn {
    /// The digest the person was shown. Approving a proposal that changed since is refused.
    pub digest: String,
}

pub async fn approve(
    State(state): State<Shared>,
    UrlPath(id): UrlPath<String>,
    ApiJson(input): ApiJson<ApproveIn>,
) -> Result<Json<Value>, ApiError> {
    let resolution = state
        .broker
        .resolve(
            &id,
            Decision::Approve {
                digest: input.digest,
            },
            Actor::Human("browser".to_owned()),
        )
        .await
        .map_err(|e| resolve_error(&e))?;
    Ok(Json(json!({
        "proposal": resolution.proposal,
        "ran": resolution.execution.as_ref().map(execution_json),
    })))
}

pub async fn deny(
    State(state): State<Shared>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    let resolution = state
        .broker
        .resolve(&id, Decision::Deny, Actor::Human("browser".to_owned()))
        .await
        .map_err(|e| resolve_error(&e))?;
    Ok(Json(json!({"proposal": resolution.proposal, "ran": null})))
}

/// The newest entries of the ledger, oldest first, with the anchor to keep somewhere else.
pub async fn ledger(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    let path = state.broker.ledger_path();
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    let all: Vec<Value> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let total = all.len();
    let entries: Vec<Value> = all.into_iter().skip(total.saturating_sub(100)).collect();
    let (length, head) = state.broker.ledger_anchor();
    Ok(Json(
        json!({"total": total, "shown": entries.len(), "entries": entries, "anchor": {"length": length, "head": head}}),
    ))
}

pub(crate) fn ledger_key(state: &Shared) -> Option<LedgerKey> {
    state
        .ledger_key
        .as_deref()
        .and_then(|bytes| LedgerKey::new(bytes).ok())
}

pub async fn verify(State(state): State<Shared>) -> Json<Value> {
    let key = ledger_key(&state);
    let report = verify_ledger(&state.broker.ledger_path(), key.as_ref());
    Json(json!({"valid": report.is_valid(), "keyed": key.is_some(), "report": report}))
}
