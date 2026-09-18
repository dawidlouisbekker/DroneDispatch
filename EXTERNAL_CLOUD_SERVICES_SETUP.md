# Cloud setup: Stripe and AWS

This guide covers creating and configuring the external services Drone Drop uses, and where each value goes in `.env`. Stripe runs in a sandbox, so no real money moves.

> The services don't read these variables yet; milestones 3–7 wire them up. Setting the accounts up now means nobody has to wait for them later.

## What you need

| Service | Used for | Needed locally? | `.env` variables |
|---|---|---|---|
| Stripe sandbox with Connect | Customer payments and saving a card (user), shop onboarding and payouts (merchant) | Yes | `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_THIN_WEBHOOK_SECRET` |
| Stripe CLI | Forwarding Stripe webhooks to your machine | Yes (Compose can run it) | none |
| Amazon Location Places | Shop search in the MCP server and address lookup (user), claiming a place (merchant) | Yes | `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` |
| Amazon Location Maps API key | MapLibre maps in the customer app and merchant portal | Yes | `AMAZON_LOCATION_MAPS_API_KEY` |
| Amazon SES | Order emails to merchants | No: Compose runs Mailpit | `SES_FROM_ADDRESS` (deployment) |

Start with `cp .env.example .env`. `.env` is git-ignored: never commit it, and share keys through a password manager, not chat.

---

## Part 1: Stripe

Drone Drop is a **marketplace**:
- Customers pay the platform. Each order is a **destination charge**: Stripe moves the money to the shop and the platform keeps an application fee.
- Shops are **Accounts v2** connected accounts with the `recipient` configuration.
- Shops sign up through Stripe-hosted onboarding and manage payouts in the **Express Dashboard**.

