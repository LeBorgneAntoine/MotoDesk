//! What a PC accepts into its fiscal journal from another. A PC of the network holds the
//! network's secret, not the signing keys of the other tills: it must not be able to write
//! into their journals, or to name a key for them. The journal cannot be edited, so what gets
//! in stays, and a till whose journal or key is spoiled stops selling for good.

use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, OpKind};
use motodesk_fiscal::chain::{ChainTip, FiscalEntry, GENESIS_HASH};
use motodesk_fiscal::numbering::normalise_register_code;
use motodesk_fiscal::SecretKey;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::local_server::fiscal::{self, EntryLinks};
use crate::local_server::mesh::apply_batch;
use crate::local_server::state::SharedState;
use crate::local_server::test_support::shop;

/// A journal entry of the register `code`, following `tip` and signed with `key`.
fn entry(code: &str, tip: &ChainTip, event: &str, payload: Value, key: &SecretKey) -> FiscalEntry {
    let mut entry = motodesk_fiscal::seal(tip, code, event, Utc::now(), payload).unwrap();
    entry.signature = key.sign_entry(&entry).unwrap();
    entry
}

fn op(register_row: &str, entry: &FiscalEntry) -> ChangeLogEntry {
    let device = Uuid::new_v4();
    let payload = json!({
        "register_id": register_row, "seq": entry.seq, "event_type": entry.event_type,
        "occurred_at": entry.occurred_at, "business_day": &entry.occurred_at[..10],
        "payload": entry.payload, "payload_hash": entry.payload_hash, "prev_hash": entry.prev_hash,
        "chain_hash": entry.chain_hash, "signature": entry.signature, "total_ttc_cents": 0,
    });
    ChangeLogEntry {
        id: Uuid::new_v4(),
        device_id: device,
        seq: entry.seq,
        entity_type: EntityType::FiscalEntry,
        entity_id: Uuid::new_v4(),
        op: OpKind::Insert,
        field_meta: Default::default(),
        payload,
        created_at: Utc::now(),
        synced_to_cloud: false,
        relayed: false,
    }
}

async fn tip(state: &SharedState, register_row: &str) -> ChainTip {
    let (seq, chain_hash, occurred_at): (i64, String, String) =
        sqlx::query_as("SELECT last_seq, last_chain_hash, last_occurred_at FROM fiscal_counters WHERE register_id = $1")
            .bind(register_row)
            .fetch_one(&state.db)
            .await
            .unwrap();
    ChainTip { seq, chain_hash, occurred_at }
}

