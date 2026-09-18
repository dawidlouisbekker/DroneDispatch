-- One login role per service, and one database per service. Edge services
-- (catalog-read, dispatch) get one database per zone, owned by the service's role.
-- Local development credentials only.
--
-- REVOKE CONNECT ... FROM PUBLIC leaves each database reachable only by its owner
-- (and the postgres superuser), so a service cannot query another's data.
--
-- Postgres runs this only on an empty data volume. It is idempotent, so apply it to
-- an existing volume with:
--   docker compose exec -T postgres psql -U postgres < config/postgres/init.sql

SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', role_name, role_name)
FROM unnest(ARRAY['auth', 'user_service', 'merchant', 'catalog_read', 'dispatch']) AS role_name
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name)
\gexec

SELECT format('CREATE DATABASE %I OWNER %I', database_name, owner_name)
FROM (VALUES
    ('auth', 'auth'),
    ('user_service', 'user_service'),
    ('merchant', 'merchant'),
    ('catalog_read_sea_north', 'catalog_read'),
    ('catalog_read_sea_south', 'catalog_read'),
    ('dispatch_sea_north', 'dispatch'),
    ('dispatch_sea_south', 'dispatch')
) AS databases (database_name, owner_name)
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = database_name)
\gexec

SELECT format('REVOKE CONNECT ON DATABASE %I FROM PUBLIC', datname)
FROM pg_database
WHERE datname IN (
    'auth', 'user_service', 'merchant',
    'catalog_read_sea_north', 'catalog_read_sea_south',
    'dispatch_sea_north', 'dispatch_sea_south'
)
\gexec
