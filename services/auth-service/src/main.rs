//! auth-service binary: see lib.rs and README.md.

use anyhow::Result;
use async_nats::jetstream::stream;
use auth_service::{AppState, config::Config, grpc, registry};
use contracts::subjects;

#[tokio::main]
async fn main() -> Result<()> {
    svc_common::load_env(env!("CARGO_MANIFEST_DIR"))?;
    svc_common::init_tracing();
    let config = Config::from_env()?;

    let db = svc_common::connect_db(&config.database_url).await?;
    sqlx::migrate!().run(&db).await?;
    registry::seed_clients(&db, &config).await?;

    let nats = svc_common::connect_nats(&config.nats_url, "auth-service").await?;
    let auth_events = stream::Config {
        name: subjects::STREAM_AUTH_EVENTS.to_owned(),
        subjects: vec!["auth.events.>".to_owned()],
        ..Default::default()
    };
    svc_common::outbox::spawn_relay(db.clone(), nats, vec![auth_events]);

    let (http_addr, grpc_addr) = (config.http_addr.clone(), config.grpc_addr.clone());
    let app = auth_service::router(AppState::new(db.clone(), config)?);
    svc_common::serve_with_grpc(&http_addr, app, &grpc_addr, grpc::routes(db)).await
}
