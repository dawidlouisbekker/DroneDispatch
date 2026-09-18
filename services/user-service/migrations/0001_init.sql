-- user-service schema (database `user_service`): customers and their spend policy,
-- pickup locations and stations, quotes, orders and the full order history,
-- approvals, payments, outbox and inbox. Conventions and cross-service references:
-- docs/DATABASE.md.
--
-- Deletes: only owned children cascade (access networks, quote and order items,
-- transitions, approvals). Everything else is RESTRICT, so order history and money
-- rows never disappear with a customer, a location or a station.

-- ---------------------------------------------------------------------------
-- Customers
-- ---------------------------------------------------------------------------

CREATE TABLE customers (
    sub                       uuid PRIMARY KEY,
    -- Voice (MCP) orders above this total wait for approval in the app. Only the app changes it.
    approval_threshold_cents  bigint NOT NULL DEFAULT 3000 CHECK (approval_threshold_cents >= 0),
    currency                  text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    stripe_customer_id        text UNIQUE CHECK (length(stripe_customer_id) > 0),
    default_payment_method_id text CHECK (length(default_payment_method_id) > 0),
    created_at                timestamptz NOT NULL DEFAULT now(),
    updated_at                timestamptz NOT NULL DEFAULT now(),
    -- A Stripe payment method is attached to a Stripe customer.
    CONSTRAINT customers_payment_method_needs_stripe_customer
        CHECK (default_payment_method_id IS NULL OR stripe_customer_id IS NOT NULL)
);
COMMENT ON COLUMN customers.sub IS 'ref: auth.users.id';

-- ---------------------------------------------------------------------------
-- Pickup locations and stations
-- ---------------------------------------------------------------------------

CREATE TABLE pickup_locations (
    id                  uuid PRIMARY KEY,
    customer_sub        uuid NOT NULL REFERENCES customers (sub) ON DELETE RESTRICT,
    label               text NOT NULL CHECK (length(label) > 0),
    address             text NOT NULL CHECK (length(address) > 0),
    lat                 double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
    lon                 double precision NOT NULL CHECK (lon BETWEEN -180 AND 180),
    -- 95% horizontal error radius, when known (e.g. the phone's reported GPS accuracy).
    position_accuracy_m real CHECK (position_accuracy_m > 0),
    position_source     text NOT NULL CHECK (position_source IN ('GEOCODE', 'MAP_PIN', 'DEVICE_GPS')),
    status              text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'VERIFIED', 'REVOKED')),
    -- Passkey step-up evidence from the customer's token.
    verified_at         timestamptz,
    verified_amr        text[],
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    -- VERIFIED needs the evidence, including a passkey ("hwk").
    CONSTRAINT pickup_locations_verified_evidence CHECK (
        status <> 'VERIFIED'
        OR (verified_at IS NOT NULL AND verified_amr IS NOT NULL AND verified_amr && ARRAY['hwk']::text[])
    ),
    -- PENDING has never been verified. REVOKED keeps whatever evidence it had.
    CONSTRAINT pickup_locations_pending_unverified CHECK (
        status <> 'PENDING' OR (verified_at IS NULL AND verified_amr IS NULL)
    ),
    -- Target of the composite foreign keys that keep stations, quotes and orders on the
    -- customer's own locations.
    CONSTRAINT pickup_locations_id_customer UNIQUE (id, customer_sub)
);
CREATE INDEX pickup_locations_customer_sub ON pickup_locations (customer_sub);
-- Labels ("home", "office") are spoken, so compare them case-insensitively.
CREATE UNIQUE INDEX pickup_locations_active_label
    ON pickup_locations (customer_sub, lower(label))
    WHERE status <> 'REVOKED';

-- A customer's station at a pickup location. The drone flies to the location's position.
CREATE TABLE stations (
    id                 uuid PRIMARY KEY,
    customer_sub       uuid NOT NULL,
    pickup_location_id uuid NOT NULL,
    -- DER SubjectPublicKeyInfo of the station's TLS key; drones pin its SHA-256.
    public_key         bytea NOT NULL CHECK (octet_length(public_key) > 0),
    public_key_sha256  text NOT NULL UNIQUE CHECK (public_key_sha256 ~ '^[0-9a-f]{64}$'),
    status             text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'ACTIVE', 'RETIRED')),
    activated_at       timestamptz,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT stations_own_location FOREIGN KEY (pickup_location_id, customer_sub)
        REFERENCES pickup_locations (id, customer_sub) ON DELETE RESTRICT,
    CONSTRAINT stations_active_activated CHECK (status <> 'ACTIVE' OR activated_at IS NOT NULL),
    -- Target of orders_own_dropoff_station.
    CONSTRAINT stations_id_customer UNIQUE (id, customer_sub)
);
CREATE INDEX stations_pickup_location ON stations (pickup_location_id, customer_sub);
CREATE UNIQUE INDEX stations_one_active_per_location ON stations (pickup_location_id) WHERE status = 'ACTIVE';

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
-- Quotes (priced carts, valid 10 minutes)
-- ---------------------------------------------------------------------------

