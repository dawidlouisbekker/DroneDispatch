//! Schema tests for merchant-service migrations. Each test gets a fresh database
//! with `./migrations` applied. Run with `scripts/test-db.sh -p merchant-service`.

use sqlx::PgPool;
use sqlx::postgres::PgQueryResult;
use uuid::Uuid;

const UNIQUE: &str = "23505";
const CHECK: &str = "23514";
const FOREIGN_KEY: &str = "23503";

#[track_caller]
fn assert_sqlstate<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, code: &str) {
    let err = result.expect_err("statement should have been rejected");
    let db_err = err.as_database_error().expect("expected a database error");
    assert_eq!(db_err.code().as_deref(), Some(code), "unexpected error: {db_err}");
}

fn key_hash(byte: &str) -> String {
    byte.repeat(32)
}

async fn insert_business(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO businesses (id, place_id, name, spoken_name, address, lat, lon, categories)
         VALUES ($1, $2, 'Bean There Coffee LLC', 'Bean There', '1 Main St', 47.6, -122.3, ARRAY['coffee_shop'])",
    )
    .bind(id)
    .bind(format!("place-{id}"))
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn insert_section(pool: &PgPool, business_id: Uuid, position: i32) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO menu_sections (id, business_id, name, position) VALUES ($1, $2, 'Drinks', $3)")
        .bind(id)
        .bind(business_id)
        .bind(position)
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn insert_item(pool: &PgPool, business_id: Uuid, section_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO menu_items (id, business_id, section_id, name, spoken_name, price_cents, weight_g, stock_qty)
         VALUES ($1, $2, $3, 'Latte 12oz', 'latte', 450, 350, 10)",
    )
    .bind(id)
    .bind(business_id)
    .bind(section_id)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn insert_item_with(
    pool: &PgPool,
    business_id: Uuid,
    section_id: Uuid,
    price_cents: i64,
    weight_g: i32,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO menu_items (id, business_id, section_id, name, spoken_name, price_cents, weight_g)
         VALUES ($1, $2, $3, 'Muffin', 'muffin', $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(business_id)
    .bind(section_id)
    .bind(price_cents)
    .bind(weight_g)
    .execute(pool)
    .await
}

async fn insert_station(
    pool: &PgPool,
    business_id: Uuid,
    public_key_sha256: &str,
    status: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO stations (id, business_id, lat, lon, position_accuracy_m, public_key, public_key_sha256, status, activated_at)
         VALUES ($1, $2, 47.6, -122.3, 1.5, '\\x3059', $3, $4, CASE WHEN $4 = 'ACTIVE' THEN now() END)",
    )
    .bind(id)
    .bind(business_id)
    .bind(public_key_sha256)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn insert_fulfilment(pool: &PgPool, business_id: Uuid, station_id: Uuid) -> Result<Uuid, sqlx::Error> {
    let order_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO fulfilment_orders (order_id, business_id, customer_sub, pickup_station_id, items, subtotal_cents, accept_by)
         VALUES ($1, $2, $3, $4, '[{\"item_id\": \"x\", \"qty\": 1}]', 900, now() + interval '5 minutes')",
    )
    .bind(order_id)
    .bind(business_id)
    .bind(Uuid::now_v7())
    .bind(station_id)
    .execute(pool)
    .await?;
    Ok(order_id)
}

