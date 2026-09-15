//! Schema tests for the `commerce` database. They need Postgres:
//!
//! ```bash
//! docker compose up -d --wait postgres
//! DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p commerce-service -- --ignored
//! ```

use sqlx::postgres::PgQueryResult;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

const ORDER_STATES: [&str; 15] = [
    "CART",
    "QUOTED",
    "AUTHORIZING",
    "AWAITING_MERCHANT",
    "CAPTURING",
    "PAID",
    "DISPATCH_REQUESTED",
    "DRONE_ASSIGNED",
    "AT_PICKUP",
    "PICKED_UP",
    "DELIVERED",
    "COMPLETED",
    "PAYMENT_FAILED",
    "CANCELLED",
    "REFUNDED",
];

const UNIQUE: &str = "23505";
const CHECK: &str = "23514";
const FOREIGN_KEY: &str = "23503";

#[track_caller]
fn assert_sqlstate(result: Result<PgQueryResult, sqlx::Error>, expected: &str) {
    match result {
        Ok(_) => panic!("expected SQLSTATE {expected}, but the statement succeeded"),
        Err(err) => {
            let code = err.as_database_error().and_then(|db| db.code()).map(|c| c.into_owned());
            assert_eq!(code.as_deref(), Some(expected), "{err}");
        }
    }
}

async fn insert_customer(pool: &PgPool) -> Uuid {
    let sub = Uuid::now_v7();
    sqlx::query("INSERT INTO customers (sub) VALUES ($1)")
        .bind(sub)
        .execute(pool)
        .await
        .expect("insert customer");
    sub
}

/// A PENDING location.
async fn insert_location(pool: &PgPool, id: Uuid, customer: Uuid, label: &str) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO delivery_locations (id, customer_sub, label, address, lat, lon)
         VALUES ($1, $2, $3, '400 Broad St, Seattle', 47.6205, -122.3493)",
    )
    .bind(id)
    .bind(customer)
    .bind(label)
    .execute(pool)
    .await
}

async fn verified_location(pool: &PgPool, customer: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO delivery_locations (id, customer_sub, label, address, lat, lon, status, verified_at, verified_amr)
         VALUES ($1, $2, 'home', '400 Broad St, Seattle', 47.6205, -122.3493, 'VERIFIED', now(), ARRAY['pwd', 'otp'])",
    )
    .bind(id)
    .bind(customer)
    .execute(pool)
    .await
    .expect("insert verified location");
    id
}

async fn insert_quote(
    pool: &PgPool,
    id: Uuid,
    customer: Uuid,
    location: Uuid,
    (subtotal, fee, total): (i64, i64, i64),
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO quotes (id, customer_sub, business_id, business_name, delivery_location_id,
                             subtotal_cents, delivery_fee_cents, total_cents, payload_g, eta_seconds, expires_at)
         VALUES ($1, $2, $3, 'Victrola Coffee', $4, $5, $6, $7, 850, 900, now() + interval '10 minutes')",
    )
    .bind(id)
    .bind(customer)
    .bind(Uuid::now_v7())
    .bind(location)
    .bind(subtotal)
    .bind(fee)
    .bind(total)
    .execute(pool)
    .await
}

async fn new_quote(pool: &PgPool, customer: Uuid, location: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    insert_quote(pool, id, customer, location, (1200, 499, 1699)).await.expect("insert quote");
    sqlx::query(
        "INSERT INTO quote_items (quote_id, item_id, name, unit_price_cents, qty, weight_g)
         VALUES ($1, $2, 'Oat latte', 600, 2, 425)",
    )
    .bind(id)
    .bind(Uuid::now_v7())
    .execute(pool)
    .await
    .expect("insert quote item");
    id
}

/// A customer with a verified location and a quote for it.
struct Fixture {
    customer: Uuid,
    location: Uuid,
    quote: Uuid,
}

async fn fixture(pool: &PgPool) -> Fixture {
    let customer = insert_customer(pool).await;
    let location = verified_location(pool, customer).await;
    let quote = new_quote(pool, customer, location).await;
    Fixture { customer, location, quote }
}

