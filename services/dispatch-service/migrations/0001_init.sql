-- dispatch-service: missions (one per paid order), their assignment attempts,
-- the mission event history, and the outbox/inbox. See docs/DATABASE.md.
-- Edge zones and drones are static config and live telemetry, not tables.

-- One drone mission per commerce order, created from dispatch.request.
CREATE TABLE missions (
    order_id            uuid PRIMARY KEY,
    -- DispatchRequest.request_id: a redelivered request cannot create a second mission.
    dispatch_request_id uuid NOT NULL UNIQUE,
    state               text NOT NULL DEFAULT 'REQUESTED'
                        CHECK (state IN ('REQUESTED', 'ASSIGNED', 'AT_PICKUP', 'VISUAL_LOCK', 'PICKED_UP',
                                         'DELIVERED', 'PICKUP_FAILED', 'ABORTED', 'NO_DRONE', 'RECALLED')),
    -- NULL until an edge zone accepts the mission and picks a drone.
    zone                text CHECK (length(zone) > 0),
    drone_id            text CHECK (length(drone_id) > 0),

    -- Copied from dispatch.request.
    pickup_lat          double precision NOT NULL CHECK (pickup_lat BETWEEN -90 AND 90),
    pickup_lon          double precision NOT NULL CHECK (pickup_lon BETWEEN -180 AND 180),
    pickup_point_id     uuid NOT NULL,
    asset_key           text NOT NULL CHECK (length(asset_key) > 0),
    dropoff_lat         double precision NOT NULL CHECK (dropoff_lat BETWEEN -90 AND 90),
    dropoff_lon         double precision NOT NULL CHECK (dropoff_lon BETWEEN -180 AND 180),
    payload_g           integer NOT NULL CHECK (payload_g > 0 AND payload_g <= 2500),

    attempts            integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    eta_seconds         integer CHECK (eta_seconds >= 0),
    requested_at        timestamptz NOT NULL DEFAULT now(),
    assigned_at         timestamptz,
    completed_at        timestamptz,
    version             integer NOT NULL DEFAULT 0,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),

    -- From ASSIGNED through DELIVERED a drone is flying the mission.
    CONSTRAINT missions_flying_has_drone CHECK (
        state NOT IN ('ASSIGNED', 'AT_PICKUP', 'VISUAL_LOCK', 'PICKED_UP', 'DELIVERED')
        OR (zone IS NOT NULL AND drone_id IS NOT NULL)
    )
);
COMMENT ON COLUMN missions.order_id IS 'ref: commerce.orders.id';
COMMENT ON COLUMN missions.dispatch_request_id IS 'ref: commerce.outbox.id';
COMMENT ON COLUMN missions.pickup_point_id IS 'ref: merchant.pickup_points.id';
COMMENT ON COLUMN missions.asset_key IS 'Key of the verified marker photo in the PICKUP_ASSETS object store';

-- Missions still waiting for a drone, oldest first (retries and the 10-minute NO_DRONE timeout).
CREATE INDEX missions_waiting ON missions (requested_at) WHERE state = 'REQUESTED';
CREATE INDEX missions_pickup_point_id ON missions (pickup_point_id);
-- The mission a drone is currently flying (telemetry, handoff). Not unique: events for
-- different orders may be applied out of order, briefly showing a drone on two missions.
CREATE INDEX missions_active_drone ON missions (drone_id)
    WHERE state IN ('ASSIGNED', 'AT_PICKUP', 'VISUAL_LOCK', 'PICKED_UP');

-- Each try to get a drone for a mission, across zones.
CREATE TABLE mission_attempts (
    id         uuid PRIMARY KEY,
    order_id   uuid NOT NULL REFERENCES missions (order_id) ON DELETE CASCADE,
    attempt    integer NOT NULL CHECK (attempt > 0),
    zone       text NOT NULL CHECK (length(zone) > 0),
    result     text NOT NULL CHECK (result IN ('ASSIGNED', 'NO_FREE_DRONE', 'ZONE_UNREACHABLE', 'TIMED_OUT')),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (order_id, attempt)   -- also serves as the order_id foreign key index
);

-- History of MissionEvents received from edge nodes (and NO_DRONE published by dispatch).
CREATE TABLE mission_events (
    -- MissionEvent.event_id: a replayed event conflicts.
    event_id    uuid PRIMARY KEY,
    order_id    uuid NOT NULL REFERENCES missions (order_id) ON DELETE CASCADE,
    kind        text NOT NULL
                CHECK (kind IN ('ASSIGNED', 'AT_PICKUP', 'VISUAL_LOCK', 'PICKUP_FAILED', 'PICKED_UP',
                                'DELIVERED', 'ABORTED', 'NO_DRONE')),
    -- NULL when the event has none, e.g. NO_DRONE.
    zone        text CHECK (length(zone) > 0),
    drone_id    text CHECK (length(drone_id) > 0),
    lat         double precision CHECK (lat BETWEEN -90 AND 90),
    lon         double precision CHECK (lon BETWEEN -180 AND 180),
    detail      text CHECK (length(detail) > 0),
    occurred_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT mission_events_position_pair CHECK ((lat IS NULL) = (lon IS NULL))
);
CREATE INDEX mission_events_order_occurred ON mission_events (order_id, occurred_at);

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
