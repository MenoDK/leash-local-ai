//! The demo's bridge: an agent and a controller joined by an in-process pipe, doing the real handshake.
//!
//! Besides the live session there are four "impostor" attempts, each against a *fresh* agent so the live session is
//! not disturbed: a valid certificate in the wrong role, a certificate from another authority, an expired one, and no
//! certificate at all.

use std::sync::Arc;
use std::time::Duration;

use leash_bridge::{
    AGENT_NAME, Agent, AgentState, Authority, BridgeError, CONTROLLER_NAME, Controller, Identity,
    Op, PkiError, Validity,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::Mutex;

pub struct Bridge {
    ca: Authority,
    agent: Arc<Agent>,
    heartbeat: Duration,
    session: Mutex<Option<Controller<DuplexStream>>>,
}

impl std::fmt::Debug for Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bridge(..)")
    }
}

fn describe(error: &BridgeError) -> String {
    error.to_string()
}

fn pki(error: PkiError) -> BridgeError {
    BridgeError::Tls(error.to_string())
}

impl Bridge {
    /// # Errors
    /// Certificate generation or TLS configuration failed.
    pub fn new(heartbeat: Duration) -> Result<Self, BridgeError> {
        let ca = Authority::new("leash demo authority").map_err(pki)?;
        let identity = ca.issue(AGENT_NAME, Validity::Current).map_err(pki)?;
        let agent = Arc::new(Agent::new(&ca.certificate(), identity, heartbeat)?);
        Ok(Self {
            ca,
            agent,
            heartbeat,
            session: Mutex::new(None),
        })
    }

    async fn connect(&self) -> Result<Controller<DuplexStream>, BridgeError> {
        let identity = self
            .ca
            .issue(CONTROLLER_NAME, Validity::Current)
            .map_err(pki)?;
        // The agent serves one session at a time and releases it a moment after the previous one is dropped.
        let mut last = BridgeError::Unexpected;
        for _ in 0..10 {
            let (client, server) = tokio::io::duplex(64 * 1024);
            let agent = Arc::clone(&self.agent);
            tokio::spawn(async move {
                let _ = agent.serve(server).await;
            });
            match Controller::connect(client, &self.ca.certificate(), identity.duplicate()).await {
                Ok(controller) => return Ok(controller),
                Err(error) => last = error,
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        Err(last)
    }

    /// Run `action` on the live session, reconnecting once if the session had ended (idle timeout, agent restart).
    async fn with_session<T>(
        &self,
        action: impl for<'c> Fn(
            &'c mut Controller<DuplexStream>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T, BridgeError>> + Send + 'c>,
        >,
    ) -> Result<T, BridgeError> {
        let mut guard = self.session.lock().await;
        for attempt in 0..2 {
            if guard.is_none() {
                *guard = Some(self.connect().await?);
            }
            let Some(controller) = guard.as_mut() else {
                return Err(BridgeError::Unexpected);
            };
            match action(controller).await {
                Err(BridgeError::Frame(_)) if attempt == 0 => *guard = None,
                other => return other,
            }
        }
        Err(BridgeError::Unexpected)
    }

    fn state_json(result: Result<(AgentState, Option<u64>), BridgeError>) -> Value {
        match result {
            Ok((state, expires_in_ms)) => {
                json!({"ok": true, "state": state, "expires_in_ms": expires_in_ms})
            }
            Err(BridgeError::Agent(code, message)) => {
                json!({"ok": false, "error": {"code": code, "message": message}})
            }
            Err(other) => {
                json!({"ok": false, "error": {"code": "transport", "message": describe(&other)}})
            }
        }
    }

    pub async fn status(&self) -> Value {
        Self::state_json(self.with_session(|c| Box::pin(c.status())).await)
    }

    pub async fn activate(&self) -> Value {
        Self::state_json(self.with_session(|c| Box::pin(c.activate())).await)
    }

    pub async fn heartbeat(&self) -> Value {
        Self::state_json(self.with_session(|c| Box::pin(c.heartbeat())).await)
    }

    pub async fn deactivate(&self) -> Value {
        Self::state_json(self.with_session(|c| Box::pin(c.deactivate())).await)
    }

    pub async fn run(&self, op: Op) -> Value {
        let result = self
            .with_session(move |c| {
                let op = op.clone();
                Box::pin(async move { c.run(op).await })
            })
            .await;
        match result {
            Ok(output) => json!({"ok": true, "output": output}),
            Err(BridgeError::Agent(code, message)) => {
                json!({"ok": false, "error": {"code": code, "message": message}})
            }
            Err(other) => {
                json!({"ok": false, "error": {"code": "transport", "message": describe(&other)}})
            }
        }
    }

    /// Drop the live session so the next call opens a new one (the agent is dormant again).
    pub async fn reset(&self) -> Value {
        *self.session.lock().await = None;
        json!({"ok": true})
    }

    pub fn heartbeat_timeout_ms(&self) -> u64 {
        u64::try_from(self.heartbeat.as_millis()).unwrap_or(u64::MAX)
    }

    /// Try to take control of a fresh agent with a credential that must not work.
    pub async fn impostor(&self, kind: &str) -> Option<Value> {
        let agent_identity = self.ca.issue(AGENT_NAME, Validity::Current).ok()?;
        let fresh =
            Arc::new(Agent::new(&self.ca.certificate(), agent_identity, self.heartbeat).ok()?);
        let (client, server) = tokio::io::duplex(64 * 1024);
        let serving = {
            let fresh = Arc::clone(&fresh);
            tokio::spawn(async move { fresh.serve(server).await })
        };

        let (stage, detail) = match kind {
            "wrong_role" => {
                let identity = self.ca.issue(AGENT_NAME, Validity::Current).ok()?;
                match Controller::connect(client, &self.ca.certificate(), identity).await {
                    Err(error) => ("handshake", describe(&error)),
                    Ok(mut controller) => match controller.activate().await {
                        Err(error) => ("application", describe(&error)),
                        Ok(_) => ("none", "the agent was activated".to_owned()),
                    },
                }
            }
            "foreign_ca" => {
                let other = Authority::new("some other authority").ok()?;
                let identity = other.issue(CONTROLLER_NAME, Validity::Current).ok()?;
                Self::attempt(client, &self.ca.certificate(), identity).await
            }
            "expired" => {
                let identity = self.ca.issue(CONTROLLER_NAME, Validity::Expired).ok()?;
                Self::attempt(client, &self.ca.certificate(), identity).await
            }
            "no_certificate" => Self::without_certificate(client, &self.ca.certificate()).await,
            "not_tls" => {
                let mut client = client;
                let _ = client.write_all(b"{\"type\":\"activate\"}\n").await;
                let mut sink = Vec::new();
                let _ = tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut sink))
                    .await;
                ("handshake", "plain text is not a TLS handshake".to_owned())
            }
            _ => return None,
        };

        let agent_outcome = tokio::time::timeout(Duration::from_secs(3), serving).await;
        let agent_side = match agent_outcome {
            Ok(Ok(Err(error))) => error.to_string(),
            Ok(Ok(Ok(()))) => "session ended".to_owned(),
            _ => "still waiting".to_owned(),
        };
        Some(json!({
            "kind": kind,
            "refused": matches!(stage, "handshake" | "application"),
            "stage": stage,
            "controller_saw": detail,
            "agent_saw": agent_side,
            "agent_state_after": fresh.state().await,
            "note": "tried against a fresh agent instance, so the live session is not disturbed",
        }))
    }

