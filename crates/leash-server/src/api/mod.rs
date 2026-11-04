//! The JSON API. Every endpoint is a thin layer over one library; the logic lives in the libraries.

mod bridge;
mod broker;
mod censor;
mod gateway;
mod lab;
mod memory;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::state::Shared;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/overview", get(overview))
        .route("/api/censor/seal", post(censor::seal_text))
        .route("/api/gateway/status", get(gateway::status))
        .route("/api/gateway/complete", post(gateway::complete))
        .route("/api/memory/index", post(memory::index))
        .route("/api/memory/ask", post(memory::ask))
        .route("/api/memory/read", post(memory::read))
        .route("/api/broker/commands", get(broker::commands))
        .route("/api/broker/propose", post(broker::propose))
        .route("/api/broker/proposals", get(broker::proposals))
        .route("/api/broker/proposals/{id}/approve", post(broker::approve))
        .route("/api/broker/proposals/{id}/deny", post(broker::deny))
        .route("/api/broker/ledger", get(broker::ledger))
        .route("/api/broker/ledger/verify", get(broker::verify))
        .route("/api/bridge/status", get(bridge::status))
        .route("/api/bridge/activate", post(bridge::activate))
        .route("/api/bridge/heartbeat", post(bridge::heartbeat))
        .route("/api/bridge/deactivate", post(bridge::deactivate))
        .route("/api/bridge/run", post(bridge::run))
        .route("/api/bridge/reconnect", post(bridge::reconnect))
        .route("/api/lab/gateway/provider/{id}", post(lab::provider))
        .route("/api/lab/broker/tamper-binary", post(lab::tamper_binary))
        .route("/api/lab/ledger/forge", post(lab::forge_ledger))
        .route("/api/lab/bridge/impostor", post(lab::impostor))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "service": "leash", "version": env!("CARGO_PKG_VERSION")}))
}

/// The lab's endpoints answer 404 unless the server was started with `LEASH_LAB=1`.
pub(crate) fn require_lab(state: &Shared) -> Result<(), ApiError> {
    if state.config.lab {
        Ok(())
    } else {
        Err(ApiError::not_found(
            "lab_disabled",
            "the lab is off; start the server with LEASH_LAB=1 to use it",
        ))
    }
}

async fn overview(State(state): State<Shared>) -> Json<Value> {
    let (ledger_len, ledger_head) = state.broker.ledger_anchor();
    let memory_chunks = state.memory.read().await.chunks();
    Json(json!({
        "service": "leash",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_s": state.started.elapsed().as_secs(),
        "environment": if state.config.production { "production" } else { "development" },
        "lab": state.config.lab,
        "ollama": state.config.ollama_url.is_some(),
        "gateway": state.gateway.status(),
        "memory": {"chunks": memory_chunks, "floor": state.memory_config.floor, "top_k": state.memory_config.top_k},
        "broker": {"commands": state.broker.commands().len(), "ledger_entries": ledger_len, "ledger_head": ledger_head, "keyed": state.ledger_key.is_some()},
        "bridge": {"heartbeat_timeout_ms": state.bridge.heartbeat_timeout_ms()},
    }))
}

/// A routing outcome as the API reports it: a refusal is an answer, not a server error.
pub(crate) fn routed_json(routed: leash_gateway::Routed) -> Value {
    match routed.answer {
        Ok(completion) => {
            json!({"outcome": "answered", "model": completion.model, "text": completion.text, "trace": routed.trace})
        }
        Err(error) => {
            json!({"outcome": "refused", "reason": error, "message": error.to_string(), "trace": routed.trace})
        }
    }
}