### 1.1 Create an account and a sandbox
1. Sign up at [dashboard.stripe.com/register](https://dashboard.stripe.com/register).
2. Open the Dashboard account picker, choose **Sandboxes**, and create a sandbox such as `drone-drop-dev`. Do everything in this guide inside that sandbox.
3. In the sandbox, open [API keys](https://dashboard.stripe.com/apikeys) and copy the **secret key** (`sk_test_…`) into `.env` as `STRIPE_SECRET_KEY`.
   - You don't need the publishable key: customers enter cards on Stripe-hosted Checkout.

### 1.2 Set up Connect as a marketplace
Stripe won't create connected accounts until the platform is set up. Until then, API calls fail with `platform_registration_required` or `connect_profile_not_submitted`.

1. In the sandbox, open [Connect](https://dashboard.stripe.com/connect) and start the platform setup.
2. When asked what you're building, pick a **marketplace**. You are the merchant of record, you collect a fee on each sale, and Stripe handles seller onboarding.
3. Answer the remaining platform profile questions.

The Workbench [Marketplace Blueprint](https://dashboard.stripe.com/test/workbench/blueprints/learn-accounts-v2-marketplace) walks through the same Accounts v2 and destination-charge flow step by step. It's a good way to see the whole picture before writing code.

### 1.3 Onboarding, branding and payment methods
- **Branding (required):** on the [Connect settings page](https://dashboard.stripe.com/account/applications/settings), set the business name (Drone Drop), a brand color and an icon. Stripe-hosted onboarding doesn't work without them.
- **Onboarding countries:** in [Onboarding options](https://dashboard.stripe.com/settings/connect/onboarding-options/countries), allow **United States**, since the demo shops are in Seattle.
- **Payment methods:** in [Payment methods](https://dashboard.stripe.com/settings/payment_methods), keep **Cards** on. Voice orders charge the customer's saved card without the customer present ("off-session"), so cards are what matter.
- **Checkout branding (optional):** customers see the platform's [branding](https://dashboard.stripe.com/account/branding) on Checkout for destination charges.

### 1.4 Stripe CLI
You don't have to install it: `docker compose --profile stripe up stripe-cli` runs it in a container. To install it anyway, follow [docs.stripe.com/cli/install](https://docs.stripe.com/cli/install), then confirm `stripe listen --help` lists `--thin-events`.

All commands in this guide pass `--api-key "$STRIPE_SECRET_KEY"`, so the CLI always uses the same sandbox as the services, and `stripe login` isn't needed. Load `.env` into your shell first:

```bash
set -a; source .env; set +a
```

### 1.5 Webhooks
Stripe sends two kinds of events, and each goes to its own endpoint with its own signing secret:
- **Snapshot events** carry the full object, like classic webhooks.
- **Thin events** carry only an id, and Accounts v2 sends only thin events. Stripe requires a separate endpoint for them.

| Endpoint | Payload | Event | What happens |
|---|---|---|---|
| `/webhooks/stripe` on user-service | Snapshot | `payment_intent.amount_capturable_updated` | Card authorized; the order waits for the merchant |
| | | `payment_intent.succeeded` | Capture succeeded; the **only** way an order becomes PAID and a drone is dispatched |
| | | `payment_intent.payment_failed` | Card declined; PAYMENT_FAILED |
| | | `payment_intent.canceled` | Authorization voided after a rejection or timeout |
| | | `charge.refunded`, `refund.updated` | Refund progress |
| | | `checkout.session.completed` | Customer saved a card (Checkout in setup mode) |
| `/webhooks/stripe/thin` on merchant-service | Thin | `v2.core.account[configuration.recipient].capability_status_updated` | A shop's `stripe_transfers` capability turned on or off, so it can or can't take orders |
| | | `v2.core.account[requirements].updated` | Stripe needs more information from a shop |
| | | `v2.core.account_link.returned` | A shop came back from onboarding |

#### Local development: Stripe CLI
Nothing to configure in the Dashboard: the CLI receives the events and forwards them.

**Option A: Compose**, when user-service and merchant-service run in Docker:
```bash
docker compose --profile stripe up -d stripe-cli
docker compose --profile stripe logs stripe-cli | grep -i secret
```

**Option B: installed CLI**, when they run on the host with `cargo run`:
```bash
stripe listen --api-key "$STRIPE_SECRET_KEY" \
  --events payment_intent.amount_capturable_updated,payment_intent.succeeded,payment_intent.payment_failed,payment_intent.canceled,charge.refunded,refund.updated,checkout.session.completed \
  --forward-to localhost:8085/webhooks/stripe \
  --thin-events 'v2.core.account[requirements].updated,v2.core.account[configuration.recipient].capability_status_updated,v2.core.account_link.returned' \
  --forward-thin-to localhost:8083/webhooks/stripe/thin
```

Both options print `Ready! Your webhook signing secret is whsec_…`. That secret signs everything the CLI forwards, and it doesn't change between restarts. Put it in **both** `STRIPE_WEBHOOK_SECRET` and `STRIPE_THIN_WEBHOOK_SECRET`.

To print the secret without listening, run `stripe listen --api-key "$STRIPE_SECRET_KEY" --print-secret`.

#### Public URL: tunnel or deployment
Create two event destinations in the sandbox:
1. Open Workbench → [Webhooks](https://dashboard.stripe.com/webhooks) → **Create an event destination**.
2. Choose **Your account**. Destination charges and Accounts v2 account events both belong to the platform, not to the connected accounts.
3. **First destination:**
   - Snapshot payload, with your account's default API version.
   - Select the snapshot events from the table.
   - Destination type **Webhook endpoint**, URL `https://user.<your-domain>/webhooks/stripe`.
   - **Reveal secret** and set it as `STRIPE_WEBHOOK_SECRET`.
4. **Second destination:**
   - Thin payload, with the three `v2.core.account…` events.
   - URL `https://merchant.<your-domain>/webhooks/stripe/thin`.
   - Set its secret as `STRIPE_THIN_WEBHOOK_SECRET`.

Use either the CLI or the Dashboard destinations for a given deployment, not both at once. Events signed with the other secret fail verification.

### 1.6 Check it works: onboard a test shop
You can confirm the sandbox is ready before any Drone Drop code exists, by making the same two calls merchant-service will make.

Accounts v2 requests need a `Stripe-Version` header. Use the current version from the [API reference](https://docs.stripe.com/api/v2/core/accounts/create); it was `2026-08-26.dahlia` when this guide was written.

```bash
set -a; source .env; set +a
STRIPE_VERSION=2026-08-26.dahlia

# 1. Create a shop. The Express Dashboard requires the platform to collect fees
#    and cover losses; `recipient` lets the shop receive destination-charge transfers.
curl -s https://api.stripe.com/v2/core/accounts \
  -H "Authorization: Bearer $STRIPE_SECRET_KEY" \
  -H "Stripe-Version: $STRIPE_VERSION" \
  -H "Content-Type: application/json" \
  -d '{
    "display_name": "Test Cafe",
    "contact_email": "test-cafe@example.com",
    "dashboard": "express",
    "identity": {"country": "US", "entity_type": "company"},
    "defaults": {"responsibilities": {"fees_collector": "application", "losses_collector": "application"}},
    "configuration": {"recipient": {"capabilities": {"stripe_balance": {"stripe_transfers": {"requested": true}}}}}
  }'
# Copy the "id" (acct_…) from the response.

# 2. Create a single-use onboarding link for it.
curl -s https://api.stripe.com/v2/core/account_links \
  -H "Authorization: Bearer $STRIPE_SECRET_KEY" \
  -H "Stripe-Version: $STRIPE_VERSION" \
  -H "Content-Type: application/json" \
  -d '{
    "account": "acct_REPLACE_ME",
    "use_case": {
      "type": "account_onboarding",
      "account_onboarding": {
        "configurations": ["recipient"],
        "refresh_url": "http://localhost:8083/stripe/onboarding/refresh",
        "return_url": "http://localhost:8083/stripe/onboarding/return"
      }
    }
  }'
# Open the "url" from the response. The link expires after a few minutes and works only once.
```

Complete onboarding with Stripe's sandbox test data. The return page doesn't exist yet, so a 404 at the end is expected.

| Field | Test value |
|---|---|
| Phone | `0000000000`, SMS code `000000` |
| Date of birth | `1901-01-01` |
| SSN (full or last 4) | `000000000` or `0000` |
| Address line 1 | `address_full_match` |
| Business tax ID (EIN) | `000000000` |
| Bank routing / account number | `110000000` / `000123456789` |

The shop should then appear under **Connect → Connected accounts** with transfers enabled. With the CLI listening, you'll see the thin events arrive.

**Test cards** for the customer side (any future expiry date, any CVC, any postal code):

| Card | Behaviour |
|---|---|
| `4242 4242 4242 4242` | Succeeds |
| `4000 0000 0000 0077` | Succeeds, and the funds are available immediately, so transfers to shops go through in the sandbox |
| `4000 0000 0000 9995` | Declined with `insufficient_funds`, giving PAYMENT_FAILED |
| `4000 0027 6000 3184` | Always requires authentication, so an off-session voice order fails with `authentication_required` |

---

## Part 2: AWS

Everything lives in **us-west-2 (US West, Oregon)**, the simulated city's region.

### 2.1 Account, CLI and a budget alarm
1. Sign in to the AWS console and switch the region selector to **US West (Oregon)**.
2. In **Billing and Cost Management → Budgets**, create a small monthly cost budget (say $10) with an email alert. Places is billed per request, and results you store cost more.
3. Install [AWS CLI v2](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html). The setup commands below need an admin identity. The services themselves get a restricted user.

### 2.2 IAM user for the services (Places)
The containers need static credentials. Create a dedicated user whose only permission is Amazon Location Places; its policy is in [config/aws/local-dev-policy.json](config/aws/local-dev-policy.json).

With the CLI, as an admin:
```bash
aws iam create-user --user-name drone-drop-local
aws iam put-user-policy --user-name drone-drop-local \
  --policy-name DroneDropPlaces \
  --policy-document file://config/aws/local-dev-policy.json
aws iam create-access-key --user-name drone-drop-local
```
Copy `AccessKeyId` and `SecretAccessKey` into `.env`. The secret is shown only once.

In the console instead:
1. **IAM → Users → Create user** `drone-drop-local`, with no console access.
2. On the user, **Add permissions → Create inline policy → JSON**, and paste the policy file.
3. **Security credentials → Create access key**, choosing "Application running outside AWS".

Check the credentials:
```bash
set -a; source .env; set +a
aws geo-places search-text --region "$AWS_REGION" \
  --query-text "Pike Place Market" --bias-position -122.3422 47.6097 \
  --max-results 3 --query 'ResultItems[].Title'
```
Pike Place Market in the results means the credentials work. `AccessDeniedException` means the policy isn't attached, or the call went to a region other than us-west-2.

**Storage pricing:** Places searches that are only shown to the customer use `IntendedUse=SingleUse`. When merchant-service stores a claimed place's coordinates, it must call `GetPlace` with `IntendedUse=Storage`. AWS requires that for any stored or cached result, and bills it at the higher "Stored" tier.

### 2.3 Maps API key (browser maps)
Maps render in the browser, so they use an **API key** rather than IAM credentials. The key is visible in page source, which is why it's limited to read-only map actions and to the pages you allow.

With the CLI:
```bash
aws location create-key --region us-west-2 \
  --key-name drone-drop-maps-local \
  --restrictions '{
    "AllowActions": ["geo-maps:*"],
    "AllowResources": ["arn:aws:geo-maps:us-west-2::provider/default"],
    "AllowReferers": ["http://localhost:8085/*", "http://localhost:8083/*"]
  }' \
  --no-expiry
aws location describe-key --region us-west-2 --key-name drone-drop-maps-local --query Key --output text
```
Put the `v1.public.…` value in `.env` as `AMAZON_LOCATION_MAPS_API_KEY`.

In the console instead, go to **Amazon Location → API keys → Create API key**:
- Choose the Maps resource with its map actions.
- Under **Client restrictions**, add the referrers `http://localhost:8085/*` (customer app) and `http://localhost:8083/*` (merchant portal).
- Then **Show API key**.

When you add tunnel hostnames, add them as referrers too, e.g. `https://map.<your-domain>/*`.

Check the key:
```bash
curl -s -o /dev/null -w '%{http_code}\n' -H 'Referer: http://localhost:8085/' \
  "https://maps.geo.us-west-2.amazonaws.com/v2/styles/Standard/descriptor?key=$AMAZON_LOCATION_MAPS_API_KEY"
```
- `200`: the key works. That same URL is the MapLibre style the map pages will load.
- `403`: the referrer (including the port), the region or the resource doesn't match the key.

### 2.4 SES for merchant emails (deployment only)
Locally, Mailpit at [localhost:8025](http://localhost:8025) catches every email. Skip this section for local work.

1. **Verify an identity.** In the **SES console** (us-west-2), go to **Identities → Create identity**. Verify a domain (by adding its DNS records) or a single sender address, and set `SES_FROM_ADDRESS` to it.
2. **Plan around the sandbox.** New SES accounts start in the sandbox: you can send only to verified identities, at most 200 emails per 24 hours and 1 per second. For a demo, verify the test merchants' addresses too. Request production access only if you need to email unverified recipients.
3. **Grant access.** Give the deployed merchant-service role `ses:SendEmail` on the identity.

---

## Part 3: Finish and check `.env`

| Variable | Value comes from |
|---|---|
| `STRIPE_SECRET_KEY` | 1.1 |
| `STRIPE_WEBHOOK_SECRET` | 1.5: the CLI secret locally, or the snapshot destination's secret |
| `STRIPE_THIN_WEBHOOK_SECRET` | 1.5: the same CLI secret locally, or the thin destination's secret |
| `AWS_REGION` | `us-west-2` |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | 2.2 |
| `AMAZON_LOCATION_MAPS_API_KEY` | 2.3 |

After editing `.env`, run `docker compose up -d`. It recreates only the services whose settings changed.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `platform_registration_required` or `connect_profile_not_submitted` when creating a shop | Connect platform setup isn't finished (1.2) |
| `account_controller_express_dash_without_application_losses_or_fees` | The Express Dashboard requires both `fees_collector` and `losses_collector` to be `application` |
| `accounts_v2_access_blocked` | Accounts v2 isn't available on this Stripe account. Finish Connect setup in the sandbox, and contact Stripe support if it persists |
| Onboarding link errors, or immediately sends you to `refresh_url` | Branding isn't set (1.3), or the link was already used or has expired; create a new one |
| Webhook signature verification fails | `.env` holds a different secret than the sender uses: CLI vs Dashboard, a different sandbox, or the snapshot and thin secrets swapped. Restart user-service and merchant-service after fixing it |
| AWS `AccessDeniedException` from Places | The policy isn't attached, or the request went to a region other than us-west-2 |
| Blank map, or the style request returns 403 | The page's origin (including its port) isn't an allowed referrer, or the style URL uses the wrong region |
