-- Parts picked from the catalogue on a repair order carry a shelf price (TTC);
-- NULL means the line was priced HT, as labour is. See 017 and motogest_fiscal::vat.
ALTER TABLE workshop_order_lines ADD COLUMN unit_price_ttc_cents INTEGER;
