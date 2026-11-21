use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, EntitySnapshot, FieldStamp, OpKind};
use serde_json::json;
use uuid::Uuid;

use super::{apply_batch, apply_operation, ApplyOutcome};
use crate::field_meta::stamps_for_payload;

fn device(n: u8) -> Uuid {
    Uuid::from_bytes([n; 16])
}

fn client_op(
    op: OpKind,
    device_id: Uuid,
    seq: i64,
    payload: serde_json::Value,
    field_meta: motodesk_core::sync::FieldMetaMap,
) -> ChangeLogEntry {
    let id = Uuid::new_v4();
    ChangeLogEntry {
        id,
        device_id,
        seq,
        entity_type: EntityType::Client,
        entity_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        op,
        payload,
        field_meta,
        created_at: Utc::now(),
        synced_to_cloud: false,
        relayed: false,
    }
}

#[test]
fn insert_idempotent_when_row_exists() {
    let eid = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let local = EntitySnapshot {
        entity_type: EntityType::Client,
        entity_id: eid,
        row_version: 5,
        deleted_at: None,
        updated_at: Utc::now(),
        payload: json!({"last_name": "Dupont"}),
        field_meta: Default::default(),
    };
    let op = client_op(
        OpKind::Insert,
        device(1),
        3,
        json!({"last_name": "Martin"}),
        Default::default(),
    );
    let r = apply_operation(Some(&local), &op).unwrap();
    assert_eq!(r.outcome, ApplyOutcome::SkippedDuplicate);
}

#[test]
fn field_merge_non_overlapping() {
    let eid = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let t0 = Utc::now();
    let mut local_meta = motodesk_core::sync::FieldMetaMap::new();
    local_meta.insert("last_name".into(), FieldStamp::new(t0, device(1)));
    let local = EntitySnapshot {
        entity_type: EntityType::Client,
        entity_id: eid,
        row_version: 1,
        deleted_at: None,
        updated_at: t0,
        payload: json!({"last_name": "Dupont", "phone": null}),
        field_meta: local_meta,
    };

    let t1 = t0 + chrono::Duration::seconds(10);
    let mut inc_meta = motodesk_core::sync::FieldMetaMap::new();
    inc_meta.insert("notes".into(), FieldStamp::new(t1, device(2)));

    let op = client_op(
        OpKind::Update,
        device(2),
        2,
        json!({"notes": "VIP"}),
        inc_meta,
    );
    let r = apply_operation(Some(&local), &op).unwrap();
    let snap = r.snapshot.unwrap();
    assert_eq!(snap.payload["last_name"], "Dupont");
    assert_eq!(snap.payload["notes"], "VIP");
}

#[test]
fn field_clash_lww() {
    let eid = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let t0 = Utc::now();
    let t1 = t0 + chrono::Duration::seconds(5);
    let mut local_meta = motodesk_core::sync::FieldMetaMap::new();
    local_meta.insert("phone".into(), FieldStamp::new(t0, device(1)));
    let local = EntitySnapshot {
        entity_type: EntityType::Client,
        entity_id: eid,
        row_version: 1,
        deleted_at: None,
        updated_at: t0,
        payload: json!({"phone": "111"}),
        field_meta: local_meta,
    };

    let mut inc_meta = motodesk_core::sync::FieldMetaMap::new();
    inc_meta.insert("phone".into(), FieldStamp::new(t1, device(2)));
    let op = client_op(
        OpKind::Update,
        device(2),
        2,
        json!({"phone": "222"}),
        inc_meta,
    );
    let r = apply_operation(Some(&local), &op).unwrap();
    assert_eq!(r.snapshot.unwrap().payload["phone"], "222");
    assert_eq!(r.conflicts.len(), 1);
}

#[test]
fn delete_after_update_tombstone() {
    let eid = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let t0 = Utc::now();
    let local = EntitySnapshot {
        entity_type: EntityType::Client,
        entity_id: eid,
        row_version: 2,
        deleted_at: None,
        updated_at: t0,
        payload: json!({"last_name": "X"}),
        field_meta: Default::default(),
    };
    let t1 = t0 + chrono::Duration::seconds(1);
    let mut op = client_op(OpKind::Delete, device(1), 3, json!({}), Default::default());
    op.created_at = t1;
    let r = apply_operation(Some(&local), &op).unwrap();
    assert!(r.snapshot.unwrap().deleted_at.is_some());
}