CREATE TABLE quotes (
    id                   uuid PRIMARY KEY,
    customer_sub         uuid NOT NULL REFERENCES customers (sub) ON DELETE RESTRICT,
    channel              text NOT NULL CHECK (channel IN ('APP', 'MCP')),
    place_id             text NOT NULL CHECK (length(place_id) > 0),
    business_id          uuid NOT NULL,
    business_spoken_name text NOT NULL CHECK (length(business_spoken_name) > 0),
    pickup_location_id   uuid NOT NULL,
    subtotal_cents       bigint NOT NULL CHECK (subtotal_cents >= 0),
    delivery_fee_cents   bigint NOT NULL CHECK (delivery_fee_cents >= 0),
    total_cents          bigint NOT NULL CHECK (total_cents >= 0),
    currency             text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    payload_g            integer NOT NULL CHECK (payload_g > 0 AND payload_g <= 2500),
    eta_seconds          integer NOT NULL CHECK (eta_seconds >= 0),
    expires_at           timestamptz NOT NULL,
    -- Set when a newer cart replaces this quote. Its only change.
    superseded_at        timestamptz,
    created_at           timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT quotes_total CHECK (total_cents = subtotal_cents + delivery_fee_cents),
    CONSTRAINT quotes_expiry CHECK (expires_at > created_at),
    CONSTRAINT quotes_own_location FOREIGN KEY (pickup_location_id, customer_sub)
        REFERENCES pickup_locations (id, customer_sub) ON DELETE RESTRICT,
    -- Target of orders_own_quote.
    CONSTRAINT quotes_id_customer UNIQUE (id, customer_sub)
);
COMMENT ON COLUMN quotes.place_id IS 'ref: merchant.businesses.place_id';
COMMENT ON COLUMN quotes.business_id IS 'ref: merchant.businesses.id';
CREATE INDEX quotes_customer_created ON quotes (customer_sub, created_at);
CREATE INDEX quotes_pickup_location ON quotes (pickup_location_id, customer_sub);

CREATE TABLE quote_items (
    quote_id         uuid NOT NULL REFERENCES quotes (id) ON DELETE CASCADE,
    item_id          uuid NOT NULL,
    -- Priced by merchant-service (Fulfilment.PriceOrder). Currency is the quote's.
    name             text NOT NULL CHECK (length(name) > 0),
    spoken_name      text NOT NULL CHECK (length(spoken_name) > 0),
    unit_price_cents bigint NOT NULL CHECK (unit_price_cents >= 0),
    qty              integer NOT NULL CHECK (qty > 0),
    weight_g         integer NOT NULL CHECK (weight_g > 0),
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (quote_id, item_id)
);
COMMENT ON COLUMN quote_items.item_id IS 'ref: merchant.menu_items.id';

-- ---------------------------------------------------------------------------
-- Payment provider webhooks
-- ---------------------------------------------------------------------------

