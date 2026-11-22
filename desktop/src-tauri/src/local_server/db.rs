use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{ConnectOptions, SqlitePool};

/// Open the local SQLite store with professional-grade PRAGMAs.
///
/// We enable WAL, NORMAL synchronous, foreign keys, and a generous busy timeout so
/// the embedded API can absorb concurrent UI writes + sync engine reads without
/// "database is locked" errors. The cache and mmap hints keep daily-use queries
/// resident in memory for sub-millisecond reads.
pub async fn init_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    let mut opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        .pragma("temp_store", "MEMORY")
        .pragma("cache_size", "-32000") // ~32 MiB
        .pragma("mmap_size", "268435456") // 256 MiB
        .pragma("journal_size_limit", "67108864"); // 64 MiB WAL cap
    opts = opts.disable_statement_logging();

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await?;

    Ok(pool)
}

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    MIGRATOR.run(pool).await?;
    Ok(())
}

/// How many copies made before an update are kept.
const PRE_MIGRATION_COPIES: usize = 3;
/// Folder, next to the database, holding those copies.
pub const PRE_MIGRATION_DIR: &str = "pre-migration";

/// Newest update already applied to this database, `None` for a database that is new.
async fn applied_version(pool: &SqlitePool) -> Option<i64> {
    sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(pool)
        .await
        .ok()
        .flatten()
}

/// Bring the database up to this build's structure without putting the shop's data at risk:
///
/// - a database written by a newer build is left alone (opening it with older code could
///   damage it), and the interface says so;
/// - when updates are pending, a consistent copy of the database is kept first, so a bad
///   update, or a build that turns out to be wrong, can be undone by hand;
/// - an update that fails is rolled back by the database itself (each one is a transaction).
pub async fn migrate_safely(pool: &SqlitePool, data_dir: &Path) -> anyhow::Result<()> {
    migrate_with(pool, &MIGRATOR, data_dir).await
}

async fn migrate_with(
    pool: &SqlitePool,
    migrator: &sqlx::migrate::Migrator,
    data_dir: &Path,
) -> anyhow::Result<()> {
    let known = migrator.iter().map(|m| m.version).max().unwrap_or(0);
    if let Some(found) = applied_version(pool).await {
        if found > known {
            return Err(crate::startup::StartupError::DatabaseTooNew { found, known }.into());
        }
        if found < known {
            if let Err(e) = keep_copy_before_update(pool, data_dir, found, known).await {
                // Not being able to copy (a full disk, say) must not lock the shop out of
                // its own application; the update itself is transactional.
                tracing::warn!(error = %e, "no copy made before the database update");
            }
        }
    }
    migrator
        .run(pool)
        .await
        .map_err(|e| crate::startup::StartupError::Migration(e.to_string()).into())
}

async fn keep_copy_before_update(
    pool: &SqlitePool,
    data_dir: &Path,
    from: i64,
    to: i64,
) -> anyhow::Result<()> {
    let dir = data_dir.join(PRE_MIGRATION_DIR);
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let copy = dir.join(format!("local-{stamp}-v{from:03}-to-v{to:03}.db"));
    let _ = std::fs::remove_file(&copy);
    // VACUUM INTO writes a consistent single file, whatever is in the write-ahead log.
    // A quote in the path (a user name such as O'Brien) would end the SQL literal.
    let literal = copy.to_string_lossy().replace('\\', "/").replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{literal}'")).execute(pool).await?;
    tracing::info!(from, to, copy = %copy.display(), "database copied before its update");

    let mut copies: Vec<_> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "db"))
        .collect();
    copies.sort();
    while copies.len() > PRE_MIGRATION_COPIES {
        let _ = std::fs::remove_file(copies.remove(0));
    }
    Ok(())
}

/// Newest schema migration this build knows. A backup made by a newer build
/// carries a higher number and must not be restored over this one.
pub fn latest_migration_version() -> i64 {
    MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0)
}

pub async fn ensure_device_id(pool: &SqlitePool) -> anyhow::Result<uuid::Uuid> {
    let row: Option<(String,)> = sqlx::query_as("SELECT device_id FROM sync_state WHERE id = 1")
        .fetch_optional(pool)
        .await?;

    let id = if let Some((id,)) = row {
        if let Ok(parsed) = uuid::Uuid::parse_str(&id) {
            parsed
        } else {
            let new_id = uuid::Uuid::new_v4();
            sqlx::query("UPDATE sync_state SET device_id = $1 WHERE id = 1")
                .bind(new_id.to_string())
                .execute(pool)
                .await?;
            new_id
        }
    } else {
        let new_id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO sync_state (id, device_id) VALUES (1, $1)")
            .bind(new_id.to_string())
            .execute(pool)
            .await?;
        new_id
    };

    Ok(id)
}