async fn insert_order(
    pool: &PgPool,
    id: Uuid,
    customer: Uuid,
    quote: Uuid,
    location: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO orders (id, customer_sub, quote_id, business_id, business_name, state,
                             delivery_location_id, dropoff_label, dropoff_address, dropoff_lat, dropoff_lon,
                             pickup_point_id, pickup_lat, pickup_lon,
                             subtotal_cents, delivery_fee_cents, platform_fee_cents, total_cents,
                             payload_g, eta_seconds, merchant_accept_by)
         VALUES ($1, $2, $3, $4, 'Victrola Coffee', 'AUTHORIZING',
                 $5, 'home', '400 Broad St, Seattle', 47.6205, -122.3493,
                 $6, 47.6097, -122.3331,
                 1200, 499, 120, 1699,
                 850, 900, now() + interval '5 minutes')",
    )
    .bind(id)
    .bind(customer)
    .bind(quote)
    .bind(Uuid::now_v7())
    .bind(location)
    .bind(Uuid::now_v7())
    .execute(pool)
    .await
}

async fn place_order(pool: &PgPool, f: &Fixture) -> Uuid {
    let id = Uuid::now_v7();
    insert_order(pool, id, f.customer, f.quote, f.location).await.expect("insert order");
    id
}

async fn insert_payment(pool: &PgPool, order: Uuid, intent: &str, key: &str) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO payments (id, order_id, stripe_payment_intent_id, idempotency_key, status,
                               amount_cents, application_fee_cents)
         VALUES ($1, $2, $3, $4, 'AUTHORIZED', 1699, 619)",
    )
    .bind(Uuid::now_v7())
    .bind(order)
    .bind(intent)
    .bind(key)
    .execute(pool)
    .await
}

async fn insert_stripe_event(pool: &PgPool, id: &str) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO stripe_events (id, type, payload)
           VALUES ($1, 'payment_intent.succeeded', '{"object": "event"}')"#,
    )
    .bind(id)
    .execute(pool)
    .await
}

