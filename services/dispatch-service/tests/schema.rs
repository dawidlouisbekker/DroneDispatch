//! Schema tests for the `dispatch_<zone>` databases. They need Postgres:
//! `scripts/test-db.sh -p dispatch-service`.

use sqlx::PgPool;
use sqlx::postgres::PgQueryResult;
use uuid::Uuid;

const UNIQUE: &str = "23505";
const CHECK: &str = "23514";
const FOREIGN_KEY: &str = "23503";

/// One Bluetooth LE access network, as copied from a dispatch request.
const NETWORKS: &str = r#"[{"kind": "BLUETOOTH_LE", "service_uuid": "6e400001-b5a3-f393-e0a9-e50e24dcca9e", "station_tag": "0a0b0c0d", "l2cap_psm": 128}]"#;

#[track_caller]
fn assert_sqlstate<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, code: &str) {
    let err = result.expect_err("expected a database error");
    assert_eq!(err.as_database_error().expect("database error").code().as_deref(), Some(code), "{err}");
}

async fn insert_dock(pool: &PgPool) {
    sqlx::query("INSERT INTO docks (id, lat, lon, capacity) VALUES ('hub-fremont', 47.651, -122.35, 4)")
        .execute(pool)
        .await
        .expect("insert dock");
}

async fn insert_drone(pool: &PgPool, id: &str, certificate_sha256: &str) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO drones (id, dock_id, model, max_payload_g, certificate_sha256) VALUES ($1, 'hub-fremont', 'sim-quad', 2500, $2)",
    )
    .bind(id)
    .bind(certificate_sha256)
    .execute(pool)
    .await
}

async fn insert_mission(
    pool: &PgPool,
    order_id: Uuid,
    request_id: Uuid,
    payload_g: i32,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO missions (order_id, dispatch_request_id, payload_g,
             pickup_station_id, pickup_lat, pickup_lon, pickup_accuracy_m, pickup_public_key_sha256, pickup_access_networks,
             dropoff_station_id, dropoff_lat, dropoff_lon, dropoff_accuracy_m, dropoff_public_key_sha256, dropoff_access_networks)
         VALUES ($1, $2, $3,
                 $4, 47.6097, -122.3422, 1.5, repeat('ab', 32), $6::jsonb,
                 $5, 47.6205, -122.3493, 4.9, repeat('cd', 32), $6::jsonb)",
    )
    .bind(order_id)
    .bind(request_id)
    .bind(payload_g)
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(NETWORKS)
    .execute(pool)
    .await
}

