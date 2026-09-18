//! Parked authorization requests: what each still needs, and finishing them
//! with an authorization code once the user is signed in.

use axum::{
    Json,
    extract::{Path, State},
};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    AppState, crypto,
    error::{ApiError, ApiResult},
    registry::{self, Client, MfaPolicy},
    session::{self, FRESH_MFA_SECS, Session, SessionView},
};

/// Authorization codes are exchanged immediately; they live one minute.
const CODE_TTL_SECS: i32 = 60;

/// A validated `/authorize` request, stored as `auth_requests.params`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParkedRequest {
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub state: Option<String>,
    pub code_challenge: String,
    pub resource: String,
    /// The client asked for a fresh second factor (`acr_values=mfa`).
    pub step_up: bool,
    pub max_age: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Next {
    SignIn,
    Mfa,
    MfaEnrollment,
    Consent,
    Complete,
}

#[derive(Clone, Copy, Debug)]
pub struct Situation {
    pub signed_in: bool,
    /// The account has a passkey.
    pub has_factors: bool,
    /// This session already verified a second factor.
    pub session_has_second_factor: bool,
    /// …within the request's `max_age`.
    pub second_factor_fresh: bool,
    pub mfa_policy: MfaPolicy,
    pub step_up: bool,
    pub first_party: bool,
}

/// What a request still needs, in the order the screens walk through it.
pub fn decide(s: Situation) -> Next {
    if !s.signed_in {
        return Next::SignIn;
    }
    if s.has_factors && !s.session_has_second_factor {
        return Next::Mfa;
    }
    let needs_mfa = s.step_up || s.mfa_policy == MfaPolicy::Always;
    if needs_mfa && !s.has_factors {
        return Next::MfaEnrollment;
    }
    if needs_mfa && !(s.session_has_second_factor && (s.second_factor_fresh || !s.step_up)) {
        return Next::Mfa;
    }
    if !s.first_party {
        return Next::Consent;
    }
    Next::Complete
}

pub async fn park(state: &AppState, request: &ParkedRequest) -> sqlx::Result<String> {
    let request_id = crypto::random_token();
    sqlx::query("INSERT INTO auth_requests (id_hash, params, expires_at) VALUES ($1, $2, now() + interval '10 minutes')")
        .bind(crypto::hash_token(&request_id))
        .bind(sqlx::types::Json(request))
        .execute(&state.db)
        .await?;
    Ok(request_id)
}

async fn load(state: &AppState, request_id: &str) -> ApiResult<(ParkedRequest, Client)> {
    let not_found =
        || ApiError::not_found("This sign-in link has expired. Start again from the app.");
    let row: Option<(sqlx::types::Json<ParkedRequest>,)> = sqlx::query_as(
        "SELECT params FROM auth_requests WHERE id_hash = $1 AND expires_at > now()",
    )
    .bind(crypto::hash_token(request_id))
    .fetch_optional(&state.db)
    .await?;
    let parked = row.ok_or_else(not_found)?.0.0;
    let client = registry::find_client(&state.db, &parked.client_id)
        .await?
        .ok_or_else(not_found)?;
    Ok((parked, client))
}

async fn evaluate(
    state: &AppState,
    parked: &ParkedRequest,
    client: &Client,
    session: Option<&Session>,
) -> ApiResult<(Next, Option<SessionView>)> {
    let Some(session) = session else {
        return Ok((Next::SignIn, None));
    };
    let view = session::view(&state.db, session).await?;
    let mfa_policy = registry::find_resource(&state.resources, &parked.resource)
        .map_or(MfaPolicy::None, |r| r.mfa);
    let next = decide(Situation {
        signed_in: true,
        has_factors: view.factors.any(),
        session_has_second_factor: session.has_second_factor(),
        second_factor_fresh: session
            .has_fresh_second_factor(parked.max_age.unwrap_or(FRESH_MFA_SECS)),
        mfa_policy,
        step_up: parked.step_up,
        first_party: client.first_party,
    });
    Ok((next, Some(view)))
}

