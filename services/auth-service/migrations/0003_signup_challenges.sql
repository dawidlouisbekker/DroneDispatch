-- Short-lived signup state. Raw email, OTP and password are never stored.
CREATE TABLE signup_challenges (
    id_hash       text PRIMARY KEY CHECK (length(id_hash) > 0),
    email_hash    text NOT NULL CHECK (length(email_hash) > 0),
    otp_hash      text NOT NULL CHECK (length(otp_hash) > 0),
    expires_at    timestamptz NOT NULL,
    attempts      integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    verified_at   timestamptz,
    last_sent_at  timestamptz NOT NULL DEFAULT now(),
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX signup_challenges_expires_at ON signup_challenges (expires_at);
CREATE INDEX signup_challenges_email_hash ON signup_challenges (email_hash);
