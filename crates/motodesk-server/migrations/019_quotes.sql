-- A quote is kept when it is turned into an invoice or a repair order, and the new
-- document says where it came from. The quote itself is never rewritten into the
-- invoice (it used to be: validating a quote sealed that very row as an invoice,
-- so the quote, and the number it was sent under, ceased to exist).
ALTER TABLE sales ADD COLUMN source_quote_id TEXT;
CREATE INDEX IF NOT EXISTS sales_source_quote_idx ON sales (source_quote_id);

ALTER TABLE workshop_orders ADD COLUMN source_quote_id TEXT;
CREATE INDEX IF NOT EXISTS workshop_orders_source_quote_idx ON workshop_orders (source_quote_id);
