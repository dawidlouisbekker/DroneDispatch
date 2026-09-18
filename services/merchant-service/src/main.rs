//! merchant-service: businesses, menus, stock, pickup markers, and the API
//! behind ui/app's merchant portal (see README.md).

mod api;
mod live;

use anyhow::Result;
use axum::extract::FromRef;
use sqlx::PgPool;
use svc_auth::{AuthConfig, TokenVerifier};
use svc_common::{env, env_or};

#[derive(Clone)]
struct AppState {
    #[allow(dead_code)] // read by the API handlers once they land (milestone 3)
    db: PgPool,
    #[allow(dead_code)] // catalog state and fulfilment events (milestone 3)
    nats: async_nats::Client,
    auth: AuthConfig,
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

    // The API's public base URL is also its token audience.
    let api_url = env_or("API_PUBLIC_URL", "http://localhost:8086/api/merchant");
    let issuer = env_or("AUTH_ISSUER", "http://localhost:8081");
    let verifier = match std::env::var("AUTH_JWKS_URL") {
        Ok(jwks_url) => TokenVerifier::with_jwks_url(issuer, api_url.clone(), jwks_url),
        Err(_) => TokenVerifier::new(issuer, api_url.clone()),
    };
    let state = AppState {
        db,
        nats: svc_common::connect_nats(
            &env_or("NATS_URL", svc_common::DEFAULT_NATS_URL),
            "merchant-service",
        )
        .await?,
        auth: AuthConfig {
            verifier,
            resource_metadata_url: format!("{api_url}/.well-known/oauth-protected-resource"),
        },
    };

    // TODO(milestone 3): station registration, the Fulfilment gRPC server, catalog state events
    // through the outbox, portal notifications.
    let app = svc_common::cors_from_env(
        svc_common::health_routes()
            .merge(api::routes())
            .with_state(state),
    );
    svc_common::serve(&env_or("HTTP_ADDR", "0.0.0.0:8083"), app).await
}
