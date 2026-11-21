use chrono::{DateTime, Utc};
use motodesk_core::entity::{is_sealed, SYNC_META_KEYS};
use motodesk_core::sync::SyncConflictRecord;
use motodesk_core::sync::{ChangeLogEntry, EntitySnapshot, FieldMetaMap, OpKind};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::SyncError;
use crate::field_meta::{entity_stamp_wins, merge_field_meta, stamps_for_payload};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    SkippedDuplicate,
    SkippedStale,
}

#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub outcome: ApplyOutcome,
    pub snapshot: Option<EntitySnapshot>,
    pub conflicts: Vec<SyncConflictRecord>,
}

/// Apply a single change-log operation to a local row snapshot (if any).
pub fn apply_operation(
    local: Option<&EntitySnapshot>,
    op: &ChangeLogEntry,
) -> Result<ApplyResult, SyncError> {
    // Two classes of row are outside last-write-wins entirely. The fiscal
    // journal is append-only by construction, and a document already sealed
    // into it cannot legally be edited or deleted afterwards
    // (BOI-TVA-DECLA-30-10-30 § 90). A peer telling us otherwise is ignored
    // rather than obeyed: the correction it should have sent is a credit note,
    // which arrives as its own new row.
    if let Some(refusal) = refuse_if_immutable(local, op) {
        return Ok(refusal);
    }

    match op.op {
        OpKind::Insert => apply_insert(local, op),
        OpKind::Update => apply_update(local, op),
        OpKind::Delete => apply_delete(local, op),
    }
}

/// Returns the outcome to use instead of merging, when the target must not move.
fn refuse_if_immutable(local: Option<&EntitySnapshot>, op: &ChangeLogEntry) -> Option<ApplyResult> {
    let keep = |row: &EntitySnapshot, outcome: ApplyOutcome| {
        Some(ApplyResult {
            outcome,
            snapshot: Some(row.clone()),
            conflicts: vec![],
        })
    };

    if op.entity_type.is_append_only() {
        return match (local, op.op) {
            // First time we see this journal row: store it verbatim.
            (None, OpKind::Insert) => None,
            // A row we already hold, or any attempt to change one.
            (Some(row), _) => keep(row, ApplyOutcome::SkippedDuplicate),
            // An update or delete for a row we never received. There is nothing
            // to write that would be trustworthy, so drop it.
            (None, _) => Some(ApplyResult {
                outcome: ApplyOutcome::SkippedStale,
                snapshot: None,
                conflicts: vec![],
            }),
        };
    }

    match local {
        Some(row) if is_sealed(&row.payload) => keep(row, ApplyOutcome::SkippedStale),
        _ => None,
    }
}

/// Apply a batch in order inside one logical transaction (caller runs DB txn).
pub fn apply_batch(
    mut local_get: impl FnMut(&motodesk_core::entity::EntityType, Uuid) -> Option<EntitySnapshot>,
    ops: &[ChangeLogEntry],
) -> Result<Vec<(Uuid, ApplyResult)>, SyncError> {
    let mut results = Vec::with_capacity(ops.len());
    for op in ops {
        let local = local_get(&op.entity_type, op.entity_id);
        let result = apply_operation(local.as_ref(), op)?;
        results.push((op.id, result));
    }
    Ok(results)
}

fn apply_insert(
    local: Option<&EntitySnapshot>,
    op: &ChangeLogEntry,
) -> Result<ApplyResult, SyncError> {
    ensure_object(&op.payload)?;

    if let Some(row) = local {
        if !row.deleted_at.is_some() {
            if row.row_version >= op.seq {
                return Ok(ApplyResult {
                    outcome: ApplyOutcome::SkippedDuplicate,
                    snapshot: Some(row.clone()),
                    conflicts: vec![],
                });
            }
        }
    }

    let payload = op.payload.clone();
    let field_meta = if op.field_meta.is_empty() {
        stamps_for_payload(&payload, op.device_id, op.created_at)
    } else {
        op.field_meta.clone()
    };

    let snapshot = EntitySnapshot {
        entity_type: op.entity_type,
        entity_id: op.entity_id,
        row_version: op.seq.max(1),
        deleted_at: None,
        updated_at: op.created_at,
        payload,
        field_meta,
    };

    Ok(ApplyResult {
        outcome: ApplyOutcome::Applied,
        snapshot: Some(snapshot),
        conflicts: vec![],
    })
}

fn apply_update(
    local: Option<&EntitySnapshot>,
    op: &ChangeLogEntry,
) -> Result<ApplyResult, SyncError> {
    ensure_object(&op.payload)?;

    let Some(mut row) = local.cloned() else {
        return apply_insert(None, op);
    };

    if row.deleted_at.is_some() {
        // Tombstones are sticky: a later update must not resurrect a deleted
        // business record (clock skew would otherwise undelete invoices/stock).
        return Ok(ApplyResult {
            outcome: ApplyOutcome::SkippedStale,
            snapshot: Some(row),
            conflicts: vec![],
        });
    }

    let (merged_payload, conflicts) = merge_payload_fields(
        &row,
        &op.payload,
        &op.field_meta,
        op.device_id,
        op.created_at,
    )?;

    row.payload = merged_payload;
    row.field_meta = merge_field_meta(&row.field_meta, &op.field_meta);
    if row.field_meta.is_empty() && !op.field_meta.is_empty() {
        row.field_meta = op.field_meta.clone();
    }
    row.updated_at = op.created_at;
    row.row_version = row.row_version.max(op.seq);
    row.deleted_at = None;

    Ok(ApplyResult {
        outcome: ApplyOutcome::Applied,
        snapshot: Some(row),
        conflicts,
    })
}

