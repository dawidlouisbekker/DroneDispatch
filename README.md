# AWS Hackathon

## Drone Drop
Voice-commerce drone delivery simulation for Alexa+: an MCP server lets Alexa find shops by category with Amazon Location Service, read out their menus and order by voice. The business accepts the order in its portal; a drone in the business's edge zone collects it from the business's station and delivers it to the customer's station, authenticating both over Bluetooth with mutual TLS.
Rust microservices over gRPC and NATS, with the catalog's read side and the drone fleet running in MEC edge zones.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Status: auth-service is implemented; the other services are scaffolded (they build, boot and serve `/healthz`).

## Repository layout
| Path | What | Runs | Local port |
|---|---|---|---|
| [services/auth-service](services/auth-service/README.md) | OAuth 2.1 authorization server, passkeys | Cloud | 8081 |
| [services/user-service](services/user-service/README.md) | Customer API, Alexa+ MCP server, orders and order history, pickup locations and stations | Cloud | 8085 |
| [services/merchant-service](services/merchant-service/README.md) | Business onboarding, catalog and stock (CQRS write side), fulfilment, stations, payouts | Cloud | 8083 |
| [services/catalog-read-service](services/catalog-read-service/README.md) | Read-only catalogs keyed by Amazon place ID (CQRS read side) | Edge, per zone | 8084 (sea-north), 8094 (sea-south) |
| [services/dispatch-service](services/dispatch-service/README.md) | Drone fleet: missions, flight simulation, station handshakes | Edge, per zone | 8082 (sea-north), 8092 (sea-south) |
| `ui/app` | Expo app: customer area and merchant portal | | 8087 (dev server), 8086 (gateway) |
| [proto/](proto/dronedrop) | Protobuf contracts: gRPC services and NATS event payloads | | |
| `crates/contracts` | Rust code generated from `proto/`, plus NATS subject names | | |
| `crates/station` | Station access networks (Bluetooth LE first) and public-key pinning | | |
| `crates/geo` | Coordinates, distance, ETA | | |
| `crates/svc-auth` | Token claims and `WWW-Authenticate` challenges for resource servers | | |
| `crates/svc-common` | Config and `.env` loading, tracing, NATS, Postgres, HTTP server, CORS, outbox | | |
| [api/openapi](api/openapi) | HTTP APIs: auth, user, merchant, catalog | | |
| [docs/DATABASE.md](docs/DATABASE.md) | Database per service, CQRS projections, references across services | | |
| [EXTERNAL_CLOUD_SERVICES_SETUP.md](EXTERNAL_CLOUD_SERVICES_SETUP.md) | Setting up Stripe and AWS: keys, Connect, webhooks, IAM, map key | | |
| `config/` | Postgres init, NATS hub and leaf configs, toxiproxy, Caddy gateway, Seattle zones | | |
| `scripts/` | MEC demo (partition, heal, add latency), schema tests, signing key | | |

```bash
cargo test --workspace         # unit tests; database schema tests show as "ignored"
docker compose up --build      # infrastructure, cloud services and both edge zones

# Run one service from source (settings from services/<svc>/.env, then the root .env):
cargo run -p user-service

# Database schema tests against Postgres on port 5432 (starts it if needed):
scripts/test-db.sh                       # every service
scripts/test-db.sh -p merchant-service   # one service
```

Services call each other over gRPC on internal ports (auth 9081, dispatch 9082, merchant 9083, catalog-read 9084; servers not wired up yet) and publish events on NATS JetStream. Edge services reach the cloud only through their zone's NATS leaf node.

## Hackathon brief

**Alexa+**
Experience how brands in Preview are building and shipping experiences on Alexa+ that our customers love:
Build a self-hosted MCP server (spec 2025-11-25 or later, Streamable HTTP) or an Agent Skill. 
MCP is the open standard that powers Alexa+ integrations.
New to MCP? Build a simulated Alexa+ experience in a web app using your preferred agentic tool.

your repo needs to actually call your track's required technology in code, an import, an entry point, a loaded agent/flow/MCP config, not just a mention in the README.
