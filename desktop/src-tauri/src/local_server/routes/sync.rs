use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use motodesk_core::sync::SyncStatus;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::local_server::error::LocalResult;
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/sync/status", get(status))
        .route("/api/sync/diagnostics", get(diagnostics))
        .route("/api/sync/bootstrap-from-cloud", post(bootstrap_from_cloud))
        .route("/api/sync/now", post(sync_now))
        .route("/api/sync/force-resync", post(force_resync))
        .route("/api/sync/restore-gap", get(restore_gap))
        .route("/api/sync/restore-gap/dismiss", post(dismiss_restore_gap))
        .route("/api/sync/recover", post(recover))
        .route("/api/sync/retry-refused", post(retry_refused))
}

/// Send the changes the cloud refused again, now, instead of when their back-off ends.
async fn retry_refused(State(state): State<SharedState>) -> LocalResult<Json<serde_json::Value>> {
    let n = crate::local_server::sync_log::retry_refused(&state.db).await?;
    state.notify_sync();
    Ok(Json(serde_json::json!({ "retrying": n })))
}

/// Fetch this PC's own changes back from the cloud after a restore, and rebuild the
/// register's working copies from its journal.
async fn recover(
    State(state): State<SharedState>,
    _admin: crate::local_server::auth_guard::AdminUser,
) -> LocalResult<Json<crate::local_server::recovery::RecoveryResult>> {
    Ok(Json(crate::local_server::recovery::recover_from_cloud(&state).await?))
}

/// How many of this PC's own changes the cloud holds and this database lacks, which
/// is what a restored backup looks like from the inside. Zero when there is nothing
/// to report or the warning has been read.
async fn restore_gap(State(state): State<SharedState>) -> LocalResult<Json<serde_json::Value>> {
    let (missing,): (i64,) = sqlx::query_as("SELECT COALESCE(seq_gap, 0) FROM sync_state WHERE id = 1")
        .fetch_one(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "missing": missing })))
}

async fn dismiss_restore_gap(State(state): State<SharedState>) -> LocalResult<Json<serde_json::Value>> {
    sqlx::query("UPDATE sync_state SET seq_gap = 0 WHERE id = 1")
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "missing": 0 })))
}

#[derive(Debug, Serialize)]
pub struct SyncNowResult {
    pub cloud_sync_ok: bool,
}

async fn sync_now(State(state): State<SharedState>) -> LocalResult<Json<SyncNowResult>> {
    let ok = crate::local_server::sync_engine::sync_once(&state)
        .await
        .is_ok();
    Ok(Json(SyncNowResult { cloud_sync_ok: ok }))
}

#[derive(Debug, Serialize)]
pub struct ForceResyncResult {
    pub bootstrap: crate::local_server::bootstrap::BootstrapResult,
    pub cloud_sync_ok: bool,
}

async fn force_resync(State(state): State<SharedState>) -> LocalResult<Json<ForceResyncResult>> {
    let bootstrap = crate::local_server::bootstrap::force_bootstrap_from_cloud(&state).await?;
    let cloud_sync_ok = crate::local_server::sync_engine::sync_once(&state)
        .await
        .is_ok();
    Ok(Json(ForceResyncResult {
        bootstrap,
        cloud_sync_ok,
    }))
}

