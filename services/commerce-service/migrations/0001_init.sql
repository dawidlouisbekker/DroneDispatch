-- commerce database: customers, delivery locations, merchant Stripe accounts,
-- quotes, orders, payments, refunds, Stripe events, outbox and inbox.
-- Conventions and cross-service references: docs/DATABASE.md.
--
-- Deletes: only owned children cascade (quote_items, order_items,
-- order_state_transitions). Everything else is RESTRICT, so money and order
-- rows can never disappear with a customer or a location.

-- ---------------------------------------------------------------------------
-- Customers
-- ---------------------------------------------------------------------------

CREATE TABLE customers (
    sub                       uuid PRIMARY KEY,
    stripe_customer_id        text UNIQUE CHECK (length(stripe_customer_id) > 0),
    default_payment_method_id text CHECK (length(default_payment_method_id) > 0),
    -- Voice-order spending caps. Defaults: $50 per order, $100 per day.
    per_order_cap_cents       bigint NOT NULL DEFAULT 5000 CHECK (per_order_cap_cents >= 0),
    daily_cap_cents           bigint NOT NULL DEFAULT 10000 CHECK (daily_cap_cents >= 0),
    currency                  text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    created_at                timestamptz NOT NULL DEFAULT now(),
    updated_at                timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT customers_caps_ordered CHECK (per_order_cap_cents <= daily_cap_cents),
    -- A Stripe payment method is attached to a Stripe customer.
    CONSTRAINT customers_payment_method_needs_stripe_customer
        CHECK (default_payment_method_id IS NULL OR stripe_customer_id IS NOT NULL)
);
COMMENT ON COLUMN customers.sub IS 'ref: auth.users.id';

CREATE TABLE delivery_locations (
    id           uuid PRIMARY KEY,
    customer_sub uuid NOT NULL REFERENCES customers (sub) ON DELETE RESTRICT,
    label        text NOT NULL CHECK (length(label) > 0),
    address      text NOT NULL CHECK (length(address) > 0),
    lat          double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
    lon          double precision NOT NULL CHECK (lon BETWEEN -180 AND 180),
    status       text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'VERIFIED', 'REVOKED')),
    -- MFA evidence from the step-up token (VerifyDeliveryLocation).
    verified_at  timestamptz,
    verified_amr text[],
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    -- VERIFIED needs the evidence, and the amr must include an MFA method
    -- ("otp" or "hwk"). The IS NOT NULL tests keep NULL from passing the CHECK.
    CONSTRAINT delivery_locations_verified_evidence CHECK (
        status <> 'VERIFIED'
        OR (verified_at IS NOT NULL
            AND verified_amr IS NOT NULL
            AND verified_amr && ARRAY['otp', 'hwk']::text[])
    ),
    -- PENDING has never been verified. REVOKED keeps whatever evidence it had.
    CONSTRAINT delivery_locations_pending_unverified CHECK (
        status <> 'PENDING' OR (verified_at IS NULL AND verified_amr IS NULL)
    ),
    -- Target of the composite foreign keys that keep quotes and orders on the
    -- customer's own locations.
    CONSTRAINT delivery_locations_id_customer UNIQUE (id, customer_sub)
);
CREATE INDEX delivery_locations_customer_sub ON delivery_locations (customer_sub);
-- Labels ("home", "office") are spoken, so compare them case-insensitively.
CREATE UNIQUE INDEX delivery_locations_active_label
    ON delivery_locations (customer_sub, lower(label))
    WHERE status <> 'REVOKED';

-- ---------------------------------------------------------------------------
-- Merchant Stripe connected accounts
-- ---------------------------------------------------------------------------

