//! dispatch-service: the drone fleet of one MEC edge zone. It turns dispatch
//! requests into missions, flies the (simulated) drones and authenticates the
//! stations at pickup and drop-off (see README.md).

use anyhow::Result;
use futures::StreamExt;
use svc_common::{env, env_or};

#[tokio::main]
async fn main() -> Result<()> {
    svc_common::load_env(env!("CARGO_MANIFEST_DIR"))?;
    svc_common::init_tracing();
    let zone = env("EDGE_ZONE")?;
    let db = svc_common::connect_db(&env("DATABASE_URL")?).await?;
    sqlx::migrate!().run(&db).await?;
    // The zone's own NATS leaf, so the fleet keeps flying when its uplink is cut.
    let nats = svc_common::connect_nats(
        &env_or("NATS_URL", svc_common::DEFAULT_NATS_URL),
        &format!("dispatch-{zone}"),
    )
    .await?;

    // TODO(milestone 6): replace with a durable JetStream consumer on DISPATCH_REQUESTS, so
    // requests queued on the hub during a partition are delivered; create missions and feed them
    // to the flight simulator and the station handshake (crates/station).
    let mut commands = nats.subscribe(contracts::subjects::dispatch_commands(&zone)).await?;
    tracing::info!(%zone, "waiting for dispatch commands");
    tokio::spawn(async move {
        while let Some(message) = commands.next().await {
            tracing::info!(subject = %message.subject, "dispatch command received");
        }
    });

    svc_common::serve(&env_or("HTTP_ADDR", "0.0.0.0:8082"), svc_common::health_routes()).await
}
