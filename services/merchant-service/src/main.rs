//! merchant-service: businesses, menus, stock, pickup markers and the
//! merchant portal (see README.md).

use anyhow::Result;
use sqlx::PgPool;
use svc_common::{env, env_or};

#[derive(Clone)]
#[allow(dead_code)] // read by the portal and NATS handlers once they land (milestone 3)
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
        nats: svc_common::connect_nats(&env_or("NATS_URL", svc_common::DEFAULT_NATS_URL), "merchant-service").await?,
    };

    // TODO(milestone 3): serve merchant.shops.*, merchant.menu.get and
    // merchant.stock.* over request-reply; portal pages; pickup marker verification.
    let app = svc_common::health_routes().with_state(state);
    svc_common::serve(&env_or("HTTP_ADDR", "0.0.0.0:8083"), app).await
}
