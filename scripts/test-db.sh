#!/usr/bin/env bash
# Runs the database schema tests against the Compose Postgres on port 5432.
# They are #[ignore]d so plain `cargo test` works without a database; this
# starts Postgres if needed, points sqlx at it and runs only those tests.
#
# Usage: scripts/test-db.sh                       # every service
#        scripts/test-db.sh -p merchant-service   # one service
set -euo pipefail
cd "$(dirname "$0")/.."

docker compose up -d --wait postgres
# Superuser URL: #[sqlx::test] creates a throwaway database per test.
export DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/postgres}"
cargo test "${@:---workspace}" -- --ignored
