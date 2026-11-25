//! Getting this PC's own changes back from the cloud after a restore.
//!
//! A restored backup is a copy of the database as it was; the cloud still holds
//! everything this PC sent afterwards (sales, payments, stock movements, fiscal
//! journal entries). Pull never returns a device's own changes, so they would stay
//! there. This asks for them explicitly, applies them the way any change arriving
//! from another device is applied, and rebuilds what the fiscal register keeps as a
//! working copy of its journal (see [`crate::local_server::fiscal::recovery`]).

use std::time::Duration;

use motodesk_core::sync::{ChangeLogEntry, SyncPullResponse};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::fiscal::recovery::{rebuild_from_journal, RebuildReport};
use crate::local_server::state::SharedState;
use crate::local_server::{mesh, sync_engine, sync_log};

const PAGE: i64 = 200;

#[derive(Debug, Default, Serialize)]
pub struct RecoveryResult {
    /// False when there was nothing to recover.
    pub recovered: bool,
    /// Changes the cloud sent.
    pub fetched: u32,
    /// Of those, applied here.
    pub applied: u32,
    /// Of those, already here, or set aside because they could not be applied (the
    /// latter are recorded in the audit log).
    pub skipped: u32,
    pub registers_moved: u32,
    pub closures_restored: u32,
    pub numbering_moved: u32,
}

/// One recovery at a time.
fn lock() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub async fn recover_from_cloud(state: &SharedState) -> LocalResult<RecoveryResult> {
    let _guard = lock().lock().await;

    let token = crate::local_server::secrets::session_access_token(&state.db)
        .await?
        .ok_or_else(|| {
            LocalError::BadRequest("connectez-vous au cloud pour récupérer les modifications manquantes".into())
        })?;
    let device_id = state.device_id().await?;
    let base = crate::local_server::cloud::cloud_base_url(state).await;

    // Makes sure the numbering is lined up and the gap is known (idempotent).
    sync_engine::align_with_cloud(state, &base, &token, device_id)
        .await
        .map_err(|e| LocalError::BadRequest(format!("le cloud n'a pas répondu : {e}")))?;

    let (gap, mut after): (i64, i64) =
        sqlx::query_as("SELECT COALESCE(seq_gap, 0), COALESCE(seq_recover_from, 0) FROM sync_state WHERE id = 1")
            .fetch_one(&state.db)
            .await?;
    if gap <= 0 {
        return Ok(RecoveryResult::default());
    }

    let mut out = RecoveryResult {
        recovered: true,
        ..Default::default()
    };
    loop {
        let res = state
            .http
            .get(format!(
                "{base}/api/sync/own-ops?device_id={device_id}&after_seq={after}&limit={PAGE}"
            ))
            .header("Authorization", format!("Bearer {token}"))
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| LocalError::BadRequest(format!("le cloud n'a pas répondu : {e}")))?;
        if !res.status().is_success() {
            return Err(LocalError::BadRequest(format!(
                "le cloud a refusé la récupération (HTTP {})",
                res.status()
            )));
        }
        let page: SyncPullResponse = res
            .json()
            .await
            .map_err(|e| LocalError::BadRequest(format!("réponse du cloud illisible : {e}")))?;
        let Some(last) = page.ops.last().map(|o| o.seq) else {
            break;
        };
        let (fetched, applied) = apply_page(state, page.ops).await?;
        out.fetched += fetched;
        out.applied += applied;
        after = last;
        if !page.has_more {
            break;
        }
    }
    out.skipped = out.fetched - out.applied;

    let rebuilt = finish(state).await?;
    out.registers_moved = rebuilt.registers_moved;
    out.closures_restored = rebuilt.closures_restored;
    out.numbering_moved = rebuilt.numbering_moved;
    Ok(out)
}

/// Apply one page of recovered changes. They come from the cloud, so none of them
/// is waiting to be sent back. Returns (fetched, applied).
pub(crate) async fn apply_page(
    state: &SharedState,
    ops: Vec<ChangeLogEntry>,
) -> LocalResult<(u32, u32)> {
    let ids: Vec<uuid::Uuid> = ops.iter().map(|o| o.id).collect();
    let fetched = ids.len() as u32;
    let applied = mesh::apply_batch(state, ops).await?.applied;
    sync_log::mark_synced(&state.db, &ids).await?;
    Ok((fetched, applied))
}

/// Everything recovered: bring the register's working copies up to its journal and
/// clear the warning.
pub(crate) async fn finish(state: &SharedState) -> LocalResult<RebuildReport> {
    let report = rebuild_from_journal(&state.db).await?;
    sqlx::query("UPDATE sync_state SET seq_gap = 0, seq_recover_from = 0 WHERE id = 1")
        .execute(&state.db)
        .await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::shop;
    use motodesk_core::entity::EntityType;
    use motodesk_core::sync::OpKind;
    use serde_json::json;
    use uuid::Uuid;

    #[tokio::test]
    async fn recovered_changes_are_applied_once_kept_as_delivered_and_lift_the_block() {
        let state = shop().await;
        let device = state.device_id().await.unwrap();
        sqlx::query("UPDATE sync_state SET seq_gap = 3, seq_recover_from = 10 WHERE id = 1")
            .execute(&state.db)
            .await
            .unwrap();

        let client = Uuid::new_v4();
        let op = |seq: i64, entity: Uuid, kind: OpKind, payload: serde_json::Value| ChangeLogEntry {
            id: Uuid::new_v4(),
            device_id: device,
            seq,
            entity_type: EntityType::Client,
            entity_id: entity,
            op: kind,
            payload,
            field_meta: Default::default(),
            created_at: chrono::Utc::now() + chrono::Duration::milliseconds(seq),
            synced_to_cloud: true,
            relayed: false,
        };
        let ops = vec![
            op(11, client, OpKind::Insert, json!({ "id": client.to_string(), "last_name": "Dupont", "first_name": "Jean" })),
            op(12, client, OpKind::Update, json!({ "id": client.to_string(), "phone": "0611223344" })),
        ];

        let (fetched, applied) = apply_page(&state, ops.clone()).await.unwrap();
        assert_eq!((fetched, applied), (2, 2));
        let (phone,): (Option<String>,) = sqlx::query_as("SELECT phone FROM clients WHERE id = $1")
            .bind(client.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(phone.as_deref(), Some("0611223344"));

        // They came from the cloud: none is queued to go back.
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM change_log WHERE seq IN (11, 12) AND device_id = $1 AND synced_to_cloud = 0",
        )
        .bind(device.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(waiting, 0);

        // The same page again (a retry after a dropped connection) changes nothing.
        let (_, applied_again) = apply_page(&state, ops).await.unwrap();
        assert_eq!(applied_again, 0);

        finish(&state).await.unwrap();
        let (gap, from): (i64, i64) = sqlx::query_as("SELECT seq_gap, seq_recover_from FROM sync_state WHERE id = 1")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!((gap, from), (0, 0), "the warning and the write block are lifted");
    }
}
