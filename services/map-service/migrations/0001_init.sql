-- map-service: server-side web sessions and parked OAuth login / step-up state.
-- See docs/DATABASE.md. map has no outbox or inbox.

-- One row per browser login. The cookie holds a random session id; only its
-- SHA-256 (base64url) is stored.
CREATE TABLE web_sessions (
    id_hash                  text PRIMARY KEY CHECK (length(id_hash) > 0),
    user_sub                 uuid NOT NULL,
    csrf_token_hash          text NOT NULL CHECK (length(csrf_token_hash) > 0),
    -- `amr` and `auth_time` of the latest tokens (updated after an MFA step-up).
    amr                      text[] NOT NULL DEFAULT '{}' CHECK (array_position(amr, NULL) IS NULL),
    auth_time                timestamptz NOT NULL,
    grant_id                 uuid NOT NULL,
    -- AES-GCM encrypted refresh token, for renewing access tokens.
    refresh_token_ciphertext bytea CHECK (octet_length(refresh_token_ciphertext) > 0),
    refresh_token_nonce      bytea CHECK (octet_length(refresh_token_nonce) = 12),
    expires_at               timestamptz NOT NULL,
    last_seen_at             timestamptz NOT NULL DEFAULT now(),
    created_at               timestamptz NOT NULL DEFAULT now(),
    updated_at               timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT web_sessions_refresh_token_pair
        CHECK ((refresh_token_ciphertext IS NULL) = (refresh_token_nonce IS NULL)),
    CONSTRAINT web_sessions_expires_after_created
        CHECK (expires_at > created_at)
);
CREATE INDEX web_sessions_user_sub ON web_sessions (user_sub);
CREATE INDEX web_sessions_grant_id ON web_sessions (grant_id);
CREATE INDEX web_sessions_expires_at ON web_sessions (expires_at);

COMMENT ON TABLE web_sessions IS 'Server-side web session per browser login.';
COMMENT ON COLUMN web_sessions.id_hash IS 'SHA-256 base64url of the session cookie value';
COMMENT ON COLUMN web_sessions.user_sub IS 'ref: auth.users.id';
COMMENT ON COLUMN web_sessions.grant_id IS 'ref: auth.grants.id';
COMMENT ON COLUMN web_sessions.csrf_token_hash IS 'SHA-256 base64url of the CSRF token';

-- OAuth state parked between the redirect to auth-service and the callback,
-- for normal login and for MFA step-up (acr_values=mfa&max_age=300).
CREATE TABLE oauth_login_states (
    state_hash               text PRIMARY KEY CHECK (length(state_hash) > 0),
    -- AES-GCM encrypted PKCE code_verifier (sent back at the token exchange).
    pkce_verifier_ciphertext bytea NOT NULL CHECK (octet_length(pkce_verifier_ciphertext) > 0),
    pkce_verifier_nonce      bytea NOT NULL CHECK (octet_length(pkce_verifier_nonce) = 12),
    nonce_hash               text NOT NULL CHECK (length(nonce_hash) > 0),
    purpose                  text NOT NULL CHECK (purpose IN ('LOGIN', 'STEP_UP')),
    -- The delivery location being verified; only for STEP_UP.
    location_id              uuid,
    -- The session that started the step-up; only for STEP_UP. The callback
    -- must come back on this session (and the new token's sub must match).
    web_session_id_hash      text REFERENCES web_sessions (id_hash) ON DELETE CASCADE,
    -- Relative path only: starts with '/', not '//', no backslashes or control
    -- characters (browsers treat '/\' and '/<tab>/' like '//'). Blocks open redirects.
    return_to                text NOT NULL DEFAULT '/',
    expires_at               timestamptz NOT NULL,
    created_at               timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT oauth_login_states_step_up_location
        CHECK ((purpose = 'STEP_UP') = (location_id IS NOT NULL)),
    CONSTRAINT oauth_login_states_step_up_session
        CHECK ((purpose = 'STEP_UP') = (web_session_id_hash IS NOT NULL)),
    CONSTRAINT oauth_login_states_return_to_relative
        CHECK (
            left(return_to, 1) = '/'
            AND left(return_to, 2) <> '//'
            AND strpos(return_to, E'\\') = 0
            AND return_to !~ '[[:cntrl:]]'
            AND length(return_to) <= 2048
        ),
    CONSTRAINT oauth_login_states_expires_after_created
        CHECK (expires_at > created_at)
);
CREATE INDEX oauth_login_states_expires_at ON oauth_login_states (expires_at);
CREATE INDEX oauth_login_states_web_session_id_hash ON oauth_login_states (web_session_id_hash)
    WHERE web_session_id_hash IS NOT NULL;

COMMENT ON TABLE oauth_login_states IS 'OAuth state between redirect and callback (login and MFA step-up); rows expire after 10 minutes.';
COMMENT ON COLUMN oauth_login_states.state_hash IS 'SHA-256 base64url of the OAuth state parameter';
COMMENT ON COLUMN oauth_login_states.nonce_hash IS 'SHA-256 base64url of the OIDC nonce';
COMMENT ON COLUMN oauth_login_states.location_id IS 'ref: commerce.delivery_locations.id';
