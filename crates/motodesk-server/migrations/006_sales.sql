-- Local SQLite mirror of sales + sale_lines (see backend 0011_sales.sql).
-- Money is integer cents, quantity is integer thousandths (quantity_milli).

CREATE TABLE IF NOT EXISTS sales (
    id              TEXT PRIMARY KEY,
    client_id       TEXT REFERENCES clients(id)  ON DELETE SET NULL,
    vehicle_id      TEXT REFERENCES vehicles(id) ON DELETE SET NULL,
    reference       TEXT,
    status          TEXT NOT NULL DEFAULT 'invoice',
    sale_date       TEXT,
    total_ht_cents  INTEGER,
    total_ttc_cents INTEGER,
    payment_method  TEXT,
    notes           TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    row_version     INTEGER NOT NULL DEFAULT 1,
    deleted_at      TEXT,
    field_meta      TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS sales_reference_idx ON sales (reference);
CREATE INDEX IF NOT EXISTS sales_client_idx ON sales (client_id);

CREATE TABLE IF NOT EXISTS sale_lines (
    id                   TEXT PRIMARY KEY,
    sale_id              TEXT REFERENCES sales(id)    ON DELETE CASCADE,
    product_id           TEXT REFERENCES products(id) ON DELETE SET NULL,
    label                TEXT,
    quantity_milli       INTEGER NOT NULL DEFAULT 1000,
    unit_price_ht_cents  INTEGER,
    unit_price_ttc_cents INTEGER,
    vat_rate_bp          INTEGER NOT NULL DEFAULT 2000,
    discount_bp          INTEGER NOT NULL DEFAULT 0,
    line_total_ttc_cents INTEGER,
    position             INTEGER NOT NULL DEFAULT 0,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    row_version          INTEGER NOT NULL DEFAULT 1,
    deleted_at           TEXT,
    field_meta           TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS sale_lines_sale_idx ON sale_lines (sale_id);
CREATE INDEX IF NOT EXISTS sale_lines_product_idx ON sale_lines (product_id);
