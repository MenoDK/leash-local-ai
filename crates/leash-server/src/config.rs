//! Configuration from the environment, validated once at start-up.

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{name}: {reason}")]
    Invalid { name: &'static str, reason: String },
    #[error(
        "LEASH_LAB=1 is refused when LEASH_ENV=production: the lab has no authentication on purpose"
    )]
    LabInProduction,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub addr: SocketAddr,
    pub production: bool,
    /// Lab mode: the unauthenticated endpoints that let a visitor break things on purpose.
    pub lab: bool,
    pub data_dir: PathBuf,
    pub corpus_dir: PathBuf,
    pub site_dir: PathBuf,
    /// Hex key for the keyed ledger. Absent: an unkeyed chain, with a warning.
    pub ledger_key: Option<Vec<u8>>,
    /// An Ollama server on this machine, optional. Without it the stand-in models answer.
    pub ollama_url: Option<String>,
    pub ollama_model: String,
    /// Host names the server answers to (any port). Anything else is refused, which defeats DNS rebinding: a page on
    /// another domain that resolves to 127.0.0.1 still sends its own name in `Host`.
    pub allowed_hosts: Vec<String>,
}

impl Config {
    /// # Errors
    /// A variable has an unusable value, or lab mode is requested in production.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// The same, reading variables through `lookup` (tests pass a map; the process environment is global state).
    ///
    /// # Errors
    /// See [`Config::from_env`].
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let var = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());
        let addr = var("LEASH_ADDR").unwrap_or_else(|| "127.0.0.1:8080".to_owned());
        let addr: SocketAddr = addr.parse().map_err(|e| ConfigError::Invalid {
            name: "LEASH_ADDR",
            reason: format!("{e}"),
        })?;
        let production = match var("LEASH_ENV").as_deref() {
            None | Some("development") => false,
            Some("production") => true,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    name: "LEASH_ENV",
                    reason: format!("expected development or production, got {other:?}"),
                });
            }
        };
        let lab = matches!(var("LEASH_LAB").as_deref(), Some("1" | "true"));
        if lab && production {
            return Err(ConfigError::LabInProduction);
        }
        let ledger_key = match var("LEASH_LEDGER_KEY") {
            None => None,
            Some(hex_key) => {
                let bytes = hex::decode(hex_key.trim()).map_err(|e| ConfigError::Invalid {
                    name: "LEASH_LEDGER_KEY",
                    reason: format!("not hex: {e}"),
                })?;
                if bytes.len() < 16 {
                    return Err(ConfigError::Invalid {
                        name: "LEASH_LEDGER_KEY",
                        reason: "needs at least 16 bytes (32 hex characters)".to_owned(),
                    });
                }
                Some(bytes)
            }
        };
        Ok(Self {
            addr,
            production,
            lab,
            data_dir: var("LEASH_DATA_DIR").map_or_else(|| PathBuf::from("data"), PathBuf::from),
            corpus_dir: var("LEASH_CORPUS_DIR")
                .map_or_else(|| PathBuf::from("corpus"), PathBuf::from),
            site_dir: var("LEASH_SITE_DIR").map_or_else(|| PathBuf::from("site"), PathBuf::from),
            ledger_key,
            ollama_url: var("LEASH_OLLAMA_URL"),
            ollama_model: var("LEASH_OLLAMA_MODEL").unwrap_or_else(|| "llama3.2:1b".to_owned()),
            allowed_hosts: allowed_hosts(var("LEASH_ALLOWED_HOSTS").as_deref()),
        })
    }
}

/// The loopback names always, plus any the operator lists (comma-separated host names, no ports).
fn allowed_hosts(extra: Option<&str>) -> Vec<String> {
    let mut hosts: Vec<String> = ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .map(|h| (*h).to_owned())
        .collect();
    for host in extra.unwrap_or_default().split(',') {
        let host = host.trim().to_ascii_lowercase();
        if !host.is_empty() && !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|name| map.get(name).cloned())
    }

    #[test]
    fn defaults_are_local_and_not_production() {
        let c = config(&[]).expect("defaults");
        assert_eq!(c.addr.to_string(), "127.0.0.1:8080");
        assert!(!c.production && !c.lab && c.ledger_key.is_none() && c.ollama_url.is_none());
    }

    #[test]
    fn the_lab_is_refused_in_production() {
        assert!(matches!(
            config(&[("LEASH_ENV", "production"), ("LEASH_LAB", "1")]),
            Err(ConfigError::LabInProduction)
        ));
        assert!(config(&[("LEASH_ENV", "production")]).is_ok());
        assert!(config(&[("LEASH_ENV", "development"), ("LEASH_LAB", "1")]).is_ok());
    }

    #[test]
    fn a_misspelt_environment_is_an_error_not_a_silent_default() {
        assert!(matches!(
            config(&[("LEASH_ENV", "prod")]),
            Err(ConfigError::Invalid {
                name: "LEASH_ENV",
                ..
            })
        ));
    }

    #[test]
    fn the_ledger_key_must_be_hex_and_long_enough() {
        assert!(config(&[("LEASH_LEDGER_KEY", "zz")]).is_err());
        assert!(config(&[("LEASH_LEDGER_KEY", "abcd")]).is_err());
        let key = "ab".repeat(16);
        assert_eq!(
            config(&[("LEASH_LEDGER_KEY", &key)])
                .expect("key")
                .ledger_key
                .expect("some")
                .len(),
            16
        );
    }

    #[test]
    fn an_unparseable_address_is_an_error() {
        assert!(config(&[("LEASH_ADDR", "not an address")]).is_err());
    }

    #[test]
    fn loopback_hosts_are_always_allowed_and_extras_are_normalised() {
        let c = config(&[("LEASH_ALLOWED_HOSTS", " Demo.Example , localhost,,")]).expect("hosts");
        assert_eq!(
            c.allowed_hosts,
            ["localhost", "127.0.0.1", "[::1]", "demo.example"]
        );
        assert_eq!(config(&[]).expect("defaults").allowed_hosts.len(), 3);
    }

    #[test]
    fn blank_values_count_as_unset() {
        let c = config(&[("LEASH_LAB", "  "), ("LEASH_ADDR", "")]).expect("blank");
        assert!(!c.lab && c.addr.port() == 8080);
    }
}
