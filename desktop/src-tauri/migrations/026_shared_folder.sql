-- The shared folder: a place for the shop's sensitive files (accounts, contracts, IDs).
--
-- Files are encrypted before they reach these tables (see `shared_folder/crypto.rs`): a random
-- key per file, itself wrapped by a key derived from the shop's network secret. Folder and file
-- names are not encrypted. Rows carry `updated_at` / `deleted_at` so that they can be replicated
-- between the PCs of the network (newest wins, deletions are tombstones).

CREATE TABLE IF NOT EXISTS shared_folders (
    id          TEXT PRIMARY KEY,
    parent_id   TEXT REFERENCES shared_folders(id),
    name        TEXT NOT NULL,
    -- 'admins': only administrators see it. 'everyone': every signed-in user does. A folder is
    -- reachable only when it and all its parents are visible to the user.
    visibility  TEXT NOT NULL DEFAULT 'admins' CHECK (visibility IN ('admins', 'everyone')),
    created_by  TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT
);
CREATE INDEX IF NOT EXISTS idx_shared_folders_parent ON shared_folders(parent_id);

CREATE TABLE IF NOT EXISTS shared_files (
    id           TEXT PRIMARY KEY,
    folder_id    TEXT NOT NULL REFERENCES shared_folders(id),
    name         TEXT NOT NULL,
    mime         TEXT,
    size_bytes   INTEGER NOT NULL,
    chunk_count  INTEGER NOT NULL,
    -- Names the stored content; a replaced content gets a new id.
    content_id   TEXT NOT NULL,
    -- The file's own key, wrapped by the shop key (nonce || ciphertext).
    wrapped_key  BLOB NOT NULL,
    key_version  INTEGER NOT NULL DEFAULT 1,
    -- 'uploading' until every chunk has arrived; only 'ready' files are listed.
    state        TEXT NOT NULL DEFAULT 'uploading' CHECK (state IN ('uploading', 'ready')),
    created_by   TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    deleted_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_shared_files_folder ON shared_files(folder_id);

CREATE TABLE IF NOT EXISTS shared_file_chunks (
    content_id  TEXT NOT NULL,
    idx         INTEGER NOT NULL,
    -- nonce || ciphertext || tag
    data        BLOB NOT NULL,
    PRIMARY KEY (content_id, idx)
) WITHOUT ROWID;

-- Who opened, added or removed what. Kept on this PC.
CREATE TABLE IF NOT EXISTS shared_audit (
    id         TEXT PRIMARY KEY,
    at         TEXT NOT NULL,
    user_id    TEXT,
    user_name  TEXT,
    action     TEXT NOT NULL,
    file_id    TEXT,
    folder_id  TEXT,
    detail     TEXT
);
CREATE INDEX IF NOT EXISTS idx_shared_audit_at ON shared_audit(at);
