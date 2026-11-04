//! Request-level protections that apply to the whole server: which `Host` and `Origin` it answers to.
//!
//! The API has no login (it is a local tool), so these two checks are what keep a web page on another site from
//! driving it through the visitor's browser. `Host` defeats DNS rebinding; `Origin` defeats cross-site requests that a
//! browser would otherwise send with the visitor's network position.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::{HOST, ORIGIN};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;

/// The host name part of an authority, lower-cased, without a port: `Localhost:8080` becomes `localhost`.
#[must_use]
pub fn host_name(authority: &str) -> String {
    let authority = authority.trim().to_ascii_lowercase();
    if authority.starts_with('[') {
        return authority
            .split_once(']')
            .map_or(authority.clone(), |(inside, _)| format!("{inside}]"));
    }
    authority.split(':').next().unwrap_or_default().to_owned()
}

fn origin_host(origin: &str) -> Option<String> {
    let (_, rest) = origin.split_once("://")?;
    Some(host_name(rest.split('/').next().unwrap_or_default()))
}

pub async fn guard(
    State(allowed): State<Arc<Vec<String>>>,
    request: Request,
    next: Next,
) -> Response {
    let host_ok = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| allowed.contains(&host_name(value)));
    if !host_ok {
        return ApiError::forbidden(
            "host_not_allowed",
            "this server answers only to its own host names",
        )
        .into_response();
    }
    if let Some(origin) = request.headers().get(ORIGIN) {
        let origin_ok = origin
            .to_str()
            .ok()
            .and_then(origin_host)
            .is_some_and(|host| allowed.contains(&host));
        if !origin_ok {
            return ApiError::forbidden(
                "origin_not_allowed",
                "requests from other sites are refused",
            )
            .into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names_lose_their_port_and_case() {
        assert_eq!(host_name("Localhost:8080"), "localhost");
        assert_eq!(host_name("127.0.0.1"), "127.0.0.1");
        assert_eq!(host_name("[::1]:8080"), "[::1]");
        assert_eq!(host_name("EVIL.example:80"), "evil.example");
    }

    #[test]
    fn origins_are_reduced_to_their_host() {
        assert_eq!(
            origin_host("http://localhost:8080").as_deref(),
            Some("localhost")
        );
        assert_eq!(
            origin_host("https://evil.example").as_deref(),
            Some("evil.example")
        );
        assert_eq!(origin_host("null"), None);
    }
}
