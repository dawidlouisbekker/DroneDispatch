//! axum extractor for Bearer access tokens, answering failures with RFC 9728 and
//! RFC 9470 `WWW-Authenticate` challenges.

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};

use crate::{AccessClaims, TokenVerifier, VerifyError, challenge_step_up, challenge_unauthorized};

/// What [`Authenticated`] needs. Provide it from the router state with `FromRef`.
#[derive(Clone)]
pub struct AuthConfig {
    pub verifier: TokenVerifier,
    /// Protected resource metadata URL (RFC 9728), advertised in `401` challenges.
    pub resource_metadata_url: String,
}

/// The claims of a valid access token for this service. Requests without one
/// are rejected with `401` and a challenge.
pub struct Authenticated(pub AccessClaims);

impl<S> FromRequestParts<S> for Authenticated
where
    AuthConfig: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let config = AuthConfig::from_ref(state);
        let token =
            bearer_token(parts).ok_or_else(|| AuthRejection::unauthorized(&config, "missing bearer token"))?;
        config.verifier.verify(token).await.map(Self).map_err(|error| match error {
            VerifyError::KeysUnavailable(detail) => {
                tracing::error!(%detail, "cannot verify access tokens: signing keys unavailable");
                AuthRejection::new(StatusCode::SERVICE_UNAVAILABLE, "Cannot verify access tokens right now", None)
            }
            other => AuthRejection::unauthorized(&config, &other.to_string()),
        })
    }
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

/// Rejects with `403 insufficient_scope` unless the token carries `scope`.
pub fn require_scope(claims: &AccessClaims, scope: &str) -> Result<(), AuthRejection> {
    if claims.has_scope(scope) {
        return Ok(());
    }
    Err(AuthRejection::new(
        StatusCode::FORBIDDEN,
        "Missing required scope",
        Some(format!(r#"Bearer error="insufficient_scope", scope="{scope}""#)),
    ))
}

/// Rejects with an RFC 9470 step-up challenge unless the user completed a second
/// factor within the last `max_age_secs`.
pub fn require_fresh_mfa(claims: &AccessClaims, max_age_secs: u64) -> Result<(), AuthRejection> {
    let now = jsonwebtoken::get_current_timestamp() as i64;
    if claims.has_fresh_mfa(now, max_age_secs as i64) {
        return Ok(());
    }
    Err(AuthRejection::new(
        StatusCode::FORBIDDEN,
        "A recent second factor is required",
        Some(challenge_step_up(max_age_secs)),
    ))
}

/// An auth failure, rendered as `application/problem+json` with a
/// `WWW-Authenticate` challenge when there is one.
#[derive(Debug)]
pub struct AuthRejection {
    status: StatusCode,
    title: &'static str,
    challenge: Option<String>,
}

impl AuthRejection {
    fn new(status: StatusCode, title: &'static str, challenge: Option<String>) -> Self {
        Self { status, title, challenge }
    }

    fn unauthorized(config: &AuthConfig, detail: &str) -> Self {
        tracing::debug!(%detail, "rejected access token");
        Self::new(
            StatusCode::UNAUTHORIZED,
            "Invalid or missing access token",
            Some(challenge_unauthorized(&config.resource_metadata_url)),
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        let mut response = svc_common::problem(self.status, self.title, None);
        if let Some(challenge) = self.challenge.and_then(|c| HeaderValue::try_from(c).ok()) {
            response.headers_mut().insert(header::WWW_AUTHENTICATE, challenge);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use axum::{Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    use super::*;
    use crate::verifier::tests::{claims, sign, verifier};

    fn app() -> Router {
        let config = AuthConfig {
            verifier: verifier(),
            resource_metadata_url: "http://app.test/api/map/.well-known/oauth-protected-resource".into(),
        };
        Router::new()
            .route("/me", get(|Authenticated(claims): Authenticated| async move { claims.sub }))
            .with_state(config)
    }

    #[tokio::test]
    async fn missing_token_gets_a_401_challenge() {
        let response = app().oneshot(Request::get("/me").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/problem+json");
        let challenge = response.headers()[header::WWW_AUTHENTICATE].to_str().unwrap();
        assert!(challenge.contains(r#"resource_metadata="http://app.test/api/map/"#), "{challenge}");
    }

    #[tokio::test]
    async fn valid_bearer_token_reaches_the_handler() {
        let request = Request::get("/me")
            .header(header::AUTHORIZATION, format!("Bearer {}", sign(&claims())))
            .body(Body::empty())
            .unwrap();
        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn stale_mfa_gets_a_step_up_challenge() {
        let mut claims: AccessClaims = serde_json::from_value(claims()).unwrap();
        let rejection = require_fresh_mfa(&claims, 300).unwrap_err();
        assert_eq!(rejection.status(), StatusCode::FORBIDDEN);
        let response = rejection.into_response();
        let challenge = response.headers()[header::WWW_AUTHENTICATE].to_str().unwrap();
        assert!(challenge.contains("insufficient_user_authentication"), "{challenge}");

        claims.amr = vec!["hwk".into()];
        assert!(require_fresh_mfa(&claims, 300).is_ok());
    }

    #[test]
    fn missing_scope_is_forbidden() {
        let claims: AccessClaims = serde_json::from_value(claims()).unwrap();
        assert!(require_scope(&claims, "email").is_ok());
        assert_eq!(require_scope(&claims, "merchant").unwrap_err().status(), StatusCode::FORBIDDEN);
    }
}
