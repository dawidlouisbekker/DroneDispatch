//! Schema tests for the `dispatch` database. They need Postgres:
//!
//! ```bash
//! docker compose up -d --wait postgres
//! DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p dispatch-service -- --ignored
//! ```

use sqlx::PgPool;
use sqlx::postgres::PgQueryResult;
use uuid::Uuid;

const EVENT_KINDS: [&str; 8] = [
    "ASSIGNED",
    "AT_PICKUP",
    "VISUAL_LOCK",
    "PICKUP_FAILED",
    "PICKED_UP",
    "DELIVERED",
    "ABORTED",
    "NO_DRONE",
];

async fn insert_mission(
    pool: &PgPool,
    order_id: Uuid,
    request_id: Uuid,
    payload_g: i32,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO missions (order_id, dispatch_request_id, pickup_lat, pickup_lon, pickup_point_id,
                               asset_key, dropoff_lat, dropoff_lon, payload_g)
         VALUES ($1, $2, 47.6097, -122.3331, $3, 'pickup-points/marker.jpg', 47.6205, -122.3493, $4)",
    )
    .bind(order_id)
    .bind(request_id)
    .bind(Uuid::now_v7())
    .bind(payload_g)
    .execute(pool)
    .await
}

async fn new_mission(pool: &PgPool) -> Uuid {
    let order_id = Uuid::now_v7();
    insert_mission(pool, order_id, Uuid::now_v7(), 1200).await.expect("insert mission");
    order_id
}

async fn insert_event(
    pool: &PgPool,
    event_id: Uuid,
    order_id: Uuid,
    kind: &str,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO mission_events (event_id, order_id, kind, zone, drone_id, lat, lon, occurred_at)
         VALUES ($1, $2, $3, 'sea-north', 'drone-07', 47.61, -122.34, now())",
    )
    .bind(event_id)
    .bind(order_id)
    .bind(kind)
    .execute(pool)
    .await
}

#[track_caller]
fn assert_sqlstate<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, code: &str) {
    let err = result.expect_err("expected a database error");
    assert_eq!(err.as_database_error().expect("database error").code().as_deref(), Some(code), "{err}");
}

