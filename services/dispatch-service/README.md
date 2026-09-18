# dispatch-service

The drone fleet of one MEC edge zone. One instance runs in each zone (locally `sea-north` and `sea-south`), next to its drones: it turns dispatch requests into missions, flies the drones (simulated), and authenticates the stations at pickup and drop-off.

Why at the edge: flight control and station handshakes run next to the drones, so latency stays low and a network partition doesn't ground the fleet.

**Status:** scaffold. The service boots, applies its migrations, logs every `dispatch.<zone>.*` command it receives on the zone's NATS leaf, and serves `/healthz`. Missions, the simulator and the station handshake are milestone 6; the MEC demos are milestone 8.

## Responsibilities
- **Fleet data only:** docks, drones, missions and station handshakes. No customer accounts, catalogs or payments: a mission carries only what the drone needs (station ids, positions, access networks and public-key hashes).
- **Missions:**
  - Consume `dispatch.<zone>.request` and assign the nearest free drone with enough payload; retry, and report `no_drone` after 10 minutes.
  - `dispatch.<zone>.recall` aborts a mission before pickup.
  - Publish `mission.<order_id>.<event>` through the outbox.
- **Flight simulation** (10 Hz, deterministic):
  - States: `IDLE → TAKEOFF → TO_PICKUP → APPROACH → HANDSHAKE → LOADING → TO_DROPOFF → DROPOFF → RETURNING → CHARGING`.
  - Cruise at 15 m/s and 60 m altitude, with a 25-minute battery and a reserve.
  - 30 m separation between drones; route around no-fly polygons (starting with KBFI, Boeing Field).
- **Station handshake** ([ARCHITECTURE.md](../../docs/ARCHITECTURE.md), flow 5):
  1. Fly to the station's position (GNSS).
  2. Within about 50 m, scan the station's access networks in order. Bluetooth LE first: match the platform service UUID and the station's tag.
  3. Open an L2CAP connection-oriented channel and run TLS 1.3 with mutual authentication: accept only the station key pinned in the mission; present the drone's certificate from the zone fleet CA.
  4. Exchange the mission id and a nonce; the station acknowledges `loaded` or `received`.
  5. Record every attempt in `station_handshakes` and publish `station_verified` or `station_failed`.
  - The transport abstraction (`AccessNetwork`) lives in `crates/station`, so Wi-Fi or UWB can be added later.
- **Partition resilience:** mission events and telemetry buffer in the zone's JetStream domain; requests queued on the hub are delivered when the link returns.

## Owns (Postgres databases `dispatch_<zone>`)
Schema in [`migrations/`](migrations/), applied at start-up to the zone's database. Conventions: [DATABASE.md](../../docs/DATABASE.md).
- `docks`: charging and launch docks
- `drones`: model, payload limit, status, battery, last position, certificate hash
- `missions`: one per order, with both legs' station details
- `station_handshakes`: every authentication attempt
- `outbox`, `inbox`

## Interfaces
**HTTP**
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness | Done |

**gRPC server** (internal port 9082, [`dispatch.proto`](../../proto/dronedrop/dispatch/v1/dispatch.proto))
| Service | RPCs | Called by |
|---|---|---|
| `Fleet` | `GetFleetSnapshot`, `GetMission` | user (live map) |

**NATS** (all through the zone's leaf node; payloads from [`fleet_events.proto`](../../proto/dronedrop/events/v1/fleet_events.proto) and `dispatch.proto`)
| Direction | Subjects | Payload |
|---|---|---|
| Consumes | `dispatch.<zone>.request`, `dispatch.<zone>.recall` (stream `DISPATCH_REQUESTS`) | `DispatchRequest`, `RecallMission` |
| Publishes | `mission.<order_id>.<event>` (stream `MISSIONS`, sourced to the hub) | `MissionEvent` |
| Publishes | `tlm.raw.<zone>.<drone>` (10 Hz, stays in the zone); `TELEMETRY` stream (1 Hz, synced to the hub) | `Telemetry` |

## Network topology (local)
```
dispatch-sea-north, catalog-read-sea-north → nats-leaf-sea-north → toxiproxy :7423 → nats-hub :7422
dispatch-sea-south, catalog-read-sea-south → nats-leaf-sea-south → toxiproxy :7424 → nats-hub :7422
```
toxiproxy sits on each zone's uplink, so the demo scripts can cut it or slow it down:
```bash
scripts/partition-edge.sh sea-north      # cut the uplink
scripts/add-latency.sh sea-north 150     # add 150 ms latency
scripts/heal-edge.sh sea-north           # restore the link and remove latency
```

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `EDGE_ZONE` | required | `sea-north` or `sea-south` |
| `DATABASE_URL` | required | e.g. `postgres://dispatch:dispatch@localhost:5432/dispatch_sea_north` |
| `NATS_URL` | `nats://localhost:4222` | The zone's leaf node, not the hub (Compose publishes sea-north's on `4223`) |
| `HTTP_ADDR` | `0.0.0.0:8082` | |

## Run
```bash
docker compose up dispatch-sea-north

# Or from source, against the Compose Postgres and the sea-north NATS leaf:
cp services/dispatch-service/.env.example services/dispatch-service/.env
cargo run -p dispatch-service

# Send a test command; it shows up in the log:
nats --server nats://localhost:4223 pub dispatch.sea-north.request '{}'
```