pub async fn get_request(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(request_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (parked, client) = load(&state, &request_id).await?;
    let session = session::load(&state, &jar).await?;
    let (next, session_view) = evaluate(&state, &parked, &client, session.as_ref()).await?;
    let mfa_required = registry::find_resource(&state.resources, &parked.resource)
        .is_some_and(|r| r.mfa == MfaPolicy::Always);
    Ok(Json(json!({
        "request_id": request_id,
        "client": { "client_id": client.client_id, "name": client.name, "first_party": client.first_party },
        "resource": parked.resource,
        "scopes": parked.scope.split_whitespace().collect::<Vec<_>>(),
        "redirect_host": redirect_host(&parked.redirect_uri),
        "step_up": parked.step_up,
        "mfa_required": mfa_required,
        "next": next,
        "session": session_view,
    })))
}

#[derive(Deserialize)]
pub struct CompleteInput {
    #[serde(default)]
    consent: bool,
}

pub async fn complete(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(request_id): Path<String>,
    Json(input): Json<CompleteInput>,
) -> ApiResult<Json<Value>> {
    let (parked, client) = load(&state, &request_id).await?;
    let session = session::load(&state, &jar)
        .await?
        .ok_or_else(ApiError::not_signed_in)?;
    match evaluate(&state, &parked, &client, Some(&session)).await?.0 {
        Next::SignIn => return Err(ApiError::not_signed_in()),
        Next::Mfa => {
            return Err(ApiError::conflict(
                "Verify your second factor first",
                "MFA_REQUIRED",
            ));
        }
        Next::MfaEnrollment => {
            return Err(ApiError::conflict(
                "Set up a second factor first",
                "MFA_ENROLLMENT_REQUIRED",
            ));
        }
        Next::Consent if !input.consent => {
            return Err(ApiError::conflict(
                "Approve access for this app first",
                "CONSENT_REQUIRED",
            ));
        }
        Next::Consent | Next::Complete => {}
    }

    let code = crypto::random_token();
    let grant_id = Uuid::now_v7();
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO grants (id, user_id, client_id, resource, scope, acr, amr, auth_time)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(grant_id)
    .bind(session.user_id)
    .bind(&client.client_id)
    .bind(&parked.resource)
    .bind(&parked.scope)
    .bind(session.acr())
    .bind(&session.amr)
    .bind(session.auth_time)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO auth_codes (code_hash, grant_id, redirect_uri, redirect_uri_provided, scope, resource,
                                 code_challenge, acr, amr, auth_time, expires_at)
         VALUES ($1, $2, $3, true, $4, $5, $6, $7, $8, $9, now() + make_interval(secs => $10))",
    )
    .bind(crypto::hash_token(&code))
    .bind(grant_id)
    .bind(&parked.redirect_uri)
    .bind(&parked.scope)
    .bind(&parked.resource)
    .bind(&parked.code_challenge)
    .bind(session.acr())
    .bind(&session.amr)
    .bind(session.auth_time)
    .bind(CODE_TTL_SECS)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM auth_requests WHERE id_hash = $1")
        .bind(crypto::hash_token(&request_id))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let mut params = vec![
        ("code", code.as_str()),
        ("iss", state.config.issuer.as_str()),
    ];
    if let Some(client_state) = &parked.state {
        params.push(("state", client_state));
    }
    Ok(Json(
        json!({ "redirect_to": redirect_with(&parked.redirect_uri, &params) }),
    ))
}

pub async fn deny(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (parked, _) = load(&state, &request_id).await?;
    sqlx::query("DELETE FROM auth_requests WHERE id_hash = $1")
        .bind(crypto::hash_token(&request_id))
        .execute(&state.db)
        .await?;
    let mut params = vec![
        ("error", "access_denied"),
        ("iss", state.config.issuer.as_str()),
    ];
    if let Some(client_state) = &parked.state {
        params.push(("state", client_state));
    }
    Ok(Json(
        json!({ "redirect_to": redirect_with(&parked.redirect_uri, &params) }),
    ))
}

/// `redirect_uri` with `params` added to its query string.
pub fn redirect_with(redirect_uri: &str, params: &[(&str, &str)]) -> String {
    match url::Url::parse(redirect_uri) {
        Ok(mut url) => {
            url.query_pairs_mut().extend_pairs(params);
            url.into()
        }
        Err(_) => redirect_uri.to_owned(),
    }
}

/// What the consent screen shows as "you'll be sent to": the host, or the scheme for app links.
fn redirect_host(redirect_uri: &str) -> String {
    url::Url::parse(redirect_uri)
        .map(|url| {
            url.host_str()
                .map_or_else(|| format!("{}://", url.scheme()), str::to_owned)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNED_IN: Situation = Situation {
        signed_in: true,
        has_factors: false,
        session_has_second_factor: false,
        second_factor_fresh: false,
        mfa_policy: MfaPolicy::StepUp,
        step_up: false,
        first_party: true,
    };

    #[test]
    fn walks_sign_in_then_mfa_then_consent() {
        assert_eq!(
            decide(Situation {
                signed_in: false,
                ..SIGNED_IN
            }),
            Next::SignIn
        );
        assert_eq!(decide(SIGNED_IN), Next::Complete);
        assert_eq!(
            decide(Situation {
                has_factors: true,
                ..SIGNED_IN
            }),
            Next::Mfa,
            "password only, factor pending"
        );
        assert_eq!(
            decide(Situation {
                first_party: false,
                ..SIGNED_IN
            }),
            Next::Consent
        );
    }

    #[test]
    fn merchant_portal_always_needs_a_second_factor() {
        let merchant = Situation {
            mfa_policy: MfaPolicy::Always,
            ..SIGNED_IN
        };
        assert_eq!(decide(merchant), Next::MfaEnrollment);
        assert_eq!(
            decide(Situation {
                has_factors: true,
                session_has_second_factor: true,
                ..merchant
            }),
            Next::Complete
        );
    }

    #[test]
    fn step_up_needs_a_fresh_second_factor() {
        let step_up = Situation {
            step_up: true,
            has_factors: true,
            session_has_second_factor: true,
            ..SIGNED_IN
        };
        assert_eq!(decide(step_up), Next::Mfa, "yesterday's passkey is not fresh");
        assert_eq!(
            decide(Situation {
                second_factor_fresh: true,
                ..step_up
            }),
            Next::Complete
        );
        assert_eq!(
            decide(Situation {
                has_factors: false,
                session_has_second_factor: false,
                ..step_up
            }),
            Next::MfaEnrollment
        );
    }

    #[test]
    fn redirects_carry_the_code_and_state() {
        assert_eq!(
            redirect_with(
                "dronedrop://auth/callback",
                &[("code", "abc"), ("state", "x y")]
            ),
            "dronedrop://auth/callback?code=abc&state=x+y"
        );
        assert_eq!(
            redirect_host("https://layla.amazon.com/api/skill/link/M1"),
            "layla.amazon.com"
        );
        assert_eq!(redirect_host("dronedrop://auth/callback"), "auth");
    }
}
