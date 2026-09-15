//! dispatch-service: the MCP server for Alexa+ and the drone fleet
//! orchestrator (see README.md).

mod tools;

use std::sync::Arc;

use anyhow::Result;
use axum::{Json, extract::State, routing::get};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use svc_common::{env, env_or};

#[derive(Clone)]
struct AppState {
    #[allow(dead_code)] // missions table, used by the fleet orchestrator (milestone 6)
    db: PgPool,
    public_url: String,
    auth_issuer: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    svc_common::init_tracing();
    let nats = svc_common::connect_nats(&env_or("NATS_URL", svc_common::DEFAULT_NATS_URL), "dispatch-service").await?;
    let db = svc_common::connect_db(&env("DATABASE_URL")?).await?;
    sqlx::migrate!().run(&db).await?;
    let state = AppState {
        db,
        public_url: env_or("PUBLIC_URL", "http://localhost:8082"),
        auth_issuer: env_or("AUTH_ISSUER", "http://localhost:8081"),
    };
    // rmcp rejects unknown Host headers to prevent DNS rebinding; add the tunnel hostname here.
    let allowed_hosts: Vec<String> = env_or("MCP_ALLOWED_HOSTS", "localhost,127.0.0.1")
        .split(',')
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(str::to_owned)
        .collect();

    // TODO(milestone 5): require a valid JWT on /mcp, answering 401 with
    // svc_auth::challenge_unauthorized(<metadata URL>) when it is missing.
    let mcp = StreamableHttpService::new(
        move || Ok(tools::DroneDrop::new(nats.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().with_allowed_hosts(allowed_hosts),
    );

    // TODO(milestone 6): consume `dispatch.request` and publish `cmd.edge.<zone>.assign`.
    let app = svc_common::health_routes()
        .route("/.well-known/oauth-protected-resource/mcp", get(protected_resource_metadata))
        .with_state(state)
        .nest_service("/mcp", mcp);
    svc_common::serve(&env_or("HTTP_ADDR", "0.0.0.0:8082"), app).await
}

/// RFC 9728 metadata: tells Alexa which authorization server protects `/mcp`.
async fn protected_resource_metadata(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "resource": format!("{}/mcp", state.public_url),
        "authorization_servers": [state.auth_issuer],
        "scopes_supported": ["openid", "delivery"],
        "bearer_methods_supported": ["header"],
    }))
}
