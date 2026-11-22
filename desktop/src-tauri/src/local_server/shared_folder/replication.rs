//! Keeping the shared folder the same on every PC of the shop's network.
//!
//! Each PC pulls from the others (and the others pull from it), so there is no leader and a PC
//! that was off simply catches up. Everything is signed with the network secret, like the
//! account exchange. Three steps, the first one usually the only one:
//!
//! 1. **Summary.** The asker sends a digest of what it holds. If the peer's is the same, that is
//!    all. If not, the peer answers with a stamp per folder and file (id, time of last change,
//!    deleted or not, a short hash), and the asker works out which rows it lacks or holds an
//!    older version of.
//! 2. **Rows.** It asks for those rows in full. Newest change wins; at the same instant a
//!    deletion wins, then the larger hash, so every PC picks the same one. Deletions travel as
//!    tombstones. A file arrives as `syncing`: known, but not listed or openable yet.
//! 3. **Chunks.** The encrypted chunks of `syncing` files are fetched one by one. Each is checked
//!    (decrypted with the file's key, which binds it to its file, position and size) before it is
//!    stored; when all are in, the file becomes `ready`. A copy interrupted halfway resumes.
//!
//! Nothing here decrypts for anyone but the integrity check: chunks stay sealed in transit and at
//! rest, and who may see what is still decided by the folder rules on each PC.

use std::collections::{HashMap, HashSet};

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::crypto::{self, ChunkContext};
use super::{clean_name, shop_key, MAX_FILE_BYTES};
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::local_accounts;
use crate::local_server::mesh::{catchup, PeerInfo};
use crate::local_server::state::SharedState;

/// Rows asked for in one request.
const ROW_BATCH: usize = 200;
/// Chunks fetched from one peer in one run; the rest waits for the next one.
const CHUNKS_PER_RUN: usize = 96;

// ------------------------------------------------------------------------------------- records ----

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq)]
pub struct FolderRecord {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub visibility: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq)]
pub struct FileRecord {
    pub id: String,
    pub folder_id: String,
    pub name: String,
    pub mime: Option<String>,
    pub size_bytes: i64,
    pub chunk_count: i64,
    pub content_id: String,
    /// The wrapped file key, hex.
    pub wrapped_key: String,
    pub key_version: i64,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// What one PC knows about one row: enough to tell who is newer without sending the row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stamp {
    pub id: String,
    pub updated_at: String,
    pub deleted: bool,
    /// Start of a hash of the row, to break a tie between two PCs that changed it at the same
    /// instant in different ways.
    pub h: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub folders: Vec<Stamp>,
    pub files: Vec<Stamp>,
}

const FOLDER_SQL: &str =
    "SELECT id, parent_id, name, visibility, created_by, created_at, updated_at, deleted_at FROM shared_folders";
// Files still being sent from here are nobody else's business yet.
const FILE_SQL: &str = "SELECT id, folder_id, name, mime, size_bytes, chunk_count, content_id,
       lower(hex(wrapped_key)) AS wrapped_key, key_version, created_by, created_at, updated_at, deleted_at
  FROM shared_files WHERE state != 'uploading'";

fn hash_of<T: Serialize>(row: &T) -> String {
    let bytes = serde_json::to_vec(row).unwrap_or_default();
    hex::encode(&Sha256::digest(bytes)[..8])
}

fn stamp_folder(r: &FolderRecord) -> Stamp {
    Stamp { id: r.id.clone(), updated_at: r.updated_at.clone(), deleted: r.deleted_at.is_some(), h: hash_of(r) }
}

fn stamp_file(r: &FileRecord) -> Stamp {
    Stamp { id: r.id.clone(), updated_at: r.updated_at.clone(), deleted: r.deleted_at.is_some(), h: hash_of(r) }
}

pub async fn local_summary(state: &SharedState) -> LocalResult<Summary> {
    let folders: Vec<FolderRecord> = sqlx::query_as(&format!("{FOLDER_SQL} ORDER BY id")).fetch_all(&state.db).await?;
    let files: Vec<FileRecord> = sqlx::query_as(&format!("{FILE_SQL} ORDER BY id")).fetch_all(&state.db).await?;
    Ok(Summary {
        folders: folders.iter().map(stamp_folder).collect(),
        files: files.iter().map(stamp_file).collect(),
    })
}

