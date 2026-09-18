# catalog-read-service

The **read side** of the catalog (CQRS). One instance runs in each MEC edge zone (locally `sea-north` and `sea-south`) and serves shops, menus and item availability, keyed by the Amazon Location place ID each business claimed during onboarding. It is read-only: every change goes to merchant-service, which publishes the new state.

Why a separate service: catalog reads are by far the most frequent calls (every voice search and menu read). Serving them from denormalized per-zone projections keeps them fast, lets them scale out without touching merchant-service, takes the load off user-service, and keeps them working while a zone is cut off from the cloud.

**Status:** scaffold. The service boots, applies its migrations, connects to its zone's NATS leaf, and serves `/healthz`; the read routes return `501`. Projections and the gRPC server are milestone 3.

## Responsibilities
- **Project** merchant-service's state events (`merchant.state.catalog.<place_id>`, `merchant.state.section.<id>`, `merchant.state.item.<id>`, stream `MERCHANT_EVENTS`, sourced into the zone's JetStream domain) into read tables, following the rules in [DATABASE.md](../../docs/DATABASE.md#cqrs-catalog-projections): inbox for duplicates, apply only newer stream sequences, checkpoint per consumer.
- **Serve** `CatalogRead` over gRPC to user-service (the MCP tools and the customer app) and a public read-only HTTP API.
- **Join search results:** `BatchGetCatalogsByPlaceIds` takes the place IDs from Amazon Location SearchNearby and returns only those with a Drone Drop catalog, with the spoken names Alexa+ reads out.
- **Report** projection lag on `/healthz`.
- **Rebuild** a zone by clearing the read tables and replaying `MERCHANT_EVENTS`, which keeps the latest state per subject.

## Owns (Postgres databases `catalog_read_<zone>`)
Schema in [`migrations/`](migrations/), applied at start-up to the zone's database.
- `catalogs`: keyed by `place_id`; spoken name, position, categories, accepting orders
- `catalog_sections`, `catalog_items`: menus with spoken names, prices, weights, availability, stock remaining
- `projection_checkpoints`: stream sequence per consumer
- `inbox`

## Interfaces
**HTTP** (public, no token)
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness (projection lag planned) | Done |
| `GET /v1/catalogs?place_ids=…` | Catalogs for place IDs | Scaffold |
| `GET /v1/catalogs/{place_id}` | One catalog | Scaffold |
| `GET /v1/catalogs/{place_id}/menu` | Menu sections and items | Scaffold |

**gRPC server** (internal port 9084, [`catalog.proto`](../../proto/dronedrop/catalog/v1/catalog.proto))
| Service | RPCs | Called by |
|---|---|---|
| `CatalogRead` | `BatchGetCatalogsByPlaceIds`, `GetCatalog`, `GetMenu`, `BatchGetItems` | user |

**NATS**
| Direction | Subjects | Payload |
|---|---|---|
| Consumes | `merchant.state.>` (stream `MERCHANT_EVENTS`, via the zone's leaf) | `CatalogState`, `CatalogSectionState`, `CatalogItemState` |

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `EDGE_ZONE` | required | `sea-north` or `sea-south` |
| `DATABASE_URL` | required | e.g. `postgres://catalog_read:catalog_read@localhost:5432/catalog_read_sea_north` |
| `NATS_URL` | `nats://localhost:4222` | The zone's leaf node, not the hub (Compose publishes sea-north's on `4223`) |
| `HTTP_ADDR` | `0.0.0.0:8084` | |
| `CORS_ALLOWED_ORIGINS` | unset | Browser origins allowed without ui-gateway |

## Run
```bash
docker compose up catalog-read-sea-north

# Or from source, against the Compose Postgres and the sea-north NATS leaf:
cp services/catalog-read-service/.env.example services/catalog-read-service/.env
cargo run -p catalog-read-service
```
