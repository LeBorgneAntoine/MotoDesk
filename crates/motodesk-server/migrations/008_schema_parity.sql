-- Per-company uniqueness and CHECK-style triggers for SQLite (INTEGER is already 64-bit).

CREATE UNIQUE INDEX IF NOT EXISTS products_company_sku_idx
    ON products (company_id, sku) WHERE sku IS NOT NULL AND deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS products_company_vin_idx
    ON products (company_id, vin_or_serial) WHERE vin_or_serial IS NOT NULL AND deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS categories_company_name_idx
    ON categories (company_id, name) WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS categories_company_slug_idx
    ON categories (company_id, slug) WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS vehicles_company_vin_idx
    ON vehicles (company_id, vin_or_serial) WHERE vin_or_serial IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS change_log_created_idx ON change_log (created_at);
