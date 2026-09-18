-- Refresh tokens issue new access tokens long after sign-in, so each grant keeps
-- how the user authenticated when it was created (the acr, amr and auth_time
-- claims every token of the grant carries).
ALTER TABLE grants
    ADD COLUMN acr       text NOT NULL DEFAULT 'pwd' CHECK (length(acr) > 0),
    ADD COLUMN amr       text[] NOT NULL DEFAULT '{pwd}' CHECK (cardinality(amr) > 0),
    ADD COLUMN auth_time timestamptz NOT NULL DEFAULT now();
