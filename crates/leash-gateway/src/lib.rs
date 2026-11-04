//! # leash-gateway
//!
//! One contract for every model backend, and the rules for choosing among them:
//!
//! - **Residency.** Text sealed as [`leash_censor::Provenance::Confidential`] is only ever sent to a provider whose
//!   locality is [`Locality::Local`]. If there is none, the call fails; it never falls back to a remote one.
//! - **Circuit breaker.** A provider that keeps failing is skipped for a cooldown, then probed by a single call.
//! - **Typed errors and a trace.** A caller learns *why* there is no answer, and the trace lists what happened to each
//!   provider, including those that were never asked.
//!
//! The input type is [`leash_censor::SafeEnvelope`]. There is no method that takes a `&str`.

mod breaker;
mod gateway;
mod ollama;
mod provider;
mod stand_in;

pub use breaker::{Breaker, Permit, State as CircuitState};
pub use gateway::{Attempt, Gateway, GatewayError, ProviderStatus, Routed, Step};
pub use ollama::{OllamaConfigError, OllamaProvider};
pub use provider::{Completion, Locality, Options, Provider, ProviderError, SystemPrompt};
pub use stand_in::{Behaviour, StandIn};
