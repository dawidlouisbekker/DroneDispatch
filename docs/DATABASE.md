# Drone Drop — Databases

This document covers:
- how Drone Drop services store their data
- how a service refers to data owned by another service
- the rules every migration follows

The system overview is in [ARCHITECTURE.md](ARCHITECTURE.md).

## Ownership
Every service that needs Postgres owns **one database and one login role** on a shared Postgres 17 server. [config/postgres/init.sql](../config/postgres/init.sql) runs `REVOKE CONNECT … FROM PUBLIC` on each database, so a role can connect only to its own database. No service can read or join another service's tables.

| Service | Database | Owns |
|---|---|---|
| auth-service | `auth` | Users, sessions, OAuth clients, grants, authorization codes, refresh tokens, MFA credentials |
| merchant-service | `merchant` | Businesses, members, menus, stock and holds, pickup points, the orders-board projection, notifications |
| commerce-service | `commerce` | Customers, delivery locations, merchant Stripe accounts, quotes, orders, payments, refunds, Stripe events |
| dispatch-service | `dispatch` | Missions, assignment attempts, mission event history |
| map-service | `map` | Web sessions, OAuth login state |
| edge-node | none | Its zone's JetStream KV and object store, so a partitioned zone never needs the cloud database |

Postgres listens on port **5432**:

| Connecting from | URL |
|---|---|
| Inside Compose | `postgres://<svc>:<svc>@postgres:5432/<svc>` |
| The host | `postgres://<svc>:<svc>@localhost:5432/<svc>` |
| Schema tests (superuser, creates throwaway databases) | `postgres://postgres:postgres@localhost:5432/postgres` |

These credentials are for local development only.

## Migrations
- **Location:** `services/<svc>/migrations/NNNN_description.sql`, numbered from `0001`.
- **Forward-only:** once a migration has been applied anywhere, never edit it; add a new one.
- **Applied at start-up** with `sqlx::migrate!().run(&db)`, right after `svc_common::connect_db`.
- **Tested** in `services/<svc>/tests/schema.rs` with `#[sqlx::test(migrations = "./migrations")]`, which gives each test a fresh database with the migrations applied.
  - Tests use runtime `sqlx::query`, never `query!`, so compiling never needs a database.
  - Tests are `#[ignore]`d, so `cargo test --workspace` works without Postgres. Run them with:
  ```bash
  docker compose up -d --wait postgres
  DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test --workspace -- --ignored
  ```

## Conventions
| Topic | Rule |
|---|---|
| Primary keys | `uuid`, generated in Rust as UUIDv7 (`Uuid::now_v7()`), because Postgres 17 has no `uuidv7()`. Natural keys only where another system owns the id (Stripe event ids, OAuth `client_id`) or the key is a hash. |
| Time | `timestamptz`. Every table has `created_at timestamptz NOT NULL DEFAULT now()`. Rows that change also get `updated_at`, which the application sets in its `UPDATE` (no triggers). |
| Money | `bigint` minor units named `*_cents`, with `CHECK (… >= 0)`, next to `currency text NOT NULL DEFAULT 'usd'`. |
| Statuses | `text` with `CHECK (status IN (…))`. Values are the proto enum names without the prefix: `ORDER_STATE_AWAITING_MERCHANT` is stored as `'AWAITING_MERCHANT'`. No Postgres `ENUM` types, because adding a value can't happen inside a transaction. |
| Coordinates | `lat double precision CHECK (lat BETWEEN -90 AND 90)` and `lon double precision CHECK (lon BETWEEN -180 AND 180)`. No PostGIS: radius searches filter on an indexed bounding box, then check distance. |
| Secrets | Never stored raw. Passwords are argon2id hashes. Other one-way secrets (session ids, codes, tokens) are SHA-256 base64url hashes in `*_hash text`. Secrets that must be read back (TOTP seeds, webhook signing keys) are AES-GCM encrypted: `*_ciphertext bytea` plus `*_nonce bytea`. |
| Names | `snake_case`, plural table names, and `<singular>_id` for foreign key columns. |
| Relationships inside a service | Real foreign keys with an explicit `ON DELETE`: `CASCADE` for rows owned by their parent, `RESTRICT` otherwise. Index every foreign key column. |
| Concurrency | Rows that sagas or concurrent consumers update (`orders`, `missions`) have `version integer NOT NULL DEFAULT 0`. Updates filter on `version = $n` and increment it. |
| Text | `text`, with `CHECK (length(col) > 0)` where an empty value makes no sense. No `varchar(n)`. |