async fn bootstrap_from_cloud(
    State(state): State<SharedState>,
) -> LocalResult<Json<crate::local_server::bootstrap::BootstrapResult>> {
    let result = crate::local_server::bootstrap::bootstrap_from_cloud(&state).await?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct StatusQuery {
    device_id: Option<Uuid>,
}

async fn status(
    State(state): State<SharedState>,
    Query(q): Query<StatusQuery>,
) -> LocalResult<Json<SyncStatus>> {
    let device_id = match q.device_id {
        Some(id) => id,
        None => state.device_id().await?,
    };

    let pending: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM change_log WHERE synced_to_cloud = 0")
            .fetch_one(&state.db)
            .await?;

    let cursor: Option<(Option<String>,)> =
        sqlx::query_as("SELECT last_cloud_cursor FROM sync_state WHERE id = 1")
            .fetch_optional(&state.db)
            .await?;

    let cloud_ok: (i64,) =
        sqlx::query_as("SELECT COALESCE(cloud_reachable, 0) FROM sync_state WHERE id = 1")
            .fetch_one(&state.db)
            .await?;

    Ok(Json(SyncStatus {
        device_id,
        pending_cloud_ops: pending.0,
        last_cloud_cursor: cursor.and_then(|(c,)| c),
        lan_peer_count: state
            .lan_peer_count
            .load(std::sync::atomic::Ordering::Relaxed),
        cloud_reachable: cloud_ok.0 != 0,
    }))
}

#[derive(Debug, Serialize)]
struct Diagnostics {
    device_id: Uuid,
    lan_peers: Vec<PeerDiag>,
    pending_cloud_ops: i64,
    last_cloud_push_at: Option<String>,
    last_cloud_push_error: Option<String>,
    cloud_backoff_until: Option<String>,
    last_lan_push_at: Option<String>,
    last_cloud_cursor: Option<String>,
    cloud_reachable: bool,
    cloud_url: String,
    /// A release build whose cloud address is this very computer: it can sync with nothing.
    cloud_url_misconfigured: bool,
    /// Changes the cloud has refused that are waiting to be tried again, and the latest
    /// reason. They do not hold up the others.
    refused_ops: i64,
    refused_latest: Option<String>,
    /// This installation has no cloud: it is run standalone, and can be put online.
    standalone: bool,
    /// Whether the company mesh secret has been provisioned (i.e. logged in &
    /// synced). Replaces the legacy pairing-code (`mesh_psk`) indicator.
    mesh_secret_set: bool,
    static_peers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PeerDiag {
    device_id: Uuid,
    host: String,
    port: u16,
}

async fn diagnostics(State(state): State<SharedState>) -> LocalResult<Json<Diagnostics>> {
    let device_id = state.device_id().await?;
    let row: Option<(
        Option<String>, // last_cloud_push_at
        Option<String>, // last_cloud_push_error
        Option<String>, // cloud_backoff_until
        Option<String>, // last_lan_push_at
        Option<String>, // last_cloud_cursor
        i64,            // cloud_reachable
        Option<String>, // cloud_url
        Option<String>, // mesh_secret
        Option<String>, // lan_peers_static
    )> = sqlx::query_as(
        r#"
        SELECT last_cloud_push_at,
               last_cloud_push_error,
               cloud_backoff_until,
               last_lan_push_at,
               last_cloud_cursor,
               COALESCE(cloud_reachable, 0),
               cloud_url,
               mesh_secret,
               lan_peers_static
        FROM sync_state WHERE id = 1
        "#,
    )
    .fetch_optional(&state.db)
    .await?;

    let (
        last_cloud_push_at,
        last_cloud_push_error,
        cloud_backoff_until,
        last_lan_push_at,
        last_cloud_cursor,
        cloud_reachable_int,
        cloud_url,
        mesh_secret,
        lan_peers_static,
    ) = row.unwrap_or((None, None, None, None, None, 0, None, None, None));

    let (pending,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM change_log WHERE synced_to_cloud = 0")
            .fetch_one(&state.db)
            .await?;

    let peers: Vec<PeerDiag> = state
        .peers
        .read()
        .await
        .iter()
        .map(|p| PeerDiag {
            device_id: p.device_id,
            host: p.host.clone(),
            port: p.port,
        })
        .collect();

    let static_peers: Vec<String> = lan_peers_static
        .as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default();

    let refused = crate::local_server::sync_log::refused_summary(&state.db).await?;
    let standalone = crate::local_server::local_accounts::is_standalone(&state.db).await;
    Ok(Json(Diagnostics {
        standalone,
        refused_ops: refused.count,
        refused_latest: refused.latest,
        device_id,
        lan_peers: peers,
        pending_cloud_ops: pending,
        last_cloud_push_at,
        last_cloud_push_error,
        cloud_backoff_until,
        last_lan_push_at,
        last_cloud_cursor,
        cloud_reachable: cloud_reachable_int != 0,
        cloud_url_misconfigured: crate::local_server::db::cloud_url_misconfigured(
            cloud_url.as_deref().unwrap_or_default(),
        ),
        cloud_url: cloud_url.unwrap_or_default(),
        mesh_secret_set: mesh_secret.map(|s| !s.is_empty()).unwrap_or(false),
        static_peers,
    }))
}
