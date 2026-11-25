-- Workshop Kanban, stock ledger, and backup metadata.

ALTER TABLE workshop_orders ADD COLUMN status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE workshop_orders ADD COLUMN assigned_to TEXT;

CREATE INDEX IF NOT EXISTS workshop_orders_status_idx ON workshop_orders (status)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS stock_movements (
    id                  TEXT PRIMARY KEY,
    product_id          TEXT NOT NULL REFERENCES products(id),
    supplier_id         TEXT REFERENCES suppliers(id) ON DELETE SET NULL,
    kind                TEXT NOT NULL,
    quantity_milli      INTEGER NOT NULL,
    unit_cost_ht_cents  INTEGER,
    reason              TEXT,
    sale_id             TEXT REFERENCES sales(id) ON DELETE SET NULL,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    row_version          INTEGER NOT NULL DEFAULT 1,
    deleted_at          TEXT,
    field_meta          TEXT NOT NULL DEFAULT '{}',
    company_id          TEXT
);

CREATE INDEX IF NOT EXISTS stock_movements_product_idx ON stock_movements (product_id);
CREATE INDEX IF NOT EXISTS stock_movements_sale_idx ON stock_movements (sale_id);

ALTER TABLE sync_state ADD COLUMN last_backup_at TEXT;
