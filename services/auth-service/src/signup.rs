use axum::{Json, extract::State, http::StatusCode};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    AppState, account, crypto,
    error::{ApiError, ApiResult},
    mailer,
    session::{self, SessionView},
};

const MAX_OTP_ATTEMPTS: i32 = 5;

#[derive(Deserialize)]
pub struct EmailInput {
    email: String,
}

#[derive(Deserialize)]
pub struct ChallengeInput {
    challenge_id: String,
    email: String,
}

#[derive(Deserialize)]
pub struct VerifyInput {
    challenge_id: String,
    email: String,
    otp: String,
}

#[derive(Deserialize)]
pub struct FinalizeInput {
    challenge_id: String,
    email: String,
    password: String,
}

#[derive(Serialize)]
struct ChallengeView {
    challenge_id: String,
    expires_at: String,
}

pub async fn check_email(
    State(state): State<AppState>,
    Json(input): Json<EmailInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let email = normalize_email(&input.email)?;
    let (exists,): (bool,) =
        sqlx::query_as("SELECT EXISTS (SELECT 1 FROM users WHERE lower(email) = $1)")
            .bind(&email)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(json!({ "exists": exists })))
}

pub async fn start(
    State(state): State<AppState>,
    Json(input): Json<EmailInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let email = normalize_email(&input.email)?;
    ensure_new_email(&state, &email).await?;
    let challenge_id = crypto::random_token();
    let otp = otp();
    let expires_at = OffsetDateTime::now_utc() + time::Duration::seconds(state.config.otp_ttl_secs);
    sqlx::query("INSERT INTO signup_challenges (id_hash, email_hash, otp_hash, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(crypto::hash_token(&challenge_id)).bind(crypto::hash_token(&email)).bind(crypto::hash_token(&otp)).bind(expires_at)
        .execute(&state.db).await?;
    mailer::send_signup_otp(&state.config, &email, &otp)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!(ChallengeView {
        challenge_id,
        expires_at: expires_at.format(&Rfc3339).unwrap_or_default()
    })))
}

pub async fn resend(
    State(state): State<AppState>,
    Json(input): Json<ChallengeInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let email = normalize_email(&input.email)?;
    let row: Option<(OffsetDateTime,)> = sqlx::query_as("SELECT last_sent_at FROM signup_challenges WHERE id_hash = $1 AND email_hash = $2 AND expires_at > now()")
        .bind(crypto::hash_token(&input.challenge_id)).bind(crypto::hash_token(&email)).fetch_optional(&state.db).await?;
    let Some((last_sent_at,)) = row else {
        return Err(ApiError::not_found("Signup challenge expired"));
    };
    if (OffsetDateTime::now_utc() - last_sent_at).whole_seconds() < state.config.otp_resend_secs {
        return Err(ApiError::conflict(
            "Wait before requesting another code",
            "OTP_RESEND_TOO_SOON",
        ));
    }
    let otp = otp();
    let expires_at = OffsetDateTime::now_utc() + time::Duration::seconds(state.config.otp_ttl_secs);
    let updated = sqlx::query("UPDATE signup_challenges SET otp_hash = $3, expires_at = $4, attempts = 0, verified_at = NULL, last_sent_at = now() WHERE id_hash = $1 AND email_hash = $2 AND expires_at > now()")
        .bind(crypto::hash_token(&input.challenge_id)).bind(crypto::hash_token(&email)).bind(crypto::hash_token(&otp)).bind(expires_at).execute(&state.db).await?;
    if updated.rows_affected() != 1 {
        return Err(ApiError::not_found("Signup challenge expired"));
    }
    mailer::send_signup_otp(&state.config, &email, &otp)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!(ChallengeView {
        challenge_id: input.challenge_id,
        expires_at: expires_at.format(&Rfc3339).unwrap_or_default()
    })))
}

