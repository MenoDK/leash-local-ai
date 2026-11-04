//! Routing: which providers may see this text, in what order, and what happens when none can answer.

use std::sync::Arc;

use leash_censor::{Residency, SafeEnvelope};
use serde::Serialize;

use crate::breaker::{Breaker, State};
use crate::provider::{Completion, Locality, Options, Provider, ProviderError, SystemPrompt};

/// What happened to one provider during one call. The whole list is the *trace*: a reader can see why each provider
/// was or was not used, which is also what the lab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Step {
    /// Never asked: the text must stay on this machine and this provider is remote.
    SkippedRemote,
    /// Never asked: its circuit is open.
    SkippedCircuitOpen,
    /// Asked and failed.
    Failed { error: String },
    /// Asked and answered.
    Answered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attempt {
    pub provider: String,
    pub locality: Locality,
    #[serde(flatten)]
    pub step: Step,
}

/// Why a call produced no answer. Every variant means "nothing left the machine that was not allowed to".
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GatewayError {
    #[error("the text is confidential and no local provider is registered")]
    NoLocalProvider,
    #[error("no provider is registered")]
    NoProvider,
    #[error("every eligible provider failed or is cooling down")]
    Unavailable,
}

/// The result of one call: the answer or the reason there is none, plus how the gateway got there.
#[derive(Debug, Clone, Serialize)]
pub struct Routed {
    pub answer: Result<Completion, GatewayError>,
    pub trace: Vec<Attempt>,
}

struct Slot {
    provider: Arc<dyn Provider>,
    breaker: Arc<Breaker>,
}

impl std::fmt::Debug for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slot")
            .field("provider", &self.provider.id())
            .field("breaker", &self.breaker.state())
            .finish()
    }
}

/// Providers in priority order, each behind its own circuit breaker.
#[derive(Debug)]
pub struct Gateway {
    slots: Vec<Slot>,
    threshold: u32,
    cooldown: std::time::Duration,
}

/// Snapshot of one provider, for status pages.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub id: String,
    pub locality: Locality,
    pub circuit: State,
}

impl Gateway {
    /// `threshold` consecutive failures open a provider's circuit; after `cooldown` one probe is allowed.
    #[must_use]
    pub fn new(threshold: u32, cooldown: std::time::Duration) -> Self {
        Self {
            slots: Vec::new(),
            threshold,
            cooldown,
        }
    }

    /// Register a provider. Order is priority: earlier is tried first.
    pub fn register(&mut self, provider: Arc<dyn Provider>) -> &mut Self {
        self.slots.push(Slot {
            provider,
            breaker: Breaker::new(self.threshold, self.cooldown),
        });
        self
    }

    #[must_use]
    pub fn status(&self) -> Vec<ProviderStatus> {
        self.slots
            .iter()
            .map(|s| ProviderStatus {
                id: s.provider.id().to_owned(),
                locality: s.provider.locality(),
                circuit: s.breaker.state(),
            })
            .collect()
    }

    /// Route `input` to the first eligible provider that answers.
    ///
    /// Eligible means: the provider's locality satisfies the text's residency. A provider that is not eligible is never
    /// called, whatever the state of the others. When nobody eligible answers, the call fails; it does **not** fall back
    /// to a provider that is not allowed to see the text.
    pub async fn complete(
        &self,
        system: &SystemPrompt,
        input: &SafeEnvelope,
        options: &Options,
    ) -> Routed {
        let local_only = input.residency() == Residency::LocalOnly;
        let mut trace = Vec::with_capacity(self.slots.len());

        if self.slots.is_empty() {
            return Routed {
                answer: Err(GatewayError::NoProvider),
                trace,
            };
        }
        if local_only
            && !self
                .slots
                .iter()
                .any(|s| s.provider.locality() == Locality::Local)
        {
            trace.extend(self.slots.iter().map(|s| Attempt {
                provider: s.provider.id().to_owned(),
                locality: s.provider.locality(),
                step: Step::SkippedRemote,
            }));
            return Routed {
                answer: Err(GatewayError::NoLocalProvider),
                trace,
            };
        }

        for slot in &self.slots {
            let id = slot.provider.id().to_owned();
            let locality = slot.provider.locality();
            if local_only && locality == Locality::Remote {
                trace.push(Attempt {
                    provider: id,
                    locality,
                    step: Step::SkippedRemote,
                });
                continue;
            }
            let Some(permit) = slot.breaker.try_acquire() else {
                trace.push(Attempt {
                    provider: id,
                    locality,
                    step: Step::SkippedCircuitOpen,
                });
                continue;
            };

            let call = slot.provider.complete(system, input, options);
            let outcome = match tokio::time::timeout(options.timeout, call).await {
                Ok(result) => result,
                Err(_) => Err(ProviderError::TimedOut(options.timeout)),
            };
            match outcome {
                Ok(completion) => {
                    permit.success();
                    trace.push(Attempt {
                        provider: id,
                        locality,
                        step: Step::Answered,
                    });
                    return Routed {
                        answer: Ok(completion),
                        trace,
                    };
                }
                Err(error) => {
                    permit.failure();
                    trace.push(Attempt {
                        provider: id,
                        locality,
                        step: Step::Failed {
                            error: error.to_string(),
                        },
                    });
                }
            }
        }
        Routed {
            answer: Err(GatewayError::Unavailable),
            trace,
        }
    }
}
