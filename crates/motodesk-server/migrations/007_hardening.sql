-- Document numbering, audit trail, company_id on business rows, unique sale refs.

ALTER TABLE clients ADD COLUMN company_id TEXT;
ALTER TABLE vehicles ADD COLUMN company_id TEXT;
ALTER TABLE categories ADD COLUMN company_id TEXT;
ALTER TABLE products ADD COLUMN company_id TEXT;
ALTER TABLE app_settings ADD COLUMN company_id TEXT;
ALTER TABLE doc_templates ADD COLUMN company_id TEXT;
ALTER TABLE sales ADD COLUMN company_id TEXT;
ALTER TABLE sale_lines ADD COLUMN company_id TEXT;
ALTER TABLE change_log ADD COLUMN company_id TEXT;

UPDATE clients SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;
UPDATE vehicles SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;
UPDATE categories SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;
UPDATE products SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;
UPDATE app_settings SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;
UPDATE doc_templates SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;
UPDATE sales SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;
UPDATE sale_lines SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;
UPDATE change_log SET company_id = (SELECT company_id FROM sync_state WHERE id = 1) WHERE company_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS sales_company_reference_idx
    ON sales (company_id, reference)
    WHERE reference IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS document_counters (
    company_id TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('invoice', 'quote')),
    year       INTEGER NOT NULL,
    next_value INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (company_id, kind, year)
);

CREATE TABLE IF NOT EXISTS audit_log (
    id          TEXT PRIMARY KEY,
    company_id  TEXT,
    actor_id    TEXT,
    action      TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id   TEXT,
    payload     TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS audit_log_created_idx ON audit_log (created_at);
