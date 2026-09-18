-- dispatch-service schema (databases `dispatch_<zone>`, one per edge zone): the zone's
-- drone fleet — docks, drones, missions and station handshakes — plus the outbox and
-- inbox. See docs/DATABASE.md. This set replaces the v1 cloud `dispatch` schema, which
-- lived in a different database.

-- Charging and launch docks, from the zone's fleet config.
CREATE TABLE docks (
    id         text PRIMARY KEY CHECK (length(id) > 0),   -- e.g. 'hub-fremont'
    lat        double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
    lon        double precision NOT NULL CHECK (lon BETWEEN -180 AND 180),
    capacity   integer NOT NULL CHECK (capacity > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE drones (
    id                 text PRIMARY KEY CHECK (length(id) > 0),   -- e.g. 'drone-07'
    dock_id            text NOT NULL REFERENCES docks (id) ON DELETE RESTRICT,
    model              text NOT NULL CHECK (length(model) > 0),
    max_payload_g      integer NOT NULL CHECK (max_payload_g BETWEEN 1 AND 2500),
    status             text NOT NULL DEFAULT 'IDLE'
                       CHECK (status IN ('IDLE', 'ASSIGNED', 'FLYING', 'CHARGING', 'OUT_OF_SERVICE')),
    battery_pct        real CHECK (battery_pct BETWEEN 0 AND 100),
    last_lat           double precision CHECK (last_lat BETWEEN -90 AND 90),
    last_lon           double precision CHECK (last_lon BETWEEN -180 AND 180),
    last_seen_at       timestamptz,
    -- SHA-256 of the drone's TLS client certificate, issued by the zone fleet CA.
    certificate_sha256 text NOT NULL UNIQUE CHECK (certificate_sha256 ~ '^[0-9a-f]{64}$'),
    version            integer NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT drones_position_pair CHECK ((last_lat IS NULL) = (last_lon IS NULL))
);
CREATE INDEX drones_dock_id ON drones (dock_id);
-- Free drones, for assignment.
CREATE INDEX drones_idle ON drones (dock_id) WHERE status = 'IDLE';

-- One mission per order, created from dispatch.<zone>.request. Each leg keeps what the drone
-- needs to find and authenticate its station, copied from the request.
CREATE TABLE missions (
    order_id                  uuid PRIMARY KEY,
    -- DispatchRequest.request_id: a redelivered request cannot create a second mission.
    dispatch_request_id       uuid NOT NULL UNIQUE,
    drone_id                  text REFERENCES drones (id) ON DELETE RESTRICT,
    state                     text NOT NULL DEFAULT 'REQUESTED'
                              CHECK (state IN ('REQUESTED', 'ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'AT_DROPOFF', 'DELIVERED',
                                               'STATION_FAILED', 'ABORTED', 'NO_DRONE', 'RECALLED')),
    payload_g                 integer NOT NULL CHECK (payload_g > 0 AND payload_g <= 2500),

    -- Pickup leg: the business station.
    pickup_station_id         uuid NOT NULL,
    pickup_lat                double precision NOT NULL CHECK (pickup_lat BETWEEN -90 AND 90),
    pickup_lon                double precision NOT NULL CHECK (pickup_lon BETWEEN -180 AND 180),
    pickup_accuracy_m         real NOT NULL CHECK (pickup_accuracy_m > 0),
    pickup_public_key_sha256  text NOT NULL CHECK (pickup_public_key_sha256 ~ '^[0-9a-f]{64}$'),
    -- dronedrop.station.v1.AccessNetwork list, in the order the drone tries them.
    pickup_access_networks    jsonb NOT NULL CHECK (
        CASE WHEN jsonb_typeof(pickup_access_networks) = 'array' THEN jsonb_array_length(pickup_access_networks) > 0 ELSE false END
    ),

    -- Drop-off leg: the customer's station.
    dropoff_station_id        uuid NOT NULL,
    dropoff_lat               double precision NOT NULL CHECK (dropoff_lat BETWEEN -90 AND 90),
    dropoff_lon               double precision NOT NULL CHECK (dropoff_lon BETWEEN -180 AND 180),
    dropoff_accuracy_m        real NOT NULL CHECK (dropoff_accuracy_m > 0),
    dropoff_public_key_sha256 text NOT NULL CHECK (dropoff_public_key_sha256 ~ '^[0-9a-f]{64}$'),
    dropoff_access_networks   jsonb NOT NULL CHECK (
        CASE WHEN jsonb_typeof(dropoff_access_networks) = 'array' THEN jsonb_array_length(dropoff_access_networks) > 0 ELSE false END
    ),

    attempts                  integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    eta_seconds               integer CHECK (eta_seconds >= 0),
    requested_at              timestamptz NOT NULL DEFAULT now(),
    assigned_at               timestamptz,
    completed_at              timestamptz,
    version                   integer NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at                timestamptz NOT NULL DEFAULT now(),
    updated_at                timestamptz NOT NULL DEFAULT now(),

    -- From ASSIGNED until delivery a drone is flying the mission.
    CONSTRAINT missions_flying_has_drone CHECK (
        state NOT IN ('ASSIGNED', 'AT_PICKUP', 'PICKED_UP', 'AT_DROPOFF', 'DELIVERED') OR drone_id IS NOT NULL
    )
);
COMMENT ON COLUMN missions.order_id IS 'ref: user_service.orders.id';
COMMENT ON COLUMN missions.dispatch_request_id IS 'ref: user_service.outbox.id';
COMMENT ON COLUMN missions.pickup_station_id IS 'ref: merchant.stations.id';
COMMENT ON COLUMN missions.dropoff_station_id IS 'ref: user_service.stations.id';
-- Missions still waiting for a drone, oldest first (retries and the 10-minute NO_DRONE timeout).
CREATE INDEX missions_waiting ON missions (requested_at) WHERE state = 'REQUESTED';
CREATE INDEX missions_drone_id ON missions (drone_id);

-- Every attempt to authenticate a station.
CREATE TABLE station_handshakes (
    id             uuid PRIMARY KEY,
    order_id       uuid NOT NULL REFERENCES missions (order_id) ON DELETE CASCADE,
    drone_id       text NOT NULL REFERENCES drones (id) ON DELETE RESTRICT,
    leg            text NOT NULL CHECK (leg IN ('PICKUP', 'DROPOFF')),
    station_id     uuid NOT NULL,
    -- dronedrop.station.v1.AccessNetwork case used. Widen with the missions' networks.
    access_network text NOT NULL CHECK (access_network IN ('BLUETOOTH_LE')),
    ranging_method text NOT NULL CHECK (ranging_method IN ('GNSS', 'RSSI', 'CHANNEL_SOUNDING')),
    distance_m     real CHECK (distance_m >= 0),
    result         text NOT NULL
                   CHECK (result IN ('VERIFIED', 'NOT_FOUND', 'KEY_MISMATCH', 'TLS_FAILED', 'REJECTED', 'TIMEOUT')),
    detail         text CHECK (length(detail) > 0),
    attempted_at   timestamptz NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX station_handshakes_order_attempted ON station_handshakes (order_id, attempted_at);
CREATE INDEX station_handshakes_drone_id ON station_handshakes (drone_id);

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