pub async fn verify(
    State(state): State<AppState>,
    Json(input): Json<VerifyInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let email = normalize_email(&input.email)?;
    let challenge_hash = crypto::hash_token(&input.challenge_id);
    let row: Option<(String, OffsetDateTime, i32, Option<OffsetDateTime>)> = sqlx::query_as("SELECT otp_hash, expires_at, attempts, verified_at FROM signup_challenges WHERE id_hash = $1 AND email_hash = $2")
        .bind(&challenge_hash).bind(crypto::hash_token(&email)).fetch_optional(&state.db).await?;
    let Some((otp_hash, expires_at, attempts, verified_at)) = row else {
        return Err(ApiError::not_found("Signup challenge not found"));
    };
    if expires_at <= OffsetDateTime::now_utc() {
        return Err(ApiError::conflict("That code has expired", "OTP_EXPIRED"));
    }
    if verified_at.is_some() {
        return Ok(Json(json!({ "verified": true })));
    }
    if attempts >= MAX_OTP_ATTEMPTS {
        return Err(ApiError::too_many_attempts());
    }
    if !crypto::hashes_match(&otp_hash, &crypto::hash_token(input.otp.trim())) {
        sqlx::query("UPDATE signup_challenges SET attempts = attempts + 1 WHERE id_hash = $1")
            .bind(&challenge_hash)
            .execute(&state.db)
            .await?;
        return Err(ApiError::bad_request(
            "That verification code is incorrect",
            "INVALID_OTP",
        ));
    }
    sqlx::query("UPDATE signup_challenges SET verified_at = now() WHERE id_hash = $1 AND verified_at IS NULL")
        .bind(&challenge_hash).execute(&state.db).await?;
    Ok(Json(json!({ "verified": true })))
}

pub async fn finalize(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<FinalizeInput>,
) -> ApiResult<(StatusCode, CookieJar, Json<SessionView>)> {
    let email = normalize_email(&input.email)?;
    if input.password.chars().count() < 12 || input.password.len() > 1024 {
        return Err(ApiError::bad_request(
            "Use a password of at least 12 characters",
            "WEAK_PASSWORD",
        ));
    }
    let challenge_hash = crypto::hash_token(&input.challenge_id);
    let verified: Option<(OffsetDateTime,)> = sqlx::query_as("SELECT verified_at FROM signup_challenges WHERE id_hash = $1 AND email_hash = $2 AND expires_at > now() AND verified_at IS NOT NULL")
        .bind(&challenge_hash).bind(crypto::hash_token(&email)).fetch_optional(&state.db).await?;
    if verified.is_none() {
        return Err(ApiError::conflict(
            "Verify your email before setting a password",
            "OTP_REQUIRED",
        ));
    }
    let password = input.password;
    let password_hash = tokio::task::spawn_blocking(move || crypto::hash_password(&password))
        .await
        .map_err(ApiError::internal)??;
    let user_id = Uuid::now_v7();
    let mut tx = state.db.begin().await?;
    let inserted = sqlx::query("INSERT INTO users (id, email, password_hash, email_verified_at) VALUES ($1, $2, $3, now())")
        .bind(user_id).bind(&email).bind(&password_hash).execute(&mut *tx).await;
    if let Err(sqlx::Error::Database(error)) = &inserted
        && error.is_unique_violation()
    {
        return Err(ApiError::conflict(
            "An account with that email already exists",
            "EMAIL_TAKEN",
        ));
    }
    inserted?;
    sqlx::query("DELETE FROM signup_challenges WHERE id_hash = $1")
        .bind(&challenge_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let (jar, session) = session::start(&state, jar, user_id, &["pwd"]).await?;
    Ok((
        StatusCode::CREATED,
        jar,
        Json(session::view(&state.db, &session).await?),
    ))
}

async fn ensure_new_email(state: &AppState, email: &str) -> ApiResult<()> {
    let (exists,): (bool,) =
        sqlx::query_as("SELECT EXISTS (SELECT 1 FROM users WHERE lower(email) = $1)")
            .bind(email)
            .fetch_one(&state.db)
            .await?;
    if exists {
        Err(ApiError::conflict(
            "An account with that email already exists. Sign in instead.",
            "EMAIL_TAKEN",
        ))
    } else {
        Ok(())
    }
}

fn normalize_email(value: &str) -> ApiResult<String> {
    let email = value.trim().to_lowercase();
    account::valid_email(&email).ok_or(ApiError::bad_request(
        "Enter a valid email address",
        "INVALID_EMAIL",
    ))
}

fn otp() -> String {
    let value = u32::from_le_bytes(crypto::random_bytes::<4>()) % 1_000_000;
    format!("{value:06}")
}
