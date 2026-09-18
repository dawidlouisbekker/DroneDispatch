//! NATS subject and stream names. Service-to-service calls use gRPC; NATS carries
//! durable events, commands, and all traffic to and from the edge zones. Payloads
//! are the protobuf messages named on each item.

/// [`GrantRevoked`](crate::dronedrop::events::v1::GrantRevoked).
pub const AUTH_GRANT_REVOKED: &str = "auth.events.grant_revoked";
/// [`UserDeleted`](crate::dronedrop::events::v1::UserDeleted).
pub const AUTH_USER_DELETED: &str = "auth.events.user_deleted";

// JetStream streams.
/// `auth.events.>`
pub const STREAM_AUTH_EVENTS: &str = "AUTH_EVENTS";
/// `merchant.>`: catalog state (latest message per subject) and fulfilment events.
pub const STREAM_MERCHANT_EVENTS: &str = "MERCHANT_EVENTS";
/// `dispatch.>`: dispatch commands, work-queue retention, sourced into each zone.
pub const STREAM_DISPATCH_REQUESTS: &str = "DISPATCH_REQUESTS";
/// `mission.>`, sourced from the edge zones.
pub const STREAM_MISSIONS: &str = "MISSIONS";
/// 1 Hz telemetry, sourced from the edge zones.
pub const STREAM_TELEMETRY: &str = "TELEMETRY";

/// Every catalog state subject: what catalog-read-service projects.
pub const MERCHANT_STATE: &str = "merchant.state.>";

/// `merchant.state.catalog.<place_id>` ([`CatalogState`]). Amazon place IDs are used
/// as a subject token, so they must not contain `.`, `*`, `>` or whitespace.
///
/// [`CatalogState`]: crate::dronedrop::events::v1::CatalogState
pub fn catalog_state(place_id: &str) -> String {
    format!("merchant.state.catalog.{place_id}")
}

/// `merchant.state.section.<section_id>` ([`CatalogSectionState`]).
///
/// [`CatalogSectionState`]: crate::dronedrop::events::v1::CatalogSectionState
pub fn section_state(section_id: &str) -> String {
    format!("merchant.state.section.{section_id}")
}

/// `merchant.state.item.<item_id>` ([`CatalogItemState`]).
///
/// [`CatalogItemState`]: crate::dronedrop::events::v1::CatalogItemState
pub fn item_state(item_id: &str) -> String {
    format!("merchant.state.item.{item_id}")
}

/// `merchant.fulfilment.<order_id>.<status>` ([`FulfilmentEvent`]); `status` in
/// lower case, e.g. `accepted`.
///
/// [`FulfilmentEvent`]: crate::dronedrop::events::v1::FulfilmentEvent
pub fn fulfilment_event(order_id: &str, status: &str) -> String {
    format!("merchant.fulfilment.{order_id}.{status}")
}

/// `dispatch.<zone>.request` ([`DispatchRequest`]): from user-service to the zone that
/// contains the pickup station, once the order is paid.
///
/// [`DispatchRequest`]: crate::dronedrop::events::v1::DispatchRequest
pub fn dispatch_request(zone: &str) -> String {
    format!("dispatch.{zone}.request")
}

/// `dispatch.<zone>.recall` ([`RecallMission`]).
///
/// [`RecallMission`]: crate::dronedrop::events::v1::RecallMission
pub fn dispatch_recall(zone: &str) -> String {
    format!("dispatch.{zone}.recall")
}

/// Wildcard matching every dispatch command for one zone.
pub fn dispatch_commands(zone: &str) -> String {
    format!("dispatch.{zone}.>")
}

/// `mission.<order_id>.<event>` ([`MissionEvent`]); `event` is the kind in lower
/// case, e.g. `station_verified`.
///
/// [`MissionEvent`]: crate::dronedrop::events::v1::MissionEvent
pub fn mission_event(order_id: &str, event: &str) -> String {
    format!("mission.{order_id}.{event}")
}

/// `tlm.raw.<zone>.<drone_id>` ([`Telemetry`]): 10 Hz, stays inside the edge zone.
///
/// [`Telemetry`]: crate::dronedrop::dispatch::v1::Telemetry
pub fn telemetry_raw(zone: &str, drone_id: &str) -> String {
    format!("tlm.raw.{zone}.{drone_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_subjects() {
        assert_eq!(catalog_state("AQAB-place"), "merchant.state.catalog.AQAB-place");
        assert_eq!(section_state("s1"), "merchant.state.section.s1");
        assert_eq!(item_state("i1"), "merchant.state.item.i1");
        assert_eq!(fulfilment_event("o1", "accepted"), "merchant.fulfilment.o1.accepted");
        assert_eq!(dispatch_request("sea-north"), "dispatch.sea-north.request");
        assert_eq!(dispatch_recall("sea-south"), "dispatch.sea-south.recall");
        assert_eq!(dispatch_commands("sea-south"), "dispatch.sea-south.>");
        assert_eq!(mission_event("o1", "station_verified"), "mission.o1.station_verified");
        assert_eq!(telemetry_raw("sea-north", "d7"), "tlm.raw.sea-north.d7");
    }
}
