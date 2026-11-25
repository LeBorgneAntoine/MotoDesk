-- Home for the data recovered from the Divalto Harmony (Motogest) backup.
--
-- Two things are missing from the schema for a legacy import to be usable:
--
-- 1. A place to keep the originating Motogest identifier. Without it the two
--    systems cannot be reconciled and a re-import cannot tell an update from a
--    duplicate, so `legacy_ref` is added to the entities that came across and
--    made unique per company.
-- 2. Tables for records that have no equivalent yet -- suppliers, individual
--    payments (a sale can be settled in several instalments), and workshop
--    orders -- plus `import_issues`, which records every row the importer
--    judged doubtful so the data can be cleaned up from inside the app rather
--    than by re-reading the binaries.
--
-- Existing columns are left untouched; the application reads explicit column
-- lists, so additions are safe.

ALTER TABLE clients  ADD COLUMN legacy_ref TEXT;
ALTER TABLE vehicles ADD COLUMN legacy_ref TEXT;
ALTER TABLE products ADD COLUMN legacy_ref TEXT;
ALTER TABLE sales    ADD COLUMN legacy_ref TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS clients_company_legacy_idx
    ON clients (company_id, legacy_ref) WHERE legacy_ref IS NOT NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS vehicles_company_legacy_idx
    ON vehicles (company_id, legacy_ref) WHERE legacy_ref IS NOT NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS products_company_legacy_idx
    ON products (company_id, legacy_ref) WHERE legacy_ref IS NOT NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS sales_company_legacy_idx
    ON sales (company_id, legacy_ref) WHERE legacy_ref IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS suppliers (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    contact     TEXT,
    phone       TEXT,
    email       TEXT,
    address     TEXT,
    notes       TEXT,
    legacy_ref  TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    row_version INTEGER NOT NULL DEFAULT 1,
    deleted_at  TEXT,
    field_meta  TEXT NOT NULL DEFAULT '{}',
    company_id  TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS suppliers_company_legacy_idx
    ON suppliers (company_id, legacy_ref) WHERE legacy_ref IS NOT NULL AND deleted_at IS NULL;

-- A sale can be settled in several instalments, each with its own method and
-- date, so payments are rows rather than columns on `sales`.
CREATE TABLE IF NOT EXISTS payments (
    id                TEXT PRIMARY KEY,
    sale_id           TEXT REFERENCES sales(id)   ON DELETE CASCADE,
    client_id         TEXT REFERENCES clients(id) ON DELETE SET NULL,
    paid_at           TEXT,
    due_at            TEXT,
    amount_cents      INTEGER NOT NULL DEFAULT 0,
    outstanding_cents INTEGER NOT NULL DEFAULT 0,
    method            TEXT,
    bank              TEXT,
    cash_register     TEXT,
    cheque_drawer     TEXT,
    notes             TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    row_version       INTEGER NOT NULL DEFAULT 1,
    deleted_at        TEXT,
    field_meta        TEXT NOT NULL DEFAULT '{}',
    company_id        TEXT
);

CREATE INDEX IF NOT EXISTS payments_sale_idx   ON payments (sale_id);
CREATE INDEX IF NOT EXISTS payments_client_idx ON payments (client_id);

CREATE TABLE IF NOT EXISTS workshop_orders (
    id            TEXT PRIMARY KEY,
    sale_id       TEXT REFERENCES sales(id)    ON DELETE SET NULL,
    client_id     TEXT REFERENCES clients(id)  ON DELETE SET NULL,
    vehicle_id    TEXT REFERENCES vehicles(id) ON DELETE SET NULL,
    received_at   TEXT,
    promised_at   TEXT,
    label         TEXT,
    mileage       INTEGER,
    fuel_level    TEXT,
    planned_hours REAL,
    notes         TEXT,
    legacy_ref    TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    row_version   INTEGER NOT NULL DEFAULT 1,
    deleted_at    TEXT,
    field_meta    TEXT NOT NULL DEFAULT '{}',
    company_id    TEXT
);

CREATE INDEX IF NOT EXISTS workshop_orders_client_idx  ON workshop_orders (client_id);
CREATE INDEX IF NOT EXISTS workshop_orders_vehicle_idx ON workshop_orders (vehicle_id);

-- One row per doubtful field, written by the importer. `severity` is 'error'
-- for something that needed correcting to load at all, 'warning' for data that
-- loaded but should be reviewed, and 'info' for context worth keeping.
CREATE TABLE IF NOT EXISTS import_issues (
    id          TEXT PRIMARY KEY,
    entity      TEXT NOT NULL,
    entity_id   TEXT,
    legacy_ref  TEXT,
    code        TEXT NOT NULL,
    severity    TEXT NOT NULL,
    field       TEXT,
    detail      TEXT,
    created_at  TEXT NOT NULL,
    company_id  TEXT
);

CREATE INDEX IF NOT EXISTS import_issues_entity_idx   ON import_issues (entity, entity_id);
CREATE INDEX IF NOT EXISTS import_issues_code_idx     ON import_issues (code);
CREATE INDEX IF NOT EXISTS import_issues_severity_idx ON import_issues (severity);
