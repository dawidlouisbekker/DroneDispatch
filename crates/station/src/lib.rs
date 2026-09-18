//! How a drone reaches and recognises a station at a pickup or drop-off point.
//!
//! A station can be reachable over several [`AccessNetwork`]s: Bluetooth LE first,
//! Wi-Fi and others later. Each implementation finds the station near its position
//! and opens a byte stream to it. The TLS 1.3 handshake on top of that stream is the
//! same for every network: the drone accepts only the station key pinned in its
//! mission ([`pinned_key_matches`]), and the station checks the drone's certificate
//! against the zone fleet CA. See docs/ARCHITECTURE.md, flow 5.

pub mod memory;

use std::fmt::Write;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};

/// Access network kinds, matching `dronedrop.station.v1.AccessNetwork` and the `kind`
/// columns in the databases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NetworkKind {
    BluetoothLe,
}

impl NetworkKind {
    /// The value stored in `kind` and `access_network` columns.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BluetoothLe => "BLUETOOTH_LE",
        }
    }
}

/// How to find one station on one access network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkParams {
    BluetoothLe {
        /// Platform-wide service UUID every station advertises.
        service_uuid: String,
        /// Identifies the station in its advertisement; BLE addresses rotate, so the MAC is never used.
        station_tag: Vec<u8>,
        /// L2CAP connection-oriented channel the TLS session runs over.
        l2cap_psm: u16,
    },
}

impl NetworkParams {
    pub const fn kind(&self) -> NetworkKind {
        match self {
            Self::BluetoothLe { .. } => NetworkKind::BluetoothLe,
        }
    }
}

/// The station a mission leg flies to, as copied into the mission.
#[derive(Clone, Debug)]
pub struct StationTarget {
    pub station_id: String,
    /// Lowercase hex SHA-256 of the station's DER SubjectPublicKeyInfo.
    pub public_key_sha256: String,
    /// In the order they should be tried.
    pub networks: Vec<NetworkParams>,
}

/// How the distance to a station was measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangingMethod {
    Gnss,
    Rssi,
    ChannelSounding,
}

/// A station found on an access network, ready to connect to.
#[derive(Clone, Debug)]
pub struct Sighting {
    pub station_id: String,
    pub params: NetworkParams,
    pub ranging_method: RangingMethod,
    /// Estimated distance to the station in metres, when known.
    pub distance_m: Option<f32>,
}

#[derive(Debug, thiserror::Error)]
pub enum StationError {
    #[error("station not found on {0:?}")]
    NotFound(NetworkKind),
    #[error("the {0:?} access network is not available on this device")]
    Unavailable(NetworkKind),
    #[error("connecting to the station failed: {0}")]
    Connect(#[from] std::io::Error),
}

/// One way of reaching stations. Implementations: Bluetooth LE (planned) and
/// [`memory::MemoryNetwork`] for the flight simulator and tests.
#[allow(async_fn_in_trait)] // implementations are used as concrete types, so no Send bound is needed
pub trait AccessNetwork {
    /// Byte stream to a station; the TLS session runs on top of it.
    type Stream: AsyncRead + AsyncWrite + Unpin + Send;

    fn kind(&self) -> NetworkKind;

    /// Looks for the station described by `params`, which must be of this network's kind.
    async fn find(&self, station_id: &str, params: &NetworkParams) -> Result<Sighting, StationError>;

    async fn connect(&self, sighting: &Sighting) -> Result<Self::Stream, StationError>;
}

/// Lowercase hex SHA-256 of a DER SubjectPublicKeyInfo, as stored in `public_key_sha256`.
pub fn public_key_sha256(public_key_der: &[u8]) -> String {
    Sha256::digest(public_key_der).as_slice().iter().fold(String::with_capacity(64), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// Whether `public_key_der` is the key pinned as `expected_sha256` (lowercase hex).
/// Compared in constant time.
pub fn pinned_key_matches(public_key_der: &[u8], expected_sha256: &str) -> bool {
    let actual = public_key_sha256(public_key_der);
    actual.len() == expected_sha256.len()
        && actual.bytes().zip(expected_sha256.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_keys_as_lowercase_hex() {
        // SHA-256 of the empty input.
        assert_eq!(public_key_sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn accepts_only_the_pinned_key() {
        let key = b"station public key";
        let pinned = public_key_sha256(key);
        assert!(pinned_key_matches(key, &pinned));
        assert!(!pinned_key_matches(b"another key", &pinned));
        assert!(!pinned_key_matches(key, &pinned.to_uppercase()));
        assert!(!pinned_key_matches(key, &pinned[..63]));
    }

    #[test]
    fn network_kinds_match_the_database_values() {
        let params = NetworkParams::BluetoothLe {
            service_uuid: "6e400001-b5a3-f393-e0a9-e50e24dcca9e".into(),
            station_tag: vec![0x0a, 0x0b],
            l2cap_psm: 128,
        };
        assert_eq!(params.kind().as_str(), "BLUETOOTH_LE");
    }
}
