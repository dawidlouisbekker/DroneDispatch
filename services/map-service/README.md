# map-service

The customer web app. Customers watch their drone on a live map, see their order history, verify delivery locations with MFA, and set up a payment method and spending caps.

**Status:** scaffold. The service boots, connects to Postgres and NATS, and serves `/healthz`. Features are milestone 7.

## Responsibilities
- **Login:** an OAuth client of auth-service (`resource={PUBLIC_URL}`, `scope=openid email`), with sessions stored in Postgres.
- **Live map:** MapLibre GL JS with the Amazon Location Maps V2 style. The API key allows Maps actions only and is restricted by referrer. The map shows:
  - drones, edge zones, hubs, no-fly zones and pickup points
  - the customer's active mission route and ETA
  - an edge health panel (cloud round-trip time compared with edge vision and control latency)
- **Order history:** past orders with shop, items, total, drop-off location, status timeline and refund status, plus reorder links.
- **Delivery locations:**
  - Lists verified and pending locations.
  - Adding or verifying a location requires a **step-up**: the token's `amr` must include `otp` or `hwk`, and `auth_time` must be at most 5 minutes old.
  - Otherwise the service redirects to auth-service with `acr_values=mfa&max_age=300`, then calls commerce-service's `DeliveryLocations.VerifyDeliveryLocation` with the token's `amr` and `auth_time`.
  - `/locations/verify?id=…` is the page Alexa sends customers to. The URL carries no personal data, and the page checks that the session's `sub` owns the location.
- **Payments and limits:** a payment method page using Stripe Checkout in setup mode, and per-order and daily caps for voice orders.

## Owns (Postgres database `map`)
- `web_sessions`: server-side session per browser login (hashed id and CSRF token, `amr`/`auth_time`, grant id, encrypted refresh token).
- `oauth_login_states`: OAuth state parked between redirect and callback, for login and MFA step-up (with the location being verified).

Migrations are in `migrations/`, applied at start-up. See [DATABASE.md](../../docs/DATABASE.md).

## Interfaces
**HTTP**
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness | Done |
| Live map, orders, locations, payment pages | Server-rendered pages plus MapLibre; live drone positions pushed to the browser | Planned |
| `GET /locations/verify?id=…` | MFA-protected location verification | Planned |

**gRPC client of** (map-service serves no gRPC; only browsers call it)
| Service | RPCs used |
|---|---|
| commerce `Orders` | `ListOrders`, `GetOrder`, `CancelOrder` |
| commerce `DeliveryLocations` | `ListDeliveryLocations`, `RequestDeliveryLocation`, `VerifyDeliveryLocation` (after the MFA step-up) |
| commerce `CustomerPayments` | `GetPaymentSettings`, `UpdateSpendingLimits`, `CreatePaymentSetupSession` |
| dispatch `Fleet` | `GetMission`, `GetFleetSnapshot` (initial map state) |
| merchant `ShopCatalog` | `GetShop` (shop names and positions) |
| auth `GrantRegistry` | `ListRevokedGrants` at start-up |

**NATS**
| Direction | Subjects | Payload |
|---|---|---|
| Reads | `ORDER_VIEW` KV bucket | Order summary |
| Consumes | `TELEMETRY` stream (1 Hz, synced from the edges) | `Telemetry` |
| Consumes | `mission.<id>.*`, `order.<id>.*` | `MissionEvent`, `OrderEvent` |
| Consumes | `metrics.edge.<zone>` for the health panel | `EdgeMetrics` |
| Consumes | `auth.events.{grant_revoked,user_deleted}` | `GrantRevoked`, `UserDeleted` |

**External:** Amazon Location Maps V2 (loaded by the browser), Stripe Checkout (via commerce-service).

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `HTTP_ADDR` | `0.0.0.0:8085` | |
| `DATABASE_URL` | required | e.g. `postgres://map:map@localhost:5432/map` |
| `NATS_URL` | `nats://localhost:4222` | |

Planned: `PUBLIC_URL`, `AUTH_ISSUER`, OAuth client credentials, `AMAZON_LOCATION_MAPS_API_KEY` and `AWS_REGION`.

## Run
```bash
docker compose up map-service
DATABASE_URL=postgres://map:map@localhost:5432/map cargo run -p map-service
```
