//! The whole application as one `Router`: the API, the static site, and the headers around both.

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::api;
use crate::security;
use crate::state::Shared;

/// Same policy as the other sites in this portfolio: nothing inline, nothing from another origin.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; \
                   object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

fn header_layer(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::if_not_present(name, HeaderValue::from_static(value))
}

pub fn app(state: Shared) -> Router {
    let hosts = Arc::new(state.config.allowed_hosts.clone());
    let site = ServeDir::new(&state.config.site_dir).append_index_html_on_directories(true);
    api::router(state)
        .fallback_service(site)
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(hosts, security::guard))
        .layer(header_layer(header::CONTENT_SECURITY_POLICY, CSP))
        .layer(header_layer(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(header_layer(header::REFERRER_POLICY, "no-referrer"))
        .layer(header_layer(header::X_FRAME_OPTIONS, "DENY"))
        .layer(header_layer(
            HeaderName::from_static("permissions-policy"),
            "camera=(), microphone=(), geolocation=(), payment=()",
        ))
        .layer(header_layer(
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ))
        .layer(header_layer(header::CACHE_CONTROL, "no-cache"))
}
