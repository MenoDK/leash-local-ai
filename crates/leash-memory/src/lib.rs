//! # leash-memory
//!
//! Search over a folder of documents that cannot be turned against the assistant.
//!
//! Indexing is a pipeline with one rule per stage, and a stage that refuses a file or a passage *says so*:
//!
//! 1. **Guard.** Only documents under `docs/`, by extension, size and name; never through a symlink or from a hidden or
//!    build directory ([`PathGuard`]).
//! 2. **Quarantine.** A passage that addresses the model ("ignore your rules…") is left out whole and reported
//!    ([`leash_censor::scan_for_instructions`]).
//! 3. **Redact.** Secrets are replaced before anything is embedded, so one pasted into a document cannot be retrieved
//!    ([`leash_censor::seal`]).
//! 4. **Embed and store.** Locally.
//!
//! Retrieval has a relevance floor. Below it the answer is "I do not know", with the nearest score, instead of the least
//! bad passage.

mod chunk;
mod embed;
mod guard;
mod memory;
mod store;

pub use chunk::{Chunk, chunk_markdown};
pub use embed::{EmbedError, Embedder, HashEmbedder, OllamaEmbedder};
pub use guard::{EXCLUDED_DIRS, GuardConfig, GuardError, PathGuard, Skip, Skipped};
pub use memory::{
    IndexReport, Memory, MemoryConfig, MemoryError, Quarantined, Retrieval, context_for_model,
};
pub use store::{Hit, StoreError, VectorStore};