CREATE TABLE merchant_accounts (
    business_id       uuid PRIMARY KEY,
    stripe_account_id text NOT NULL UNIQUE CHECK (length(stripe_account_id) > 0),
    charges_enabled   boolean NOT NULL DEFAULT false,
    payouts_enabled   boolean NOT NULL DEFAULT false,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
COMMENT ON COLUMN merchant_accounts.business_id IS 'ref: merchant.businesses.id';

-- ---------------------------------------------------------------------------
-- Quotes (cart snapshots, valid 10 minutes)
-- ---------------------------------------------------------------------------

CREATE TABLE quotes (
    id                   uuid PRIMARY KEY,
    customer_sub         uuid NOT NULL REFERENCES customers (sub) ON DELETE RESTRICT,
    business_id          uuid NOT NULL,
    business_name        text NOT NULL CHECK (length(business_name) > 0),
    delivery_location_id uuid NOT NULL,
    subtotal_cents       bigint NOT NULL CHECK (subtotal_cents >= 0),
    delivery_fee_cents   bigint NOT NULL CHECK (delivery_fee_cents >= 0),
    total_cents          bigint NOT NULL CHECK (total_cents >= 0),
    currency             text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    payload_g            integer NOT NULL CHECK (payload_g > 0 AND payload_g <= 2500),
    eta_seconds          integer NOT NULL CHECK (eta_seconds >= 0),
    expires_at           timestamptz NOT NULL,
    -- Set when a newer UpsertCart replaces this quote. Its only change.
    superseded_at        timestamptz,
    created_at           timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT quotes_total CHECK (total_cents = subtotal_cents + delivery_fee_cents),
    CONSTRAINT quotes_expiry CHECK (expires_at > created_at),
    CONSTRAINT quotes_own_location FOREIGN KEY (delivery_location_id, customer_sub)
        REFERENCES delivery_locations (id, customer_sub) ON DELETE RESTRICT,
    -- Target of orders_own_quote.
    CONSTRAINT quotes_id_customer UNIQUE (id, customer_sub)
);
COMMENT ON COLUMN quotes.business_id IS 'ref: merchant.businesses.id';
CREATE INDEX quotes_customer_created ON quotes (customer_sub, created_at);
CREATE INDEX quotes_business_id ON quotes (business_id);
CREATE INDEX quotes_delivery_location_id ON quotes (delivery_location_id);

CREATE TABLE quote_items (
    quote_id         uuid NOT NULL REFERENCES quotes (id) ON DELETE CASCADE,
    item_id          uuid NOT NULL,
    -- Copied from merchant-service at quote time. Currency is the quote's.
    name             text NOT NULL CHECK (length(name) > 0),
    unit_price_cents bigint NOT NULL CHECK (unit_price_cents >= 0),
    qty              integer NOT NULL CHECK (qty > 0),
    weight_g         integer NOT NULL CHECK (weight_g > 0),
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (quote_id, item_id)
);
COMMENT ON COLUMN quote_items.item_id IS 'ref: merchant.menu_items.id';
CREATE INDEX quote_items_item_id ON quote_items (item_id);

-- ---------------------------------------------------------------------------
-- Orders
-- ---------------------------------------------------------------------------

CREATE TABLE orders (
    id                   uuid PRIMARY KEY,
    customer_sub         uuid NOT NULL REFERENCES customers (sub) ON DELETE RESTRICT,
    -- PlaceOrder is idempotent on quote_id.
    quote_id             uuid NOT NULL UNIQUE,
    business_id          uuid NOT NULL,
    business_name        text NOT NULL CHECK (length(business_name) > 0),
    -- dronedrop.commerce.v1.OrderState without the prefix. Only a verified
    -- Stripe webhook may set PAID (see order_state_transitions).
    state                text NOT NULL CHECK (state IN (
        'CART', 'QUOTED', 'AUTHORIZING', 'AWAITING_MERCHANT', 'CAPTURING', 'PAID',
        'DISPATCH_REQUESTED', 'DRONE_ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'DELIVERED',
        'COMPLETED', 'PAYMENT_FAILED', 'CANCELLED', 'REFUNDED'
    )),
    -- Drop-off, copied at order time.
    delivery_location_id uuid NOT NULL,
    dropoff_label        text NOT NULL CHECK (length(dropoff_label) > 0),
    dropoff_address      text NOT NULL CHECK (length(dropoff_address) > 0),
    dropoff_lat          double precision NOT NULL CHECK (dropoff_lat BETWEEN -90 AND 90),
    dropoff_lon          double precision NOT NULL CHECK (dropoff_lon BETWEEN -180 AND 180),
    -- Pickup, copied at order time.
    pickup_point_id      uuid NOT NULL,
    pickup_lat           double precision NOT NULL CHECK (pickup_lat BETWEEN -90 AND 90),
    pickup_lon           double precision NOT NULL CHECK (pickup_lon BETWEEN -180 AND 180),
    -- Amounts. platform_fee_cents is the commission taken from the merchant's
    -- share; Stripe's application fee is delivery fee + platform fee.
    subtotal_cents       bigint NOT NULL CHECK (subtotal_cents >= 0),
    delivery_fee_cents   bigint NOT NULL CHECK (delivery_fee_cents >= 0),
    platform_fee_cents   bigint NOT NULL DEFAULT 0 CHECK (platform_fee_cents >= 0),
    total_cents          bigint NOT NULL CHECK (total_cents > 0),
    currency             text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    payload_g            integer NOT NULL CHECK (payload_g > 0 AND payload_g <= 2500),
    eta_seconds          integer NOT NULL CHECK (eta_seconds >= 0),
    -- Set when the order starts waiting for the merchant (5-minute window).
    merchant_accept_by   timestamptz,
    version              integer NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at           timestamptz NOT NULL DEFAULT now(),
    updated_at           timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT orders_total CHECK (total_cents = subtotal_cents + delivery_fee_cents),
    CONSTRAINT orders_platform_fee CHECK (platform_fee_cents <= subtotal_cents),
    CONSTRAINT orders_accept_by CHECK (state <> 'AWAITING_MERCHANT' OR merchant_accept_by IS NOT NULL),
    -- The quote and the location must both belong to the ordering customer.
    CONSTRAINT orders_own_quote FOREIGN KEY (quote_id, customer_sub)
        REFERENCES quotes (id, customer_sub) ON DELETE RESTRICT,
    CONSTRAINT orders_own_location FOREIGN KEY (delivery_location_id, customer_sub)
        REFERENCES delivery_locations (id, customer_sub) ON DELETE RESTRICT
);
COMMENT ON COLUMN orders.business_id IS 'ref: merchant.businesses.id';
COMMENT ON COLUMN orders.pickup_point_id IS 'ref: merchant.pickup_points.id';
-- Order history (newest first) and the daily spending cap.
CREATE INDEX orders_customer_created ON orders (customer_sub, created_at);
-- Merchant-timeout sweep.
CREATE INDEX orders_state_accept_by ON orders (state, merchant_accept_by);
CREATE INDEX orders_business_id ON orders (business_id);
CREATE INDEX orders_delivery_location_id ON orders (delivery_location_id);
CREATE INDEX orders_pickup_point_id ON orders (pickup_point_id);

CREATE TABLE order_items (
    order_id         uuid NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    item_id          uuid NOT NULL,
    -- Copied at order time. Currency is the order's.
    name             text NOT NULL CHECK (length(name) > 0),
    unit_price_cents bigint NOT NULL CHECK (unit_price_cents >= 0),
    qty              integer NOT NULL CHECK (qty > 0),
    weight_g         integer NOT NULL CHECK (weight_g > 0),
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (order_id, item_id)
);
COMMENT ON COLUMN order_items.item_id IS 'ref: merchant.menu_items.id';
CREATE INDEX order_items_item_id ON order_items (item_id);

-- ---------------------------------------------------------------------------
-- Stripe
-- ---------------------------------------------------------------------------

-- Received webhook events. The primary key makes a duplicate delivery conflict.
CREATE TABLE stripe_events (
    id           text PRIMARY KEY CHECK (starts_with(id, 'evt_')),
    type         text NOT NULL CHECK (length(type) > 0),
    payload      jsonb NOT NULL,
    received_at  timestamptz NOT NULL DEFAULT now(),
    processed_at timestamptz
);
CREATE INDEX stripe_events_unprocessed ON stripe_events (received_at) WHERE processed_at IS NULL;

CREATE TABLE order_state_transitions (
    id              uuid PRIMARY KEY,
    order_id        uuid NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    -- NULL for the order's first state.
    from_state      text CHECK (from_state IN (
        'CART', 'QUOTED', 'AUTHORIZING', 'AWAITING_MERCHANT', 'CAPTURING', 'PAID',
        'DISPATCH_REQUESTED', 'DRONE_ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'DELIVERED',
        'COMPLETED', 'PAYMENT_FAILED', 'CANCELLED', 'REFUNDED'
    )),
    to_state        text NOT NULL CHECK (to_state IN (
        'CART', 'QUOTED', 'AUTHORIZING', 'AWAITING_MERCHANT', 'CAPTURING', 'PAID',
        'DISPATCH_REQUESTED', 'DRONE_ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'DELIVERED',
        'COMPLETED', 'PAYMENT_FAILED', 'CANCELLED', 'REFUNDED'
    )),
    -- e.g. "merchant rejected: out of oat milk".
    reason          text CHECK (length(reason) > 0),
    cause           text NOT NULL CHECK (cause IN ('GRPC', 'STRIPE_WEBHOOK', 'TIMER', 'EVENT')),
    stripe_event_id text REFERENCES stripe_events (id) ON DELETE RESTRICT,
    created_at      timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT order_state_transitions_changes_state CHECK (from_state IS DISTINCT FROM to_state),
    CONSTRAINT order_state_transitions_webhook_event
        CHECK ((cause = 'STRIPE_WEBHOOK') = (stripe_event_id IS NOT NULL)),
    -- Only a verified (stored) Stripe webhook can move an order to PAID.
    CONSTRAINT order_state_transitions_paid_by_webhook
        CHECK (to_state <> 'PAID' OR cause = 'STRIPE_WEBHOOK')
);
CREATE INDEX order_state_transitions_order_created ON order_state_transitions (order_id, created_at);
CREATE INDEX order_state_transitions_stripe_event_id ON order_state_transitions (stripe_event_id);

-- One manual-capture PaymentIntent per order.
CREATE TABLE payments (
    id                       uuid PRIMARY KEY,
    order_id                 uuid NOT NULL UNIQUE REFERENCES orders (id) ON DELETE RESTRICT,
    -- NULL only until Stripe has returned the PaymentIntent (or if creating it failed).
    stripe_payment_intent_id text UNIQUE CHECK (length(stripe_payment_intent_id) > 0),
    -- The quote id, sent as the Stripe Idempotency-Key.
    idempotency_key          text NOT NULL UNIQUE CHECK (length(idempotency_key) > 0),
    status                   text NOT NULL DEFAULT 'AUTHORIZING' CHECK (status IN (
        'AUTHORIZING', 'AUTHORIZED', 'CAPTURED', 'VOIDED', 'FAILED', 'PARTIALLY_REFUNDED', 'REFUNDED'
    )),
    amount_cents             bigint NOT NULL CHECK (amount_cents > 0),
    application_fee_cents    bigint NOT NULL DEFAULT 0 CHECK (application_fee_cents >= 0),
    currency                 text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    captured_at              timestamptz,
    canceled_at              timestamptz,
    created_at               timestamptz NOT NULL DEFAULT now(),
    updated_at               timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT payments_application_fee CHECK (application_fee_cents <= amount_cents),
    CONSTRAINT payments_intent_known CHECK (
        stripe_payment_intent_id IS NOT NULL OR status IN ('AUTHORIZING', 'FAILED')
    ),
    CONSTRAINT payments_captured_at CHECK (
        status NOT IN ('CAPTURED', 'PARTIALLY_REFUNDED', 'REFUNDED') OR captured_at IS NOT NULL
    ),
    CONSTRAINT payments_canceled_at CHECK (status <> 'VOIDED' OR canceled_at IS NOT NULL),
    CONSTRAINT payments_captured_or_canceled CHECK (captured_at IS NULL OR canceled_at IS NULL)
);

CREATE TABLE refunds (
    id               uuid PRIMARY KEY,
    payment_id       uuid NOT NULL REFERENCES payments (id) ON DELETE RESTRICT,
    -- NULL only until Stripe has returned the refund (or if creating it failed).
    stripe_refund_id text UNIQUE CHECK (length(stripe_refund_id) > 0),
    amount_cents     bigint NOT NULL CHECK (amount_cents > 0),
    currency         text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    status           text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'SUCCEEDED', 'FAILED')),
    -- e.g. "no_drone", "pickup_failed", "mission_aborted", "customer_cancelled".
    reason           text NOT NULL CHECK (length(reason) > 0),
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT refunds_refund_known CHECK (stripe_refund_id IS NOT NULL OR status IN ('PENDING', 'FAILED'))
);
CREATE INDEX refunds_payment_id ON refunds (payment_id);

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