/// One short string that is the same on two PCs exactly when their summaries are.
pub fn digest(summary: &Summary) -> String {
    let mut hasher = Sha256::new();
    for (kind, stamps) in [("d", &summary.folders), ("f", &summary.files)] {
        let mut sorted: Vec<&Stamp> = stamps.iter().collect();
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        for s in sorted {
            hasher.update(format!("{kind}|{}|{}|{}|{}\n", s.id, s.updated_at, s.deleted, s.h));
        }
    }
    hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------------- who wins ----

fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

/// Whether the other PC's version of a row should replace ours.
pub fn remote_wins(local: &Stamp, remote: &Stamp) -> bool {
    match (parse_time(&remote.updated_at), parse_time(&local.updated_at)) {
        (Some(r), Some(l)) if r != l => r > l,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        _ => {
            if remote.deleted != local.deleted {
                remote.deleted
            } else {
                remote.h > local.h
            }
        }
    }
}

/// The ids of the rows in `theirs` that we lack or hold an older version of.
fn wanted(mine: &[Stamp], theirs: &[Stamp]) -> Vec<String> {
    let have: HashMap<&str, &Stamp> = mine.iter().map(|s| (s.id.as_str(), s)).collect();
    theirs
        .iter()
        .filter(|t| have.get(t.id.as_str()).is_none_or(|l| remote_wins(l, t)))
        .map(|t| t.id.clone())
        .collect()
}

// ---------------------------------------------------------------------------------- serving ----

pub async fn serve_summary(state: &SharedState, their_digest: &str) -> LocalResult<serde_json::Map<String, Value>> {
    shop_key(state).await?;
    let mine = local_summary(state).await?;
    let mut out = serde_json::Map::new();
    if digest(&mine) == their_digest {
        out.insert("same".into(), true.into());
    } else {
        out.insert("same".into(), false.into());
        out.insert("summary".into(), to_value(&mine)?);
    }
    Ok(out)
}

fn to_value<T: Serialize>(v: &T) -> LocalResult<Value> {
    serde_json::to_value(v).map_err(|e| LocalError::Internal(e.into()))
}

fn ids_of(fields: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    fields
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).take(ROW_BATCH).collect())
        .unwrap_or_default()
}

pub async fn serve_rows(
    state: &SharedState,
    fields: &serde_json::Map<String, Value>,
) -> LocalResult<serde_json::Map<String, Value>> {
    shop_key(state).await?;
    let folder_ids = ids_of(fields, "folders");
    let file_ids = ids_of(fields, "files");
    let mut folders = Vec::new();
    for id in &folder_ids {
        let row: Option<FolderRecord> = sqlx::query_as(&format!("{FOLDER_SQL} WHERE id = $1"))
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
        folders.extend(row);
    }
    let mut files = Vec::new();
    for id in &file_ids {
        let row: Option<FileRecord> = sqlx::query_as(&format!("{FILE_SQL} AND id = $1"))
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
        files.extend(row);
    }
    let mut out = serde_json::Map::new();
    out.insert("folders".into(), to_value(&folders)?);
    out.insert("files".into(), to_value(&files)?);
    Ok(out)
}

/// One encrypted chunk of a finished file. A file that is itself still being copied is not served.
pub async fn serve_chunk(
    state: &SharedState,
    fields: &serde_json::Map<String, Value>,
) -> LocalResult<serde_json::Map<String, Value>> {
    shop_key(state).await?;
    let content_id = fields.get("content_id").and_then(|v| v.as_str()).ok_or(LocalError::NotFound)?;
    let index = fields.get("idx").and_then(|v| v.as_i64()).ok_or(LocalError::NotFound)?;
    let ready: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM shared_files WHERE content_id = $1 AND state = 'ready' AND deleted_at IS NULL",
    )
    .bind(content_id)
    .fetch_optional(&state.db)
    .await?;
    if ready.is_none() {
        return Err(LocalError::NotFound);
    }
    let data: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT data FROM shared_file_chunks WHERE content_id = $1 AND idx = $2")
            .bind(content_id)
            .bind(index)
            .fetch_optional(&state.db)
            .await?;
    let (data,) = data.ok_or(LocalError::NotFound)?;
    let mut out = serde_json::Map::new();
    out.insert("data".into(), base64::engine::general_purpose::STANDARD.encode(data).into());
    Ok(out)
}