    /// A TLS client that trusts the authority but has no certificate of its own. Under TLS 1.3 the client's side of the
    /// handshake finishes before the server looks for the certificate, so the refusal arrives on the first read.
    async fn without_certificate(
        client: DuplexStream,
        ca: &rustls::pki_types::CertificateDer<'static>,
    ) -> (&'static str, String) {
        let mut roots = rustls::RootCertStore::empty();
        if roots.add(ca.clone()).is_err() {
            return ("setup", "could not trust the authority".to_owned());
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let Ok(builder) = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
        else {
            return ("setup", "could not build the TLS client".to_owned());
        };
        let config = builder.with_root_certificates(roots).with_no_client_auth();
        let Ok(name) = rustls::pki_types::ServerName::try_from(AGENT_NAME) else {
            return ("setup", "bad server name".to_owned());
        };
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let mut tls =
            match tokio::time::timeout(Duration::from_secs(5), connector.connect(name, client))
                .await
            {
                Ok(Ok(tls)) => tls,
                Ok(Err(error)) => return ("handshake", format!("I/O: {error}")),
                Err(_) => return ("handshake", "timed out".to_owned()),
            };
        let _ = leash_bridge::write_frame(&mut tls, &leash_bridge::Request::Status {}).await;
        match leash_bridge::read_frame::<leash_bridge::Response>(&mut tls).await {
            Err(error) => ("handshake", format!("the agent hung up: {error}")),
            Ok(_) => ("none", "the agent answered".to_owned()),
        }
    }

    async fn attempt(
        client: DuplexStream,
        ca: &rustls::pki_types::CertificateDer<'static>,
        identity: Identity,
    ) -> (&'static str, String) {
        match Controller::connect(client, ca, identity).await {
            Err(error) => ("handshake", describe(&error)),
            Ok(mut controller) => match controller.status().await {
                Err(error) => ("handshake", describe(&error)),
                Ok(_) => ("none", "the agent answered".to_owned()),
            },
        }
    }
}
