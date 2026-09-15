-- One database and one role per service. Local development credentials only.
-- REVOKE CONNECT ... FROM PUBLIC leaves each database reachable only by its
-- owner (and the postgres superuser), so a service cannot query another's data.
CREATE ROLE auth LOGIN PASSWORD 'auth';
CREATE DATABASE auth OWNER auth;
REVOKE CONNECT ON DATABASE auth FROM PUBLIC;

CREATE ROLE dispatch LOGIN PASSWORD 'dispatch';
CREATE DATABASE dispatch OWNER dispatch;
REVOKE CONNECT ON DATABASE dispatch FROM PUBLIC;

CREATE ROLE merchant LOGIN PASSWORD 'merchant';
CREATE DATABASE merchant OWNER merchant;
REVOKE CONNECT ON DATABASE merchant FROM PUBLIC;

CREATE ROLE commerce LOGIN PASSWORD 'commerce';
CREATE DATABASE commerce OWNER commerce;
REVOKE CONNECT ON DATABASE commerce FROM PUBLIC;

CREATE ROLE map LOGIN PASSWORD 'map';
CREATE DATABASE map OWNER map;
REVOKE CONNECT ON DATABASE map FROM PUBLIC;
