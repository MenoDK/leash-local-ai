//! The agent's state machine, free of any I/O so that every transition can be tested with a fake clock.
//!
//! Dormant → (authenticated `activate`) → Active → (no heartbeat within the timeout, or `deactivate`, or the session
//! ends) → Dormant. In the dormant state the agent answers `status` and `activate` and nothing else.

use std::time::Duration;

use tokio::time::Instant;

use crate::protocol::{AgentState, ErrorCode, Op, Request, Response};

/// Longest `echo` text, in characters.
pub const MAX_ECHO_CHARS: usize = 256;

#[derive(Debug)]
pub struct AgentCore {
    heartbeat_timeout: Duration,
    started: Instant,
    active_until: Option<Instant>,
}

impl AgentCore {
    #[must_use]
    pub fn new(heartbeat_timeout: Duration) -> Self {
        Self {
            heartbeat_timeout,
            started: Instant::now(),
            active_until: None,
        }
    }

    /// Current state, taking the clock into account: an expired activation is dormant.
    #[must_use]
    pub fn state(&self) -> AgentState {
        match self.active_until {
            Some(until) if Instant::now() < until => AgentState::Active,
            _ => AgentState::Dormant,
        }
    }

    /// Return to dormant (the session ended).
    pub fn reset(&mut self) {
        self.active_until = None;
    }

    fn refresh(&mut self) {
        self.active_until = Some(Instant::now() + self.heartbeat_timeout);
    }

    fn expires_in_ms(&self) -> Option<u64> {
        let until = self.active_until?;
        let left = until.checked_duration_since(Instant::now())?;
        u64::try_from(left.as_millis()).ok()
    }

    fn dormant() -> Response {
        Response::Error {
            code: ErrorCode::Dormant,
            message: "the agent is dormant; activate it first".to_owned(),
        }
    }

    /// Handle one request from an authenticated controller.
    pub fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Status { .. } => Response::State {
                state: self.state(),
                expires_in_ms: self.expires_in_ms(),
            },
            Request::Activate { .. } => {
                self.refresh();
                Response::State {
                    state: AgentState::Active,
                    expires_in_ms: self.expires_in_ms(),
                }
            }
            _ if self.state() == AgentState::Dormant => Self::dormant(),
            Request::Heartbeat { .. } => {
                self.refresh();
                Response::State {
                    state: AgentState::Active,
                    expires_in_ms: self.expires_in_ms(),
                }
            }
            Request::Deactivate { .. } => {
                self.reset();
                Response::State {
                    state: AgentState::Dormant,
                    expires_in_ms: None,
                }
            }
            Request::Run { op } => self.run(op),
        }
    }

    fn run(&self, op: Op) -> Response {
        match op {
            Op::Ping {} => Response::Output {
                output: "pong".to_owned(),
            },
            Op::Uptime {} => Response::Output {
                output: format!("{}s", self.started.elapsed().as_secs()),
            },
            Op::Echo { text } => {
                if text.chars().count() > MAX_ECHO_CHARS || text.chars().any(char::is_control) {
                    Response::Error {
                        code: ErrorCode::BadRequest,
                        message: format!(
                            "echo takes at most {MAX_ECHO_CHARS} printable characters"
                        ),
                    }
                } else {
                    Response::Output { output: text }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(15);

    fn is_dormant_error(r: &Response) -> bool {
        matches!(
            r,
            Response::Error {
                code: ErrorCode::Dormant,
                ..
            }
        )
    }

    #[tokio::test(start_paused = true)]
    async fn a_dormant_agent_answers_status_and_activate_and_nothing_else() {
        let mut agent = AgentCore::new(TIMEOUT);
        assert_eq!(
            agent.handle(Request::Status {}),
            Response::State {
                state: AgentState::Dormant,
                expires_in_ms: None
            }
        );
        for request in [
            Request::Heartbeat {},
            Request::Deactivate {},
            Request::Run { op: Op::Ping {} },
            Request::Run {
                op: Op::Echo { text: "x".into() },
            },
        ] {
            assert!(
                is_dormant_error(&agent.handle(request.clone())),
                "{request:?} was served while dormant"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn activation_allows_operations_and_expires_without_a_heartbeat() {
        let mut agent = AgentCore::new(TIMEOUT);
        agent.handle(Request::Activate {});
        assert_eq!(
            agent.handle(Request::Run { op: Op::Ping {} }),
            Response::Output {
                output: "pong".into()
            }
        );

        tokio::time::advance(TIMEOUT - Duration::from_secs(1)).await;
        assert_eq!(agent.state(), AgentState::Active);
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(
            agent.state(),
            AgentState::Dormant,
            "fail closed: no heartbeat, no agent"
        );
        assert!(is_dormant_error(
            &agent.handle(Request::Run { op: Op::Ping {} })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn a_heartbeat_extends_the_activation_and_only_while_active() {
        let mut agent = AgentCore::new(TIMEOUT);
        agent.handle(Request::Activate {});
        for _ in 0..5 {
            tokio::time::advance(Duration::from_secs(10)).await;
            assert!(matches!(
                agent.handle(Request::Heartbeat {}),
                Response::State {
                    state: AgentState::Active,
                    ..
                }
            ));
        }
        tokio::time::advance(TIMEOUT).await;
        assert!(
            is_dormant_error(&agent.handle(Request::Heartbeat {})),
            "a late heartbeat does not revive an expired activation"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn deactivate_and_reset_return_to_dormant() {
        let mut agent = AgentCore::new(TIMEOUT);
        agent.handle(Request::Activate {});
        agent.handle(Request::Deactivate {});
        assert_eq!(agent.state(), AgentState::Dormant);
        agent.handle(Request::Activate {});
        agent.reset();
        assert_eq!(agent.state(), AgentState::Dormant);
    }

    #[tokio::test(start_paused = true)]
    async fn echo_takes_short_printable_text_only() {
        let mut agent = AgentCore::new(TIMEOUT);
        agent.handle(Request::Activate {});
        for bad in ["a\nb", "a\u{1b}[31m", &"x".repeat(257), "a\0b"] {
            let reply = agent.handle(Request::Run {
                op: Op::Echo {
                    text: bad.to_owned(),
                },
            });
            assert!(
                matches!(
                    reply,
                    Response::Error {
                        code: ErrorCode::BadRequest,
                        ..
                    }
                ),
                "echoed {bad:?}"
            );
        }
        assert_eq!(
            agent.handle(Request::Run {
                op: Op::Echo {
                    text: "olá, мир".into()
                }
            }),
            Response::Output {
                output: "olá, мир".into()
            }
        );
    }
}