async fn new_mission(pool: &PgPool) -> Uuid {
    let order_id = Uuid::now_v7();
    insert_mission(pool, order_id, Uuid::now_v7(), 1200).await.expect("insert mission");
    order_id
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn one_mission_per_order(pool: PgPool) {
    let order_id = Uuid::now_v7();
    let request_id = Uuid::now_v7();
    insert_mission(&pool, order_id, request_id, 1200).await.expect("first mission");

    // Same order, different request.
    assert_sqlstate(insert_mission(&pool, order_id, Uuid::now_v7(), 1200).await, UNIQUE);
    // Redelivered dispatch request for a different order id.
    assert_sqlstate(insert_mission(&pool, Uuid::now_v7(), request_id, 1200).await, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn payload_is_limited_to_2500_g(pool: PgPool) {
    insert_mission(&pool, Uuid::now_v7(), Uuid::now_v7(), 2500).await.expect("2500 g is allowed");
    assert_sqlstate(insert_mission(&pool, Uuid::now_v7(), Uuid::now_v7(), 2501).await, CHECK);
    assert_sqlstate(insert_mission(&pool, Uuid::now_v7(), Uuid::now_v7(), 0).await, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn flying_mission_needs_a_known_drone(pool: PgPool) {
    insert_dock(&pool).await;
    insert_drone(&pool, "drone-07", &"ef".repeat(32)).await.expect("drone");
    let order_id = new_mission(&pool).await;
    let assign = |state: &'static str, drone: Option<&'static str>| {
        sqlx::query(
            "UPDATE missions SET state = $2, drone_id = $3, version = version + 1, updated_at = now() WHERE order_id = $1",
        )
        .bind(order_id)
        .bind(state)
        .bind(drone)
        .execute(&pool)
    };

    assert_sqlstate(assign("ASSIGNED", None).await, CHECK);
    assert_sqlstate(assign("AT_DROPOFF", None).await, CHECK);
    assert_sqlstate(assign("LOST", None).await, CHECK);
    assert_sqlstate(assign("ASSIGNED", Some("drone-99")).await, FOREIGN_KEY);
    assign("NO_DRONE", None).await.expect("NO_DRONE needs no drone");
    assign("ASSIGNED", Some("drone-07")).await.expect("assigned with a drone");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn legs_need_access_networks_and_key_hashes(pool: PgPool) {
    let order_id = new_mission(&pool).await;
    let update = |sql: &'static str| sqlx::query(sql).bind(order_id).execute(&pool);

    assert_sqlstate(update("UPDATE missions SET pickup_access_networks = '[]' WHERE order_id = $1").await, CHECK);
    assert_sqlstate(update("UPDATE missions SET dropoff_access_networks = '{}' WHERE order_id = $1").await, CHECK);
    assert_sqlstate(update("UPDATE missions SET pickup_public_key_sha256 = 'not-a-hash' WHERE order_id = $1").await, CHECK);
    assert_sqlstate(update("UPDATE missions SET dropoff_accuracy_m = 0 WHERE order_id = $1").await, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn handshakes_are_checked_and_follow_their_mission(pool: PgPool) {
    insert_dock(&pool).await;
    insert_drone(&pool, "drone-07", &"ef".repeat(32)).await.expect("drone");
    let order_id = new_mission(&pool).await;
    let handshake = |leg: &'static str, network: &'static str, result: &'static str| {
        sqlx::query(
            "INSERT INTO station_handshakes (id, order_id, drone_id, leg, station_id, access_network, ranging_method,
                                             distance_m, result, attempted_at)
             VALUES ($1, $2, 'drone-07', $3, $4, $5, 'RSSI', 3.2, $6, now())",
        )
        .bind(Uuid::now_v7())
        .bind(order_id)
        .bind(leg)
        .bind(Uuid::now_v7())
        .bind(network)
        .bind(result)
        .execute(&pool)
    };

    handshake("PICKUP", "BLUETOOTH_LE", "KEY_MISMATCH").await.expect("failed attempt");
    handshake("PICKUP", "BLUETOOTH_LE", "VERIFIED").await.expect("verified attempt");
    assert_sqlstate(handshake("MIDAIR", "BLUETOOTH_LE", "VERIFIED").await, CHECK);
    assert_sqlstate(handshake("DROPOFF", "SMOKE_SIGNAL", "VERIFIED").await, CHECK);
    assert_sqlstate(handshake("DROPOFF", "BLUETOOTH_LE", "LUCKY").await, CHECK);

    // A drone with recorded handshakes can't be deleted; a mission's handshakes go with it.
    assert_sqlstate(sqlx::query("DELETE FROM drones WHERE id = 'drone-07'").execute(&pool).await, FOREIGN_KEY);
    sqlx::query("DELETE FROM missions WHERE order_id = $1").bind(order_id).execute(&pool).await.expect("delete mission");
    let (handshakes,): (i64,) = sqlx::query_as("SELECT count(*) FROM station_handshakes").fetch_one(&pool).await.unwrap();
    assert_eq!(handshakes, 0);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn drone_certificates_are_unique_hex_hashes(pool: PgPool) {
    insert_dock(&pool).await;
    assert_sqlstate(insert_drone(&pool, "drone-01", "xyz").await, CHECK);
    insert_drone(&pool, "drone-01", &"ef".repeat(32)).await.expect("drone");
    assert_sqlstate(insert_drone(&pool, "drone-02", &"ef".repeat(32)).await, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn inbox_detects_duplicate_messages(pool: PgPool) {
    let insert = || {
        sqlx::query("INSERT INTO inbox (consumer, message_id) VALUES ('dispatch-requests', 'msg-1') ON CONFLICT DO NOTHING")
            .execute(&pool)
    };
    assert_eq!(insert().await.expect("first").rows_affected(), 1);
    assert_eq!(insert().await.expect("duplicate").rows_affected(), 0);
}
