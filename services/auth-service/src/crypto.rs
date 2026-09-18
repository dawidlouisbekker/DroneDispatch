//! Random tokens, token hashing and password hashing.

use anyhow::{Result, anyhow};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// 256 random bits, base64url. Used for session ids, codes, refresh tokens and request ids.
pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

/// What gets stored for a token: its SHA-256, base64url. Tokens have 256 bits of
/// entropy, so a fast hash is enough.
pub fn hash_token(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

/// The PKCE S256 challenge for `verifier` (RFC 7636).
pub fn pkce_s256(verifier: &str) -> String {
    hash_token(verifier)
}

/// Constant-time comparison for hashes of equal length.
pub fn hashes_match(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// argon2id with the crate's default (OWASP-recommended) parameters.
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::encode_b64(&random_bytes::<16>()).map_err(|e| anyhow!("salt: {e}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("hashing password: {e}"))?
        .to_string())
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc_7636_example() {
        assert_eq!(
            pkce_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn passwords_verify_only_with_the_right_password() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password(&hash, "correct horse battery"));
        assert!(!verify_password(&hash, "wrong horse battery"));
        assert!(!verify_password("not a hash", "anything"));
    }

    #[test]
    fn hash_comparison_is_exact() {
        assert!(hashes_match("abc", "abc"));
        assert!(!hashes_match("abc", "abd"));
        assert!(!hashes_match("abc", "ab"));
    }
}
