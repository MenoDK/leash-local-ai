//! Mutual TLS, the agent's serving loop and the controller's client.

use std::sync::Arc;
use std::time::Duration;

use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::server::WebPkiClientVerifier;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::agent::AgentCore;
use crate::pki::{AGENT_NAME, CONTROLLER_NAME, Identity, dns_names};
use crate::protocol::{
    AgentState, ErrorCode, FrameError, Op, Request, Response, read_frame, write_frame,
};

/// Time allowed for the TLS handshake, and for the controller to send its next request.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("TLS configuration: {0}")]
    Tls(String),
    #[error("handshake failed: {0}")]
    Handshake(String),
    #[error("{0}")]
    Frame(#[from] FrameError),
    #[error("the agent answered with an error: {0:?}: {1}")]
    Agent(ErrorCode, String),
    #[error("unexpected reply from the agent")]
    Unexpected,
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn roots(ca: &CertificateDer<'static>) -> Result<RootCertStore, BridgeError> {
    let mut store = RootCertStore::empty();
    store
        .add(ca.clone())
        .map_err(|e| BridgeError::Tls(e.to_string()))?;
    Ok(store)
}

/// TLS 1.3 only; the client must present a certificate signed by `ca`.
fn server_config(
    ca: &CertificateDer<'static>,
    identity: Identity,
) -> Result<rustls::ServerConfig, BridgeError> {
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots(ca)?), provider())
        .build()
        .map_err(|e| BridgeError::Tls(e.to_string()))?;
    let (chain, key) = identity.into_parts();
    rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| BridgeError::Tls(e.to_string()))?
        .with_client_cert_verifier(verifier)
        .with_single_cert(chain, key)
        .map_err(|e| BridgeError::Tls(e.to_string()))
}

fn client_config(
    ca: &CertificateDer<'static>,
    identity: Identity,
) -> Result<rustls::ClientConfig, BridgeError> {
    let (chain, key) = identity.into_parts();
    rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| BridgeError::Tls(e.to_string()))?
        .with_root_certificates(roots(ca)?)
        .with_client_auth_cert(chain, key)
        .map_err(|e| BridgeError::Tls(e.to_string()))
}

/// The agent: waits for the controller, never dials out.
pub struct Agent {
    acceptor: TlsAcceptor,
    core: Arc<Mutex<AgentCore>>,
    /// One session at a time: while the controller is connected, a second connection is turned away.
    session: Arc<Mutex<()>>,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Agent(..)")
    }
}

impl Agent {
    /// # Errors
    /// The identity or the CA cannot form a TLS configuration.
    pub fn new(
        ca: &CertificateDer<'static>,
        identity: Identity,
        heartbeat_timeout: Duration,
    ) -> Result<Self, BridgeError> {
        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(server_config(ca, identity)?)),
            core: Arc::new(Mutex::new(AgentCore::new(heartbeat_timeout))),
            session: Arc::new(Mutex::new(())),
        })
    }

    /// State as seen from outside the session (for status pages).
    pub async fn state(&self) -> AgentState {
        self.core.lock().await.state()
    }

    /// Serve one connection. Returns when the controller disconnects, misbehaves or the handshake fails. The agent is
    /// dormant again afterwards in every case.
    pub async fn serve<S>(&self, stream: S) -> Result<(), BridgeError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let Ok(_session) = self.session.try_lock() else {
            // Another controller is connected. The stream is dropped without a handshake.
            return Ok(());
        };
        let outcome = self.serve_session(stream).await;
        self.core.lock().await.reset();
        outcome
    }

    async fn serve_session<S>(&self, stream: S) -> Result<(), BridgeError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let mut tls = tokio::time::timeout(HANDSHAKE_TIMEOUT, self.acceptor.accept(stream))
            .await
            .map_err(|_| BridgeError::Handshake("timed out".to_owned()))?
            .map_err(|e| BridgeError::Handshake(e.to_string()))?;

        // Signed by our CA is not enough: the peer must be the *controller*, not the agent's own role or any other leaf.
        let peer_is_controller = tls
            .get_ref()
            .1
            .peer_certificates()
            .and_then(<[_]>::first)
            .is_some_and(|cert| dns_names(cert).iter().any(|name| name == CONTROLLER_NAME));

        loop {
            let request: Request =
                match tokio::time::timeout(IDLE_TIMEOUT, read_frame(&mut tls)).await {
                    Err(_) => return Ok(()),
                    Ok(Err(FrameError::Closed)) => return Ok(()),
                    Ok(Err(other)) => return Err(other.into()),
                    Ok(Ok(request)) => request,
                };
            let response = if peer_is_controller {
                self.core.lock().await.handle(request)
            } else {
                Response::Error {
                    code: ErrorCode::Unauthorized,
                    message: "this identity may not control the agent".to_owned(),
                }
            };
            let refused = matches!(
                response,
                Response::Error {
                    code: ErrorCode::Unauthorized,
                    ..
                }
            );
            write_frame(&mut tls, &response).await?;
            if refused {
                return Ok(());
            }
        }
    }
}