// ----------------------------------------------------------------------------------- applying ----

fn valid_time(s: &str) -> bool {
    parse_time(s).is_some()
}

fn valid_uuid(s: &str) -> bool {
    Uuid::parse_str(s).is_ok()
}

fn check_folder(r: &FolderRecord) -> Result<(), &'static str> {
    if !valid_uuid(&r.id) || r.parent_id.as_deref().is_some_and(|p| !valid_uuid(p)) {
        return Err("folder id");
    }
    if clean_name(&r.name).ok().as_deref() != Some(r.name.as_str()) {
        return Err("folder name");
    }
    if !matches!(r.visibility.as_str(), "admins" | "everyone") {
        return Err("folder visibility");
    }
    if !valid_time(&r.created_at) || !valid_time(&r.updated_at) || r.deleted_at.as_deref().is_some_and(|t| !valid_time(t)) {
        return Err("folder time");
    }
    Ok(())
}

fn check_file(r: &FileRecord) -> Result<(), &'static str> {
    if !valid_uuid(&r.id) || !valid_uuid(&r.folder_id) || !valid_uuid(&r.content_id) {
        return Err("file id");
    }
    if clean_name(&r.name).ok().as_deref() != Some(r.name.as_str()) {
        return Err("file name");
    }
    if r.size_bytes < 0 || r.size_bytes as u64 > MAX_FILE_BYTES || r.chunk_count != crypto::chunks_for(r.size_bytes as u64) as i64 {
        return Err("file size");
    }
    match hex::decode(&r.wrapped_key) {
        Ok(k) if k.len() >= 28 && k.len() <= 128 => {}
        _ => return Err("file key"),
    }
    if !valid_time(&r.created_at) || !valid_time(&r.updated_at) || r.deleted_at.as_deref().is_some_and(|t| !valid_time(t)) {
        return Err("file time");
    }
    Ok(())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub folders: usize,
    pub files: usize,
    pub chunks: usize,
}

impl Stats {
    pub fn changed(&self) -> bool {
        self.folders + self.files + self.chunks > 0
    }
}

