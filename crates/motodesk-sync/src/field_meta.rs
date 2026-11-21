use chrono::{DateTime, Utc};
use motodesk_core::sync::{FieldMetaMap, FieldStamp};
use serde_json::Value;
use uuid::Uuid;

use motodesk_core::entity::SYNC_META_KEYS;

/// Build field stamps for all scalar keys present in `payload` at `now`.
pub fn stamps_for_payload(payload: &Value, device_id: Uuid, now: DateTime<Utc>) -> FieldMetaMap {
    let mut meta = FieldMetaMap::new();
    let Some(obj) = payload.as_object() else {
        return meta;
    };
    for (key, _) in obj {
        if SYNC_META_KEYS.contains(&key.as_str()) {
            continue;
        }
        meta.insert(key.clone(), FieldStamp::new(now, device_id));
    }
    meta
}

/// Merge incoming field_meta into local, keeping the newer stamp per field.
pub fn merge_field_meta(local: &FieldMetaMap, incoming: &FieldMetaMap) -> FieldMetaMap {
    let mut out = local.clone();
    for (field, stamp) in incoming {
        // `_`-prefixed keys are markers ("this operation changed nothing"), not fields.
        if field.starts_with('_') {
            continue;
        }
        match out.get(field) {
            Some(existing) if stamp.wins_over(existing) => {
                out.insert(field.clone(), stamp.clone());
            }
            None => {
                out.insert(field.clone(), stamp.clone());
            }
            _ => {}
        }
    }
    out
}

/// Update field_meta after a PATCH: only keys present in `patch` get new stamps.
pub fn apply_patch_stamps(
    local_meta: &FieldMetaMap,
    patch: &Value,
    device_id: Uuid,
    now: DateTime<Utc>,
) -> FieldMetaMap {
    let mut meta = local_meta.clone();
    let Some(obj) = patch.as_object() else {
        return meta;
    };
    for key in obj.keys() {
        if SYNC_META_KEYS.contains(&key.as_str()) {
            continue;
        }
        meta.insert(key.clone(), FieldStamp::new(now, device_id));
    }
    meta
}

/// Compare two optional entity-level timestamps (fallback when field_meta missing).
pub fn entity_stamp_wins(
    incoming_at: DateTime<Utc>,
    incoming_device: Uuid,
    local_at: DateTime<Utc>,
    local_device: Uuid,
) -> bool {
    FieldStamp::new(incoming_at, incoming_device)
        .wins_over(&FieldStamp::new(local_at, local_device))
}
