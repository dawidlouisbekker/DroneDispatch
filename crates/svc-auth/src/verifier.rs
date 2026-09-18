//! Verifies ES256 access tokens against auth-service's published signing keys.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::AccessClaims;

/// Accepted clock skew when checking `exp`.
const LEEWAY_SECS: u64 = 30;
/// Minimum time between key reloads, so tokens with made-up key ids can't hammer auth-service.
const RELOAD_COOLDOWN: Duration = Duration::from_secs(30);
/// Keys are reloaded at least this often, so rotated-out keys stop verifying.
const MAX_KEY_AGE: Duration = Duration::from_secs(3600);

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("malformed token: {0}")]
    Malformed(String),
    #[error("not an ES256 `at+jwt` access token")]
    WrongType,
    #[error("token signed with an unknown key")]
    UnknownKey,
    #[error("invalid token: {0}")]
    Invalid(String),
    #[error("the grant behind this token was revoked")]
    Revoked,
    #[error("signing keys unavailable: {0}")]
    KeysUnavailable(String),
}

/// Verifies access tokens issued by one authorization server for one resource
/// (audience). Clones share the key cache and the revoked-grant list.
#[derive(Clone)]
pub struct TokenVerifier {
    inner: Arc<Inner>,
}

struct Inner {
    issuer: String,
    audience: String,
    /// `None` for a fixed key set; otherwise keys are loaded from the issuer.
    http: Option<reqwest::Client>,
    /// Where to load keys from. `None` discovers `jwks_uri` from the issuer's metadata.
    jwks_url: Option<String>,
    keys: RwLock<KeyCache>,
    revoked_grants: RwLock<HashSet<String>>,
}

#[derive(Default)]
struct KeyCache {
    by_kid: HashMap<String, DecodingKey>,
    loaded_at: Option<Instant>,
}

#[derive(Deserialize)]
struct AuthorizationServerMetadata {
    jwks_uri: String,
}

impl TokenVerifier {
    /// Loads signing keys from the issuer's `/.well-known/oauth-authorization-server`
    /// metadata on first use, and again when they age out or an unknown key id appears.
    pub fn new(issuer: impl Into<String>, audience: impl Into<String>) -> Self {
        Self::build(issuer.into(), audience.into(), Some(reqwest::Client::new()), None, KeyCache::default())
    }

    /// Loads signing keys from `jwks_url` instead of discovering it. Use this when the
    /// public issuer URL isn't reachable from the service (e.g. `localhost` inside Docker).
    pub fn with_jwks_url(issuer: impl Into<String>, audience: impl Into<String>, jwks_url: impl Into<String>) -> Self {
        Self::build(
            issuer.into(),
            audience.into(),
            Some(reqwest::Client::new()),
            Some(jwks_url.into()),
            KeyCache::default(),
        )
    }

    /// Uses a fixed key set and never loads keys over the network.
    pub fn with_jwks(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        jwks: &JwkSet,
    ) -> Result<Self, VerifyError> {
        let cache = KeyCache { by_kid: index_keys(jwks)?, loaded_at: Some(Instant::now()) };
        Ok(Self::build(issuer.into(), audience.into(), None, None, cache))
    }

