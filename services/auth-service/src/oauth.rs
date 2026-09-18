//! OAuth 2.1 endpoints: metadata, JWKS, `/authorize`, `/token` and `/revoke`.

use axum::{
    Form, Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use contracts::{dronedrop::events::v1::GrantRevoked, subjects};
use prost::Message;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AppState, crypto,
    flow::{self, ParkedRequest, redirect_with},
    registry::{self, Client},
};

const ACCESS_TOKEN_TTL_SECS: i64 = 3600;
const PUBLIC_REFRESH_DAYS: i32 = 30;
/// Confidential clients (Alexa) keep one refresh token whose expiry slides on every use.
const CONFIDENTIAL_REFRESH_DAYS: i32 = 90;
/// A rotated refresh token may be presented again this long (parallel refreshes).
/// Later reuse suggests a stolen token, and revokes the grant.
const REUSE_WINDOW_SECS: i64 = 60;

pub async fn metadata(State(state): State<AppState>) -> Json<Value> {
    let issuer = &state.config.issuer;
    let mut scopes: Vec<&str> = state
        .resources
        .iter()
        .flat_map(|r| r.scopes.iter().copied())
        .collect();
    scopes.sort_unstable();
    scopes.dedup();
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "revocation_endpoint": format!("{issuer}/revoke"),
        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_basic", "client_secret_post"],
        "scopes_supported": scopes,
        "acr_values_supported": ["mfa"],
        "authorization_response_iss_parameter_supported": true,
    }))
}

pub async fn jwks(State(state): State<AppState>) -> Json<Value> {
    Json(state.keys.jwks())
}

#[derive(Deserialize)]
pub struct AuthorizeQuery {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    resource: Option<String>,
    acr_values: Option<String>,
    max_age: Option<i64>,
}

pub async fn authorize(
    State(state): State<AppState>,
    Query(query): Query<AuthorizeQuery>,
) -> Response {
    match authorize_request(&state, query).await {
        Ok(response) | Err(response) => response,
    }
}

async fn authorize_request(state: &AppState, q: AuthorizeQuery) -> Result<Response, Response> {
    // Until the client and redirect URI check out, errors are shown here rather than
    // redirected, so an attacker can't bounce users to an arbitrary address.
    let client = match q.client_id.as_deref() {
        Some(client_id) => registry::find_client(&state.db, client_id)
            .await
            .map_err(server_error_page)?,
        None => None,
    }
    .ok_or_else(|| error_page("This app isn't registered with Drone Drop."))?;
    let redirect_uri = q
        .redirect_uri
        .clone()
        .filter(|uri| client.redirect_uris.contains(uri))
        .ok_or_else(|| error_page("This app's return address isn't registered with Drone Drop."))?;

    let fail = |error: &str, description: &str| {
        let mut params = vec![
            ("error", error),
            ("error_description", description),
            ("iss", state.config.issuer.as_str()),
        ];
        if let Some(client_state) = &q.state {
            params.push(("state", client_state));
        }
        Redirect::to(&redirect_with(&redirect_uri, &params)).into_response()
    };

    if q.response_type.as_deref() != Some("code") {
        return Err(fail(
            "unsupported_response_type",
            "Only the authorization code flow is supported",
        ));
    }
    let challenge = q.code_challenge.as_deref().unwrap_or_default();
    let pkce_ok = q.code_challenge_method.as_deref() == Some("S256")
        && challenge.len() == 43
        && challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !pkce_ok {
        return Err(fail("invalid_request", "PKCE with S256 is required"));
    }

    // Alexa may omit `resource`; it only ever needs the MCP server.
    let resource_uri = q.resource.clone().or_else(|| {
        (client.client_id == "alexa").then(|| state.config.mcp_resource_url.clone())
    });
    let Some(resource) = resource_uri
        .as_deref()
        .and_then(|uri| registry::find_resource(&state.resources, uri))
    else {
        return Err(fail("invalid_target", "Unknown resource"));
    };
    let requested: Vec<&str> = match q.scope.as_deref() {
        Some(scope) if !scope.trim().is_empty() => scope.split_whitespace().collect(),
        _ => resource.scopes.to_vec(),
    };
    if requested
        .iter()
        .any(|scope| !resource.scopes.contains(scope))
    {
        return Err(fail(
            "invalid_scope",
            "A requested scope isn't available for this resource",
        ));
    }

    let parked = ParkedRequest {
        client_id: client.client_id.clone(),
        redirect_uri: redirect_uri.clone(),
        scope: requested.join(" "),
        state: q.state.clone(),
        code_challenge: challenge.to_owned(),
        resource: resource.uri.clone(),
        step_up: q
            .acr_values
            .as_deref()
            .is_some_and(|values| values.split_whitespace().any(|v| v == "mfa")),
        max_age: q.max_age,
    };
    let request_id = flow::park(state, &parked)
        .await
        .map_err(server_error_page)?;
    Ok(Redirect::to(&format!(
        "{}/sign-in?request={request_id}",
        state.config.ui_url
    ))
    .into_response())
}

