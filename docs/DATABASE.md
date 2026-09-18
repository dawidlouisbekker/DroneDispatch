# Drone Drop — Databases

This document covers:
- how Drone Drop services store their data, in the cloud and at the edge
- how a service refers to data owned by another service
- how the catalog's write and read sides (CQRS) stay in step
- the rules every migration follows

The system overview is in [ARCHITECTURE.md](ARCHITECTURE.md).

## Ownership
Every service owns its own database and login role. No service can read or join another service's tables.

| Service | Runs | Database | Owns |
|---|---|---|---|
| auth-service | Cloud | `auth` | Users, passkeys, sessions, OAuth clients, grants, authorization codes, refresh tokens |
| user-service | Cloud | `user_service` | Customers and spend policy, pickup locations, customer stations, orders and order history, approvals, payments |
| merchant-service | Cloud | `merchant` | Businesses (with their Amazon Location place ID), members, catalog, stock and holds, fulfilment orders, business stations, Stripe Connect accounts, notifications |
| catalog-read-service | Edge, per zone | `catalog_read_<zone>` | Read-only catalog projections keyed by place ID |
| dispatch-service | Edge, per zone | `dispatch_<zone>` | Docks, drones, missions, station handshakes |

user-service's database and role are named `user_service` because `user` is a reserved word in Postgres.

**Edge databases.** In a real MEC deployment each zone runs its own Postgres next to its services, so a partitioned zone never needs the cloud database. Locally, one Postgres 17 server hosts every database; [config/postgres/init.sql](../config/postgres/init.sql) creates one database per zone (`catalog_read_sea_north`, `dispatch_sea_south`, …) owned by the service's role (`catalog_read`, `dispatch`), and runs `REVOKE CONNECT … FROM PUBLIC` on each.

Postgres listens on port **5432**:

| Connecting from | URL |
|---|---|
| Inside Compose | `postgres://<role>:<role>@postgres:5432/<database>` |
| The host | `postgres://<role>:<role>@localhost:5432/<database>` |
| Schema tests (superuser, creates throwaway databases) | `postgres://postgres:postgres@localhost:5432/postgres` |

These credentials are for local development only. `init.sql` only runs on an empty data volume; it is idempotent, so on an existing volume apply it with `docker compose exec -T postgres psql -U postgres < config/postgres/init.sql`.

