//! The `/v1` customer API described in `api/openapi/user.yaml`.
//!
//! Every operation requires an access token for this API. Operations return `501`
//! until orders, pickup locations and stations are implemented; the passkey step-up
//! checks and the static parts of the fleet snapshot are already real.

use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use svc_auth::{Authenticated, require_fresh_mfa};
use svc_common::not_implemented;

use crate::AppState;

/// How recent the passkey must be to verify a pickup location or approve an order.
const STEP_UP_MAX_AGE_SECS: u64 = 300;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/.well-known/oauth-protected-resource", get(protected_resource_metadata))
        .route("/v1/me", get(pending))
        .route("/v1/orders", get(pending))
        .route("/v1/orders/{order_id}", get(pending))
        .route("/v1/orders/{order_id}/cancel", post(pending))
        .route("/v1/orders/{order_id}/approve", post(approve_order))
        .route("/v1/pickup-locations", get(pending).post(pending))
        .route("/v1/pickup-locations/{location_id}/verify", post(verify_pickup_location))
        .route("/v1/pickup-locations/{location_id}/station", get(pending).put(pending))
        .route("/v1/places/autocomplete", get(pending))
        .route("/v1/spend-policy", get(pending).put(pending))
        .route("/v1/payment-methods/setup-session", post(pending))
        .route("/v1/fleet/snapshot", get(fleet_snapshot))
        .route("/v1/live", get(crate::live::upgrade))
}

/// RFC 9728 metadata: which authorization server issues tokens for this API.
async fn protected_resource_metadata(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "resource": state.auth.verifier.audience(),
        "authorization_servers": [state.auth.verifier.issuer()],
        "scopes_supported": ["openid", "email"],
        "bearer_methods_supported": ["header"],
    }))
}

/// An authenticated operation that isn't implemented yet.
async fn pending(Authenticated(_): Authenticated) -> Response {
    not_implemented()
}

async fn verify_pickup_location(Authenticated(claims): Authenticated) -> Response {
    if let Err(rejection) = require_fresh_mfa(&claims, STEP_UP_MAX_AGE_SECS) {
        return rejection.into_response();
    }
    // TODO(milestone 4): mark the location VERIFIED with claims.amr and claims.auth_time as evidence.
    not_implemented()
}

async fn approve_order(Authenticated(claims): Authenticated) -> Response {
    if let Err(rejection) = require_fresh_mfa(&claims, STEP_UP_MAX_AGE_SECS) {
        return rejection.into_response();
    }
    // TODO(milestone 4): record the approval, then authorize payment and submit the order.
    not_implemented()
}

async fn fleet_snapshot(Authenticated(_): Authenticated, State(state): State<AppState>) -> Json<Value> {
    let zones: Vec<Value> = state
        .zones
        .zones
        .iter()
        // Uplink status and drones come from each zone's dispatch Fleet.GetFleetSnapshot once it exists.
        .map(|zone| json!({ "zone_id": zone.zone_id, "name": zone.name, "boundary": zone.boundary, "uplink_connected": null }))
        .collect();
    Json(json!({
        "zones": zones,
        "hubs": state.zones.hubs,
        "no_fly_zones": state.zones.no_fly_zones,
        "drones": [],
    }))
}

/// Zones, hubs and no-fly areas of the simulated city (`config/zones.seattle.json`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ZoneConfig {
    pub zones: Vec<Zone>,
    pub hubs: Vec<Hub>,
    pub no_fly_zones: Vec<NoFlyZone>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Zone {
    pub zone_id: String,
    pub name: String,
    /// Closed ring of `[lon, lat]`.
    pub boundary: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hub {
    pub hub_id: String,
    pub zone_id: String,
    pub position: geo::LatLon,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NoFlyZone {
    pub name: String,
    pub boundary: Vec<[f64; 2]>,
}

impl ZoneConfig {
    pub fn seattle() -> serde_json::Result<Self> {
        serde_json::from_str(include_str!("../../../config/zones.seattle.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seattle_config_parses() {
        let config = ZoneConfig::seattle().unwrap();
        assert_eq!(config.zones.len(), 2);
        assert!(config.zones.iter().all(|zone| zone.boundary.first() == zone.boundary.last()));
    }
}