fn error_page(message: &'static str) -> Response {
    let body = format!(
        "<!doctype html><meta name=viewport content=\"width=device-width\"><title>Drone Drop sign-in</title>\
         <body style=\"font-family:system-ui;margin:3rem auto;max-width:28rem;padding:0 1rem\">\
         <h1>We couldn't start signing in</h1><p>{message}</p></body>"
    );
    (StatusCode::BAD_REQUEST, Html(body)).into_response()
}

fn server_error_page(error: impl std::fmt::Display) -> Response {
    tracing::error!(%error, "authorize failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Html("<!doctype html><title>Drone Drop</title><p>Something went wrong.</p>"),
    )
        .into_response()
}

/// An RFC 6749 error response from `/token`.
#[derive(Debug)]
pub struct OAuthError {
    status: StatusCode,
    error: &'static str,
    description: &'static str,
}

impl OAuthError {
    fn invalid_grant(description: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: "invalid_grant",
            description,
        }
    }

    fn invalid_request(description: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: "invalid_request",
            description,
        }
    }

    fn invalid_client() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            error: "invalid_client",
            description: "Client authentication failed",
        }
    }

    fn server_error(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "token endpoint failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: "server_error",
            description: "Something went wrong",
        }
    }
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.error, "error_description": self.description }));
        (self.status, [(header::CACHE_CONTROL, "no-store")], body).into_response()
    }
}

impl From<sqlx::Error> for OAuthError {
    fn from(error: sqlx::Error) -> Self {
        Self::server_error(error)
    }
}

#[derive(Deserialize)]
pub struct TokenForm {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
}

