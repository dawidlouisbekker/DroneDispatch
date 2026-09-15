//! map-service: the customer web app with the live drone map (see README.md).

use anyhow::Result;
use sqlx::PgPool;
use svc_common::{env, env_or};

#[derive(Clone)]
#[allow(dead_code)] // read by the web handlers once they land (milestone 7)
struct AppState {
    db: PgPool,
    nats: async_nats::Client,
}

#[tokio::main]
async fn main() -> Result<()> {
    svc_common::init_tracing();
    let db = svc_common::connect_db(&env("DATABASE_URL")?).await?;
    sqlx::migrate!().run(&db).await?;
    let state = AppState {
        db,
        nats: svc_common::connect_nats(&env_or("NATS_URL", svc_common::DEFAULT_NATS_URL), "map-service").await?,
    };

    // TODO(milestone 7): OAuth login, live map, order history, delivery
    // locations with MFA step-up, payment method and spending caps.
    let app = svc_common::health_routes().with_state(state);
    svc_common::serve(&env_or("HTTP_ADDR", "0.0.0.0:8085"), app).await
}
