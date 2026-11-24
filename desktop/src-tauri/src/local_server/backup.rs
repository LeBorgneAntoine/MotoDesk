//! Shop backup and restore.
//!
//! A backup is one encrypted file (`.mgbak`, see `backup_format`) holding the
//! local SQLite database, the fiscal archives and the secrets that make a new PC
//! the same cash register: the LAN mesh secret and the fiscal signing keys.
//! Everything is encrypted under a password the shop owner chooses, so a copied
//! file reveals nothing.
//!
//! Older backups (`.zip`, secrets encrypted with a weaker key and the database in
//! clear) can still be restored; new ones are never written in that format.
//!
//! Restoring is staged: the archive is unpacked and checked (integrity, schema
//! version) in a scratch folder, and only then queued for the next start, where
//! the current database is *kept* beside the restored one instead of deleted.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use crate::local_server::backup_format::{
    self as format, DecryptReader, EncryptWriter, OpenError, DEFAULT_ITERATIONS,
};

/// Key-derivation cost written into new archives. Tests use a token cost: an
/// unoptimised build would otherwise spend half a minute per derivation. The
/// count is stored in each archive's header, and `backup_format` is tested with
/// both the published PBKDF2 vectors and the production floor.
const KDF_ITERATIONS: u32 = if cfg!(test) { 2_000 } else { DEFAULT_ITERATIONS };
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;

const BACKUP_FORMAT: u32 = 2;
pub const EXT: &str = "mgbak";
const MANIFEST_ENTRY: &str = "manifest.json";
const SECRETS_ENTRY: &str = "secrets.json";
const DB_ENTRY: &str = "local.db";
const ARCHIVES_PREFIX: &str = "archives/";
const PENDING_DB: &str = "local.db.incoming";
const STAGING_DIR: &str = "restore-staging";
const SAFETY_COPY_PREFIX: &str = "local.db.before-restore-";
/// Present while the restored database has still to be given its new identity. A file, so that
/// a start interrupted between the swap and that step still takes it.
const RESTORED_MARKER: &str = "restore.new-identity";
/// Safety copies of replaced databases to keep (each is as large as the database).
const SAFETY_COPIES_KEPT: usize = 2;
/// Automatic backups to keep; older ones are removed after each new one.
const AUTO_KEPT: usize = 8;
pub const MIN_PASSWORD_CHARS: usize = 10;

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format: u32,
    pub created_at: String,
    pub device_id: Option<String>,
    pub app_version: String,
    /// Newest schema migration in the database, so a restore can refuse a
    /// backup made by a newer version of the app.
    pub schema_version: i64,
}

#[derive(Debug, Serialize)]
pub struct BackupFile {
    pub filename: String,
    pub path: String,
    pub size_bytes: u64,
    pub created_at: String,
    /// `automatic`, `manual` or `legacy` (an old ZIP).
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct BackupStatus {
    pub backups_dir: String,
    pub last_backup_at: Option<String>,
    pub auto_enabled: bool,
    pub frequency_days: i64,
    /// Automatic backups need a password the owner chose; without one nothing
    /// is written, and the screen says so.
    pub auto_password_set: bool,
    /// A restore is queued and will replace the data at the next start.
    pub restore_pending: bool,
    pub files: Vec<BackupFile>,
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub restart_required: bool,
    /// Where the data being replaced is kept until the next restores prune it.
    pub safety_copy_note: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SecretsPayload {
    mesh_secret: Option<String>,
    fiscal_keys: Vec<(String, String)>,
}

pub fn backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

fn io_err(e: impl Into<anyhow::Error>) -> LocalError {
    LocalError::Internal(e.into())
}

/// Same, but says which operation and file failed: "in use by another process"
/// alone does not tell anyone what to look at.
fn io_at<'a>(what: &'a str, path: &'a Path) -> impl FnOnce(std::io::Error) -> LocalError + 'a {
    move |e| LocalError::Internal(anyhow::anyhow!("{what} ({}) : {e}", path.display()))
}

// ------------------------------------------------------------- start-up -----

/// Swap a restored database in before the pool opens.
///
/// The database being replaced is renamed, not deleted: a restore is the one
/// operation that throws away a day's trading if the wrong file was picked, so
/// the previous data stays on disk (`local.db.before-restore-<time>`).
pub fn apply_pending_restore(data_dir: &Path) -> anyhow::Result<bool> {
    let incoming = data_dir.join(PENDING_DB);
    if !incoming.exists() {
        return Ok(false);
    }
    let dest = data_dir.join("local.db");
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let keep = data_dir.join(format!("{SAFETY_COPY_PREFIX}{stamp}"));

    fs::write(data_dir.join(RESTORED_MARKER), b"")?;

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let from = data_dir.join(format!("local.db{suffix}"));
        if from.exists() {
            let to = PathBuf::from(format!("{}{suffix}", keep.display()));
            fs::rename(&from, &to)?;
            moved.push((from, to));
        }
    }
    if let Err(e) = fs::rename(&incoming, &dest) {
        // Put everything back: better the old data than none.
        for (from, to) in moved.iter().rev() {
            let _ = fs::rename(to, from);
        }
        return Err(e.into());
    }

    prune_safety_copies(data_dir);
    tracing::info!(kept = %keep.display(), "applied pending shop restore");
    Ok(true)
}

/// After a restore on a PC that runs without a cloud, give the restored data a new identity
/// (`db::renew_identity`). Call once the database is open and migrated.
pub async fn finish_restore(pool: &sqlx::SqlitePool, data_dir: &Path) -> anyhow::Result<()> {
    let marker = data_dir.join(RESTORED_MARKER);
    if !marker.exists() {
        return Ok(());
    }
    if crate::local_server::local_accounts::is_standalone(pool).await {
        let id = crate::local_server::db::renew_identity(pool).await?;
        tracing::info!(device = %id, "restored data: this PC continues as a new device, with a new till");
    }
    fs::remove_file(marker)?;
    Ok(())
}

fn prune_safety_copies(data_dir: &Path) {
    let mut copies: Vec<String> = fs::read_dir(data_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(SAFETY_COPY_PREFIX) && !n.ends_with("-wal") && !n.ends_with("-shm"))
        .collect();
    copies.sort();
    while copies.len() > SAFETY_COPIES_KEPT {
        let old = copies.remove(0);
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(data_dir.join(format!("{old}{suffix}")));
        }
    }
}