pub async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Response {
    let result = async {
        let client = authenticate_client(&state, &headers, &form).await?;
        match form.grant_type.as_str() {
            "authorization_code" => exchange_code(&state, &client, &form).await,
            "refresh_token" => refresh(&state, &client, &form).await,
            _ => Err(OAuthError {
                status: StatusCode::BAD_REQUEST,
                error: "unsupported_grant_type",
                description: "Use authorization_code or refresh_token",
            }),
        }
    }
    .await;
    match result {
        Ok(tokens) => ([(header::CACHE_CONTROL, "no-store")], Json(tokens)).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn authenticate_client(
    state: &AppState,
    headers: &HeaderMap,
    form: &TokenForm,
) -> Result<Client, OAuthError> {
    let (client_id, secret) = match basic_auth(headers) {
        Some((client_id, secret)) => (client_id, Some(secret)),
        None => (
            form.client_id
                .clone()
                .ok_or_else(OAuthError::invalid_client)?,
            form.client_secret.clone(),
        ),
    };
    let client = registry::find_client(&state.db, &client_id)
        .await?
        .ok_or_else(OAuthError::invalid_client)?;
    match (&client.secret_hash, secret) {
        (None, None) => Ok(client),
        (Some(expected), Some(secret))
            if crypto::hashes_match(expected, &crypto::hash_token(&secret)) =>
        {
            Ok(client)
        }
        _ => Err(OAuthError::invalid_client()),
    }
}

/// `Authorization: Basic` client credentials, form-decoded as RFC 6749 §2.3.1 requires.
fn basic_auth(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = String::from_utf8(STANDARD.decode(encoded.trim()).ok()?).ok()?;
    let (client_id, secret) = decoded.split_once(':')?;
    let form_decode = |value: &str| {
        url::form_urlencoded::parse(format!("v={value}").as_bytes())
            .next()
            .map(|(_, v)| v.into_owned())
    };
    Some((form_decode(client_id)?, form_decode(secret)?))
}

#[derive(sqlx::FromRow)]
pub(crate) struct Grant {
    id: Uuid,
    user_id: Uuid,
    client_id: String,
    resource: String,
    scope: String,
    acr: String,
    amr: Vec<String>,
    auth_time: OffsetDateTime,
    revoked_at: Option<OffsetDateTime>,
}

async fn load_grant(tx: &mut Transaction<'_, Postgres>, grant_id: Uuid) -> sqlx::Result<Grant> {
    sqlx::query_as("SELECT id, user_id, client_id, resource, scope, acr, amr, auth_time, revoked_at FROM grants WHERE id = $1")
        .bind(grant_id)
        .fetch_one(&mut **tx)
        .await
}

async fn exchange_code(
    state: &AppState,
    client: &Client,
    form: &TokenForm,
) -> Result<Value, OAuthError> {
    let code = form
        .code
        .as_deref()
        .ok_or(OAuthError::invalid_request("code is required"))?;
    let verifier = form
        .code_verifier
        .as_deref()
        .ok_or(OAuthError::invalid_request("code_verifier is required"))?;
    let code_hash = crypto::hash_token(code);

    let mut tx = state.db.begin().await?;
    let row: Option<(Uuid, String, String, bool, bool)> = sqlx::query_as(
        "SELECT grant_id, redirect_uri, code_challenge, consumed_at IS NOT NULL, expires_at <= now()
         FROM auth_codes WHERE code_hash = $1 FOR UPDATE",
    )
    .bind(&code_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((grant_id, redirect_uri, challenge, consumed, expired)) = row else {
        return Err(OAuthError::invalid_grant("Unknown authorization code"));
    };
    let grant = load_grant(&mut tx, grant_id).await?;

    if consumed {
        // A code presented twice was probably intercepted: revoke everything it issued.
        revoke_grant(&mut tx, &grant).await?;
        tx.commit().await?;
        return Err(OAuthError::invalid_grant("Authorization code already used"));
    }
    if expired || grant.revoked_at.is_some() || grant.client_id != client.client_id {
        return Err(OAuthError::invalid_grant(
            "Authorization code is invalid or expired",
        ));
    }
    if form.redirect_uri.as_deref() != Some(redirect_uri.as_str()) {
        return Err(OAuthError::invalid_grant(
            "redirect_uri doesn't match the authorization request",
        ));
    }
    if !crypto::hashes_match(&crypto::pkce_s256(verifier), &challenge) {
        return Err(OAuthError::invalid_grant("PKCE verification failed"));
    }

    sqlx::query("UPDATE auth_codes SET consumed_at = now() WHERE code_hash = $1")
        .bind(&code_hash)
        .execute(&mut *tx)
        .await?;
    let refresh_token = new_refresh_token(&mut tx, grant.id, client).await?;
    tx.commit().await?;
    token_response(state, &grant, refresh_token)
}

async fn refresh(state: &AppState, client: &Client, form: &TokenForm) -> Result<Value, OAuthError> {
    let token = form
        .refresh_token
        .as_deref()
        .ok_or(OAuthError::invalid_request("refresh_token is required"))?;
    let token_hash = crypto::hash_token(token);

    let mut tx = state.db.begin().await?;
    let row: Option<(Uuid, bool, Option<i64>)> = sqlx::query_as(
        "SELECT grant_id, expires_at <= now() OR revoked_at IS NOT NULL,
                extract(epoch FROM now() - rotated_at)::bigint
         FROM refresh_tokens WHERE token_hash = $1 FOR UPDATE",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((grant_id, dead, rotated_secs_ago)) = row else {
        return Err(OAuthError::invalid_grant("Unknown refresh token"));
    };
    let grant = load_grant(&mut tx, grant_id).await?;
    if dead || grant.revoked_at.is_some() || grant.client_id != client.client_id {
        return Err(OAuthError::invalid_grant(
            "Refresh token is invalid or expired",
        ));
    }

    let refresh_token = if client.secret_hash.is_some() {
        // Alexa's account link must never break: slide the expiry, keep the token.
        sqlx::query("UPDATE refresh_tokens SET expires_at = now() + make_interval(days => $2) WHERE token_hash = $1")
            .bind(&token_hash)
            .bind(CONFIDENTIAL_REFRESH_DAYS)
            .execute(&mut *tx)
            .await?;
        token.to_owned()
    } else {
        if rotated_secs_ago.is_some_and(|secs| secs > REUSE_WINDOW_SECS) {
            revoke_grant(&mut tx, &grant).await?;
            tx.commit().await?;
            return Err(OAuthError::invalid_grant("Refresh token was already used"));
        }
        sqlx::query("UPDATE refresh_tokens SET rotated_at = COALESCE(rotated_at, now()) WHERE token_hash = $1")
            .bind(&token_hash)
            .execute(&mut *tx)
            .await?;
        new_refresh_token(&mut tx, grant.id, client).await?
    };
    tx.commit().await?;
    token_response(state, &grant, refresh_token)
}

async fn new_refresh_token(
    tx: &mut Transaction<'_, Postgres>,
    grant_id: Uuid,
    client: &Client,
) -> sqlx::Result<String> {
    let token = crypto::random_token();
    let days = if client.secret_hash.is_some() {
        CONFIDENTIAL_REFRESH_DAYS
    } else {
        PUBLIC_REFRESH_DAYS
    };
    sqlx::query("INSERT INTO refresh_tokens (token_hash, grant_id, expires_at) VALUES ($1, $2, now() + make_interval(days => $3))")
        .bind(crypto::hash_token(&token))
        .bind(grant_id)
        .bind(days)
        .execute(&mut **tx)
        .await?;
    Ok(token)
}

fn token_response(
    state: &AppState,
    grant: &Grant,
    refresh_token: String,
) -> Result<Value, OAuthError> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let access_token = state
        .keys
        .sign(&json!({
            "iss": state.config.issuer,
            "sub": grant.user_id,
            "aud": grant.resource,
            "client_id": grant.client_id,
            "scope": grant.scope,
            "iat": now,
            "exp": now + ACCESS_TOKEN_TTL_SECS,
            "jti": Uuid::now_v7(),
            "gid": grant.id,
            "acr": grant.acr,
            "amr": grant.amr,
            "auth_time": grant.auth_time.unix_timestamp(),
        }))
        .map_err(OAuthError::server_error)?;
    Ok(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": ACCESS_TOKEN_TTL_SECS,
        "refresh_token": refresh_token,
        "scope": grant.scope,
    }))
}