fn apply_delete(
    local: Option<&EntitySnapshot>,
    op: &ChangeLogEntry,
) -> Result<ApplyResult, SyncError> {
    let Some(mut row) = local.cloned() else {
        let snapshot = EntitySnapshot {
            entity_type: op.entity_type,
            entity_id: op.entity_id,
            row_version: op.seq.max(1),
            deleted_at: Some(op.created_at),
            updated_at: op.created_at,
            payload: op.payload.clone(),
            field_meta: op.field_meta.clone(),
        };
        return Ok(ApplyResult {
            outcome: ApplyOutcome::Applied,
            snapshot: Some(snapshot),
            conflicts: vec![],
        });
    };

    let last_edit = row.updated_at;
    if op.created_at < last_edit && row.deleted_at.is_none() {
        return Ok(ApplyResult {
            outcome: ApplyOutcome::SkippedStale,
            snapshot: Some(row),
            conflicts: vec![],
        });
    }

    row.deleted_at = Some(op.created_at);
    row.updated_at = op.created_at;
    row.row_version = row.row_version.max(op.seq);

    Ok(ApplyResult {
        outcome: ApplyOutcome::Applied,
        snapshot: Some(row),
        conflicts: vec![],
    })
}

fn merge_payload_fields(
    local: &EntitySnapshot,
    incoming_patch: &Value,
    incoming_meta: &FieldMetaMap,
    incoming_device: Uuid,
    incoming_at: DateTime<Utc>,
) -> Result<(Value, Vec<SyncConflictRecord>), SyncError> {
    let mut out = local
        .payload
        .as_object()
        .cloned()
        .ok_or(SyncError::PayloadNotObject)?;
    let patch = incoming_patch
        .as_object()
        .ok_or(SyncError::PayloadNotObject)?;

    let mut conflicts = Vec::new();

    // An operation that carries stamps says which of its fields it changed: the rest
    // of the row it sends is context, not a claim. Without this a device that renames
    // a product also "writes" the price it last saw, stamped now, and undoes a price
    // changed elsewhere a moment before. An operation with no stamps at all (an older
    // device) keeps the old behaviour: every field is judged by the time of the op.
    let stamps_decide = !incoming_meta.is_empty();

    for (key, new_val) in patch {
        if SYNC_META_KEYS.contains(&key.as_str()) {
            continue;
        }
        if stamps_decide && !incoming_meta.contains_key(key) {
            continue;
        }

        let apply_incoming = match (incoming_meta.get(key), local.field_meta.get(key)) {
            (Some(inc), Some(loc)) => {
                if inc.wins_over(loc) {
                    if loc.device_id != inc.device_id {
                        conflicts.push(SyncConflictRecord {
                            entity_type: local.entity_type,
                            entity_id: local.entity_id,
                            field: key.clone(),
                            winner_device_id: inc.device_id,
                            loser_device_id: loc.device_id,
                            resolved_at: incoming_at,
                        });
                    }
                    true
                } else {
                    false
                }
            }
            (Some(_), None) => true,
            (None, Some(loc)) => {
                entity_stamp_wins(incoming_at, incoming_device, loc.updated_at, loc.device_id)
            }
            (None, None) => {
                entity_stamp_wins(incoming_at, incoming_device, local.updated_at, Uuid::nil())
            }
        };

        if apply_incoming {
            out.insert(key.clone(), new_val.clone());
        }
    }

    Ok((Value::Object(out), conflicts))
}

fn ensure_object(v: &Value) -> Result<(), SyncError> {
    if v.is_object() || v.is_null() {
        Ok(())
    } else {
        Err(SyncError::PayloadNotObject)
    }
}

/// Build a JSON payload object from a snapshot for persistence.
pub fn snapshot_to_row_json(snapshot: &EntitySnapshot) -> Value {
    let mut obj = snapshot.payload.as_object().cloned().unwrap_or_default();
    obj.insert("id".into(), json!(snapshot.entity_id.to_string()));
    obj.insert("row_version".into(), json!(snapshot.row_version));
    if let Some(d) = snapshot.deleted_at {
        obj.insert("deleted_at".into(), json!(d.to_rfc3339()));
    }
    obj.insert(
        "field_meta".into(),
        serde_json::to_value(&snapshot.field_meta).unwrap_or(json!({})),
    );
    obj.insert("updated_at".into(), json!(snapshot.updated_at.to_rfc3339()));
    Value::Object(obj)
}

#[cfg(test)]
mod tests;
