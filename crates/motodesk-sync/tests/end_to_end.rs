//! End-to-end style test of the sync engine semantics, using in-memory
//! snapshots as stand-ins for the SQLite/Postgres entity tables.
//!
//! Scenario: device A creates a client, then device B (LAN peer) receives the
//! op and re-applies it locally. Later, device B amends a different field; the
//! merge result should contain both A's and B's values, with no conflict
//! recorded. Then device A pushes a conflicting edit on the same field as B's
//! later edit; LWW + device_id tie-break must reproduce the same final
//! snapshot on every node.

use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, EntitySnapshot, FieldMetaMap, FieldStamp, OpKind};
use motodesk_sync::field_meta::stamps_for_payload;
use motodesk_sync::{apply_operation, ApplyOutcome};
use serde_json::json;
use uuid::Uuid;

fn dev(n: u8) -> Uuid {
    Uuid::from_bytes([n; 16])
}

fn op(
    device_id: Uuid,
    seq: i64,
    op: OpKind,
    payload: serde_json::Value,
    field_meta: FieldMetaMap,
    eid: Uuid,
) -> ChangeLogEntry {
    ChangeLogEntry {
        id: Uuid::new_v4(),
        device_id,
        seq,
        entity_type: EntityType::Client,
        entity_id: eid,
        op,
        payload,
        field_meta,
        created_at: Utc::now(),
        synced_to_cloud: false,
        relayed: false,
    }
}

/// Replicate `op` into a node's in-memory store via the same `apply_operation`
/// the desktop/backend sync layer uses.
fn replicate(node: &mut Option<EntitySnapshot>, change: &ChangeLogEntry) -> ApplyOutcome {
    let r = apply_operation(node.as_ref(), change).expect("apply");
    if let Some(snap) = r.snapshot.clone() {
        *node = Some(snap);
    }
    r.outcome
}

#[test]
fn insert_replicates_across_peers() {
    let eid = Uuid::new_v4();
    let device_a = dev(1);
    let device_b = dev(2);

    let payload = json!({ "last_name": "Martin", "phone": "0102" });
    let meta = stamps_for_payload(&payload, device_a, Utc::now());
    let insert = op(device_a, 1, OpKind::Insert, payload.clone(), meta, eid);

    let mut node_a: Option<EntitySnapshot> = None;
    let mut node_b: Option<EntitySnapshot> = None;
    let mut cloud: Option<EntitySnapshot> = None;

    // Originator
    replicate(&mut node_a, &insert);
    // Mesh peer
    replicate(&mut node_b, &insert);
    // Cloud push
    replicate(&mut cloud, &insert);

    for (name, n) in [("A", &node_a), ("B", &node_b), ("Cloud", &cloud)] {
        let snap = n
            .as_ref()
            .unwrap_or_else(|| panic!("{name} missing snapshot"));
        assert_eq!(snap.payload["last_name"], "Martin", "{name} last_name");
        assert_eq!(snap.payload["phone"], "0102", "{name} phone");
    }
    let _ = device_b; // silence unused
}