/// Revokes a grant and its refresh tokens, and queues `auth.events.grant_revoked`
/// so resource servers reject its access tokens too.
pub(crate) async fn revoke_grant(
    tx: &mut Transaction<'_, Postgres>,
    grant: &Grant,
) -> sqlx::Result<()> {
    let revoked =
        sqlx::query("UPDATE grants SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
            .bind(grant.id)
            .execute(&mut **tx)
            .await?;
    if revoked.rows_affected() == 0 {
        return Ok(());
    }
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE grant_id = $1 AND revoked_at IS NULL",
    )
    .bind(grant.id)
    .execute(&mut **tx)
    .await?;

    let event_id = Uuid::now_v7();
    let event = GrantRevoked {
        event_id: event_id.to_string(),
        grant_id: grant.id.to_string(),
        sub: grant.user_id.to_string(),
        client_id: grant.client_id.clone(),
        resource: grant.resource.clone(),
        revoked_at: Some(prost_types::Timestamp {
            seconds: OffsetDateTime::now_utc().unix_timestamp(),
            nanos: 0,
        }),
    };
    svc_common::outbox::enqueue(
        tx,
        event_id,
        subjects::AUTH_GRANT_REVOKED,
        &event.encode_to_vec(),
    )
    .await
}

#[derive(Deserialize)]
pub struct RevokeForm {
    token: String,
}

/// RFC 7009: always `200`, whether or not the token was known.
pub async fn revoke(State(state): State<AppState>, Form(form): Form<RevokeForm>) -> Response {
    let result: sqlx::Result<()> = async {
        let mut tx = state.db.begin().await?;
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT grant_id FROM refresh_tokens WHERE token_hash = $1")
                .bind(crypto::hash_token(&form.token))
                .fetch_optional(&mut *tx)
                .await?;
        if let Some((grant_id,)) = row {
            let grant = load_grant(&mut tx, grant_id).await?;
            revoke_grant(&mut tx, &grant).await?;
        }
        tx.commit().await
    }
    .await;
    match result {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => OAuthError::server_error(error).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_is_form_decoded() {
        let mut headers = HeaderMap::new();
        let encoded = STANDARD.encode("alexa:s%3Acret");
        headers.insert(
            header::AUTHORIZATION,
            format!("Basic {encoded}").parse().unwrap(),
        );
        assert_eq!(
            basic_auth(&headers),
            Some(("alexa".to_owned(), "s:cret".to_owned()))
        );

        headers.insert(header::AUTHORIZATION, "Bearer token".parse().unwrap());
        assert_eq!(basic_auth(&headers), None);
    }
}
