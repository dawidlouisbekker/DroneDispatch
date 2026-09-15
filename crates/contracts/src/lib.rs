//! Contracts between Drone Drop services, generated from the `.proto` files in
//! the repository's `proto/` directory:
//!
//! - gRPC clients and servers for service-to-service calls, e.g.
//!   `dronedrop::merchant::v1::shop_catalog_client::ShopCatalogClient`.
//! - Protobuf payloads for NATS events and edge commands, in
//!   [`dronedrop::events::v1`] and [`dronedrop::edge::v1`].
//!
//! Subject, stream and bucket names live in [`subjects`]. Evolve messages
//! compatibly: add fields with new numbers and never reuse or renumber one. The
//! conventions are at the top of `proto/dronedrop/common/v1/common.proto`.

pub mod subjects;

#[allow(clippy::all, clippy::pedantic)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/dronedrop.rs"));
}

pub use generated::dronedrop;

/// Encoded `FileDescriptorSet` of every proto, for gRPC server reflection.
pub const FILE_DESCRIPTOR_SET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/descriptor.bin"));

impl From<geo::LatLon> for dronedrop::common::v1::LatLon {
    fn from(point: geo::LatLon) -> Self {
        Self { lat: point.lat, lon: point.lon }
    }
}

impl From<dronedrop::common::v1::LatLon> for geo::LatLon {
    fn from(point: dronedrop::common::v1::LatLon) -> Self {
        Self::new(point.lat, point.lon)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use prost::Message;

    use super::dronedrop::{
        commerce::v1::OrderState,
        common::v1::{LatLon, Money},
        events::v1::{DispatchRequest, OrderAuthorized, OrderEvent, order_event},
    };

    #[test]
    fn order_event_round_trips_with_its_detail() {
        let event = OrderEvent {
            event_id: "evt-1".into(),
            order_id: "order-1".into(),
            customer_sub: "user-1".into(),
            business_id: "business-1".into(),
            state: OrderState::AwaitingMerchant.into(),
            occurred_at: None,
            detail: Some(order_event::Detail::Authorized(OrderAuthorized {
                total: Some(Money { amount_cents: 1_250, currency: "usd".into() }),
                payload_g: 800,
                ..Default::default()
            })),
        };
        let decoded = OrderEvent::decode(event.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(decoded.state(), OrderState::AwaitingMerchant);
    }

    #[test]
    fn dispatch_request_round_trips() {
        let request = DispatchRequest {
            request_id: "req-1".into(),
            order_id: "order-1".into(),
            pickup: Some(geo::LatLon::new(47.6097, -122.3422).into()),
            pickup_point_id: "pickup-1".into(),
            asset_key: "pickup-1.jpg".into(),
            dropoff: Some(LatLon { lat: 47.6205, lon: -122.3493 }),
            payload_g: 800,
            paid_at: None,
        };
        assert_eq!(DispatchRequest::decode(request.encode_to_vec().as_slice()).unwrap(), request);
    }

    #[test]
    fn descriptor_set_covers_every_package() {
        let descriptors = prost_types::FileDescriptorSet::decode(super::FILE_DESCRIPTOR_SET).unwrap();
        let packages: BTreeSet<&str> = descriptors.file.iter().map(|file| file.package()).collect();
        for package in [
            "dronedrop.common.v1",
            "dronedrop.auth.v1",
            "dronedrop.merchant.v1",
            "dronedrop.commerce.v1",
            "dronedrop.dispatch.v1",
            "dronedrop.edge.v1",
            "dronedrop.events.v1",
        ] {
            assert!(packages.contains(package), "{package} missing");
        }
    }

    #[test]
    fn lat_lon_converts_both_ways() {
        let point = geo::LatLon::new(47.6205, -122.3493);
        let proto: LatLon = point.into();
        assert_eq!(geo::LatLon::from(proto), point);
    }
}
