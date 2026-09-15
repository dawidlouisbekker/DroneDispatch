//! Schema tests for the `map` database. Run with:
//! `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p map-service -- --ignored`

use sqlx::PgPool;
use uuid::Uuid;

const CHECK_VIOLATION: &str = "23514";
const UNIQUE_VIOLATION: &str = "23505";
const FOREIGN_KEY_VIOLATION: &str = "23503";

fn sqlstate(err: sqlx::Error) -> String {
    err.as_database_error()
        .expect("expected a database error")
        .code()
        .expect("expected a SQLSTATE")
        .into_owned()
}

/// Inserts a web session with the given refresh token parts.
async fn insert_session(
    pool: &PgPool,
    id_hash: &str,
    refresh_ciphertext: Option<&[u8]>,
    refresh_nonce: Option<&[u8]>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO web_sessions
             (id_hash, user_sub, csrf_token_hash, auth_time, grant_id,
              refresh_token_ciphertext, refresh_token_nonce, expires_at)
         VALUES ($1, $2, 'csrf-hash', now(), $3, $4, $5, now() + interval '7 days')",
    )
    .bind(id_hash)
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(refresh_ciphertext)
    .bind(refresh_nonce)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Inserts a login state row.
async fn insert_state(
    pool: &PgPool,
    state_hash: &str,
    purpose: &str,
    location_id: Option<Uuid>,
    web_session_id_hash: Option<&str>,
    return_to: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_login_states
             (state_hash, pkce_verifier_ciphertext, pkce_verifier_nonce, nonce_hash,
              purpose, location_id, web_session_id_hash, return_to, expires_at)
         VALUES ($1, '\\x00ff'::bytea, '\\x000102030405060708090a0b'::bytea, 'nonce-hash',
                 $2, $3, $4, $5, now() + interval '10 minutes')",
    )
    .bind(state_hash)
    .bind(purpose)
    .bind(location_id)
    .bind(web_session_id_hash)
    .bind(return_to)
    .execute(pool)
    .await
    .map(|_| ())
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn step_up_requires_location(pool: PgPool) {
    insert_session(&pool, "sess", None, None).await.unwrap();

    let err = insert_state(&pool, "s1", "STEP_UP", None, Some("sess"), "/locations")
        .await
        .unwrap_err();
    assert_eq!(sqlstate(err), CHECK_VIOLATION);

    insert_state(&pool, "s2", "STEP_UP", Some(Uuid::now_v7()), Some("sess"), "/locations")
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn login_needs_no_location_and_rejects_one(pool: PgPool) {
    insert_state(&pool, "s1", "LOGIN", None, None, "/").await.unwrap();

    let err = insert_state(&pool, "s2", "LOGIN", Some(Uuid::now_v7()), None, "/")
        .await
        .unwrap_err();
    assert_eq!(sqlstate(err), CHECK_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn step_up_is_bound_to_an_existing_session(pool: PgPool) {
    let err = insert_state(&pool, "s1", "STEP_UP", Some(Uuid::now_v7()), None, "/locations")
        .await
        .unwrap_err();
    assert_eq!(sqlstate(err), CHECK_VIOLATION);

    let err = insert_state(&pool, "s2", "STEP_UP", Some(Uuid::now_v7()), Some("missing"), "/locations")
        .await
        .unwrap_err();
    assert_eq!(sqlstate(err), FOREIGN_KEY_VIOLATION);

    // Dropping the session (e.g. on grant_revoked) drops its pending step-up.
    insert_session(&pool, "sess", None, None).await.unwrap();
    insert_state(&pool, "s3", "STEP_UP", Some(Uuid::now_v7()), Some("sess"), "/locations")
        .await
        .unwrap();
    sqlx::query("DELETE FROM web_sessions WHERE id_hash = 'sess'")
        .execute(&pool)
        .await
        .unwrap();
    let (left,): (i64,) = sqlx::query_as("SELECT count(*) FROM oauth_login_states")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn bogus_purpose_is_rejected(pool: PgPool) {
    let err = insert_state(&pool, "s1", "SIGNUP", None, None, "/").await.unwrap_err();
    assert_eq!(sqlstate(err), CHECK_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn return_to_must_be_a_relative_path(pool: PgPool) {
    for (i, bad) in [
        "https://evil.example",
        "//evil.example",
        "/\\evil.example",
        "/\t/evil.example",
        "evil.example",
        "",
    ]
    .into_iter()
    .enumerate()
    {
        let err = insert_state(&pool, &format!("bad{i}"), "LOGIN", None, None, bad)
            .await
            .unwrap_err();
        assert_eq!(sqlstate(err), CHECK_VIOLATION, "return_to {bad:?} should be rejected");
    }

    for (i, good) in ["/", "/locations", "/locations/verify?id=0192f0c2-0000-7000-8000-000000000000"]
        .into_iter()
        .enumerate()
    {
        insert_state(&pool, &format!("good{i}"), "LOGIN", None, None, good)
            .await
            .unwrap_or_else(|e| panic!("return_to {good:?} should be accepted: {e}"));
    }
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn duplicate_state_hash_is_rejected(pool: PgPool) {
    insert_state(&pool, "same", "LOGIN", None, None, "/").await.unwrap();
    let err = insert_state(&pool, "same", "LOGIN", None, None, "/").await.unwrap_err();
    assert_eq!(sqlstate(err), UNIQUE_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn refresh_token_ciphertext_and_nonce_are_set_together(pool: PgPool) {
    let nonce = [7u8; 12];

    let err = insert_session(&pool, "a", Some(b"ciphertext"), None).await.unwrap_err();
    assert_eq!(sqlstate(err), CHECK_VIOLATION);

    let err = insert_session(&pool, "b", None, Some(&nonce)).await.unwrap_err();
    assert_eq!(sqlstate(err), CHECK_VIOLATION);

    // AES-GCM nonces are 96 bits.
    let err = insert_session(&pool, "c", Some(b"ciphertext"), Some(&[7u8; 8])).await.unwrap_err();
    assert_eq!(sqlstate(err), CHECK_VIOLATION);

    insert_session(&pool, "d", Some(b"ciphertext"), Some(&nonce)).await.unwrap();
    insert_session(&pool, "e", None, None).await.unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn amr_defaults_to_empty_array(pool: PgPool) {
    insert_session(&pool, "sess", None, None).await.unwrap();
    let (amr,): (Vec<String>,) = sqlx::query_as("SELECT amr FROM web_sessions WHERE id_hash = 'sess'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(amr.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs Postgres: docker compose up -d --wait postgres"]
async fn session_must_expire_after_creation(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO web_sessions (id_hash, user_sub, csrf_token_hash, auth_time, grant_id, expires_at)
         VALUES ('sess', $1, 'csrf-hash', now(), $2, now() - interval '1 minute')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .unwrap_err();
    assert_eq!(sqlstate(err), CHECK_VIOLATION);
}
