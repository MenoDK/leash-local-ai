//! The real handshake over an in-process pipe and over a Unix socket, including the connections that must fail.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use leash_bridge::{
    AGENT_NAME, Agent, AgentState, Authority, BridgeError, CONTROLLER_NAME, Controller, ErrorCode,
    FrameError, Identity, Op, Request, Validity, read_frame, write_frame,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

const TIMEOUT: Duration = Duration::from_secs(15);

struct Setup {
    ca: Authority,
    agent: Arc<Agent>,
}

fn setup() -> Setup {
    let ca = Authority::new("demo ca").unwrap();
    let agent_identity = ca.issue(AGENT_NAME, Validity::Current).unwrap();
    let agent = Arc::new(Agent::new(&ca.certificate(), agent_identity, TIMEOUT).unwrap());
    Setup { ca, agent }
}

/// Start the agent on one end of a pipe; returns the other end and the agent task.
fn connect_pipe(
    agent: &Arc<Agent>,
) -> (
    DuplexStream,
    tokio::task::JoinHandle<Result<(), BridgeError>>,
) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let agent = Arc::clone(agent);
    (
        client,
        tokio::spawn(async move { agent.serve(server).await }),
    )
}

#[tokio::test]
async fn the_controller_wakes_the_agent_runs_typed_operations_and_puts_it_back_to_sleep() {
    let s = setup();
    let (pipe, served) = connect_pipe(&s.agent);
    let mut controller = Controller::connect(
        pipe,
        &s.ca.certificate(),
        s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(controller.status().await.unwrap().0, AgentState::Dormant);
    assert!(
        matches!(
            controller.run(Op::Ping {}).await,
            Err(BridgeError::Agent(ErrorCode::Dormant, _))
        ),
        "dormant agents run nothing"
    );

    assert_eq!(controller.activate().await.unwrap().0, AgentState::Active);
    assert_eq!(controller.run(Op::Ping {}).await.unwrap(), "pong");
    assert_eq!(
        controller
            .run(Op::Echo {
                text: "olá".into()
            })
            .await
            .unwrap(),
        "olá"
    );
    assert!(controller.heartbeat().await.is_ok());

    assert_eq!(
        controller.deactivate().await.unwrap().0,
        AgentState::Dormant
    );
    assert!(matches!(
        controller.run(Op::Ping {}).await,
        Err(BridgeError::Agent(ErrorCode::Dormant, _))
    ));

    drop(controller);
    served.await.unwrap().unwrap();
    assert_eq!(s.agent.state().await, AgentState::Dormant);
}

#[tokio::test]
async fn ending_the_session_leaves_the_agent_dormant() {
    let s = setup();
    let (pipe, served) = connect_pipe(&s.agent);
    let mut controller = Controller::connect(
        pipe,
        &s.ca.certificate(),
        s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap(),
    )
    .await
    .unwrap();
    controller.activate().await.unwrap();
    assert_eq!(s.agent.state().await, AgentState::Active);
    drop(controller);
    served.await.unwrap().unwrap();
    assert_eq!(
        s.agent.state().await,
        AgentState::Dormant,
        "a vanished controller does not leave an active agent behind"
    );
}

#[tokio::test]
async fn a_valid_certificate_in_the_wrong_role_may_not_control_the_agent() {
    let s = setup();
    let (pipe, served) = connect_pipe(&s.agent);
    // Signed by the right CA, but it is the *agent's* identity, not the controller's.
    let impostor = s.ca.issue(AGENT_NAME, Validity::Current).unwrap();
    let mut controller = Controller::connect(pipe, &s.ca.certificate(), impostor)
        .await
        .unwrap();
    let reply = controller.activate().await;
    assert!(
        matches!(reply, Err(BridgeError::Agent(ErrorCode::Unauthorized, _))),
        "{reply:?}"
    );
    served.await.unwrap().unwrap();
    assert_eq!(s.agent.state().await, AgentState::Dormant);
}

#[tokio::test]
async fn a_certificate_from_another_authority_fails_the_handshake() {
    let s = setup();
    let other = Authority::new("someone else's ca").unwrap();
    let (pipe, served) = connect_pipe(&s.agent);
    let forged = other.issue(CONTROLLER_NAME, Validity::Current).unwrap();
    let result = Controller::connect(pipe, &s.ca.certificate(), forged).await;
    // The client side may only notice on the first read, once the server has rejected its certificate.
    let failed = match result {
        Err(_) => true,
        Ok(mut c) => c.status().await.is_err(),
    };
    assert!(failed, "an unknown CA must not get a session");
    assert!(served.await.unwrap().is_err());
}

#[tokio::test]
async fn an_expired_certificate_fails_the_handshake() {
    let s = setup();
    let (pipe, served) = connect_pipe(&s.agent);
    let expired = s.ca.issue(CONTROLLER_NAME, Validity::Expired).unwrap();
    let failed = match Controller::connect(pipe, &s.ca.certificate(), expired).await {
        Err(_) => true,
        Ok(mut c) => c.status().await.is_err(),
    };
    assert!(failed);
    assert!(served.await.unwrap().is_err());
}

#[tokio::test]
async fn a_client_without_any_certificate_is_turned_away() {
    let s = setup();
    let (mut pipe, served) = connect_pipe(&s.agent);
    // A plain TLS client hello would need rustls plumbing; the cheap attack is plain bytes, which is not TLS at all.
    pipe.write_all(b"{\"type\":\"activate\"}\n").await.unwrap();
    let mut sink = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), pipe.read_to_end(&mut sink)).await;
    assert!(
        served.await.unwrap().is_err(),
        "the agent must not treat plaintext as a session"
    );
    assert_eq!(s.agent.state().await, AgentState::Dormant);
}