async fn rows(state: &SharedState, register_row: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM fiscal_journal WHERE register_id = $1")
        .bind(register_row)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

/// The key this PC holds for its register (tests keep it in the database, not the keychain).
async fn own_key(state: &SharedState, code: &str) -> SecretKey {
    let hex: String = sqlx::query_scalar("SELECT secret_key FROM fiscal_key_fallback WHERE register_code = $1")
        .bind(code)
        .fetch_one(&state.db)
        .await
        .unwrap();
    SecretKey::from_hex(&hex).unwrap()
}

async fn recorded_key(state: &SharedState, register_row: &str) -> String {
    sqlx::query_scalar("SELECT public_key FROM fiscal_registers WHERE id = $1").bind(register_row).fetch_one(&state.db).await.unwrap()
}

/// This till can still seal an event.
async fn seals(state: &SharedState) -> Result<(), String> {
    let register = fiscal::current(state).await.map_err(|e| e.to_string())?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.map_err(|e| e.to_string())?;
    fiscal::append(&mut tx, &register, motodesk_fiscal::FiscalEventType::DrawerOpened, Utc::now(), json!({}), EntryLinks::default())
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

fn code_of(device: Uuid) -> String {
    normalise_register_code(&format!("C{}", &device.simple().to_string()[..4]))
}

#[tokio::test]
async fn no_other_pc_writes_into_this_tills_journal() {
    let state = shop().await;
    let own = fiscal::current(&state).await.unwrap();
    let id = own.id.to_string();
    let before = rows(&state, &id).await;

    let forged = entry(&own.code, &tip(&state, &id).await, "document_sealed", json!({ "reference": "FA-forged" }), &SecretKey::generate());
    apply_batch(&state, vec![op(&id, &forged)]).await.unwrap();

    assert_eq!(rows(&state, &id).await, before, "an entry signed by a stranger was stored");
    seals(&state).await.expect("the till still seals");
}

#[tokio::test]
async fn no_other_pc_names_this_tills_signing_key() {
    let state = shop().await;
    let own = fiscal::current(&state).await.unwrap();
    let id = own.id.to_string();
    let key = recorded_key(&state, &id).await;
    let stranger = SecretKey::generate();

    // Signed by the very key it declares, as a real replacement is.
    let declared = json!({ "register_code": own.code, "public_key": stranger.public().to_hex() });
    let forged = entry(&own.code, &tip(&state, &id).await, "register_key_changed", declared, &stranger);
    apply_batch(&state, vec![op(&id, &forged)]).await.unwrap();

    assert_eq!(recorded_key(&state, &id).await, key, "the till's key was replaced from outside");
    seals(&state).await.expect("the till still seals");
}

#[tokio::test]
async fn no_register_is_planted_under_this_pcs_code() {
    let state = shop().await;
    // Before this till has sealed anything. Its code is announced on the network.
    let code = code_of(state.device_id().await.unwrap());
    let stranger = SecretKey::generate();
    let planted = Uuid::new_v4().to_string();
    let opening = json!({ "register_code": code, "public_key": stranger.public().to_hex(), "device_id": Uuid::new_v4().to_string() });
    let first = ChainTip { seq: 0, chain_hash: GENESIS_HASH.into(), occurred_at: String::new() };
    let forged = entry(&code, &first, "register_opened", opening, &stranger);
    apply_batch(&state, vec![op(&planted, &forged)]).await.unwrap();

    seals(&state).await.expect("the till opens its own register and seals");
    assert_ne!(recorded_key(&state, &fiscal::current(&state).await.unwrap().id.to_string()).await, stranger.public().to_hex());
}

#[tokio::test]
async fn another_tills_journal_is_taken_only_as_it_wrote_it() {
    let state = shop().await;
    let other = Uuid::new_v4().to_string();
    let key = SecretKey::generate();
    let first = ChainTip { seq: 0, chain_hash: GENESIS_HASH.into(), occurred_at: String::new() };
    let opening = json!({ "register_code": "CBEEF", "public_key": key.public().to_hex(), "device_id": Uuid::new_v4().to_string() });
    let opened = entry("CBEEF", &first, "register_opened", opening, &key);
    let sealed = entry("CBEEF", &opened.tip(), "document_sealed", json!({ "total": 100 }), &key);
    apply_batch(&state, vec![op(&other, &opened), op(&other, &sealed)]).await.unwrap();
    assert_eq!(rows(&state, &other).await, 2);

    // Altered after it was signed, then signed by someone else.
    let mut altered = entry("CBEEF", &sealed.tip(), "document_sealed", json!({ "total": 100 }), &key);
    altered.payload = json!({ "total": 1 });
    let foreign = entry("CBEEF", &sealed.tip(), "document_sealed", json!({ "total": 5 }), &SecretKey::generate());
    let mut unsigned = entry("CBEEF", &sealed.tip(), "document_sealed", json!({ "total": 7 }), &key);
    unsigned.signature = String::new();
    apply_batch(&state, vec![op(&other, &altered), op(&other, &foreign), op(&other, &unsigned)]).await.unwrap();
    assert_eq!(rows(&state, &other).await, 2, "only what that till signed, unaltered");

    // A till whose key was lost declares a new one, and signs with it from there on.
    let replacement = SecretKey::generate();
    let declared = json!({ "register_code": "CBEEF", "public_key": replacement.public().to_hex() });
    let changed = entry("CBEEF", &sealed.tip(), "register_key_changed", declared, &replacement);
    let after = entry("CBEEF", &changed.tip(), "document_sealed", json!({ "total": 9 }), &replacement);
    apply_batch(&state, vec![op(&other, &changed), op(&other, &after)]).await.unwrap();
    assert_eq!(rows(&state, &other).await, 4);
    assert_eq!(recorded_key(&state, &other).await, replacement.public().to_hex());
}

/// A restored PC is behind its own journal and takes the rest back from the others: entries it
/// signed itself must still come in, including a replacement of its key that it holds.
#[tokio::test]
async fn a_till_takes_its_own_entries_back() {
    let state = shop().await;
    let own = fiscal::current(&state).await.unwrap();
    let id = own.id.to_string();
    let before = rows(&state, &id).await;

    let mine = entry(&own.code, &tip(&state, &id).await, "document_sealed", json!({ "total": 1 }), &own_key(&state, &own.code).await);
    apply_batch(&state, vec![op(&id, &mine)]).await.unwrap();
    assert_eq!(rows(&state, &id).await, before + 1);

    // A key replacement this PC made: the new key is on this PC.
    let replacement = SecretKey::generate();
    sqlx::query("UPDATE fiscal_key_fallback SET secret_key = $2 WHERE register_code = $1")
        .bind(&own.code)
        .bind(replacement.to_hex())
        .execute(&state.db)
        .await
        .unwrap();
    let declared = json!({ "register_code": own.code, "public_key": replacement.public().to_hex() });
    let changed = entry(&own.code, &mine.tip(), "register_key_changed", declared, &replacement);
    apply_batch(&state, vec![op(&id, &changed)]).await.unwrap();
    assert_eq!(recorded_key(&state, &id).await, replacement.public().to_hex());
}

/// A refused change waits to be tried again. It must not hold back the real entry that travels
/// under the same id, or sending a spoiled copy first would keep a till's journal out for good.
#[tokio::test]
async fn a_spoiled_copy_sent_first_does_not_keep_the_real_entry_out() {
    let state = shop().await;
    let other = Uuid::new_v4().to_string();
    let key = SecretKey::generate();
    let first = ChainTip { seq: 0, chain_hash: GENESIS_HASH.into(), occurred_at: String::new() };
    let opening = json!({ "register_code": "CBEEF", "public_key": key.public().to_hex(), "device_id": Uuid::new_v4().to_string() });
    let opened = entry("CBEEF", &first, "register_opened", opening, &key);
    let sealed = entry("CBEEF", &opened.tip(), "document_sealed", json!({ "total": 100 }), &key);
    apply_batch(&state, vec![op(&other, &opened)]).await.unwrap();

    let real = op(&other, &sealed);
    let mut spoiled = op(&other, &sealed);
    spoiled.entity_id = real.entity_id;
    spoiled.payload["payload"] = json!({ "total": 1 });
    apply_batch(&state, vec![spoiled]).await.unwrap();
    apply_batch(&state, vec![real]).await.unwrap();

    let stored: String = sqlx::query_scalar("SELECT payload FROM fiscal_journal WHERE register_id = $1 AND seq = 2")
        .bind(&other)
        .fetch_one(&state.db)
        .await
        .expect("the real entry is stored");
    assert_eq!(stored, r#"{"total":100}"#);
}

/// Cloud recovery lifts its write block only when this PC's journal is back. A replacement key
/// this PC does not have leaves an entry out: writing on from before it would fork the chain.
#[tokio::test]
async fn a_recovery_missing_part_of_the_journal_does_not_lift_the_block() {
    use crate::local_server::recovery::{apply_page, ensure_journal_is_back};

    let state = shop().await;
    let own = fiscal::current(&state).await.unwrap();
    let id = own.id.to_string();

    let mine = entry(&own.code, &tip(&state, &id).await, "document_sealed", json!({ "total": 1 }), &own_key(&state, &own.code).await);
    let back = op(&id, &mine);
    let back_id = back.id;
    apply_page(&state, vec![back]).await.unwrap();
    ensure_journal_is_back(&state, &[back_id]).await.expect("everything is in");

    let unheld = SecretKey::generate();
    let declared = json!({ "register_code": own.code, "public_key": unheld.public().to_hex() });
    let changed = op(&id, &entry(&own.code, &mine.tip(), "register_key_changed", declared, &unheld));
    let changed_id = changed.id;
    apply_page(&state, vec![changed]).await.unwrap();
    assert!(ensure_journal_is_back(&state, &[back_id, changed_id]).await.is_err());
}
