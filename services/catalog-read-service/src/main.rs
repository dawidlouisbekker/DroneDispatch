//! catalog-read-service: the read-only side of the catalog (CQRS) for one edge
//! zone. It projects merchant-service's state events into its own database and
//! serves catalogs keyed by Amazon Location place ID (see README.md).

mod api;

use anyhow::Result;
use sqlx::PgPool;
use svc_common::{env, env_or};

#[derive(Clone)]
#[allow(dead_code)] // read by the query handlers and the projection consumer (milestone 3)
struct AppState {
    db: PgPool,
    zone: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    svc_common::load_env(env!("CARGO_MANIFEST_DIR"))?;
    svc_common::init_tracing();
    let zone = env("EDGE_ZONE")?;
    let db = svc_common::connect_db(&env("DATABASE_URL")?).await?;
    sqlx::migrate!().run(&db).await?;
    // The zone's own NATS leaf, so reads keep being served while the uplink is cut.
    let _nats = svc_common::connect_nats(
        &env_or("NATS_URL", svc_common::DEFAULT_NATS_URL),
        &format!("catalog-read-{zone}"),
    )
    .await?;

    // TODO(milestone 3): a durable JetStream consumer on MERCHANT_EVENTS (`merchant.state.>`)
    // that projects state events (docs/DATABASE.md, "CQRS"), and the CatalogRead gRPC server.
    tracing::info!(%zone, "serving catalog reads");
    let app = svc_common::health_routes().merge(api::routes()).with_state(AppState { db, zone });
    svc_common::serve(&env_or("HTTP_ADDR", "0.0.0.0:8084"), svc_common::cors_from_env(app)).await
}