#[test]
fn concurrent_field_edits_converge_with_lww() {
    let eid = Uuid::new_v4();
    let device_a = dev(1);
    let device_b = dev(2);

    let t0 = Utc::now();
    let initial = json!({ "last_name": "Martin", "phone": "0102", "notes": null });
    let meta0 = stamps_for_payload(&initial, device_a, t0);
    let insert = op(device_a, 1, OpKind::Insert, initial.clone(), meta0, eid);

    let mut node_a: Option<EntitySnapshot> = None;
    let mut node_b: Option<EntitySnapshot> = None;
    let mut cloud: Option<EntitySnapshot> = None;
    replicate(&mut node_a, &insert);
    replicate(&mut node_b, &insert);
    replicate(&mut cloud, &insert);

    // Device A updates phone at t1
    let t1 = t0 + chrono::Duration::seconds(5);
    let mut a_meta = FieldMetaMap::new();
    a_meta.insert("phone".into(), FieldStamp::new(t1, device_a));
    let mut op_a = op(
        device_a,
        2,
        OpKind::Update,
        json!({ "phone": "0103" }),
        a_meta,
        eid,
    );
    op_a.created_at = t1;

    // Device B updates phone at t2 > t1 — B wins, no conflict because A's local
    // hasn't replicated to B yet (out-of-order delivery is fine).
    let t2 = t0 + chrono::Duration::seconds(10);
    let mut b_meta = FieldMetaMap::new();
    b_meta.insert("phone".into(), FieldStamp::new(t2, device_b));
    let mut op_b = op(
        device_b,
        2,
        OpKind::Update,
        json!({ "phone": "0104" }),
        b_meta,
        eid,
    );
    op_b.created_at = t2;

    // Order 1: A applies, then B applies — final phone = 0104 (B wins on LWW).
    replicate(&mut node_a, &op_a);
    replicate(&mut node_a, &op_b);
    // Order 2: B applies, then A applies — final phone must still be 0104.
    replicate(&mut node_b, &op_b);
    replicate(&mut node_b, &op_a);
    // Cloud sees both (arbitrary order) — same final state.
    replicate(&mut cloud, &op_b);
    replicate(&mut cloud, &op_a);

    for (name, n) in [("A", &node_a), ("B", &node_b), ("Cloud", &cloud)] {
        let snap = n.as_ref().unwrap();
        assert_eq!(snap.payload["phone"], "0104", "{name} phone diverged");
        assert_eq!(snap.payload["last_name"], "Martin", "{name} last_name");
    }
}

#[test]
fn delete_then_late_update_does_not_resurrect() {
    let eid = Uuid::new_v4();
    let device_a = dev(1);
    let device_b = dev(2);

    let t0 = Utc::now();
    let initial = json!({ "last_name": "X" });
    let meta = stamps_for_payload(&initial, device_a, t0);
    let insert = op(device_a, 1, OpKind::Insert, initial.clone(), meta, eid);

    let mut node_a: Option<EntitySnapshot> = None;
    let mut node_b: Option<EntitySnapshot> = None;
    replicate(&mut node_a, &insert);
    replicate(&mut node_b, &insert);

    // A deletes at t1.
    let t1 = t0 + chrono::Duration::seconds(2);
    let mut delete_op = op(
        device_a,
        2,
        OpKind::Delete,
        json!({"id": eid}),
        Default::default(),
        eid,
    );
    delete_op.created_at = t1;

    // B issues a stale update at t0+1s (before the delete decision).
    let t_stale = t0 + chrono::Duration::seconds(1);
    let mut b_meta = FieldMetaMap::new();
    b_meta.insert("last_name".into(), FieldStamp::new(t_stale, device_b));
    let mut stale_update = op(
        device_b,
        2,
        OpKind::Update,
        json!({"last_name": "Y"}),
        b_meta,
        eid,
    );
    stale_update.created_at = t_stale;

    // Order: B applies stale update first (still alive), then delete arrives.
    replicate(&mut node_b, &stale_update);
    replicate(&mut node_b, &delete_op);
    assert!(
        node_b.as_ref().unwrap().deleted_at.is_some(),
        "delete didn't tombstone B"
    );

    // Order: A applies delete first, then receives B's stale update — must stay deleted.
    replicate(&mut node_a, &delete_op);
    let outcome = replicate(&mut node_a, &stale_update);
    assert_eq!(outcome, ApplyOutcome::SkippedStale);
    assert!(
        node_a.as_ref().unwrap().deleted_at.is_some(),
        "stale update revived A"
    );
}

#[test]
fn out_of_order_delivery_is_idempotent() {
    let eid = Uuid::new_v4();
    let device_a = dev(1);

    let payload = json!({ "last_name": "M", "phone": "0100" });
    let meta = stamps_for_payload(&payload, device_a, Utc::now());
    let insert = op(device_a, 1, OpKind::Insert, payload, meta, eid);

    let mut store: Option<EntitySnapshot> = None;
    let o1 = replicate(&mut store, &insert);
    let o2 = replicate(&mut store, &insert);
    let o3 = replicate(&mut store, &insert);

    assert_eq!(o1, ApplyOutcome::Applied);
    // Re-applying the exact same op against the resulting snapshot is a
    // duplicate from the merge layer's perspective (row_version >= seq).
    assert_eq!(o2, ApplyOutcome::SkippedDuplicate);
    assert_eq!(o3, ApplyOutcome::SkippedDuplicate);
    assert_eq!(store.unwrap().payload["last_name"], "M");
}
