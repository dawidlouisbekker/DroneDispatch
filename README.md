# AWS Hackathon

## Drone Drop
Voice-commerce drone delivery simulation for Alexa+: an MCP server lets Alexa find shops via Amazon Location Service, build an order conversationally, and pay by voice (Stripe Connect). Drones are dispatched only after the business accepts and payment succeeds.
Microservices in Rust over NATS, with drone flight simulated on MEC edge zones and a live customer map.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Status: scaffolded. Every service builds, boots and serves `/healthz`; features are not implemented yet.

## Repository layout
| Path | What | Local port |
|---|---|---|
| [services/auth-service](services/auth-service/README.md) | OAuth 2.1 authorization server, MFA | 8081 |
| [services/dispatch-service](services/dispatch-service/README.md) | MCP server for Alexa+, drone fleet orchestration | 8082 |
| [services/merchant-service](services/merchant-service/README.md) | Business registration, menus, stock, pickup markers, portal | 8083 |
| [services/commerce-service](services/commerce-service/README.md) | Order saga, Stripe Connect, history, delivery locations | 8084 |
| [services/map-service](services/map-service/README.md) | Customer web app with live drone map | 8085 |
| [services/edge-node](services/edge-node/README.md) | Flight simulation and marker vision, one per MEC zone | 8091, 8092 |
| [proto/](proto/dronedrop) | Protobuf contracts: gRPC services and NATS event payloads | |
| `crates/contracts` | Rust code generated from `proto/`, plus NATS subject names | |
| [docs/DATABASE.md](docs/DATABASE.md) | Database per service, schema conventions, references across services | |
| `crates/geo` | Coordinates, distance, ETA | |
| `crates/svc-auth` | Token claims and `WWW-Authenticate` challenges for resource servers | |
| `crates/svc-common` | Config, tracing, NATS, Postgres, HTTP server | |
| `config/` | Postgres init, NATS hub and leaf configs, toxiproxy | |
| `scripts/` | MEC demo: partition, heal, add latency | |

```bash
cargo test --workspace         # build and run the unit tests
docker compose up --build      # infrastructure, services and both edge zones

# Schema tests against real Postgres (port 5432):
docker compose up -d --wait postgres
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test --workspace -- --ignored
```

Services call each other over gRPC on internal ports (auth 9081, dispatch 9082, merchant 9083, commerce 9084; servers not wired up yet) and publish events on NATS JetStream.

## Hackathon brief

**Alexa+**
Experience how brands in Preview are building and shipping experiences on Alexa+ that our customers love:
Build a self-hosted MCP server (spec 2025-11-25 or later, Streamable HTTP) or an Agent Skill. 
MCP is the open standard that powers Alexa+ integrations.
New to MCP? Build a simulated Alexa+ experience in a web app using your preferred agentic tool.

your repo needs to actually call your track's required technology in code, an import, an entry point, a loaded agent/flow/MCP config, not just a mention in the README.