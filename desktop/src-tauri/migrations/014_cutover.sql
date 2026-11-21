-- Encours, workshop lines, and sale lookup of Motogest pieces.

ALTER TABLE clients ADD COLUMN credit_limit_cents INTEGER;
ALTER TABLE clients ADD COLUMN credit_blocked INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS workshop_order_lines (
    id                   TEXT PRIMARY KEY,
    workshop_order_id   TEXT NOT NULL REFERENCES workshop_orders(id) ON DELETE CASCADE,
    product_id           TEXT REFERENCES products(id) ON DELETE SET NULL,
    label                TEXT,
    quantity_milli       INTEGER NOT NULL DEFAULT 1000,
    unit_price_ht_cents  INTEGER NOT NULL DEFAULT 0,
    vat_rate_bp          INTEGER NOT NULL DEFAULT 2000,
    discount_bp          INTEGER NOT NULL DEFAULT 0,
    position             INTEGER NOT NULL DEFAULT 0,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS workshop_order_lines_order_idx
    ON workshop_order_lines (workshop_order_id);
