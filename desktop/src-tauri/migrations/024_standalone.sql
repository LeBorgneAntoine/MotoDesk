-- Local network mode: a shop's PCs form a mesh with no cloud behind it.
--
-- `sync_state.standalone` says the installation is run that way. Going online turns the
-- flag off once the cloud has accepted the company under the ids it already has.
ALTER TABLE sync_state ADD COLUMN standalone INTEGER NOT NULL DEFAULT 0;

-- How far this PC has pushed its change log to its LAN peers. With no cloud nothing is ever
-- acknowledged upstream, so "everything not yet synced" never shrinks; a cursor does.
ALTER TABLE sync_state ADD COLUMN lan_pushed_rowid INTEGER NOT NULL DEFAULT 0;

-- The network's accounts. They are replicated between the PCs (newest `updated_at` wins;
-- `deleted_at` is a tombstone), so an account created on one PC signs in on the others.
--   password_hash : Argon2id, to sign in on a PC that holds the account;
--   kdf_salt, srp_verifier : to prove a password to a PC on the LAN without ever sending it
--                            (SRP-6a, with an Argon2id-derived secret).
CREATE TABLE IF NOT EXISTS local_accounts (
    id            TEXT PRIMARY KEY,
    username      TEXT NOT NULL COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    kdf_salt      BLOB NOT NULL,
    srp_verifier  BLOB NOT NULL,
    role          TEXT NOT NULL DEFAULT 'admin',
    full_name     TEXT,
    is_active     INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    deleted_at    TEXT
);

-- A username is unique among the accounts that still exist.
CREATE UNIQUE INDEX IF NOT EXISTS idx_local_accounts_username
    ON local_accounts (username) WHERE deleted_at IS NULL;
