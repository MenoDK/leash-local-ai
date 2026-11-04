//! A deterministic stand-in for a model.
//!
//! It exists so that the routing, the circuit breaker and the lab can be exercised without a model server. It does
//! **not** generate language: it answers with a fixed sentence that states what it received. It is labelled as a
//! stand-in everywhere it appears.

use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use leash_censor::SafeEnvelope;

use crate::provider::{Completion, Locality, Options, Provider, ProviderError, SystemPrompt};

/// What a [`StandIn`] does when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Behaviour {
    Healthy,
    /// Fails every call with a transport error.
    Failing,
    /// Answers after this long.
    Slow(Duration),
}

#[derive(Debug)]
pub struct StandIn {
    id: String,
    locality: Locality,
    behaviour: Mutex<Behaviour>,
    calls: std::sync::atomic::AtomicU64,
}

impl StandIn {
    #[must_use]
    pub fn new(id: &str, locality: Locality) -> Self {
        Self {
            id: id.to_owned(),
            locality,
            behaviour: Mutex::new(Behaviour::Healthy),
            calls: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn set_behaviour(&self, behaviour: Behaviour) {
        *self
            .behaviour
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = behaviour;
    }

    #[must_use]
    pub fn behaviour(&self) -> Behaviour {
        *self
            .behaviour
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// How many times the provider was actually asked. The routing tests assert on this: "never called" is the point.
    #[must_use]
    pub fn calls(&self) -> u64 {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl Provider for StandIn {
    fn id(&self) -> &str {
        &self.id
    }

    fn locality(&self) -> Locality {
        self.locality
    }

    async fn complete(
        &self,
        _system: &SystemPrompt,
        input: &SafeEnvelope,
        _options: &Options,
    ) -> Result<Completion, ProviderError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match self.behaviour() {
            Behaviour::Failing => Err(ProviderError::Transport(
                "stand-in is set to fail".to_owned(),
            )),
            Behaviour::Slow(delay) => {
                tokio::time::sleep(delay).await;
                Ok(self.answer(input))
            }
            Behaviour::Healthy => Ok(self.answer(input)),
        }
    }
}

impl StandIn {
    fn answer(&self, input: &SafeEnvelope) -> Completion {
        Completion {
            model: format!("stand-in:{}", self.id),
            text: format!(
                "[stand-in model, no language generation] received {} characters of {:?} data.",
                input.text().chars().count(),
                input.provenance()
            ),
        }
    }
}
