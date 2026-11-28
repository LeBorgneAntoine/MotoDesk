//! Whether a journal entry that arrives from elsewhere may be written down.
//!
//! The journal cannot be edited: what is stored stays. And a till's own journal and key are
//! what it sells with: a row slipped into the first takes the number its next entry needs, and
//! a key named in its place stops it signing. A PC of the network holds the network's secret,
//! not the signing keys of the other tills, so the signatures are what tells them apart.
//!
//! - An entry must be intact, and signed with a key its register has declared.
//! - A declaration (the opening, a replaced key) is signed by the key it declares, which proves
//!   nothing: anyone can make a key. For **this PC's own** register it is believed only if this
//!   PC holds the private half. A restored PC taking its own history back does.
//! - For another PC's register a declaration is taken at its word: the key it replaces is by
//!   definition lost. What it can spoil is the copy of that journal held here, not a till.
//!
//! A refused entry is set aside and tried again later, like any change that cannot be applied:
//! one that only arrived before the declaration it depends on gets in then.

use motodesk_fiscal::chain::FiscalEntry;
use motodesk_fiscal::{PublicKey, SecretKey};
use serde_json::Value;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::fiscal::keys::{SigningKeychain, SystemKeychain};
use crate::local_server::fiscal::code_for_device;

const OPENED: &str = "register_opened";
const KEY_CHANGED: &str = "register_key_changed";

fn refused(why: &str) -> LocalError {
    LocalError::BadRequest(format!("écriture fiscale refusée : {why}"))
}

/// The entry's payload, however it travelled (a value, or its text).
pub(super) fn payload_of(p: &Value) -> Value {
    match p.get("payload") {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
        Some(value) => value.clone(),
        None => Value::Null,
    }
}

pub(super) async fn admit(tx: &mut Transaction<'_, Sqlite>, p: &Value) -> LocalResult<()> {
    let text = |key: &str| p.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let register = text("register_id");
    let event = text("event_type");
    let payload = payload_of(p);

    let known: Option<(String, String, bool)> = sqlx::query_as(
        "SELECT code, public_key, EXISTS (SELECT 1 FROM fiscal_counters c WHERE c.register_id = r.id)
           FROM fiscal_registers r WHERE r.id = $1",
    )
    .bind(&register)
    .fetch_optional(&mut **tx)
    .await?;

    let code = match &known {
        Some((code, _, _)) => code.clone(),
        None if event == OPENED => payload.get("register_code").and_then(Value::as_str).unwrap_or_default().to_string(),
        // Its register is not here yet: the journal refuses the row, and it waits.
        None => return Ok(()),
    };

    // A sealed entry names its register by the code, not by the row.
    let entry = FiscalEntry {
        register_id: code.clone(),
        seq: p.get("seq").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0),
        event_type: event.clone(),
        occurred_at: text("occurred_at"),
        payload: payload.clone(),
        payload_hash: text("payload_hash"),
        prev_hash: text("prev_hash"),
        chain_hash: text("chain_hash"),
        signature: text("signature"),
    };
    if !entry.is_intact() {
        return Err(refused("son contenu ne correspond pas à ses empreintes"));
    }

    // The keys this register has: the one on record and the ones its journal declared.
    let mut keys: Vec<String> = Vec::new();
    if let Some((_, recorded, _)) = &known {
        keys.push(recorded.clone());
        let declarations: Vec<(String,)> =
            sqlx::query_as("SELECT payload FROM fiscal_journal WHERE register_id = $1 AND event_type IN ($2, $3)")
                .bind(&register)
                .bind(OPENED)
                .bind(KEY_CHANGED)
                .fetch_all(&mut **tx)
                .await?;
        keys.extend(declarations.iter().filter_map(|(payload,)| {
            let payload: Value = serde_json::from_str(payload).ok()?;
            payload.get("public_key")?.as_str().map(str::to_string)
        }));
    }

    let declaration = event == KEY_CHANGED || (event == OPENED && known.is_none());
    let signers = if declaration {
        let Some(declared) = payload.get("public_key").and_then(Value::as_str) else {
            return Err(refused("elle ne nomme pas de clé"));
        };
        if is_own(tx, &code, known.as_ref().is_some_and(|(_, _, counted)| *counted)).await?
            && !keys.iter().any(|key| key == declared)
            && !held_here(tx, &code, declared).await?
        {
            return Err(refused("elle nomme pour cette caisse une clé que ce poste n'a pas"));
        }
        vec![declared.to_string()]
    } else {
        keys
    };

    let signed = signers.iter().filter_map(|hex| PublicKey::from_hex(hex).ok()).any(|key| key.verify_entry(&entry).is_ok());
    if !signed {
        return Err(refused("elle n'est pas signée par la clé de sa caisse"));
    }
    Ok(())
}

/// A register this PC keeps counters for, or one under the code this PC's register has (or will
/// have: its code is announced on the network before the first sale).
async fn is_own(tx: &mut Transaction<'_, Sqlite>, code: &str, counted: bool) -> LocalResult<bool> {
    if counted {
        return Ok(true);
    }
    let device: Option<(String,)> = sqlx::query_as("SELECT device_id FROM sync_state WHERE id = 1").fetch_optional(&mut **tx).await?;
    Ok(device.and_then(|(id,)| Uuid::parse_str(&id).ok()).is_some_and(|id| code_for_device(id) == code))
}

/// Whether the private half of `public_key` is on this PC, in the keychain or the database.
async fn held_here(tx: &mut Transaction<'_, Sqlite>, code: &str, public_key: &str) -> LocalResult<bool> {
    let in_database: Option<(String,)> = sqlx::query_as("SELECT secret_key FROM fiscal_key_fallback WHERE register_code = $1")
        .bind(code)
        .fetch_optional(&mut **tx)
        .await?;
    Ok([SystemKeychain.load(code), in_database.map(|(hex,)| hex)]
        .into_iter()
        .flatten()
        .filter_map(|hex| SecretKey::from_hex(&hex).ok())
        .any(|key| key.public().to_hex() == public_key))
}
