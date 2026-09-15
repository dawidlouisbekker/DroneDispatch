//! Plumbing shared by every Drone Drop service: environment config, tracing,
//! NATS and Postgres connections, and an HTTP server with `/healthz` and
//! graceful shutdown.

use anyhow::{Context, Result};
use axum::{Router, routing::get};
use sqlx::postgres::{PgPool, PgPoolOptions};

pub const DEFAULT_NATS_URL: &str = "nats://localhost:4222";

/// Reads a required environment variable.
pub fn env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing environment variable {key}"))
}

/// Reads an environment variable, falling back to `default` when it is unset.
pub fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

/// Logs to stdout. Verbosity comes from `RUST_LOG` (e.g. `debug`,
/// `info,async_nats=warn`) and defaults to `info`.
pub fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into());
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

/// Connects to NATS as `name`. The client keeps retrying in the background
/// until the server is reachable, so start-up order in Compose doesn't matter.
pub async fn connect_nats(url: &str, name: &str) -> Result<async_nats::Client> {
    async_nats::ConnectOptions::new()
        .name(name)
        .retry_on_initial_connect()
        .connect(url)
        .await
        .with_context(|| format!("connecting to NATS at {url}"))
}

/// Opens a connection pool to the service's own Postgres database.
pub async fn connect_db(url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(url)
        .await
        .context("connecting to Postgres")
}

/// Base router every service extends: `GET /healthz`.
pub fn health_routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route("/healthz", get(|| async { "ok" }))
}

/// Serves `app` on `addr` until Ctrl-C or SIGTERM.
pub async fn serve(addr: &str, app: Router) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server")
}

/// Resolves on Ctrl-C or SIGTERM (what `docker compose stop` sends).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("shutting down");
}
