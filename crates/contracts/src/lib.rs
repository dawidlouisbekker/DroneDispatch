//! Contracts between Drone Drop services, generated from the `.proto` files in
//! the repository's `proto/` directory:
//!
//! - gRPC clients and servers for service-to-service calls, e.g.
//!   `dronedrop::catalog::v1::catalog_read_client::CatalogReadClient`.
//! - Protobuf payloads for NATS events and commands, in [`dronedrop::events::v1`],
//!   and the station and order types they share ([`dronedrop::station::v1`],
//!   [`dronedrop::user::v1`]).
//!
//! Subject and stream names live in [`subjects`]. Evolve messages compatibly: add
//! fields with new numbers and never reuse or renumber one. The conventions are at
//! the top of `proto/dronedrop/common/v1/common.proto`.

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
        common::v1::LatLon,
        events::v1::{DispatchRequest, FulfilmentEvent},
        merchant::v1::FulfilmentStatus,
        station::v1::{AccessNetwork, BluetoothLe, OwnerKind, Station, access_network},
    };

    fn station(owner_kind: OwnerKind, position: LatLon) -> Station {
        Station {
            station_id: "station-1".into(),
            owner_kind: owner_kind.into(),
            owner_id: "owner-1".into(),
            position: Some(position),
            position_accuracy_m: 1.5,
            access_networks: vec![AccessNetwork {
                network: Some(access_network::Network::BluetoothLe(BluetoothLe {
                    service_uuid: "6e400001-b5a3-f393-e0a9-e50e24dcca9e".into(),
                    station_tag: vec![0x0a, 0x0b, 0x0c, 0x0d],
                    l2cap_psm: 128,
                })),
            }],
            public_key: vec![0x30, 0x59],
            public_key_sha256: "ab".repeat(32),
        }
    }

    #[test]
    fn dispatch_request_round_trips_with_both_stations() {
        let request = DispatchRequest {
            request_id: "req-1".into(),
            order_id: "order-1".into(),
            pickup: Some(station(OwnerKind::Business, geo::LatLon::new(47.6097, -122.3422).into())),
            dropoff: Some(station(OwnerKind::User, LatLon { lat: 47.6205, lon: -122.3493 })),
            payload_g: 800,
            paid_at: None,
        };
        let decoded = DispatchRequest::decode(request.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, request);
        assert_eq!(decoded.dropoff.unwrap().owner_kind(), OwnerKind::User);
    }

    #[test]
    fn fulfilment_event_round_trips() {
        let event = FulfilmentEvent {
            event_id: "evt-1".into(),
            order_id: "order-1".into(),
            business_id: "business-1".into(),
            status: FulfilmentStatus::Accepted.into(),
            member_sub: "member-1".into(),
            reason: String::new(),
            occurred_at: None,
        };
        let decoded = FulfilmentEvent::decode(event.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(decoded.status(), FulfilmentStatus::Accepted);
    }

    #[test]
    fn descriptor_set_covers_every_package() {
        let descriptors = prost_types::FileDescriptorSet::decode(super::FILE_DESCRIPTOR_SET).unwrap();
        let packages: BTreeSet<&str> = descriptors.file.iter().map(|file| file.package()).collect();
        for package in [
            "dronedrop.common.v1",
            "dronedrop.auth.v1",
            "dronedrop.user.v1",
            "dronedrop.merchant.v1",
            "dronedrop.catalog.v1",
            "dronedrop.dispatch.v1",
            "dronedrop.station.v1",
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
