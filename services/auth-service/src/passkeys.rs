//! Passkeys (WebAuthn), the only second factor: adding one to an account,
//! signing in with one without typing an email (discoverable credentials), and
//! finishing a password sign-in with one.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use webauthn_rs::prelude::{
    AuthenticationResult, CredentialID, DiscoverableAuthentication, DiscoverableKey, Passkey,
    PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential,
};

use crate::{
    AppState,
    account::client_ip,
    crypto,
    error::{ApiError, ApiResult},
    session::{self, FRESH_MFA_SECS, SessionView},
};

/// How long the browser has to complete a WebAuthn ceremony.
const CHALLENGE_TTL_SECS: i32 = 300;

const fn challenge_expired() -> ApiError {
    ApiError::bad_request("That took too long. Please try again.", "CHALLENGE_EXPIRED")
}

async fn store_challenge(
    state: &AppState,
    user_id: Option<Uuid>,
    kind: &str,
    ceremony: &impl Serialize,
) -> ApiResult<String> {
    let challenge_id = crypto::random_token();
    sqlx::query(
        "INSERT INTO webauthn_challenges (id_hash, user_id, kind, state, expires_at)
         VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5))",
    )
    .bind(crypto::hash_token(&challenge_id))
    .bind(user_id)
    .bind(kind)
    .bind(sqlx::types::Json(ceremony))
    .bind(CHALLENGE_TTL_SECS)
    .execute(&state.db)
    .await?;
    Ok(challenge_id)
}

/// Takes (and deletes) a ceremony, so each challenge is used at most once.
async fn take_challenge(
    state: &AppState,
    challenge_id: &str,
    kind: &str,
) -> ApiResult<(Option<Uuid>, Value)> {
    let row: Option<(Option<Uuid>, Value)> = sqlx::query_as(
        "DELETE FROM webauthn_challenges WHERE id_hash = $1 AND kind = $2 AND expires_at > now()
         RETURNING user_id, state",
    )
    .bind(crypto::hash_token(challenge_id))
    .bind(kind)
    .fetch_optional(&state.db)
    .await?;
    row.ok_or(challenge_expired())
}

/// The passkeys of an account that isn't disabled, with their row ids.
async fn user_passkeys(state: &AppState, user_id: Uuid) -> sqlx::Result<Vec<(Uuid, Passkey)>> {
    let rows: Vec<(Uuid, sqlx::types::Json<Passkey>)> = sqlx::query_as(
        "SELECT passkeys.id, passkeys.passkey FROM passkeys JOIN users ON users.id = passkeys.user_id
         WHERE passkeys.user_id = $1 AND users.disabled_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, passkey)| (id, passkey.0))
        .collect())
}

