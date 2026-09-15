# edge-node

Runs inside a (simulated) MEC edge zone, next to the drones. It flies them, reads the business's pickup marker for a precise landing, and keeps working when the zone loses its link to the cloud. There is one instance per zone; locally these are Seattle `sea-north` and `sea-south`.

Why at the edge: image processing and flight control run next to the drone, not in the cloud region. That keeps latency low, and a network partition doesn't ground the fleet.

**Status:** scaffold. The node connects to its zone's NATS leaf, logs every `cmd.edge.<zone>.*` command it receives, and serves `/healthz`. Simulation and vision are milestone 6; the MEC demos are milestone 8.

## Responsibilities
- **Flight simulation** (10 Hz, deterministic):
  - States: `IDLE → TAKEOFF → TO_PICKUP → APPROACH → VISUAL_LOCK → LOADING → TO_CUSTOMER → DROPOFF → RETURNING → CHARGING`.
  - Cruise at 15 m/s and 60 m altitude, with a 25-minute battery. Abort when the battery reserve runs low.
  - Keep 30 m separation between drones, and route around no-fly polygons (starting with KBFI, Boeing Field).
- **Precision pickup (edge vision):**
  1. Within 30 m of the pickup point, the drone enters `APPROACH`. The node builds a simulated camera frame from the business's verified photo (local `PICKUP_ASSETS` mirror), randomly rotated, scaled and noised.
  2. It decodes the QR marker and requires its `pickup_id` and HMAC to match the mission.
  3. On a match, the drone enters `VISUAL_LOCK` and lands at the pad coordinates.
  4. After 3 failed attempts it hovers and publishes `mission.<order>.pickup_failed`, which leads to a refund.
  - Vision latency is reported on `metrics.edge.<zone>`.
- **Loading:** the business taps **Loaded onto drone** in the portal (merchant-service calls `Fleet.ConfirmLoaded`, which sends `cmd.edge.<zone>.loaded`), or in demo mode it happens automatically after 30 seconds. The drone then moves to `TO_CUSTOMER`.
- **Partition resilience:** missions, telemetry and events buffer in the zone's JetStream domain and sync to the hub when the link returns. Commands for the zone wait on the hub.
- **Handoff:** when a drone crosses into a neighbouring zone, the node hands it off to that zone's node, if the hub link is up.

## Owns
No Postgres. State lives in the zone's local JetStream domain: a KV bucket and the mirrored `PICKUP_ASSETS` object store.

## Interfaces
**HTTP**
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness | Done |

**NATS** (all through the zone's leaf node; protobuf payloads from [`edge.proto`](../../proto/dronedrop/edge/v1/edge.proto) and [`fleet_events.proto`](../../proto/dronedrop/events/v1/fleet_events.proto)). edge-node has no gRPC API, so it keeps working while its zone is cut off.
| Direction | Subjects | Kind |
|---|---|---|
| Consumes | `cmd.edge.<zone>.{assign,recall,handoff,loaded}` (`EdgeCommand`) | Command (scaffold logs them) |
| Publishes | `mission.<order_id>.{assigned,at_pickup,visual_lock,pickup_failed,picked_up,delivered,aborted}` (stream `MISSIONS`) | Event |
| Publishes | `tlm.raw.<zone>.<drone>` (10 Hz, stays in the zone); `TELEMETRY` stream (1 Hz, synced to the hub) | Telemetry |
| Publishes | `metrics.edge.<zone>` | Metrics |
| Reads | `PICKUP_ASSETS` object store mirror | Object store |

## Network topology (local)
```
edge-sea-north → nats-leaf-sea-north → toxiproxy :7423 → nats-hub :7422
edge-sea-south → nats-leaf-sea-south → toxiproxy :7424 → nats-hub :7422
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
| `NATS_URL` | `nats://localhost:4222` | The zone's leaf node, not the hub |
| `HTTP_ADDR` | `0.0.0.0:8090` | |

## Run
```bash
docker compose up edge-sea-north
EDGE_ZONE=sea-north cargo run -p edge-node

# Send a test command; it shows up in the node's log:
nats pub cmd.edge.sea-north.assign '{}'
```
