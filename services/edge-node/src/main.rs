//! edge-node: drone flight simulation and pickup marker vision for one MEC
//! edge zone (see README.md).

use anyhow::Result;
use futures::StreamExt;
use svc_common::{env, env_or};

#[tokio::main]
async fn main() -> Result<()> {
    svc_common::init_tracing();
    let zone = env("EDGE_ZONE")?;
    // The zone's own NATS leaf, so the node keeps working when its uplink is cut.
    let nats = svc_common::connect_nats(
        &env_or("NATS_URL", svc_common::DEFAULT_NATS_URL),
        &format!("edge-node-{zone}"),
    )
    .await?;

    // TODO(milestone 6): replace with a JetStream consumer, so commands queued on
    // the hub during a partition are delivered, and feed missions to the 10 Hz simulator.
    let mut commands = nats.subscribe(contracts::subjects::edge_commands(&zone)).await?;
    tracing::info!(%zone, "waiting for edge commands");
    tokio::spawn(async move {
        while let Some(message) = commands.next().await {
            tracing::info!(subject = %message.subject, "edge command received");
        }
    });

    svc_common::serve(&env_or("HTTP_ADDR", "0.0.0.0:8090"), svc_common::health_routes()).await
}
