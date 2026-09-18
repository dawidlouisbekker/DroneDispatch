//! Schema tests for the `auth` database. Each test gets a fresh database with
//! `./migrations` applied. Run them with:
//!
//! ```bash
//! docker compose up -d --wait postgres
//! DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p auth-service -- --ignored
//! ```

use sqlx::PgPool;
use sqlx::postgres::PgQueryResult;
use uuid::Uuid;

const UNIQUE_VIOLATION: &str = "23505";
const CHECK_VIOLATION: &str = "23514";
const FOREIGN_KEY_VIOLATION: &str = "23503";
const NOT_NULL_VIOLATION: &str = "23502";

const PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNo";
/// The S256 challenge from RFC 7636 appendix B.
const CODE_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

/// SQLSTATE of a statement that must have failed.
fn sqlstate(result: Result<PgQueryResult, sqlx::Error>) -> String {
    let err = result.expect_err("statement should have failed");
    err.as_database_error()
        .expect("database error")
        .code()
        .expect("SQLSTATE")
        .into_owned()
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.expect(sql)
}

async fn insert_user(pool: &PgPool, email: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(email)
        .bind(PASSWORD_HASH)
        .execute(pool)
        .await
        .expect("insert user");
    id
}

async fn insert_client(pool: &PgPool, client_id: &str) {
    sqlx::query(
        "INSERT INTO clients (client_id, secret_hash, name, redirect_uris, auth_method, kind)
         VALUES ($1, 'c2VjcmV0LWhhc2g', 'Alexa', ARRAY['https://layla.amazon.com/api/skill/link/M1'],
                 'client_secret_basic', 'STATIC')",
    )
    .bind(client_id)
    .execute(pool)
    .await
    .expect("insert client");
}

async fn insert_grant(pool: &PgPool, user_id: Uuid, client_id: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO grants (id, user_id, client_id, resource, scope)
         VALUES ($1, $2, $3, 'https://dispatch.example/mcp', 'openid delivery')",
    )
    .bind(id)
    .bind(user_id)
    .bind(client_id)
    .execute(pool)
    .await
    .expect("insert grant");
    id
}

