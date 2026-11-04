//! The pipeline: walk → quarantine → redact → seal → embed → store, and retrieval that is willing to say "I do not know".

use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use leash_censor::{Finding, Limits, Provenance, SafeEnvelope, scan_for_instructions, seal};
use serde::Serialize;

use crate::chunk::chunk_markdown;
use crate::embed::{EmbedError, Embedder};
use crate::guard::{GuardConfig, PathGuard, Skip, Skipped};
use crate::store::{Hit, StoreError, VectorStore};

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("{0}")]
    Embed(#[from] EmbedError),
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("cannot read the corpus root: {0}")]
    Root(String),
}

/// A passage that was left out because it talks to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Quarantined {
    pub doc: String,
    pub chunk: usize,
    pub rule: &'static str,
}

/// What an indexing run did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IndexReport {
    pub files_indexed: usize,
    pub chunks_indexed: usize,
    pub secrets_redacted: usize,
    pub quarantined: Vec<Quarantined>,
    pub skipped: Vec<Skipped>,
}

/// The outcome of a question.
#[derive(Debug, Serialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Retrieval {
    /// Passages above the relevance floor, best first.
    Found { hits: Vec<Hit> },
    /// Nothing is relevant enough. `nearest` is the best score seen, so the refusal can be explained.
    DontKnow { nearest: Option<f32> },
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryConfig {
    pub max_chunk_chars: usize,
    pub top_k: usize,
    /// Cosine similarity below which a passage is not an answer.
    pub floor: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_chunk_chars: 900,
            top_k: 3,
            floor: 0.30,
        }
    }
}

pub struct Memory {
    embedder: Arc<dyn Embedder>,
    store: VectorStore,
    config: MemoryConfig,
    guard_config: GuardConfig,
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory")
            .field("embedder", &self.embedder.id())
            .field("chunks", &self.store.len())
            .finish()
    }
}

/// Read at most `limit + 1` bytes, so a file that grew after the size check is noticed instead of loaded whole.
fn read_bounded(path: &std::path::Path, limit: u64) -> Result<String, Skip> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| Skip::Unreadable)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Skip::Unreadable)?;
    if bytes.len() as u64 > limit {
        return Err(Skip::TooLarge);
    }
    String::from_utf8(bytes).map_err(|_| Skip::NotUtf8)
}

impl Memory {
    #[must_use]
    pub fn new(
        embedder: Arc<dyn Embedder>,
        config: MemoryConfig,
        guard_config: GuardConfig,
    ) -> Self {
        Self {
            embedder,
            store: VectorStore::new(),
            config,
            guard_config,
        }
    }

    #[must_use]
    pub fn chunks(&self) -> usize {
        self.store.len()
    }

    /// Replace the index with the documents under `root`.
    ///
    /// # Errors
    /// The root cannot be read, or the embedder fails. A document that is merely unreadable or refused is *reported*,
    /// not an error.
    pub async fn index(&mut self, root: &std::path::Path) -> Result<IndexReport, MemoryError> {
        let guard = PathGuard::new(root, self.guard_config.clone())
            .map_err(|e| MemoryError::Root(e.to_string()))?;
        let (files, skipped) = guard.walk();
        let mut report = IndexReport {
            skipped,
            ..IndexReport::default()
        };
        let mut fresh = VectorStore::new();

        for path in files {
            let doc = path
                .strip_prefix(guard.root())
                .unwrap_or(&path)
                .display()
                .to_string();
            let text = match read_bounded(&path, self.guard_config.max_file_bytes) {
                Ok(text) => text,
                Err(reason) => {
                    report.skipped.push(Skipped { path: doc, reason });
                    continue;
                }
            };
            report.files_indexed += 1;
            for (index, chunk) in chunk_markdown(&text, self.config.max_chunk_chars)
                .into_iter()
                .enumerate()
            {
                // A passage that addresses the model is left out whole; the heading is part of the passage.
                let finding: Option<Finding> = scan_for_instructions(&chunk.text)
                    .or_else(|| scan_for_instructions(&chunk.heading_path));
                if let Some(Finding { rule }) = finding {
                    report.quarantined.push(Quarantined {
                        doc: doc.clone(),
                        chunk: index,
                        rule,
                    });
                    continue;
                }
                let limits = Limits {
                    max_chars: self.config.max_chunk_chars,
                };
                let body = seal(&chunk.text, Provenance::Document, limits);
                let heading = seal(
                    &chunk.heading_path,
                    Provenance::Document,
                    Limits { max_chars: 200 },
                );
                report.secrets_redacted +=
                    body.report().secrets_redacted + heading.report().secrets_redacted;

                let vector = self
                    .embedder
                    .embed(&format!("{}\n{}", heading.text(), body.text()))
                    .await?;
                fresh.add(&doc, heading.text(), body.text(), vector)?;
                report.chunks_indexed += 1;
            }
        }
        self.store = fresh;
        Ok(report)
    }

    /// Find passages for `question`, or say there is nothing relevant.
    ///
    /// # Errors
    /// The embedder fails.
    pub async fn retrieve(&self, question: &str) -> Result<Retrieval, MemoryError> {
        let query = self.embedder.embed(question).await?;
        if query.iter().all(|x| *x == 0.0) {
            return Ok(Retrieval::DontKnow { nearest: None });
        }
        let hits = self
            .store
            .search(&query, self.config.top_k, self.config.floor)?;
        if hits.is_empty() {
            return Ok(Retrieval::DontKnow {
                nearest: self.store.best_score(&query),
            });
        }
        Ok(Retrieval::Found { hits })
    }
}

/// The passages as the model may see them: one sealed block, each passage headed by where it came from.
#[must_use]
pub fn context_for_model(question: &str, hits: &[Hit]) -> SafeEnvelope {
    let mut text = format!("Question: {question}\n\nSources:\n");
    for (n, hit) in hits.iter().enumerate() {
        text.push_str(&format!(
            "\n[{}] {} › {}\n{}\n",
            n + 1,
            hit.doc,
            hit.heading,
            hit.text
        ));
    }
    seal(&text, Provenance::Document, Limits::default())
}