/// (attempts, events) stored for an order.
async fn child_rows(pool: &PgPool, order_id: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM mission_attempts WHERE order_id = $1),
                (SELECT count(*) FROM mission_events WHERE order_id = $1)",
    )
    .bind(order_id)
    .fetch_one(pool)
    .await
    .expect("count child rows")
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn one_mission_per_order(pool: PgPool) {
    let order_id = Uuid::now_v7();
    let request_id = Uuid::now_v7();
    insert_mission(&pool, order_id, request_id, 1200).await.expect("first mission");

    // Same order, different request.
    assert_sqlstate(insert_mission(&pool, order_id, Uuid::now_v7(), 1200).await, "23505");
    // Redelivered dispatch.request for a different order id.
    assert_sqlstate(insert_mission(&pool, Uuid::now_v7(), request_id, 1200).await, "23505");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn payload_is_limited_to_2500_g(pool: PgPool) {
    insert_mission(&pool, Uuid::now_v7(), Uuid::now_v7(), 2500).await.expect("2500 g is allowed");
    assert_sqlstate(insert_mission(&pool, Uuid::now_v7(), Uuid::now_v7(), 2501).await, "23514");
    assert_sqlstate(insert_mission(&pool, Uuid::now_v7(), Uuid::now_v7(), 0).await, "23514");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn flying_mission_needs_zone_and_drone(pool: PgPool) {
    let order_id = new_mission(&pool).await;
    let set_state = |state: &'static str, zone: Option<&'static str>, drone: Option<&'static str>| {
        sqlx::query(
            "UPDATE missions SET state = $2, zone = $3, drone_id = $4, version = version + 1, updated_at = now()
             WHERE order_id = $1",
        )
        .bind(order_id)
        .bind(state)
        .bind(zone)
        .bind(drone)
        .execute(&pool)
    };

    assert_sqlstate(set_state("ASSIGNED", None, None).await, "23514");
    assert_sqlstate(set_state("ASSIGNED", Some("sea-north"), None).await, "23514");
    assert_sqlstate(set_state("DELIVERED", None, Some("drone-07")).await, "23514");
    assert_sqlstate(set_state("LOST", None, None).await, "23514");
    set_state("NO_DRONE", None, None).await.expect("NO_DRONE needs no drone");
    set_state("ASSIGNED", Some("sea-north"), Some("drone-07")).await.expect("assigned with a drone");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn every_event_kind_is_accepted(pool: PgPool) {
    let order_id = new_mission(&pool).await;
    for kind in EVENT_KINDS {
        insert_event(&pool, Uuid::now_v7(), order_id, kind).await.unwrap_or_else(|e| panic!("{kind}: {e}"));
    }
    assert_eq!(child_rows(&pool, order_id).await.1, EVENT_KINDS.len() as i64);

    assert_sqlstate(insert_event(&pool, Uuid::now_v7(), order_id, "TELEPORTED").await, "23514");
    assert_sqlstate(insert_event(&pool, Uuid::now_v7(), order_id, "MISSION_EVENT_KIND_ASSIGNED").await, "23514");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn replayed_event_is_rejected(pool: PgPool) {
    let order_id = new_mission(&pool).await;
    let event_id = Uuid::now_v7();
    insert_event(&pool, event_id, order_id, "AT_PICKUP").await.expect("first delivery");
    assert_sqlstate(insert_event(&pool, event_id, order_id, "AT_PICKUP").await, "23505");

    // Events must belong to a known mission.
    assert_sqlstate(insert_event(&pool, Uuid::now_v7(), Uuid::now_v7(), "ASSIGNED").await, "23503");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn attempt_numbers_are_unique_per_order(pool: PgPool) {
    let order_id = new_mission(&pool).await;
    let attempt = |n: i32, result: &'static str| {
        sqlx::query("INSERT INTO mission_attempts (id, order_id, attempt, zone, result) VALUES ($1, $2, $3, 'sea-north', $4)")
            .bind(Uuid::now_v7())
            .bind(order_id)
            .bind(n)
            .bind(result)
            .execute(&pool)
    };

    attempt(1, "NO_FREE_DRONE").await.expect("attempt 1");
    attempt(2, "ASSIGNED").await.expect("attempt 2");
    assert_sqlstate(attempt(2, "TIMED_OUT").await, "23505");
    assert_sqlstate(attempt(0, "TIMED_OUT").await, "23514");
    assert_sqlstate(attempt(3, "BUSY").await, "23514");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn deleting_a_mission_cascades(pool: PgPool) {
    let order_id = new_mission(&pool).await;
    let other_order_id = new_mission(&pool).await;
    for id in [order_id, other_order_id] {
        sqlx::query("INSERT INTO mission_attempts (id, order_id, attempt, zone, result) VALUES ($1, $2, 1, 'sea-north', 'ASSIGNED')")
            .bind(Uuid::now_v7())
            .bind(id)
            .execute(&pool)
            .await
            .expect("attempt");
        insert_event(&pool, Uuid::now_v7(), id, "ASSIGNED").await.expect("event");
    }

    sqlx::query("DELETE FROM missions WHERE order_id = $1").bind(order_id).execute(&pool).await.expect("delete");

    assert_eq!(child_rows(&pool, order_id).await, (0, 0));
    assert_eq!(child_rows(&pool, other_order_id).await, (1, 1));
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn inbox_detects_duplicate_messages(pool: PgPool) {
    let insert = || {
        sqlx::query(
            "INSERT INTO inbox (consumer, message_id) VALUES ('dispatch-requests', 'msg-1') ON CONFLICT DO NOTHING",
        )
        .execute(&pool)
    };
    assert_eq!(insert().await.expect("first").rows_affected(), 1);
    assert_eq!(insert().await.expect("duplicate").rows_affected(), 0);
}
