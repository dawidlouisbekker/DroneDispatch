//! Plumbing shared by every Drone Drop service: environment config, tracing,
//! NATS and Postgres connections, an HTTP (and optional gRPC) server with
//! `/healthz` and graceful shutdown, and the transactional outbox.

pub mod outbox;

use std::path::Path;

use anyhow::{Context, Result};
use axum::{
    Router,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
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

/// Loads local `.env` files for `cargo run`: the service's own `.env`, then the
/// repository root's `.env` for shared settings. A file never overrides a
/// variable that is already set, so the real environment (Compose) wins, then
/// the service file, then the root file. Missing files are skipped.
///
/// Call it first in `main` with `env!("CARGO_MANIFEST_DIR")`, so it works from
/// any working directory.
pub fn load_env(service_dir: &str) -> Result<()> {
    let service_dir = Path::new(service_dir);
    for path in [service_dir.join(".env"), service_dir.join("../../.env")] {
        match dotenvy::from_path(&path) {
            Ok(()) => {}
            Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        }
    }
    Ok(())
}

/// Lets browsers on `CORS_ALLOWED_ORIGINS` (comma-separated, e.g. the Expo dev
/// server) call this API directly. Unset, the router is returned unchanged:
/// behind ui-gateway the app and the API share an origin.
pub fn cors_from_env(router: Router) -> Router {
    use axum::http::{HeaderValue, Method};
    use tower_http::cors::CorsLayer;

    let origins: Vec<HeaderValue> = env_or("CORS_ALLOWED_ORIGINS", "")
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .filter_map(|origin| origin.parse().ok())
        .collect();
    if origins.is_empty() {
        return router;
    }
    router.layer(
        CorsLayer::new()
            .allow_origin(origins)
            .allow_credentials(true)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
            .allow_headers([header::ACCEPT, header::AUTHORIZATION, header::CONTENT_TYPE]),
    )
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

/// An RFC 9457 `application/problem+json` response. `reason` carries the gRPC
/// `ErrorInfo` reason (e.g. `LOCATION_UNVERIFIED`) when there is one.
pub fn problem(status: StatusCode, title: &str, reason: Option<&str>) -> Response {
    let mut body = serde_json::json!({ "type": "about:blank", "title": title, "status": status.as_u16() });
    if let Some(reason) = reason {
        body["reason"] = reason.into();
    }
    (status, [(header::CONTENT_TYPE, "application/problem+json")], body.to_string()).into_response()
}

/// `501` for API operations whose backing gRPC call isn't wired up yet.
pub fn not_implemented() -> Response {
    problem(StatusCode::NOT_IMPLEMENTED, "Not implemented yet", None)
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

/// Serves `app` on `http_addr` and the gRPC `grpc` router on `grpc_addr` until Ctrl-C or SIGTERM.
pub async fn serve_with_grpc(
    http_addr: &str,
    app: Router,
    grpc_addr: &str,
    grpc: tonic::transport::server::Router,
) -> Result<()> {
    let grpc_socket: std::net::SocketAddr = grpc_addr.parse().with_context(|| format!("parsing {grpc_addr}"))?;
    let grpc_server = async move {
        tracing::info!(addr = %grpc_socket, "gRPC listening");
        grpc.serve_with_shutdown(grpc_socket, shutdown_signal()).await.context("gRPC server")
    };
    tokio::try_join!(serve(http_addr, app), grpc_server)?;
    Ok(())
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
