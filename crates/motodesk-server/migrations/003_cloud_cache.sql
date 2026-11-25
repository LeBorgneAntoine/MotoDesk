ALTER TABLE sync_state ADD COLUMN cloud_reachable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sync_state ADD COLUMN cloud_checked_at TEXT;
