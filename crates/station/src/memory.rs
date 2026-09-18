//! An in-memory [`AccessNetwork`] for the flight simulator and tests. Stations
//! register by id; connecting hands the station one end of an in-process duplex
//! pipe and returns the other end to the drone.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use tokio::{
    io::{DuplexStream, duplex},
    sync::mpsc,
};

use crate::{AccessNetwork, NetworkKind, NetworkParams, RangingMethod, Sighting, StationError};

/// Buffer size of each simulated connection, in bytes.
const BUFFER_BYTES: usize = 16 * 1024;

/// Simulates Bluetooth LE: every registered station is in range.
#[derive(Default)]
pub struct MemoryNetwork {
    stations: Mutex<HashMap<String, mpsc::UnboundedSender<DuplexStream>>>,
}

impl MemoryNetwork {
    /// Registers a station. Its incoming connections arrive on the returned receiver.
    pub fn register(&self, station_id: &str) -> mpsc::UnboundedReceiver<DuplexStream> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.stations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(station_id.to_owned(), sender);
        receiver
    }

    fn sender(&self, station_id: &str) -> Option<mpsc::UnboundedSender<DuplexStream>> {
        self.stations.lock().unwrap_or_else(PoisonError::into_inner).get(station_id).cloned()
    }
}

impl AccessNetwork for MemoryNetwork {
    type Stream = DuplexStream;

    fn kind(&self) -> NetworkKind {
        NetworkKind::BluetoothLe
    }

    async fn find(&self, station_id: &str, params: &NetworkParams) -> Result<Sighting, StationError> {
        if params.kind() != self.kind() || self.sender(station_id).is_none() {
            return Err(StationError::NotFound(self.kind()));
        }
        Ok(Sighting {
            station_id: station_id.to_owned(),
            params: params.clone(),
            ranging_method: RangingMethod::Rssi,
            distance_m: None,
        })
    }

    async fn connect(&self, sighting: &Sighting) -> Result<Self::Stream, StationError> {
        let sender = self.sender(&sighting.station_id).ok_or(StationError::NotFound(self.kind()))?;
        let (drone_end, station_end) = duplex(BUFFER_BYTES);
        sender.send(station_end).map_err(|_| StationError::NotFound(self.kind()))?;
        Ok(drone_end)
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    fn bluetooth() -> NetworkParams {
        NetworkParams::BluetoothLe {
            service_uuid: "6e400001-b5a3-f393-e0a9-e50e24dcca9e".into(),
            station_tag: vec![0x0a, 0x0b],
            l2cap_psm: 128,
        }
    }

    #[tokio::test]
    async fn finds_registered_stations_and_connects_to_them() {
        let network = MemoryNetwork::default();
        let mut incoming = network.register("station-1");

        assert!(matches!(
            network.find("station-2", &bluetooth()).await,
            Err(StationError::NotFound(NetworkKind::BluetoothLe))
        ));

        let sighting = network.find("station-1", &bluetooth()).await.unwrap();
        let mut drone = network.connect(&sighting).await.unwrap();
        let mut station = incoming.recv().await.unwrap();

        drone.write_all(b"mission-1").await.unwrap();
        let mut received = [0u8; 9];
        station.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"mission-1");
    }
}