/// The controller: dials the agent and sends typed requests.
pub struct Controller<S> {
    stream: tokio_rustls::client::TlsStream<S>,
}

impl<S> std::fmt::Debug for Controller<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Controller(..)")
    }
}

impl<S> Controller<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    /// Handshake over `stream`, trusting only `ca` and expecting the agent's identity.
    ///
    /// # Errors
    /// The TLS configuration is unusable or the handshake fails (wrong CA, wrong name, expired certificate…).
    pub async fn connect(
        stream: S,
        ca: &CertificateDer<'static>,
        identity: Identity,
    ) -> Result<Self, BridgeError> {
        let connector = TlsConnector::from(Arc::new(client_config(ca, identity)?));
        let name = ServerName::try_from(AGENT_NAME).map_err(|e| BridgeError::Tls(e.to_string()))?;
        let stream = tokio::time::timeout(HANDSHAKE_TIMEOUT, connector.connect(name, stream))
            .await
            .map_err(|_| BridgeError::Handshake("timed out".to_owned()))?
            .map_err(|e| BridgeError::Handshake(e.to_string()))?;
        Ok(Self { stream })
    }

    async fn call(&mut self, request: Request) -> Result<Response, BridgeError> {
        write_frame(&mut self.stream, &request).await?;
        Ok(read_frame(&mut self.stream).await?)
    }

    async fn state_reply(
        &mut self,
        request: Request,
    ) -> Result<(AgentState, Option<u64>), BridgeError> {
        match self.call(request).await? {
            Response::State {
                state,
                expires_in_ms,
            } => Ok((state, expires_in_ms)),
            Response::Error { code, message } => Err(BridgeError::Agent(code, message)),
            Response::Output { .. } => Err(BridgeError::Unexpected),
        }
    }

    /// # Errors
    /// Transport failure or an error reply.
    pub async fn status(&mut self) -> Result<(AgentState, Option<u64>), BridgeError> {
        self.state_reply(Request::Status {}).await
    }

    /// # Errors
    /// Transport failure or an error reply.
    pub async fn activate(&mut self) -> Result<(AgentState, Option<u64>), BridgeError> {
        self.state_reply(Request::Activate {}).await
    }

    /// # Errors
    /// Transport failure, or the agent is dormant.
    pub async fn heartbeat(&mut self) -> Result<(AgentState, Option<u64>), BridgeError> {
        self.state_reply(Request::Heartbeat {}).await
    }

    /// # Errors
    /// Transport failure, or the agent is dormant.
    pub async fn deactivate(&mut self) -> Result<(AgentState, Option<u64>), BridgeError> {
        self.state_reply(Request::Deactivate {}).await
    }

    /// # Errors
    /// Transport failure, the agent is dormant, or it refuses the parameters.
    pub async fn run(&mut self, op: Op) -> Result<String, BridgeError> {
        match self.call(Request::Run { op }).await? {
            Response::Output { output } => Ok(output),
            Response::Error { code, message } => Err(BridgeError::Agent(code, message)),
            Response::State { .. } => Err(BridgeError::Unexpected),
        }
    }
}
