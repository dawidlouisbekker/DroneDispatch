//! Schema tests for the `catalog_read_<zone>` databases. They need Postgres:
//! `scripts/test-db.sh -p catalog-read-service`.

use sqlx::PgPool;
use uuid::Uuid;

const UNIQUE: &str = "23505";
const CHECK: &str = "23514";

#[track_caller]
fn assert_sqlstate<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, code: &str) {
    let err = result.expect_err("expected a database error");
    assert_eq!(err.as_database_error().expect("database error").code().as_deref(), Some(code), "{err}");
}

/// The projection's upsert: applies only when the message is newer than the row.
async fn project_catalog(
    pool: &PgPool,
    place_id: &str,
    business_id: Uuid,
    spoken_name: &str,
    source_seq: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO catalogs (place_id, business_id, spoken_name, address, lat, lon, categories,
                               accepting_orders, prep_time_minutes, source_seq)
         VALUES ($1, $2, $3, '1 Main St, Seattle', 47.6, -122.3, ARRAY['coffee_shop'], true, 10, $4)
         ON CONFLICT (place_id) DO UPDATE
             SET spoken_name = EXCLUDED.spoken_name, source_seq = EXCLUDED.source_seq, updated_at = now()
             WHERE catalogs.source_seq < EXCLUDED.source_seq",
    )
    .bind(place_id)
    .bind(business_id)
    .bind(spoken_name)
    .bind(source_seq)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

async fn insert_item(pool: &PgPool, price_cents: i64, weight_g: i32, stock: Option<i32>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO catalog_items (item_id, place_id, section_id, name, spoken_name, price_cents, weight_g,
                                    available, stock_remaining, source_seq)
         VALUES ($1, 'place-1', $2, 'Latte 12oz', 'latte', $3, $4, true, $5, 1)",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(price_cents)
    .bind(weight_g)
    .bind(stock)
    .execute(pool)
    .await
    .map(|_| ())
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn stale_and_replayed_messages_change_nothing(pool: PgPool) {
    let business_id = Uuid::now_v7();
    assert_eq!(project_catalog(&pool, "place-1", business_id, "Joe's Coffee", 5).await.unwrap(), 1);
    assert_eq!(project_catalog(&pool, "place-1", business_id, "Old name", 3).await.unwrap(), 0, "stale");
    assert_eq!(project_catalog(&pool, "place-1", business_id, "Joe's", 5).await.unwrap(), 0, "replayed");
    assert_eq!(project_catalog(&pool, "place-1", business_id, "Joe's Coffee & Bakery", 7).await.unwrap(), 1);

    let (spoken_name,): (String,) = sqlx::query_as("SELECT spoken_name FROM catalogs WHERE place_id = 'place-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(spoken_name, "Joe's Coffee & Bakery");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn one_catalog_per_business(pool: PgPool) {
    let business_id = Uuid::now_v7();
    project_catalog(&pool, "place-1", business_id, "Joe's Coffee", 1).await.unwrap();
    assert_sqlstate(project_catalog(&pool, "place-2", business_id, "Joe's Coffee", 2).await, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn items_need_weight_and_non_negative_price_and_stock(pool: PgPool) {
    assert_sqlstate(insert_item(&pool, 450, 0, None).await, CHECK);
    assert_sqlstate(insert_item(&pool, -1, 350, None).await, CHECK);
    assert_sqlstate(insert_item(&pool, 450, 350, Some(-1)).await, CHECK);
    // No foreign keys: an item may arrive before its catalog and section.
    insert_item(&pool, 450, 350, Some(0)).await.expect("valid item");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn inbox_detects_duplicate_messages(pool: PgPool) {
    let insert = || {
        sqlx::query("INSERT INTO inbox (consumer, message_id) VALUES ('catalog-projection', 'msg-1') ON CONFLICT DO NOTHING")
            .execute(&pool)
    };
    assert_eq!(insert().await.expect("first").rows_affected(), 1);
    assert_eq!(insert().await.expect("duplicate").rows_affected(), 0);
}
