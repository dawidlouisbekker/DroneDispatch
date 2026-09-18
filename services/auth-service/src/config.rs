//! Settings from the environment. `scripts/dev-secrets.sh` writes the secrets for local use.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use svc_common::{env, env_or};
use url::Url;

pub struct Config {
    pub http_addr: String,
    pub grpc_addr: String,
    pub database_url: String,
    pub nats_url: String,
    /// Gateway origin that serves the app and the APIs, e.g. `http://localhost:8086`.
    pub app_url: String,
    /// OAuth issuer, `<APP>/api/auth`.
    pub issuer: String,
    /// Where the sign-in screens run. Defaults to the app URL; the Expo dev server has its own.
    pub ui_url: String,
    /// Browser origins allowed to make mutating calls to the `/v1` JSON API.
    pub allowed_origins: Vec<String>,
    /// Path the session cookie is scoped to: the issuer's path (`/api/auth`).
    pub cookie_path: String,
    pub secure_cookies: bool,
    /// The Alexa+ MCP resource served by user-service, e.g. `https://user.example/mcp`.
    pub mcp_resource_url: String,
    /// Token audience (`resource`) of user-service's customer API. Defaults to `<APP>/api/user`,
    /// its path behind ui-gateway; without the gateway it is the service's own URL.
    pub user_api_url: String,
    /// Token audience of merchant-service's API. Defaults to `<APP>/api/merchant`.
    pub merchant_api_url: String,
    pub signing_key_pem: String,
    pub webauthn_rp_id: String,
    pub webauthn_origin: String,
    pub ui_web_redirect_uris: Vec<String>,
    pub ui_native_redirect_uris: Vec<String>,
    pub alexa_client_secret: Option<String>,
    pub alexa_redirect_uris: Vec<String>,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_from: String,
    pub otp_ttl_secs: i64,
    pub otp_resend_secs: i64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let app_url = trim(&env_or("APP_PUBLIC_URL", "http://localhost:8086"));
        let issuer = trim(&env_or("ISSUER", &format!("{app_url}/api/auth")));
        let ui_url = trim(&env_or("UI_URL", &app_url));

        let mut allowed_origins = vec![origin(&app_url)?, origin(&ui_url)?];
        allowed_origins.extend(list(&env_or("EXTRA_ALLOWED_ORIGINS", "")));
        allowed_origins.dedup();

        let issuer_url = Url::parse(&issuer).context("ISSUER must be a URL")?;
        let cookie_path = match issuer_url.path().trim_end_matches('/') {
            "" => "/".to_owned(),
            path => path.to_owned(),
        };
        let default_rp_id = Url::parse(&ui_url)?
            .host_str()
            .unwrap_or("localhost")
            .to_owned();

        Ok(Self {
            http_addr: env_or("HTTP_ADDR", "0.0.0.0:8081"),
            grpc_addr: env_or("GRPC_ADDR", "0.0.0.0:9081"),
            database_url: env("DATABASE_URL")?,
            nats_url: env_or("NATS_URL", svc_common::DEFAULT_NATS_URL),
            secure_cookies: issuer_url.scheme() == "https",
            cookie_path,
            mcp_resource_url: trim(&env_or("MCP_RESOURCE_URL", "http://localhost:8085/mcp")),
            user_api_url: trim(&env_or("USER_API_URL", &format!("{app_url}/api/user"))),
            merchant_api_url: trim(&env_or(
                "MERCHANT_API_URL",
                &format!("{app_url}/api/merchant"),
            )),
            signing_key_pem: signing_key_pem()?,
            webauthn_rp_id: env_or("WEBAUTHN_RP_ID", &default_rp_id),
            webauthn_origin: env_or("WEBAUTHN_ORIGIN", &origin(&ui_url)?),
            ui_web_redirect_uris: list(&env_or(
                "UI_WEB_REDIRECT_URIS",
                &format!("{app_url}/auth/callback,http://localhost:8087/auth/callback"),
            )),
            ui_native_redirect_uris: list(&env_or(
                "UI_NATIVE_REDIRECT_URIS",
                "dronedrop://auth/callback",
            )),
            alexa_client_secret: std::env::var("ALEXA_CLIENT_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            alexa_redirect_uris: list(&env_or("ALEXA_REDIRECT_URIS", "")),
            smtp_host: env_or("SMTP_HOST", "localhost"),
            smtp_port: env_or("SMTP_PORT", "1025")
                .parse()
                .context("SMTP_PORT must be a number")?,
            smtp_from: env_or("SMTP_FROM", "Drone Drop <no-reply@dronedrop.local>"),
            otp_ttl_secs: env_or("OTP_TTL_SECS", "600")
                .parse()
                .context("OTP_TTL_SECS must be a number")?,
            otp_resend_secs: env_or("OTP_RESEND_SECS", "30")
                .parse()
                .context("OTP_RESEND_SECS must be a number")?,
            allowed_origins,
            app_url,
            issuer,
            ui_url,
        })
    }
}

fn signing_key_pem() -> Result<String> {
    if let Ok(path) = std::env::var("AUTH_SIGNING_KEY_FILE") {
        let path = resolve_key_path(&path);
        return std::fs::read_to_string(&path)
            .with_context(|| format!("reading AUTH_SIGNING_KEY_FILE at {}", path.display()));
    }

    // Keep the old variable as a migration path for existing deployments.
    Ok(env("AUTH_SIGNING_KEY_PEM")?.replace("\\n", "\n"))
}

fn resolve_key_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.exists() || path.is_absolute() {
        return path.to_owned();
    }

    let service_relative = Path::new("services/auth-service").join(path);
    if service_relative.exists() {
        service_relative
    } else if let Ok(stripped) = path.strip_prefix("services/auth-service") {
        stripped.to_owned()
    } else {
        path.to_owned()
    }
}

fn trim(url: &str) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

fn origin(url: &str) -> Result<String> {
    Ok(Url::parse(url)
        .with_context(|| format!("{url} is not a URL"))?
        .origin()
        .ascii_serialization())
}

fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
