-- Till sessions. The fiscal journal remains the source of truth; this table is
-- the working copy a cashier uses to open and close a shift. Closing a session
-- does not replace a daily closure: it records the drawer count so the Z report
-- can then call the existing J/M/A machinery.

CREATE TABLE IF NOT EXISTS pos_sessions (
    id                      TEXT PRIMARY KEY,
    register_id             TEXT NOT NULL,
    opened_at               TEXT NOT NULL,
    opened_by               TEXT,
    opening_float_cents     INTEGER NOT NULL DEFAULT 0,
    closed_at               TEXT,
    closed_by               TEXT,
    closing_counted_cents   INTEGER,
    expected_cash_cents     INTEGER,
    variance_cents          INTEGER,
    fiscal_open_seq         INTEGER,
    fiscal_close_seq        INTEGER
);

CREATE INDEX IF NOT EXISTS pos_sessions_register_open_idx
    ON pos_sessions (register_id, closed_at);
