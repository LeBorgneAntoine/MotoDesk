-- The network's recovery key: what gets an owner back in when no administrator's password is
-- known. Only a hash is kept, never the key. It is replicated with the accounts, so the key
-- works on any PC of the network (newest `updated_at` wins).
--   confirmed : someone typed it back, so it was really written down.
CREATE TABLE IF NOT EXISTS network_recovery (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    key_hash   TEXT NOT NULL,
    confirmed  INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
