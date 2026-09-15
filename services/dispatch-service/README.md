# dispatch-service

The **MCP server** Alexa+ talks to, and the fleet orchestrator that turns paid orders into drone missions on the MEC edge zones.

**Status:** scaffold.
- `/mcp` serves the full tool list over Streamable HTTP, but every tool returns `not_implemented`.
- The OAuth protected-resource metadata is served, but JWTs are **not validated yet**, so `/mcp` is open.
- MCP tools are milestone 5; fleet orchestration is milestone 6.

## Responsibilities
- **MCP server** (spec 2025-11-25, Streamable HTTP) at `/mcp`, built on [`rmcp`](https://crates.io/crates/rmcp).
- **OAuth resource server:**
  - A request without a token gets `401` and a `WWW-Authenticate` header pointing at the protected-resource metadata, which names auth-service.
  - Every request's JWT is validated against auth-service's JWKS: `iss`, `aud` = `{PUBLIC_URL}/mcp`, `exp`, and the `delivery` scope.
  - Grants revoked on `auth.events.grant_revoked` are rejected immediately.
  - Tools read the customer's identity only from the validated JWT.
- **Shop discovery:** calls Amazon Location Places V2 (`SearchNearby`, `SearchText`, `GetPlace`, `Geocode`) and joins the results with registered merchants from merchant-service.
- **Fleet orchestration:**
  - Consumes `dispatch.request` and picks the edge zone that owns the pickup point.
  - Publishes `cmd.edge.<zone>.assign`, retrying in the neighbouring zone if no drone is free.
  - Reports `no_drone` after 10 minutes.
  - Relays `mission.*` events.

## Owns (Postgres database `dispatch`)
- `missions`: one per paid order, keyed by `order_id`, with state, zone, drone, pickup and drop-off.
- `mission_attempts`: each try to assign a drone, per zone.
- `mission_events`: history of received `MissionEvent`s, keyed by `event_id`.
- `outbox`, `inbox`.

Migrations in [`migrations/`](migrations/) run at start-up. Conventions and cross-service references: [DATABASE.md](../../docs/DATABASE.md).

## MCP tools
Alexa runs the conversation with these tools:
1. Present options.
2. Adjust the cart until the customer is happy.
3. Read back the shop, items, total, drop-off location and ETA.
4. Call `place_order` only after the customer explicitly says yes.

The server `instructions` and each tool description spell out this protocol.

| Tool | Purpose |
|---|---|
| `search_nearby_shops(category?, near_location_id?, radius_m?)` | Places `SearchNearby` around a verified delivery location, joined with registered merchants. Unregistered shops come back as "not on Drone Drop". |
| `search_places(query, near_location_id?)` | Places `SearchText` for a specific shop or address; flags registered merchants. |
| `get_shop_details(shop_id)` | Places `GetPlace` (address, hours, contacts) merged with the merchant profile, plus distance and ETA. |
| `get_menu(shop_id, query?)` | Items with price, availability and weight. |
| `list_past_order_locations(limit?)` | Shops ordered from and drop-off locations used. |
| `list_order_history(limit?, shop_id?)` | Past orders. |
| `reorder(order_id)` | Starts a new cart from a past order. |
| `list_delivery_locations()` | Verified locations, plus any pending verification. |
| `request_new_delivery_location(address, label)` | Geocodes the address and creates a **pending** location that the customer must verify with MFA in map-service. |
| `update_cart(shop_id, items, delivery_location_id)` | Replaces the cart and returns a quote: totals, weight check (2.5 kg limit), ETA, and a `quote_id` valid for 10 minutes with stock reserved. |
| `place_order(quote_id, expected_total_cents, customer_confirmed)` | Authorizes payment by voice. See the checks below. |
| `get_order_status(order_id?)` | Live status. |
| `cancel_order(order_id)` | Cancels before pickup: voids or refunds the payment and recalls the drone. |

`place_order` succeeds only if all of these hold (commerce-service enforces them):
- The quote belongs to the caller's `sub`, is still valid, and `expected_total_cents` matches.
- The delivery location is verified.
- The per-order and daily spending caps allow it.
- A default payment method is on file.

The Stripe idempotency key is the `quote_id`, so a retried call cannot charge twice.

## Interfaces
**HTTP**
| Endpoint | Purpose | Status |
|---|---|---|
| `POST`/`GET`/`DELETE /mcp` | MCP Streamable HTTP | Scaffold (stub tools, no auth) |
| `GET /.well-known/oauth-protected-resource/mcp` | Protected-resource metadata (RFC 9728) | Scaffold |
| `GET /healthz` | Liveness | Done |

**gRPC server** (internal port 9082, [`dispatch.proto`](../../proto/dronedrop/dispatch/v1/dispatch.proto))
| Service | RPCs | Called by |
|---|---|---|
| `Fleet` | `GetMission`, `RecallMission`, `ConfirmLoaded`, `GetFleetSnapshot` | commerce, merchant, map |

**gRPC client of**
| Service | RPCs used |
|---|---|
| merchant `ShopCatalog` | `SearchShopsNear`, `BatchGetShopsByPlaceIds`, `GetShop`, `GetMenu` |
| commerce `Checkout`, `Orders`, `DeliveryLocations`, `CustomerPayments` | Cart, order, history and delivery-location calls behind the MCP tools |
| auth `GrantRegistry` | `ListRevokedGrants` at start-up |

**NATS** (payloads from [`fleet_events.proto`](../../proto/dronedrop/events/v1/fleet_events.proto) and [`edge.proto`](../../proto/dronedrop/edge/v1/edge.proto))
| Direction | Subjects | Payload |
|---|---|---|
| Consumes | `dispatch.request` (work queue `DISPATCH_REQUESTS`) | `DispatchRequest` |
| Publishes | `cmd.edge.<zone>.{assign,recall,handoff,loaded}` | `EdgeCommand` |
| Consumes and relays | `mission.<order_id>.*` (stream `MISSIONS`); publishes `no_drone` itself | `MissionEvent` |
| Consumes | `TELEMETRY` stream (1 Hz), for `GetFleetSnapshot` | `Telemetry` |
| Consumes | `auth.events.grant_revoked` | `GrantRevoked` |

**External:** Amazon Location Places V2 (`us-west-2`).

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `HTTP_ADDR` | `0.0.0.0:8082` | |
| `DATABASE_URL` | required | e.g. `postgres://dispatch:dispatch@localhost:5432/dispatch` |
| `NATS_URL` | `nats://localhost:4222` | |
| `PUBLIC_URL` | `http://localhost:8082` | The MCP resource is `{PUBLIC_URL}/mcp`. |
| `AUTH_ISSUER` | `http://localhost:8081` | Listed in the protected-resource metadata. |
| `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1` | `Host` headers accepted on `/mcp` (DNS-rebinding protection). Add the tunnel hostname. |

Planned: `GRPC_ADDR` (`0.0.0.0:9082`), plus AWS credentials and `AWS_REGION` for Places.

## Run
```bash
docker compose up dispatch-service
DATABASE_URL=postgres://dispatch:dispatch@localhost:5432/dispatch cargo run -p dispatch-service

# Browse the tools:
npx @modelcontextprotocol/inspector   # connect to http://localhost:8082/mcp (Streamable HTTP)
```

For Alexa+, expose the service on the stable `dispatch.` tunnel hostname, set `PUBLIC_URL` and `MCP_ALLOWED_HOSTS` to match, then run `alexa-ai configure-account-linking` and `alexa-ai deploy`.
