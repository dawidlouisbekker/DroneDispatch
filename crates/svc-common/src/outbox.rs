//! Transactional outbox: an event is written in the same transaction as the
//! change it describes, and a relay publishes it to JetStream afterwards
//! (docs/DATABASE.md, "Outbox and inbox").

use std::time::Duration;

use async_nats::{HeaderMap, jetstream};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Queues `payload` for `subject`. `id` becomes the `Nats-Msg-Id`, so use the
/// event's own id (e.g. `event_id` in the protobuf message).
pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    subject: &str,
    payload: &[u8],
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO outbox (id, subject, payload) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(subject)
        .bind(payload)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Publishes queued events until the process exits, creating `streams` first if missing.
pub fn spawn_relay(pool: PgPool, nats: async_nats::Client, streams: Vec<jetstream::stream::Config>) {
    tokio::spawn(async move {
        let jetstream = jetstream::new(nats);
        for config in streams {
            let name = config.name.clone();
            if let Err(error) = jetstream.get_or_create_stream(config).await {
                tracing::warn!(%error, stream = %name, "creating stream failed; publishing will retry");
            }
        }
        loop {
            match relay_batch(&pool, &jetstream).await {
                Ok(0) => tokio::time::sleep(Duration::from_secs(1)).await,
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "outbox relay failed; retrying");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
}

async fn relay_batch(pool: &PgPool, jetstream: &jetstream::Context) -> anyhow::Result<usize> {
    let mut tx = pool.begin().await?;
    let rows: Vec<(Uuid, String, Vec<u8>)> = sqlx::query_as(
        "SELECT id, subject, payload FROM outbox WHERE published_at IS NULL
         ORDER BY created_at LIMIT 100 FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(&mut *tx)
    .await?;

    for (id, subject, payload) in &rows {
        let mut headers = HeaderMap::new();
        headers.insert("Nats-Msg-Id", id.to_string().as_str());
        jetstream.publish_with_headers(subject.clone(), headers, payload.clone().into()).await?.await?;
        sqlx::query("UPDATE outbox SET published_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(rows.len())
}