/// Load the cloud-issued company mesh secret (the LAN trust root). `None` until
/// the user has logged in and the secret has been fetched from the cloud.
pub async fn load_mesh_secret(pool: &SqlitePool) -> anyhow::Result<Option<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT mesh_secret FROM sync_state WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(s,)| s).filter(|s| !s.is_empty()))
}

/// Load the company id this install belongs to, if known.
pub async fn load_company_id(pool: &SqlitePool) -> anyhow::Result<Option<uuid::Uuid>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT company_id FROM sync_state WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(row
        .and_then(|(s,)| s)
        .and_then(|s| uuid::Uuid::parse_str(&s).ok()))
}

/// The cloud address this build was made for. Set `MOTODESK_CLOUD_URL` when building
/// a release (the sign-in screen reads the same variable); without it the build points
/// at a backend on the same computer, which is only right for development.
pub const BUILT_IN_CLOUD_URL: &str = match option_env!("MOTODESK_CLOUD_URL") {
    Some(url) => url,
    None => "http://127.0.0.1:8080",
};

/// True for an address that is this very computer.
pub fn is_loopback_url(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        // [::1]:8080
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "0.0.0.0")
}

/// A release build that still points at its own computer cannot reach any cloud.
pub fn cloud_url_misconfigured(url: &str) -> bool {
    !cfg!(debug_assertions) && is_loopback_url(url)
}

/// Which address to use: the one stored on this PC, unless it is missing, malformed,
/// or a development leftover (this computer) that the build now knows better than.
fn choose_cloud_url(stored: Option<&str>, default: &str) -> String {
    match stored.filter(|s| s.starts_with("http://") || s.starts_with("https://")) {
        Some(s) if is_loopback_url(s) && !is_loopback_url(default) => default.to_string(),
        Some(s) => s.to_string(),
        None => default.to_string(),
    }
}

/// Ensure `sync_state.cloud_url` points at the cloud backend (not a stale local port).
pub async fn ensure_cloud_url(pool: &SqlitePool) -> anyhow::Result<String> {
    let default = std::env::var("MOTODESK_CLOUD_URL")
        .unwrap_or_else(|_| BUILT_IN_CLOUD_URL.to_string());

    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT cloud_url FROM sync_state WHERE id = 1")
            .fetch_optional(pool)
            .await?;

    let url = choose_cloud_url(row.and_then(|(u,)| u).as_deref(), &default);
    if cloud_url_misconfigured(&url) {
        tracing::error!(
            %url,
            "this release build points at its own computer: it cannot reach any cloud. Rebuild with MOTODESK_CLOUD_URL set."
        );
    }

    sqlx::query("UPDATE sync_state SET cloud_url = $1 WHERE id = 1")
        .bind(&url)
        .execute(pool)
        .await?;

    Ok(url)
}

pub async fn device_id(pool: &SqlitePool) -> anyhow::Result<uuid::Uuid> {
    let (id,): (String,) = sqlx::query_as("SELECT device_id FROM sync_state WHERE id = 1")
        .fetch_one(pool)
        .await?;
    Ok(uuid::Uuid::parse_str(&id)?)
}

/// Allocate the next sequence number for this device, transactionally.
///
/// Combined with the `UNIQUE (device_id, seq)` constraint on `change_log`, this
/// prevents duplicate seq values under concurrent writes.
///
/// The count never goes back below the highest number ever handed out
/// (`sync_state.last_seq`), even when the log rows that held it are gone: old
/// delivered changes are pruned, and a restored backup brings back an older log.
pub async fn next_seq(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    device_id: uuid::Uuid,
) -> anyhow::Result<i64> {
    let (top,): (i64,) = sqlx::query_as(
        "SELECT MAX(
             COALESCE((SELECT MAX(seq) FROM change_log WHERE device_id = $1), 0),
             COALESCE((SELECT last_seq FROM sync_state WHERE id = 1), 0))",
    )
    .bind(device_id.to_string())
    .fetch_one(&mut **tx)
    .await?;

    let next = top + 1;
    sqlx::query("UPDATE sync_state SET last_seq = $1 WHERE id = 1")
        .bind(next)
        .execute(&mut **tx)
        .await?;
    Ok(next)
}