## References across services
Foreign keys cannot cross databases. Tools that fake them (`postgres_fdw`, distributed SQL) would couple the services back into one database. References across services follow these rules instead:

1. **Reference by id.** Store the other service's id in a column without a foreign key, index it, and document where it points:
   ```sql
   COMMENT ON COLUMN quotes.business_id IS 'ref: merchant.businesses.id';
   ```
   The user key everywhere is `sub` (`uuid`). It is `auth.users.id` and the JWT subject.
2. **Validate when writing.** Before storing a reference, check it with the owning service over gRPC. For example, commerce calls `ShopCatalog.BatchGetMenuItems` before creating a quote.
3. **Copy what must not change.** Orders keep each item's name, price and weight, and the drop-off address, as they were at order time. A later change in the owning service must not rewrite history.
4. **Follow events.** Owners publish changes, such as `auth.events.user_deleted` or `merchant.business.<id>.status_changed`. Services holding references update or clean up in response. This is eventually consistent.
5. **Use sagas, not distributed transactions.** A change that spans services is a series of local transactions with compensating steps (void, release stock, refund), as in the order saga.

### Reference catalogue
| Column | Points to | Checked when written | Kept consistent by |
|---|---|---|---|
| `user_sub`, `member_sub`, `*_by_sub` columns, and `commerce.customers.sub` (other commerce tables reference `customers.sub` with real foreign keys) | `auth.users.id` | The user's validated JWT | `auth.events.user_deleted`: delete or anonymize |
| `map.web_sessions.grant_id` | `auth.grants.id` | The `gid` claim at login | `auth.events.grant_revoked`: delete the sessions |
| `merchant.stock_reservations.id` (= `quote_id`) | `commerce.quotes.id` | `Stock.ReserveStock` from commerce | Hold expiry, plus `ReleaseStock`/`CommitStock` from the saga |
| `merchant.stock_reservations.order_id` | `commerce.orders.id` | `Stock.CommitStock` | Unique: one hold per order |
| `merchant.notifications.order_id` | `commerce.orders.id` | `order.<id>.*` event | None needed: it is a log |
| `merchant.merchant_orders.order_id` | `commerce.orders.id` | `order.<id>.authorized` event | `order.<id>.*` events |
| `commerce.quotes.business_id`, `commerce.orders.business_id` | `merchant.businesses.id` | `ShopCatalog.GetShop` | `merchant.business.<id>.status_changed` blocks new quotes |
| `commerce.quote_items.item_id`, `commerce.order_items.item_id` | `merchant.menu_items.id` | `ShopCatalog.BatchGetMenuItems` | Name, price and weight copied, so none needed |
| `commerce.orders.pickup_point_id` | `merchant.pickup_points.id` | `PickupPoints.GetPickupPoint` | Position copied, so none needed |
| `commerce.merchant_accounts.business_id` | `merchant.businesses.id` | `MerchantOrders.CreateOnboardingLink` from merchant | `merchant.business.<id>.status_changed` |
| `dispatch.missions.order_id` | `commerce.orders.id` | `dispatch.request` | `mission.<id>.*` events back to commerce |
| `dispatch.missions.pickup_point_id` | `merchant.pickup_points.id` | Copied from `dispatch.request` | Position copied, so none needed |
| `dispatch.missions.dispatch_request_id` | commerce `outbox.id` (the `Nats-Msg-Id`) | `dispatch.request` | Unique: a redelivered request can't create a second mission |
| `map.web_sessions.user_sub` | `auth.users.id` | OAuth login | `auth.events.user_deleted`: delete sessions |
| `map.oauth_login_states.location_id` | `commerce.delivery_locations.id` | `DeliveryLocations.VerifyDeliveryLocation` checks the owner | Rows expire after 10 minutes |

