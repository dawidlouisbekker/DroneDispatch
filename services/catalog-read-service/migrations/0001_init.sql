-- catalog-read-service schema (databases `catalog_read_<zone>`, one per edge zone):
-- read-only projections of merchant-service's catalog state events, keyed by the
-- Amazon Location place ID each business claimed during onboarding.
--
-- Read-model rules (docs/DATABASE.md, "CQRS"): one table per query shape; no foreign
-- keys, because events for different subjects may arrive in any order; and every row
-- keeps the MERCHANT_EVENTS sequence it was projected from, so an upsert applies only
-- when that sequence is newer and stale or replayed messages change nothing.

CREATE TABLE catalogs (
    place_id          text PRIMARY KEY CHECK (length(place_id) > 0),
    business_id       uuid NOT NULL UNIQUE,
    -- What Alexa+ reads out.
    spoken_name       text NOT NULL CHECK (length(spoken_name) > 0),
    address           text NOT NULL CHECK (length(address) > 0),
    lat               double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
    lon               double precision NOT NULL CHECK (lon BETWEEN -180 AND 180),
    categories        text[] NOT NULL DEFAULT '{}',
    accepting_orders  boolean NOT NULL,
    prep_time_minutes integer NOT NULL CHECK (prep_time_minutes > 0),
    -- MERCHANT_EVENTS sequence of the state message this row was projected from.
    source_seq        bigint NOT NULL CHECK (source_seq > 0),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
COMMENT ON COLUMN catalogs.place_id IS 'ref: merchant.businesses.place_id';
COMMENT ON COLUMN catalogs.business_id IS 'ref: merchant.businesses.id';
-- Radius search: filter on an indexed bounding box, then check distance.
CREATE INDEX catalogs_lat_lon ON catalogs (lat, lon);
CREATE INDEX catalogs_categories ON catalogs USING gin (categories);

CREATE TABLE catalog_sections (
    section_id uuid PRIMARY KEY,
    place_id   text NOT NULL CHECK (length(place_id) > 0),
    name       text NOT NULL CHECK (length(name) > 0),
    position   integer NOT NULL CHECK (position >= 0),
    source_seq bigint NOT NULL CHECK (source_seq > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
COMMENT ON COLUMN catalog_sections.section_id IS 'ref: merchant.menu_sections.id';
CREATE INDEX catalog_sections_place_position ON catalog_sections (place_id, position);

CREATE TABLE catalog_items (
    item_id         uuid PRIMARY KEY,
    place_id        text NOT NULL CHECK (length(place_id) > 0),
    section_id      uuid NOT NULL,
    name            text NOT NULL CHECK (length(name) > 0),
    -- What Alexa+ reads out.
    spoken_name     text NOT NULL CHECK (length(spoken_name) > 0),
    description     text NOT NULL DEFAULT '',
    price_cents     bigint NOT NULL CHECK (price_cents >= 0),
    currency        text NOT NULL DEFAULT 'usd' CHECK (currency ~ '^[a-z]{3}$'),
    weight_g        integer NOT NULL CHECK (weight_g > 0),
    available       boolean NOT NULL,
    stock_remaining integer CHECK (stock_remaining >= 0),
    source_seq      bigint NOT NULL CHECK (source_seq > 0),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);
COMMENT ON COLUMN catalog_items.item_id IS 'ref: merchant.menu_items.id';
COMMENT ON COLUMN catalog_items.stock_remaining IS 'NULL means unlimited';
-- Menu reads: a catalog's items by section.
CREATE INDEX catalog_items_place_section ON catalog_items (place_id, section_id);

-- How far each durable consumer has projected MERCHANT_EVENTS.
CREATE TABLE projection_checkpoints (
    consumer   text PRIMARY KEY CHECK (length(consumer) > 0),
    stream_seq bigint NOT NULL CHECK (stream_seq >= 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE inbox (
    consumer    text NOT NULL,              -- durable consumer name
    message_id  text NOT NULL,              -- Nats-Msg-Id of the received message
    received_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (consumer, message_id)
);
