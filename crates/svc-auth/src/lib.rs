//! Resource-server side of Drone Drop auth:
//!
//! - [`TokenVerifier`] checks ES256 access tokens against auth-service's
//!   published keys (`iss`, `aud`, `exp`, and revoked grants).
//! - [`Authenticated`] is the axum extractor services put on protected handlers.
//! - [`require_scope`] and [`require_fresh_mfa`] add per-operation checks, and
//!   the challenge helpers build the matching `WWW-Authenticate` headers.

mod extract;
mod verifier;

pub use extract::{AuthConfig, AuthRejection, Authenticated, require_fresh_mfa, require_scope};
pub use verifier::{TokenVerifier, VerifyError};

use serde::{Deserialize, Serialize};

/// Claims of an RFC 9068 access token (`typ: at+jwt`) issued by auth-service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessClaims {
    pub iss: String,
    pub sub: String,
    /// Exactly one resource, e.g. `https://user.example/mcp`.
    pub aud: String,
    pub client_id: String,
    /// Space-separated scopes.
    pub scope: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    /// Grant id shared by every token from one authorization, so revoking the
    /// grant revokes them all.
    pub gid: String,
    #[serde(default)]
    pub acr: Option<String>,
    /// Authentication methods (RFC 8176), e.g. `pwd`, `hwk`.
    #[serde(default)]
    pub amr: Vec<String>,
    #[serde(default)]
    pub auth_time: Option<i64>,
}

impl AccessClaims {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scope.split(' ').any(|s| s == scope)
    }

    /// Whether the user completed the second factor, a passkey (`hwk`), at most
    /// `max_age_secs` before `now` (unix seconds).
    pub fn has_fresh_mfa(&self, now: i64, max_age_secs: i64) -> bool {
        let second_factor = self.amr.iter().any(|m| m == "hwk");
        second_factor && self.auth_time.is_some_and(|t| now - t <= max_age_secs)
    }
}

/// Challenge for a missing or invalid token. Points the client at the
/// protected resource metadata (RFC 9728), which names the authorization server.
pub fn challenge_unauthorized(resource_metadata_url: &str) -> String {
    format!(r#"Bearer resource_metadata="{resource_metadata_url}""#)
}

/// Step-up challenge (RFC 9470) when an action needs a recent second factor.
pub fn challenge_step_up(max_age_secs: u64) -> String {
    format!(
        r#"Bearer error="insufficient_user_authentication", error_description="A recent second factor is required", acr_values="mfa", max_age={max_age_secs}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(amr: &[&str], auth_time: Option<i64>) -> AccessClaims {
        AccessClaims {
            iss: "http://localhost:8081".into(),
            sub: "user-1".into(),
            aud: "http://localhost:8085".into(),
            client_id: "map-web".into(),
            scope: "openid email".into(),
            iat: 1_000,
            exp: 4_600,
            jti: "jti-1".into(),
            gid: "grant-1".into(),
            acr: None,
            amr: amr.iter().map(|m| (*m).to_owned()).collect(),
            auth_time,
        }
    }

    #[test]
    fn matches_whole_scopes_only() {
        let c = claims(&["pwd"], Some(1_000));
        assert!(c.has_scope("email"));
        assert!(!c.has_scope("mail"));
    }

    #[test]
    fn fresh_mfa_needs_second_factor_and_recent_auth() {
        assert!(claims(&["pwd", "hwk"], Some(1_000)).has_fresh_mfa(1_300, 300));
        assert!(claims(&["hwk"], Some(1_000)).has_fresh_mfa(1_300, 300));
        assert!(!claims(&["pwd", "hwk"], Some(1_000)).has_fresh_mfa(1_301, 300));
        assert!(!claims(&["pwd"], Some(1_000)).has_fresh_mfa(1_000, 300));
        assert!(
            !claims(&["pwd", "otp"], Some(1_000)).has_fresh_mfa(1_000, 300),
            "authenticator app codes no longer count"
        );
        assert!(!claims(&["hwk"], None).has_fresh_mfa(1_000, 300));
    }
}
