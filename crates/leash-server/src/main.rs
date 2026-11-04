use std::process::ExitCode;

use leash_server::app::app;
use leash_server::config::Config;
use leash_server::healthcheck;
use leash_server::state::AppState;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .compact()
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(%error, "invalid configuration");
            return ExitCode::from(2);
        }
    };

    if std::env::args().any(|arg| arg == "--healthcheck") {
        return if healthcheck::healthy(config.addr).await {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    let addr = config.addr;
    let lab = config.lab;
    let state = match AppState::build(config).await {
        Ok(state) => std::sync::Arc::new(state),
        Err(error) => {
            tracing::error!(%error, "could not start");
            return ExitCode::FAILURE;
        }
    };
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, %addr, "could not bind");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(%addr, lab, "listening");
    if lab {
        tracing::warn!(
            "lab mode is on: endpoints that attack the system on purpose are open, with no authentication"
        );
    }

    let served = axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown())
        .await;
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "server stopped");
            ExitCode::FAILURE
        }
    }
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { () = ctrl_c => {}, () = terminate => {} }
    tracing::info!("shutting down");
}