#[tokio::test]
async fn the_controller_refuses_an_agent_that_is_not_the_agent() {
    let s = setup();
    let other = Authority::new("rogue ca").unwrap();
    let rogue_identity = other.issue(AGENT_NAME, Validity::Current).unwrap();
    let rogue = Arc::new(Agent::new(&other.certificate(), rogue_identity, TIMEOUT).unwrap());
    let (pipe, _served) = connect_pipe(&rogue);
    let result = Controller::connect(
        pipe,
        &s.ca.certificate(),
        s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap(),
    )
    .await;
    assert!(result.is_err(), "the controller trusts only its own CA");
}

#[tokio::test]
async fn the_controller_checks_the_name_of_the_agent_it_reached() {
    let s = setup();
    // Right CA, wrong name: a leaf for some other host.
    let wrong = s.ca.issue("some-other-host", Validity::Current).unwrap();
    let agent = Arc::new(Agent::new(&s.ca.certificate(), wrong, TIMEOUT).unwrap());
    let (pipe, _served) = connect_pipe(&agent);
    let result = Controller::connect(
        pipe,
        &s.ca.certificate(),
        s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap(),
    )
    .await;
    assert!(result.is_err());
}

/// A TLS client built by hand, so a test can send bytes the typed `Controller` would never produce.
async fn raw_tls_client(
    s: &Setup,
    pipe: DuplexStream,
) -> tokio_rustls::client::TlsStream<DuplexStream> {
    let identity = s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(s.ca.certificate()).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_client_auth_cert(identity.chain, identity.key)
        .unwrap();
    let name = rustls::pki_types::ServerName::try_from(AGENT_NAME).unwrap();
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, pipe)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_oversized_frame_header_ends_the_session_before_anything_is_allocated() {
    let s = setup();
    let (pipe, served) = connect_pipe(&s.agent);
    let mut tls = raw_tls_client(&s, pipe).await;
    tls.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
    tls.flush().await.unwrap();
    let outcome = served.await.unwrap();
    assert!(
        matches!(outcome, Err(BridgeError::Frame(FrameError::TooLarge(_)))),
        "{outcome:?}"
    );
    assert_eq!(s.agent.state().await, AgentState::Dormant);
}

