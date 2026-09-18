//! Schema tests for the `user_service` database. They need Postgres:
//! `scripts/test-db.sh -p user-service`.

use sqlx::PgPool;
use uuid::Uuid;

const UNIQUE: &str = "23505";
const CHECK: &str = "23514";
const FOREIGN_KEY: &str = "23503";

#[track_caller]
fn assert_sqlstate<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, code: &str) {
    let err = result.expect_err("expected a database error");
    assert_eq!(err.as_database_error().expect("database error").code().as_deref(), Some(code), "{err}");
}

fn key_hash(byte: &str) -> String {
    byte.repeat(32)
}

async fn insert_customer(pool: &PgPool) -> Uuid {
    let sub = Uuid::now_v7();
    sqlx::query("INSERT INTO customers (sub) VALUES ($1)").bind(sub).execute(pool).await.expect("insert customer");
    sub
}

async fn insert_location(pool: &PgPool, sub: Uuid, label: &str) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO pickup_locations (id, customer_sub, label, address, lat, lon, position_accuracy_m, position_source)
         VALUES ($1, $2, $3, '400 Broad St, Seattle', 47.6205, -122.3493, 4.9, 'DEVICE_GPS')",
    )
    .bind(id)
    .bind(sub)
    .bind(label)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn insert_quote(pool: &PgPool, sub: Uuid, location_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO quotes (id, customer_sub, channel, place_id, business_id, business_spoken_name, pickup_location_id,
                             subtotal_cents, delivery_fee_cents, total_cents, payload_g, eta_seconds, expires_at)
         VALUES ($1, $2, 'MCP', 'place-1', $3, 'Joe''s Coffee', $4, 900, 300, 1200, 800, 900, now() + interval '10 minutes')",
    )
    .bind(id)
    .bind(sub)
    .bind(Uuid::now_v7())
    .bind(location_id)
    .execute(pool)
    .await
    .expect("insert quote");
    id
}

