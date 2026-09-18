//! auth-service: Drone Drop's OAuth 2.1 authorization server (see README.md).
//!
//! It has two faces: the OAuth endpoints clients use (`/authorize`, `/token`,
//! `/revoke`, metadata and JWKS), and the `/v1` JSON API that the sign-in
//! screens in ui/app call with a session cookie.

pub mod account;
pub mod config;
pub mod crypto;
pub mod error;
pub mod flow;
pub mod grpc;
pub mod keys;
pub mod limits;
pub mod mailer;
pub mod oauth;
pub mod passkeys;
pub mod registry;
pub mod session;
pub mod signup;

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    http::{HeaderValue, header},
    middleware,
    routing::{delete, get, post},
};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use webauthn_rs::{Webauthn, WebauthnBuilder, prelude::Url};

use crate::{config::Config, keys::SigningKey, limits::Limiter, registry::Resource};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub keys: Arc<SigningKey>,
    pub webauthn: Arc<Webauthn>,
    pub limiter: Arc<Limiter>,
    pub resources: Arc<Vec<Resource>>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> anyhow::Result<Self> {
        let origin = Url::parse(&config.webauthn_origin)?;
        let webauthn = WebauthnBuilder::new(&config.webauthn_rp_id, &origin)?
            .rp_name("Drone Drop")
            .build()?;
        Ok(Self {
            keys: Arc::new(SigningKey::from_pem(&config.signing_key_pem)?),
            webauthn: Arc::new(webauthn),
            // 10 failed attempts per account or IP address per 15 minutes.
            limiter: Arc::new(Limiter::new(10, Duration::from_secs(15 * 60))),
            resources: Arc::new(registry::resources(&config)),
            config: Arc::new(config),
            db,
        })
    }
}

pub fn router(state: AppState) -> Router {
    let allowed_origins = state
        .config
        .allowed_origins
        .iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();
    let v1 = Router::new()
        .route("/requests/{request_id}", get(flow::get_request))
        .route("/requests/{request_id}/complete", post(flow::complete))
        .route("/requests/{request_id}/deny", post(flow::deny))
        .route("/sign-up/check", post(signup::check_email))
        .route("/sign-up/otp", post(signup::start))
        .route("/sign-up/otp/resend", post(signup::resend))
        .route("/sign-up/otp/verify", post(signup::verify))
        .route("/sign-up/finalize", post(signup::finalize))
        .route("/sign-in/password", post(account::sign_in_password))
        .route("/sign-in/passkey/options", post(passkeys::sign_in_options))
        .route("/sign-in/passkey", post(passkeys::sign_in))
        .route(
            "/mfa/passkey/options",
            post(passkeys::second_factor_options),
        )
        .route("/mfa/passkey/verify", post(passkeys::verify_second_factor))
        .route("/passkeys/options", post(passkeys::registration_options))
        .route("/passkeys", get(passkeys::list).post(passkeys::add))
        .route("/passkeys/{passkey_id}", delete(passkeys::remove))
        .route("/session", get(account::get_session))
        .route("/sign-out", post(account::sign_out))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            account::origin_guard,
        ));

    svc_common::health_routes()
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth::metadata),
        )
        .route("/.well-known/jwks.json", get(oauth::jwks))
        .route("/authorize", get(oauth::authorize))
        .route("/token", post(oauth::token))
        .route("/revoke", post(oauth::revoke))
        .nest("/v1", v1)
        .layer(
            CorsLayer::new()
                .allow_origin(allowed_origins)
                .allow_credentials(true)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([header::ACCEPT, header::CONTENT_TYPE, header::ORIGIN]),
        )
        .with_state(state)
}
