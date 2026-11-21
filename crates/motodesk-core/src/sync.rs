use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::EntityType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Insert,
    Update,
    Delete,
}

impl OpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OpKind::Insert => "insert",
            OpKind::Update => "update",
            OpKind::Delete => "delete",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "insert" => Some(OpKind::Insert),
            "update" => Some(OpKind::Update),
            "delete" => Some(OpKind::Delete),
            _ => None,
        }
    }
}

/// Per-field version stamp for conflict resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldStamp {
    pub updated_at: DateTime<Utc>,
    pub device_id: Uuid,
}

impl FieldStamp {
    pub fn new(updated_at: DateTime<Utc>, device_id: Uuid) -> Self {
        Self {
            updated_at,
            device_id,
        }
    }

    /// Returns true if `self` should win over `other`.
    pub fn wins_over(&self, other: &FieldStamp) -> bool {
        if self.updated_at > other.updated_at {
            return true;
        }
        if self.updated_at < other.updated_at {
            return false;
        }
        self.device_id.to_string() >= other.device_id.to_string()
    }
}

pub type FieldMetaMap = HashMap<String, FieldStamp>;

/// Local or replicated row with sync metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub entity_type: EntityType,
    pub entity_id: Uuid,
    pub row_version: i64,
    pub deleted_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub payload: serde_json::Value,
    pub field_meta: FieldMetaMap,
}

/// Append-only change log entry (local mesh + cloud).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeLogEntry {
    pub id: Uuid,
    pub device_id: Uuid,
    pub seq: i64,
    pub entity_type: EntityType,
    pub entity_id: Uuid,
    pub op: OpKind,
    pub payload: serde_json::Value,
    pub field_meta: FieldMetaMap,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub synced_to_cloud: bool,
    #[serde(default)]
    pub relayed: bool,
}

/// Push/pull API payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPushRequest {
    pub device_id: Uuid,
    pub ops: Vec<ChangeLogEntry>,
    #[serde(default)]
    pub relay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPullResponse {
    pub ops: Vec<ChangeLogEntry>,
    pub cursor: String,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub device_id: Uuid,
    pub pending_cloud_ops: i64,
    pub last_cloud_cursor: Option<String>,
    pub lan_peer_count: u32,
    pub cloud_reachable: bool,
}

/// Recorded when field-level LWW tie-break fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflictRecord {
    pub entity_type: EntityType,
    pub entity_id: Uuid,
    pub field: String,
    pub winner_device_id: Uuid,
    pub loser_device_id: Uuid,
    pub resolved_at: DateTime<Utc>,
}

/// Mesh apply batch. Authenticated with HMAC over nonce+timestamp+ops,
/// not by sending the shared secret on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshApplyRequest {
    pub ops: Vec<ChangeLogEntry>,
    /// Legacy field; ignored. Kept so old peers fail closed on missing sig.
    #[serde(default)]
    pub mesh_psk: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub ts: Option<i64>,
    #[serde(default)]
    pub sig: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshApplyResponse {
    pub applied: u32,
    pub skipped: u32,
}
