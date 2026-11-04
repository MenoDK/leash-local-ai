use axum::Json;
use leash_censor::{Limits, Provenance, Residency, SafeEnvelope, scan_for_instructions, seal};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::ApiJson;

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceIn {
    #[default]
    ToolOutput,
    Document,
    Confidential,
}

impl From<ProvenanceIn> for Provenance {
    fn from(value: ProvenanceIn) -> Self {
        match value {
            ProvenanceIn::ToolOutput => Self::ToolOutput,
            ProvenanceIn::Document => Self::Document,
            ProvenanceIn::Confidential => Self::Confidential,
        }
    }
}

pub fn residency_name(envelope: &SafeEnvelope) -> &'static str {
    match envelope.residency() {
        Residency::Anywhere => "anywhere",
        Residency::LocalOnly => "local_only",
    }
}

pub fn envelope_json(envelope: &SafeEnvelope, input_chars: usize) -> Value {
    json!({
        "provenance": envelope.provenance(),
        "residency": residency_name(envelope),
        "sealed": envelope.text(),
        "block": envelope.render_block(),
        "report": envelope.report(),
        "input_chars": input_chars,
        "output_chars": envelope.text().chars().count(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealIn {
    pub text: String,
    #[serde(default)]
    pub provenance: ProvenanceIn,
}

/// Seal the text and, for untrusted provenance, say whether the instruction screen would hold it back.
pub async fn seal_text(ApiJson(input): ApiJson<SealIn>) -> Json<Value> {
    let provenance = Provenance::from(input.provenance);
    let sealed = seal(&input.text, provenance, Limits::default());
    let mut out = envelope_json(&sealed, input.text.chars().count());
    let finding = if provenance == Provenance::Confidential {
        None
    } else {
        scan_for_instructions(&input.text)
    };
    out["screen"] = json!({"held": finding.is_some(), "rule": finding.map(|f| f.rule)});
    Json(out)
}