/// What lining this PC's numbering up with the cloud's did.
#[derive(Debug, PartialEq, Eq)]
pub struct SequenceAlignment {
    /// Changes the cloud holds from this PC that this database does not.
    pub missing: i64,
    /// Queued changes renumbered because the cloud already had those numbers.
    pub renumbered: u32,
}

/// Number this PC's changes past what the cloud has already received from it.
///
/// A restored backup comes back with an old log and an old counter, so the next
/// changes reuse numbers the cloud has seen. The cloud refuses a number twice (and
/// so would every other PC), which left the restored PC unable to send anything,
/// for good. `cloud_last` is the highest number the cloud holds for this device.
pub async fn align_sequence(
    pool: &sqlx::SqlitePool,
    device_id: uuid::Uuid,
    cloud_last: i64,
) -> anyhow::Result<Option<SequenceAlignment>> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let device = device_id.to_string();
    let (top,): (i64,) = sqlx::query_as(
        "SELECT MAX(
             COALESCE((SELECT MAX(seq) FROM change_log WHERE device_id = $1), 0),
             COALESCE((SELECT last_seq FROM sync_state WHERE id = 1), 0))",
    )
    .bind(&device)
    .fetch_one(&mut *tx)
    .await?;

    // Queued changes numbered inside what the cloud already holds.
    let clashing: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM change_log
          WHERE device_id = $1 AND synced_to_cloud = 0 AND seq <= $2
          ORDER BY seq",
    )
    .bind(&device)
    .bind(cloud_last)
    .fetch_all(&mut *tx)
    .await?;

    let missing = (cloud_last - top).max(0);
    if clashing.is_empty() && missing == 0 {
        return Ok(None);
    }

    // Everything new goes above every number that exists, here or in the cloud.
    let mut next = top.max(cloud_last) + 1;
    for (id,) in &clashing {
        sqlx::query("UPDATE change_log SET seq = $2 WHERE id = $1")
            .bind(id)
            .bind(next)
            .execute(&mut *tx)
            .await?;
        next += 1;
    }
    sqlx::query(
        "UPDATE sync_state SET last_seq = $1, seq_gap = seq_gap + $2,
                seq_recover_from = CASE WHEN $2 > 0 AND seq_gap = 0 THEN $3 ELSE seq_recover_from END
          WHERE id = 1",
    )
    .bind(next - 1)
    .bind(missing)
    .bind(top)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(SequenceAlignment {
        missing,
        renumbered: clashing.len() as u32,
    }))
}

#[cfg(test)]
mod seq_tests {
    use super::*;
    use crate::local_server::test_support::shop;
    use sqlx::SqlitePool;
    use uuid::Uuid;

