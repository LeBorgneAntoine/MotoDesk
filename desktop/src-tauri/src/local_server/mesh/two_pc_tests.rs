//! Two PCs of one shop working: what is done on A must be on B, whatever order it arrives in
//! and however long B was away. Same harness as `network_tests` (real HTTP between them), with
//! the real till code doing the work.

use axum::extract::State;
use axum::Json;
use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, OpKind};
use serde_json::json;
use uuid::Uuid;

use crate::local_server::fiscal;
use crate::local_server::local_accounts;
use crate::local_server::mesh::network_tests::serve;
use crate::local_server::mesh::{self, catchup, join, PeerInfo};
use crate::local_server::routes::pos::{checkout, CheckoutBody, CheckoutPayment};
use crate::local_server::routes::sales::{compute, LineInputBody};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log::{self, field_meta_for_patch};
use crate::local_server::test_support::{insert_product, shop};

/// A has an administrator; B has joined and has caught up.
async fn two_pcs() -> (SharedState, PeerInfo, SharedState) {
    let a = shop().await;
    local_accounts::setup(&a, "camille", "mot de passe admin", "mot de passe admin").await.unwrap();
    let peer_a = serve(&a).await;
    let b = shop().await;
    join::join_network(&b, "camille", "mot de passe admin", std::slice::from_ref(&peer_a)).await.unwrap();
    (a, peer_a, b)
}

/// A product on A's shelf, announced to the network as the catalogue screen would.
async fn announced_product(a: &SharedState, stock: i64) -> Uuid {
    let id = insert_product(a, "Huile", Some("HUI-1"), stock, false, 1_200, 2_000).await;
    let now = Utc::now().to_rfc3339();
    sync_log::append(
        &a.db,
        a.device_id().await.unwrap(),
        EntityType::Product,
        id,
        OpKind::Insert,
        json!({
            "id": id.to_string(), "name": "Huile", "sku": "HUI-1", "unit": "piece",
            "is_service": 0, "selling_price_ttc_cents": 1_200, "vat_rate_bp": 2_000,
            "stock_milli": stock * 1_000, "is_active": 1, "created_at": now, "updated_at": now,
        }),
        None,
    )
    .await
    .unwrap();
    id
}

async fn sell(a: &SharedState, product: Uuid, quantity_milli: i64) {
    let line = LineInputBody {
        product_id: Some(product.to_string()),
        label: Some("Huile".into()),
        quantity_milli,
        unit_price_ht_cents: 1_000,
        vat_rate_bp: 2_000,
        discount_bp: 0,
        unit_price_ttc_cents: None,
    };
    let totals = compute(std::slice::from_ref(&line)).unwrap();
    let paid = CheckoutPayment {
        amount_cents: totals.total_ttc_cents,
        method: Some("cash".into()),
        bank: None,
        cheque_drawer: None,
        notes: None,
        auth_ref: None,
    };
    let _ = checkout(
        State(a.clone()),
        Json(CheckoutBody {
            client_id: None,
            kind: None,
            notes: None,
            lines: vec![line],
            payments: vec![paid],
        }),
    )
    .await
    .unwrap();
}

async fn count(state: &SharedState, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&state.db).await.unwrap()
}

/// Changes B received and could not apply.
async fn refused(state: &SharedState) -> Vec<String> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT entity_type, payload FROM audit_log WHERE action = 'sync_op_rejected'")
            .fetch_all(&state.db)
            .await
            .unwrap();
    rows.into_iter()
        .map(|(entity, payload)| {
            let why: serde_json::Value = serde_json::from_str(&payload).unwrap();
            format!("{entity}: {}", why["error"].as_str().unwrap_or("?"))
        })
        .collect()
}

#[tokio::test]
async fn a_till_sale_moves_the_stock_on_the_other_pc() {
    let (a, peer_a, b) = two_pcs().await;
    let oil = announced_product(&a, 10).await;
    sell(&a, oil, 2_000).await;
    catchup::pull_from(&b, &peer_a).await.unwrap();

    assert_eq!(refused(&b).await, Vec::<String>::new());
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sales").await, 1);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM payments").await, 1);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM stock_movements").await, 1);
    assert_eq!(count(&b, "SELECT stock_milli FROM products").await, 8_000);
}

#[tokio::test]
async fn the_other_pc_holds_a_copy_of_the_fiscal_journal() {
    let (a, peer_a, b) = two_pcs().await;
    let oil = announced_product(&a, 10).await;
    sell(&a, oil, 1_000).await;
    catchup::pull_from(&b, &peer_a).await.unwrap();

    let mine = count(&a, "SELECT COUNT(*) FROM fiscal_journal").await;
    assert!(mine >= 3, "opening, receipt, payment");
    assert_eq!(count(&b, "SELECT COUNT(*) FROM fiscal_registers").await, 1);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM fiscal_journal").await, mine);
    assert_eq!(refused(&b).await, Vec::<String>::new());

    // The copy verifies on its own: chain and signatures.
    let report = fiscal::verify_all(&b).await.unwrap();
    assert!(report.ok, "{:?}", report.registers);
    assert!(report.registers[0].signatures_verified >= 3);
}

