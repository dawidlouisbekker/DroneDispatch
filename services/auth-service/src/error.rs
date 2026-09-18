//! Errors of the `/v1` JSON API, rendered as `application/problem+json`.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    title: &'static str,
    reason: Option<&'static str>,
}

impl ApiError {
    pub const fn new(
        status: StatusCode,
        title: &'static str,
        reason: Option<&'static str>,
    ) -> Self {
        Self {
            status,
            title,
            reason,
        }
    }

    pub const fn bad_request(title: &'static str, reason: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, title, Some(reason))
    }

    pub const fn unauthorized(title: &'static str, reason: &'static str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, title, Some(reason))
    }

    pub const fn not_signed_in() -> Self {
        Self::unauthorized("Sign in first", "NOT_SIGNED_IN")
    }

    pub const fn forbidden(title: &'static str, reason: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, title, Some(reason))
    }

    /// For account changes that need a second factor in the last few minutes.
    pub const fn fresh_mfa_required() -> Self {
        Self::forbidden(
            "Confirm it's you with your second factor first",
            "FRESH_MFA_REQUIRED",
        )
    }

    pub const fn not_found(title: &'static str) -> Self {
        Self::new(StatusCode::NOT_FOUND, title, None)
    }

    pub const fn conflict(title: &'static str, reason: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, title, Some(reason))
    }

    pub const fn too_many_attempts() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts. Try again in a few minutes.",
            Some("TOO_MANY_ATTEMPTS"),
        )
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "internal error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Something went wrong",
            None,
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn reason(&self) -> Option<&'static str> {
        self.reason
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        svc_common::problem(self.status, self.title, self.reason)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error)
    }
}
