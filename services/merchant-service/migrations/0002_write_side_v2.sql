-- Architecture v2 (docs/ARCHITECTURE.md): merchant-service is the write side for
-- businesses. Orders now belong to user-service and catalog reads to
-- catalog-read-service; pickup is authenticated by a wireless station instead of a
-- QR marker photo.

-- ---------------------------------------------------------------------------
-- Spoken names that Alexa+ reads out. Existing rows start from their display names.
-- ---------------------------------------------------------------------------

ALTER TABLE businesses ADD COLUMN spoken_name text;
UPDATE businesses SET spoken_name = name;
ALTER TABLE businesses
    ALTER COLUMN spoken_name SET NOT NULL,
    ADD CONSTRAINT businesses_spoken_name_not_empty CHECK (length(spoken_name) > 0);
COMMENT ON COLUMN businesses.payments_enabled IS 'true once merchant_accounts has charges and payouts enabled';

ALTER TABLE menu_items ADD COLUMN spoken_name text;
UPDATE menu_items SET spoken_name = name;
ALTER TABLE menu_items
    ALTER COLUMN spoken_name SET NOT NULL,
    ADD CONSTRAINT menu_items_spoken_name_not_empty CHECK (length(spoken_name) > 0);

-- ---------------------------------------------------------------------------
-- Stations replace photo-verified pickup points
-- ---------------------------------------------------------------------------

DROP TABLE pickup_points;

CREATE TABLE stations (
    id                  uuid PRIMARY KEY,
    business_id         uuid NOT NULL REFERENCES businesses (id) ON DELETE RESTRICT,
    -- Where drones fly before scanning: the pickup pad.
    lat                 double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
    lon                 double precision NOT NULL CHECK (lon BETWEEN -180 AND 180),
    -- 95% horizontal error radius of the position.
    position_accuracy_m real NOT NULL CHECK (position_accuracy_m > 0),
    -- DER SubjectPublicKeyInfo of the station's TLS key; drones pin its SHA-256.
    public_key          bytea NOT NULL CHECK (octet_length(public_key) > 0),
    public_key_sha256   text NOT NULL UNIQUE CHECK (public_key_sha256 ~ '^[0-9a-f]{64}$'),
    status              text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'ACTIVE', 'RETIRED')),
    activated_at        timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT stations_active_activated CHECK (status <> 'ACTIVE' OR activated_at IS NOT NULL),
    -- Target of fulfilment_orders' composite foreign key.
    CONSTRAINT stations_id_business UNIQUE (id, business_id)
);
CREATE INDEX stations_business_id ON stations (business_id);
CREATE UNIQUE INDEX stations_one_active_per_business ON stations (business_id) WHERE status = 'ACTIVE';