async fn insert_hold(pool: &PgPool, order_id: Uuid, business_id: Uuid) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO stock_reservations (order_id, business_id, expires_at) VALUES ($1, $2, now() + interval '5 minutes')",
    )
    .bind(order_id)
    .bind(business_id)
    .execute(pool)
    .await
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_duplicate_place_id(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    let result = sqlx::query(
        "INSERT INTO businesses (id, place_id, name, spoken_name, address, lat, lon)
         VALUES ($1, $2, 'Copycat', 'Copycat', '2 Main St', 47.6, -122.3)",
    )
    .bind(Uuid::now_v7())
    .bind(format!("place-{business_id}"))
    .execute(&pool)
    .await;
    assert_sqlstate(result, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_out_of_range_coordinates(pool: PgPool) {
    let result = sqlx::query(
        "INSERT INTO businesses (id, place_id, name, spoken_name, address, lat, lon) VALUES ($1, 'p', 'n', 's', 'a', 91, 0)",
    )
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await;
    assert_sqlstate(result, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_empty_spoken_names(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    let item_id = insert_item(&pool, business_id, insert_section(&pool, business_id, 0).await).await;

    let business = sqlx::query("UPDATE businesses SET spoken_name = '' WHERE id = $1").bind(business_id).execute(&pool).await;
    assert_sqlstate(business, CHECK);
    let item = sqlx::query("UPDATE menu_items SET spoken_name = '' WHERE id = $1").bind(item_id).execute(&pool).await;
    assert_sqlstate(item, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_zero_weight_and_negative_price(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    let section_id = insert_section(&pool, business_id, 0).await;

    assert_sqlstate(insert_item_with(&pool, business_id, section_id, 300, 0).await, CHECK);
    assert_sqlstate(insert_item_with(&pool, business_id, section_id, -1, 100).await, CHECK);
    insert_item_with(&pool, business_id, section_id, 0, 1).await.unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_item_in_another_business_section(pool: PgPool) {
    let business_a = insert_business(&pool).await;
    let business_b = insert_business(&pool).await;
    let section_of_b = insert_section(&pool, business_b, 0).await;

    assert_sqlstate(insert_item_with(&pool, business_a, section_of_b, 300, 100).await, FOREIGN_KEY);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_duplicate_section_position(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    insert_section(&pool, business_id, 0).await;
    let result = sqlx::query("INSERT INTO menu_sections (id, business_id, name, position) VALUES ($1, $2, 'Food', 0)")
        .bind(Uuid::now_v7())
        .bind(business_id)
        .execute(&pool)
        .await;
    assert_sqlstate(result, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn one_active_station_per_business(pool: PgPool) {
    let business_id = insert_business(&pool).await;

    assert_sqlstate(insert_station(&pool, business_id, "not-hex", "PENDING").await, CHECK);
    let station_id = insert_station(&pool, business_id, &key_hash("ab"), "ACTIVE").await.unwrap();
    assert_sqlstate(insert_station(&pool, business_id, &key_hash("cd"), "ACTIVE").await, UNIQUE);
    // A replacement can be registered while the active station keeps working.
    insert_station(&pool, business_id, &key_hash("cd"), "PENDING").await.unwrap();

    let without_time = sqlx::query("UPDATE stations SET activated_at = NULL WHERE id = $1")
        .bind(station_id)
        .execute(&pool)
        .await;
    assert_sqlstate(without_time, CHECK);

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
    network(0, "BLUETOOTH_LE").await.unwrap();
    assert_sqlstate(network(1, "CARRIER_PIGEON").await, CHECK);
    assert_sqlstate(network(0, "BLUETOOTH_LE").await, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn fulfilment_decision_rules(pool: PgPool) {
    let business_a = insert_business(&pool).await;
    let business_b = insert_business(&pool).await;
    let station_a = insert_station(&pool, business_a, &key_hash("ab"), "ACTIVE").await.unwrap();
    let station_b = insert_station(&pool, business_b, &key_hash("cd"), "ACTIVE").await.unwrap();

    // The pickup station must be the business's own.
    assert_sqlstate(insert_fulfilment(&pool, business_a, station_b).await, FOREIGN_KEY);
    let order_id = insert_fulfilment(&pool, business_a, station_a).await.unwrap();

    let update = |sql: &'static str| sqlx::query(sql).bind(order_id).execute(&pool);
    assert_sqlstate(update("UPDATE fulfilment_orders SET status = 'ACCEPTED' WHERE order_id = $1").await, CHECK);
    assert_sqlstate(update("UPDATE fulfilment_orders SET loaded_at = now() WHERE order_id = $1").await, CHECK);
    assert_sqlstate(
        update("UPDATE fulfilment_orders SET status = 'LOADED', decided_at = now() WHERE order_id = $1").await,
        CHECK,
    );
    update("UPDATE fulfilment_orders SET status = 'ACCEPTED', decided_at = now(), updated_at = now() WHERE order_id = $1")
        .await
        .unwrap();
    update("UPDATE fulfilment_orders SET status = 'LOADED', loaded_at = now(), updated_at = now() WHERE order_id = $1")
        .await
        .unwrap();

    let empty_items = sqlx::query(
        "INSERT INTO fulfilment_orders (order_id, business_id, customer_sub, pickup_station_id, items, subtotal_cents, accept_by)
         VALUES ($1, $2, $3, $4, '[]', 0, now())",
    )
    .bind(Uuid::now_v7())
    .bind(business_a)
    .bind(Uuid::now_v7())
    .bind(station_a)
    .execute(&pool)
    .await;
    assert_sqlstate(empty_items, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn holds_follow_their_order(pool: PgPool) {
    let business_a = insert_business(&pool).await;
    let business_b = insert_business(&pool).await;
    let item_a = insert_item(&pool, business_a, insert_section(&pool, business_a, 0).await).await;
    let item_b = insert_item(&pool, business_b, insert_section(&pool, business_b, 0).await).await;
    let station_a = insert_station(&pool, business_a, &key_hash("ab"), "ACTIVE").await.unwrap();
    let order_id = insert_fulfilment(&pool, business_a, station_a).await.unwrap();

    // A hold needs a submitted order of the same business.
    assert_sqlstate(insert_hold(&pool, Uuid::now_v7(), business_a).await, FOREIGN_KEY);
    assert_sqlstate(insert_hold(&pool, order_id, business_b).await, FOREIGN_KEY);
    insert_hold(&pool, order_id, business_a).await.unwrap();

    let insert_line = |item_id: Uuid, qty: i32| {
        sqlx::query("INSERT INTO stock_reservation_items (order_id, item_id, business_id, qty) VALUES ($1, $2, $3, $4)")
            .bind(order_id)
            .bind(item_id)
            .bind(business_a)
            .bind(qty)
            .execute(&pool)
    };
    assert_sqlstate(insert_line(item_a, 0).await, CHECK);
    assert_sqlstate(insert_line(item_b, 1).await, FOREIGN_KEY);
    insert_line(item_a, 2).await.unwrap();

    // A business with orders can't be deleted; removing the order removes its hold.
    let delete_business = sqlx::query("DELETE FROM businesses WHERE id = $1").bind(business_a).execute(&pool).await;
    assert_sqlstate(delete_business, FOREIGN_KEY);
    sqlx::query("DELETE FROM fulfilment_orders WHERE order_id = $1").bind(order_id).execute(&pool).await.unwrap();
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM stock_reservations), (SELECT count(*) FROM stock_reservation_items)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0));
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn deleting_a_business_cascades_its_menu(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    insert_item(&pool, business_id, insert_section(&pool, business_id, 0).await).await;

    sqlx::query("DELETE FROM businesses WHERE id = $1").bind(business_id).execute(&pool).await.unwrap();
    let (items,): (i64,) = sqlx::query_as("SELECT count(*) FROM menu_items").fetch_one(&pool).await.unwrap();
    assert_eq!(items, 0);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn webhook_url_must_be_https(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    let insert = |url: &'static str| {
        sqlx::query(
            "INSERT INTO business_webhooks (business_id, url, signing_secret_ciphertext, signing_secret_nonce)
             VALUES ($1, $2, '\\x01', decode(repeat('00', 12), 'hex'))",
        )
        .bind(business_id)
        .bind(url)
        .execute(&pool)
    };
    assert_sqlstate(insert("http://pos.example.com/hook").await, CHECK);
    insert("https://pos.example.com/hook").await.unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_duplicate_inbox_message(pool: PgPool) {
    let insert = || {
        sqlx::query("INSERT INTO inbox (consumer, message_id) VALUES ('mission-events', 'msg-1')").execute(&pool)
    };
    insert().await.unwrap();
    assert_sqlstate(insert().await, UNIQUE);
}
