//! Browser sessions on the sign-in screens: an opaque cookie whose SHA-256 is the
//! `sessions` row id. A session remembers how the user authenticated (`amr`) and
//! when they last did (`auth_time`).

use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AppState, crypto,
    error::{ApiError, ApiResult},
};

pub const COOKIE: &str = "dd_session";
const SESSION_DAYS: i64 = 7;
/// How recent a second factor must be for step-up and sensitive account changes.
pub const FRESH_MFA_SECS: i64 = 300;
/// The RFC 8176 method of the only second factor, a passkey.
const SECOND_FACTOR: &str = "hwk";

#[derive(Clone, Debug)]
pub struct Session {
    pub id_hash: String,
    pub user_id: Uuid,
    pub email: String,
    pub amr: Vec<String>,
    pub auth_time: OffsetDateTime,
}

impl Session {
    pub fn has_second_factor(&self) -> bool {
        self.amr.iter().any(|method| method == SECOND_FACTOR)
    }

    pub fn has_fresh_second_factor(&self, max_age_secs: i64) -> bool {
        self.has_second_factor()
            && (OffsetDateTime::now_utc() - self.auth_time).whole_seconds() <= max_age_secs
    }

    pub fn acr(&self) -> &'static str {
        if self.has_second_factor() {
            "mfa"
        } else {
            "pwd"
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Factors {
    pub passkeys: i64,
}

impl Factors {
    /// Whether the account has a passkey to use as its second factor.
    pub fn any(&self) -> bool {
        self.passkeys > 0
    }
}

#[derive(Serialize)]
pub struct SessionView {
    pub user: UserView,
    pub amr: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub auth_time: OffsetDateTime,
    pub second_factor_pending: bool,
    pub factors: Factors,
}

#[derive(Serialize)]
pub struct UserView {
    pub sub: Uuid,
    pub email: String,
}

pub async fn factors(db: &PgPool, user_id: Uuid) -> sqlx::Result<Factors> {
    let (passkeys,): (i64,) = sqlx::query_as("SELECT count(*) FROM passkeys WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await?;
    Ok(Factors { passkeys })
}

pub async fn view(db: &PgPool, session: &Session) -> sqlx::Result<SessionView> {
    let factors = factors(db, session.user_id).await?;
    Ok(SessionView {
        user: UserView {
            sub: session.user_id,
            email: session.email.clone(),
        },
        amr: session.amr.clone(),
        auth_time: session.auth_time,
        second_factor_pending: factors.any() && !session.has_second_factor(),
        factors,
    })
}

/// Starts a session for `user_id` authenticated with `amr`, replacing any session cookie.
pub async fn start(
    state: &AppState,
    jar: CookieJar,
    user_id: Uuid,
    amr: &[&str],
) -> ApiResult<(CookieJar, Session)> {
    let jar = end(state, jar).await?;
    let token = crypto::random_token();
    let id_hash = crypto::hash_token(&token);
    let amr: Vec<String> = amr.iter().map(|method| (*method).to_owned()).collect();
    let (email, auth_time): (String, OffsetDateTime) = sqlx::query_as(
        "WITH created AS (
             INSERT INTO sessions (id_hash, user_id, csrf_token_hash, auth_time, amr, expires_at)
             VALUES ($1, $2, $3, now(), $4, now() + make_interval(days => $5))
             RETURNING auth_time
         )
         SELECT users.email, created.auth_time FROM created, users WHERE users.id = $2",
    )
    .bind(&id_hash)
    .bind(user_id)
    // Unused: writes are protected by the Origin check and SameSite. The column is NOT NULL.
    .bind(crypto::hash_token(&crypto::random_token()))
    .bind(&amr)
    .bind(SESSION_DAYS as i32)
    .fetch_one(&state.db)
    .await?;

    let cookie = Cookie::build((COOKIE, token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(state.config.secure_cookies)
        .path(state.config.cookie_path.clone())
        .max_age(time::Duration::days(SESSION_DAYS))
        .build();
    Ok((
        jar.add(cookie),
        Session {
            id_hash,
            user_id,
            email,
            amr,
            auth_time,
        },
    ))
}

pub async fn load(state: &AppState, jar: &CookieJar) -> sqlx::Result<Option<Session>> {
    let Some(cookie) = jar.get(COOKIE) else {
        return Ok(None);
    };
    let id_hash = crypto::hash_token(cookie.value());
    let row: Option<(Uuid, String, Vec<String>, OffsetDateTime)> = sqlx::query_as(
        "SELECT sessions.user_id, users.email, sessions.amr, sessions.auth_time
         FROM sessions JOIN users ON users.id = sessions.user_id
         WHERE sessions.id_hash = $1 AND sessions.expires_at > now() AND users.disabled_at IS NULL",
    )
    .bind(&id_hash)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|(user_id, email, amr, auth_time)| Session {
        id_hash,
        user_id,
        email,
        amr,
        auth_time,
    }))
}

/// The signed-in session. Unless `allow_pending`, a session still waiting for its
/// second factor is rejected.
pub async fn require(state: &AppState, jar: &CookieJar, allow_pending: bool) -> ApiResult<Session> {
    let session = load(state, jar)
        .await?
        .ok_or_else(ApiError::not_signed_in)?;
    if !allow_pending
        && !session.has_second_factor()
        && factors(&state.db, session.user_id).await?.any()
    {
        return Err(ApiError::unauthorized(
            "Finish signing in with your second factor",
            "SECOND_FACTOR_PENDING",
        ));
    }
    Ok(session)
}

/// Records a completed authentication method and refreshes `auth_time`.
pub async fn add_method(
    state: &AppState,
    session: &Session,
    method: &str,
) -> sqlx::Result<Session> {
    let (amr, auth_time): (Vec<String>, OffsetDateTime) = sqlx::query_as(
        "UPDATE sessions
         SET amr = CASE WHEN $2 = ANY (amr) THEN amr ELSE array_append(amr, $2) END,
             auth_time = now(), updated_at = now()
         WHERE id_hash = $1
         RETURNING amr, auth_time",
    )
    .bind(&session.id_hash)
    .bind(method)
    .fetch_one(&state.db)
    .await?;
    Ok(Session {
        amr,
        auth_time,
        ..session.clone()
    })
}

pub async fn end(state: &AppState, jar: CookieJar) -> sqlx::Result<CookieJar> {
    if let Some(cookie) = jar.get(COOKIE) {
        sqlx::query("DELETE FROM sessions WHERE id_hash = $1")
            .bind(crypto::hash_token(cookie.value()))
            .execute(&state.db)
            .await?;
    }
    Ok(jar.remove(Cookie::build(COOKIE).path(state.config.cookie_path.clone())))
}