/// Keeps the stored signature counter and backup state of the passkey that was used current.
async fn record_use(
    state: &AppState,
    passkeys: Vec<(Uuid, Passkey)>,
    result: &AuthenticationResult,
) -> sqlx::Result<()> {
    let Some((id, mut passkey)) = passkeys
        .into_iter()
        .find(|(_, passkey)| passkey.cred_id() == result.cred_id())
    else {
        return Ok(());
    };
    passkey.update_credential(result);
    sqlx::query(
        "UPDATE passkeys SET passkey = $2, last_used_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(sqlx::types::Json(&passkey))
    .execute(&state.db)
    .await?;
    Ok(())
}

pub async fn registration_options(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<Value>> {
    let session = session::require(&state, &jar, false).await?;
    let existing: Vec<(Vec<u8>,)> =
        sqlx::query_as("SELECT credential_id FROM passkeys WHERE user_id = $1")
            .bind(session.user_id)
            .fetch_all(&state.db)
            .await?;
    let exclude: Vec<CredentialID> = existing
        .into_iter()
        .map(|(id,)| CredentialID::from(id))
        .collect();
    let (options, registration) = state
        .webauthn
        .start_passkey_registration(
            session.user_id,
            &session.email,
            &session.email,
            Some(exclude),
        )
        .map_err(ApiError::internal)?;
    let challenge_id =
        store_challenge(&state, Some(session.user_id), "REGISTRATION", &registration).await?;
    Ok(Json(
        json!({ "challenge_id": challenge_id, "options": options.public_key }),
    ))
}

#[derive(Deserialize)]
pub struct NewPasskey {
    challenge_id: String,
    name: String,
    credential: RegisterPublicKeyCredential,
}

pub async fn add(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<NewPasskey>,
) -> ApiResult<(StatusCode, Json<SessionView>)> {
    let session = session::require(&state, &jar, false).await?;
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(ApiError::bad_request(
            "Give the passkey a name of up to 60 characters",
            "INVALID_NAME",
        ));
    }

    let (user_id, ceremony) = take_challenge(&state, &input.challenge_id, "REGISTRATION").await?;
    if user_id != Some(session.user_id) {
        return Err(challenge_expired());
    }
    let registration: PasskeyRegistration =
        serde_json::from_value(ceremony).map_err(ApiError::internal)?;
    let passkey = state
        .webauthn
        .finish_passkey_registration(&input.credential, &registration)
        .map_err(|error| {
            tracing::info!(%error, "passkey registration rejected");
            ApiError::bad_request(
                "Your device couldn't create the passkey",
                "PASSKEY_REJECTED",
            )
        })?;

    let credential_id: &[u8] = passkey.cred_id().as_ref();
    let inserted = sqlx::query("INSERT INTO passkeys (id, user_id, credential_id, passkey, name) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::now_v7())
        .bind(session.user_id)
        .bind(credential_id)
        .bind(sqlx::types::Json(&passkey))
        .bind(name)
        .execute(&state.db)
        .await;
    if let Err(sqlx::Error::Database(error)) = &inserted
        && error.is_unique_violation()
    {
        return Err(ApiError::conflict(
            "That passkey is already registered",
            "PASSKEY_EXISTS",
        ));
    }
    inserted?;

    // Creating a passkey requires user verification, so it counts as using it.
    let session = session::add_method(&state, &session, "hwk").await?;
    Ok((
        StatusCode::CREATED,
        Json(session::view(&state.db, &session).await?),
    ))
}

pub async fn list(State(state): State<AppState>, jar: CookieJar) -> ApiResult<Json<Value>> {
    let session = session::require(&state, &jar, false).await?;
    let rows: Vec<(Uuid, String, OffsetDateTime, Option<OffsetDateTime>)> =
        sqlx::query_as("SELECT id, name, created_at, last_used_at FROM passkeys WHERE user_id = $1 ORDER BY created_at")
            .bind(session.user_id)
            .fetch_all(&state.db)
            .await?;
    let rfc3339 = |t: OffsetDateTime| t.format(&Rfc3339).unwrap_or_default();
    let passkeys: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, created_at, last_used_at)| {
            json!({ "passkey_id": id, "name": name, "created_at": rfc3339(created_at), "last_used_at": last_used_at.map(rfc3339) })
        })
        .collect();
    Ok(Json(json!({ "passkeys": passkeys })))
}