/// Merge rows from a peer. Each row replaces ours only if it is newer (re-checked here, in case
/// it changed since the summary). Returns how many folders and files changed.
pub async fn apply_rows(
    state: &SharedState,
    folders: Vec<FolderRecord>,
    files: Vec<FileRecord>,
) -> LocalResult<Stats> {
    let mut stats = Stats::default();
    let mut tx = state.db.begin().await?;
    // A file can arrive before its folder, a folder before its parent: the order is checked once
    // everything is in.
    sqlx::query("PRAGMA defer_foreign_keys = ON").execute(&mut *tx).await?;

    for f in folders {
        if let Err(why) = check_folder(&f) {
            tracing::warn!(why, "a shared folder row from a peer was refused");
            continue;
        }
        let local: Option<FolderRecord> = sqlx::query_as(&format!("{FOLDER_SQL} WHERE id = $1"))
            .bind(&f.id)
            .fetch_optional(&mut *tx)
            .await?;
        if local.as_ref().is_some_and(|l| !remote_wins(&stamp_folder(l), &stamp_folder(&f))) {
            continue;
        }
        sqlx::query(
            "INSERT INTO shared_folders (id, parent_id, name, visibility, created_by, created_at, updated_at, deleted_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT(id) DO UPDATE SET parent_id = excluded.parent_id, name = excluded.name,
                 visibility = excluded.visibility, updated_at = excluded.updated_at, deleted_at = excluded.deleted_at",
        )
        .bind(&f.id)
        .bind(&f.parent_id)
        .bind(&f.name)
        .bind(&f.visibility)
        .bind(&f.created_by)
        .bind(&f.created_at)
        .bind(&f.updated_at)
        .bind(&f.deleted_at)
        .execute(&mut *tx)
        .await?;
        stats.folders += 1;
    }

    for f in files {
        if let Err(why) = check_file(&f) {
            tracing::warn!(why, "a shared file row from a peer was refused");
            continue;
        }
        let local: Option<FileRecord> = sqlx::query_as(&format!("{FILE_SQL} AND id = $1"))
            .bind(&f.id)
            .fetch_optional(&mut *tx)
            .await?;
        if local.as_ref().is_some_and(|l| !remote_wins(&stamp_file(l), &stamp_file(&f))) {
            continue;
        }
        let key = hex::decode(&f.wrapped_key).unwrap_or_default();

        if f.deleted_at.is_some() {
            // Gone everywhere: the encrypted content goes too, here and under any older content id.
            for content in [Some(f.content_id.clone()), local.as_ref().map(|l| l.content_id.clone())].into_iter().flatten() {
                sqlx::query("DELETE FROM shared_file_chunks WHERE content_id = $1")
                    .bind(content)
                    .execute(&mut *tx)
                    .await?;
            }
        } else if let Some(l) = local.as_ref().filter(|l| l.content_id != f.content_id) {
            // New content under the same file: the old one is useless.
            sqlx::query("DELETE FROM shared_file_chunks WHERE content_id = $1")
                .bind(&l.content_id)
                .execute(&mut *tx)
                .await?;
        }

        let have: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM shared_file_chunks WHERE content_id = $1")
            .bind(&f.content_id)
            .fetch_one(&mut *tx)
            .await?;
        let state_after = if f.deleted_at.is_some() || have.0 == f.chunk_count { "ready" } else { "syncing" };

        sqlx::query(
            "INSERT INTO shared_files (id, folder_id, name, mime, size_bytes, chunk_count, content_id, wrapped_key,
                                       key_version, state, created_by, created_at, updated_at, deleted_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
             ON CONFLICT(id) DO UPDATE SET folder_id = excluded.folder_id, name = excluded.name, mime = excluded.mime,
                 size_bytes = excluded.size_bytes, chunk_count = excluded.chunk_count, content_id = excluded.content_id,
                 wrapped_key = excluded.wrapped_key, key_version = excluded.key_version, state = excluded.state,
                 updated_at = excluded.updated_at, deleted_at = excluded.deleted_at",
        )
        .bind(&f.id)
        .bind(&f.folder_id)
        .bind(&f.name)
        .bind(&f.mime)
        .bind(f.size_bytes)
        .bind(f.chunk_count)
        .bind(&f.content_id)
        .bind(key)
        .bind(f.key_version)
        .bind(state_after)
        .bind(&f.created_by)
        .bind(&f.created_at)
        .bind(&f.updated_at)
        .bind(&f.deleted_at)
        .execute(&mut *tx)
        .await?;
        stats.files += 1;
    }
    tx.commit().await?;
    Ok(stats)
}

// ------------------------------------------------------------------------------------- asking ----

async fn ask(
    state: &SharedState,
    peer: &PeerInfo,
    path: &str,
    fields: serde_json::Map<String, Value>,
) -> LocalResult<serde_json::Map<String, Value>> {
    let secret = catchup::secret_of(state).await?;
    let reply = catchup::post(state, peer, path, catchup::signed(&secret, fields)).await?;
    catchup::verified(&secret, reply)
}

fn fields(pairs: Vec<(&str, Value)>) -> serde_json::Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Bring this PC's shared folder level with a peer's: metadata first, then the missing content.
pub async fn sync_with(state: &SharedState, peer: &PeerInfo) -> LocalResult<Stats> {
    sync_with_budget(state, peer, CHUNKS_PER_RUN).await
}

