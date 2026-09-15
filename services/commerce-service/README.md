# commerce-service

Orders and money. It runs the order saga and talks to Stripe Connect. It is the only service that can decide an order is paid, and therefore the only one that can request a drone.

**Status:** scaffold. The service boots, connects to Postgres and NATS, and serves `/healthz`. Features are milestone 4.

## Responsibilities
- **Carts and quotes:** a quote is valid for 10 minutes and reserves stock in merchant-service.
- **Order saga:**
  ```
  CART → QUOTED ─place_order→ AUTHORIZING ─webhook amount_capturable_updated→ AWAITING_MERCHANT
  AWAITING_MERCHANT ─merchant accepted→ CAPTURING ─webhook payment_intent.succeeded→ PAID → DISPATCH_REQUESTED
    → DRONE_ASSIGNED → AT_PICKUP → PICKED_UP → DELIVERED → COMPLETED

  declined card                                        → PAYMENT_FAILED
  merchant rejects / 5-minute timeout (void)           → CANCELLED
  after PAID: no drone in 10 min / pickup failed /
    mission aborted / customer cancels before pickup   → refund → REFUNDED
  ```
- **`place_order` guardrails** (voice payments):
  - The quote belongs to the caller, is still valid, and `expected_total_cents` matches.
  - The delivery location is verified.
  - The per-order and daily spending caps allow it.
  - A default payment method is on file.
  - The `quote_id` is the Stripe idempotency key, so retries never double-charge.
- **Stripe Connect** (test mode):
  - **Merchants:** Accounts v2 connected accounts with hosted onboarding links. A business can take orders only once card payments and payouts are active.
  - **Customers:** save a card once through Stripe Checkout in setup mode. Card details never pass through Alexa or our services.
  - **Payments:** a PaymentIntent with `capture_method=manual`, `off_session`, a destination charge to the merchant, and `application_fee_amount` = delivery fee + platform commission. The service captures it after the merchant accepts, voids it on rejection or timeout, and refunds with `reverse_transfer` and `refund_application_fee`.
- **Webhooks:**
  - `POST /webhooks/stripe` verifies the `Stripe-Signature` header and stores event ids so duplicates are ignored.
  - **Only this path can move an order to PAID, and only PAID publishes `dispatch.request`.**
- **Customer data:**
  - Delivery locations are created `pending` and become `verified` only after an MFA step-up in map-service.
  - Spending caps, order history, and past order locations.

## Owns (Postgres database `commerce`)
Schema in [`migrations/`](migrations), applied at start-up. Conventions and cross-service references are in [DATABASE.md](../../docs/DATABASE.md).
- `customers`: spending caps and Stripe customer
- `delivery_locations`: `PENDING` → `VERIFIED` after MFA, or `REVOKED`
- `merchant_accounts`: Stripe connected accounts
- `quotes`, `quote_items`: cart snapshots, valid 10 minutes
- `orders`, `order_items`, `order_state_transitions`: the order saga and its timeline
- `payments`, `refunds`, `stripe_events`: manual-capture PaymentIntents, refunds, deduplicated webhooks
- `outbox`, `inbox`

## Interfaces
**HTTP** (the public `commerce.` hostname is only for Stripe webhooks)
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness | Done |
| `POST /webhooks/stripe` | Signed Stripe events | Planned |

**gRPC server** (internal port 9084, [`commerce.proto`](../../proto/dronedrop/commerce/v1/commerce.proto))
| Service | RPCs | Called by |
|---|---|---|
| `Checkout` | `UpsertCart`, `Reorder`, `PlaceOrder` | dispatch |
| `Orders` | `GetOrder`, `ListOrders`, `CancelOrder`, `ListPastOrderLocations` | dispatch, map |
| `DeliveryLocations` | `ListDeliveryLocations`, `RequestDeliveryLocation`, `VerifyDeliveryLocation` | dispatch, map (verify: map only) |
| `CustomerPayments` | `GetPaymentSettings`, `UpdateSpendingLimits`, `CreatePaymentSetupSession` | dispatch, map |
| `MerchantOrders` | `AcceptOrder`, `RejectOrder`, `CreateOnboardingLink`, `GetMerchantPaymentStatus` | merchant |

**gRPC client of**
| Service | RPCs used |
|---|---|
| merchant `ShopCatalog`, `Stock`, `PickupPoints` | `GetShop` and `BatchGetMenuItems` for quote prices; stock holds; the pickup point for `dispatch.request` |
| dispatch `Fleet` | `RecallMission` when an order is cancelled |
| auth `UserDirectory` | Customer email for Stripe |

**NATS** (payloads from [`commerce_events.proto`](../../proto/dronedrop/events/v1/commerce_events.proto) and [`fleet_events.proto`](../../proto/dronedrop/events/v1/fleet_events.proto))
| Direction | Subjects | Payload |
|---|---|---|
| Publishes | `order.<id>.{authorized,accepted,paid,dispatched,picked_up,delivered,payment_failed,cancelled,refunded}` (stream `ORDERS`) | `OrderEvent` |
| Publishes | `dispatch.request` (work queue `DISPATCH_REQUESTS`), **only when PAID** | `DispatchRequest` |
| Publishes | `commerce.merchant_account.<business_id>.updated` | `MerchantAccountUpdated` |
| Consumes | `mission.<id>.*` (stream `MISSIONS`) | `MissionEvent` |
| Consumes | `merchant.business.<id>.status_changed` | `BusinessStatusChanged` |
| Consumes | `auth.events.user_deleted` | `UserDeleted` |
| Writes | `ORDER_VIEW` KV bucket (key `<sub>.<order_id>`), read by map-service | Order summary |

**External:** Stripe API and webhooks.

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `HTTP_ADDR` | `0.0.0.0:8084` | |
| `DATABASE_URL` | required | e.g. `postgres://commerce:commerce@localhost:5432/commerce` |
| `NATS_URL` | `nats://localhost:4222` | |

Planned: `GRPC_ADDR` (`0.0.0.0:9084`), `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, the platform commission, and `MAP_PUBLIC_URL` for Checkout return URLs.

## Run
```bash
docker compose up commerce-service
DATABASE_URL=postgres://commerce:commerce@localhost:5432/commerce cargo run -p commerce-service

# Forward Stripe test webhooks. `stripe listen` prints the signing secret to use as STRIPE_WEBHOOK_SECRET.
stripe listen --forward-to localhost:8084/webhooks/stripe
# ...or run the Compose Stripe CLI: docker compose --profile stripe up stripe-cli
```

## Tests to write
Run these against a Stripe mock server:
- **Happy path:** place → authorize → merchant accepts → capture → PAID → exactly one `dispatch.request`.
- **Rejection or timeout:** the authorization is voided and nothing is dispatched.
- **Duplicate webhook:** no second transition.
- **Forged signature:** rejected.
- **Retried `place_order`:** one PaymentIntent.
- **Rejected orders:** wrong total, unverified location, or over the spending cap.