async fn insert_transition(
    pool: &PgPool,
    order: Uuid,
    (from, to): (&str, &str),
    cause: &str,
    stripe_event: Option<&str>,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO order_state_transitions (id, order_id, from_state, to_state, cause, stripe_event_id)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(order)
    .bind(from)
    .bind(to)
    .bind(cause)
    .bind(stripe_event)
    .execute(pool)
    .await
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn one_order_per_quote(pool: PgPool) {
    let f = fixture(&pool).await;
    place_order(&pool, &f).await;

    // A retried PlaceOrder must find the existing order, never create a second.
    assert_sqlstate(insert_order(&pool, Uuid::now_v7(), f.customer, f.quote, f.location).await, UNIQUE);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn order_uses_the_customers_own_quote_and_location(pool: PgPool) {
    let f = fixture(&pool).await;
    let other = insert_customer(&pool).await;
    let other_location = verified_location(&pool, other).await;

    assert_sqlstate(insert_order(&pool, Uuid::now_v7(), other, f.quote, other_location).await, FOREIGN_KEY);
    assert_sqlstate(insert_order(&pool, Uuid::now_v7(), f.customer, f.quote, other_location).await, FOREIGN_KEY);
    assert_sqlstate(
        insert_quote(&pool, Uuid::now_v7(), f.customer, other_location, (100, 0, 100)).await,
        FOREIGN_KEY,
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn every_order_state_is_accepted_and_others_are_rejected(pool: PgPool) {
    let f = fixture(&pool).await;
    let order = place_order(&pool, &f).await;
    let set_state = |state: &'static str| {
        sqlx::query("UPDATE orders SET state = $1, version = version + 1, updated_at = now() WHERE id = $2")
            .bind(state)
            .bind(order)
            .execute(&pool)
    };

    for state in ORDER_STATES {
        let result = set_state(state).await.unwrap_or_else(|e| panic!("state {state}: {e}"));
        assert_eq!(result.rows_affected(), 1);
    }
    assert_sqlstate(set_state("SHIPPED").await, CHECK);
    assert_sqlstate(set_state("ORDER_STATE_PAID").await, CHECK);
    assert_sqlstate(set_state("paid").await, CHECK);

    // Waiting for the merchant needs a deadline for the timeout sweep.
    assert_sqlstate(
        sqlx::query("UPDATE orders SET state = 'AWAITING_MERCHANT', merchant_accept_by = NULL WHERE id = $1")
            .bind(order)
            .execute(&pool)
            .await,
        CHECK,
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn total_must_equal_subtotal_plus_fee(pool: PgPool) {
    let f = fixture(&pool).await;
    assert_sqlstate(
        insert_quote(&pool, Uuid::now_v7(), f.customer, f.location, (1200, 499, 1700)).await,
        CHECK,
    );
    assert_sqlstate(
        insert_quote(&pool, Uuid::now_v7(), f.customer, f.location, (1200, -1, 1199)).await,
        CHECK,
    );

    let order = place_order(&pool, &f).await;
    assert_sqlstate(
        sqlx::query("UPDATE orders SET total_cents = total_cents + 1 WHERE id = $1")
            .bind(order)
            .execute(&pool)
            .await,
        CHECK,
    );

    // The drone payload limit is 2500 g.
    let set_payload = |grams: i32| {
        sqlx::query("UPDATE quotes SET payload_g = $1 WHERE id = $2")
            .bind(grams)
            .bind(f.quote)
            .execute(&pool)
    };
    set_payload(2500).await.expect("2500 g fits");
    assert_sqlstate(set_payload(2501).await, CHECK);
    assert_sqlstate(set_payload(0).await, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn one_payment_per_order_and_unique_payment_intents(pool: PgPool) {
    let f = fixture(&pool).await;
    let order = place_order(&pool, &f).await;
    let key = f.quote.to_string();
    insert_payment(&pool, order, "pi_first", &key).await.expect("first payment");
    assert_sqlstate(insert_payment(&pool, order, "pi_second", "another-key").await, UNIQUE);

    let second = Fixture { quote: new_quote(&pool, f.customer, f.location).await, ..f };
    let second_order = place_order(&pool, &second).await;
    let second_key = second.quote.to_string();
    assert_sqlstate(insert_payment(&pool, second_order, "pi_first", &second_key).await, UNIQUE);
    assert_sqlstate(insert_payment(&pool, second_order, "pi_second", &key).await, UNIQUE);
    insert_payment(&pool, second_order, "pi_second", &second_key).await.expect("second order's payment");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn duplicate_stripe_event_is_rejected(pool: PgPool) {
    insert_stripe_event(&pool, "evt_1PaidIntent").await.expect("first delivery");
    assert_sqlstate(insert_stripe_event(&pool, "evt_1PaidIntent").await, UNIQUE);

    let retry = sqlx::query(
        "INSERT INTO stripe_events (id, type, payload) VALUES ($1, 'payment_intent.succeeded', '{}')
         ON CONFLICT (id) DO NOTHING",
    )
    .bind("evt_1PaidIntent")
    .execute(&pool)
    .await
    .expect("insert or ignore");
    assert_eq!(retry.rows_affected(), 0);

    assert_sqlstate(insert_stripe_event(&pool, "not_an_event").await, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn only_a_stripe_webhook_moves_an_order_to_paid(pool: PgPool) {
    let f = fixture(&pool).await;
    let order = place_order(&pool, &f).await;
    let to_paid = ("CAPTURING", "PAID");

    assert_sqlstate(insert_transition(&pool, order, to_paid, "GRPC", None).await, CHECK);
    assert_sqlstate(insert_transition(&pool, order, to_paid, "STRIPE_WEBHOOK", None).await, CHECK);
    assert_sqlstate(
        insert_transition(&pool, order, to_paid, "STRIPE_WEBHOOK", Some("evt_never_received")).await,
        FOREIGN_KEY,
    );

    insert_stripe_event(&pool, "evt_succeeded").await.expect("store event");
    insert_transition(&pool, order, to_paid, "STRIPE_WEBHOOK", Some("evt_succeeded"))
        .await
        .expect("webhook transition");

    insert_transition(&pool, order, ("PAID", "DISPATCH_REQUESTED"), "EVENT", None)
        .await
        .expect("ordinary transition");
    assert_sqlstate(insert_transition(&pool, order, ("PAID", "SHIPPED"), "EVENT", None).await, CHECK);
    assert_sqlstate(insert_transition(&pool, order, ("PAID", "PAID"), "EVENT", None).await, CHECK);
    assert_sqlstate(insert_transition(&pool, order, ("PAID", "REFUNDED"), "CRON", None).await, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn label_is_unique_per_customer_until_revoked(pool: PgPool) {
    let customer = insert_customer(&pool).await;
    let office = Uuid::now_v7();
    insert_location(&pool, office, customer, "office").await.expect("first office");

    assert_sqlstate(insert_location(&pool, Uuid::now_v7(), customer, "office").await, UNIQUE);
    assert_sqlstate(insert_location(&pool, Uuid::now_v7(), customer, "Office").await, UNIQUE);

    let other = insert_customer(&pool).await;
    insert_location(&pool, Uuid::now_v7(), other, "office").await.expect("another customer's office");

    sqlx::query("UPDATE delivery_locations SET status = 'REVOKED', updated_at = now() WHERE id = $1")
        .bind(office)
        .execute(&pool)
        .await
        .expect("revoke");
    insert_location(&pool, Uuid::now_v7(), customer, "office").await.expect("label reused after revoke");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn verified_location_needs_mfa_evidence(pool: PgPool) {
    let customer = insert_customer(&pool).await;
    assert_sqlstate(
        sqlx::query(
            "INSERT INTO delivery_locations (id, customer_sub, label, address, lat, lon, status, verified_amr)
             VALUES ($1, $2, 'home', '400 Broad St, Seattle', 47.6205, -122.3493, 'VERIFIED', ARRAY['otp'])",
        )
        .bind(Uuid::now_v7())
        .bind(customer)
        .execute(&pool)
        .await,
        CHECK,
    );

    let location = Uuid::now_v7();
    insert_location(&pool, location, customer, "home").await.expect("pending location");
    let verify = |set: &'static str| {
        // `set` is always a literal from this test.
        sqlx::query(AssertSqlSafe(format!(
            "UPDATE delivery_locations SET {set}, updated_at = now() WHERE id = $1"
        )))
            .bind(location)
            .execute(&pool)
    };

    assert_sqlstate(verify("status = 'VERIFIED'").await, CHECK);
    assert_sqlstate(verify("status = 'VERIFIED', verified_at = now()").await, CHECK);
    assert_sqlstate(verify("status = 'VERIFIED', verified_at = now(), verified_amr = '{pwd}'").await, CHECK);
    verify("status = 'VERIFIED', verified_at = now(), verified_amr = '{pwd,otp}'").await.expect("verify");

    // Back to PENDING would leave stale evidence; REVOKED keeps it.
    assert_sqlstate(verify("status = 'PENDING'").await, CHECK);
    verify("status = 'REVOKED'").await.expect("revoke");

    // Out-of-range coordinates.
    assert_sqlstate(verify("lat = 91").await, CHECK);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn money_rows_never_disappear_with_customer_or_location(pool: PgPool) {
    let f = fixture(&pool).await;
    let order = place_order(&pool, &f).await;
    sqlx::query(
        "INSERT INTO order_items (order_id, item_id, name, unit_price_cents, qty, weight_g)
         VALUES ($1, $2, 'Oat latte', 600, 2, 425)",
    )
    .bind(order)
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .expect("insert order item");
    insert_transition(&pool, order, ("AUTHORIZING", "AWAITING_MERCHANT"), "GRPC", None)
        .await
        .expect("insert transition");
    insert_payment(&pool, order, "pi_keep", &f.quote.to_string()).await.expect("insert payment");

    let delete = |sql: &'static str, id: Uuid| sqlx::query(sql).bind(id).execute(&pool);
    assert_sqlstate(delete("DELETE FROM customers WHERE sub = $1", f.customer).await, FOREIGN_KEY);
    assert_sqlstate(delete("DELETE FROM delivery_locations WHERE id = $1", f.location).await, FOREIGN_KEY);
    assert_sqlstate(delete("DELETE FROM quotes WHERE id = $1", f.quote).await, FOREIGN_KEY);
    assert_sqlstate(delete("DELETE FROM orders WHERE id = $1", order).await, FOREIGN_KEY);

    // Owned children do cascade once their parent may go.
    delete("DELETE FROM payments WHERE order_id = $1", order).await.expect("delete payment");
    delete("DELETE FROM orders WHERE id = $1", order).await.expect("delete order");
    let (items, transitions): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM order_items WHERE order_id = $1),
                (SELECT count(*) FROM order_state_transitions WHERE order_id = $1)",
    )
    .bind(order)
    .fetch_one(&pool)
    .await
    .expect("count children");
    assert_eq!((items, transitions), (0, 0));
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn spending_caps_have_defaults_and_stay_ordered(pool: PgPool) {
    let customer = insert_customer(&pool).await;
    let caps: (i64, i64) = sqlx::query_as("SELECT per_order_cap_cents, daily_cap_cents FROM customers WHERE sub = $1")
        .bind(customer)
        .fetch_one(&pool)
        .await
        .expect("read caps");
    assert_eq!(caps, (5000, 10000));

    let set_caps = |per_order: i64, daily: i64| {
        sqlx::query("UPDATE customers SET per_order_cap_cents = $1, daily_cap_cents = $2 WHERE sub = $3")
            .bind(per_order)
            .bind(daily)
            .bind(customer)
            .execute(&pool)
    };
    set_caps(10000, 10000).await.expect("equal caps");
    assert_sqlstate(set_caps(10001, 10000).await, CHECK);
    assert_sqlstate(set_caps(-1, 10000).await, CHECK);

    assert_sqlstate(
        sqlx::query("UPDATE customers SET default_payment_method_id = 'pm_card' WHERE sub = $1")
            .bind(customer)
            .execute(&pool)
            .await,
        CHECK,
    );
}
