use axum::Json;
use axum::extract::State;
use leash_censor::{Limits, Provenance, seal};
use leash_gateway::{Options, SystemPrompt};
use leash_memory::{GuardError, PathGuard, Retrieval, Skip, context_for_model};
use serde::Deserialize;
use serde_json::{Value, json};

use super::gateway::CALL_TIMEOUT;
use super::routed_json;
use crate::error::{ApiError, ApiJson};
use crate::state::Shared;

/// Rebuild the index from the corpus (the runtime copy).
pub async fn index(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    let mut memory = state.memory.write().await;
    let report = memory
        .index(&state.corpus)
        .await
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    Ok(Json(json!({"report": report, "chunks": memory.chunks()})))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskIn {
    pub question: String,
}

const MAX_QUESTION_CHARS: usize = 500;

/// Find passages; if there are any, hand them to the gateway as one sealed block. If not, say so and call nobody.
pub async fn ask(
    State(state): State<Shared>,
    ApiJson(input): ApiJson<AskIn>,
) -> Result<Json<Value>, ApiError> {
    let question = input.question.trim();
    if question.is_empty() || question.chars().count() > MAX_QUESTION_CHARS {
        return Err(ApiError::unprocessable(
            "bad_question",
            format!("the question must have 1 to {MAX_QUESTION_CHARS} characters"),
        ));
    }
    let retrieval = state
        .memory
        .read()
        .await
        .retrieve(question)
        .await
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    match retrieval {
        Retrieval::DontKnow { nearest } => Ok(Json(json!({
            "answer": "dont_know",
            "nearest": nearest,
            "floor": state.memory_config.floor,
            "note": "nothing in the documents is relevant enough, so no model was asked",
        }))),
        Retrieval::Found { hits } => {
            let envelope = context_for_model(question, &hits);
            let system = SystemPrompt::new(
                "Answer the question using only the sources, and name the source you used.",
            );
            let options = Options {
                max_tokens: 256,
                temperature: 0.0,
                timeout: CALL_TIMEOUT,
            };
            let routed = state.gateway.complete(&system, &envelope, &options).await;
            Ok(Json(json!({
                "answer": "found",
                "hits": hits,
                "block": envelope.render_block(),
                "completion": routed_json(routed),
            })))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadIn {
    pub path: String,
}

fn skip_name(skip: Skip) -> String {
    serde_json::to_value(skip)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Read one document by a caller-supplied relative path, through the same guard the indexer uses.
pub async fn read(
    State(state): State<Shared>,
    ApiJson(input): ApiJson<ReadIn>,
) -> Result<Json<Value>, ApiError> {
    let guard = PathGuard::new(&state.corpus, state.guard.clone())
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    let path = guard.resolve(&input.path).map_err(|error| match error {
        GuardError::Malformed => ApiError::bad_request("malformed_path", error.to_string()),
        GuardError::Traversal => ApiError::forbidden("path_traversal", error.to_string()),
        GuardError::Symlink => ApiError::forbidden("symlink", error.to_string()),
        GuardError::NotAllowed(skip) => ApiError::forbidden(
            "not_allowed",
            format!("{error} (reason: {})", skip_name(skip)),
        ),
        GuardError::Missing => ApiError::not_found("missing", error.to_string()),
    })?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    let text = String::from_utf8_lossy(&bytes);
    let sealed = seal(&text, Provenance::Document, Limits { max_chars: 4000 });
    Ok(Json(
        json!({"path": input.path, "text": sealed.text(), "report": sealed.report()}),
    ))
}
