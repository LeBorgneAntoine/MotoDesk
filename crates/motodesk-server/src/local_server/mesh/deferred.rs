//! Incoming changes that could not be applied yet, and trying them again.
//!
//! Changes do not arrive in dependency order: a PC catching up receives one device's history
//! and then another's, so a vehicle can come before its owner. A change that fails used to be
//! recorded and forgotten, which lost the record on that PC for good.

use std::collections::HashSet;

use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::ChangeLogEntry;
use sqlx::{Acquire, Sqlite, Transaction};
use uuid::Uuid;

use crate::local_server::error::LocalResult;
use crate::local_server::sync_log;

use super::{apply_op_to_sqlite, Applied};

/// Whether earlier changes to this row are still waiting: a newer one must not overtake them.
pub(super) async fn is_waiting(
    tx: &mut Transaction<'_, Sqlite>,
    entity_type: EntityType,
    entity_id: Uuid,
) -> LocalResult<bool> {
    if stands_alone(entity_type) {
        return Ok(false);
    }
    let found: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM sync_deferred WHERE entity_type = $1 AND entity_id = $2 LIMIT 1")
            .bind(entity_type.as_str())
            .bind(entity_id.to_string())
            .fetch_optional(&mut **tx)
            .await?;
    Ok(found.is_some())
}

/// A journal entry is written once and never changed, so nothing of it can overtake anything.
/// Held back by id, a spoiled copy sent first would keep the real entry out for good.
fn stands_alone(entity_type: EntityType) -> bool {
    entity_type == EntityType::FiscalEntry
}

/// Keep a change (already in the change log) for another try. `error` is why it failed.
pub(super) async fn keep(
    tx: &mut Transaction<'_, Sqlite>,
    op: &ChangeLogEntry,
    error: Option<&str>,
) -> LocalResult<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO sync_deferred (change_id, entity_type, entity_id, error, deferred_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(op.id.to_string())
    .bind(op.entity_type.as_str())
    .bind(op.entity_id.to_string())
    .bind(error)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Try everything that is waiting, again and again while something gets through: applying a
/// parent is what lets its children through. Returns how many were applied.
pub(super) async fn retry(tx: &mut Transaction<'_, Sqlite>) -> LocalResult<u32> {
    let mut applied = 0;
    loop {
        let waiting = sync_log::deferred_ops(tx).await?;
        let mut progressed = false;
        // Rows whose oldest waiting change did not get through in this pass.
        let mut blocked: HashSet<(&'static str, Uuid)> = HashSet::new();
        for op in waiting {
            let row = (op.entity_type.as_str(), op.entity_id);
            if !stands_alone(op.entity_type) && blocked.contains(&row) {
                continue;
            }
            let mut savepoint = Acquire::begin(&mut *tx).await?;
            match apply_op_to_sqlite(&mut savepoint, &op).await {
                Ok(Applied::Done) => {
                    savepoint.commit().await?;
                    sqlx::query("DELETE FROM sync_deferred WHERE change_id = $1")
                        .bind(op.id.to_string())
                        .execute(&mut **tx)
                        .await?;
                    progressed = true;
                    applied += 1;
                }
                Ok(Applied::WaitsForRow) => {
                    savepoint.rollback().await?;
                    blocked.insert(row);
                }
                Err(err) => {
                    savepoint.rollback().await?;
                    blocked.insert(row);
                    sqlx::query("UPDATE sync_deferred SET error = $2 WHERE change_id = $1")
                        .bind(op.id.to_string())
                        .bind(err.to_string())
                        .execute(&mut **tx)
                        .await?;
                }
            }
        }
        if !progressed {
            return Ok(applied);
        }
    }
}
