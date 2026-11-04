use std::time::Duration;

use axum::Json;
use axum::extract::State;
use leash_censor::{Limits, Provenance, scan_for_instructions, seal};
use leash_gateway::{Behaviour, Options, SystemPrompt};
use serde::Deserialize;
use serde_json::{Value, json};

use super::censor::{ProvenanceIn, envelope_json};
use super::routed_json;
use crate::error::{ApiError, ApiJson};
use crate::state::Shared;

/// One attempt may take this long; a slower provider counts as failed.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(3);

/// Providers in routing order. The two stand-ins also report how they are currently set to behave.
pub async fn status(State(state): State<Shared>) -> Json<Value> {
    let providers: Vec<Value> = state
        .gateway
        .status()
        .into_iter()
        .map(|provider| {
            let behaviour = match provider.id.as_str() {
                "local-stand-in" => Some(behaviour_name(state.local.behaviour())),
                "remote-stand-in" => Some(behaviour_name(state.remote.behaviour())),
                _ => None,
            };
            let mut value = serde_json::to_value(&provider).unwrap_or(Value::Null);
            value["behaviour"] = json!(behaviour);
            value
        })
        .collect();
    Json(json!({
        "providers": providers,
        "call_timeout_ms": CALL_TIMEOUT.as_millis(),
        "ollama": state.config.ollama_url.is_some(),
    }))
}

pub(crate) fn behaviour_name(behaviour: Behaviour) -> &'static str {
    match behaviour {
        Behaviour::Healthy => "healthy",
        Behaviour::Failing => "failing",
        Behaviour::Slow(_) => "slow",
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteIn {
    pub text: String,
    #[serde(default)]
    pub provenance: ProvenanceIn,
}

/// Seal the text, refuse it if it talks to the model, otherwise route it. The task given to the model is fixed here: it
/// is never taken from the request.
pub async fn complete(
    State(state): State<Shared>,
    ApiJson(input): ApiJson<CompleteIn>,
) -> Result<Json<Value>, ApiError> {
    let provenance = Provenance::from(input.provenance);
    if provenance != Provenance::Confidential
        && let Some(finding) = scan_for_instructions(&input.text)
    {
        return Err(ApiError::unprocessable(
            "held_for_review",
            format!(
                "the text addresses the model (rule {:?}); it was held back and no provider saw it",
                finding.rule
            ),
        ));
    }
    let sealed = seal(&input.text, provenance, Limits::default());
    let system = SystemPrompt::new("Describe the data in one short sentence.");
    let options = Options {
        max_tokens: 128,
        temperature: 0.0,
        timeout: CALL_TIMEOUT,
    };
    let routed = state.gateway.complete(&system, &sealed, &options).await;
    let mut out = routed_json(routed);
    out["envelope"] = envelope_json(&sealed, input.text.chars().count());
    Ok(Json(out))
}