fn snapshot(entity_type: EntityType, payload: serde_json::Value) -> EntitySnapshot {
    EntitySnapshot {
        entity_type,
        entity_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        row_version: 4,
        deleted_at: None,
        updated_at: Utc::now(),
        payload,
        field_meta: Default::default(),
    }
}

fn op_on(entity_type: EntityType, op: OpKind, payload: serde_json::Value) -> ChangeLogEntry {
    ChangeLogEntry {
        id: Uuid::new_v4(),
        device_id: device(2),
        seq: 99,
        entity_type,
        entity_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        op,
        payload,
        field_meta: Default::default(),
        created_at: Utc::now() + chrono::Duration::hours(1),
        synced_to_cloud: false,
        relayed: false,
    }
}

#[test]
fn a_sealed_invoice_cannot_be_edited_by_a_peer() {
    let local = snapshot(
        EntityType::Sale,
        json!({
            "reference": "FA-2026-C1-00042",
            "total_ttc_cents": 12000,
            "locked_at": "2026-08-21T09:00:00Z"
        }),
    );
    let op = op_on(
        EntityType::Sale,
        OpKind::Update,
        json!({ "total_ttc_cents": 100 }),
    );

    let r = apply_operation(Some(&local), &op).unwrap();
    assert_eq!(r.outcome, ApplyOutcome::SkippedStale);
    assert_eq!(r.snapshot.unwrap().payload["total_ttc_cents"], 12000);
}

#[test]
fn a_sealed_invoice_cannot_be_deleted_by_a_peer() {
    let local = snapshot(
        EntityType::Sale,
        json!({ "locked_at": "2026-08-21T09:00:00Z" }),
    );
    let op = op_on(EntityType::Sale, OpKind::Delete, json!({}));

    let r = apply_operation(Some(&local), &op).unwrap();
    assert_eq!(r.outcome, ApplyOutcome::SkippedStale);
    assert!(r.snapshot.unwrap().deleted_at.is_none());
}

#[test]
fn an_unsealed_draft_still_merges_normally() {
    let local = snapshot(
        EntityType::Sale,
        json!({ "status": "draft", "notes": "old" }),
    );
    let op = op_on(EntityType::Sale, OpKind::Update, json!({ "notes": "new" }));

    let r = apply_operation(Some(&local), &op).unwrap();
    assert_eq!(r.outcome, ApplyOutcome::Applied);
    assert_eq!(r.snapshot.unwrap().payload["notes"], "new");
}

#[test]
fn a_journal_entry_is_stored_once_and_never_changed() {
    let insert = op_on(
        EntityType::FiscalEntry,
        OpKind::Insert,
        json!({ "seq": 7, "chain_hash": "abc" }),
    );
    let first = apply_operation(None, &insert).unwrap();
    assert_eq!(first.outcome, ApplyOutcome::Applied);

    let stored = first.snapshot.unwrap();
    let replay = apply_operation(Some(&stored), &insert).unwrap();
    assert_eq!(replay.outcome, ApplyOutcome::SkippedDuplicate);
    assert_eq!(replay.snapshot.unwrap().payload["chain_hash"], "abc");
}

#[test]
fn a_journal_entry_resists_update_and_delete() {
    let stored = snapshot(
        EntityType::FiscalEntry,
        json!({ "seq": 7, "chain_hash": "abc" }),
    );

    let tampered = apply_operation(
        Some(&stored),
        &op_on(
            EntityType::FiscalEntry,
            OpKind::Update,
            json!({ "chain_hash": "forged" }),
        ),
    )
    .unwrap();
    assert_eq!(tampered.outcome, ApplyOutcome::SkippedDuplicate);
    assert_eq!(tampered.snapshot.unwrap().payload["chain_hash"], "abc");

    let removed = apply_operation(
        Some(&stored),
        &op_on(EntityType::FiscalEntry, OpKind::Delete, json!({})),
    )
    .unwrap();
    assert_eq!(removed.outcome, ApplyOutcome::SkippedDuplicate);
    assert!(removed.snapshot.unwrap().deleted_at.is_none());
}

#[test]
fn a_delete_for_an_unknown_journal_entry_writes_no_tombstone() {
    let r = apply_operation(
        None,
        &op_on(EntityType::FiscalEntry, OpKind::Delete, json!({})),
    )
    .unwrap();
    assert_eq!(r.outcome, ApplyOutcome::SkippedStale);
    assert!(r.snapshot.is_none());
}

