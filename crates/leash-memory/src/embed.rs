//! Turning text into vectors.
//!
//! Two implementations: [`HashEmbedder`], which needs nothing and is *lexical* (shared words, not shared meaning), and
//! [`OllamaEmbedder`], which asks a model server on this machine. Documents may be private, so the second one refuses
//! any address that is not loopback.

use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use url::{Host, Url};

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("embedding request failed: {0}")]
    Transport(String),
    #[error("the embedding server answered HTTP {0}")]
    Status(u16),
    #[error("unusable embedding response: {0}")]
    BadResponse(String),
    #[error("the embedding server must be on this machine (loopback), got {0}")]
    NotLocal(String),
    #[error("invalid embedding server URL: {0}")]
    BadUrl(String),
}

#[async_trait]
pub trait Embedder: Send + Sync {
    fn id(&self) -> &str;

    /// Embeds `text` into a unit-length vector.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError>;
}

/// Feature hashing of words and word pairs into a fixed number of buckets, with a sign bit so that collisions tend to
/// cancel instead of add up. Deterministic and offline.
#[derive(Debug, Clone)]
pub struct HashEmbedder {
    dimensions: usize,
}

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "in", "on", "and", "or", "is", "are", "was", "it", "this",
    "that", "for", "with", "as", "by", "be", "at", "do", "does", "how", "what", "which", "who",
    "when", "where", "why", "i", "you", "we", "can", "should", "from", "into", "me", "my", "o",
    "os", "as", "um", "uma", "de", "do", "da", "em", "no", "na", "e", "ou", "que", "com", "para",
    "por", "se",
];

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

impl HashEmbedder {
    #[must_use]
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions: dimensions.max(16),
        }
    }

    fn words(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() > 1 && !STOP_WORDS.contains(w))
            .map(str::to_owned)
            .collect()
    }

    /// The vector for `text`. Synchronous because it does no I/O.
    #[must_use]
    pub fn vector(&self, text: &str) -> Vec<f32> {
        let words = Self::words(text);
        let mut v = vec![0.0f32; self.dimensions];
        let mut add = |feature: &str, weight: f32| {
            let h = fnv1a(feature.as_bytes());
            let bucket = usize::try_from(h % self.dimensions as u64).unwrap_or(0);
            let sign = if (h >> 63) & 1 == 0 { 1.0 } else { -1.0 };
            v[bucket] += sign * weight;
        };
        for word in &words {
            add(word, 1.0);
        }
        for pair in words.windows(2) {
            add(&format!("{} {}", pair[0], pair[1]), 0.6);
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            v.iter_mut().for_each(|x| *x /= norm);
        }
        v
    }
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self::new(512)
    }
}

#[async_trait]
impl Embedder for HashEmbedder {
    fn id(&self) -> &str {
        "hash-512 (lexical stand-in)"
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        Ok(self.vector(text))
    }
}

/// `POST /api/embed` on an Ollama server on this machine.
#[derive(Debug)]
pub struct OllamaEmbedder {
    model: String,
    url: Url,
    client: reqwest::Client,
}

impl OllamaEmbedder {
    /// # Errors
    /// The URL is not `http://` on a loopback host.
    pub fn new(base_url: &str, model: &str) -> Result<Self, EmbedError> {
        let base = Url::parse(base_url).map_err(|e| EmbedError::BadUrl(e.to_string()))?;
        let local = base.scheme() == "http"
            && match base.host() {
                Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
                Some(Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
                Some(Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
                None => false,
            };
        if !local {
            return Err(EmbedError::NotLocal(
                base.host_str().unwrap_or("?").to_owned(),
            ));
        }
        let url = base
            .join("api/embed")
            .map_err(|e| EmbedError::BadUrl(e.to_string()))?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| EmbedError::BadUrl(e.to_string()))?;
        Ok(Self {
            model: model.to_owned(),
            url,
            client,
        })
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    fn id(&self) -> &str {
        &self.model
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let response = self
            .client
            .post(self.url.clone())
            .json(&serde_json::json!({"model": self.model, "input": text}))
            .send()
            .await
            .map_err(|e| EmbedError::Transport(e.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(EmbedError::Status(response.status().as_u16()));
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| EmbedError::BadResponse(e.to_string()))?;
        let numbers = value
            .pointer("/embeddings/0")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| EmbedError::BadResponse("no embeddings[0]".to_owned()))?;
        let mut vector: Vec<f32> = numbers
            .iter()
            .filter_map(|n| n.as_f64())
            .map(|f| f as f32)
            .collect();
        if vector.len() != numbers.len()
            || vector.is_empty()
            || vector.iter().any(|x| !x.is_finite())
        {
            return Err(EmbedError::BadResponse(
                "empty or non-numeric vector".to_owned(),
            ));
        }
        let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            vector.iter_mut().for_each(|x| *x /= norm);
        }
        Ok(vector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn vectors_are_deterministic_and_unit_length() {
        let e = HashEmbedder::default();
        let a = e.vector("Rotate the signing key every ninety days");
        assert_eq!(a, e.vector("Rotate the signing key every ninety days"));
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn shared_words_score_higher_than_unrelated_text() {
        let e = HashEmbedder::default();
        let q = e.vector("how often is the signing key rotated");
        let near = e.vector("The signing key is rotated every ninety days.");
        let far = e.vector("Paint the fence on Saturday morning with two coats.");
        assert!(
            cosine(&q, &near) > cosine(&q, &far) + 0.2,
            "{} vs {}",
            cosine(&q, &near),
            cosine(&q, &far)
        );
    }

    #[test]
    fn text_without_content_words_gives_a_zero_vector_not_nan() {
        let v = HashEmbedder::default().vector("the of a ... !!!");
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn the_model_server_must_be_on_this_machine() {
        assert!(OllamaEmbedder::new("http://127.0.0.1:11434", "m").is_ok());
        assert!(OllamaEmbedder::new("http://localhost:11434", "m").is_ok());
        for url in [
            "http://10.0.0.5:11434",
            "http://localhost.evil.test",
            "http://localhost@evil.test",
            "https://localhost",
            "http://0.0.0.0:11434",
        ] {
            assert!(
                matches!(OllamaEmbedder::new(url, "m"), Err(EmbedError::NotLocal(_))),
                "{url}"
            );
        }
    }
}