pub async fn remove(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(passkey_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let session = session::require(&state, &jar, false).await?;
    if !session.has_fresh_second_factor(FRESH_MFA_SECS) {
        return Err(ApiError::fresh_mfa_required());
    }
    let deleted = sqlx::query("DELETE FROM passkeys WHERE id = $1 AND user_id = $2")
        .bind(passkey_id)
        .bind(session.user_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("Passkey not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn sign_in_options(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let (options, authentication) = state
        .webauthn
        .start_discoverable_authentication()
        .map_err(ApiError::internal)?;
    let challenge_id = store_challenge(&state, None, "AUTHENTICATION", &authentication).await?;
    Ok(Json(
        json!({ "challenge_id": challenge_id, "options": options.public_key }),
    ))
}

#[derive(Deserialize)]
pub struct PasskeySignIn {
    challenge_id: String,
    credential: PublicKeyCredential,
}

pub async fn sign_in(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<PasskeySignIn>,
) -> ApiResult<(CookieJar, Json<SessionView>)> {
    let rejected = || {
        ApiError::unauthorized(
            "That passkey isn't registered with Drone Drop",
            "PASSKEY_REJECTED",
        )
    };

    let (_, ceremony) = take_challenge(&state, &input.challenge_id, "AUTHENTICATION").await?;
    let authentication: DiscoverableAuthentication =
        serde_json::from_value(ceremony).map_err(ApiError::internal)?;
    let (user_id, _) = state
        .webauthn
        .identify_discoverable_authentication(&input.credential)
        .map_err(|_| rejected())?;

    let passkeys = user_passkeys(&state, user_id).await?;
    let keys: Vec<DiscoverableKey> = passkeys
        .iter()
        .map(|(_, passkey)| DiscoverableKey::from(passkey))
        .collect();
    let result = state
        .webauthn
        .finish_discoverable_authentication(&input.credential, authentication, &keys)
        .map_err(|_| rejected())?;
    record_use(&state, passkeys, &result).await?;

    // A passkey is possession plus user verification: both factors at once.
    let (jar, session) = session::start(&state, jar, user_id, &["hwk"]).await?;
    Ok((jar, Json(session::view(&state.db, &session).await?)))
}

/// Starts checking one of the signed-in account's passkeys, to finish a password sign-in
/// or to step up.
pub async fn second_factor_options(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<Value>> {
    let session = session::require(&state, &jar, true).await?;
    let passkeys: Vec<Passkey> = user_passkeys(&state, session.user_id)
        .await?
        .into_iter()
        .map(|(_, passkey)| passkey)
        .collect();
    if passkeys.is_empty() {
        return Err(ApiError::bad_request(
            "No passkey is set up on this account",
            "NO_PASSKEYS",
        ));
    }
    let (options, authentication) = state
        .webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(ApiError::internal)?;
    let challenge_id = store_challenge(
        &state,
        Some(session.user_id),
        "AUTHENTICATION",
        &authentication,
    )
    .await?;
    Ok(Json(
        json!({ "challenge_id": challenge_id, "options": options.public_key }),
    ))
}

pub async fn verify_second_factor(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<PasskeySignIn>,
) -> ApiResult<Json<SessionView>> {
    let session = session::require(&state, &jar, true).await?;
    let account_key = format!("passkey:{}", session.user_id);
    let ip_key = format!("passkey-ip:{}", client_ip(&headers));
    let keys = [account_key.as_str(), ip_key.as_str()];
    if !state.limiter.allowed(&keys) {
        return Err(ApiError::too_many_attempts());
    }

    // Only a challenge started for this account; passwordless sign-in challenges have no user.
    let (user_id, ceremony) = take_challenge(&state, &input.challenge_id, "AUTHENTICATION").await?;
    if user_id != Some(session.user_id) {
        return Err(challenge_expired());
    }
    let authentication: PasskeyAuthentication =
        serde_json::from_value(ceremony).map_err(ApiError::internal)?;
    let result = match state
        .webauthn
        .finish_passkey_authentication(&input.credential, &authentication)
    {
        Ok(result) => result,
        Err(error) => {
            tracing::info!(%error, "passkey second factor rejected");
            state.limiter.record_failure(&keys);
            return Err(ApiError::unauthorized(
                "That passkey isn't registered to this account",
                "PASSKEY_REJECTED",
            ));
        }
    };
    record_use(&state, user_passkeys(&state, session.user_id).await?, &result).await?;

    state.limiter.clear(&account_key);
    let session = session::add_method(&state, &session, "hwk").await?;
    Ok(Json(session::view(&state.db, &session).await?))
}