async fn insert_code(
    pool: &PgPool,
    grant_id: Uuid,
    code_challenge: &'static str,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO auth_codes (code_hash, grant_id, redirect_uri, redirect_uri_provided, scope, resource,
                                 code_challenge, acr, amr, auth_time, expires_at)
         VALUES ('Y29kZS1oYXNo', $1, 'https://layla.amazon.com/api/skill/link/M1', true, 'openid delivery',
                 'https://dispatch.example/mcp', $2, 'pwd', ARRAY['pwd'], now(), now() + interval '1 minute')",
    )
    .bind(grant_id)
    .bind(code_challenge)
    .execute(pool)
    .await
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_duplicate_email_ignoring_case(pool: PgPool) {
    insert_user(&pool, "pilot@example.com").await;

    let duplicate = sqlx::query(
        "INSERT INTO users (id, email, password_hash) VALUES ($1, 'Pilot@Example.COM', $2)",
    )
    .bind(Uuid::now_v7())
    .bind(PASSWORD_HASH)
    .execute(&pool)
    .await;
    assert_eq!(sqlstate(duplicate), UNIQUE_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_password_hash_that_is_not_argon2id(pool: PgPool) {
    let bcrypt = sqlx::query(
        "INSERT INTO users (id, email, password_hash) VALUES ($1, 'a@example.com', '$2b$12$abc')",
    )
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await;
    assert_eq!(sqlstate(bcrypt), CHECK_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn client_kind_and_auth_method_must_be_known(pool: PgPool) {
    let insert = |kind: &'static str, auth_method: &'static str| {
        sqlx::query(
            "INSERT INTO clients (client_id, secret_hash, name, redirect_uris, auth_method, kind)
             VALUES ($1, 'aGFzaA', 'Client', ARRAY['https://app.example/cb'], $2, $3)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(auth_method)
        .bind(kind)
        .execute(&pool)
    };

    assert!(insert("DCR", "client_secret_post").await.is_ok());
    assert_eq!(
        sqlstate(insert("static", "client_secret_basic").await),
        CHECK_VIOLATION
    );
    assert_eq!(
        sqlstate(insert("CIMD", "client_secret_basic").await),
        CHECK_VIOLATION
    );
    assert_eq!(
        sqlstate(insert("STATIC", "private_key_jwt").await),
        CHECK_VIOLATION
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn client_secret_must_match_auth_method(pool: PgPool) {
    let insert = |secret_hash: Option<&'static str>, auth_method: &'static str| {
        sqlx::query(
            "INSERT INTO clients (client_id, secret_hash, name, redirect_uris, auth_method, kind)
             VALUES ($1, $2, 'Client', ARRAY['https://app.example/cb'], $3, 'DCR')",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(secret_hash)
        .bind(auth_method)
        .execute(&pool)
    };

    assert!(insert(None, "none").await.is_ok());
    assert!(insert(Some("aGFzaA"), "client_secret_basic").await.is_ok());
    assert_eq!(
        sqlstate(insert(Some("aGFzaA"), "none").await),
        CHECK_VIOLATION
    );
    assert_eq!(
        sqlstate(insert(None, "client_secret_post").await),
        CHECK_VIOLATION
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn client_needs_at_least_one_redirect_uri(pool: PgPool) {
    let insert = |redirect_uris: Option<Vec<Option<&'static str>>>| {
        sqlx::query(
            "INSERT INTO clients (client_id, name, redirect_uris, auth_method, kind)
             VALUES ($1, 'Client', $2, 'none', 'DCR')",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(redirect_uris)
        .execute(&pool)
    };

    assert_eq!(sqlstate(insert(Some(vec![])).await), CHECK_VIOLATION);
    assert_eq!(
        sqlstate(insert(Some(vec![Some("https://app.example/cb"), None])).await),
        CHECK_VIOLATION
    );
    assert_eq!(sqlstate(insert(None).await), NOT_NULL_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn auth_code_requires_s256_challenge_and_existing_grant(pool: PgPool) {
    let user_id = insert_user(&pool, "pilot@example.com").await;
    insert_client(&pool, "alexa").await;
    let grant_id = insert_grant(&pool, user_id, "alexa").await;

    // A plain-method challenge is the raw verifier, not a 43-character SHA-256.
    let plain = insert_code(&pool, grant_id, "a-plain-pkce-verifier-that-is-not-hashed").await;
    assert_eq!(sqlstate(plain), CHECK_VIOLATION);

    let orphan = insert_code(&pool, Uuid::now_v7(), CODE_CHALLENGE).await;
    assert_eq!(sqlstate(orphan), FOREIGN_KEY_VIOLATION);

    insert_code(&pool, grant_id, CODE_CHALLENGE)
        .await
        .expect("valid code");
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn rejects_duplicate_passkey_credential_id(pool: PgPool) {
    let alice = insert_user(&pool, "alice@example.com").await;
    let bob = insert_user(&pool, "bob@example.com").await;
    let insert = |user_id: Uuid| {
        sqlx::query(
            "INSERT INTO passkeys (id, user_id, credential_id, passkey, name)
             VALUES ($1, $2, '\\x0102030405'::bytea, '{\"cred\": {}}', 'Phone')",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .execute(&pool)
    };

    insert(alice).await.expect("first passkey");
    assert_eq!(sqlstate(insert(bob).await), UNIQUE_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn webauthn_challenge_kind_and_user_rules(pool: PgPool) {
    let user_id = insert_user(&pool, "pilot@example.com").await;
    let insert = |user_id: Option<Uuid>, kind: &'static str| {
        sqlx::query(
            "INSERT INTO webauthn_challenges (id_hash, user_id, kind, state, expires_at)
             VALUES ($1, $2, $3, '{}', now() + interval '5 minutes')",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(user_id)
        .bind(kind)
        .execute(&pool)
    };

    // Passkey-only login starts before the user is known.
    assert!(insert(None, "AUTHENTICATION").await.is_ok());
    assert!(insert(Some(user_id), "REGISTRATION").await.is_ok());
    assert_eq!(
        sqlstate(insert(None, "REGISTRATION").await),
        CHECK_VIOLATION
    );
    assert_eq!(
        sqlstate(insert(Some(user_id), "authentication").await),
        CHECK_VIOLATION
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn deleting_user_cascades_to_everything_they_own(pool: PgPool) {
    let user_id = insert_user(&pool, "pilot@example.com").await;
    let other_user = insert_user(&pool, "other@example.com").await;
    insert_client(&pool, "alexa").await;
    let grant_id = insert_grant(&pool, user_id, "alexa").await;
    let other_grant = insert_grant(&pool, other_user, "alexa").await;
    insert_code(&pool, grant_id, CODE_CHALLENGE)
        .await
        .expect("insert code");

    let refresh = "INSERT INTO refresh_tokens (token_hash, grant_id, expires_at) VALUES ($1, $2, now() + interval '30 days')";
    for (token_hash, grant) in [("cnQtMQ", grant_id), ("cnQtMg", other_grant)] {
        sqlx::query(refresh)
            .bind(token_hash)
            .bind(grant)
            .execute(&pool)
            .await
            .expect("insert refresh token");
    }

    for statement in [
        "INSERT INTO sessions (id_hash, user_id, csrf_token_hash, auth_time, amr, expires_at)
         VALUES ('c2Vzc2lvbg', $1, 'Y3NyZg', now(), ARRAY['pwd', 'hwk'], now() + interval '1 day')",
        "INSERT INTO webauthn_challenges (id_hash, user_id, kind, state, expires_at)
         VALUES ('Y2hhbGxlbmdl', $1, 'REGISTRATION', '{}', now() + interval '5 minutes')",
    ] {
        sqlx::query(statement).bind(user_id).execute(&pool).await.expect(statement);
    }
    sqlx::query(
        "INSERT INTO passkeys (id, user_id, credential_id, passkey, name) VALUES ($1, $2, '\\x0a0b', '{}', 'Laptop')",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("insert passkey");

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("delete user");

    for sql in [
        "SELECT count(*) FROM sessions",
        "SELECT count(*) FROM auth_codes",
        "SELECT count(*) FROM passkeys",
        "SELECT count(*) FROM webauthn_challenges",
    ] {
        assert_eq!(count(&pool, sql).await, 0, "{sql}");
    }
    // Only the other user's grant and refresh token survive; the client stays.
    assert_eq!(count(&pool, "SELECT count(*) FROM grants").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM refresh_tokens").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM clients").await, 1);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn client_with_grants_cannot_be_deleted(pool: PgPool) {
    let user_id = insert_user(&pool, "pilot@example.com").await;
    insert_client(&pool, "alexa").await;
    insert_grant(&pool, user_id, "alexa").await;

    let delete = sqlx::query("DELETE FROM clients WHERE client_id = 'alexa'")
        .execute(&pool)
        .await;
    assert_eq!(sqlstate(delete), FOREIGN_KEY_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn signup_challenges_require_valid_attempt_state(pool: PgPool) {
    let insert = |attempts: i32| {
        sqlx::query(
            "INSERT INTO signup_challenges (id_hash, email_hash, otp_hash, expires_at, attempts)
             VALUES ($1, 'email-hash', 'otp-hash', now() + interval '10 minutes', $2)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(attempts)
        .execute(&pool)
    };

    insert(0).await.expect("valid signup challenge");
    assert_eq!(sqlstate(insert(-1).await), CHECK_VIOLATION);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "schema test: run scripts/test-db.sh (Postgres on localhost:5432)"]
async fn outbox_relay_sees_only_unpublished_rows(pool: PgPool) {
    let indexdef: String = sqlx::query_scalar(
        "SELECT indexdef FROM pg_indexes WHERE tablename = 'outbox' AND indexname = 'outbox_unpublished'",
    )
    .fetch_one(&pool)
    .await
    .expect("outbox_unpublished index");
    assert!(
        indexdef.contains("(created_at) WHERE (published_at IS NULL)"),
        "{indexdef}"
    );

    let (published, pending) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "INSERT INTO outbox (id, subject, payload, published_at) VALUES
             ($1, 'auth.events.grant_revoked', '\\x0a01', now()),
             ($2, 'auth.events.user_deleted', '\\x0a02', NULL)",
    )
    .bind(published)
    .bind(pending)
    .execute(&pool)
    .await
    .expect("insert outbox rows");

    let waiting: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM outbox WHERE published_at IS NULL
         ORDER BY created_at LIMIT 100 FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(&pool)
    .await
    .expect("relay query");
    assert_eq!(waiting, vec![pending]);
}
