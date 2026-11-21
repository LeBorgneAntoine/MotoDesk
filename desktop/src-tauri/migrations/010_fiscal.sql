-- The fiscal tier: an append-only journal that satisfies CGI art. 286 I-3 bis.
--
-- The business tables above this one are last-write-wins replicas: any peer may
-- rewrite any field, and rows soft-delete. That is the right model for a client
-- record and exactly the wrong one for an invoice, which BOI-TVA-DECLA-30-10-30
-- § 90 says may only be corrected by an opposite entry, never edited in place.
--
-- So validated documents leave a second, parallel trace here. Each register (one
-- per device) owns a chain: every row carries the digest of its own payload and
-- the digest of the row before it, signed with a key held in the OS keychain.
-- Editing one row invalidates every row after it, and the break is locatable.
--
-- Nothing in this file is ever updated or deleted. Triggers enforce that even
-- against direct SQL, so the admin console cannot quietly rewrite history.

-- One row per device that issues documents. `code` is the segment that appears
-- in every number it hands out (FA-2026-C1-00042), which is what lets two
-- offline peers both keep an unbroken sequence without ever talking.
CREATE TABLE IF NOT EXISTS fiscal_registers (
    id               TEXT PRIMARY KEY,
    company_id       TEXT,
    code             TEXT NOT NULL,
    label            TEXT,
    device_id        TEXT,
    -- Verifying half of the signing key. The private half lives in the keychain
    -- and is never written to the database or to a backup.
    public_key       TEXT NOT NULL,
    -- 'keychain' or 'database'; see fiscal_key_fallback.
    key_store        TEXT NOT NULL DEFAULT 'keychain',
    software_version TEXT NOT NULL,
    -- § 273: counters restart at zero for a new software. The recovered Motogest
    -- figures are archived separately rather than seeded here.
    opened_at        TEXT NOT NULL,
    closed_at        TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS fiscal_registers_code_idx
    ON fiscal_registers (company_id, code);

-- The journal fiscal permanent. Chronological, exhaustive, inalterable.
CREATE TABLE IF NOT EXISTS fiscal_journal (
    id           TEXT PRIMARY KEY,
    register_id  TEXT NOT NULL REFERENCES fiscal_registers(id),
    -- Unbroken from 1 within a register. A hole is a detected break, not a gap
    -- to be tolerated.
    seq          INTEGER NOT NULL,
    event_type   TEXT NOT NULL,
    -- RFC 3339 UTC to the second; § 220 asks for the dated detail of every
    -- operation down to the minute.
    occurred_at  TEXT NOT NULL,
    -- Local calendar day the entry belongs to, so closures can be cut on the
    -- shop's day rather than on UTC.
    business_day TEXT NOT NULL,
    payload      TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    prev_hash    TEXT NOT NULL,
    chain_hash   TEXT NOT NULL,
    signature    TEXT NOT NULL DEFAULT '',
    -- Convenience columns duplicated out of the payload for querying. The
    -- payload remains the authority; these are never hashed.
    document_ref TEXT,
    sale_id      TEXT,
    total_ttc_cents INTEGER NOT NULL DEFAULT 0,
    actor_id     TEXT,
    company_id   TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS fiscal_journal_seq_idx
    ON fiscal_journal (register_id, seq);
CREATE UNIQUE INDEX IF NOT EXISTS fiscal_journal_chain_idx
    ON fiscal_journal (chain_hash);
CREATE INDEX IF NOT EXISTS fiscal_journal_day_idx  ON fiscal_journal (business_day);
CREATE INDEX IF NOT EXISTS fiscal_journal_sale_idx ON fiscal_journal (sale_id);
CREATE INDEX IF NOT EXISTS fiscal_journal_ref_idx  ON fiscal_journal (document_ref);
CREATE INDEX IF NOT EXISTS fiscal_journal_type_idx ON fiscal_journal (event_type, occurred_at);

-- History is append-only, including from the SQL console.
CREATE TRIGGER IF NOT EXISTS fiscal_journal_no_update
BEFORE UPDATE ON fiscal_journal
BEGIN
    SELECT RAISE(ABORT, 'fiscal_journal is append-only (CGI art. 286 I-3 bis)');
END;

CREATE TRIGGER IF NOT EXISTS fiscal_journal_no_delete
BEFORE DELETE ON fiscal_journal
BEGIN
    SELECT RAISE(ABORT, 'fiscal_journal is append-only (CGI art. 286 I-3 bis)');
END;

-- § 170: daily, monthly and annual closures are cumulative and mandatory.
-- § 260: these totals stay online even after the detail behind them is purged.
CREATE TABLE IF NOT EXISTS fiscal_closures (
    id            TEXT PRIMARY KEY,
    register_id   TEXT NOT NULL REFERENCES fiscal_registers(id),
    kind          TEXT NOT NULL CHECK (kind IN ('day', 'month', 'year')),
    period_start  TEXT NOT NULL,
    period_end    TEXT NOT NULL,
    document_count   INTEGER NOT NULL DEFAULT 0,
    first_seq        INTEGER NOT NULL DEFAULT 0,
    last_seq         INTEGER NOT NULL DEFAULT 0,
    total_ht_cents   INTEGER NOT NULL DEFAULT 0,
    total_vat_cents  INTEGER NOT NULL DEFAULT 0,
    total_ttc_cents  INTEGER NOT NULL DEFAULT 0,
    -- One entry per VAT rate: [{"rate_bp":2000,"base_ht_cents":...}, ...]
    vat_breakdown    TEXT NOT NULL DEFAULT '[]',
    -- Turnover since the accounting period opened.
    period_grand_total_cents INTEGER NOT NULL DEFAULT 0,
    -- Turnover since the software was first used. Never resets.
    perpetual_total_cents    INTEGER NOT NULL DEFAULT 0,
    -- The journal entry that sealed this closure.
    journal_seq   INTEGER NOT NULL,
    chain_hash    TEXT NOT NULL,
    closed_at     TEXT NOT NULL,
    actor_id      TEXT,
    company_id    TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS fiscal_closures_period_idx
    ON fiscal_closures (register_id, kind, period_start);
CREATE INDEX IF NOT EXISTS fiscal_closures_end_idx ON fiscal_closures (kind, period_end);

CREATE TRIGGER IF NOT EXISTS fiscal_closures_no_update
BEFORE UPDATE ON fiscal_closures
BEGIN
    SELECT RAISE(ABORT, 'a sealed closure cannot be changed (BOI-TVA-DECLA-30-10-30 § 170)');
END;

CREATE TRIGGER IF NOT EXISTS fiscal_closures_no_delete
BEFORE DELETE ON fiscal_closures
BEGIN
    SELECT RAISE(ABORT, 'a sealed closure cannot be purged (BOI-TVA-DECLA-30-10-30 § 170)');
END;

-- The running tail of each chain plus the two counters. Unlike everything else
-- in this file it is updated, because it is a cursor rather than a record: the
-- history it summarises is in fiscal_journal and can rebuild it.
CREATE TABLE IF NOT EXISTS fiscal_counters (
    register_id  TEXT PRIMARY KEY REFERENCES fiscal_registers(id),
    last_seq     INTEGER NOT NULL DEFAULT 0,
    last_chain_hash TEXT NOT NULL,
    last_occurred_at TEXT NOT NULL DEFAULT '',
    period_grand_total_cents INTEGER NOT NULL DEFAULT 0,
    perpetual_total_cents    INTEGER NOT NULL DEFAULT 0,
    period_opened_on TEXT,
    updated_at   TEXT NOT NULL
);

-- § 240: the production of an archive is itself traced and the trace kept.
CREATE TABLE IF NOT EXISTS fiscal_archives (
    id           TEXT PRIMARY KEY,
    register_id  TEXT NOT NULL REFERENCES fiscal_registers(id),
    period_start TEXT NOT NULL,
    period_end   TEXT NOT NULL,
    file_path    TEXT NOT NULL,
    file_sha256  TEXT NOT NULL,
    file_bytes   INTEGER NOT NULL DEFAULT 0,
    entry_count  INTEGER NOT NULL DEFAULT 0,
    first_seq    INTEGER NOT NULL DEFAULT 0,
    last_seq     INTEGER NOT NULL DEFAULT 0,
    -- § 230 requires a French-language notice inside the archive.
    notice_included INTEGER NOT NULL DEFAULT 1,
    purged       INTEGER NOT NULL DEFAULT 0,
    journal_seq  INTEGER NOT NULL,
    created_at   TEXT NOT NULL,
    actor_id     TEXT,
    company_id   TEXT
);

CREATE INDEX IF NOT EXISTS fiscal_archives_period_idx
    ON fiscal_archives (register_id, period_start);

CREATE TRIGGER IF NOT EXISTS fiscal_archives_no_delete
BEFORE DELETE ON fiscal_archives
BEGIN
    SELECT RAISE(ABORT, 'archive traces must be kept (BOI-TVA-DECLA-30-10-30 § 240)');
END;

-- Signing keys belong in the OS keychain, so that holding a copy of this file
-- is not enough to forge a chain. When no keychain is available the key lands
-- here instead and fiscal_registers.key_store records that it did, because an
-- auditor should be able to tell the two situations apart.
CREATE TABLE IF NOT EXISTS fiscal_key_fallback (
    register_code TEXT PRIMARY KEY,
    secret_key    TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

-- Per-register, per-year document numbering. Replaces the company-wide
-- document_counters, which handed the same number to two offline peers.
CREATE TABLE IF NOT EXISTS fiscal_number_series (
    register_id TEXT NOT NULL REFERENCES fiscal_registers(id),
    kind        TEXT NOT NULL CHECK (kind IN ('invoice', 'credit_note', 'quote', 'receipt')),
    year        INTEGER NOT NULL,
    next_value  INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (register_id, kind, year)
);

-- Link the mutable sale back to the entry that froze it. Once fiscal_seq is
-- set the row is closed to editing, in the application and in the merge path.
ALTER TABLE sales ADD COLUMN fiscal_register_id TEXT;
ALTER TABLE sales ADD COLUMN fiscal_seq INTEGER;
ALTER TABLE sales ADD COLUMN locked_at TEXT;
-- Set on the credit note, pointing at the document it corrects.
ALTER TABLE sales ADD COLUMN corrects_sale_id TEXT;
ALTER TABLE sales ADD COLUMN total_vat_cents INTEGER;
ALTER TABLE sales ADD COLUMN document_kind TEXT NOT NULL DEFAULT 'invoice';

CREATE INDEX IF NOT EXISTS sales_locked_idx   ON sales (locked_at);
CREATE INDEX IF NOT EXISTS sales_corrects_idx ON sales (corrects_sale_id);

-- Payments existed as a table with no code behind it; these columns tie each
-- receipt to the journal entry that recorded it.
ALTER TABLE payments ADD COLUMN fiscal_register_id TEXT;
ALTER TABLE payments ADD COLUMN fiscal_seq INTEGER;
ALTER TABLE payments ADD COLUMN reverses_payment_id TEXT;

-- A sale is settled by the payments attached to it.
CREATE VIEW IF NOT EXISTS sale_settlement AS
SELECT
    s.id                                        AS sale_id,
    s.reference                                 AS reference,
    s.total_ttc_cents                           AS total_ttc_cents,
    COALESCE(SUM(p.amount_cents), 0)            AS paid_cents,
    s.total_ttc_cents - COALESCE(SUM(p.amount_cents), 0) AS outstanding_cents
FROM sales s
LEFT JOIN payments p
       ON p.sale_id = s.id AND p.deleted_at IS NULL
WHERE s.deleted_at IS NULL
GROUP BY s.id, s.reference, s.total_ttc_cents;