async fn insert_order(
    pool: &PgPool,
    sub: Uuid,
    quote_id: Uuid,
    location_id: Uuid,
    total_cents: i64,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO orders (id, customer_sub, quote_id, channel, place_id, business_id, business_spoken_name, state,
                             pickup_location_id, dropoff_label, dropoff_address, dropoff_lat, dropoff_lon,
                             subtotal_cents, delivery_fee_cents, total_cents, payload_g)
         VALUES ($1, $2, $3, 'MCP', 'place-1', $4, 'Joe''s Coffee', 'AUTHORIZING',
                 $5, 'home', '400 Broad St, Seattle', 47.6205, -122.3493, 900, 300, $6, 800)",
    )
    .bind(id)
    .bind(sub)
    .bind(quote_id)
    .bind(Uuid::now_v7())
    .bind(location_id)
    .bind(total_cents)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn insert_station(
    pool: &PgPool,
    sub: Uuid,
    location_id: Uuid,
    public_key_sha256: &str,
    status: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO stations (id, customer_sub, pickup_location_id, public_key, public_key_sha256, status, activated_at)
         VALUES ($1, $2, $3, '\\x3059', $4, $5, CASE WHEN $5 = 'ACTIVE' THEN now() END)",
    )
    .bind(id)
    .bind(sub)
    .bind(location_id)
    .bind(public_key_sha256)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(id)
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn verified_location_needs_passkey_evidence(pool: PgPool) {
    let sub = insert_customer(&pool).await;
    let location_id = insert_location(&pool, sub, "home").await.expect("location");
    let verify = |amr: Option<Vec<String>>| {
        sqlx::query(
            "UPDATE pickup_locations SET status = 'VERIFIED', verified_at = now(), verified_amr = $2, updated_at = now()
             WHERE id = $1",
        )
        .bind(location_id)
        .bind(amr)
        .execute(&pool)
    };

    assert_sqlstate(verify(None).await, CHECK);
    assert_sqlstate(verify(Some(vec!["pwd".into()])).await, CHECK);
    assert_sqlstate(verify(Some(vec!["pwd".into(), "otp".into()])).await, CHECK);
    verify(Some(vec!["pwd".into(), "hwk".into()])).await.expect("passkey evidence");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn labels_are_unique_per_customer_ignoring_case(pool: PgPool) {
    let sub = insert_customer(&pool).await;
    insert_location(&pool, sub, "Home").await.expect("first");
    assert_sqlstate(insert_location(&pool, sub, "home").await, UNIQUE);

    let other = insert_customer(&pool).await;
    insert_location(&pool, other, "home").await.expect("another customer's label");

    sqlx::query("UPDATE pickup_locations SET status = 'REVOKED', updated_at = now() WHERE customer_sub = $1")
        .bind(sub)
        .execute(&pool)
        .await
        .expect("revoke");
    insert_location(&pool, sub, "home").await.expect("label reused after revoking");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn orders_check_totals_and_only_a_webhook_marks_paid(pool: PgPool) {
    let sub = insert_customer(&pool).await;
    let location_id = insert_location(&pool, sub, "home").await.expect("location");
    let quote_id = insert_quote(&pool, sub, location_id).await;

    assert_sqlstate(insert_order(&pool, sub, quote_id, location_id, 999).await, CHECK);
    let order_id = insert_order(&pool, sub, quote_id, location_id, 1200).await.expect("order");
    // Placing is idempotent on the quote.
    assert_sqlstate(insert_order(&pool, sub, quote_id, location_id, 1200).await, UNIQUE);

    let to_paid = |cause: &'static str, event_id: Option<&'static str>| {
        sqlx::query(
            "INSERT INTO order_state_transitions (id, order_id, from_state, to_state, cause, payment_event_id)
             VALUES ($1, $2, 'CAPTURING', 'PAID', $3, $4)",
        )
        .bind(Uuid::now_v7())
        .bind(order_id)
        .bind(cause)
        .bind(event_id)
        .execute(&pool)
    };
    assert_sqlstate(to_paid("APP", None).await, CHECK);
    assert_sqlstate(to_paid("PAYMENT_WEBHOOK", None).await, CHECK);
    assert_sqlstate(to_paid("PAYMENT_WEBHOOK", Some("evt_missing")).await, FOREIGN_KEY);

    sqlx::query(
        "INSERT INTO payment_events (id, provider, type, payload) VALUES ('evt_1', 'STRIPE', 'payment_intent.succeeded', '{}')",
    )
    .execute(&pool)
    .await
    .expect("payment event");
    to_paid("PAYMENT_WEBHOOK", Some("evt_1")).await.expect("paid by webhook");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn approvals_need_a_passkey(pool: PgPool) {
    let sub = insert_customer(&pool).await;
    let location_id = insert_location(&pool, sub, "home").await.expect("location");
    let quote_id = insert_quote(&pool, sub, location_id).await;
    let order_id = insert_order(&pool, sub, quote_id, location_id, 1200).await.expect("order");

    let request = |threshold: i64| {
        sqlx::query(
            "INSERT INTO order_approvals (order_id, threshold_cents, total_cents, expires_at)
             VALUES ($1, $2, 1200, now() + interval '15 minutes')",
        )
        .bind(order_id)
        .bind(threshold)
        .execute(&pool)
    };
    // Under the threshold there is nothing to approve.
    assert_sqlstate(request(5000).await, CHECK);
    request(1000).await.expect("approval requested");

    let decide = |status: &'static str, amr: Option<Vec<String>>| {
        sqlx::query(
            "UPDATE order_approvals SET status = $2, decided_at = now(), decided_amr = $3, updated_at = now()
             WHERE order_id = $1",
        )
        .bind(order_id)
        .bind(status)
        .bind(amr)
        .execute(&pool)
    };
    assert_sqlstate(decide("APPROVED", Some(vec!["pwd".into()])).await, CHECK);
    decide("APPROVED", Some(vec!["pwd".into(), "hwk".into()])).await.expect("approved with a passkey");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn one_active_station_per_location(pool: PgPool) {
    let sub = insert_customer(&pool).await;
    let location_id = insert_location(&pool, sub, "home").await.expect("location");

    assert_sqlstate(insert_station(&pool, sub, location_id, "not-hex", "PENDING").await, CHECK);
    let station_id =
        insert_station(&pool, sub, location_id, &key_hash("ab"), "ACTIVE").await.expect("active station");
    assert_sqlstate(insert_station(&pool, sub, location_id, &key_hash("cd"), "ACTIVE").await, UNIQUE);
    // A key can be registered only once.
    assert_sqlstate(insert_station(&pool, sub, location_id, &key_hash("ab"), "PENDING").await, UNIQUE);

    let network = |priority: i16, kind: &'static str| {
        sqlx::query(
            "INSERT INTO station_access_networks (id, station_id, priority, kind, params)
             VALUES ($1, $2, $3, $4, '{\"service_uuid\": \"6e400001-b5a3-f393-e0a9-e50e24dcca9e\", \"station_tag\": \"0a0b0c0d\", \"l2cap_psm\": 128}')",
        )
        .bind(Uuid::now_v7())
        .bind(station_id)
        .bind(priority)
        .bind(kind)
        .execute(&pool)
    };
    network(0, "BLUETOOTH_LE").await.expect("bluetooth network");
    assert_sqlstate(network(1, "CARRIER_PIGEON").await, CHECK);
    assert_sqlstate(network(0, "BLUETOOTH_LE").await, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn stations_stay_on_the_customers_own_locations(pool: PgPool) {
    let sub = insert_customer(&pool).await;
    let other = insert_customer(&pool).await;
    let others_location = insert_location(&pool, other, "home").await.expect("location");
    assert_sqlstate(insert_station(&pool, sub, others_location, &key_hash("ab"), "PENDING").await, FOREIGN_KEY);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn inbox_detects_duplicate_messages(pool: PgPool) {
    let insert = || {
        sqlx::query("INSERT INTO inbox (consumer, message_id) VALUES ('fulfilment-events', 'msg-1') ON CONFLICT DO NOTHING")
            .execute(&pool)
    };
    assert_eq!(insert().await.expect("first").rows_affected(), 1);
    assert_eq!(insert().await.expect("duplicate").rows_affected(), 0);
}
