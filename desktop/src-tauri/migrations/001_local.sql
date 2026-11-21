-- Local SQLite mirror + sync tables

CREATE TABLE IF NOT EXISTS sync_state (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    device_id           TEXT NOT NULL,
    last_cloud_cursor   TEXT,
    last_seq            INTEGER NOT NULL DEFAULT 0,
    local_port          INTEGER,
    cloud_url           TEXT,
    mesh_psk            TEXT,
    cloud_relay_enabled INTEGER NOT NULL DEFAULT 1
);

INSERT OR IGNORE INTO sync_state (id, device_id) VALUES (1, lower(hex(randomblob(16))));

CREATE TABLE IF NOT EXISTS change_log (
    id              TEXT PRIMARY KEY,
    device_id       TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    entity_type     TEXT NOT NULL,
    entity_id       TEXT NOT NULL,
    op              TEXT NOT NULL,
    payload         TEXT NOT NULL,
    field_meta      TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL,
    synced_to_cloud INTEGER NOT NULL DEFAULT 0,
    relayed         INTEGER NOT NULL DEFAULT 0,
    UNIQUE (device_id, seq)
);

CREATE INDEX IF NOT EXISTS change_log_synced_idx ON change_log (synced_to_cloud);
CREATE INDEX IF NOT EXISTS change_log_created_idx ON change_log (created_at);

CREATE TABLE IF NOT EXISTS cached_session (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    user_json       TEXT NOT NULL,
    tokens_json     TEXT NOT NULL,
    cached_at       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id              TEXT PRIMARY KEY,
    email           TEXT NOT NULL,
    full_name       TEXT NOT NULL,
    role            TEXT NOT NULL DEFAULT 'staff',
    is_active       INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    row_version     INTEGER NOT NULL DEFAULT 1,
    deleted_at      TEXT,
    field_meta      TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS clients (
    id              TEXT PRIMARY KEY,
    first_name      TEXT,
    last_name       TEXT NOT NULL,
    phone           TEXT,
    email           TEXT,
    address         TEXT,
    notes           TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    row_version     INTEGER NOT NULL DEFAULT 1,
    deleted_at      TEXT,
    field_meta      TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS clients_last_name_idx ON clients (last_name);

CREATE TABLE IF NOT EXISTS vehicles (
    id                      TEXT PRIMARY KEY,
    client_id               TEXT REFERENCES clients(id) ON DELETE SET NULL,
    vehicle_type            TEXT NOT NULL DEFAULT 'moto',
    make                    TEXT NOT NULL,
    model                   TEXT NOT NULL,
    year                    INTEGER,
    cylinder_capacity       INTEGER,
    vin_or_serial           TEXT UNIQUE,
    plate_number            TEXT,
    odometer_value          INTEGER NOT NULL DEFAULT 0,
    odometer_unit           TEXT NOT NULL DEFAULT 'km',
    is_stock                INTEGER NOT NULL DEFAULT 0,
    purchase_price_ht_cents INTEGER,
    selling_price_ttc_cents INTEGER,
    notes                   TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    row_version             INTEGER NOT NULL DEFAULT 1,
    deleted_at              TEXT,
    field_meta              TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS categories (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    slug            TEXT NOT NULL UNIQUE,
    position        INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    row_version     INTEGER NOT NULL DEFAULT 1,
    deleted_at      TEXT,
    field_meta      TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS products (
    id                       TEXT PRIMARY KEY,
    category_id              TEXT REFERENCES categories(id) ON DELETE SET NULL,
    name                     TEXT NOT NULL,
    sku                      TEXT UNIQUE,
    brand                    TEXT,
    description              TEXT,
    unit                     TEXT NOT NULL DEFAULT 'piece',
    is_service               INTEGER NOT NULL DEFAULT 0,
    purchase_price_ht_cents  INTEGER,
    selling_price_ttc_cents  INTEGER,
    vat_rate_bp              INTEGER NOT NULL DEFAULT 2000,
    stock_quantity           INTEGER NOT NULL DEFAULT 0,
    low_stock_threshold      INTEGER,
    is_active                INTEGER NOT NULL DEFAULT 1,
    vehicle_type             TEXT,
    year                     INTEGER,
    cylinder_capacity        INTEGER,
    vin_or_serial            TEXT UNIQUE,
    plate_number             TEXT,
    odometer_value           INTEGER,
    odometer_unit            TEXT,
    notes                    TEXT,
    created_at               TEXT NOT NULL,
    updated_at               TEXT NOT NULL,
    row_version              INTEGER NOT NULL DEFAULT 1,
    deleted_at               TEXT,
    field_meta               TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS app_settings (
    key             TEXT PRIMARY KEY,
    value           TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    row_version     INTEGER NOT NULL DEFAULT 1,
    deleted_at      TEXT,
    field_meta      TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS doc_templates (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    kind                TEXT NOT NULL,
    page_size           TEXT NOT NULL DEFAULT 'A4',
    orientation         TEXT NOT NULL DEFAULT 'portrait',
    schema              TEXT NOT NULL,
    thumbnail_data_url  TEXT,
    is_default          INTEGER NOT NULL DEFAULT 0,
    is_archived         INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    row_version         INTEGER NOT NULL DEFAULT 1,
    deleted_at          TEXT,
    field_meta          TEXT NOT NULL DEFAULT '{}'
);

-- Seed categories (mirrors cloud migration)
INSERT OR IGNORE INTO categories (id, name, slug, position, created_at, updated_at) VALUES
    ('c0000001-0000-0000-0000-000000000001', 'Véhicules', 'vehicules', 10, datetime('now'), datetime('now')),
    ('c0000001-0000-0000-0000-000000000002', 'Pièces', 'pieces', 20, datetime('now'), datetime('now')),
    ('c0000001-0000-0000-0000-000000000003', 'Accessoires', 'accessoires', 30, datetime('now'), datetime('now')),
    ('c0000001-0000-0000-0000-000000000004', 'Consommables', 'consommables', 40, datetime('now'), datetime('now')),
    ('c0000001-0000-0000-0000-000000000005', 'Services', 'services', 50, datetime('now'), datetime('now'));
