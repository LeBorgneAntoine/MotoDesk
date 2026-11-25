-- Which PCs are in the shop's local network, and which version of its key this PC holds.
--
-- Removing a PC (lost, stolen) gives the network a new secret: `key_epoch` counts them, so a
-- PC left with an older one knows it must be reconnected.
ALTER TABLE sync_state ADD COLUMN key_epoch INTEGER NOT NULL DEFAULT 1;

-- Each PC records itself and the list is replicated with the accounts.
--   removed_at : set when an administrator removed the PC, cleared if it is reconnected;
--   status_at  : when `removed_at` last changed, so the newest decision wins.
CREATE TABLE IF NOT EXISTS network_devices (
    device_id  TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    first_seen TEXT NOT NULL,
    last_seen  TEXT NOT NULL,
    removed_at TEXT,
    status_at  TEXT NOT NULL
);