    async fn own_op(pool: &SqlitePool, device: Uuid, seq: i64, synced: bool) -> String {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO change_log (id, device_id, seq, entity_type, entity_id, op, payload, field_meta, created_at, synced_to_cloud, relayed)
             VALUES ($1, $2, $3, 'client', $4, 'insert', '{}', '{}', '2026-11-15T00:00:00Z', $5, 0)",
        )
        .bind(&id)
        .bind(device.to_string())
        .bind(seq)
        .bind(Uuid::new_v4().to_string())
        .bind(synced as i64)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn seqs(pool: &SqlitePool, synced: bool) -> Vec<i64> {
        sqlx::query_scalar("SELECT seq FROM change_log WHERE synced_to_cloud = $1 ORDER BY seq")
            .bind(synced as i64)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    async fn allocate(pool: &SqlitePool, device: Uuid) -> i64 {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let n = next_seq(&mut tx, device).await.unwrap();
        tx.commit().await.unwrap();
        n
    }

    #[tokio::test]
    async fn a_restored_pc_numbers_its_changes_past_what_the_cloud_already_has() {
        let state = shop().await;
        let device = state.device_id().await.unwrap();
        sqlx::query("DELETE FROM change_log").execute(&state.db).await.unwrap();
        sqlx::query("UPDATE sync_state SET last_seq = 5 WHERE id = 1").execute(&state.db).await.unwrap();

        // The backup held changes 1..5, all delivered. Since the restore the PC has
        // queued two more: 6 and 7, numbers the cloud gave to other changes.
        for seq in 1..=5 {
            own_op(&state.db, device, seq, true).await;
        }
        for _ in 0..2 {
            let n = allocate(&state.db, device).await;
            own_op(&state.db, device, n, false).await;
        }
        assert_eq!(seqs(&state.db, false).await, vec![6, 7]);

        // The cloud has received everything up to 20 from this device.
        let done = align_sequence(&state.db, device, 20).await.unwrap().unwrap();
        assert_eq!(done, SequenceAlignment { missing: 13, renumbered: 2 });
        assert_eq!(seqs(&state.db, false).await, vec![21, 22], "queued changes move past the cloud's");
        assert_eq!(seqs(&state.db, true).await, vec![1, 2, 3, 4, 5], "delivered ones are untouched");
        assert_eq!(allocate(&state.db, device).await, 23);

        let gap: i64 = sqlx::query_scalar("SELECT seq_gap FROM sync_state WHERE id = 1")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(gap, 13, "the user is told 13 changes are not in this base");

        // Asking again changes nothing.
        assert_eq!(align_sequence(&state.db, device, 20).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_pc_that_is_in_step_with_the_cloud_is_left_alone() {
        let state = shop().await;
        let device = state.device_id().await.unwrap();
        sqlx::query("DELETE FROM change_log").execute(&state.db).await.unwrap();
        sqlx::query("UPDATE sync_state SET last_seq = 0, seq_gap = 0 WHERE id = 1").execute(&state.db).await.unwrap();
        for seq in 1..=3 {
            own_op(&state.db, device, seq, true).await;
        }
        own_op(&state.db, device, 4, false).await;
        sqlx::query("UPDATE sync_state SET last_seq = 4 WHERE id = 1").execute(&state.db).await.unwrap();

        assert_eq!(align_sequence(&state.db, device, 3).await.unwrap(), None);
        assert_eq!(seqs(&state.db, false).await, vec![4]);
    }

    /// Delivered changes are pruned after 90 days. A PC idle since then has an empty
    /// log, and used to start numbering from 1 again.
    #[tokio::test]
    async fn numbering_survives_the_log_being_pruned() {
        let state = shop().await;
        let device = state.device_id().await.unwrap();
        sqlx::query("DELETE FROM change_log").execute(&state.db).await.unwrap();
        sqlx::query("UPDATE sync_state SET last_seq = 40 WHERE id = 1").execute(&state.db).await.unwrap();

        assert_eq!(allocate(&state.db, device).await, 41);
        assert_eq!(align_sequence(&state.db, device, 40).await.unwrap(), None);
    }
}

#[cfg(test)]
mod cloud_url_tests {
    use super::*;

    #[test]
    fn loopback_addresses_are_recognised() {
        for url in [
            "http://127.0.0.1:8080",
            "http://localhost:8080/api",
            "https://localhost",
            "http://[::1]:8080",
            "http://0.0.0.0:8080",
        ] {
            assert!(is_loopback_url(url), "{url}");
        }
        for url in ["https://cloud.example.fr", "https://cloud.motodesk.example/", "http://192.168.1.20:8080"] {
            assert!(!is_loopback_url(url), "{url}");
        }
    }

    #[test]
    fn a_corrected_build_replaces_a_stale_development_address() {
        let real = "https://cloud.example.fr";
        // A PC that ran a development build once keeps its own computer's address forever.
        assert_eq!(choose_cloud_url(Some("http://127.0.0.1:8080"), real), real);
        // A deliberate address (another server) is respected.
        assert_eq!(choose_cloud_url(Some("https://other.example.fr"), real), "https://other.example.fr");
        // Nothing stored, or garbage: the build's address.
        assert_eq!(choose_cloud_url(None, real), real);
        assert_eq!(choose_cloud_url(Some("pas une adresse"), real), real);
        // A development build keeps using its own computer.
        assert_eq!(
            choose_cloud_url(Some("http://127.0.0.1:8080"), "http://127.0.0.1:8080"),
            "http://127.0.0.1:8080"
        );
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use std::borrow::Cow;

    fn scratch() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("motodesk_migrate_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The structure as it was once: only the updates up to `version`.
    fn migrator_until(version: i64) -> sqlx::migrate::Migrator {
        sqlx::migrate::Migrator {
            migrations: Cow::Owned(MIGRATOR.iter().filter(|m| m.version <= version).cloned().collect()),
            ..sqlx::migrate::Migrator::DEFAULT
        }
    }

    async fn database_at(dir: &Path, version: i64) -> SqlitePool {
        let pool = init_pool(&dir.join("local.db")).await.unwrap();
        migrator_until(version).run(&pool).await.unwrap();
        pool
    }

    async fn healthy(pool: &SqlitePool) {
        let (check,): (String,) = sqlx::query_as("PRAGMA integrity_check").fetch_one(pool).await.unwrap();
        assert_eq!(check, "ok");
        let broken = sqlx::query("PRAGMA foreign_key_check").fetch_all(pool).await.unwrap();
        assert!(broken.is_empty(), "foreign keys broken after the update");
    }

    #[tokio::test]
    async fn a_database_from_any_earlier_version_upgrades_cleanly() {
        let versions: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
        for version in versions {
            let dir = scratch();
            let pool = database_at(&dir, version).await;
            migrate_safely(&pool, &dir).await.unwrap_or_else(|e| panic!("from version {version}: {e:#}"));
            assert_eq!(applied_version(&pool).await, Some(latest_migration_version()));
            healthy(&pool).await;
            pool.close().await;
        }
    }

    #[tokio::test]
    async fn an_update_keeps_a_copy_of_what_was_there() {
        let dir = scratch();
        let pool = database_at(&dir, 10).await;
        sqlx::query("UPDATE sync_state SET local_port = 4242 WHERE id = 1").execute(&pool).await.unwrap();

        migrate_safely(&pool, &dir).await.unwrap();

        let copies: Vec<_> = std::fs::read_dir(dir.join(PRE_MIGRATION_DIR)).unwrap().flatten().collect();
        assert_eq!(copies.len(), 1);
        let old = init_pool(&copies[0].path()).await.unwrap();
        assert_eq!(applied_version(&old).await, Some(10), "the copy is the structure before the update");
        let (port,): (i64,) = sqlx::query_as("SELECT local_port FROM sync_state WHERE id = 1").fetch_one(&old).await.unwrap();
        assert_eq!(port, 4242, "and it holds the data");
        old.close().await;

        let (port,): (i64,) = sqlx::query_as("SELECT local_port FROM sync_state WHERE id = 1").fetch_one(&pool).await.unwrap();
        assert_eq!(port, 4242, "the data survived the update");
        pool.close().await;
    }

    #[tokio::test]
    async fn nothing_is_copied_when_there_is_nothing_to_update() {
        let dir = scratch();
        let pool = init_pool(&dir.join("local.db")).await.unwrap();
        migrate_safely(&pool, &dir).await.unwrap(); // a new database
        migrate_safely(&pool, &dir).await.unwrap(); // already up to date
        assert!(!dir.join(PRE_MIGRATION_DIR).exists());
        pool.close().await;
    }

    #[tokio::test]
    async fn a_database_from_a_newer_version_is_left_alone() {
        let dir = scratch();
        let pool = init_pool(&dir.join("local.db")).await.unwrap();
        migrate_safely(&pool, &dir).await.unwrap();
        let newer = latest_migration_version() + 5;
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)              VALUES ($1, 'from the future', 1, x'00', 0)",
        )
        .bind(newer)
        .execute(&pool)
        .await
        .unwrap();

        let err = migrate_safely(&pool, &dir).await.unwrap_err();
        assert!(matches!(
            err.downcast_ref::<crate::startup::StartupError>(),
            Some(crate::startup::StartupError::DatabaseTooNew { found, .. }) if *found == newer
        ));
        assert!(!dir.join(PRE_MIGRATION_DIR).exists());
        pool.close().await;
    }

    #[tokio::test]
    async fn only_the_latest_copies_are_kept() {
        let dir = scratch();
        let pool = database_at(&dir, 5).await;
        for from in 1..=5 {
            keep_copy_before_update(&pool, &dir, from, 25).await.unwrap();
        }
        let mut names: Vec<String> = std::fs::read_dir(dir.join(PRE_MIGRATION_DIR))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), PRE_MIGRATION_COPIES);
        assert!(names[0].contains("-v003-"), "the oldest were removed: {names:?}");
        pool.close().await;
    }
}