// --------------------------------------------------------------- status -----

pub async fn status(state: &SharedState) -> LocalResult<BackupStatus> {
    let dir = backups_dir(&state.data_dir);
    let _ = fs::create_dir_all(&dir);
    let files = list_files(&dir);
    let last_backup_at: Option<(Option<String>,)> =
        sqlx::query_as("SELECT last_backup_at FROM sync_state WHERE id = 1")
            .fetch_optional(&state.db)
            .await?;
    Ok(BackupStatus {
        backups_dir: dir.display().to_string(),
        last_backup_at: last_backup_at.and_then(|(t,)| t).filter(|s| !s.is_empty()),
        auto_enabled: setting_bool(state, "system.auto_backup_enabled", true).await,
        frequency_days: setting_i64(state, "system.backup_frequency_days", 7).await,
        auto_password_set: crate::local_server::secrets::load_backup_password()
            .ok()
            .flatten()
            .is_some(),
        restore_pending: state.data_dir.join(PENDING_DB).exists(),
        files,
    })
}

// --------------------------------------------------------------- export -----

struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn check_password(password: &str) -> LocalResult<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(LocalError::BadRequest(format!(
            "le mot de passe de sauvegarde doit faire au moins {MIN_PASSWORD_CHARS} caractères"
        )));
    }
    Ok(())
}

pub async fn export(state: &SharedState, password: &str) -> LocalResult<BackupFile> {
    write_backup(state, password, "motodesk").await
}