## Migrations
- **Location:** `services/<svc>/migrations/NNNN_description.sql`, numbered from `0001`. A per-zone service has one migration set, applied to every zone's database.
- **Forward-only:** once a migration has been applied anywhere, never edit it; add a new one. (dispatch-service's migrations restarted at `0001` in v2 because they target new per-zone databases; the old cloud `dispatch` database is retired.)
- **Applied at start-up** with `sqlx::migrate!().run(&db)`, right after `svc_common::connect_db`.
- **Tested** in `services/<svc>/tests/schema.rs` with `#[sqlx::test(migrations = "./migrations")]`, which gives each test a fresh database with the migrations applied.
  - Tests use runtime `sqlx::query`, never `query!`, so compiling never needs a database.
  - Tests are `#[ignore]`d, so `cargo test --workspace` works without Postgres:
  ```bash
  scripts/test-db.sh                       # every service
  scripts/test-db.sh -p merchant-service   # one service
  ```

## Conventions
| Topic | Rule |
|---|---|
| Primary keys | `uuid`, generated in Rust as UUIDv7 (`Uuid::now_v7()`). Natural keys only where another system owns the id (Amazon `place_id`, Stripe event ids, OAuth `client_id`, drone and dock ids from fleet config) or the key is a hash. |
| Time | `timestamptz`. Every table has `created_at timestamptz NOT NULL DEFAULT now()`. Rows that change also get `updated_at`, which the application sets in its `UPDATE` (no triggers). |
| Money | `bigint` minor units named `*_cents`, with `CHECK (… >= 0)`, next to `currency text NOT NULL DEFAULT 'usd'`. |
| Statuses | `text` with `CHECK (status IN (…))`. Values are the proto enum names without the prefix. No Postgres `ENUM` types. |
| Coordinates | `lat double precision CHECK (lat BETWEEN -90 AND 90)` and `lon double precision CHECK (lon BETWEEN -180 AND 180)`, WGS84. Positions a drone flies to also store `position_accuracy_m real` (95% horizontal radius) and, where it varies, their source. No PostGIS: radius searches filter on an indexed bounding box, then check distance. |
| Public keys | `public_key bytea` (DER SubjectPublicKeyInfo) plus `public_key_sha256 text CHECK (public_key_sha256 ~ '^[0-9a-f]{64}$')`, unique. Private keys are never stored. |
| Access networks | A child table with `kind text CHECK (kind IN (…))` and `params jsonb`, ordered by `priority`. Adding a network kind is a migration that widens the `CHECK`. |
| Secrets | Never stored raw. Passwords are argon2id hashes. Other one-way secrets are SHA-256 base64url hashes in `*_hash text`. Secrets that must be read back are AES-GCM encrypted: `*_ciphertext bytea` plus `*_nonce bytea`. |
| Names | `snake_case`, plural table names, and `<singular>_id` for foreign key columns. |
| Relationships inside a service | Real foreign keys with an explicit `ON DELETE`: `CASCADE` for rows owned by their parent, `RESTRICT` otherwise. Index every foreign key column. Read-model tables are the exception (see CQRS). |
| Concurrency | Rows that sagas or concurrent consumers update (`orders`, `fulfilment_orders`, `missions`, `drones`) have `version integer NOT NULL DEFAULT 0`. Updates filter on `version = $n` and increment it. |
| Text | `text`, with `CHECK (length(col) > 0)` where an empty value makes no sense. No `varchar(n)`. |

## References across services
Foreign keys cannot cross databases. References across services follow these rules instead:

1. **Reference by id.** Store the other service's id in a column without a foreign key, index it, and document where it points:
   ```sql
   COMMENT ON COLUMN orders.business_id IS 'ref: merchant.businesses.id';
   ```
   The user key everywhere is `sub` (`uuid`): `auth.users.id` and the JWT subject. The shop key visible to customers is the Amazon Location `place_id`.
2. **Validate when writing.** Before storing a reference, check it with the owning service over gRPC. For example, user-service calls `Fulfilment.PriceOrder` before storing an order's items.
3. **Copy what must not change.** Orders keep each item's name, spoken name, price and weight, and the pickup location's address and position, as they were at order time. Missions keep both stations' positions and key hashes as they were at dispatch.
4. **Follow events.** Owners publish changes; services holding references update or clean up in response. This is eventually consistent.
5. **Use sagas, not distributed transactions.** A change that spans services is a series of local transactions with compensating steps (void payment, release stock, recall drone), as in the order flow.

### Reference catalogue
| Column | Points to | Checked when written | Kept consistent by |
|---|---|---|---|
| `user_service.customers.sub`, `*_sub` columns everywhere | `auth.users.id` | The user's validated JWT | `auth.events.user_deleted`: delete or anonymize |
| `user_service.orders.business_id`, `user_service.orders.place_id` | `merchant.businesses.id`, `.place_id` | `Fulfilment.PriceOrder` | Copied at order time; none needed |
| `user_service.order_items.item_id` | `merchant.menu_items.id` | `Fulfilment.PriceOrder` | Name, price and weight copied |
| `user_service.orders.pickup_station_id` | `merchant.stations.id` | `Fulfilment.SubmitOrder` response | Position and key hash copied into the dispatch request |
| `merchant.fulfilment_orders.order_id`, `merchant.stock_reservations.order_id` | `user_service.orders.id` | `Fulfilment.SubmitOrder` from user | `Fulfilment.CancelOrder`; hold expiry |
| `merchant.fulfilment_orders.customer_sub` | `auth.users.id` | `Fulfilment.SubmitOrder` | `auth.events.user_deleted` |
| `merchant.notifications.order_id` | `user_service.orders.id` | Fulfilment change | None needed: it is a log |
| `catalog_read.catalogs.business_id`, `.place_id` | `merchant.businesses.id`, `.place_id` | `merchant.state.catalog.<place_id>` | Later state events (latest wins) |
| `catalog_read.catalog_items.item_id`, `catalog_sections.section_id` | `merchant.menu_items.id`, `merchant.menu_sections.id` | State events | Later state events; `deleted` tombstones |
| `dispatch.missions.order_id` | `user_service.orders.id` | `dispatch.<zone>.request` | `mission.<order_id>.*` events back to user |
| `dispatch.missions.dispatch_request_id` | user_service `outbox.id` (the `Nats-Msg-Id`) | `dispatch.<zone>.request` | Unique: a redelivered request can't create a second mission |
| `dispatch.missions.pickup_station_id` / `dropoff_station_id` | `merchant.stations.id` / `user_service.stations.id` | Copied from the dispatch request | Position, networks and key hash copied |

## CQRS: catalog projections
merchant-service is the write side; catalog-read-service is the read side, one database per zone.

**Publishing (merchant-service).** Every catalog, section or item change commits with an outbox row whose payload is that aggregate's **full current state**, on a per-aggregate subject: `merchant.state.catalog.<place_id>`, `merchant.state.section.<section_id>`, `merchant.state.item.<item_id>`. The `MERCHANT_EVENTS` stream keeps the latest message per subject, so it doubles as a snapshot of the whole catalog. Deletes publish a state message with `deleted = true`.

**Projecting (catalog-read-service).**
1. A durable consumer per zone reads the stream (sourced into the zone's JetStream domain).
2. In one transaction: `INSERT INTO inbox … ON CONFLICT DO NOTHING`; upsert the row **only if** the message's stream sequence is greater than the row's `source_seq` (a stale or replayed message changes nothing); update `projection_checkpoints`.
3. Commit, then acknowledge.

**Read-model rules.**
- One table per query shape, denormalized; no joins at query time beyond a single parent-child read.
- **No foreign keys** between read tables: events for different subjects may arrive in any order, and a projection must never fail because a parent hasn't arrived yet.
- Rebuild a zone by dropping the read tables' rows and replaying `MERCHANT_EVENTS` from the start.
- `/healthz` reports projection lag (stream last sequence minus checkpoint).
- Reads are never used to charge money or reserve stock; merchant-service re-validates.

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

**Publishing:** in the same transaction as the state change, insert the event into `outbox`. A relay (`svc_common::outbox::spawn_relay`) publishes waiting rows with `Nats-Msg-Id` set to the row's `id` and sets `published_at`.

**Consuming:** in one transaction, `INSERT INTO inbox … ON CONFLICT DO NOTHING`. If no row was inserted, the message is a duplicate: acknowledge it and stop. Otherwise apply the change, commit, and then acknowledge.

| Service | Outbox | Inbox |
|---|---|---|
| auth | Yes | No |
| user | Yes (dispatch requests) | Yes (fulfilment, mission events) |
| merchant | Yes (state and fulfilment events) | Yes (user deletion) |
| catalog-read | No (read-only) | Yes (state events) |
| dispatch | Yes (mission events) | Yes (dispatch requests) |

## Schemas by service
The migrations in `services/<svc>/migrations/` are the source of truth. This is a summary.

### auth
- **`users`:** `id` (the `sub`), `email` unique on `lower(email)`, `password_hash`, `email_verified_at`, `disabled_at`.
- **`sessions`:** login sessions with the `auth_time` and `amr` (`text[]`) the next tokens will carry.
- **`clients`:** `client_id`, `secret_hash`, `redirect_uris text[]`, `auth_method`, `kind` (`STATIC` or `DCR`).
- **`auth_requests`:** validated `/authorize` parameters (`jsonb`) parked during login and consent.
- **`grants`:** one row per authorization; its `id` is the `gid` claim; also `acr`, `amr`, `auth_time`, `revoked_at`.
- **`auth_codes`**, **`refresh_tokens`:** belong to a grant.
- **`passkeys`**, **`webauthn_challenges`:** the only second factor.
- **`signup_challenges`:** email verification codes during sign-up.
- **`outbox`**.

### user_service
- **`customers`:** keyed by `sub`. `approval_threshold_cents` (voice orders above it need app approval), Stripe customer and default payment method.
- **`pickup_locations`:** `label` (unique per customer among non-revoked, ignoring case), `address`, position with `position_accuracy_m` and `position_source` (`GEOCODE|MAP_PIN|DEVICE_GPS`), status `PENDING|VERIFIED|REVOKED`. `VERIFIED` needs `verified_at` and a `verified_amr` containing `hwk`.
- **`stations`** and **`station_access_networks`:** a customer station for a pickup location (public key and hash, status `PENDING|ACTIVE|RETIRED`, at most one active per location), with its access networks in priority order (`BLUETOOTH_LE` for now).
- **`quotes`**, **`quote_items`:** priced carts, valid 10 minutes. Placing an order must cite the latest quote and its total, and is idempotent on the quote.
- **Orders:**
  - `orders`: the customer's order and history. `place_id`, `business_id`, `business_spoken_name`, `placed_via` (`APP|MCP`), pickup location and its address/position copied, pickup and drop-off station ids, amounts, `payload_g`, `state` with the order states, `merchant_accept_by`, `version`. A composite foreign key keeps the pickup location the customer's own.
  - `order_items`: item name, spoken name, unit price, quantity and weight copied at order time.
  - `order_state_transitions`: the timeline, with a `cause` (`APP|MCP|MERCHANT_EVENT|MISSION_EVENT|PAYMENT_WEBHOOK|TIMER`). A transition to `PAID` must have cause `PAYMENT_WEBHOOK`.
  - `order_approvals`: one per order that crossed the threshold; status `PENDING|APPROVED|DECLINED|EXPIRED`; `APPROVED` needs `decided_amr` containing `hwk`.
- **Payments:** `payments` (one per order; `provider` `STRIPE` for now, idempotency key = order id; status `AUTHORIZING|AUTHORIZED|CAPTURED|VOIDED|FAILED|PARTIALLY_REFUNDED|REFUNDED`) and `payment_events` (provider webhook events keyed by event id, so duplicates are ignored). No wallet tables yet.
- **`outbox`**, **`inbox`**.

### merchant
- **`businesses`:** `place_id` (unique; claimed with `IntendedUse=Storage`), `name`, `spoken_name`, `address`, position, `categories text[]`, status `PENDING|ACTIVE|SUSPENDED`, `prep_time_minutes`, `accepting_orders`, `payments_enabled`, `approved_at`.
- **`business_members`:** `business_id` and `user_sub`, role `OWNER|STAFF`.
- **Catalog:** `menu_sections`; `menu_items` (`name`, `spoken_name`, `price_cents`, `weight_g > 0`, `stock_qty` NULL = unlimited, `available`). Composite foreign keys keep an item's section on the same business.
- **Stock holds:** `stock_reservations` (keyed by `order_id`; status `HELD|COMMITTED|RELEASED`; `expires_at`) and `stock_reservation_items`.
- **`fulfilment_orders`:** the business side of each order: items snapshot, subtotal, `accept_by`, status `AWAITING_DECISION|ACCEPTED|REJECTED|EXPIRED|CANCELLED`, `decided_by_sub`, `loaded_at`, pickup station, `version`.
- **`stations`** and **`station_access_networks`:** the business's pickup station (position with accuracy, public key and hash, status `PENDING|ACTIVE|RETIRED`, at most one active per business) and its access networks.
- **`merchant_accounts`:** Stripe Connect account per business, `charges_enabled`, `payouts_enabled`.
- **`notifications`**, **`business_webhooks`**, **`outbox`**, **`inbox`**.

### catalog_read (per zone)
- **`catalogs`:** primary key `place_id`; `business_id` (unique), `spoken_name`, `address`, position (bounding-box index), `categories` (GIN), `accepting_orders`, `prep_time_minutes`, `source_seq`.
- **`catalog_sections`:** `section_id`, `place_id`, `name`, `position`, `source_seq`.
- **`catalog_items`:** `item_id`, `place_id`, `section_id`, `name`, `spoken_name`, `description`, `price_cents`, `currency`, `weight_g`, `available`, `stock_remaining`, `source_seq`.
- **`projection_checkpoints`:** `consumer`, `stream_seq`.
- **`inbox`**.

### dispatch (per zone)
- **`docks`:** charging and launch docks from the zone's fleet config: id, position, capacity.
- **`drones`:** id, home dock, model, `max_payload_g`, status `IDLE|ASSIGNED|FLYING|CHARGING|OUT_OF_SERVICE`, battery, last position and `last_seen_at`, `certificate_sha256` (the drone's TLS client certificate), `version`.
- **`missions`:** one per order, keyed by `order_id`, with a unique `dispatch_request_id`. `drone_id`, state with the mission states, `payload_g`, and for each leg (pickup, drop-off) the station id, position, accuracy, `public_key_sha256` and access networks (`jsonb`) copied from the request; `attempts`, `eta_seconds`, `version`. Flying states need a drone.
- **`station_handshakes`:** every attempt: order, drone, leg `PICKUP|DROPOFF`, station id, access network, ranging method `GNSS|RSSI|CHANNEL_SOUNDING`, distance, result `VERIFIED|NOT_FOUND|KEY_MISMATCH|TLS_FAILED|REJECTED|TIMEOUT`, detail, `attempted_at`.
- **`outbox`**, **`inbox`**.