pub async fn sync_with_budget(state: &SharedState, peer: &PeerInfo, chunk_budget: usize) -> LocalResult<Stats> {
    if !local_accounts::is_standalone(&state.db).await || !super::available(state).await {
        return Ok(Stats::default());
    }
    let mut stats = Stats::default();

    // 1. Do we differ at all?
    let mine = local_summary(state).await?;
    let reply = ask(state, peer, "/mesh/shared/summary", fields(vec![("digest", json!(digest(&mine)))])).await?;
    if !reply.get("same").and_then(|v| v.as_bool()).unwrap_or(false) {
        let theirs: Summary = serde_json::from_value(reply.get("summary").cloned().unwrap_or(Value::Null))
            .map_err(|e| LocalError::Internal(e.into()))?;

        // 2. Fetch the rows we lack or hold an older version of, in batches.
        let want_folders = wanted(&mine.folders, &theirs.folders);
        let want_files = wanted(&mine.files, &theirs.files);
        let mut folder_batches = want_folders.chunks(ROW_BATCH);
        let mut file_batches = want_files.chunks(ROW_BATCH);
        loop {
            let (fb, ib) = (folder_batches.next(), file_batches.next());
            if fb.is_none() && ib.is_none() {
                break;
            }
            let reply = ask(
                state,
                peer,
                "/mesh/shared/rows",
                fields(vec![("folders", json!(fb.unwrap_or(&[]))), ("files", json!(ib.unwrap_or(&[])))]),
            )
            .await?;
            let folders: Vec<FolderRecord> = serde_json::from_value(reply.get("folders").cloned().unwrap_or(Value::Null))
                .map_err(|e| LocalError::Internal(e.into()))?;
            let files: Vec<FileRecord> = serde_json::from_value(reply.get("files").cloned().unwrap_or(Value::Null))
                .map_err(|e| LocalError::Internal(e.into()))?;
            let applied = apply_rows(state, folders, files).await?;
            stats.folders += applied.folders;
            stats.files += applied.files;
        }
    }

    // 3. Content for files that are known but not complete here (also resumes an earlier run).
    stats.chunks += fetch_missing(state, peer, chunk_budget).await?;
    Ok(stats)
}

/// Fetch the missing chunks of `syncing` files from a peer, up to `budget`.
async fn fetch_missing(state: &SharedState, peer: &PeerInfo, mut budget: usize) -> LocalResult<usize> {
    let shop = shop_key(state).await?;
    let pending: Vec<(String, String, i64, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT id, content_id, chunk_count, size_bytes, wrapped_key FROM shared_files
          WHERE state = 'syncing' AND deleted_at IS NULL ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await?;

    let mut fetched = 0;
    for (id, content_id, chunk_count, size, wrapped) in pending {
        let key = match crypto::unwrap_file_key(&shop, &content_id, &wrapped) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!(file = %id, error = %e, "a replicated file's key does not open with this shop's key");
                continue;
            }
        };
        let have: HashSet<i64> = sqlx::query_as::<_, (i64,)>("SELECT idx FROM shared_file_chunks WHERE content_id = $1")
            .bind(&content_id)
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .map(|(i,)| i)
            .collect();

        let mut complete = true;
        for idx in 0..chunk_count {
            if have.contains(&idx) {
                continue;
            }
            if budget == 0 {
                complete = false;
                break;
            }
            let reply = match ask(state, peer, "/mesh/shared/chunk", fields(vec![("content_id", json!(content_id)), ("idx", json!(idx))])).await {
                Ok(r) => r,
                // This peer does not have it (yet): another one may.
                Err(_) => {
                    complete = false;
                    break;
                }
            };
            let stored = reply
                .get("data")
                .and_then(|v| v.as_str())
                .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
                .unwrap_or_default();
            let ctx = ChunkContext { content_id: &content_id, chunk_count: chunk_count as u32, total_size: size as u64 };
            if crypto::open_chunk(&key, &ctx, idx as u32, &stored).is_err() {
                tracing::warn!(file = %id, idx, "a chunk from a peer failed its integrity check and was dropped");
                complete = false;
                break;
            }
            sqlx::query("INSERT OR REPLACE INTO shared_file_chunks (content_id, idx, data) VALUES ($1, $2, $3)")
                .bind(&content_id)
                .bind(idx)
                .bind(stored)
                .execute(&state.db)
                .await?;
            budget -= 1;
            fetched += 1;
        }

        if complete {
            sqlx::query("UPDATE shared_files SET state = 'ready' WHERE id = $1 AND state = 'syncing' AND deleted_at IS NULL")
                .bind(&id)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(fetched)
}

// ------------------------------------------------------------------------------------ nudging ----

/// Tell the other PCs that something changed here, so they come and look without waiting for
/// their next round.
pub fn start(state: SharedState) {
    tokio::spawn(async move {
        loop {
            state.shared_signal.notified().await;
            // Several changes in a row (an upload) make one announcement.
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            if !local_accounts::is_standalone(&state.db).await {
                continue;
            }
            let peers = state.peers.read().await.clone();
            for peer in peers {
                if let Err(e) = ask(&state, &peer, "/mesh/shared/nudge", serde_json::Map::new()).await {
                    tracing::debug!(peer = %peer.host, error = %e, "shared folder nudge not delivered");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests;
