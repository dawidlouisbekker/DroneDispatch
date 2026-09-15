//! Schema tests for merchant-service migrations. Each test gets a fresh
//! database with `./migrations` applied. Run with:
//! `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p merchant-service -- --ignored`

use sqlx::PgPool;
use sqlx::postgres::PgQueryResult;
use uuid::Uuid;

const UNIQUE: &str = "23505";
const CHECK: &str = "23514";
const FOREIGN_KEY: &str = "23503";

fn assert_sqlstate(result: Result<PgQueryResult, sqlx::Error>, code: &str) {
    let err = result.expect_err("statement should have been rejected");
    let db_err = err.as_database_error().expect("expected a database error");
    assert_eq!(
        db_err.code().as_deref(),
        Some(code),
        "unexpected error: {db_err}"
    );
}

async fn insert_business(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO businesses (id, place_id, name, address, lat, lon, categories)
         VALUES ($1, $2, 'Bean There', '1 Main St', 47.6, -122.3, ARRAY['coffee_shop'])",
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
    sqlx::query(
        "INSERT INTO menu_sections (id, business_id, name, position) VALUES ($1, $2, 'Drinks', $3)",
    )
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
        "INSERT INTO menu_items (id, business_id, section_id, name, price_cents, weight_g, stock_qty)
         VALUES ($1, $2, $3, 'Latte', 450, 350, 10)",
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
        "INSERT INTO menu_items (id, business_id, section_id, name, price_cents, weight_g)
         VALUES ($1, $2, $3, 'Muffin', $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(business_id)
    .bind(section_id)
    .bind(price_cents)
    .bind(weight_g)
    .execute(pool)
    .await
}

async fn insert_hold(pool: &PgPool, business_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO stock_reservations (id, business_id, expires_at) VALUES ($1, $2, now() + interval '10 minutes')",
    )
    .bind(id)
    .bind(business_id)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn insert_pickup_point(
    pool: &PgPool,
    business_id: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO pickup_points (id, business_id, lat, lon) VALUES ($1, $2, 47.6, -122.3)",
    )
    .bind(Uuid::now_v7())
    .bind(business_id)
    .execute(pool)
    .await
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn rejects_duplicate_place_id(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    let result = sqlx::query(
        "INSERT INTO businesses (id, place_id, name, address, lat, lon)
         VALUES ($1, $2, 'Copycat', '2 Main St', 47.6, -122.3)",
    )
    .bind(Uuid::now_v7())
    .bind(format!("place-{business_id}"))
    .execute(&pool)
    .await;
    assert_sqlstate(result, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn rejects_out_of_range_coordinates(pool: PgPool) {
    let result = sqlx::query(
        "INSERT INTO businesses (id, place_id, name, address, lat, lon) VALUES ($1, 'p', 'n', 'a', 91, 0)",
    )
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await;
    assert_sqlstate(result, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn rejects_zero_weight_and_negative_price(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    let section_id = insert_section(&pool, business_id, 0).await;

    assert_sqlstate(
        insert_item_with(&pool, business_id, section_id, 300, 0).await,
        CHECK,
    );
    assert_sqlstate(
        insert_item_with(&pool, business_id, section_id, -1, 100).await,
        CHECK,
    );
    insert_item_with(&pool, business_id, section_id, 0, 1)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn rejects_item_in_another_business_section(pool: PgPool) {
    let business_a = insert_business(&pool).await;
    let business_b = insert_business(&pool).await;
    let section_of_b = insert_section(&pool, business_b, 0).await;

    assert_sqlstate(
        insert_item_with(&pool, business_a, section_of_b, 300, 100).await,
        FOREIGN_KEY,
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn rejects_duplicate_section_position(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    insert_section(&pool, business_id, 0).await;
    let result = sqlx::query(
        "INSERT INTO menu_sections (id, business_id, name, position) VALUES ($1, $2, 'Food', 0)",
    )
    .bind(Uuid::now_v7())
    .bind(business_id)
    .execute(&pool)
    .await;
    assert_sqlstate(result, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn committed_hold_needs_order_id(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    let hold_id = insert_hold(&pool, business_id).await;

    let result = sqlx::query(
        "UPDATE stock_reservations SET status = 'COMMITTED', updated_at = now() WHERE id = $1",
    )
    .bind(hold_id)
    .execute(&pool)
    .await;
    assert_sqlstate(result, CHECK);

    sqlx::query("UPDATE stock_reservations SET status = 'COMMITTED', order_id = $2, updated_at = now() WHERE id = $1")
        .bind(hold_id)
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn hold_lines_must_be_positive_and_on_the_same_menu(pool: PgPool) {
    let business_a = insert_business(&pool).await;
    let business_b = insert_business(&pool).await;
    let item_a = insert_item(
        &pool,
        business_a,
        insert_section(&pool, business_a, 0).await,
    )
    .await;
    let item_b = insert_item(
        &pool,
        business_b,
        insert_section(&pool, business_b, 0).await,
    )
    .await;
    let hold_id = insert_hold(&pool, business_a).await;

    let insert_line = |item_id: Uuid, qty: i32| {
        sqlx::query("INSERT INTO stock_reservation_items (reservation_id, item_id, business_id, qty) VALUES ($1, $2, $3, $4)")
            .bind(hold_id)
            .bind(item_id)
            .bind(business_a)
            .bind(qty)
            .execute(&pool)
    };
    assert_sqlstate(insert_line(item_a, 0).await, CHECK);
    assert_sqlstate(insert_line(item_b, 1).await, FOREIGN_KEY);
    insert_line(item_a, 2).await.unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn one_active_pickup_point_per_business(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    insert_pickup_point(&pool, business_id).await.unwrap();
    assert_sqlstate(insert_pickup_point(&pool, business_id).await, UNIQUE);

    // Once the first is rejected, a new one can be placed.
    sqlx::query(
        "UPDATE pickup_points SET status = 'REJECTED', updated_at = now() WHERE business_id = $1",
    )
    .bind(business_id)
    .execute(&pool)
    .await
    .unwrap();
    insert_pickup_point(&pool, business_id).await.unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn verified_pickup_point_needs_photo_fields(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    insert_pickup_point(&pool, business_id).await.unwrap();

    let without_photo = sqlx::query(
        "UPDATE pickup_points SET status = 'VERIFIED', verified_at = now(), updated_at = now() WHERE business_id = $1",
    )
    .bind(business_id)
    .execute(&pool)
    .await;
    assert_sqlstate(without_photo, CHECK);

    let bad_hash =
        sqlx::query("UPDATE pickup_points SET photo_sha256 = 'not-hex' WHERE business_id = $1")
            .bind(business_id)
            .execute(&pool)
            .await;
    assert_sqlstate(bad_hash, CHECK);

    sqlx::query(
        "UPDATE pickup_points
         SET status = 'VERIFIED', photo_object_key = 'pickup/marker.jpg', photo_sha256 = repeat('ab', 32),
             verified_at = now(), updated_at = now()
         WHERE business_id = $1",
    )
    .bind(business_id)
    .execute(&pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn board_order_decision_rules(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    let order_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO merchant_orders (order_id, business_id, items, total_cents, accept_by)
         VALUES ($1, $2, '[{\"item_id\": \"x\", \"qty\": 1}]', 900, now() + interval '5 minutes')",
    )
    .bind(order_id)
    .bind(business_id)
    .execute(&pool)
    .await
    .unwrap();

    let accepted_without_time =
        sqlx::query("UPDATE merchant_orders SET status = 'ACCEPTED' WHERE order_id = $1")
            .bind(order_id)
            .execute(&pool)
            .await;
    assert_sqlstate(accepted_without_time, CHECK);

    let loaded_before_accept =
        sqlx::query("UPDATE merchant_orders SET loaded_at = now() WHERE order_id = $1")
            .bind(order_id)
            .execute(&pool)
            .await;
    assert_sqlstate(loaded_before_accept, CHECK);

    let empty_items = sqlx::query(
        "INSERT INTO merchant_orders (order_id, business_id, items, total_cents, accept_by)
         VALUES ($1, $2, '[]', 0, now())",
    )
    .bind(Uuid::now_v7())
    .bind(business_id)
    .execute(&pool)
    .await;
    assert_sqlstate(empty_items, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
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
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn deleting_a_business_cascades_menu_but_not_holds(pool: PgPool) {
    let business_id = insert_business(&pool).await;
    insert_item(
        &pool,
        business_id,
        insert_section(&pool, business_id, 0).await,
    )
    .await;
    let hold_id = insert_hold(&pool, business_id).await;

    let delete = || {
        sqlx::query("DELETE FROM businesses WHERE id = $1")
            .bind(business_id)
            .execute(&pool)
    };
    assert_sqlstate(delete().await, FOREIGN_KEY);

    sqlx::query("DELETE FROM stock_reservations WHERE id = $1")
        .bind(hold_id)
        .execute(&pool)
        .await
        .unwrap();
    delete().await.unwrap();
    let (items,): (i64,) = sqlx::query_as("SELECT count(*) FROM menu_items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(items, 0);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn rejects_duplicate_inbox_message(pool: PgPool) {
    let insert = || {
        sqlx::query(
            "INSERT INTO inbox (consumer, message_id) VALUES ('merchant-orders-board', 'msg-1')",
        )
        .execute(&pool)
    };
    insert().await.unwrap();
    assert_sqlstate(insert().await, UNIQUE);
}