#[tokio::test]
async fn a_frame_that_is_not_a_request_ends_the_session() {
    let s = setup();
    let (pipe, served) = connect_pipe(&s.agent);
    let mut tls = raw_tls_client(&s, pipe).await;
    let body = br#"{"type":"run","op":{"kind":"shell","cmd":"id"}}"#;
    tls.write_all(&u32::try_from(body.len()).unwrap().to_be_bytes())
        .await
        .unwrap();
    tls.write_all(body).await.unwrap();
    tls.flush().await.unwrap();
    assert!(matches!(
        served.await.unwrap(),
        Err(BridgeError::Frame(FrameError::Invalid(_)))
    ));
}

#[tokio::test(start_paused = true)]
async fn a_peer_that_never_finishes_the_handshake_is_cut_off() {
    let s = setup();
    let (_silent_client, server) = tokio::io::duplex(1024);
    let agent = Arc::clone(&s.agent);
    let outcome = tokio::spawn(async move { agent.serve(server).await })
        .await
        .unwrap();
    assert!(matches!(outcome, Err(BridgeError::Handshake(m)) if m.contains("timed out")));
}

#[tokio::test(start_paused = true)]
async fn a_controller_that_goes_silent_after_the_handshake_is_dropped_and_the_agent_sleeps() {
    let s = setup();
    let (pipe, served) = connect_pipe(&s.agent);
    let mut controller = Controller::connect(
        pipe,
        &s.ca.certificate(),
        s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap(),
    )
    .await
    .unwrap();
    controller.activate().await.unwrap();
    served.await.unwrap().unwrap(); // the agent gave up waiting for the next request
    assert_eq!(s.agent.state().await, AgentState::Dormant);
    drop(controller);
}

#[tokio::test]
async fn only_one_controller_at_a_time() {
    let s = setup();
    let (first_pipe, first_served) = connect_pipe(&s.agent);
    let mut first = Controller::connect(
        first_pipe,
        &s.ca.certificate(),
        s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap(),
    )
    .await
    .unwrap();
    first.activate().await.unwrap();

    let (second_pipe, second_served) = connect_pipe(&s.agent);
    let (ca_cert, identity) = (
        s.ca.certificate(),
        s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap(),
    );
    let second = Controller::connect(second_pipe, &ca_cert, identity);
    assert!(
        tokio::time::timeout(Duration::from_secs(3), second)
            .await
            .map_or(true, |r| r.is_err()),
        "a second session must not be served"
    );
    let _ = second_served.await;

    assert_eq!(
        first.run(Op::Ping {}).await.unwrap(),
        "pong",
        "the first session is unaffected"
    );
    drop(first);
    first_served.await.unwrap().unwrap();
}

#[tokio::test]
async fn it_works_the_same_over_a_unix_socket() {
    let s = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bridge.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let agent = Arc::clone(&s.agent);
    let served = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        agent.serve(stream).await
    });
    let stream = tokio::net::UnixStream::connect(&path).await.unwrap();
    let mut controller = Controller::connect(
        stream,
        &s.ca.certificate(),
        s.ca.issue(CONTROLLER_NAME, Validity::Current).unwrap(),
    )
    .await
    .unwrap();
    controller.activate().await.unwrap();
    assert_eq!(
        controller
            .run(Op::Echo {
                text: "unix".into()
            })
            .await
            .unwrap(),
        "unix"
    );
    drop(controller);
    served.await.unwrap().unwrap();
}

#[tokio::test]
async fn frames_helpers_reject_what_the_server_would() {
    let (mut a, mut b) = tokio::io::duplex(1024);
    write_frame(&mut a, &Request::Status {}).await.unwrap();
    let got: Request = read_frame(&mut b).await.unwrap();
    assert_eq!(got, Request::Status {});
    a.write_all(&(u32::MAX).to_be_bytes()).await.unwrap();
    assert!(matches!(
        read_frame::<Request>(&mut b).await,
        Err(FrameError::TooLarge(_))
    ));
    let _: Identity = Authority::new("x")
        .unwrap()
        .issue("y", Validity::Current)
        .unwrap();
}
