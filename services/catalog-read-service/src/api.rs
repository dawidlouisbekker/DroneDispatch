//! The read-only `/v1` catalog API described in `api/openapi/catalog.yaml`.
//!
//! Catalogs are public, so no token is needed. Every operation returns `501` until
//! the projections are implemented.

use axum::{Router, response::Response, routing::get};
use svc_common::not_implemented;

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/catalogs", get(pending))
        .route("/v1/catalogs/{place_id}", get(pending))
        .route("/v1/catalogs/{place_id}/menu", get(pending))
}

/// A read that isn't implemented yet.
async fn pending() -> Response {
    not_implemented()
}
