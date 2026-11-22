//! The shared folder: a file manager for the shop's sensitive documents.
//!
//! - **Storage.** Files live in the database, cut into chunks and encrypted (see `crypto`), so
//!   they are included in backups and can be replicated between PCs like the rest of the shop.
//! - **Access.** Administrators see everything. Every folder is either visible to administrators
//!   only or to everyone signed in; a folder is reachable only when it and all its parents are
//!   visible to the user, and an unreachable one looks like one that does not exist. Only
//!   administrators create, rename, move and delete; a user who can see a folder may add files.
//! - **Uploads** are resumable: a file is created in the `uploading` state, its chunks arrive one
//!   by one, and it appears in listings only once complete.
//! - **Audit.** Opening, adding and removing files is recorded, on this PC.

pub mod crypto;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;
use crypto::{ChunkContext, ShopKey, CHUNK_BYTES};

/// The biggest file accepted.
pub const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const NAME_MAX: usize = 200;
const MAX_DEPTH: usize = 8;
/// An upload that has not finished after this long is abandoned and cleaned up.
const STALE_UPLOAD_HOURS: i64 = 24;

// ------------------------------------------------------------------------------------------ who ----

/// The person using this PC, as far as the shared folder is concerned.
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: String,
    pub name: String,
    pub is_admin: bool,
}

/// The signed-in user. Fails when nobody is signed in.
pub async fn actor(state: &SharedState) -> LocalResult<Actor> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_json FROM cached_session WHERE id = 1")
        .fetch_optional(&state.db)
        .await?;
    let user: serde_json::Value = serde_json::from_str(&row.ok_or(LocalError::Unauthorized)?.0)
        .map_err(|e| LocalError::Internal(e.into()))?;
    let text = |k: &str| user.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let user_id = text("id");
    if user_id.is_empty() {
        return Err(LocalError::Unauthorized);
    }
    let name = [text("full_name"), text("username"), text("email")]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    Ok(Actor {
        user_id,
        name,
        is_admin: crate::local_server::auth_guard::is_admin(state).await,
    })
}

async fn shop_key(state: &SharedState) -> LocalResult<ShopKey> {
    let secret = state.mesh_secret.read().await.clone().unwrap_or_default();
    ShopKey::from_network_secret(&secret).map_err(|_| {
        LocalError::BadRequest(
            "Le dossier partagé sera disponible une fois le réseau créé ou le compte en ligne connecté.".into(),
        )
    })
}

/// Whether the shared folder can be used on this PC right now.
pub async fn available(state: &SharedState) -> bool {
    shop_key(state).await.is_ok()
}

