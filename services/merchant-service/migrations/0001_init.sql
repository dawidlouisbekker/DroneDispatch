-- merchant-service schema: businesses, menus, stock holds, pickup points,
-- the orders-board projection, notifications, POS webhooks, outbox and inbox.
-- Conventions and cross-service references: docs/DATABASE.md.

-- ---------------------------------------------------------------------------
-- Businesses and members
-- ---------------------------------------------------------------------------

CREATE TABLE businesses (
    id                uuid PRIMARY KEY,
    place_id          text NOT NULL UNIQUE CHECK (length(place_id) > 0),
    name              text NOT NULL CHECK (length(name) > 0),
    address           text NOT NULL CHECK (length(address) > 0),
    lat               double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
    lon               double precision NOT NULL CHECK (lon BETWEEN -180 AND 180),
    categories        text[] NOT NULL DEFAULT '{}',
    status            text NOT NULL DEFAULT 'PENDING'
                      CHECK (status IN ('PENDING', 'ACTIVE', 'SUSPENDED')),
    prep_time_minutes integer NOT NULL DEFAULT 10 CHECK (prep_time_minutes > 0),
    accepting_orders  boolean NOT NULL DEFAULT false,
    payments_enabled  boolean NOT NULL DEFAULT false,
    approved_at       timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    -- A business only becomes ACTIVE through admin approval.
    CONSTRAINT businesses_active_is_approved
        CHECK (status <> 'ACTIVE' OR approved_at IS NOT NULL)
);
-- Radius search: filter on an indexed bounding box, then check distance.
CREATE INDEX businesses_lat_lon ON businesses (lat, lon);
CREATE INDEX businesses_categories ON businesses USING gin (categories);
COMMENT ON COLUMN businesses.place_id IS 'Amazon Location Places V2 place id, claimed with GetPlace IntendedUse=Storage';
COMMENT ON COLUMN businesses.payments_enabled IS 'projection of commerce MerchantAccountUpdated (commerce.merchant_account.<business_id>.updated)';

