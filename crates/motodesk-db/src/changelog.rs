use chrono::{DateTime, Utc};
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, FieldMetaMap, OpKind};
use serde_json::Value;
use uuid::Uuid;

/// Encode cursor as `device_id:seq` for pull pagination.
pub fn encode_cursor(device_id: Uuid, seq: i64) -> String {
    format!("{device_id}:{seq}")
}

/// Parse cursor; returns None if invalid.
pub fn parse_cursor(cursor: &str) -> Option<(DateTime<Utc>, Uuid, i64)> {
    let (device, seq) = cursor.rsplit_once(':')?;
    let device_id = Uuid::parse_str(device).ok()?;
    let seq = seq.parse().ok()?;
    Some((DateTime::<Utc>::MIN_UTC, device_id, seq))
}

/// Build a change-log entry for persistence layers.
pub fn build_entry(
    id: Uuid,
    device_id: Uuid,
    seq: i64,
    entity_type: EntityType,
    entity_id: Uuid,
    op: OpKind,
    payload: Value,
    field_meta: FieldMetaMap,
) -> ChangeLogEntry {
    ChangeLogEntry {
        id,
        device_id,
        seq,
        entity_type,
        entity_id,
        op,
        payload,
        field_meta,
        created_at: Utc::now(),
        synced_to_cloud: false,
        relayed: false,
    }
}

/// Row shape when reading from SQL (both SQLite and Postgres).
#[derive(Debug, Clone)]
pub struct ChangeLogRow {
    pub id: Uuid,
    pub device_id: Uuid,
    pub seq: i64,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub op: String,
    pub payload: Value,
    pub field_meta: Value,
    pub created_at: DateTime<Utc>,
    pub synced_to_cloud: bool,
    pub relayed: bool,
}

impl ChangeLogRow {
    pub fn into_entry(self) -> Option<ChangeLogEntry> {
        let entity_type = EntityType::parse(&self.entity_type)?;
        let op = OpKind::parse(&self.op)?;
        let field_meta: FieldMetaMap = serde_json::from_value(self.field_meta).unwrap_or_default();
        Some(ChangeLogEntry {
            id: self.id,
            device_id: self.device_id,
            seq: self.seq,
            entity_type,
            entity_id: self.entity_id,
            op,
            payload: self.payload,
            field_meta,
            created_at: self.created_at,
            synced_to_cloud: self.synced_to_cloud,
            relayed: self.relayed,
        })
    }
}
