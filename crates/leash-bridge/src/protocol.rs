//! The wire protocol: length-prefixed JSON frames and a closed set of typed operations.
//!
//! Nothing on this channel is a command line. The agent understands a handful of operations, each with typed
//! parameters; an unknown field or an unknown operation is a refusal, not an ignored extra.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest frame either side accepts. The length is checked before any buffer is allocated.
pub const MAX_FRAME_BYTES: usize = 16 * 1024;

/// What the controller can ask the agent to do once it is active.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    /// Answers `pong`.
    Ping {},
    /// Returns the text unchanged (printable characters only, at most 256).
    Echo { text: String },
    /// Seconds since the agent started.
    Uptime {},
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Ask whether the agent is dormant or active. Allowed in both states.
    Status {},
    /// Wake the agent for this session.
    Activate {},
    /// Keep an active agent active.
    Heartbeat {},
    /// Run one typed operation. Only an active agent does.
    Run { op: Op },
    /// Go back to dormant.
    Deactivate {},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Dormant,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The agent is dormant and the request needs it active.
    Dormant,
    /// The peer is not the controller (right CA, wrong role).
    Unauthorized,
    /// The operation's parameters are not acceptable.
    BadRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    State {
        state: AgentState,
        /// Milliseconds until an active agent falls dormant without a heartbeat.
        expires_in_ms: Option<u64>,
    },
    Output {
        output: String,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_BYTES}-byte limit")]
    TooLarge(usize),
    #[error("connection closed")]
    Closed,
    #[error("frame is not valid for this protocol: {0}")]
    Invalid(String),
    #[error("I/O: {0}")]
    Io(String),
}

/// Read one frame: four bytes of big-endian length, then that many bytes of JSON.
///
/// # Errors
/// The peer closed, the length is above the limit (nothing is allocated for it), or the JSON does not fit `T`.
pub async fn read_frame<T: serde::de::DeserializeOwned>(
    stream: &mut (impl AsyncRead + Unpin),
) -> Result<T, FrameError> {
    let mut len = [0u8; 4];
    stream
        .read_exact(&mut len)
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::UnexpectedEof => FrameError::Closed,
            _ => FrameError::Io(e.to_string()),
        })?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|e| FrameError::Io(e.to_string()))?;
    serde_json::from_slice(&body).map_err(|e| FrameError::Invalid(e.to_string()))
}

/// # Errors
/// Serialisation or I/O failure, or a value too large to send.
pub async fn write_frame<T: Serialize>(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> Result<(), FrameError> {
    let body = serde_json::to_vec(value).map_err(|e| FrameError::Invalid(e.to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(body.len()));
    }
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(body.len()))?;
    stream
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| FrameError::Io(e.to_string()))?;
    stream
        .write_all(&body)
        .await
        .map_err(|e| FrameError::Io(e.to_string()))?;
    stream
        .flush()
        .await
        .map_err(|e| FrameError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn roundtrip<T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
        value: T,
    ) {
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        write_frame(&mut a, &value).await.expect("write");
        assert_eq!(read_frame::<T>(&mut b).await.expect("read"), value);
    }

    #[tokio::test]
    async fn requests_and_responses_roundtrip() {
        roundtrip(Request::Status {}).await;
        roundtrip(Request::Run {
            op: Op::Echo { text: "hi".into() },
        })
        .await;
        roundtrip(Request::Run { op: Op::Ping {} }).await;
        roundtrip(Response::Output {
            output: "pong".into(),
        })
        .await;
        roundtrip(Response::State {
            state: AgentState::Active,
            expires_in_ms: Some(5000),
        })
        .await;
    }

    #[tokio::test]
    async fn the_length_is_checked_before_anything_is_allocated() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&u32::MAX.to_be_bytes()).await.expect("write");
        assert!(matches!(
            read_frame::<Request>(&mut b).await,
            Err(FrameError::TooLarge(_))
        ));
    }

    #[tokio::test]
    async fn unknown_operations_and_unknown_fields_are_refused() {
        for body in [
            r#"{"type":"run","op":{"kind":"shell","cmd":"id"}}"#,
            r#"{"type":"run","op":{"kind":"ping","extra":1}}"#,
            r#"{"type":"run","op":"ping"}"#,
            r#"{"type":"run","op":{"kind":"ping"},"extra":1}"#,
            r#"{"type":"status","sudo":true}"#,
            r#"{"type":"format_disk"}"#,
            r#"[]"#,
            r#"not json"#,
        ] {
            let (mut a, mut b) = tokio::io::duplex(1024);
            a.write_all(&u32::try_from(body.len()).expect("len").to_be_bytes())
                .await
                .expect("write");
            a.write_all(body.as_bytes()).await.expect("write");
            assert!(
                matches!(
                    read_frame::<Request>(&mut b).await,
                    Err(FrameError::Invalid(_))
                ),
                "accepted {body}"
            );
        }
    }

    #[tokio::test]
    async fn a_closed_stream_is_reported_as_closed() {
        let (a, mut b) = tokio::io::duplex(64);
        drop(a);
        assert!(matches!(
            read_frame::<Request>(&mut b).await,
            Err(FrameError::Closed)
        ));
    }
}
