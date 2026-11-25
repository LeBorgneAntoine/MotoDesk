-- A line is priced either HT (a workshop quote) or TTC (a shelf price). The two
-- cannot be converted into each other in whole cents -- 9,99 EUR at 20 % has no
-- net amount in cents -- so a TTC line must remember that its TTC price is the
-- price of record. `unit_price_ttc_cents` on its own cannot say that: older rows
-- also carry one, derived from the HT price.
ALTER TABLE sale_lines ADD COLUMN price_basis TEXT NOT NULL DEFAULT 'ht';