## Outbox and inbox
A service must never commit a change and then fail to publish its event, or publish an event for a change that rolled back. So every service that publishes events has an **outbox**, and every service that consumes them has an **inbox**. The tables are identical everywhere:

```sql
CREATE TABLE outbox (
    id           uuid PRIMARY KEY,          -- also the Nats-Msg-Id, so JetStream drops duplicates
    subject      text NOT NULL,
    payload      bytea NOT NULL,            -- encoded protobuf message
    created_at   timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz
);
CREATE INDEX outbox_unpublished ON outbox (created_at) WHERE published_at IS NULL;

CREATE TABLE inbox (
    consumer    text NOT NULL,              -- durable consumer name
    message_id  text NOT NULL,              -- Nats-Msg-Id of the received message
    received_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (consumer, message_id)
);
```

**Publishing:** in the same transaction as the state change, insert the event into `outbox`. A relay then publishes waiting rows:
1. `SELECT … WHERE published_at IS NULL ORDER BY created_at LIMIT 100 FOR UPDATE SKIP LOCKED`.
2. Publish each row to JetStream with `Nats-Msg-Id` set to the row's `id`.
3. Set `published_at`.

**Consuming:** in one transaction, `INSERT INTO inbox … ON CONFLICT DO NOTHING`.
- If no row was inserted, the message is a duplicate: acknowledge it and stop.
- Otherwise apply the change, commit, and then acknowledge.

| Service | Outbox | Inbox |
|---|---|---|
| auth | Yes | No |
| merchant | Yes | Yes |
| commerce | Yes | Yes |
| dispatch | Yes | Yes |
| map | No (it streams events straight to browsers and keeps no state from them) | No |

## Schemas by service
The migrations in `services/<svc>/migrations/` are the source of truth. This is a summary.

### auth
Ported from the earlier SQLite draft. `access_tokens` is gone, because access tokens are JWTs and are never stored.
- **`users`:** `id` (the `sub`), `email` unique on `lower(email)`, `password_hash`, `email_verified_at`, `disabled_at`.
- **`sessions`:** login sessions, with a hashed CSRF token and the `auth_time` and `amr` (`text[]`) the next tokens will carry.
- **`clients`:** `client_id`, `secret_hash`, `redirect_uris text[]`, `auth_method`, `kind` (`STATIC` or `DCR`).
- **`auth_requests`:** validated `/authorize` parameters (`jsonb`) parked during login and consent.
- **`grants`:** one row per authorization. Its `id` is the `gid` claim; it also has `user_id`, `client_id`, `resource`, `scope` and `revoked_at`.
- **`auth_codes`:** belong to a grant; also store PKCE challenge, redirect URI, `acr`, `amr`, `auth_time` and `consumed_at`.
- **`refresh_tokens`:** belong to a grant, with `expires_at`, `rotated_at` and `revoked_at`.
- **MFA:**
  - `mfa_totp`: encrypted secret, `confirmed_at`, and `last_used_step` to block code replay.
  - `passkeys`: unique `credential_id`; the passkey stored as `jsonb`.
  - `webauthn_challenges`: ceremony state.
  - `recovery_codes`: hashed codes with `used_at`.
- **`outbox`**.

### merchant
- **`businesses`:**
  - `status` `PENDING|ACTIVE|SUSPENDED`
  - `place_id` (unique), `name`, `address`, `lat`/`lon`, `categories text[]`
  - `prep_time_minutes`, `accepting_orders`, `payments_enabled` (kept up to date from commerce events), `approved_at`
- **`business_members`:** `business_id` and `user_sub`, with role `OWNER|STAFF`.
- **Menu:**
  - `menu_sections`.
  - `menu_items`: `price_cents`, `currency`, `weight_g > 0`, `stock_qty` (NULL means unlimited), `available`.
  - Composite foreign keys keep an item's section, and every held item, on the same business.
  - An item that has ever been held can't be deleted (`RESTRICT`); set `available = false` instead.