CREATE TABLE business_members (
    business_id uuid NOT NULL REFERENCES businesses (id) ON DELETE CASCADE,
    user_sub    uuid NOT NULL,
    role        text NOT NULL CHECK (role IN ('OWNER', 'STAFF')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (business_id, user_sub)
);
CREATE INDEX business_members_user_sub ON business_members (user_sub);
COMMENT ON COLUMN business_members.user_sub IS 'ref: auth.users.id';

-- ---------------------------------------------------------------------------
-- Menu
-- ---------------------------------------------------------------------------

CREATE TABLE menu_sections (
    id          uuid PRIMARY KEY,
    business_id uuid NOT NULL REFERENCES businesses (id) ON DELETE CASCADE,
    name        text NOT NULL CHECK (length(name) > 0),
    position    integer NOT NULL CHECK (position >= 0),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    -- Deferrable so a reorder can shift positions within one statement or transaction.
    CONSTRAINT menu_sections_business_position
        UNIQUE (business_id, position) DEFERRABLE INITIALLY IMMEDIATE,
    -- Target of menu_items' composite foreign key.
    CONSTRAINT menu_sections_id_business UNIQUE (id, business_id)
);

CREATE TABLE menu_items (
    id          uuid PRIMARY KEY,
    business_id uuid NOT NULL REFERENCES businesses (id) ON DELETE CASCADE,
    section_id  uuid NOT NULL,
    name        text NOT NULL CHECK (length(name) > 0),
    description text NOT NULL DEFAULT '',
    price_cents bigint NOT NULL CHECK (price_cents >= 0),
    currency    text NOT NULL DEFAULT 'usd',
    weight_g    integer NOT NULL CHECK (weight_g > 0),
    stock_qty   integer CHECK (stock_qty >= 0),
    available   boolean NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    -- The section must belong to the same business as the item.
    CONSTRAINT menu_items_section_same_business
        FOREIGN KEY (section_id, business_id)
        REFERENCES menu_sections (id, business_id) ON DELETE CASCADE,
    -- Target of stock_reservation_items' composite foreign key.
    CONSTRAINT menu_items_id_business UNIQUE (id, business_id)
);
CREATE INDEX menu_items_business_id ON menu_items (business_id);
CREATE INDEX menu_items_section_id ON menu_items (section_id, business_id);
COMMENT ON COLUMN menu_items.stock_qty IS 'NULL means unlimited';

-- ---------------------------------------------------------------------------
-- Stock holds
-- ---------------------------------------------------------------------------

CREATE TABLE stock_reservations (
    id          uuid PRIMARY KEY,
    business_id uuid NOT NULL REFERENCES businesses (id) ON DELETE RESTRICT,
    status      text NOT NULL DEFAULT 'HELD'
                CHECK (status IN ('HELD', 'COMMITTED', 'RELEASED')),
    expires_at  timestamptz NOT NULL,
    order_id    uuid,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT stock_reservations_committed_has_order
        CHECK (status <> 'COMMITTED' OR order_id IS NOT NULL),
    -- Target of stock_reservation_items' composite foreign key.
    CONSTRAINT stock_reservations_id_business UNIQUE (id, business_id)
);
CREATE INDEX stock_reservations_business_id ON stock_reservations (business_id);
-- Expiry sweep over live holds.
CREATE INDEX stock_reservations_held_expires_at ON stock_reservations (expires_at) WHERE status = 'HELD';
-- An order is placed from exactly one quote.
CREATE UNIQUE INDEX stock_reservations_order_id ON stock_reservations (order_id) WHERE order_id IS NOT NULL;
COMMENT ON COLUMN stock_reservations.id IS 'ref: commerce.quotes.id (the quote_id)';
COMMENT ON COLUMN stock_reservations.order_id IS 'ref: commerce.orders.id';

CREATE TABLE stock_reservation_items (
    reservation_id uuid NOT NULL,
    item_id        uuid NOT NULL,
    business_id    uuid NOT NULL,
    qty            integer NOT NULL CHECK (qty > 0),
    created_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (reservation_id, item_id),
    CONSTRAINT stock_reservation_items_reservation
        FOREIGN KEY (reservation_id, business_id)
        REFERENCES stock_reservations (id, business_id) ON DELETE CASCADE,
    -- Only items on the reserving business's menu can be held.
    CONSTRAINT stock_reservation_items_item_same_business
        FOREIGN KEY (item_id, business_id)
        REFERENCES menu_items (id, business_id) ON DELETE RESTRICT
);
CREATE INDEX stock_reservation_items_reservation ON stock_reservation_items (reservation_id, business_id);
CREATE INDEX stock_reservation_items_item ON stock_reservation_items (item_id, business_id);

-- ---------------------------------------------------------------------------
-- Pickup points
-- ---------------------------------------------------------------------------

CREATE TABLE pickup_points (
    id               uuid PRIMARY KEY,
    business_id      uuid NOT NULL REFERENCES businesses (id) ON DELETE RESTRICT,
    lat              double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
    lon              double precision NOT NULL CHECK (lon BETWEEN -180 AND 180),
    status           text NOT NULL DEFAULT 'PENDING_PHOTO'
                     CHECK (status IN ('PENDING_PHOTO', 'VERIFIED', 'REJECTED')),
    photo_object_key text CHECK (length(photo_object_key) > 0),
    photo_sha256     text CHECK (photo_sha256 ~ '^[0-9a-f]{64}$'),
    verified_at      timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT pickup_points_verified_has_photo
        CHECK (status <> 'VERIFIED'
               OR (photo_object_key IS NOT NULL AND photo_sha256 IS NOT NULL AND verified_at IS NOT NULL))
);
CREATE INDEX pickup_points_business_id ON pickup_points (business_id);
-- At most one pending or verified pickup point per business.
CREATE UNIQUE INDEX pickup_points_one_active_per_business ON pickup_points (business_id) WHERE status <> 'REJECTED';
COMMENT ON COLUMN pickup_points.photo_object_key IS 'key of the marker photo in the PICKUP_ASSETS object store (the proto asset_key)';
COMMENT ON COLUMN pickup_points.photo_sha256 IS 'lowercase hex SHA-256 of the marker photo';

-- ---------------------------------------------------------------------------
-- Orders board (projection of commerce order.<id>.* events)
-- ---------------------------------------------------------------------------

CREATE TABLE merchant_orders (
    order_id       uuid PRIMARY KEY,
    business_id    uuid NOT NULL REFERENCES businesses (id) ON DELETE RESTRICT,
    items          jsonb NOT NULL
                   CHECK (CASE WHEN jsonb_typeof(items) = 'array' THEN jsonb_array_length(items) > 0 ELSE false END),
    total_cents    bigint NOT NULL CHECK (total_cents >= 0),
    currency       text NOT NULL DEFAULT 'usd',
    accept_by      timestamptz NOT NULL,
    status         text NOT NULL DEFAULT 'AWAITING_DECISION'
                   CHECK (status IN ('AWAITING_DECISION', 'ACCEPTED', 'REJECTED', 'EXPIRED')),
    decided_by_sub uuid,
    decided_at     timestamptz,
    loaded_at      timestamptz,
    version        integer NOT NULL DEFAULT 0,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT merchant_orders_awaiting_undecided
        CHECK (status <> 'AWAITING_DECISION' OR (decided_at IS NULL AND decided_by_sub IS NULL)),
    CONSTRAINT merchant_orders_decision_has_time
        CHECK (status NOT IN ('ACCEPTED', 'REJECTED') OR decided_at IS NOT NULL),
    CONSTRAINT merchant_orders_loaded_after_accept
        CHECK (loaded_at IS NULL OR status = 'ACCEPTED')
);
-- Board listing per business, newest first; also covers the foreign key.
CREATE INDEX merchant_orders_business_created ON merchant_orders (business_id, created_at DESC);
-- Timeout sweep over orders still waiting for a decision.
CREATE INDEX merchant_orders_awaiting_accept_by ON merchant_orders (accept_by) WHERE status = 'AWAITING_DECISION';
CREATE INDEX merchant_orders_decided_by_sub ON merchant_orders (decided_by_sub) WHERE decided_by_sub IS NOT NULL;
COMMENT ON COLUMN merchant_orders.order_id IS 'ref: commerce.orders.id';
COMMENT ON COLUMN merchant_orders.decided_by_sub IS 'ref: auth.users.id';
COMMENT ON COLUMN merchant_orders.items IS 'snapshot of the order line items (item_id, name, unit price, qty, weight_g) at authorization';

-- ---------------------------------------------------------------------------
-- Notifications and POS webhooks
-- ---------------------------------------------------------------------------

CREATE TABLE notifications (
    id          uuid PRIMARY KEY,
    business_id uuid NOT NULL REFERENCES businesses (id) ON DELETE CASCADE,
    order_id    uuid,
    channel     text NOT NULL CHECK (channel IN ('EMAIL', 'WEBHOOK', 'PORTAL')),
    status      text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'SENT', 'FAILED')),
    error       text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    sent_at     timestamptz,
    CONSTRAINT notifications_sent_has_time CHECK (status <> 'SENT' OR sent_at IS NOT NULL),
    CONSTRAINT notifications_failed_has_error CHECK (status <> 'FAILED' OR error IS NOT NULL)
);
CREATE INDEX notifications_business_created ON notifications (business_id, created_at DESC);
CREATE INDEX notifications_order_id ON notifications (order_id) WHERE order_id IS NOT NULL;
CREATE INDEX notifications_pending ON notifications (created_at) WHERE status = 'PENDING';
COMMENT ON COLUMN notifications.order_id IS 'ref: commerce.orders.id';

CREATE TABLE business_webhooks (
    business_id               uuid PRIMARY KEY REFERENCES businesses (id) ON DELETE CASCADE,
    url                       text NOT NULL CHECK (url ~ '^https://[^/[:space:]]'),
    signing_secret_ciphertext bytea NOT NULL CHECK (length(signing_secret_ciphertext) > 0),
    signing_secret_nonce      bytea NOT NULL CHECK (length(signing_secret_nonce) = 12),
    created_at                timestamptz NOT NULL DEFAULT now(),
    updated_at                timestamptz NOT NULL DEFAULT now()
);
COMMENT ON COLUMN business_webhooks.signing_secret_ciphertext IS 'AES-GCM encrypted HMAC signing secret';
COMMENT ON COLUMN business_webhooks.signing_secret_nonce IS '96-bit AES-GCM nonce';

-- ---------------------------------------------------------------------------
-- Outbox and inbox (identical in every service, see docs/DATABASE.md)
-- ---------------------------------------------------------------------------

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
