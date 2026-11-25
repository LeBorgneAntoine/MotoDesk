-- Stock is held in thousandths of a unit, like every quantity on a document.
--
-- It was a whole number of units, so a metre of cable or 2,5 L of oil could be sold
-- (the ledger recorded 2 500) while the shelf moved by 2 (or by nothing at all for
-- 0,5). The stock of a product is the sum of its movements; both are now in the same
-- unit. The column is renamed rather than added to, so nothing can go on reading it
-- as units by mistake.
ALTER TABLE products RENAME COLUMN stock_quantity TO stock_milli;
UPDATE products SET stock_milli = stock_milli * 1000;
