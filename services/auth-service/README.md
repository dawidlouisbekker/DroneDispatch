# auth-service

The OAuth 2.1 authorization server for Drone Drop. Every login goes through it: Alexa+ account linking, the customer app and the merchant portal. It issues ES256 JWT access tokens that the other services validate on their own against its JWKS, so no other service ever sees a password or an MFA secret.

**Status:** scaffold. The service boots, connects to Postgres and NATS, and serves `/healthz`. The OAuth and MFA features are milestone 2.

## Responsibilities
- **Authorization code grant** with PKCE (S256 required), exact redirect URI matching, and a consent page that shows the redirect host.
- **Clients:** static clients created from the CLI (Alexa, map-web, merchant-web), Dynamic Client Registration, and Client ID Metadata Documents fetched with SSRF protection.
- **Access tokens:** RFC 9068 JWTs (`typ: at+jwt`, ES256), `expires_in` = 3600, one audience per resource. Claims: `iss`, `sub`, `aud`, `client_id`, `scope`, `iat`, `exp`, `jti`, `gid`, `acr`, `amr`, `auth_time`.
- **Refresh tokens:** opaque and stored as hashes. Confidential clients (Alexa) get a sliding expiry without rotation. Public clients rotate, with a 60-second reuse window.
- **MFA:**
  - TOTP: secret encrypted with AES-GCM, ±1 time step, rate-limited attempts.
  - Passkeys (WebAuthn, RP ID = auth host).
  - 10 recovery codes, stored hashed.
  - Login is a password plus a second factor, or a passkey on its own.
- **Step-up:** `/authorize` honours `acr_values=mfa` and `max_age`.
- **Revocation:** revoking a grant publishes `auth.events.grant_revoked`, and resource servers reject that `gid` from then on.

## Owns (Postgres database `auth`)
Schema in [`migrations/`](migrations/), applied at start-up; conventions in [DATABASE.md](../../docs/DATABASE.md). Access tokens are JWTs and are not stored.
- `users`: accounts; `id` is the `sub`.
- `sessions`: login sessions with `auth_time` and `amr`.
- `clients`: static and DCR OAuth clients.
- `auth_requests`: validated `/authorize` requests parked during login and consent.
- `grants`: one per authorization; `id` is the `gid` claim.
- `auth_codes`, `refresh_tokens`: belong to a grant.
- `mfa_totp`, `passkeys`, `webauthn_challenges`, `recovery_codes`: MFA.
- `outbox`: `auth.events.*` waiting to be published.

## Resources it issues tokens for
| Resource | Scopes | MFA policy |
|---|---|---|
| `{DISPATCH}/mcp` | `openid`, `delivery` | None, so voice payments work |
| `{MAP}` | `openid`, `email` | Step-up when adding a delivery location |
| `{MERCHANT}` | `openid`, `email`, `merchant` | Always |

## Interfaces
**HTTP**
| Endpoint | Purpose | Status |
|---|---|---|
| `GET /healthz` | Liveness | Done |
| `GET /.well-known/oauth-authorization-server` | AS metadata (RFC 8414), including `jwks_uri` and `S256` in `code_challenge_methods_supported` | Planned |
| `GET /.well-known/jwks.json` | Public ES256 signing keys | Planned |
| `GET /authorize` | Login, MFA and consent pages | Planned |
| `POST /token` | Authorization code and refresh token grants | Planned |
| `POST /register` | Dynamic Client Registration | Planned |
| `POST /revoke` | Token revocation | Planned |

**gRPC server** (internal port 9081, [`auth.proto`](../../proto/dronedrop/auth/v1/auth.proto))
| Service | RPCs | Called by |
|---|---|---|
| `UserDirectory` | `GetUser`, `BatchGetUsers` | merchant, commerce |
| `GrantRegistry` | `ListRevokedGrants` | dispatch, merchant, map |

**NATS** (payloads from [`auth_events.proto`](../../proto/dronedrop/events/v1/auth_events.proto))
| Direction | Subject | Payload |
|---|---|---|
| Publishes | `auth.events.grant_revoked` | `GrantRevoked` |
| Publishes | `auth.events.user_deleted` | `UserDeleted` |

## Alexa+ constraints
Account linking breaks if any of these are violated:
- Alexa uses a **static client**; register every regional Alexa redirect URI on it.
- The metadata must list `S256` in `code_challenge_methods_supported`, or `alexa-ai deploy` fails.
- Refresh must never break the link. Alexa's refresh requests carry no `resource` parameter.
- `expires_in` must be at least 3600, and `/token` must respond in under 4.5 seconds.
- Pages must be mobile-friendly and must not open pop-ups.
- Alexa caches the metadata at deploy time, so the service needs a stable public hostname (`auth.` on the named cloudflared tunnel).

## Configuration
| Variable | Default | Notes |
|---|---|---|
| `HTTP_ADDR` | `0.0.0.0:8081` | |
| `DATABASE_URL` | required | e.g. `postgres://auth:auth@localhost:5432/auth` |
| `NATS_URL` | `nats://localhost:4222` | |

Planned: `GRPC_ADDR` (`0.0.0.0:9081`), `AUTH_ISSUER` (public URL), the ES256 signing key, and the TOTP encryption key.

## Run
```bash
docker compose up auth-service         # starts Postgres and NATS too
curl localhost:8081/healthz

# Or run it from source against the Compose infrastructure:
DATABASE_URL=postgres://auth:auth@localhost:5432/auth cargo run -p auth-service
```
