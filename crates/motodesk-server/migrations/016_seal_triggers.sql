-- A sealed document is the legal record (CGI art. 286 I-3° bis). The application
-- already refuses to edit one, but the data manager, the SQL console, the CSV
-- import, a cloud resync or a replicated peer all write to the same tables, and
-- each of them was able to rewrite or delete a sealed invoice. These triggers
-- make the rule a property of the database instead of a habit of each caller.
--
-- Triggers only fire when a *protected* value would change: re-saving the same
-- row (an idempotent upsert from a peer, a status moving to 'paid', a version
-- bump) is untouched.

-- ---------------------------------------------------------------- sales -----

CREATE TRIGGER IF NOT EXISTS sales_sealed_no_delete
BEFORE DELETE ON sales
WHEN OLD.locked_at IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'document scellé : suppression interdite (émettez un avoir)');
END;

CREATE TRIGGER IF NOT EXISTS sales_sealed_no_rewrite
BEFORE UPDATE ON sales
WHEN OLD.locked_at IS NOT NULL AND (
       NEW.locked_at          IS NOT OLD.locked_at
    OR NEW.reference          IS NOT OLD.reference
    OR NEW.document_kind      IS NOT OLD.document_kind
    OR NEW.total_ht_cents     IS NOT OLD.total_ht_cents
    OR NEW.total_vat_cents    IS NOT OLD.total_vat_cents
    OR NEW.total_ttc_cents    IS NOT OLD.total_ttc_cents
    OR NEW.fiscal_register_id IS NOT OLD.fiscal_register_id
    OR NEW.fiscal_seq         IS NOT OLD.fiscal_seq
    OR NEW.sale_date          IS NOT OLD.sale_date
    OR NEW.corrects_sale_id   IS NOT OLD.corrects_sale_id
    OR NEW.deleted_at         IS NOT OLD.deleted_at
    OR (NEW.client_id IS NOT OLD.client_id AND NEW.client_id IS NOT NULL)
    OR NEW.status IN ('draft', 'quote')
)
BEGIN
    SELECT RAISE(ABORT, 'document scellé : modification interdite (émettez un avoir)');
END;

-- ---------------------------------------------------------- sale_lines ------
-- Inserts are left alone on purpose: a peer replicates the sealed sale and its
-- lines as separate operations, in either order. Changing or removing an
-- existing line of a sealed sale is what falsifies the document.

CREATE TRIGGER IF NOT EXISTS sale_lines_sealed_no_delete
BEFORE DELETE ON sale_lines
WHEN OLD.sale_id IN (SELECT id FROM sales WHERE locked_at IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'ligne d''un document scellé : suppression interdite');
END;

CREATE TRIGGER IF NOT EXISTS sale_lines_sealed_no_rewrite
BEFORE UPDATE ON sale_lines
WHEN OLD.sale_id IN (SELECT id FROM sales WHERE locked_at IS NOT NULL) AND (
       NEW.sale_id              IS NOT OLD.sale_id
    OR NEW.product_id           IS NOT OLD.product_id
    OR NEW.label                IS NOT OLD.label
    OR NEW.quantity_milli       IS NOT OLD.quantity_milli
    OR NEW.unit_price_ht_cents  IS NOT OLD.unit_price_ht_cents
    OR NEW.unit_price_ttc_cents IS NOT OLD.unit_price_ttc_cents
    OR NEW.vat_rate_bp          IS NOT OLD.vat_rate_bp
    OR NEW.discount_bp          IS NOT OLD.discount_bp
    OR NEW.line_total_ttc_cents IS NOT OLD.line_total_ttc_cents
    OR NEW.deleted_at           IS NOT OLD.deleted_at
)
BEGIN
    SELECT RAISE(ABORT, 'ligne d''un document scellé : modification interdite');
END;

-- ------------------------------------------------------------- payments -----
-- A payment that carries a fiscal sequence number is in the journal. A mistake
-- is corrected by a reversing payment, never by editing the original.

CREATE TRIGGER IF NOT EXISTS payments_sealed_no_delete
BEFORE DELETE ON payments
WHEN OLD.fiscal_seq IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'règlement scellé : suppression interdite (annulez-le par un règlement inverse)');
END;

CREATE TRIGGER IF NOT EXISTS payments_sealed_no_rewrite
BEFORE UPDATE ON payments
WHEN OLD.fiscal_seq IS NOT NULL AND (
       NEW.amount_cents        IS NOT OLD.amount_cents
    OR NEW.method              IS NOT OLD.method
    OR NEW.paid_at             IS NOT OLD.paid_at
    OR NEW.sale_id             IS NOT OLD.sale_id
    OR NEW.fiscal_seq          IS NOT OLD.fiscal_seq
    OR NEW.fiscal_register_id  IS NOT OLD.fiscal_register_id
    OR NEW.reverses_payment_id IS NOT OLD.reverses_payment_id
    OR NEW.deleted_at          IS NOT OLD.deleted_at
)
BEGIN
    SELECT RAISE(ABORT, 'règlement scellé : modification interdite');
END;
