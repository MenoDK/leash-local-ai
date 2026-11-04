# ADR 0005: A bridge that sleeps until the right certificate wakes it, and has no command line

Status: Accepted

## Context
A guest machine (a virtual machine used for risky work, say) that a host can drive is a door between the two. The usual
mistakes are the same ones as everywhere: a port that anything on the network can reach, a shared secret in a script, a
"run this string" operation because it was the quickest to write, and an agent that is always listening.

## Decision
- **The host chooses; the guest never does.** The controller opens the connection and the agent only answers. Nothing on
  the guest side dials out.
- **Dormant by default.** Before an authenticated `activate` the agent answers `status` and `activate` and nothing else. It
  goes back to dormant by itself when the heartbeat stops (twelve seconds in the demo), when the controller deactivates it,
  or when the session ends. A late heartbeat does not revive an expired activation. *No heartbeat, no agent.*
- **Mutual TLS 1.3, and the role is checked as well as the signature.** Both sides present a certificate from the same demo
  authority. A certificate from the right authority in the *wrong role* (an agent's certificate used as a controller) passes
  the handshake and is refused at the application layer. The lab shows the five ways to fail and where each one stops:
  `wrong_role`, `foreign_ca`, `expired`, `no_certificate`, `not_tls`.
- **Four operations, and none of them is a command line.** `status`, `ping`, `echo` (capped at 256 characters) and
  `uptime`, plus the lifecycle messages. The wire format is a tagged enum; an unknown operation does not deserialise, so
  `{"kind":"exec"}` is refused before any code can look at it.
- **Frames are bounded before anything is allocated.** A four-byte length prefix, a 16 KiB cap checked first, and
  `deny_unknown_fields` on every message.
- **The transport is a trait.** The demo runs the agent and the controller over an in-process pipe, which is what every test
  uses. A hypervisor-socket transport (virtio-vsock) sits behind a cargo feature.

## Rejected alternatives
- **SSH.** Excellent, and a different project: it brings a shell, which is what this is trying not to have.
- **A shared token in a header.** One leak and the door is open; and a token proves possession, not role.
- **An agent that is always active, protected by the network.** Being on the right network is not an identity.
- **An `exec` operation with a deny list.** [ADR 0003](0003-a-broker-instead-of-a-shell.md) says why not.
- **A listening port on the VM bridge.** The point of the socket transport is that there is nothing on the network to scan.

## Consequences
- (+) A mutant guards the role check (`scripts/sabotage.sh`): without it the wrong-role impostor is served, and it is killed.
- (+) Every impostor leaves the agent dormant, and the lab prints what each side saw.
- (−) **The vsock transport has been compiled and linted in CI but never run between a real host and a real guest.** The
  protocol, the TLS and the state machine are tested end to end over the in-process pipe. The README says so.
- (−) The authority is generated at start-up for the demo. A real deployment needs offline issuance, rotation and
  revocation, none of which is built.
- (−) Four operations are a demonstration of the shape, not a toolbox.
