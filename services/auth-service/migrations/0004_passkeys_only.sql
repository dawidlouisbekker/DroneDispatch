-- Passkeys are the only second factor: the authenticator app (TOTP) and the
-- recovery codes that came with the first factor are gone. Accounts that only
-- had TOTP are password accounts again until they add a passkey.
DROP TABLE mfa_totp;
DROP TABLE recovery_codes;