-- Received webhook events. The primary key makes a duplicate delivery conflict.
CREATE TABLE payment_events (
    id           text PRIMARY KEY CHECK (length(id) > 0),
    provider     text NOT NULL CHECK (provider IN ('STRIPE')),
    type         text NOT NULL CHECK (length(type) > 0),
    payload      jsonb NOT NULL,
    received_at  timestamptz NOT NULL DEFAULT now(),
    processed_at timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX payment_events_unprocessed ON payment_events (received_at) WHERE processed_at IS NULL;

-- ---------------------------------------------------------------------------
-- Orders and order history
-- ---------------------------------------------------------------------------

CREATE TABLE orders (
    id                   uuid PRIMARY KEY,
    customer_sub         uuid NOT NULL REFERENCES customers (sub) ON DELETE RESTRICT,
    -- Placing is idempotent on quote_id.
    quote_id             uuid NOT NULL UNIQUE,
    channel              text NOT NULL CHECK (channel IN ('APP', 'MCP')),
    place_id             text NOT NULL CHECK (length(place_id) > 0),
    business_id          uuid NOT NULL,
    business_spoken_name text NOT NULL CHECK (length(business_spoken_name) > 0),
    -- dronedrop.user.v1.OrderState without the prefix. Only a verified payment webhook may set PAID.
    state                text NOT NULL CHECK (state IN (
        'AWAITING_APPROVAL', 'AUTHORIZING', 'AWAITING_MERCHANT', 'CAPTURING', 'PAID',
        'DISPATCH_REQUESTED', 'DRONE_ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'DELIVERED',
        'COMPLETED', 'PAYMENT_FAILED', 'CANCELLED', 'REFUNDED'
    )),
    -- Drop-off: the customer's pickup location, copied at order time.
    pickup_location_id   uuid NOT NULL,
    dropoff_label        text NOT NULL CHECK (length(dropoff_label) > 0),
    dropoff_address      text NOT NULL CHECK (length(dropoff_address) > 0),
    dropoff_lat          double precision NOT NULL CHECK (dropoff_lat BETWEEN -90 AND 90),
    dropoff_lon          double precision NOT NULL CHECK (dropoff_lon BETWEEN -180 AND 180),
    dropoff_station_id   uuid,
    -- Pickup: the business station from Fulfilment.SubmitOrder.
    pickup_station_id    uuid,
    pickup_lat           double precision CHECK (pickup_lat BETWEEN -90 AND 90),
    pickup_lon           double precision CHECK (pickup_lon BETWEEN -180 AND 180),
    subtotal_cents       bigint NOT NULL CHECK (subtotal_cents >= 0),
    delivery_fee_cents   bigint NOT NULL CHECK (delivery_fee_cents >= 0),
    total_cents          bigint NOT NULL CHECK (total_cents > 0),
    currency             text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    payload_g            integer NOT NULL CHECK (payload_g > 0 AND payload_g <= 2500),
    eta_seconds          integer CHECK (eta_seconds >= 0),
    -- Set when the order starts waiting for the business.
    merchant_accept_by   timestamptz,
    version              integer NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at           timestamptz NOT NULL DEFAULT now(),
    updated_at           timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT orders_total CHECK (total_cents = subtotal_cents + delivery_fee_cents),
    CONSTRAINT orders_accept_by CHECK (state <> 'AWAITING_MERCHANT' OR merchant_accept_by IS NOT NULL),
    CONSTRAINT orders_pickup_position_pair CHECK ((pickup_lat IS NULL) = (pickup_lon IS NULL)),
    -- A drone can only be requested with both stations known.
    CONSTRAINT orders_dispatched_has_stations CHECK (
        state NOT IN ('DISPATCH_REQUESTED', 'DRONE_ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'DELIVERED', 'COMPLETED')
        OR (pickup_station_id IS NOT NULL AND pickup_lat IS NOT NULL AND dropoff_station_id IS NOT NULL)
    ),
    -- The quote, location and station must all belong to the ordering customer.
    CONSTRAINT orders_own_quote FOREIGN KEY (quote_id, customer_sub)
        REFERENCES quotes (id, customer_sub) ON DELETE RESTRICT,
    CONSTRAINT orders_own_location FOREIGN KEY (pickup_location_id, customer_sub)
        REFERENCES pickup_locations (id, customer_sub) ON DELETE RESTRICT,
    CONSTRAINT orders_own_dropoff_station FOREIGN KEY (dropoff_station_id, customer_sub)
        REFERENCES stations (id, customer_sub) ON DELETE RESTRICT
);
COMMENT ON COLUMN orders.place_id IS 'ref: merchant.businesses.place_id';
COMMENT ON COLUMN orders.business_id IS 'ref: merchant.businesses.id';
COMMENT ON COLUMN orders.pickup_station_id IS 'ref: merchant.stations.id';
-- Order history (newest first).
CREATE INDEX orders_customer_created ON orders (customer_sub, created_at);
CREATE INDEX orders_business_id ON orders (business_id);
CREATE INDEX orders_pickup_location ON orders (pickup_location_id, customer_sub);
CREATE INDEX orders_dropoff_station ON orders (dropoff_station_id, customer_sub) WHERE dropoff_station_id IS NOT NULL;

CREATE TABLE order_items (
    order_id         uuid NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    item_id          uuid NOT NULL,
    -- Copied at order time. Currency is the order's.
    name             text NOT NULL CHECK (length(name) > 0),
    spoken_name      text NOT NULL CHECK (length(spoken_name) > 0),
    unit_price_cents bigint NOT NULL CHECK (unit_price_cents >= 0),
    qty              integer NOT NULL CHECK (qty > 0),
    weight_g         integer NOT NULL CHECK (weight_g > 0),
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (order_id, item_id)
);
COMMENT ON COLUMN order_items.item_id IS 'ref: merchant.menu_items.id';

CREATE TABLE order_state_transitions (
    id               uuid PRIMARY KEY,
    order_id         uuid NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    -- NULL for the order's first state.
    from_state       text CHECK (from_state IN (
        'AWAITING_APPROVAL', 'AUTHORIZING', 'AWAITING_MERCHANT', 'CAPTURING', 'PAID',
        'DISPATCH_REQUESTED', 'DRONE_ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'DELIVERED',
        'COMPLETED', 'PAYMENT_FAILED', 'CANCELLED', 'REFUNDED'
    )),
    to_state         text NOT NULL CHECK (to_state IN (
        'AWAITING_APPROVAL', 'AUTHORIZING', 'AWAITING_MERCHANT', 'CAPTURING', 'PAID',
        'DISPATCH_REQUESTED', 'DRONE_ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'DELIVERED',
        'COMPLETED', 'PAYMENT_FAILED', 'CANCELLED', 'REFUNDED'
    )),
    -- e.g. "merchant rejected: out of oat milk".
    reason           text CHECK (length(reason) > 0),
    cause            text NOT NULL CHECK (cause IN ('APP', 'MCP', 'MERCHANT_EVENT', 'MISSION_EVENT', 'PAYMENT_WEBHOOK', 'TIMER')),
    payment_event_id text REFERENCES payment_events (id) ON DELETE RESTRICT,
    created_at       timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT order_state_transitions_changes_state CHECK (from_state IS DISTINCT FROM to_state),
    CONSTRAINT order_state_transitions_webhook_event
        CHECK ((cause = 'PAYMENT_WEBHOOK') = (payment_event_id IS NOT NULL)),
    -- Only a verified (stored) payment webhook can move an order to PAID.
    CONSTRAINT order_state_transitions_paid_by_webhook
        CHECK (to_state <> 'PAID' OR cause = 'PAYMENT_WEBHOOK')
);
CREATE INDEX order_state_transitions_order_created ON order_state_transitions (order_id, created_at);
CREATE INDEX order_state_transitions_payment_event_id ON order_state_transitions (payment_event_id);

-- A voice order above the customer's approval threshold.
CREATE TABLE order_approvals (
    order_id        uuid PRIMARY KEY REFERENCES orders (id) ON DELETE CASCADE,
    threshold_cents bigint NOT NULL CHECK (threshold_cents >= 0),
    total_cents     bigint NOT NULL,
    status          text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'APPROVED', 'DECLINED', 'EXPIRED')),
    expires_at      timestamptz NOT NULL,
    decided_at      timestamptz,
    -- Passkey step-up evidence from the approving token.
    decided_amr     text[],
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT order_approvals_over_threshold CHECK (total_cents > threshold_cents),
    CONSTRAINT order_approvals_decided CHECK ((status = 'PENDING') = (decided_at IS NULL)),
    CONSTRAINT order_approvals_passkey CHECK (
        status <> 'APPROVED' OR (decided_amr IS NOT NULL AND decided_amr && ARRAY['hwk']::text[])
    )
);
-- Expiry sweep.
CREATE INDEX order_approvals_pending_expires_at ON order_approvals (expires_at) WHERE status = 'PENDING';

