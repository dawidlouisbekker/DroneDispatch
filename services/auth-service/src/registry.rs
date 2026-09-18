//! The resources auth-service issues tokens for, and the OAuth clients it knows.

use sqlx::PgPool;

use crate::{config::Config, crypto};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MfaPolicy {
    /// No second factor needed (voice ordering through Alexa).
    None,
    /// A second factor only when the client asks for a step-up.
    StepUp,
    /// Every sign-in needs a second factor (the merchant portal).
    Always,
}

#[derive(Clone, Debug)]
pub struct Resource {
    pub uri: String,
    pub scopes: &'static [&'static str],
    pub mfa: MfaPolicy,
}

pub fn resources(config: &Config) -> Vec<Resource> {
    vec![
        Resource {
            uri: config.user_api_url.clone(),
            scopes: &["openid", "email"],
            mfa: MfaPolicy::StepUp,
        },
        Resource {
            uri: config.merchant_api_url.clone(),
            scopes: &["openid", "email", "merchant"],
            mfa: MfaPolicy::Always,
        },
        Resource {
            uri: config.mcp_resource_url.clone(),
            scopes: &["openid", "delivery"],
            mfa: MfaPolicy::None,
        },
    ]
}

pub fn find_resource<'a>(resources: &'a [Resource], uri: &str) -> Option<&'a Resource> {
    resources.iter().find(|r| r.uri == uri)
}

/// Our own apps: they skip the consent screen.
const FIRST_PARTY: [&str; 2] = ["ui-web", "ui-native"];

#[derive(Clone, Debug)]
pub struct Client {
    pub client_id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub secret_hash: Option<String>,
    pub first_party: bool,
}

/// Creates or updates the static clients from config.
pub async fn seed_clients(db: &PgPool, config: &Config) -> sqlx::Result<()> {
    upsert(
        db,
        "ui-web",
        "Drone Drop",
        &config.ui_web_redirect_uris,
        None,
    )
    .await?;
    upsert(
        db,
        "ui-native",
        "Drone Drop app",
        &config.ui_native_redirect_uris,
        None,
    )
    .await?;
    match (
        &config.alexa_client_secret,
        config.alexa_redirect_uris.is_empty(),
    ) {
        (Some(secret), false) => {
            upsert(
                db,
                "alexa",
                "Alexa",
                &config.alexa_redirect_uris,
                Some(&crypto::hash_token(secret)),
            )
            .await?
        }
        _ => tracing::info!(
            "ALEXA_CLIENT_SECRET or ALEXA_REDIRECT_URIS not set; Alexa client not registered"
        ),
    }
    Ok(())
}

async fn upsert(
    db: &PgPool,
    client_id: &str,
    name: &str,
    redirect_uris: &[String],
    secret_hash: Option<&str>,
) -> sqlx::Result<()> {
    let auth_method = if secret_hash.is_some() {
        "client_secret_basic"
    } else {
        "none"
    };
    sqlx::query(
        "INSERT INTO clients (client_id, secret_hash, name, redirect_uris, auth_method, kind)
         VALUES ($1, $2, $3, $4, $5, 'STATIC')
         ON CONFLICT (client_id) DO UPDATE SET
             secret_hash = EXCLUDED.secret_hash, name = EXCLUDED.name, redirect_uris = EXCLUDED.redirect_uris,
             auth_method = EXCLUDED.auth_method, updated_at = now()",
    )
    .bind(client_id)
    .bind(secret_hash)
    .bind(name)
    .bind(redirect_uris)
    .bind(auth_method)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn find_client(db: &PgPool, client_id: &str) -> sqlx::Result<Option<Client>> {
    let row: Option<(String, String, Vec<String>, Option<String>)> = sqlx::query_as(
        "SELECT client_id, name, redirect_uris, secret_hash FROM clients WHERE client_id = $1",
    )
    .bind(client_id)
    .fetch_optional(db)
    .await?;
    Ok(
        row.map(|(client_id, name, redirect_uris, secret_hash)| Client {
            first_party: FIRST_PARTY.contains(&client_id.as_str()),
            client_id,
            name,
            redirect_uris,
            secret_hash,
        }),
    )
}