#[test]
fn batch_apply_all_ops() {
    let eid = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let mut store: Option<EntitySnapshot> = None;
    let insert = client_op(
        OpKind::Insert,
        device(1),
        1,
        json!({"last_name": "A"}),
        stamps_for_payload(&json!({"last_name": "A"}), device(1), Utc::now()),
    );
    let results = apply_batch(
        |_, id| {
            if id == eid {
                store.clone()
            } else {
                None
            }
        },
        &[insert],
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    store = results[0].1.snapshot.clone();
    assert_eq!(store.as_ref().unwrap().payload["last_name"], "A");
}

/// Two devices edit different fields of the same product at about the same time. The
/// later *operation* sends the whole row, but only claims the field it changed.
mod concurrent_edits {
    use super::*;
    use chrono::{Duration, TimeZone};
    use motodesk_core::sync::FieldMetaMap;

    fn at(secs: i64) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 15, 10, 0, 0).unwrap() + Duration::seconds(secs)
    }

    fn stamp(secs: i64, dev: u8) -> FieldStamp {
        FieldStamp::new(at(secs), device(dev))
    }

    /// The local row: name written by device 1 at t=0, price changed by device 2 at t=5.
    fn local() -> EntitySnapshot {
        let mut meta = FieldMetaMap::new();
        meta.insert("name".into(), stamp(0, 1));
        meta.insert("price".into(), stamp(5, 2));
        EntitySnapshot {
            entity_type: EntityType::Product,
            entity_id: Uuid::new_v4(),
            row_version: 2,
            deleted_at: None,
            updated_at: at(5),
            payload: json!({ "name": "Filtre", "price": 200 }),
            field_meta: meta,
        }
    }

    fn update(created: i64, payload: serde_json::Value, meta: FieldMetaMap, local: &EntitySnapshot) -> ChangeLogEntry {
        ChangeLogEntry {
            id: Uuid::new_v4(),
            device_id: device(1),
            seq: 9,
            entity_type: EntityType::Product,
            entity_id: local.entity_id,
            op: OpKind::Update,
            payload,
            field_meta: meta,
            created_at: at(created),
            synced_to_cloud: false,
            relayed: false,
        }
    }

    #[test]
    fn a_rename_does_not_undo_a_price_changed_elsewhere() {
        let local = local();
        // Device 1 renames at t=10; the row it sends still carries the old price (100).
        let mut meta = FieldMetaMap::new();
        meta.insert("name".into(), stamp(10, 1));
        let op = update(10, json!({ "name": "Filtre à huile", "price": 100 }), meta, &local);

        let merged = apply_operation(Some(&local), &op).unwrap().snapshot.unwrap();
        assert_eq!(merged.payload["name"], "Filtre à huile", "the field it changed is applied");
        assert_eq!(merged.payload["price"], 200, "the one it did not change is left as it was");
        assert_eq!(merged.field_meta["price"], stamp(5, 2), "and keeps its own stamp");
    }

    #[test]
    fn a_field_it_did_change_still_loses_to_a_newer_edit() {
        let local = local();
        // Device 1 also changed the price, but at t=3: device 2's t=5 edit is newer.
        let mut meta = FieldMetaMap::new();
        meta.insert("price".into(), stamp(3, 1));
        let op = update(10, json!({ "name": "Filtre", "price": 100 }), meta, &local);

        let merged = apply_operation(Some(&local), &op).unwrap().snapshot.unwrap();
        assert_eq!(merged.payload["price"], 200);
    }

    #[test]
    fn an_operation_that_changed_nothing_changes_nothing() {
        let local = local();
        let mut meta = FieldMetaMap::new();
        meta.insert("_unchanged".into(), stamp(10, 1));
        let op = update(10, json!({ "name": "Autre", "price": 1 }), meta, &local);

        let merged = apply_operation(Some(&local), &op).unwrap().snapshot.unwrap();
        assert_eq!(merged.payload, json!({ "name": "Filtre", "price": 200 }));
    }

    #[test]
    fn an_older_device_without_stamps_is_still_judged_by_the_time_of_its_operation() {
        let local = local();
        let op = update(10, json!({ "name": "Filtre à huile", "price": 100 }), FieldMetaMap::new(), &local);

        let merged = apply_operation(Some(&local), &op).unwrap().snapshot.unwrap();
        assert_eq!(merged.payload["price"], 100, "as before: the later operation wins every field");
    }
}
