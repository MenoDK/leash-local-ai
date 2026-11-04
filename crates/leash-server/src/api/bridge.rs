use axum::Json;
use axum::extract::State;
use leash_bridge::Op;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{ApiError, ApiJson};
use crate::state::Shared;

pub async fn status(State(state): State<Shared>) -> Json<Value> {
    let mut out = state.bridge.status().await;
    out["heartbeat_timeout_ms"] = json!(state.bridge.heartbeat_timeout_ms());
    Json(out)
}

pub async fn activate(State(state): State<Shared>) -> Json<Value> {
    Json(state.bridge.activate().await)
}

pub async fn heartbeat(State(state): State<Shared>) -> Json<Value> {
    Json(state.bridge.heartbeat().await)
}

pub async fn deactivate(State(state): State<Shared>) -> Json<Value> {
    Json(state.bridge.deactivate().await)
}

pub async fn reconnect(State(state): State<Shared>) -> Json<Value> {
    Json(state.bridge.reset().await)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunIn {
    pub op: Op,
}

/// One typed operation. Nothing here takes a command line.
pub async fn run(
    State(state): State<Shared>,
    ApiJson(input): ApiJson<RunIn>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bridge.run(input.op).await))
}