- **Stock holds:** `stock_reservations` (id = `quote_id`; status `HELD|COMMITTED|RELEASED`; `expires_at`; `order_id` once committed) and `stock_reservation_items`.
- **`pickup_points`:** position, status `PENDING_PHOTO|VERIFIED|REJECTED`, `photo_object_key`, `photo_sha256`, `verified_at`.
- **`merchant_orders`:** the orders-board projection, built from `order.*` events.
  - Order snapshot (items as `jsonb`, total), `accept_by`.
  - Status `AWAITING_DECISION|ACCEPTED|REJECTED|EXPIRED`, with `decided_by_sub`, `decided_at`, `loaded_at` (only once ACCEPTED) and `version`.
- **`notifications`:** email, webhook and portal notices, with delivery status.
- **`business_webhooks`:** POS webhook URL with an encrypted signing secret.
- **`outbox`**, **`inbox`**.

### commerce
- **`customers`:** keyed by `sub`. Stripe customer id, default payment method id, `per_order_cap_cents` (default $50) and `daily_cap_cents` (default $100), with the per-order cap at most the daily cap.
- **`delivery_locations`:**
  - Status `PENDING|VERIFIED|REVOKED`.
  - VERIFIED needs `verified_at` and a `verified_amr` containing `otp` or `hwk`.
  - Labels are unique per customer among non-revoked locations, ignoring case.
  - Locations are `RESTRICT`, not cascaded, because orders keep pointing at them. Revoke a location rather than deleting it.
- **`merchant_accounts`:** `business_id`, `stripe_account_id`, `charges_enabled`, `payouts_enabled`.
- **Quotes:**
  - `quotes`: totals, `payload_g`, `eta_seconds`, `expires_at`, `superseded_at`.
  - `quote_items`: name, price and weight copied at quote time.
- **Orders:**
  - `orders`: unique `quote_id`; `state` with all 15 `OrderState` values; drop-off and pickup copied at order time; subtotal, fee, platform fee and total; `merchant_accept_by`; `version`.
  - Composite foreign keys ensure a quote or order uses only the customer's own quote and delivery location.
  - `order_items`: copied item details.
  - `order_state_transitions`: the status timeline, with a `cause` (`GRPC|STRIPE_WEBHOOK|TIMER|EVENT`). A transition to `PAID` must have cause `STRIPE_WEBHOOK` and reference the stored `stripe_events` row. The application writes the transition in the same transaction as the state change.
- **Payments:**
  - `payments`: one per order, unique `stripe_payment_intent_id`, idempotency key (the `quote_id`). Status `AUTHORIZING|AUTHORIZED|CAPTURED|VOIDED|FAILED|PARTIALLY_REFUNDED|REFUNDED`; the PaymentIntent id may be NULL only while authorizing or after a failed create.
  - `refunds`.
  - `stripe_events`: keyed by Stripe event id, with `processed_at`, so duplicate webhooks are ignored.
- **`outbox`**, **`inbox`**.

### dispatch
- **`missions`:** one per order, keyed by `order_id`, with a unique `dispatch_request_id` so a redelivered `dispatch.request` is ignored.
  - `state` with the `MissionState` values; `zone`, `drone_id`.
  - Pickup position, `pickup_point_id` and `asset_key`; drop-off position; `payload_g`.
  - `attempts`, `version`.
- **`mission_attempts`:** each assignment try: zone, result, time.
- **`mission_events`:** history of received `MissionEvent`s.
- **`outbox`**, **`inbox`**.

### map
- **`web_sessions`:** `id_hash`, `user_sub`, `grant_id`, `csrf_token_hash`, `amr text[]`, `auth_time`, encrypted refresh token, `expires_at`, `last_seen_at`.
- **`oauth_login_states`:**
  - `state_hash`, encrypted PKCE verifier, `nonce_hash`.
  - Purpose `LOGIN|STEP_UP`. A step-up must name the `location_id` being verified and the `web_session_id_hash` that started it; deleting that session cascades.
  - `return_to` must be a relative path (no `//`, backslashes or control characters), which blocks open redirects.
  - `expires_at`.
