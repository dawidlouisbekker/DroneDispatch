-- auth-service schema (database `auth`). Conventions: docs/DATABASE.md.
--
-- Secrets are never stored raw. Passwords are argon2id PHC strings. Session ids,
-- CSRF tokens, client secrets, request ids, codes, refresh tokens and recovery
-- codes are SHA-256 base64url hashes. The TOTP seed is AES-GCM encrypted.
-- Access tokens are ES256 JWTs and are not stored at all: every token of one
-- authorization carries the grant id (`gid`), and revoking the grant revokes them.

-- Accounts. `id` is the `sub` claim and the user key in every other service.
CREATE TABLE users (
    id                uuid PRIMARY KEY,
    email             text NOT NULL CHECK (email LIKE '_%@_%'),
    password_hash     text NOT NULL CHECK (password_hash LIKE '$argon2id$%'),
    email_verified_at timestamptz,
    disabled_at       timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email_lower ON users (lower(email));

-- Browser login sessions on the auth host. `auth_time` and `amr` are what the
-- next authorization code issued from this session will carry; step-up updates them.
CREATE TABLE sessions (
    id_hash         text PRIMARY KEY CHECK (length(id_hash) > 0),
    user_id         uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    csrf_token_hash text NOT NULL CHECK (length(csrf_token_hash) > 0),
    auth_time       timestamptz NOT NULL,
    amr             text[] NOT NULL CHECK (cardinality(amr) > 0),
    expires_at      timestamptz NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_id ON sessions (user_id);
CREATE INDEX sessions_expires_at ON sessions (expires_at);

-- Static clients (created from the CLI: Alexa, map-web, merchant-web) and
-- Dynamic Client Registration clients. CIMD clients are resolved from their
-- metadata URL and cached in memory, so they have no row.
-- Public clients (`none`) have no secret; confidential clients must have one.
CREATE TABLE clients (
    client_id     text PRIMARY KEY CHECK (length(client_id) > 0),
    secret_hash   text CHECK (length(secret_hash) > 0),
    name          text NOT NULL CHECK (length(name) > 0),
    redirect_uris text[] NOT NULL CHECK (
        cardinality(redirect_uris) > 0 AND array_position(redirect_uris, NULL) IS NULL
    ),
    auth_method   text NOT NULL CHECK (
        auth_method IN ('none', 'client_secret_basic', 'client_secret_post')
    ),
    kind          text NOT NULL CHECK (kind IN ('STATIC', 'DCR')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT clients_secret_matches_auth_method
        CHECK ((auth_method = 'none') = (secret_hash IS NULL))
);

-- A validated /authorize request parked while the user logs in and consents.
CREATE TABLE auth_requests (
    id_hash    text PRIMARY KEY CHECK (length(id_hash) > 0),
    params     jsonb NOT NULL CHECK (jsonb_typeof(params) = 'object'),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX auth_requests_expires_at ON auth_requests (expires_at);

-- One row per authorization. `id` is the `gid` claim. Deleting the user removes
-- the grant; a client cannot be deleted while it still has grants, so the
-- application revokes them (and publishes auth.events.grant_revoked) first.
CREATE TABLE grants (
    id         uuid PRIMARY KEY,
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    client_id  text NOT NULL REFERENCES clients (client_id) ON DELETE RESTRICT,
    resource   text NOT NULL CHECK (length(resource) > 0),
    scope      text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);
CREATE INDEX grants_user_id ON grants (user_id);
CREATE INDEX grants_client_id ON grants (client_id);
-- GrantRegistry.ListRevokedGrants(revoked_since).
CREATE INDEX grants_revoked_at ON grants (revoked_at) WHERE revoked_at IS NOT NULL;

-- Authorization codes. PKCE is S256 only, so the challenge is always the
-- 43-character base64url SHA-256 of the verifier (RFC 7636).
CREATE TABLE auth_codes (
    code_hash             text PRIMARY KEY CHECK (length(code_hash) > 0),
    grant_id              uuid NOT NULL REFERENCES grants (id) ON DELETE CASCADE,
    redirect_uri          text NOT NULL CHECK (length(redirect_uri) > 0),
    redirect_uri_provided boolean NOT NULL,
    scope                 text NOT NULL,
    resource              text NOT NULL CHECK (length(resource) > 0),
    code_challenge        text NOT NULL CHECK (code_challenge ~ '^[A-Za-z0-9_-]{43}$'),
    acr                   text NOT NULL CHECK (length(acr) > 0),
    amr                   text[] NOT NULL CHECK (cardinality(amr) > 0),
    auth_time             timestamptz NOT NULL,
    expires_at            timestamptz NOT NULL,
    consumed_at           timestamptz,
    created_at            timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX auth_codes_grant_id ON auth_codes (grant_id);
CREATE INDEX auth_codes_expires_at ON auth_codes (expires_at);

-- Opaque refresh tokens. Confidential clients slide `expires_at`; public
-- clients rotate (`rotated_at`) with a short reuse window.
CREATE TABLE refresh_tokens (
    token_hash text PRIMARY KEY CHECK (length(token_hash) > 0),
    grant_id   uuid NOT NULL REFERENCES grants (id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL,
    rotated_at timestamptz,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX refresh_tokens_grant_id ON refresh_tokens (grant_id);
CREATE INDEX refresh_tokens_expires_at ON refresh_tokens (expires_at);

-- TOTP enrolment, at most one per user. `confirmed_at` is NULL until the first
-- valid code; `last_used_step` rejects replay of an already accepted code.
CREATE TABLE mfa_totp (
    user_id           uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    secret_ciphertext bytea NOT NULL CHECK (octet_length(secret_ciphertext) > 0),
    secret_nonce      bytea NOT NULL CHECK (octet_length(secret_nonce) = 12),
    confirmed_at      timestamptz,
    last_used_step    bigint CHECK (last_used_step >= 0),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

-- WebAuthn credentials. `passkey` is the serialized webauthn-rs Passkey; it is
-- rewritten after each use because the signature counter changes.
CREATE TABLE passkeys (
    id            uuid PRIMARY KEY,
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    credential_id bytea NOT NULL UNIQUE CHECK (octet_length(credential_id) > 0),
    passkey       jsonb NOT NULL CHECK (jsonb_typeof(passkey) = 'object'),
    name          text NOT NULL CHECK (length(name) > 0),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    last_used_at  timestamptz
);
CREATE INDEX passkeys_user_id ON passkeys (user_id);

-- In-flight WebAuthn ceremony state. Registration always belongs to a user;
-- passkey-only login starts without one.
CREATE TABLE webauthn_challenges (
    id_hash    text PRIMARY KEY CHECK (length(id_hash) > 0),
    user_id    uuid REFERENCES users (id) ON DELETE CASCADE,
    kind       text NOT NULL CHECK (kind IN ('REGISTRATION', 'AUTHENTICATION')),
    state      jsonb NOT NULL,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT webauthn_challenges_registration_has_user
        CHECK (kind <> 'REGISTRATION' OR user_id IS NOT NULL)
);
CREATE INDEX webauthn_challenges_user_id ON webauthn_challenges (user_id);
CREATE INDEX webauthn_challenges_expires_at ON webauthn_challenges (expires_at);

-- Hashed single-use recovery codes. The primary key's leading column indexes user_id.
CREATE TABLE recovery_codes (
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash  text NOT NULL CHECK (length(code_hash) > 0),
    used_at    timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, code_hash)
);

-- Transactional outbox for auth.events.* (docs/DATABASE.md, "Outbox and inbox").
CREATE TABLE outbox (
    id           uuid PRIMARY KEY,          -- also the Nats-Msg-Id, so JetStream drops duplicates
    subject      text NOT NULL,
    payload      bytea NOT NULL,            -- encoded protobuf message
    created_at   timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz
);
CREATE INDEX outbox_unpublished ON outbox (created_at) WHERE published_at IS NULL;
