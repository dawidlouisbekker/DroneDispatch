//! Geodesy helpers shared by the MCP tools, fleet dispatch and the edge flight
//! simulator. Zone polygons and no-fly checks land here in milestone 6.

use serde::{Deserialize, Serialize};

/// Mean Earth radius (IUGG), in metres.
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// A WGS84 coordinate in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub const fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// Great-circle distance in metres (haversine).
    pub fn distance_m(self, other: LatLon) -> f64 {
        let (lat1, lat2) = (self.lat.to_radians(), other.lat.to_radians());
        let d_lat = lat2 - lat1;
        let d_lon = (other.lon - self.lon).to_radians();
        let a = (d_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_M * a.sqrt().asin()
    }
}

/// Straight-line flight time from `from` to `to` at `speed_mps`, rounded up to
/// whole seconds.
pub fn eta_secs(from: LatLon, to: LatLon, speed_mps: f64) -> u64 {
    (from.distance_m(to) / speed_mps).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE_NEEDLE: LatLon = LatLon::new(47.6205, -122.3493);
    const PIKE_PLACE: LatLon = LatLon::new(47.6097, -122.3422);

    #[test]
    fn distance_between_seattle_landmarks() {
        let d = SPACE_NEEDLE.distance_m(PIKE_PLACE);
        assert!((1_250.0..1_400.0).contains(&d), "got {d} m");
    }

    #[test]
    fn distance_to_self_is_zero() {
        assert_eq!(SPACE_NEEDLE.distance_m(SPACE_NEEDLE), 0.0);
    }

    #[test]
    fn eta_rounds_up() {
        assert_eq!(eta_secs(SPACE_NEEDLE, PIKE_PLACE, 15.0), (SPACE_NEEDLE.distance_m(PIKE_PLACE) / 15.0).ceil() as u64);
        assert_eq!(eta_secs(SPACE_NEEDLE, SPACE_NEEDLE, 15.0), 0);
    }
}
