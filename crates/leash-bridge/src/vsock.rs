//! The hypervisor transport (`AF_VSOCK`).
//!
//! **Compile-checked only.** A vsock device needs a virtual machine; neither the tests nor CI have one, so the two
//! functions below are the thin layer that has not been run in this repository. Everything above them (TLS, framing, the
//! state machine) is the code the tests exercise over an in-process pipe and a Unix socket.

use tokio_vsock::{VMADDR_CID_ANY, VsockAddr, VsockListener, VsockStream};

/// Listen on a vsock port (in the guest).
///
/// # Errors
/// The socket cannot be bound (no vsock device, port in use).
pub fn listen(port: u32) -> std::io::Result<VsockListener> {
    VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, port))
}

/// Connect from the host to the guest at `cid`.
///
/// # Errors
/// The guest is not reachable.
pub async fn connect(cid: u32, port: u32) -> std::io::Result<VsockStream> {
    VsockStream::connect(VsockAddr::new(cid, port)).await
}
