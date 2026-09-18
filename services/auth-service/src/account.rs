//! Sign-up, password sign-in, the current session, sign-out, and the Origin
//! check that guards every mutating `/v1` call.

use std::sync::LazyLock;

use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    AppState, crypto,
    error::{ApiError, ApiResult},
    session::{self, SessionView},
};

const MIN_PASSWORD_CHARS: usize = 12;
const MAX_PASSWORD_BYTES: usize = 1024;

/// Verified against when the email is unknown, so response time doesn't reveal which emails exist.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    crypto::hash_password("not a real password").expect("hashing the dummy password")
});

#[derive(Deserialize)]
pub struct Credentials {
    email: String,
    password: String,
}

/// Mutating calls must come from one of our own pages (CSRF protection alongside SameSite cookies).
pub async fn origin_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        let allowed = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|origin| origin.to_str().ok())
            .is_some_and(|origin| {
                state
                    .config
                    .allowed_origins
                    .iter()
                    .any(|allowed| allowed == origin)
            });
        if !allowed {
            return ApiError::forbidden(
                "This request didn't come from a Drone Drop page",
                "ORIGIN_NOT_ALLOWED",
            )
            .into_response();
        }
    }
    next.run(request).await
}

pub async fn sign_up(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<Credentials>,
) -> ApiResult<(StatusCode, CookieJar, Json<SessionView>)> {
    let email = valid_email(&input.email).ok_or(ApiError::bad_request(
        "Enter a valid email address",
        "INVALID_EMAIL",
    ))?;
    if input.password.chars().count() < MIN_PASSWORD_CHARS
        || input.password.len() > MAX_PASSWORD_BYTES
    {
        return Err(ApiError::bad_request(
            "Use a password of at least 12 characters",
            "WEAK_PASSWORD",
        ));
    }
    let password = input.password;
    let password_hash = tokio::task::spawn_blocking(move || crypto::hash_password(&password))
        .await
        .map_err(ApiError::internal)??;

    let user_id = Uuid::now_v7();
    let inserted = sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(&email)
        .bind(&password_hash)
        .execute(&state.db)
        .await;
    if let Err(sqlx::Error::Database(error)) = &inserted
        && error.is_unique_violation()
    {
        return Err(ApiError::conflict(
            "An account with that email already exists",
            "EMAIL_TAKEN",
        ));
    }
    inserted?;

    let (jar, session) = session::start(&state, jar, user_id, &["pwd"]).await?;
    Ok((
        StatusCode::CREATED,
        jar,
        Json(session::view(&state.db, &session).await?),
    ))
}

pub async fn sign_in_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<Credentials>,
) -> ApiResult<(CookieJar, Json<SessionView>)> {
    let email = input.email.trim().to_lowercase();
    let account_key = format!("password:{email}");
    let ip_key = format!("password-ip:{}", client_ip(&headers));
    let keys = [account_key.as_str(), ip_key.as_str()];
    if !state.limiter.allowed(&keys) {
        return Err(ApiError::too_many_attempts());
    }

    let user: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, password_hash FROM users WHERE lower(email) = $1 AND disabled_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;
    let (user_id, hash) = match user {
        Some((id, hash)) => (Some(id), hash),
        None => (None, DUMMY_HASH.clone()),
    };
    let password = input.password;
    let password_ok =
        tokio::task::spawn_blocking(move || crypto::verify_password(&hash, &password))
            .await
            .map_err(ApiError::internal)?;

    match (password_ok, user_id) {
        (true, Some(user_id)) => {
            state.limiter.clear(&account_key);
            let (jar, session) = session::start(&state, jar, user_id, &["pwd"]).await?;
            Ok((jar, Json(session::view(&state.db, &session).await?)))
        }
        _ => {
            state.limiter.record_failure(&keys);
            Err(ApiError::unauthorized(
                "That email and password don't match",
                "INVALID_CREDENTIALS",
            ))
        }
    }
}

pub async fn get_session(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<SessionView>> {
    let session = session::require(&state, &jar, true).await?;
    Ok(Json(session::view(&state.db, &session).await?))
}

pub async fn sign_out(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<(StatusCode, CookieJar)> {
    Ok((StatusCode::NO_CONTENT, session::end(&state, jar).await?))
}

pub(crate) fn valid_email(email: &str) -> Option<String> {
    let email = email.trim();
    let (local, domain) = email.split_once('@')?;
    (!local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && email.len() <= 254
        && !email.contains(char::is_whitespace))
    .then(|| email.to_owned())
}

/// The caller's IP as reported by ui-gateway, for per-IP attempt limits.
pub fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|ip| ip.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_validation_is_lenient_but_sane() {
        assert_eq!(
            valid_email("  Jo@Example.com ").as_deref(),
            Some("Jo@Example.com")
        );
        assert!(valid_email("jo@localhost").is_none());
        assert!(valid_email("@example.com").is_none());
        assert!(valid_email("jo example@example.com").is_none());
    }
}