async fn write_backup(state: &SharedState, password: &str, prefix: &str) -> LocalResult<BackupFile> {
    check_password(password)?;
    let dir = backups_dir(&state.data_dir);
    fs::create_dir_all(&dir).map_err(io_err)?;

    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let final_path = dir.join(format!("{prefix}-{stamp}.{EXT}"));
    let partial = dir.join(format!("{prefix}-{stamp}.{EXT}.partial"));
    let db_copy = dir.join(format!(".tmp-{stamp}.db"));
    let _db_guard = TempFile(db_copy.clone());
    let _partial_guard = TempFile(partial.clone());

    // A consistent copy taken while the app keeps running.
    vacuum_into(&state.db, &db_copy).await?;

    let secrets = collect_secrets(state).await?;
    let manifest = BackupManifest {
        format: BACKUP_FORMAT,
        created_at: Utc::now().to_rfc3339(),
        device_id: state.device_id().await.ok().map(|id| id.to_string()),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: crate::local_server::db::latest_migration_version(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let secrets_bytes = serde_json::to_vec(&secrets)?;
    let archives = state.data_dir.join("archives");
    let password = password.to_string();

    {
        let partial = partial.clone();
        let db_copy = db_copy.clone();
        tokio::task::spawn_blocking(move || {
            write_archive(&partial, &password, &db_copy, &manifest_bytes, &secrets_bytes, &archives)
        })
        .await
        .map_err(io_err)?
        .map_err(|e| LocalError::Internal(anyhow::anyhow!("écriture de la sauvegarde : {e}")))?;
    }

    // Visible under its final name only once it is complete.
    fs::rename(&partial, &final_path).map_err(io_err)?;

    let now = Utc::now().to_rfc3339();
    let _ = sqlx::query("UPDATE sync_state SET last_backup_at = $1 WHERE id = 1")
        .bind(&now)
        .execute(&state.db)
        .await;

    Ok(file_info(&final_path).unwrap_or(BackupFile {
        filename: final_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: final_path.display().to_string(),
        size_bytes: 0,
        created_at: now,
        kind: "manual".into(),
    }))
}

fn write_archive(
    dest: &Path,
    password: &str,
    db_copy: &Path,
    manifest: &[u8],
    secrets: &[u8],
    archives: &Path,
) -> std::io::Result<()> {
    let file = BufWriter::new(File::create(dest)?);
    let enc = EncryptWriter::new(file, password, KDF_ITERATIONS)?;
    let mut gz = GzEncoder::new(enc, Compression::fast());

    format::write_bytes_entry(&mut gz, MANIFEST_ENTRY, manifest)?;
    format::write_bytes_entry(&mut gz, SECRETS_ENTRY, secrets)?;

    let mut db = File::open(db_copy)?;
    let db_len = db.metadata()?.len();
    format::write_entry(&mut gz, DB_ENTRY, db_len, &mut BufReader::new(&mut db))?;

    if archives.is_dir() {
        for entry in fs::read_dir(archives)?.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = format!("{ARCHIVES_PREFIX}{}", entry.file_name().to_string_lossy());
            let mut f = File::open(&path)?;
            let len = f.metadata()?.len();
            format::write_entry(&mut gz, &name, len, &mut BufReader::new(&mut f))?;
        }
    }
    format::write_end(&mut gz)?;

    let enc = gz.finish()?;
    let mut file = enc.finish()?;
    file.flush()?;
    file.into_inner().map_err(|e| e.into_error())?.sync_all()
}

// -------------------------------------------------------------- restore -----

pub async fn restore_named(
    state: &SharedState,
    filename: &str,
    password: &str,
) -> LocalResult<RestoreResult> {
    let safe = sanitize_filename(filename)?;
    let path = backups_dir(&state.data_dir).join(safe);
    restore_path(state, &path, password).await
}

pub async fn restore_path(
    state: &SharedState,
    path: &Path,
    password: &str,
) -> LocalResult<RestoreResult> {
    if !path.exists() {
        return Err(LocalError::NotFound);
    }
    let staging = state.data_dir.join(STAGING_DIR);
    remove_dir_patiently(&staging).await;
    fs::create_dir_all(&staging).map_err(io_err)?;

    let result = stage_and_queue(state, path, password, &staging).await;
    // Whatever happened, the scratch folder does not stay behind.
    remove_dir_patiently(&staging).await;
    result
}

/// Move a file, trying again for a moment when Windows says it is still in use
/// (a scanner or indexer holding a file that was just written or closed).
async fn rename_patiently(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if attempt >= 20 => return Err(e),
            Err(_) => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Remove a scratch folder, trying again for a moment when Windows says a file
/// is still in use: an antivirus or the search indexer often holds a freshly
/// written file for a few hundred milliseconds.
async fn remove_dir_patiently(dir: &Path) {
    for attempt in 0..20 {
        match fs::remove_dir_all(dir) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) if attempt == 19 => {
                tracing::warn!(dir = %dir.display(), error = %e, "could not clear the restore scratch folder");
                return;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

async fn stage_and_queue(
    state: &SharedState,
    path: &Path,
    password: &str,
    staging: &Path,
) -> LocalResult<RestoreResult> {
    let is_v2 = {
        let mut head = [0u8; 8];
        let mut f = File::open(path).map_err(io_err)?;
        f.read_exact(&mut head).is_ok() && &head == format::MAGIC
    };

    let (secrets, manifest) = {
        let path = path.to_path_buf();
        let password = password.to_string();
        let staging = staging.to_path_buf();
        tokio::task::spawn_blocking(move || {
            if is_v2 {
                unpack_v2(&path, &password, &staging)
            } else {
                unpack_legacy_zip(&path, &password, &staging)
            }
        })
        .await
        .map_err(io_err)??
    };

    if let Some(m) = &manifest {
        if m.schema_version > crate::local_server::db::latest_migration_version() {
            return Err(LocalError::BadRequest(
                "cette sauvegarde a été créée par une version plus récente de Motodesk ; mettez l'application à jour avant de la restaurer".into(),
            ));
        }
    }

    let staged_db = staging.join(DB_ENTRY);
    if !staged_db.exists() {
        return Err(LocalError::BadRequest("la sauvegarde ne contient pas de base de données".into()));
    }
    verify_database(&staged_db, &secrets).await?;

    // Archives first, the database last: the database file under its pending
    // name is what arms the restore, so nothing can fail after it.
    let staged_archives = staging.join("archives");
    if staged_archives.is_dir() {
        let dest = state.data_dir.join("archives");
        fs::create_dir_all(&dest).map_err(io_err)?;
        for entry in fs::read_dir(&staged_archives).map_err(io_err)?.flatten() {
            fs::copy(entry.path(), dest.join(entry.file_name())).map_err(io_err)?;
        }
    }
    adopt_secrets(&staged_db, &secrets, manifest.as_ref().map(|m| m.created_at.as_str())).await?;
    let pending = state.data_dir.join(PENDING_DB);
    rename_patiently(&staged_db, &pending)
        .await
        .map_err(|e| io_err(anyhow::anyhow!("mise en attente de la base ({} -> {}) : {e}", staged_db.display(), pending.display())))?;

    Ok(RestoreResult {
        restart_required: true,
        safety_copy_note: format!(
            "Les données actuelles sont conservées dans « {SAFETY_COPY_PREFIX}… » à côté de la base."
        ),
    })
}

fn sanitize_entry_name(name: &str) -> Option<&str> {
    let rest = name.strip_prefix(ARCHIVES_PREFIX)?;
    let bad = rest.is_empty()
        || rest.contains('/')
        || rest.contains('\\')
        || rest.contains("..")
        || rest.contains(':')
        || rest.starts_with('.');
    (!bad).then_some(rest)
}

/// Decrypt a format 2 backup into the staging folder.
fn unpack_v2(
    path: &Path,
    password: &str,
    staging: &Path,
) -> LocalResult<(SecretsPayload, Option<BackupManifest>)> {
    let file = BufReader::new(File::open(path).map_err(io_err)?);
    let dec = DecryptReader::open(file, password).map_err(|e| match e {
        OpenError::BadPassword => LocalError::BadRequest("mot de passe de restauration incorrect, ou fichier de sauvegarde endommagé".into()),
        OpenError::NotABackup => LocalError::BadRequest(e.to_string()),
        OpenError::Corrupt(_) => LocalError::BadRequest(e.to_string()),
        OpenError::Io(err) => io_err(err),
    })?;
    let mut gz = GzDecoder::new(dec);

    let bad = |e: std::io::Error| LocalError::BadRequest(format!("sauvegarde illisible : {e}"));
    let mut secrets = SecretsPayload::default();
    let mut manifest = None;
    let mut saw_db = false;

    while let Some(entry) = format::next_entry(&mut gz).map_err(bad)? {
        let mut data = (&mut gz).take(entry.len);
        match entry.name.as_str() {
            MANIFEST_ENTRY | SECRETS_ENTRY => {
                if entry.len > 1 << 20 {
                    return Err(LocalError::BadRequest("sauvegarde illisible : entrée trop grande".into()));
                }
                let mut buf = Vec::new();
                data.read_to_end(&mut buf).map_err(bad)?;
                if entry.name == MANIFEST_ENTRY {
                    manifest = serde_json::from_slice(&buf).ok();
                } else {
                    secrets = serde_json::from_slice(&buf)
                        .map_err(|e| LocalError::BadRequest(format!("clés illisibles : {e}")))?;
                }
            }
            DB_ENTRY => {
                let db_path = staging.join(DB_ENTRY);
            let mut out = BufWriter::new(File::create(&db_path).map_err(io_at("création de la base", &db_path))?);
                let copied = std::io::copy(&mut data, &mut out).map_err(bad)?;
                out.flush().map_err(io_err)?;
                if copied != entry.len {
                    return Err(LocalError::BadRequest("sauvegarde tronquée".into()));
                }
                saw_db = true;
            }
            other => match sanitize_entry_name(other) {
                Some(name) => {
                    let dir = staging.join("archives");
                    fs::create_dir_all(&dir).map_err(io_err)?;
                    let mut out = File::create(dir.join(name)).map_err(io_err)?;
                    std::io::copy(&mut data, &mut out).map_err(bad)?;
                }
                // Unknown entries from a future version are skipped, not trusted.
                None => {
                    std::io::copy(&mut data, &mut std::io::sink()).map_err(bad)?;
                }
            },
        }
    }
    if !saw_db {
        return Err(LocalError::BadRequest("la sauvegarde ne contient pas de base de données".into()));
    }
    // Read to the very end so the compression checksum and the final encrypted
    // block are verified too: an archive cut short after its last entry is not
    // a complete backup.
    std::io::copy(&mut gz, &mut std::io::sink()).map_err(bad)?;
    Ok((secrets, manifest))
}

/// Old ZIP backups: database in clear, secrets under a weaker key.
fn unpack_legacy_zip(
    path: &Path,
    password: &str,
    staging: &Path,
) -> LocalResult<(SecretsPayload, Option<BackupManifest>)> {
    let file = File::open(path).map_err(io_err)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|_| LocalError::BadRequest("ce fichier n'est pas une sauvegarde Motodesk".into()))?;

    // The keys come first: a wrong password is refused before a single byte of
    // the (possibly large) database is written anywhere.
    let blob = {
        let mut entry = zip
            .by_name("secrets.enc")
            .map_err(|_| LocalError::BadRequest("archive sans clés chiffrées".into()))?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(io_err)?;
        buf
    };
    let secrets = legacy::decrypt_secrets(password, &blob)
        .map_err(|_| LocalError::BadRequest("mot de passe de restauration incorrect, ou fichier de sauvegarde endommagé".into()))?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| LocalError::BadRequest(e.to_string()))?;
        let name = entry.name().map_err(|e| LocalError::BadRequest(e.to_string()))?.replace('\\', "/");
        if name == DB_ENTRY {
            let db_path = staging.join(DB_ENTRY);
            let mut out = BufWriter::new(File::create(&db_path).map_err(io_at("création de la base", &db_path))?);
            std::io::copy(&mut entry, &mut out).map_err(io_at("écriture de la base", &db_path))?;
            out.flush().map_err(io_at("écriture de la base", &db_path))?;
        } else if name == "secrets.enc" {
            continue;
        } else if let Some(rest) = sanitize_entry_name(&name) {
            let dir = staging.join("archives");
            fs::create_dir_all(&dir).map_err(io_err)?;
            let mut out = File::create(dir.join(rest)).map_err(io_err)?;
            std::io::copy(&mut entry, &mut out).map_err(io_err)?;
        }
    }
    Ok((secrets, None))
}

/// Open the staged database and make sure it is one this app can run on, then
/// hand it the secrets that belong to it.
async fn verify_database(path: &Path, secrets: &SecretsPayload) -> LocalResult<()> {
    let pool = crate::local_server::db::init_pool(path)
        .await
        .map_err(|e| LocalError::BadRequest(format!("base de données illisible : {e}")))?;

    let outcome = verify_open_database(&pool, secrets).await;
    close_into_one_file(pool, path).await;
    outcome
}

/// Fold the write-ahead log into the file so a single file is what gets queued.
async fn close_into_one_file(pool: sqlx::SqlitePool, path: &Path) {
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&pool).await;
    pool.close().await;
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

async fn verify_open_database(pool: &sqlx::SqlitePool, secrets: &SecretsPayload) -> LocalResult<()> {
    let (check,): (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .map_err(|e| LocalError::BadRequest(format!("base de données illisible : {e}")))?;
    if check != "ok" {
        return Err(LocalError::BadRequest(format!(
            "la base de données de la sauvegarde est endommagée ({check})"
        )));
    }

    let (is_motodesk,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sync_state'")
            .fetch_one(pool)
            .await?;
    if is_motodesk == 0 {
        return Err(LocalError::BadRequest("ce fichier ne contient pas une base Motodesk".into()));
    }

    let newest: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    if let Some((Some(v),)) = newest {
        if v > crate::local_server::db::latest_migration_version() {
            return Err(LocalError::BadRequest(
                "cette sauvegarde a été créée par une version plus récente de Motodesk ; mettez l'application à jour avant de la restaurer".into(),
            ));
        }
    }

    // A key must be the one whose public half the restored journal names. Nothing is stored
    // here: this runs before the restore is certain, and the keychain holds the keys of the
    // data that is still live.
    for (code, hex) in &secrets.fiscal_keys {
        let recorded: Option<(String,)> =
            sqlx::query_as("SELECT public_key FROM fiscal_registers WHERE code = $1")
                .bind(code)
                .fetch_optional(pool)
                .await?;
        if let (Some((recorded,)), Ok(key)) = (recorded, motodesk_fiscal::SecretKey::from_hex(hex)) {
            if key.public().to_hex() != recorded {
                return Err(LocalError::BadRequest(format!(
                    "la clé de signature de la caisse {code} ne correspond pas au journal de la sauvegarde"
                )));
            }
        }
    }
    Ok(())
}

/// Hand the backup's keys to this computer: the OS keychain, or where there is none, the
/// restored database itself (which is where the app looks next). Done last, once nothing
/// can fail any more, so a restore that is refused never replaces a key.
async fn adopt_secrets(path: &Path, secrets: &SecretsPayload, made_at: Option<&str>) -> LocalResult<()> {
    let pool = crate::local_server::db::init_pool(path)
        .await
        .map_err(|e| LocalError::BadRequest(format!("base de données illisible : {e}")))?;
    let mut outcome = store_secrets(&pool, secrets).await;
    // The copy also holds whoever was signed in when it was made. Restoring must not sign
    // anyone in without their password.
    if outcome.is_ok() {
        outcome = sqlx::query("DELETE FROM cached_session")
            .execute(&pool)
            .await
            .map(|_| ())
            .map_err(LocalError::from);
    }
    // The copy was taken before "last backup" was written down, so the restored data would
    // claim it was never backed up. It is exactly as recent as the backup it came from.
    if outcome.is_ok() {
        if let Some(made_at) = made_at {
            let _ = sqlx::query("UPDATE sync_state SET last_backup_at = $1 WHERE id = 1")
                .bind(made_at)
                .execute(&pool)
                .await;
        }
    }
    close_into_one_file(pool, path).await;
    outcome
}

async fn store_secrets(pool: &sqlx::SqlitePool, secrets: &SecretsPayload) -> LocalResult<()> {
    for (code, hex) in &secrets.fiscal_keys {
        if let Err(e) = crate::local_server::secrets::store_fiscal_key(code, hex) {
            tracing::warn!(error = %e, register = %code, "keychain unavailable; fiscal key stored in the database");
            sqlx::query(
                "INSERT OR REPLACE INTO fiscal_key_fallback (register_code, secret_key, created_at) VALUES ($1, $2, $3)",
            )
            .bind(code)
            .bind(hex)
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        }
    }
    if let Some(secret) = &secrets.mesh_secret {
        if let Err(e) = crate::local_server::secrets::store_mesh_secret(secret) {
            tracing::warn!(error = %e, "keychain unavailable; mesh secret stored in the database");
            sqlx::query("UPDATE sync_state SET mesh_secret = $1 WHERE id = 1")
                .bind(secret)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

// ----------------------------------------------------------------- auto -----

pub fn spawn_auto(state: SharedState) {
    tokio::spawn(async move {
        // The first look comes soon after start, not an hour later: the app is
        // often opened for a shift and closed again.
        tokio::time::sleep(Duration::from_secs(120)).await;
        loop {
            if let Err(e) = maybe_auto_backup(&state).await {
                tracing::warn!(error = %e, "automatic backup skipped");
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

async fn maybe_auto_backup(state: &SharedState) -> LocalResult<()> {
    if !setting_bool(state, "system.auto_backup_enabled", true).await {
        return Ok(());
    }
    let days = setting_i64(state, "system.backup_frequency_days", 7).await.max(1);
    let last: Option<(Option<String>,)> =
        sqlx::query_as("SELECT last_backup_at FROM sync_state WHERE id = 1")
            .fetch_optional(&state.db)
            .await?;
    if let Some(Some(ts)) = last.map(|(t,)| t) {
        if let Ok(when) = chrono::DateTime::parse_from_rfc3339(&ts) {
            let age = Utc::now().signed_duration_since(when.with_timezone(&Utc));
            if age.num_days() < days {
                return Ok(());
            }
        }
    }
    // The password is one the owner chose and wrote down. The old scheme derived
    // it from the device id, which sat in the clear inside the same file, and
    // gave the owner nothing to type when restoring.
    let Some(password) = crate::local_server::secrets::load_backup_password().ok().flatten() else {
        tracing::info!("automatic backup waiting for a backup password (Réglages > Sauvegardes)");
        return Ok(());
    };
    let file = write_backup(state, &password, "auto").await?;
    tracing::info!(path = %file.path, "automatic shop backup written");
    prune_auto(&backups_dir(&state.data_dir));
    Ok(())
}

fn prune_auto(dir: &Path) {
    let mut autos: Vec<PathBuf> = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("auto-") && n.ends_with(&format!(".{EXT}")))
        })
        .collect();
    autos.sort();
    while autos.len() > AUTO_KEPT {
        let _ = fs::remove_file(autos.remove(0));
    }
}

pub fn set_auto_password(password: &str) -> LocalResult<()> {
    check_password(password)?;
    crate::local_server::secrets::store_backup_password(password).map_err(|e| {
        LocalError::BadRequest(format!(
            "le trousseau du système est indisponible, la sauvegarde automatique ne peut pas être activée ({e})"
        ))
    })
}

pub fn clear_auto_password() -> LocalResult<()> {
    crate::local_server::secrets::clear_backup_password().map_err(io_err)
}

// -------------------------------------------------------------- helpers -----

async fn vacuum_into(pool: &sqlx::SqlitePool, dest: &Path) -> LocalResult<()> {
    let _ = fs::remove_file(dest);
    // A quote in the path (a user name such as O'Brien) would end the literal.
    let path = dest.to_string_lossy().replace('\\', "/").replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{path}'")).execute(pool).await?;
    Ok(())
}

async fn collect_secrets(state: &SharedState) -> LocalResult<SecretsPayload> {
    let mesh_secret = crate::local_server::secrets::load_mesh_secret()
        .ok()
        .flatten()
        .or(crate::local_server::db::load_mesh_secret(&state.db).await.ok().flatten());
    let codes: Vec<(String,)> = sqlx::query_as("SELECT code FROM fiscal_registers")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    let mut fiscal_keys = Vec::new();
    for (code,) in codes {
        if let Ok(Some(hex)) = crate::local_server::secrets::load_fiscal_key(&code) {
            fiscal_keys.push((code, hex));
            continue;
        }
        if let Ok(Some((hex,))) = sqlx::query_as::<_, (String,)>(
            "SELECT secret_key FROM fiscal_key_fallback WHERE register_code = $1",
        )
        .bind(&code)
        .fetch_optional(&state.db)
        .await
        {
            fiscal_keys.push((code, hex));
        }
    }
    Ok(SecretsPayload { mesh_secret, fiscal_keys })
}

fn list_files(dir: &Path) -> Vec<BackupFile> {
    let mut files: Vec<BackupFile> = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension().and_then(|s| s.to_str())?;
            if ext != EXT && ext != "zip" {
                return None;
            }
            file_info(&path)
        })
        .collect();
    files.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    files
}

fn file_info(path: &Path) -> Option<BackupFile> {
    let meta = fs::metadata(path).ok()?;
    let created = meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<Utc>::from(t).to_rfc3339())
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let filename = path.file_name()?.to_string_lossy().into_owned();
    let kind = if filename.ends_with(".zip") {
        "legacy"
    } else if filename.starts_with("auto-") {
        "automatic"
    } else {
        "manual"
    };
    Some(BackupFile {
        filename,
        path: path.display().to_string(),
        size_bytes: meta.len(),
        created_at: created,
        kind: kind.into(),
    })
}

fn sanitize_filename(name: &str) -> LocalResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.contains(':')
    {
        return Err(LocalError::BadRequest("nom de fichier invalide".into()));
    }
    Ok(trimmed.to_string())
}

async fn setting_bool(state: &SharedState, key: &str, default: bool) -> bool {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM app_settings WHERE key = $1 AND deleted_at IS NULL")
            .bind(key)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    match row {
        Some((v,)) => matches!(v.trim().trim_matches('"'), "true" | "1" | "yes"),
        None => default,
    }
}

async fn setting_i64(state: &SharedState, key: &str, default: i64) -> i64 {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM app_settings WHERE key = $1 AND deleted_at IS NULL")
            .bind(key)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    row.and_then(|(v,)| v.trim().trim_matches('"').parse().ok()).unwrap_or(default)
}

/// Format 1, read-only: what the old ZIP backups used for their secrets.
mod legacy {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use sha2::{Digest, Sha256};

    use super::SecretsPayload;

    fn key_from_password(password: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"motogest-backup-v1");
        hasher.update(password.as_bytes());
        hasher.finalize().into()
    }

    pub fn decrypt_secrets(password: &str, blob: &[u8]) -> anyhow::Result<SecretsPayload> {
        if blob.len() < 13 {
            anyhow::bail!("blob too short");
        }
        let cipher = Aes256Gcm::new_from_slice(&key_from_password(password))?;
        let plain = cipher
            .decrypt(Nonce::from_slice(&blob[..12]), &blob[12..])
            .map_err(|e| anyhow::anyhow!("decrypt: {e}"))?;
        Ok(serde_json::from_slice(&plain)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::shop;

    #[test]
    fn rejects_path_escape() {
        assert!(sanitize_filename("../x.mgbak").is_err());
        assert!(sanitize_filename("C:evil.mgbak").is_err());
        assert!(sanitize_filename("motogest-ok.mgbak").is_ok());
        assert_eq!(sanitize_entry_name("archives/2026.csv"), Some("2026.csv"));
        assert_eq!(sanitize_entry_name("archives/../evil"), None);
        assert_eq!(sanitize_entry_name("archives/sub/x"), None);
        assert_eq!(sanitize_entry_name("local.db"), None);
    }

    #[test]
    fn production_key_derivation_is_expensive() {
        assert!(DEFAULT_ITERATIONS >= 600_000);
    }

    #[test]
    fn short_passwords_are_refused() {
        assert!(check_password("court").is_err());
        assert!(check_password("0123456789").is_ok());
    }

    #[test]
    fn a_restore_keeps_the_database_it_replaces() {
        let dir = std::env::temp_dir().join(format!("motodesk_restore_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("local.db"), b"old data").unwrap();
        fs::write(dir.join("local.db-wal"), b"old wal").unwrap();
        fs::write(dir.join(PENDING_DB), b"restored data").unwrap();

        assert!(apply_pending_restore(&dir).unwrap());
        assert_eq!(fs::read(dir.join("local.db")).unwrap(), b"restored data");
        assert!(!dir.join(PENDING_DB).exists());
        let kept: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(SAFETY_COPY_PREFIX))
            .collect();
        assert!(kept.iter().any(|n| !n.ends_with("-wal")), "the old database is kept: {kept:?}");
        let old = kept.iter().find(|n| !n.ends_with("-wal")).unwrap();
        assert_eq!(fs::read(dir.join(old)).unwrap(), b"old data");

        // Nothing pending: nothing happens.
        assert!(!apply_pending_restore(&dir).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_latest_safety_copies_are_kept() {
        let dir = std::env::temp_dir().join(format!("motodesk_prune_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        for i in 1..=5 {
            fs::write(dir.join(format!("{SAFETY_COPY_PREFIX}2026010{i}-000000")), b"x").unwrap();
        }
        prune_safety_copies(&dir);
        let n = fs::read_dir(&dir).unwrap().count();
        assert_eq!(n, SAFETY_COPIES_KEPT);
        assert!(dir.join(format!("{SAFETY_COPY_PREFIX}20260105-000000")).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The whole path: back up a live database, wipe it, restore, find the data.
    #[tokio::test]
    async fn a_backup_restores_into_a_pending_database_with_its_data() {
        let state = shop().await;
        sqlx::query(
            "INSERT INTO clients (id, last_name, created_at, updated_at, row_version, field_meta)
             VALUES ('c1', 'Martin', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, '{}')",
        )
        .execute(&state.db)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO cached_session (id, user_json, tokens_json, cached_at)
             VALUES (1, '{\"id\":\"u1\",\"role\":\"admin\"}', '{}', '2026-01-01T00:00:00Z')",
        )
        .execute(&state.db)
        .await
        .unwrap();

        // A fast key derivation would be nicer, but the production path is what
        // must work: the cost is a second or two in a test.
        let file = export(&state, "un-mot-de-passe-solide").await.unwrap();
        assert!(file.filename.ends_with(".mgbak"));
        assert_eq!(file.kind, "manual");

        // What is on disk is not readable.
        let raw = fs::read(&file.path).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"Martin"));
        assert!(!raw.windows(15).any(|w| w == b"SQLite format 3"));

        let wrong = restore_named(&state, &file.filename, "pas-le-bon-mot-de-passe").await;
        assert!(matches!(wrong, Err(LocalError::BadRequest(m)) if m.contains("incorrect")));
        assert!(!state.data_dir.join(PENDING_DB).exists(), "a failed restore queues nothing");
        assert!(!state.data_dir.join(STAGING_DIR).exists(), "and leaves no scratch folder");

        let ok = restore_named(&state, &file.filename, "un-mot-de-passe-solide").await.unwrap();
        assert!(ok.restart_required);
        let pending = state.data_dir.join(PENDING_DB);
        assert!(pending.exists());
        assert!(!state.data_dir.join(STAGING_DIR).exists());

        let pool = crate::local_server::db::init_pool(&pending).await.unwrap();
        let (name,): (String,) = sqlx::query_as("SELECT last_name FROM clients WHERE id = 'c1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(name, "Martin");
        // Whoever was signed in when the copy was made is not signed in by restoring it.
        let (sessions,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cached_session")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(sessions, 0);
        // The restored data knows how recent its backup is (it was copied before that was written).
        let (last,): (Option<String>,) = sqlx::query_as("SELECT last_backup_at FROM sync_state WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(last.is_some_and(|t| t.starts_with("20")), "last_backup_at should name the backup's own time");
        pool.close().await;

        // No half-written files are left in the backups folder.
        let leftovers: Vec<String> = fs::read_dir(backups_dir(&state.data_dir))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.') || n.ends_with(".partial"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// Backups written by the previous version (a ZIP with the database in clear)
    /// must still be restorable: they are what exists on disk today.
    #[tokio::test]
    async fn an_old_zip_backup_can_still_be_restored() {
        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};
        use sha2::{Digest, Sha256};

        let state = shop().await;
        sqlx::query(
            "INSERT INTO clients (id, last_name, created_at, updated_at, row_version, field_meta)
             VALUES ('old1', 'Ancien', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, '{}')",
        )
        .execute(&state.db)
        .await
        .unwrap();
        let dir = backups_dir(&state.data_dir);
        fs::create_dir_all(&dir).unwrap();
        let db_copy = dir.join("legacy-source.db");
        vacuum_into(&state.db, &db_copy).await.unwrap();

        // Exactly what the old exporter produced.
        let mut hasher = Sha256::new();
        hasher.update(b"motogest-backup-v1");
        hasher.update(b"ancien-mot-de-passe");
        let key: [u8; 32] = hasher.finalize().into();
        let nonce = [7u8; 12];
        let mut blob = nonce.to_vec();
        blob.extend(
            Aes256Gcm::new_from_slice(&key)
                .unwrap()
                .encrypt(Nonce::from_slice(&nonce), br#"{"mesh_secret":null,"fiscal_keys":[]}"#.as_ref())
                .unwrap(),
        );
        let zip_path = dir.join("motogest-20260829-120000.zip");
        {
            let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("manifest.json", opts).unwrap();
            zip.write_all(br#"{"format":1}"#).unwrap();
            zip.start_file("local.db", opts).unwrap();
            zip.write_all(&fs::read(&db_copy).unwrap()).unwrap();
            zip.start_file("secrets.enc", opts).unwrap();
            zip.write_all(&blob).unwrap();
            zip.finish().unwrap();
        }
        fs::remove_file(&db_copy).unwrap();

        let listed = list_files(&dir);
        assert!(listed.iter().any(|f| f.filename.ends_with(".zip") && f.kind == "legacy"));

        let wrong = restore_named(&state, "motogest-20260829-120000.zip", "pas-le-bon-mot-de-passe").await;
        assert!(wrong.is_err());
        restore_named(&state, "motogest-20260829-120000.zip", "ancien-mot-de-passe").await.unwrap();

        let pool = crate::local_server::db::init_pool(&state.data_dir.join(PENDING_DB)).await.unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clients WHERE id = 'old1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        pool.close().await;
    }

    /// A backup carrying a signing key that does not belong to its own journal is refused,
    /// and refusing it must not have replaced the key this computer is using.
    #[tokio::test]
    async fn a_key_that_does_not_match_the_journal_is_refused_before_it_is_stored() {
        let state = shop().await;
        let code = format!("TEST-ORDER-{}", uuid::Uuid::new_v4());
        let journal_key = motodesk_fiscal::SecretKey::generate();
        sqlx::query(
            "INSERT INTO fiscal_registers (id, code, public_key, software_version, opened_at)
             VALUES ('r1', $1, $2, 'test', '2026-01-01T00:00:00Z')",
        )
        .bind(&code)
        .bind(journal_key.public().to_hex())
        .execute(&state.db)
        .await
        .unwrap();
        let dir = backups_dir(&state.data_dir);
        fs::create_dir_all(&dir).unwrap();
        let copy = dir.join("mismatch.db");
        vacuum_into(&state.db, &copy).await.unwrap();

        let someone_elses_key = motodesk_fiscal::SecretKey::generate().to_hex();
        let secrets = SecretsPayload { mesh_secret: None, fiscal_keys: vec![(code.clone(), someone_elses_key)] };
        let refused = verify_database(&copy, &secrets).await;
        assert!(matches!(refused, Err(LocalError::BadRequest(m)) if m.contains("ne correspond pas")));
        assert_eq!(
            crate::local_server::secrets::load_fiscal_key(&code).unwrap(),
            None,
            "the keychain was written to by a restore that was then refused"
        );

        // The matching key passes the same check (and still stores nothing).
        let secrets = SecretsPayload { mesh_secret: None, fiscal_keys: vec![(code.clone(), journal_key.to_hex())] };
        verify_database(&copy, &secrets).await.unwrap();
        assert_eq!(crate::local_server::secrets::load_fiscal_key(&code).unwrap(), None);
    }

    #[tokio::test]
    async fn a_file_that_is_not_a_backup_is_refused_cleanly() {
        let state = shop().await;
        let dir = backups_dir(&state.data_dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("junk.mgbak"), b"this is not a backup").unwrap();
        let err = restore_named(&state, "junk.mgbak", "un-mot-de-passe-solide").await.unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
        assert!(!state.data_dir.join(PENDING_DB).exists());
    }
}