CREATE TABLE station_access_networks (
    id         uuid PRIMARY KEY,
    station_id uuid NOT NULL REFERENCES stations (id) ON DELETE CASCADE,
    -- Lower tries first.
    priority   smallint NOT NULL CHECK (priority >= 0),
    -- dronedrop.station.v1.AccessNetwork case. Widen this CHECK to add Wi-Fi and other networks.
    kind       text NOT NULL CHECK (kind IN ('BLUETOOTH_LE')),
    -- Kind-specific settings, e.g. {"service_uuid": "…", "station_tag": "…", "l2cap_psm": 128}.
    params     jsonb NOT NULL CHECK (jsonb_typeof(params) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (station_id, priority)   -- also serves as the station_id foreign key index
);

-- ---------------------------------------------------------------------------
-- Fulfilment: the business side of user-service's orders
-- ---------------------------------------------------------------------------

DROP TABLE merchant_orders;
-- Holds are now taken when an order is submitted, and keyed by the order.
DROP TABLE stock_reservation_items;
DROP TABLE stock_reservations;

CREATE TABLE fulfilment_orders (
    order_id          uuid PRIMARY KEY,
    business_id       uuid NOT NULL REFERENCES businesses (id) ON DELETE RESTRICT,
    customer_sub      uuid NOT NULL,
    -- The business station the drone collects from, fixed when the order is submitted.
    pickup_station_id uuid NOT NULL,
    -- Snapshot of the priced lines (item_id, name, spoken name, unit price, qty, weight_g).
    items             jsonb NOT NULL
                      CHECK (CASE WHEN jsonb_typeof(items) = 'array' THEN jsonb_array_length(items) > 0 ELSE false END),
    subtotal_cents    bigint NOT NULL CHECK (subtotal_cents >= 0),
    currency          text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    accept_by         timestamptz NOT NULL,
    status            text NOT NULL DEFAULT 'AWAITING_DECISION'
                      CHECK (status IN ('AWAITING_DECISION', 'ACCEPTED', 'REJECTED', 'EXPIRED', 'CANCELLED', 'LOADED')),
    decided_by_sub    uuid,
    decided_at        timestamptz,
    reject_reason     text CHECK (length(reject_reason) > 0),
    loaded_at         timestamptz,
    version           integer NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT fulfilment_orders_station_same_business FOREIGN KEY (pickup_station_id, business_id)
        REFERENCES stations (id, business_id) ON DELETE RESTRICT,
    CONSTRAINT fulfilment_orders_awaiting_undecided
        CHECK (status <> 'AWAITING_DECISION' OR (decided_at IS NULL AND decided_by_sub IS NULL)),
    CONSTRAINT fulfilment_orders_decision_has_time
        CHECK (status NOT IN ('ACCEPTED', 'REJECTED', 'LOADED') OR decided_at IS NOT NULL),
    -- Loaded (the station acknowledged the handoff) exactly when loaded_at is set.
    CONSTRAINT fulfilment_orders_loaded CHECK ((status = 'LOADED') = (loaded_at IS NOT NULL)),
    -- Target of stock_reservations' composite foreign key.
    CONSTRAINT fulfilment_orders_order_business UNIQUE (order_id, business_id)
);
COMMENT ON COLUMN fulfilment_orders.order_id IS 'ref: user_service.orders.id';
COMMENT ON COLUMN fulfilment_orders.customer_sub IS 'ref: auth.users.id';
COMMENT ON COLUMN fulfilment_orders.decided_by_sub IS 'ref: auth.users.id';
-- Board listing per business, newest first; also covers the business foreign key.
CREATE INDEX fulfilment_orders_business_created ON fulfilment_orders (business_id, created_at DESC);
-- Timeout sweep over orders still waiting for a decision.
CREATE INDEX fulfilment_orders_awaiting_accept_by ON fulfilment_orders (accept_by) WHERE status = 'AWAITING_DECISION';
CREATE INDEX fulfilment_orders_pickup_station ON fulfilment_orders (pickup_station_id, business_id);
CREATE INDEX fulfilment_orders_decided_by_sub ON fulfilment_orders (decided_by_sub) WHERE decided_by_sub IS NOT NULL;

CREATE TABLE stock_reservations (
    order_id    uuid PRIMARY KEY,
    business_id uuid NOT NULL,
    status      text NOT NULL DEFAULT 'HELD' CHECK (status IN ('HELD', 'COMMITTED', 'RELEASED')),
    expires_at  timestamptz NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT stock_reservations_order FOREIGN KEY (order_id, business_id)
        REFERENCES fulfilment_orders (order_id, business_id) ON DELETE CASCADE,
    -- Target of stock_reservation_items' composite foreign key; also indexes the order foreign key.
    CONSTRAINT stock_reservations_order_business UNIQUE (order_id, business_id)
);
-- Expiry sweep over live holds.
CREATE INDEX stock_reservations_held_expires_at ON stock_reservations (expires_at) WHERE status = 'HELD';

CREATE TABLE stock_reservation_items (
    order_id    uuid NOT NULL,
    item_id     uuid NOT NULL,
    business_id uuid NOT NULL,
    qty         integer NOT NULL CHECK (qty > 0),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (order_id, item_id),
    CONSTRAINT stock_reservation_items_reservation FOREIGN KEY (order_id, business_id)
        REFERENCES stock_reservations (order_id, business_id) ON DELETE CASCADE,
    -- Only items on the reserving business's menu can be held.
    CONSTRAINT stock_reservation_items_item_same_business FOREIGN KEY (item_id, business_id)
        REFERENCES menu_items (id, business_id) ON DELETE RESTRICT
);
CREATE INDEX stock_reservation_items_reservation ON stock_reservation_items (order_id, business_id);
CREATE INDEX stock_reservation_items_item ON stock_reservation_items (item_id, business_id);

-- ---------------------------------------------------------------------------
-- Stripe Connect accounts (moved from commerce-service)
-- ---------------------------------------------------------------------------

CREATE TABLE merchant_accounts (
    business_id       uuid PRIMARY KEY REFERENCES businesses (id) ON DELETE RESTRICT,
    stripe_account_id text NOT NULL UNIQUE CHECK (length(stripe_account_id) > 0),
    charges_enabled   boolean NOT NULL DEFAULT false,
    payouts_enabled   boolean NOT NULL DEFAULT false,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

COMMENT ON COLUMN notifications.order_id IS 'ref: user_service.orders.id';
