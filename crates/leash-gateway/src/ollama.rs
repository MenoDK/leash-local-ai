//! A client for an Ollama server (`/api/chat`, non-streaming).
//!
//! Whether it counts as [`Locality::Local`] is computed from the address, not declared: only a loopback host is local.
//! Plain HTTP only: a loopback server needs no TLS, and a remote one should not be reached by this client.

use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use leash_censor::SafeEnvelope;
use serde_json::json;
use url::{Host, Url};

use crate::provider::{Completion, Locality, Options, Provider, ProviderError, SystemPrompt};

/// A model reply larger than this is refused rather than buffered.
const MAX_REPLY_BYTES: usize = 256 * 1024;

#[derive(Debug)]
pub struct OllamaProvider {
    id: String,
    model: String,
    chat_url: Url,
    locality: Locality,
    client: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum OllamaConfigError {
    #[error("not a valid URL: {0}")]
    Url(String),
    #[error("only http:// is supported, got {0}://")]
    Scheme(String),
    #[error("the URL has no host")]
    NoHost,
}

fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(ip) => IpAddr::V4(*ip).is_loopback(),
        Host::Ipv6(ip) => IpAddr::V6(*ip).is_loopback(),
    }
}

impl OllamaProvider {
    /// # Errors
    /// The URL is not a usable `http://` address.
    pub fn new(id: &str, base_url: &str, model: &str) -> Result<Self, OllamaConfigError> {
        let base = Url::parse(base_url).map_err(|e| OllamaConfigError::Url(e.to_string()))?;
        if base.scheme() != "http" {
            return Err(OllamaConfigError::Scheme(base.scheme().to_owned()));
        }
        let host = base.host().ok_or(OllamaConfigError::NoHost)?;
        let locality = if is_loopback(&host) {
            Locality::Local
        } else {
            Locality::Remote
        };
        let chat_url = base
            .join("api/chat")
            .map_err(|e| OllamaConfigError::Url(e.to_string()))?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| OllamaConfigError::Url(e.to_string()))?;
        Ok(Self {
            id: id.to_owned(),
            model: model.to_owned(),
            chat_url,
            locality,
            client,
        })
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn locality(&self) -> Locality {
        self.locality
    }

    async fn complete(
        &self,
        system: &SystemPrompt,
        input: &SafeEnvelope,
        options: &Options,
    ) -> Result<Completion, ProviderError> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "messages": [
                {"role": "system", "content": system.as_str()},
                {"role": "user", "content": input.render_block()},
            ],
            "options": {"temperature": options.temperature, "num_predict": options.max_tokens},
        });
        let mut response = self
            .client
            .post(self.chat_url.clone())
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(ProviderError::Status {
                status: response.status().as_u16(),
            });
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| ProviderError::Transport(e.without_url().to_string()))?
        {
            if bytes.len() + chunk.len() > MAX_REPLY_BYTES {
                return Err(ProviderError::BadResponse(format!(
                    "reply larger than {MAX_REPLY_BYTES} bytes"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| ProviderError::BadResponse(e.to_string()))?;
        let text = value
            .pointer("/message/content")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                ProviderError::BadResponse("no message.content in the reply".to_owned())
            })?;
        Ok(Completion {
            model: self.model.clone(),
            text: text.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locality_of(url: &str) -> Locality {
        OllamaProvider::new("o", url, "m")
            .expect("valid url")
            .locality()
    }

    #[test]
    fn only_loopback_hosts_are_local() {
        assert_eq!(locality_of("http://127.0.0.1:11434"), Locality::Local);
        assert_eq!(locality_of("http://127.1.2.3:11434"), Locality::Local);
        assert_eq!(locality_of("http://localhost:11434"), Locality::Local);
        assert_eq!(locality_of("http://[::1]:11434"), Locality::Local);
        assert_eq!(locality_of("http://LOCALHOST:11434"), Locality::Local);
    }

    #[test]
    fn lookalikes_and_lan_hosts_are_remote() {
        for url in [
            "http://localhost.evil.test:11434",
            "http://127.0.0.1.evil.test:11434",
            "http://10.0.0.5:11434",
            "http://192.168.1.10:11434",
            "http://model-server.internal:11434",
            "http://0.0.0.0:11434",
            "http://user@10.0.0.5:11434", // userinfo does not change the host
        ] {
            assert_eq!(locality_of(url), Locality::Remote, "{url}");
        }
    }

    #[test]
    fn a_userinfo_trick_does_not_make_a_remote_host_local() {
        // `http://localhost@evil.test/` has the host `evil.test`.
        assert_eq!(
            locality_of("http://localhost@evil.test:11434"),
            Locality::Remote
        );
    }

    #[test]
    fn https_and_junk_are_refused() {
        assert!(matches!(
            OllamaProvider::new("o", "https://localhost", "m"),
            Err(OllamaConfigError::Scheme(_))
        ));
        assert!(matches!(
            OllamaProvider::new("o", "not a url", "m"),
            Err(OllamaConfigError::Url(_))
        ));
        assert!(OllamaProvider::new("o", "file:///etc/passwd", "m").is_err());
    }
}
