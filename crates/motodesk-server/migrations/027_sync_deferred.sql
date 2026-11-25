-- Changes received from another PC that cannot be applied yet: typically what they refer
-- to has not arrived (a PC catching up is sent one device's changes, then another's). They
-- stay in the change log, are listed here, and are tried again after each batch received.
--   error : why the last try failed; NULL when the change is only waiting its turn.
CREATE TABLE IF NOT EXISTS sync_deferred (
    change_id   TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    error       TEXT,
    deferred_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sync_deferred_entity ON sync_deferred (entity_type, entity_id);
