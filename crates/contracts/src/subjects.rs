//! NATS subject, stream and bucket names. Service-to-service calls use gRPC;
//! NATS carries only durable events, commands and edge traffic. Payloads are
//! the protobuf messages named on each item.

/// Work-queue command from commerce to dispatch ([`DispatchRequest`]), published
/// only once an order is PAID.
///
/// [`DispatchRequest`]: crate::dronedrop::events::v1::DispatchRequest
pub const DISPATCH_REQUEST: &str = "dispatch.request";

/// [`GrantRevoked`](crate::dronedrop::events::v1::GrantRevoked).
pub const AUTH_GRANT_REVOKED: &str = "auth.events.grant_revoked";
/// [`UserDeleted`](crate::dronedrop::events::v1::UserDeleted).
pub const AUTH_USER_DELETED: &str = "auth.events.user_deleted";
/// [`PickupPointVerified`](crate::dronedrop::events::v1::PickupPointVerified).
pub const MERCHANT_PICKUP_POINT_VERIFIED: &str = "merchant.pickup_point.verified";

// JetStream streams and buckets.
/// `order.>`
pub const STREAM_ORDERS: &str = "ORDERS";
/// `mission.>`, sourced from the edge domains.
pub const STREAM_MISSIONS: &str = "MISSIONS";
/// 1 Hz telemetry, sourced from the edge domains.
pub const STREAM_TELEMETRY: &str = "TELEMETRY";
/// `dispatch.request`, work-queue retention.
pub const STREAM_DISPATCH_REQUESTS: &str = "DISPATCH_REQUESTS";
/// `auth.events.>`
pub const STREAM_AUTH_EVENTS: &str = "AUTH_EVENTS";
/// `merchant.business.>` and `merchant.pickup_point.>`
pub const STREAM_MERCHANT_EVENTS: &str = "MERCHANT_EVENTS";
/// `commerce.merchant_account.>`
pub const STREAM_COMMERCE_EVENTS: &str = "COMMERCE_EVENTS";
pub const KV_ORDER_VIEW: &str = "ORDER_VIEW";
pub const OBJECT_STORE_PICKUP_ASSETS: &str = "PICKUP_ASSETS";

/// `order.<order_id>.<event>` ([`OrderEvent`]); `event` is the detail case,
/// e.g. `paid` or `payment_failed`.
///
/// [`OrderEvent`]: crate::dronedrop::events::v1::OrderEvent
pub fn order_event(order_id: &str, event: &str) -> String {
    format!("order.{order_id}.{event}")
}

/// `merchant.business.<business_id>.status_changed` ([`BusinessStatusChanged`]).
///
/// [`BusinessStatusChanged`]: crate::dronedrop::events::v1::BusinessStatusChanged
pub fn business_status_changed(business_id: &str) -> String {
    format!("merchant.business.{business_id}.status_changed")
}

/// `commerce.merchant_account.<business_id>.updated` ([`MerchantAccountUpdated`]).
///
/// [`MerchantAccountUpdated`]: crate::dronedrop::events::v1::MerchantAccountUpdated
pub fn merchant_account_updated(business_id: &str) -> String {
    format!("commerce.merchant_account.{business_id}.updated")
}

/// `cmd.edge.<zone>.<command>` ([`EdgeCommand`]): assign, recall, handoff or loaded.
///
/// [`EdgeCommand`]: crate::dronedrop::edge::v1::EdgeCommand
pub fn edge_command(zone: &str, command: &str) -> String {
    format!("cmd.edge.{zone}.{command}")
}

/// Wildcard matching every command for one zone.
pub fn edge_commands(zone: &str) -> String {
    format!("cmd.edge.{zone}.>")
}

/// `mission.<order_id>.<event>` ([`MissionEvent`]).
///
/// [`MissionEvent`]: crate::dronedrop::events::v1::MissionEvent
pub fn mission_event(order_id: &str, event: &str) -> String {
    format!("mission.{order_id}.{event}")
}

/// `tlm.raw.<zone>.<drone_id>` ([`Telemetry`]): 10 Hz, stays inside the edge zone.
///
/// [`Telemetry`]: crate::dronedrop::edge::v1::Telemetry
pub fn telemetry_raw(zone: &str, drone_id: &str) -> String {
    format!("tlm.raw.{zone}.{drone_id}")
}

/// `metrics.edge.<zone>` ([`EdgeMetrics`]).
///
/// [`EdgeMetrics`]: crate::dronedrop::edge::v1::EdgeMetrics
pub fn edge_metrics(zone: &str) -> String {
    format!("metrics.edge.{zone}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_subjects() {
        assert_eq!(order_event("o1", "paid"), "order.o1.paid");
        assert_eq!(business_status_changed("b1"), "merchant.business.b1.status_changed");
        assert_eq!(merchant_account_updated("b1"), "commerce.merchant_account.b1.updated");
        assert_eq!(edge_command("sea-north", "loaded"), "cmd.edge.sea-north.loaded");
        assert_eq!(edge_commands("sea-south"), "cmd.edge.sea-south.>");
        assert_eq!(mission_event("o1", "visual_lock"), "mission.o1.visual_lock");
        assert_eq!(telemetry_raw("sea-north", "d7"), "tlm.raw.sea-north.d7");
    }
}