    fn build(
        issuer: String,
        audience: String,
        http: Option<reqwest::Client>,
        jwks_url: Option<String>,
        keys: KeyCache,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                issuer,
                audience,
                http,
                jwks_url,
                keys: RwLock::new(keys),
                revoked_grants: RwLock::default(),
            }),
        }
    }

    pub fn issuer(&self) -> &str {
        &self.inner.issuer
    }

    pub fn audience(&self) -> &str {
        &self.inner.audience
    }

    /// Rejects every token of `grant_id` from now on (fed by `auth.events.grant_revoked`).
    pub async fn revoke_grant(&self, grant_id: impl Into<String>) {
        self.inner.revoked_grants.write().await.insert(grant_id.into());
    }

    pub async fn verify(&self, token: &str) -> Result<AccessClaims, VerifyError> {
        let header = decode_header(token).map_err(|e| VerifyError::Malformed(e.to_string()))?;
        let is_access_token = header.typ.as_deref().is_some_and(|typ| typ.eq_ignore_ascii_case("at+jwt"));
        if header.alg != Algorithm::ES256 || !is_access_token {
            return Err(VerifyError::WrongType);
        }
        let kid = header.kid.ok_or(VerifyError::UnknownKey)?;
        let key = self.key(&kid).await?;

        let mut validation = Validation::new(Algorithm::ES256);
        validation.set_issuer(&[&self.inner.issuer]);
        validation.set_audience(&[&self.inner.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = LEEWAY_SECS;
        let claims = decode::<AccessClaims>(token, &key, &validation)
            .map_err(|e| VerifyError::Invalid(e.to_string()))?
            .claims;

        if self.inner.revoked_grants.read().await.contains(&claims.gid) {
            return Err(VerifyError::Revoked);
        }
        Ok(claims)
    }

    async fn key(&self, kid: &str) -> Result<DecodingKey, VerifyError> {
        let (cached, stale, may_reload) = {
            let cache = self.inner.keys.read().await;
            let age = cache.loaded_at.map(|at| at.elapsed());
            (
                cache.by_kid.get(kid).cloned(),
                age.is_none_or(|age| age >= MAX_KEY_AGE),
                self.inner.http.is_some() && age.is_none_or(|age| age >= RELOAD_COOLDOWN),
            )
        };
        match cached {
            Some(key) if !stale || !may_reload => Ok(key),
            // Stale: reload, but keep verifying with the cached key while auth-service is unreachable.
            Some(key) => match self.reload().await {
                Ok(()) => self.cached_key(kid).await.ok_or(VerifyError::UnknownKey),
                Err(error) => {
                    tracing::warn!(%error, "reloading signing keys failed; using cached keys");
                    Ok(key)
                }
            },
            None if may_reload => {
                self.reload().await?;
                self.cached_key(kid).await.ok_or(VerifyError::UnknownKey)
            }
            None => Err(VerifyError::UnknownKey),
        }
    }

    async fn cached_key(&self, kid: &str) -> Option<DecodingKey> {
        self.inner.keys.read().await.by_kid.get(kid).cloned()
    }

    async fn reload(&self) -> Result<(), VerifyError> {
        let Some(http) = &self.inner.http else {
            return Ok(());
        };
        let unavailable = |e: reqwest::Error| VerifyError::KeysUnavailable(e.to_string());
        let jwks_uri = match &self.inner.jwks_url {
            Some(url) => url.clone(),
            None => {
                let metadata_url =
                    format!("{}/.well-known/oauth-authorization-server", self.inner.issuer.trim_end_matches('/'));
                let metadata: AuthorizationServerMetadata = http
                    .get(&metadata_url)
                    .send()
                    .await
                    .and_then(|r| r.error_for_status())
                    .map_err(unavailable)?
                    .json()
                    .await
                    .map_err(unavailable)?;
                metadata.jwks_uri
            }
        };
        let jwks: JwkSet = http
            .get(&jwks_uri)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(unavailable)?
            .json()
            .await
            .map_err(unavailable)?;

        let by_kid = index_keys(&jwks)?;
        tracing::info!(keys = by_kid.len(), %jwks_uri, "loaded signing keys");
        *self.inner.keys.write().await = KeyCache { by_kid, loaded_at: Some(Instant::now()) };
        Ok(())
    }
}

/// Indexes a JWK set by key id. Keys without a `kid` can't be selected, so they're skipped.
fn index_keys(jwks: &JwkSet) -> Result<HashMap<String, DecodingKey>, VerifyError> {
    jwks.keys
        .iter()
        .filter_map(|jwk| jwk.common.key_id.clone().map(|kid| (kid, jwk)))
        .map(|(kid, jwk)| {
            DecodingKey::from_jwk(jwk)
                .map_err(|e| VerifyError::KeysUnavailable(format!("unusable JWK {kid}: {e}")))
                .map(|key| (kid, key))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use jsonwebtoken::{EncodingKey, Header, encode, get_current_timestamp};
    use serde_json::{Value, json};

    use super::*;

    pub(crate) const ISSUER: &str = "http://auth.test";
    pub(crate) const AUDIENCE: &str = "http://app.test/api/map";

    // Throwaway P-256 key generated for these tests only.
    const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg2pvAk2OyqgSI8teV
Kyea+wkOGV0c69MpYhVLmsTXXpuhRANCAATkYlPe+MQmKZ6JV69eDymG4mC8Jso4
O8wJ+Xwa9UBSUK6yJv7XXPY62nu4pjQLmwbvp7Ky/j/DjhEPwnQOMApH
-----END PRIVATE KEY-----
";
    const TEST_KEY_X: &str = "5GJT3vjEJimeiVevXg8phuJgvCbKODvMCfl8GvVAUlA";
    const TEST_KEY_Y: &str = "rrIm_tdc9jrae7imNAubBu-nsrL-P8OOEQ_CdA4wCkc";

    pub(crate) fn verifier() -> TokenVerifier {
        let jwks: JwkSet = serde_json::from_value(json!({
            "keys": [{
                "kty": "EC", "crv": "P-256", "x": TEST_KEY_X, "y": TEST_KEY_Y,
                "kid": "test-key", "alg": "ES256", "use": "sig"
            }]
        }))
        .unwrap();
        TokenVerifier::with_jwks(ISSUER, AUDIENCE, &jwks).unwrap()
    }

    pub(crate) fn claims() -> Value {
        let now = get_current_timestamp() as i64;
        json!({
            "iss": ISSUER,
            "sub": "0190f3a2-7c1e-7a5b-9d2e-4b8f6c1a2d3e",
            "aud": AUDIENCE,
            "client_id": "ui-web",
            "scope": "openid email",
            "iat": now,
            "exp": now + 3600,
            "jti": "jti-1",
            "gid": "grant-1",
            "amr": ["pwd"],
            "auth_time": now
        })
    }

    pub(crate) fn sign(claims: &Value) -> String {
        sign_with(claims, "test-key", Some("at+jwt"))
    }

    fn sign_with(claims: &Value, kid: &str, typ: Option<&str>) -> String {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(kid.into());
        header.typ = typ.map(Into::into);
        encode(&header, claims, &EncodingKey::from_ec_pem(TEST_KEY_PEM.as_bytes()).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn accepts_a_valid_token() {
        let verified = verifier().verify(&sign(&claims())).await.unwrap();
        assert_eq!(verified.gid, "grant-1");
        assert_eq!(verified.aud, AUDIENCE);
    }

    #[tokio::test]
    async fn rejects_another_audience_or_issuer() {
        let mut other_audience = claims();
        other_audience["aud"] = "http://app.test/api/merchant".into();
        assert!(matches!(verifier().verify(&sign(&other_audience)).await, Err(VerifyError::Invalid(_))));

        let mut other_issuer = claims();
        other_issuer["iss"] = "http://evil.test".into();
        assert!(matches!(verifier().verify(&sign(&other_issuer)).await, Err(VerifyError::Invalid(_))));
    }

    #[tokio::test]
    async fn rejects_an_expired_token() {
        let mut expired = claims();
        expired["exp"] = (get_current_timestamp() as i64 - 600).into();
        assert!(matches!(verifier().verify(&sign(&expired)).await, Err(VerifyError::Invalid(_))));
    }

    #[tokio::test]
    async fn rejects_a_tampered_payload() {
        let token = sign(&claims());
        let mut elevated = claims();
        elevated["scope"] = "openid email merchant".into();
        let forged_payload = sign(&elevated).split('.').nth(1).unwrap().to_owned();
        let parts: Vec<&str> = token.split('.').collect();
        let forged = format!("{}.{}.{}", parts[0], forged_payload, parts[2]);
        assert!(matches!(verifier().verify(&forged).await, Err(VerifyError::Invalid(_))));
    }

    #[tokio::test]
    async fn rejects_unknown_keys_and_non_access_tokens() {
        let unknown_key = sign_with(&claims(), "other-key", Some("at+jwt"));
        assert!(matches!(verifier().verify(&unknown_key).await, Err(VerifyError::UnknownKey)));

        let id_token = sign_with(&claims(), "test-key", Some("JWT"));
        assert!(matches!(verifier().verify(&id_token).await, Err(VerifyError::WrongType)));

        assert!(matches!(verifier().verify("not-a-jwt").await, Err(VerifyError::Malformed(_))));
    }

    #[tokio::test]
    async fn rejects_tokens_of_a_revoked_grant() {
        let verifier = verifier();
        let token = sign(&claims());
        assert!(verifier.verify(&token).await.is_ok());
        verifier.revoke_grant("grant-1").await;
        assert!(matches!(verifier.verify(&token).await, Err(VerifyError::Revoked)));
    }
}
