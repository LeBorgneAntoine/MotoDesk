-- Product variants: each colour / size / etc. is a full catalogue row so
-- stock, sales and the till keep pointing at a single product id.
-- parent_id is the model (BIKE BMW); a variant of a variant is stored as a
-- sibling of the original parent.

ALTER TABLE products ADD COLUMN parent_id TEXT REFERENCES products(id) ON DELETE SET NULL;
ALTER TABLE products ADD COLUMN variant_label TEXT;
ALTER TABLE products ADD COLUMN variant_attrs TEXT;

CREATE INDEX IF NOT EXISTS products_parent_id_idx
    ON products(parent_id)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;
