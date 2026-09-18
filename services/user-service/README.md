# user-service

Everything the customer owns, behind one API: orders and their full history, pickup locations, and the stations drones deliver to. It serves the customer app and the **MCP server** Alexa+ talks to.

**Status:** scaffold.
- The customer API routes exist and check tokens; most return `501`. The passkey step-up checks (verify a pickup location, approve an order) and the static parts of the fleet snapshot are real.
- `/mcp` serves the full tool list over Streamable HTTP, but every tool returns `not_implemented`, and JWTs are **not validated on `/mcp` yet**.
- Orders, payments and pickup locations are milestone 4; MCP tools are milestone 5.

## Responsibilities
- **Customer API** (`api/openapi/user.yaml`) for the app, and the live map WebSocket.
- **MCP server** (spec 2025-11-25, Streamable HTTP) at `/mcp`, built on [`rmcp`](https://crates.io/crates/rmcp), as an OAuth resource server: requests without a token get `401` pointing at the protected-resource metadata; tokens need `aud = {MCP_PUBLIC_URL}/mcp` and the `delivery` scope.
- **Shop search:** Amazon Location **SearchNearby** (by category) or **SearchText** (by name), with `IntendedUse=SingleUse`, then catalog-read-service `BatchGetCatalogsByPlaceIds` so only Drone Drop shops come back, with spoken names. Catalog reads never touch this service's database.
- **Orders** (this service owns them):
  - Quotes priced by merchant-service `Fulfilment.PriceOrder`, valid 10 minutes.
  - `place_order` checks: the quote belongs to the caller and is current, the total matches what was read back, the pickup location is verified, and the customer confirmed.
  - **Spend policy:** voice orders above the customer's approval threshold wait in `AWAITING_APPROVAL` until approved in the app with a passkey. The MCP server can never add funds or change limits.
  - Payment authorized, `Fulfilment.SubmitOrder`, captured when the business accepts, voided on rejection; only a signed payment webhook marks an order `PAID`.
  - Once paid, publishes `dispatch.<zone>.request` with both stations copied in; follows mission events to the end.
  - Every order is kept as history.
- **Pickup locations and stations:** address search, a pin on the map or the phone's GPS (with its accuracy), passkey verification, and the customer's station (public key, access networks).
- **Wallet:** may be implemented here later as the fund source the MCP server spends from. Not designed yet.

## Owns (Postgres database `user_service`)
Schema in [`migrations/`](migrations/), applied at start-up. Conventions and cross-service references: [DATABASE.md](../../docs/DATABASE.md).
- `customers`: approval threshold, Stripe customer and saved card
- `pickup_locations`: position with accuracy and source, passkey verification
- `stations`, `station_access_networks`: customer stations
- `quotes`, `quote_items`: priced carts
- `orders`, `order_items`, `order_state_transitions`, `order_approvals`: orders and their history
- `payments`, `payment_events`: payments and deduplicated webhooks
- `outbox`, `inbox`

## MCP tools
| Tool | Purpose |
|---|---|
| `search_nearby_shops(category, pickup_location_id?, radius_m?)` | SearchNearby, joined with catalogs by place ID |
| `search_places(query, pickup_location_id?)` | SearchText for a named shop, joined the same way |
| `get_shop_details(place_id)` | Catalog details, distance and ETA |
| `get_menu(place_id, query?)` | Items with spoken names, price, availability and weight |
| `list_past_order_locations(limit?)` | Shops ordered from and pickup locations used |
| `list_order_history(limit?, place_id?)` | Previous orders |
| `reorder(order_id)` | New cart from a previous order |
| `list_pickup_locations()` | Verified and pending pickup locations |
| `request_new_pickup_location(address, label)` | Creates a pending location to verify in the app |
| `update_cart(place_id, items, pickup_location_id)` | Priced quote, valid 10 minutes |
| `place_order(quote_id, expected_total_cents, customer_confirmed)` | Places the order under the spend policy |
| `get_order_status(order_id?)` | Approval, payment, acceptance and drone progress |
| `cancel_order(order_id)` | Cancels before pickup |

## Interfaces
**HTTP**
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness | Done |
| `/v1/*` | Customer API (`api/openapi/user.yaml`) | Scaffold |
| `POST`/`GET`/`DELETE /mcp` | MCP Streamable HTTP | Scaffold (stub tools, no auth) |
| `GET /.well-known/oauth-protected-resource/mcp` | MCP protected-resource metadata (RFC 9728) | Done |
| `POST /webhooks/stripe` | Signed payment webhooks | Planned |

**gRPC client of**
| Service | RPCs used |
|---|---|
| catalog-read `CatalogRead` (customer's zone) | `BatchGetCatalogsByPlaceIds`, `GetCatalog`, `GetMenu`, `BatchGetItems` |
| merchant `Fulfilment` | `PriceOrder`, `SubmitOrder`, `CancelOrder` |
| dispatch `Fleet` (each zone) | `GetFleetSnapshot`, `GetMission` |
| auth `UserDirectory`, `GrantRegistry` | Customer email; revoked grants at start-up |

**NATS**
| Direction | Subjects | Payload |
|---|---|---|
| Publishes | `dispatch.<zone>.request`, `dispatch.<zone>.recall` | `DispatchRequest`, `RecallMission` |
| Consumes | `merchant.fulfilment.<order_id>.*` | `FulfilmentEvent` |
| Consumes | `mission.<order_id>.*` (stream `MISSIONS`), `TELEMETRY` | `MissionEvent`, `Telemetry` |
| Consumes | `auth.events.{grant_revoked,user_deleted}` | `GrantRevoked`, `UserDeleted` |

**External:** Amazon Location Places V2 (`us-west-2`), Stripe.

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `HTTP_ADDR` | `0.0.0.0:8085` | |
| `DATABASE_URL` | required | e.g. `postgres://user_service:user_service@localhost:5432/user_service` |
| `NATS_URL` | `nats://localhost:4222` | |
| `API_PUBLIC_URL` | `http://localhost:8086/api/user` | Customer API base URL and token audience |
| `MCP_PUBLIC_URL` | `http://localhost:8085` | The MCP resource is `{MCP_PUBLIC_URL}/mcp` |
| `AUTH_ISSUER` | `http://localhost:8081` | |
| `AUTH_JWKS_URL` | discovered from the issuer | Set when the issuer URL isn't reachable from the service |
| `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1` | `Host` headers accepted on `/mcp`; add the tunnel hostname |
| `CORS_ALLOWED_ORIGINS` | unset | Browser origins allowed without ui-gateway, e.g. `http://localhost:8087` |

Planned: gRPC addresses of merchant, catalog-read and dispatch per zone, AWS credentials, Stripe keys.

## Run
```bash
docker compose up user-service

# Or from source, against the Compose Postgres and NATS:
cp services/user-service/.env.example services/user-service/.env
cargo run -p user-service

# Browse the tools:
pnpm dlx @modelcontextprotocol/inspector   # connect to http://localhost:8085/mcp (Streamable HTTP)
```

For Alexa+, expose the service on a stable tunnel hostname, set `MCP_PUBLIC_URL`, `MCP_ALLOWED_HOSTS` and auth-service's `MCP_RESOURCE_URL` to match, then run `alexa-ai configure-account-linking` and `alexa-ai deploy`.
