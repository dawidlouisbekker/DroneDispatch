# merchant-service

The business side of Drone Drop and the **write side** of the catalog (CQRS): onboarding, catalog and stock, order fulfilment, pickup stations and payouts. Customers never read the catalog from here: [catalog-read-service](../catalog-read-service/README.md) serves it at the edge from the events this service publishes.

**Status:** scaffold. The service boots and serves `/healthz`; the portal API checks tokens, and listing businesses and editing the menu already work. Fulfilment, stations, state events and payouts are milestone 3.

## Responsibilities
- **Onboarding:**
  1. A merchant signs up in auth-service (a passkey is required for merchant logins).
  2. They find their business with Amazon Location (SearchNearby or SearchText) and claim it: `GetPlace` with `IntendedUse=Storage`, which Amazon requires before a place ID may be stored. `businesses.place_id` links the place to the business.
  3. They set the **spoken name** Alexa+ reads out (defaults to the place title), onboard with Stripe Connect, build the menu, and register the pickup station.
  4. The business becomes `ACTIVE` once payouts are enabled, the menu is published, a station is active, and a platform admin approves it.
- **Catalog and stock:** sections and items: name, spoken name, description, price in cents, weight in grams, stock quantity or unlimited, and an available flag. Every item needs a weight, so the drone's 2.5 kg payload limit can be enforced.
- **CQRS write side:** every catalog change commits with an outbox row holding the aggregate's full state (`merchant.state.catalog.<place_id>`, `merchant.state.section.<id>`, `merchant.state.item.<id>`). See [DATABASE.md](../../docs/DATABASE.md#cqrs-catalog-projections).
- **Fulfilment** (the `Fulfilment` gRPC service, called by user-service, which owns the order):
  - `PriceOrder`: authoritative prices, weights and stock.
  - `SubmitOrder`: reserves stock and puts the order on the board with a 5-minute accept window; returns the business station.
  - The business accepts or rejects in the portal, or the window expires. Each publishes `merchant.fulfilment.<order_id>.<status>`, so user-service captures or voids the payment.
  - `LOADED` when the station acknowledges the drone handoff. `CancelOrder` releases stock before that.
- **Pickup station:** the business registers a station at its pickup pad: position and accuracy, public key (drones pin its SHA-256), and access networks (Bluetooth LE first). The station receives the zone fleet CA certificate so it can verify drones.
- **Payouts:** Stripe Connect accounts, onboarding links and account events.
- **Notifications:** email (SES; Mailpit locally) and an optional signed webhook to the business's POS. Every notification is recorded.

## Owns (Postgres database `merchant`)
Schema in [`migrations/`](migrations/), applied at start-up. Conventions and cross-service references: [DATABASE.md](../../docs/DATABASE.md).
- `businesses` (place ID, spoken name, status) and `business_members`
- `menu_sections`, `menu_items`
- `fulfilment_orders`, `stock_reservations`, `stock_reservation_items`
- `stations`, `station_access_networks`
- `merchant_accounts` (Stripe Connect)
- `notifications`, `business_webhooks`
- `outbox`, `inbox`

## Interfaces
**HTTP**
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness | Done |
| `GET /v1/businesses`, `/v1/businesses/{id}/menu…` | Portal API ([`merchant.yaml`](../../api/openapi/merchant.yaml)). Resource `{MERCHANT}`, scope `merchant`, passkey always. | Partly done |
| `/v1/businesses/{id}/orders…` | Orders board: accept, reject | Scaffold |
| `/v1/businesses/{id}/station` | Register the pickup station | Scaffold |
| `GET /v1/orders/live` | Orders board WebSocket | Scaffold |
| `POST /webhooks/stripe/thin` | Stripe Connect account events | Planned |

**gRPC server** (internal port 9083, [`merchant.proto`](../../proto/dronedrop/merchant/v1/merchant.proto))
| Service | RPCs | Called by |
|---|---|---|
| `Fulfilment` | `PriceOrder`, `SubmitOrder`, `CancelOrder` | user |

**gRPC client of:** auth `UserDirectory` (member emails) and `GrantRegistry` (revoked grants at start-up).

**NATS** (payloads from [`merchant_events.proto`](../../proto/dronedrop/events/v1/merchant_events.proto))
| Direction | Subjects | Payload |
|---|---|---|
| Publishes | `merchant.state.{catalog,section,item}.<id>` (stream `MERCHANT_EVENTS`, latest per subject) | `CatalogState`, `CatalogSectionState`, `CatalogItemState` |
| Publishes | `merchant.fulfilment.<order_id>.<status>` | `FulfilmentEvent` |
| Consumes | `mission.<order_id>.*` (stream `MISSIONS`) | `MissionEvent`, for the board |
| Consumes | `auth.events.{grant_revoked,user_deleted}` | `GrantRevoked`, `UserDeleted` |

**External:** Amazon Location Places V2, Stripe Connect, SES (Mailpit locally).

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `HTTP_ADDR` | `0.0.0.0:8083` | |
| `DATABASE_URL` | required | e.g. `postgres://merchant:merchant@localhost:5432/merchant` |
| `NATS_URL` | `nats://localhost:4222` | |
| `API_PUBLIC_URL` | `http://localhost:8086/api/merchant` | API base URL and token audience |
| `AUTH_ISSUER` | `http://localhost:8081` | |
| `AUTH_JWKS_URL` | discovered from the issuer | |
| `CORS_ALLOWED_ORIGINS` | unset | Browser origins allowed without ui-gateway |

Planned: `GRPC_ADDR` (`0.0.0.0:9083`), AWS credentials, Stripe keys, SMTP settings, the fleet CA certificate per zone.

## Run
```bash
docker compose up merchant-service

# Or from source, against the Compose Postgres and NATS:
cargo run -p merchant-service
```