-- ---------------------------------------------------------------------------
-- Payments
-- ---------------------------------------------------------------------------

-- One manual-capture payment per order. A wallet, if added later, becomes another provider.
CREATE TABLE payments (
    id                  uuid PRIMARY KEY,
    order_id            uuid NOT NULL UNIQUE REFERENCES orders (id) ON DELETE RESTRICT,
    provider            text NOT NULL CHECK (provider IN ('STRIPE')),
    -- NULL only until the provider has returned the payment (or if creating it failed).
    provider_payment_id text UNIQUE CHECK (length(provider_payment_id) > 0),
    -- The order id, sent as the provider idempotency key.
    idempotency_key     text NOT NULL UNIQUE CHECK (length(idempotency_key) > 0),
    status              text NOT NULL DEFAULT 'AUTHORIZING' CHECK (status IN (
        'AUTHORIZING', 'AUTHORIZED', 'CAPTURED', 'VOIDED', 'FAILED', 'PARTIALLY_REFUNDED', 'REFUNDED'
    )),
    amount_cents        bigint NOT NULL CHECK (amount_cents > 0),
    currency            text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    captured_at         timestamptz,
    canceled_at         timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT payments_provider_payment_known CHECK (
        provider_payment_id IS NOT NULL OR status IN ('AUTHORIZING', 'FAILED')
    ),
    CONSTRAINT payments_captured_at CHECK (
        status NOT IN ('CAPTURED', 'PARTIALLY_REFUNDED', 'REFUNDED') OR captured_at IS NOT NULL
    ),
    CONSTRAINT payments_canceled_at CHECK (status <> 'VOIDED' OR canceled_at IS NOT NULL),
    CONSTRAINT payments_captured_or_canceled CHECK (captured_at IS NULL OR canceled_at IS NULL)
);

-- ---------------------------------------------------------------------------
-- Outbox and inbox (docs/DATABASE.md, "Outbox and inbox")
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
