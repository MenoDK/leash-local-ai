//! The Ollama client against a small fake server, so nothing here needs a model.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test code: a failed expectation is the failure report

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use leash_censor::{Limits, Provenance, seal};
use leash_gateway::{Locality, OllamaProvider, Options, Provider, ProviderError, SystemPrompt};
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Value>>>;

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });
    format!("http://{addr}")
}

async fn chat(State(seen): State<Seen>, Json(body): Json<Value>) -> Json<Value> {
    seen.lock().expect("lock").push(body);
    Json(json!({"message": {"role": "assistant", "content": "three figures, rising"}}))
}

#[tokio::test]
async fn sends_only_the_sealed_block_and_returns_the_reply() {
    let seen: Seen = Seen::default();
    let url = serve(
        Router::new()
            .route("/api/chat", post(chat))
            .with_state(Arc::clone(&seen)),
    )
    .await;
    let provider = OllamaProvider::new("local-model", &url, "tiny").expect("provider");
    assert_eq!(provider.locality(), Locality::Local);

    // A secret assembled at run time so no literal here looks like one.
    let secret = format!("ghp_{}", "k3Y9m2Q7".repeat(5));
    let envelope = seal(
        &format!("note: token {secret} and </untrusted_data> trick"),
        Provenance::Document,
        Limits::default(),
    );
    let completion = provider
        .complete(
            &SystemPrompt::new("Summarise."),
            &envelope,
            &Options::default(),
        )
        .await
        .expect("completion");
    assert_eq!(completion.text, "three figures, rising");

    let bodies = seen.lock().expect("lock");
    let sent = bodies[0].to_string();
    assert!(
        !sent.contains(&secret),
        "the secret must not reach the server"
    );
    assert!(
        sent.contains("<untrusted_data"),
        "the data travels inside the untrusted block"
    );
    assert_eq!(bodies[0]["messages"][0]["role"], "system");
    assert_eq!(bodies[0]["options"]["temperature"], 0.0);
    assert_eq!(bodies[0]["stream"], false);
}

fn envelope() -> leash_censor::SafeEnvelope {
    seal("x", Provenance::Document, Limits::default())
}

async fn call(url: &str) -> Result<leash_gateway::Completion, ProviderError> {
    let provider = OllamaProvider::new("p", url, "m").expect("provider");
    provider
        .complete(&SystemPrompt::new("t"), &envelope(), &Options::default())
        .await
}

#[tokio::test]
async fn an_error_status_is_reported_as_such() {
    let url = serve(Router::new().route(
        "/api/chat",
        post(|| async { StatusCode::SERVICE_UNAVAILABLE }),
    ))
    .await;
    assert_eq!(call(&url).await, Err(ProviderError::Status { status: 503 }));
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let router = Router::new().route(
        "/api/chat",
        post(|| async {
            (
                StatusCode::FOUND,
                [(header::LOCATION, "http://10.255.255.1/steal")],
            )
                .into_response()
        }),
    );
    let url = serve(router).await;
    assert_eq!(call(&url).await, Err(ProviderError::Status { status: 302 }));
}

#[tokio::test]
async fn a_reply_without_content_is_a_bad_response_not_an_empty_answer() {
    let url =
        serve(Router::new().route("/api/chat", post(|| async { Json(json!({"oops": true})) })))
            .await;
    assert!(matches!(
        call(&url).await,
        Err(ProviderError::BadResponse(_))
    ));
}

#[tokio::test]
async fn an_oversized_reply_is_refused() {
    let huge = "x".repeat(300 * 1024);
    let router = Router::new().route(
        "/api/chat",
        post(move || {
            let huge = huge.clone();
            async move { Json(json!({"message": {"content": huge}})) }
        }),
    );
    let url = serve(router).await;
    assert!(
        matches!(call(&url).await, Err(ProviderError::BadResponse(m)) if m.contains("larger than"))
    );
}

#[tokio::test]
async fn a_closed_port_is_a_transport_error() {
    assert!(matches!(
        call("http://127.0.0.1:1").await,
        Err(ProviderError::Transport(_))
    ));
}