// --------------------------------------------------------------------------------------- views ----

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FolderView {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub visibility: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FileView {
    pub id: String,
    pub folder_id: String,
    pub name: String,
    pub mime: Option<String>,
    pub size_bytes: i64,
    pub chunk_count: i64,
    pub state: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    /// The folder being shown; `None` for the top level.
    pub folder: Option<FolderView>,
    /// From the top level down to `folder`.
    pub breadcrumb: Vec<FolderView>,
    pub folders: Vec<FolderView>,
    pub files: Vec<FileView>,
    /// Whether this user may create, rename, move and delete here.
    pub can_manage: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct FolderRow {
    id: String,
    parent_id: Option<String>,
    name: String,
    visibility: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
}

impl FolderRow {
    fn view(&self) -> FolderView {
        FolderView {
            id: self.id.clone(),
            parent_id: self.parent_id.clone(),
            name: self.name.clone(),
            visibility: self.visibility.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

const FOLDER_COLS: &str = "id, parent_id, name, visibility, created_at, updated_at, deleted_at";
const FILE_COLS: &str =
    "id, folder_id, name, mime, size_bytes, chunk_count, state, created_by, created_at, updated_at";

// ------------------------------------------------------------------------------- name helpers ----

/// A name that is safe as a file or folder name on any of the PCs.
pub fn clean_name(raw: &str) -> LocalResult<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(LocalError::BadRequest("Le nom est obligatoire.".into()));
    }
    if name.chars().count() > NAME_MAX {
        return Err(LocalError::BadRequest(format!("Le nom est trop long ({NAME_MAX} caractères au plus).")));
    }
    if name == "." || name == ".." || name.ends_with('.') {
        return Err(LocalError::BadRequest("Ce nom n'est pas valide.".into()));
    }
    if name.chars().any(|c| c.is_control() || "\\/:*?\"<>|".contains(c)) {
        return Err(LocalError::BadRequest(
            "Le nom ne peut pas contenir \\ / : * ? \" < > |".into(),
        ));
    }
    Ok(name)
}

/// `name`, or `name (2)`, `name (3)`… when the folder already holds a file of that name.
async fn free_file_name(state: &SharedState, folder_id: &str, name: &str) -> LocalResult<String> {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1..1000 {
        let candidate = if n == 1 { name.to_string() } else { format!("{stem} ({n}){ext}") };
        let taken: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM shared_files WHERE folder_id = $1 AND deleted_at IS NULL AND lower(name) = lower($2)",
        )
        .bind(folder_id)
        .bind(&candidate)
        .fetch_optional(&state.db)
        .await?;
        if taken.is_none() {
            return Ok(candidate);
        }
    }
    Err(LocalError::Conflict("Trop de fichiers portent déjà ce nom.".into()))
}

async fn folder_name_taken(
    state: &SharedState,
    parent_id: Option<&str>,
    name: &str,
    except: Option<&str>,
) -> LocalResult<bool> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM shared_folders WHERE parent_id IS $1 AND deleted_at IS NULL
           AND lower(name) = lower($2) AND id != COALESCE($3, '')",
    )
    .bind(parent_id)
    .bind(name)
    .bind(except)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------- folder reachability ----

/// The folder and its ancestors, nearest first. Stops at the top, or at a loop (corrupt data).
async fn chain(state: &SharedState, folder_id: &str) -> LocalResult<Vec<FolderRow>> {
    let mut out: Vec<FolderRow> = Vec::new();
    let mut next = Some(folder_id.to_string());
    while let Some(id) = next {
        if out.len() > MAX_DEPTH + 4 || out.iter().any(|f| f.id == id) {
            break;
        }
        let row: Option<FolderRow> =
            sqlx::query_as(&format!("SELECT {FOLDER_COLS} FROM shared_folders WHERE id = $1"))
                .bind(&id)
                .fetch_optional(&state.db)
                .await?;
        let Some(row) = row else { break };
        next = row.parent_id.clone();
        out.push(row);
    }
    Ok(out)
}

fn reachable(actor: &Actor, chain: &[FolderRow]) -> bool {
    !chain.is_empty()
        && chain.iter().all(|f| f.deleted_at.is_none())
        && (actor.is_admin || chain.iter().all(|f| f.visibility == "everyone"))
}

/// The folder's chain, or "not found" when it does not exist or the user may not see it.
async fn reachable_chain(state: &SharedState, actor: &Actor, folder_id: &str) -> LocalResult<Vec<FolderRow>> {
    let chain = chain(state, folder_id).await?;
    if reachable(actor, &chain) {
        Ok(chain)
    } else {
        Err(LocalError::NotFound)
    }
}

fn require_admin(actor: &Actor) -> LocalResult<()> {
    if actor.is_admin {
        Ok(())
    } else {
        Err(LocalError::Forbidden)
    }
}

// ----------------------------------------------------------------------------------- browsing ----

pub async fn browse(state: &SharedState, actor: &Actor, folder_id: Option<&str>) -> LocalResult<Listing> {
    shop_key(state).await?;
    let (folder, breadcrumb) = match folder_id {
        Some(id) => {
            let chain = reachable_chain(state, actor, id).await?;
            let breadcrumb: Vec<FolderView> = chain.iter().rev().map(FolderRow::view).collect();
            (breadcrumb.last().cloned(), breadcrumb)
        }
        None => (None, Vec::new()),
    };

    let visibility_filter = if actor.is_admin { "" } else { "AND visibility = 'everyone'" };
    let folders: Vec<FolderView> = sqlx::query_as(&format!(
        "SELECT id, parent_id, name, visibility, created_at, updated_at FROM shared_folders
          WHERE parent_id IS $1 AND deleted_at IS NULL {visibility_filter} ORDER BY lower(name)"
    ))
    .bind(folder_id)
    .fetch_all(&state.db)
    .await?;

    let files: Vec<FileView> = match folder_id {
        Some(id) => sqlx::query_as(&format!(
            "SELECT {FILE_COLS} FROM shared_files
              WHERE folder_id = $1 AND deleted_at IS NULL AND state = 'ready' ORDER BY lower(name)"
        ))
        .bind(id)
        .fetch_all(&state.db)
        .await?,
        None => Vec::new(),
    };

    Ok(Listing { folder, breadcrumb, folders, files, can_manage: actor.is_admin })
}

// -------------------------------------------------------------------------------- folder edits ----

pub async fn create_folder(
    state: &SharedState,
    actor: &Actor,
    parent_id: Option<&str>,
    name: &str,
    visibility: &str,
) -> LocalResult<FolderView> {
    require_admin(actor)?;
    shop_key(state).await?;
    let name = clean_name(name)?;
    if !matches!(visibility, "admins" | "everyone") {
        return Err(LocalError::BadRequest("La visibilité doit être « admins » ou « everyone ».".into()));
    }
    if let Some(parent) = parent_id {
        let chain = reachable_chain(state, actor, parent).await?;
        if chain.len() >= MAX_DEPTH {
            return Err(LocalError::BadRequest(format!("Les dossiers ne peuvent pas être imbriqués sur plus de {MAX_DEPTH} niveaux.")));
        }
    }
    if folder_name_taken(state, parent_id, &name, None).await? {
        return Err(LocalError::Conflict("Un dossier de ce nom existe déjà ici.".into()));
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO shared_folders (id, parent_id, name, visibility, created_by, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $6)",
    )
    .bind(&id)
    .bind(parent_id)
    .bind(&name)
    .bind(visibility)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&state.db)
    .await?;
    audit(state, actor, "folder_created", None, Some(&id), Some(&name)).await;
    Ok(FolderView {
        id,
        parent_id: parent_id.map(str::to_string),
        name,
        visibility: visibility.to_string(),
        created_at: now.clone(),
        updated_at: now,
    })
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct FolderPatch {
    pub name: Option<String>,
    pub visibility: Option<String>,
    /// `Some(None)` moves the folder to the top level.
    #[serde(default, deserialize_with = "present")]
    pub parent_id: Option<Option<String>>,
}

fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

pub async fn update_folder(
    state: &SharedState,
    actor: &Actor,
    id: &str,
    patch: FolderPatch,
) -> LocalResult<FolderView> {
    require_admin(actor)?;
    let chain_of = reachable_chain(state, actor, id).await?;
    let current = &chain_of[0];

    let name = match &patch.name {
        Some(n) => clean_name(n)?,
        None => current.name.clone(),
    };
    let visibility = match patch.visibility.as_deref() {
        Some(v @ ("admins" | "everyone")) => v.to_string(),
        Some(_) => return Err(LocalError::BadRequest("La visibilité doit être « admins » ou « everyone ».".into())),
        None => current.visibility.clone(),
    };
    let parent_id = match &patch.parent_id {
        Some(p) => p.clone(),
        None => current.parent_id.clone(),
    };
    if let Some(parent) = parent_id.as_deref() {
        let target = reachable_chain(state, actor, parent).await?;
        if target.iter().any(|f| f.id == id) {
            return Err(LocalError::BadRequest("Un dossier ne peut pas être déplacé dans lui-même.".into()));
        }
        // Depth of the moved subtree counts too.
        if target.len() + subtree_height(state, id).await? > MAX_DEPTH {
            return Err(LocalError::BadRequest(format!("Les dossiers ne peuvent pas être imbriqués sur plus de {MAX_DEPTH} niveaux.")));
        }
    }
    if folder_name_taken(state, parent_id.as_deref(), &name, Some(id)).await? {
        return Err(LocalError::Conflict("Un dossier de ce nom existe déjà ici.".into()));
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE shared_folders SET name = $2, visibility = $3, parent_id = $4, updated_at = $5 WHERE id = $1")
        .bind(id)
        .bind(&name)
        .bind(&visibility)
        .bind(parent_id.as_deref())
        .bind(&now)
        .execute(&state.db)
        .await?;
    if visibility != current.visibility {
        audit(state, actor, "folder_visibility", None, Some(id), Some(&format!("{} -> {visibility}", current.visibility))).await;
    }
    Ok(FolderView {
        id: id.to_string(),
        parent_id,
        name,
        visibility,
        created_at: current.created_at.clone(),
        updated_at: now,
    })
}

/// Levels in the subtree under `id`, counting `id` itself.
async fn subtree_height(state: &SharedState, id: &str) -> LocalResult<usize> {
    let mut height = 1;
    let mut level = vec![id.to_string()];
    for _ in 0..MAX_DEPTH {
        let mut next = Vec::new();
        for parent in &level {
            let kids: Vec<(String,)> =
                sqlx::query_as("SELECT id FROM shared_folders WHERE parent_id = $1 AND deleted_at IS NULL")
                    .bind(parent)
                    .fetch_all(&state.db)
                    .await?;
            next.extend(kids.into_iter().map(|(k,)| k));
        }
        if next.is_empty() {
            break;
        }
        height += 1;
        level = next;
    }
    Ok(height)
}

/// Delete a folder with everything in it. The rows stay as tombstones (so that the deletion
/// reaches the other PCs); the encrypted content is erased now.
pub async fn delete_folder(state: &SharedState, actor: &Actor, id: &str) -> LocalResult<()> {
    require_admin(actor)?;
    let chain_of = reachable_chain(state, actor, id).await?;
    let name = chain_of[0].name.clone();
    let now = Utc::now().to_rfc3339();

    let mut tx = state.db.begin().await?;
    // Every folder of the subtree, breadth first.
    let mut all = vec![id.to_string()];
    let mut i = 0;
    while i < all.len() {
        let kids: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM shared_folders WHERE parent_id = $1 AND deleted_at IS NULL")
                .bind(&all[i])
                .fetch_all(&mut *tx)
                .await?;
        all.extend(kids.into_iter().map(|(k,)| k));
        i += 1;
    }
    for folder in &all {
        let contents: Vec<(String,)> =
            sqlx::query_as("SELECT content_id FROM shared_files WHERE folder_id = $1 AND deleted_at IS NULL")
                .bind(folder)
                .fetch_all(&mut *tx)
                .await?;
        for (content_id,) in contents {
            sqlx::query("DELETE FROM shared_file_chunks WHERE content_id = $1")
                .bind(content_id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE shared_files SET deleted_at = $2, updated_at = $2 WHERE folder_id = $1 AND deleted_at IS NULL")
            .bind(folder)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE shared_folders SET deleted_at = $2, updated_at = $2 WHERE id = $1")
            .bind(folder)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    audit(state, actor, "folder_deleted", None, Some(id), Some(&name)).await;
    Ok(())
}

// --------------------------------------------------------------------------------------- upload ----

#[derive(Debug, Serialize)]
pub struct UploadStart {
    pub file: FileView,
    pub chunk_bytes: usize,
}

/// Start a file: the entry exists, hidden, until [`finish_upload`].
pub async fn begin_upload(
    state: &SharedState,
    actor: &Actor,
    folder_id: &str,
    name: &str,
    size_bytes: u64,
    mime: Option<&str>,
) -> LocalResult<UploadStart> {
    let shop = shop_key(state).await?;
    reachable_chain(state, actor, folder_id).await?;
    let name = clean_name(name)?;
    if size_bytes > MAX_FILE_BYTES {
        return Err(LocalError::BadRequest(format!(
            "Ce fichier est trop volumineux ({} Mo au plus).",
            MAX_FILE_BYTES / (1024 * 1024)
        )));
    }
    purge_stale_uploads(state).await;

    let name = free_file_name(state, folder_id, &name).await?;
    let id = Uuid::new_v4().to_string();
    let content_id = Uuid::new_v4().to_string();
    let file_key = crypto::new_file_key();
    let wrapped = crypto::wrap_file_key(&shop, &content_id, &file_key);
    let chunk_count = crypto::chunks_for(size_bytes);
    let now = Utc::now().to_rfc3339();
    let mime = mime.map(|m| m.chars().take(120).collect::<String>()).filter(|m| !m.is_empty());

    sqlx::query(
        "INSERT INTO shared_files (id, folder_id, name, mime, size_bytes, chunk_count, content_id,
                                   wrapped_key, key_version, state, created_by, created_at, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'uploading',$10,$11,$11)",
    )
    .bind(&id)
    .bind(folder_id)
    .bind(&name)
    .bind(mime.as_deref())
    .bind(size_bytes as i64)
    .bind(chunk_count as i64)
    .bind(&content_id)
    .bind(&wrapped)
    .bind(crypto::KEY_VERSION)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&state.db)
    .await?;
    let file: FileView = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM shared_files WHERE id = $1"))
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    Ok(UploadStart { file, chunk_bytes: CHUNK_BYTES })
}

#[derive(Debug, sqlx::FromRow)]
struct FileRow {
    id: String,
    folder_id: String,
    name: String,
    mime: Option<String>,
    size_bytes: i64,
    chunk_count: i64,
    content_id: String,
    wrapped_key: Vec<u8>,
    state: String,
    created_by: Option<String>,
    deleted_at: Option<String>,
}

const FILE_ROW_COLS: &str =
    "id, folder_id, name, mime, size_bytes, chunk_count, content_id, wrapped_key, state, created_by, deleted_at";

async fn file_row(state: &SharedState, file_id: &str) -> LocalResult<FileRow> {
    let row: Option<FileRow> =
        sqlx::query_as(&format!("SELECT {FILE_ROW_COLS} FROM shared_files WHERE id = $1"))
            .bind(file_id)
            .fetch_optional(&state.db)
            .await?;
    match row {
        Some(r) if r.deleted_at.is_none() => Ok(r),
        _ => Err(LocalError::NotFound),
    }
}

fn file_key_of(shop: &ShopKey, row: &FileRow) -> LocalResult<[u8; 32]> {
    crypto::unwrap_file_key(shop, &row.content_id, &row.wrapped_key)
        .map_err(|e| LocalError::Internal(anyhow::anyhow!("shared file key: {e}")))
}

/// Store one chunk of a file being uploaded. Only the person who started it may.
pub async fn put_chunk(
    state: &SharedState,
    actor: &Actor,
    file_id: &str,
    index: u32,
    bytes: &[u8],
) -> LocalResult<()> {
    let shop = shop_key(state).await?;
    let row = file_row(state, file_id).await?;
    reachable_chain(state, actor, &row.folder_id).await?;
    if row.state != "uploading" || row.created_by.as_deref() != Some(actor.user_id.as_str()) {
        return Err(LocalError::Forbidden);
    }
    let count = row.chunk_count as u32;
    if index >= count {
        return Err(LocalError::BadRequest("Morceau hors du fichier.".into()));
    }
    let expected = if index + 1 < count {
        CHUNK_BYTES
    } else {
        (row.size_bytes as u64 - (count as u64 - 1) * CHUNK_BYTES as u64) as usize
    };
    if bytes.len() != expected {
        return Err(LocalError::BadRequest(format!(
            "Le morceau {index} devrait faire {expected} octets, il en fait {}.",
            bytes.len()
        )));
    }
    let key = file_key_of(&shop, &row)?;
    let ctx = ChunkContext { content_id: &row.content_id, chunk_count: count, total_size: row.size_bytes as u64 };
    let sealed = crypto::seal_chunk(&key, &ctx, index, bytes);
    sqlx::query("INSERT OR REPLACE INTO shared_file_chunks (content_id, idx, data) VALUES ($1, $2, $3)")
        .bind(&row.content_id)
        .bind(index as i64)
        .bind(sealed)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Make an uploaded file visible, once every chunk is in.
pub async fn finish_upload(state: &SharedState, actor: &Actor, file_id: &str) -> LocalResult<FileView> {
    let row = file_row(state, file_id).await?;
    reachable_chain(state, actor, &row.folder_id).await?;
    if row.created_by.as_deref() != Some(actor.user_id.as_str()) {
        return Err(LocalError::Forbidden);
    }
    if row.state == "uploading" {
        let (have,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM shared_file_chunks WHERE content_id = $1")
            .bind(&row.content_id)
            .fetch_one(&state.db)
            .await?;
        if have != row.chunk_count {
            return Err(LocalError::BadRequest(format!(
                "Envoi incomplet : {have} morceau(x) sur {}.",
                row.chunk_count
            )));
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE shared_files SET state = 'ready', updated_at = $2 WHERE id = $1")
            .bind(file_id)
            .bind(&now)
            .execute(&state.db)
            .await?;
        audit(state, actor, "file_added", Some(file_id), Some(&row.folder_id), Some(&row.name)).await;
    }
    sqlx::query_as(&format!("SELECT {FILE_COLS} FROM shared_files WHERE id = $1"))
        .bind(file_id)
        .fetch_one(&state.db)
        .await
        .map_err(Into::into)
}

/// Forget uploads that were started and never finished.
async fn purge_stale_uploads(state: &SharedState) {
    let cutoff = (Utc::now() - Duration::hours(STALE_UPLOAD_HOURS)).to_rfc3339();
    let stale: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, content_id FROM shared_files WHERE state = 'uploading' AND created_at < $1",
    )
    .bind(&cutoff)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    for (id, content_id) in stale {
        let _ = sqlx::query("DELETE FROM shared_file_chunks WHERE content_id = $1")
            .bind(&content_id)
            .execute(&state.db)
            .await;
        let _ = sqlx::query("DELETE FROM shared_files WHERE id = $1").bind(&id).execute(&state.db).await;
    }
}

// ------------------------------------------------------------------------------------- reading ----

/// Everything needed to stream a file out.
pub struct Download {
    pub file_id: String,
    pub name: String,
    pub mime: Option<String>,
    pub size_bytes: u64,
    content_id: String,
    chunk_count: u32,
    key: [u8; 32],
}

pub async fn open_download(state: &SharedState, actor: &Actor, file_id: &str) -> LocalResult<Download> {
    let shop = shop_key(state).await?;
    let row = file_row(state, file_id).await?;
    reachable_chain(state, actor, &row.folder_id).await?;
    if row.state != "ready" {
        return Err(LocalError::NotFound);
    }
    let key = file_key_of(&shop, &row)?;
    audit(state, actor, "file_opened", Some(file_id), Some(&row.folder_id), Some(&row.name)).await;
    Ok(Download {
        file_id: row.id,
        name: row.name,
        mime: row.mime,
        size_bytes: row.size_bytes as u64,
        content_id: row.content_id,
        chunk_count: row.chunk_count as u32,
        key,
    })
}

impl Download {
    pub fn chunk_count(&self) -> u32 {
        self.chunk_count
    }

    /// One chunk, decrypted and checked.
    pub async fn read_chunk(&self, state: &SharedState, index: u32) -> LocalResult<Vec<u8>> {
        let stored: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT data FROM shared_file_chunks WHERE content_id = $1 AND idx = $2")
                .bind(&self.content_id)
                .bind(index as i64)
                .fetch_optional(&state.db)
                .await?;
        let (stored,) = stored.ok_or_else(|| {
            LocalError::Internal(anyhow::anyhow!("shared file {}: chunk {index} is missing", self.file_id))
        })?;
        let ctx = ChunkContext { content_id: &self.content_id, chunk_count: self.chunk_count, total_size: self.size_bytes };
        crypto::open_chunk(&self.key, &ctx, index, &stored)
            .map_err(|e| LocalError::Internal(anyhow::anyhow!("shared file {}: {e}", self.file_id)))
    }
}

// ---------------------------------------------------------------------------------- file edits ----

#[derive(Debug, Default, serde::Deserialize)]
pub struct FilePatch {
    pub name: Option<String>,
    pub folder_id: Option<String>,
}

pub async fn update_file(
    state: &SharedState,
    actor: &Actor,
    file_id: &str,
    patch: FilePatch,
) -> LocalResult<FileView> {
    require_admin(actor)?;
    let row = file_row(state, file_id).await?;
    reachable_chain(state, actor, &row.folder_id).await?;
    let folder_id = match patch.folder_id.as_deref() {
        Some(target) => {
            reachable_chain(state, actor, target).await?;
            target.to_string()
        }
        None => row.folder_id.clone(),
    };
    let name = match patch.name.as_deref() {
        Some(n) => clean_name(n)?,
        None => row.name.clone(),
    };
    if folder_id != row.folder_id || name != row.name {
        // The name must be free in the destination (the file itself does not count).
        let taken: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM shared_files WHERE folder_id = $1 AND deleted_at IS NULL
               AND lower(name) = lower($2) AND id != $3",
        )
        .bind(&folder_id)
        .bind(&name)
        .bind(file_id)
        .fetch_optional(&state.db)
        .await?;
        if taken.is_some() {
            return Err(LocalError::Conflict("Un fichier de ce nom existe déjà dans ce dossier.".into()));
        }
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE shared_files SET name = $2, folder_id = $3, updated_at = $4 WHERE id = $1")
        .bind(file_id)
        .bind(&name)
        .bind(&folder_id)
        .bind(&now)
        .execute(&state.db)
        .await?;
    sqlx::query_as(&format!("SELECT {FILE_COLS} FROM shared_files WHERE id = $1"))
        .bind(file_id)
        .fetch_one(&state.db)
        .await
        .map_err(Into::into)
}

/// Delete a file (an administrator), or cancel an upload (the person who started it).
pub async fn delete_file(state: &SharedState, actor: &Actor, file_id: &str) -> LocalResult<()> {
    let row = file_row(state, file_id).await?;
    reachable_chain(state, actor, &row.folder_id).await?;
    let cancelling = row.state == "uploading" && row.created_by.as_deref() == Some(actor.user_id.as_str());
    if !actor.is_admin && !cancelling {
        return Err(LocalError::Forbidden);
    }
    let now = Utc::now().to_rfc3339();
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM shared_file_chunks WHERE content_id = $1")
        .bind(&row.content_id)
        .execute(&mut *tx)
        .await?;
    if row.state == "uploading" {
        // Never seen by anyone else: nothing to replicate.
        sqlx::query("DELETE FROM shared_files WHERE id = $1").bind(file_id).execute(&mut *tx).await?;
    } else {
        sqlx::query("UPDATE shared_files SET deleted_at = $2, updated_at = $2 WHERE id = $1")
            .bind(file_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    if row.state == "ready" {
        audit(state, actor, "file_deleted", Some(file_id), Some(&row.folder_id), Some(&row.name)).await;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------- audit ----

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditEntry {
    pub at: String,
    pub user_name: Option<String>,
    pub action: String,
    pub file_id: Option<String>,
    pub folder_id: Option<String>,
    pub detail: Option<String>,
}

async fn audit(
    state: &SharedState,
    actor: &Actor,
    action: &str,
    file_id: Option<&str>,
    folder_id: Option<&str>,
    detail: Option<&str>,
) {
    let result = sqlx::query(
        "INSERT INTO shared_audit (id, at, user_id, user_name, action, file_id, folder_id, detail)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(Utc::now().to_rfc3339())
    .bind(&actor.user_id)
    .bind(&actor.name)
    .bind(action)
    .bind(file_id)
    .bind(folder_id)
    .bind(detail)
    .execute(&state.db)
    .await;
    if let Err(e) = result {
        tracing::warn!(error = %e, "shared folder audit entry not written");
    }
}

pub async fn recent_audit(state: &SharedState, actor: &Actor, limit: i64) -> LocalResult<Vec<AuditEntry>> {
    require_admin(actor)?;
    sqlx::query_as(
        "SELECT at, user_name, action, file_id, folder_id, detail FROM shared_audit ORDER BY at DESC LIMIT $1",
    )
    .bind(limit.clamp(1, 500))
    .fetch_all(&state.db)
    .await
    .map_err(Into::into)
}

// ---------------------------------------------------------------------------------------- usage ----

#[derive(Debug, Serialize)]
pub struct Usage {
    pub files: i64,
    pub bytes: i64,
    pub max_file_bytes: u64,
    pub chunk_bytes: usize,
}

pub async fn usage(state: &SharedState) -> LocalResult<Usage> {
    let (files, bytes): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM shared_files WHERE deleted_at IS NULL AND state = 'ready'",
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Usage { files, bytes, max_file_bytes: MAX_FILE_BYTES, chunk_bytes: CHUNK_BYTES })
}

#[cfg(test)]
mod tests;
