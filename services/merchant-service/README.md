# merchant-service

The business side of Drone Drop: registration, the merchant portal, menus and stock, pickup markers, and the board where businesses accept or reject orders.

**Status:** scaffold. The service boots, connects to Postgres and NATS, and serves `/healthz`. Features are milestone 3.

## Responsibilities
- **Registration:**
  1. A merchant signs up in auth-service. MFA enrollment is mandatory for merchant accounts.
  2. They create a business and **claim a real place**: Places `SearchText`, then `GetPlace` with `IntendedUse=Storage`, because the coordinates are stored.
  3. They complete Stripe onboarding, through commerce-service.
  4. They set up the menu and the pickup point.
  5. The business moves from `PENDING` to `ACTIVE` once Stripe is enabled, the menu is published, the pickup point is verified, and a platform admin approves it (`merchant-service admin approve <id>`).
- **Catalog and stock:**
  - Menu sections and items: name, description, price in cents, weight in grams, stock quantity or unlimited, and an available flag.
  - CSV import.
  - Every item needs a weight, so the drone's 2.5 kg payload limit can be enforced.
  - A quote reserves stock for 10 minutes. PAID commits the reservation; a void, expiry or refund before pickup releases it.
- **Pickup point** (what lets the drone land precisely):
  1. The business drops a pin on the map at the exact pickup pad.
  2. The service generates a printable QR marker with an HMAC-signed payload: `ddp:v1:<pickup_id>:<hmac>`.
  3. The business places the marker on the pad, photographs it, and uploads the photo to S3 with a presigned PUT.
  4. The service decodes the QR from the photo, checks the HMAC and pickup id, stores the image hash, and marks the point `VERIFIED`.
  5. It publishes `merchant.pickup_point.verified` and copies the photo into the `PICKUP_ASSETS` object store, which is mirrored to the edge zones.
- **Order board and notifications:**
  - Live orders over SSE, with **Accept** / **Reject** buttons, a 5-minute countdown, and a **Loaded onto drone** button.
  - Email (SES; Mailpit locally) and an optional signed webhook to the business's POS.
  - Every notification is recorded.

## Owns (Postgres database `merchant`)
Schema in [`migrations/`](migrations/), applied at start-up. Conventions and cross-service references: [DATABASE.md](../../docs/DATABASE.md).
- `businesses` (claimed Amazon Location place, status, `payments_enabled`) and `business_members`
- `menu_sections`, `menu_items` (price, weight, stock)
- `stock_reservations` and `stock_reservation_items` (holds keyed by commerce `quote_id`)
- `pickup_points` (verified from a photo of the signed QR marker)
- `merchant_orders` (orders-board projection of `order.*` events)
- `notifications`, `business_webhooks`
- `outbox`, `inbox`

## Interfaces
**HTTP**
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness | Done |
| Portal pages | Server-rendered pages plus MapLibre. Logs in as an OAuth client of auth-service (resource `{MERCHANT}`, scopes `openid email merchant`, MFA always). | Planned |

**gRPC server** (internal port 9083, [`merchant.proto`](../../proto/dronedrop/merchant/v1/merchant.proto))
| Service | RPCs | Called by |
|---|---|---|
| `ShopCatalog` | `SearchShopsNear`, `BatchGetShopsByPlaceIds`, `GetShop`, `GetMenu`, `BatchGetMenuItems` | dispatch, commerce, map |
| `Stock` | `ReserveStock`, `ReleaseStock`, `CommitStock` | commerce |
| `PickupPoints` | `GetPickupPoint` | commerce, dispatch |

**gRPC client of**
| Service | RPCs used |
|---|---|
| commerce `MerchantOrders` | `AcceptOrder` and `RejectOrder` (portal buttons), `CreateOnboardingLink`, `GetMerchantPaymentStatus` |
| dispatch `Fleet` | `ConfirmLoaded` (the "Loaded onto drone" button) |
| auth `UserDirectory`, `GrantRegistry` | Member emails; revoked grants at start-up |

**NATS** (payloads from [`merchant_events.proto`](../../proto/dronedrop/events/v1/merchant_events.proto) and [`commerce_events.proto`](../../proto/dronedrop/events/v1/commerce_events.proto))
| Direction | Subjects | Payload |
|---|---|---|
| Consumes | `order.<id>.*` (stream `ORDERS`), to fill the orders board | `OrderEvent` |
| Consumes | `commerce.merchant_account.<business_id>.updated` | `MerchantAccountUpdated` |
| Publishes | `merchant.business.<id>.status_changed` | `BusinessStatusChanged` |
| Publishes | `merchant.pickup_point.verified` | `PickupPointVerified` |
| Writes | `PICKUP_ASSETS` object store, mirrored to edge nodes | Photo bytes |
| Consumes | `auth.events.{grant_revoked,user_deleted}` | `GrantRevoked`, `UserDeleted` |

**External:** Amazon Location Places V2, S3 (MinIO locally), SES (Mailpit locally).

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `HTTP_ADDR` | `0.0.0.0:8083` | |
| `DATABASE_URL` | required | e.g. `postgres://merchant:merchant@localhost:5432/merchant` |
| `NATS_URL` | `nats://localhost:4222` | |

Planned: `GRPC_ADDR` (`0.0.0.0:9083`), `PUBLIC_URL`, `AUTH_ISSUER`, OAuth client credentials, AWS credentials and `AWS_REGION`, S3 endpoint and bucket, SMTP URL, the marker HMAC key, and the Amazon Location Maps API key.

## Run
```bash
docker compose up merchant-service
DATABASE_URL=postgres://merchant:merchant@localhost:5432/merchant cargo run -p merchant-service
```