/// A till opened before its opening entry was sent: starting the application sends it.
#[tokio::test]
async fn journal_entries_never_sent_are_sent_at_startup() {
    let (a, peer_a, b) = two_pcs().await;
    let oil = announced_product(&a, 10).await;
    sell(&a, oil, 1_000).await;
    sqlx::query(
        "DELETE FROM change_log WHERE entity_type = 'fiscal_entry'
          AND json_extract(payload, '$.event_type') = 'register_opened'",
    )
    .execute(&a.db)
    .await
    .unwrap();

    assert_eq!(fiscal::announce_unsent(&a).await.unwrap(), 1);
    assert_eq!(fiscal::announce_unsent(&a).await.unwrap(), 0, "only once");

    catchup::pull_from(&b, &peer_a).await.unwrap();
    let mine = count(&a, "SELECT COUNT(*) FROM fiscal_journal").await;
    assert_eq!(count(&b, "SELECT COUNT(*) FROM fiscal_journal").await, mine);
}

fn op(device: Uuid, seq: i64, entity_type: EntityType, id: Uuid, payload: serde_json::Value) -> ChangeLogEntry {
    ChangeLogEntry {
        id: Uuid::new_v4(),
        device_id: device,
        seq,
        entity_type,
        entity_id: id,
        op: OpKind::Insert,
        field_meta: field_meta_for_patch(&Default::default(), &payload, device),
        payload,
        created_at: Utc::now(),
        synced_to_cloud: false,
        relayed: false,
    }
}

/// A vehicle recorded on one PC for a customer created on another: a PC catching up is sent
/// one device's changes and then the other's, so the vehicle can come first.
#[tokio::test]
async fn a_late_pc_keeps_records_that_arrive_before_what_they_refer_to() {
    let (a, peer_a, b) = two_pcs().await;
    // The vehicle's device sorts before the customer's: it is served first.
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(u128::MAX);
    let client = Uuid::new_v4();
    let vehicle = Uuid::new_v4();
    mesh::apply_batch(
        &a,
        vec![op(second, 1, EntityType::Client, client, json!({ "id": client.to_string(), "last_name": "Girard" }))],
    )
    .await
    .unwrap();
    mesh::apply_batch(
        &a,
        vec![op(
            first,
            1,
            EntityType::Vehicle,
            vehicle,
            json!({ "id": vehicle.to_string(), "client_id": client.to_string(), "brand": "Yamaha", "model": "MT-07" }),
        )],
    )
    .await
    .unwrap();

    catchup::pull_from(&b, &peer_a).await.unwrap();
    assert_eq!(count(&b, "SELECT COUNT(*) FROM clients").await, 1);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM vehicles").await, 1, "{:?}", refused(&b).await);
}

/// B missed changes while off, then received a newer one by push before it caught up.
#[tokio::test]
async fn a_pc_that_missed_changes_still_gets_them_after_a_newer_one() {
    let (a, peer_a, b) = two_pcs().await;
    let device = Uuid::new_v4();
    let name = |n: &str, seq: i64| {
        let id = Uuid::new_v4();
        op(device, seq, EntityType::Client, id, json!({ "id": id.to_string(), "last_name": n }))
    };
    let history = vec![name("Bernard", 1), name("Moreau", 2), name("Girard", 3)];
    mesh::apply_batch(&a, history.clone()).await.unwrap();
    // Only the last one reaches B directly.
    mesh::apply_batch(&b, vec![history[2].clone()]).await.unwrap();

    catchup::pull_from(&b, &peer_a).await.unwrap();
    assert_eq!(count(&b, "SELECT COUNT(*) FROM clients").await, 3);
}

/// A deletion can overtake the creation it follows (pushed live while the creation is still to
/// be caught up). The row must end up deleted, not come back.
#[tokio::test]
async fn a_deletion_that_overtakes_its_creation_still_deletes() {
    let b = shop().await;
    let device = Uuid::new_v4();
    let id = Uuid::new_v4();
    let created = op(device, 1, EntityType::Client, id, json!({ "id": id.to_string(), "last_name": "Moreau" }));
    let mut deleted = op(device, 2, EntityType::Client, id, json!({ "id": id.to_string() }));
    deleted.op = OpKind::Delete;

    mesh::apply_batch(&b, vec![deleted]).await.unwrap();
    mesh::apply_batch(&b, vec![created]).await.unwrap();

    assert_eq!(count(&b, "SELECT COUNT(*) FROM clients WHERE deleted_at IS NULL").await, 0);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_deferred").await, 0);
}
