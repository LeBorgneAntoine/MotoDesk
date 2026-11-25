-- Company multi-tenant support on the desktop side.
--
-- The desktop install belongs to exactly one company (the logged-in user's),
-- so business tables stay company-agnostic. We only need to remember which
-- company we belong to (for the mesh fingerprint), the cloud-issued mesh
-- secret (replaces the old locally-generated mesh_psk as the LAN trust root),
-- and an offline cache of the company identity + logo.

ALTER TABLE sync_state ADD COLUMN company_id TEXT;
ALTER TABLE sync_state ADD COLUMN mesh_secret TEXT;

CREATE TABLE IF NOT EXISTS company_cache (
    id                 INTEGER PRIMARY KEY CHECK (id = 1),
    company_id         TEXT,
    name               TEXT,
    legal_name         TEXT,
    logo_bytes         BLOB,
    logo_content_type  TEXT,
    logo_etag          TEXT,
    logo_updated_at    TEXT,
    cached_at          TEXT
);
