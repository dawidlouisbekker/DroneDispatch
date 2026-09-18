//! The ES256 key that signs access tokens, and its public JWK.

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p256::{SecretKey, pkcs8::DecodePrivateKey};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub struct SigningKey {
    kid: String,
    encoding: EncodingKey,
    public_jwk: Value,
}

impl SigningKey {
    /// `pem` is a PKCS#8 P-256 private key (`openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256`).
    pub fn from_pem(pem: &str) -> Result<Self> {
        let secret = SecretKey::from_pkcs8_pem(pem)
            .context("AUTH_SIGNING_KEY_PEM is not a PKCS#8 P-256 key")?;
        let mut public_jwk = serde_json::to_value(secret.public_key().to_jwk())?;
        let kid = thumbprint(&public_jwk)?;
        public_jwk["kid"] = kid.clone().into();
        public_jwk["alg"] = "ES256".into();
        public_jwk["use"] = "sig".into();
        Ok(Self {
            kid,
            encoding: EncodingKey::from_ec_pem(pem.as_bytes())?,
            public_jwk,
        })
    }

    pub fn jwks(&self) -> Value {
        json!({ "keys": [self.public_jwk] })
    }

    /// Signs RFC 9068 access-token claims.
    pub fn sign<T: Serialize>(&self, claims: &T) -> Result<String> {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.kid.clone());
        header.typ = Some("at+jwt".into());
        Ok(encode(&header, claims, &self.encoding)?)
    }
}

/// RFC 7638 JWK thumbprint, used as the key id.
fn thumbprint(jwk: &Value) -> Result<String> {
    let field = |name: &str| {
        jwk[name]
            .as_str()
            .with_context(|| format!("JWK has no {name}"))
    };
    let canonical = format!(
        r#"{{"crv":"{}","kty":"{}","x":"{}","y":"{}"}}"#,
        field("crv")?,
        field("kty")?,
        field("x")?,
        field("y")?
    );
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes())))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // Throwaway P-256 key generated for tests only.
    pub(crate) const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg2pvAk2OyqgSI8teV
Kyea+wkOGV0c69MpYhVLmsTXXpuhRANCAATkYlPe+MQmKZ6JV69eDymG4mC8Jso4
O8wJ+Xwa9UBSUK6yJv7XXPY62nu4pjQLmwbvp7Ky/j/DjhEPwnQOMApH
-----END PRIVATE KEY-----
";

    #[test]
    fn jwks_publishes_the_public_half_with_a_stable_kid() {
        let key = SigningKey::from_pem(TEST_KEY_PEM).unwrap();
        let jwk = &key.jwks()["keys"][0];
        assert_eq!(jwk["kty"], "EC");
        assert_eq!(jwk["crv"], "P-256");
        assert_eq!(jwk["x"], "5GJT3vjEJimeiVevXg8phuJgvCbKODvMCfl8GvVAUlA");
        assert!(jwk.get("d").is_none(), "private key must not be published");
        assert_eq!(jwk["kid"], SigningKey::from_pem(TEST_KEY_PEM).unwrap().kid);
    }

    #[tokio::test]
    async fn signed_tokens_pass_the_resource_server_verifier() {
        let key = SigningKey::from_pem(TEST_KEY_PEM).unwrap();
        let jwks = serde_json::from_value(key.jwks()).unwrap();
        let verifier = svc_auth::TokenVerifier::with_jwks(
            "http://auth.test",
            "http://app.test/api/map",
            &jwks,
        )
        .unwrap();
        let now = jsonwebtoken::get_current_timestamp() as i64;
        let token = key
            .sign(&json!({
                "iss": "http://auth.test", "sub": "user-1", "aud": "http://app.test/api/map",
                "client_id": "ui-web", "scope": "openid email", "iat": now, "exp": now + 3600,
                "jti": "jti-1", "gid": "grant-1", "acr": "mfa", "amr": ["pwd", "hwk"], "auth_time": now
            }))
            .unwrap();
        let claims = verifier.verify(&token).await.unwrap();
        assert!(claims.has_fresh_mfa(now, 300));
    }
}
