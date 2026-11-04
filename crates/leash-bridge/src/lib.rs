//! # leash-bridge
//!
//! A channel between a host and a guest (a VM) with four properties:
//!
//! - **The host dials, the guest listens.** The guest never opens a connection to the host.
//! - **Mutual TLS 1.3.** Each side proves it holds a certificate from the shared CA, and the agent additionally checks
//!   that the peer has the *controller* identity: a valid certificate in the wrong role is refused.
//! - **Dormant until activated.** The agent answers `status` and `activate` and nothing else until an authenticated
//!   `activate`; it falls dormant again without a heartbeat, when the session ends, or on `deactivate`.
//! - **Typed operations only.** `ping`, `echo`, `uptime`. There is no command string anywhere in the protocol.
//!
//! The transport is whatever implements `AsyncRead + AsyncWrite`: an in-process pipe for tests and the demo, a Unix
//! socket, or (feature `vsock`) a hypervisor socket. The protocol and the TLS do not change.

mod agent;
mod pki;
mod protocol;
mod session;
#[cfg(feature = "vsock")]
pub mod vsock;

pub use agent::{AgentCore, MAX_ECHO_CHARS};
pub use pki::{AGENT_NAME, Authority, CONTROLLER_NAME, Identity, PkiError, Validity};
pub use protocol::{
    AgentState, ErrorCode, FrameError, MAX_FRAME_BYTES, Op, Request, Response, read_frame,
    write_frame,
};
pub use session::{Agent, BridgeError, Controller};
