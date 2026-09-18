//! user-service: everything the customer owns — orders and their full history,
//! pickup locations and stations — behind the customer app's API and the Alexa+
//! MCP server (see README.md).

mod api;
mod live;
mod tools;

use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json,
    extract::{FromRef, State},
    routing::get,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use svc_auth::{AuthConfig, TokenVerifier};
use svc_common::{env, env_or};

#[derive(Clone)]
struct AppState {
    #[allow(dead_code)] // orders, pickup locations and stations (milestone 4)
    db: PgPool,
    #[allow(dead_code)] // fulfilment and mission events, dispatch requests (milestone 4)
    nats: async_nats::Client,
    auth: AuthConfig,
    zones: api::ZoneConfig,
    /// The Alexa+ MCP resource, `{MCP_PUBLIC_URL}/mcp`.
    mcp_resource: String,
}

impl FromRef<AppState> for AuthConfig {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    svc_common::load_env(env!("CARGO_MANIFEST_DIR"))?;
    svc_common::init_tracing();
    let db = svc_common::connect_db(&env("DATABASE_URL")?).await?;
    sqlx::migrate!().run(&db).await?;
    let nats = svc_common::connect_nats(&env_or("NATS_URL", svc_common::DEFAULT_NATS_URL), "user-service").await?;

    let issuer = env_or("AUTH_ISSUER", "http://localhost:8081");
    // The customer API's public base URL is also its token audience.
    let api_url = env_or("API_PUBLIC_URL", "http://localhost:8086/api/user");
    let mcp_public_url = env_or("MCP_PUBLIC_URL", "http://localhost:8085");
    let state = AppState {
        db,
        nats: nats.clone(),
        auth: auth_config(&issuer, &api_url),
        zones: api::ZoneConfig::seattle()?,
        mcp_resource: format!("{}/mcp", mcp_public_url.trim_end_matches('/')),
    };

    // rmcp rejects unknown Host headers to prevent DNS rebinding; add the tunnel hostname here.
    let allowed_hosts: Vec<String> = env_or("MCP_ALLOWED_HOSTS", "localhost,127.0.0.1")
        .split(',')
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(str::to_owned)
        .collect();
    // TODO(milestone 5): require a valid JWT for the MCP resource on /mcp, answering 401 with
    // svc_auth::challenge_unauthorized(<metadata URL>) when it is missing.
    let mcp = StreamableHttpService::new(
        move || Ok(tools::DroneDrop::new(nats.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().with_allowed_hosts(allowed_hosts),
    );

    let app = svc_common::health_routes()
        .merge(api::routes())
        .route("/.well-known/oauth-protected-resource/mcp", get(mcp_resource_metadata))
        .with_state(state)
        .nest_service("/mcp", mcp);
    svc_common::serve(&env_or("HTTP_ADDR", "0.0.0.0:8085"), svc_common::cors_from_env(app)).await
}

fn auth_config(issuer: &str, api_url: &str) -> AuthConfig {
    let verifier = match std::env::var("AUTH_JWKS_URL") {
        Ok(jwks_url) => TokenVerifier::with_jwks_url(issuer, api_url, jwks_url),
        Err(_) => TokenVerifier::new(issuer, api_url),
    };
    AuthConfig { verifier, resource_metadata_url: format!("{api_url}/.well-known/oauth-protected-resource") }
}

/// RFC 9728 metadata: tells Alexa which authorization server protects `/mcp`.
async fn mcp_resource_metadata(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "resource": state.mcp_resource,
        "authorization_servers": [state.auth.verifier.issuer()],
        "scopes_supported": ["openid", "delivery"],
        "bearer_methods_supported": ["header"],
    }))
}
