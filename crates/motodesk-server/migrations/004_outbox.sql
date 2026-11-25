-- Sync engine outbox state: tracks the last push attempt, error, and any
-- exponential backoff window. Combined with the partial index below, this
-- makes drain_cloud O(unsynced) regardless of total change_log size.

ALTER TABLE sync_state ADD COLUMN last_cloud_push_at TEXT;
ALTER TABLE sync_state ADD COLUMN last_cloud_push_error TEXT;
ALTER TABLE sync_state ADD COLUMN cloud_backoff_until TEXT;
ALTER TABLE sync_state ADD COLUMN last_lan_push_at TEXT;
ALTER TABLE sync_state ADD COLUMN lan_peers_static TEXT; -- JSON array of "host:port"

CREATE INDEX IF NOT EXISTS change_log_unsynced_idx
    ON change_log (created_at)
    WHERE synced_to_cloud = 0;
