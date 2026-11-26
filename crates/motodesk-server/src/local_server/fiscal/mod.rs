//! The register: everything that writes to the permanent fiscal journal.
//!
//! One register per device. It owns a hash chain, a signing key, a document
//! number series and the two running totals, and it is the only code allowed to
//! insert into `fiscal_journal`. Routes call [`append`] inside their own
//! transaction so that the business row and the journal entry either both land
//! or neither does.
//!
//! Legal basis: CGI art. 286 I-3° bis and BOI-TVA-DECLA-30-10-30, whose
//! paragraphs are cited where they drive a decision.

pub mod archive;
pub mod closures;
pub mod fec;
pub mod keyrepair;
pub mod keytool;
pub mod keys;
pub mod mentions;
pub mod recovery;

use chrono::{DateTime, Datelike, Local, Utc};
use motodesk_core::entity::EntityType;
use motodesk_core::sync::OpKind;
use motodesk_fiscal::{
    chain::{format_timestamp, ChainTip},
    numbering::{format_number, normalise_register_code, DocumentKind},
    ChainBreak, ChainReport, FiscalEntry, FiscalEventType, PublicKey, SecretKey,
    GENESIS_HASH,
};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

/// Named in the attestation and stamped on every entry, so a change to the
/// ISCA code can be told apart from a change to anything else.
pub const SOFTWARE_VERSION: &str = concat!("Motodesk ", env!("CARGO_PKG_VERSION"));

/// A device's register, resolved once per request.
#[derive(Clone)]
pub struct Register {
    pub id: Uuid,
    pub code: String,
    pub company_id: Option<String>,
    key: std::sync::Arc<SecretKey>,
    pub key_store: keys::KeyStore,
}

impl Register {
    pub fn public_key_hex(&self) -> String {
        self.key.public().to_hex()
    }
}

#[derive(Debug, Serialize)]
pub struct RegisterInfo {
    pub id: String,
    pub code: String,
    pub public_key: String,
    pub key_store: String,
    pub software_version: String,
    pub opened_at: String,
    pub last_seq: i64,
    pub last_chain_hash: String,
    pub period_grand_total_cents: i64,
    pub perpetual_total_cents: i64,
    /// How far the newest journal entry is ahead of this computer's clock, in
    /// seconds (0 when it is not). The chain cannot go back in time, so a clock that
    /// once ran fast freezes every later entry at that moment: this is how it shows.
    pub journal_ahead_secs: i64,
}

/// Resolve this device's register, opening one on first use.
///
/// The code is derived from the device id rather than assigned centrally: two
/// machines that have never met must still produce numbers that cannot collide,
/// and asking a server for a code would make invoicing depend on the network.
pub async fn current(state: &SharedState) -> LocalResult<Register> {
    current_with(state, &keys::SystemKeychain).await
}

/// [`current`], with the keychain given by the caller (tests give one of their own).
pub(crate) async fn current_with(
    state: &SharedState,
    keychain: &dyn keys::SigningKeychain,
) -> LocalResult<Register> {
    // First use can arrive as several requests at once (the till asks for the
    // session and the register in parallel). Without this, each would see "no
    // register yet" and generate its own key.
    static RESOLVE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = RESOLVE.lock().await;

    let device_id = state.device_id().await.map_err(LocalError::Internal)?;
    let company_id = state.company_id.read().await.map(|id| id.to_string());
    let code = code_for_device(device_id);

    let existing: Option<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, key_store, company_id, public_key FROM fiscal_registers WHERE code = $1",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await?;

    // An existing register signs with the key its journal names and no other: the key
    // is chosen among those at hand by the public key on record, not by where it is kept.
    let recorded = existing.as_ref().map(|(_, _, _, public_key)| public_key.as_str());
    let (key, key_store) = keys::load_or_create(keychain, &state.db, &code, recorded).await?;

    if let Some((id, stored_store, stored_company, stored_public_key)) = existing {
        // `load_or_create` already refuses any other key; this is the last line before
        // a signature is made, kept in case the choice above is ever changed.
        if key.public().to_hex() != stored_public_key {
            return Err(LocalError::BadRequest(format!(
                "la clé de signature de la caisse {code} ne correspond pas à la clé publique enregistrée ;                  les écritures fiscales sont bloquées pour ne pas produire de signatures invalides"
            )));
        }
        let id = Uuid::parse_str(&id).map_err(|e| LocalError::Internal(e.into()))?;
        return Ok(Register {
            id,
            code,
            company_id: stored_company.or(company_id),
            key: std::sync::Arc::new(key),
            key_store: if stored_store == "database" {
                keys::KeyStore::Database
            } else {
                keys::KeyStore::Keychain
            },
        });
    }

    let register = Register {
        id: Uuid::new_v4(),
        code,
        company_id,
        key: std::sync::Arc::new(key),
        key_store,
    };
    open_register(state, &register, device_id).await?;
    Ok(register)
}

fn code_for_device(device_id: Uuid) -> String {
    let hex = device_id.simple().to_string();
    normalise_register_code(&format!("C{}", &hex[..4]))
}

async fn open_register(
    state: &SharedState,
    register: &Register,
    device_id: Uuid,
) -> LocalResult<()> {
    let now = Utc::now();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;

    sqlx::query(
        r#"
        INSERT INTO fiscal_registers (
            id, company_id, code, label, device_id, public_key, key_store,
            software_version, opened_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        ON CONFLICT(company_id, code) DO NOTHING
        "#,
    )
    .bind(register.id.to_string())
    .bind(&register.company_id)
    .bind(&register.code)
    .bind(format!("Caisse {}", register.code))
    .bind(device_id.to_string())
    .bind(register.public_key_hex())
    .bind(register.key_store.as_str())
    .bind(SOFTWARE_VERSION)
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO fiscal_counters (
            register_id, last_seq, last_chain_hash, last_occurred_at,
            period_grand_total_cents, perpetual_total_cents, period_opened_on, updated_at
        ) VALUES ($1, 0, $2, '', 0, 0, $3, $4)
        ON CONFLICT(register_id) DO NOTHING
        "#,
    )
    .bind(register.id.to_string())
    .bind(GENESIS_HASH)
    .bind(business_day(now))
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await?;

    // § 273: a new software starts its counters at zero, and says so in the
    // first entry of the chain. The recovered Motogest turnover is archived
    // alongside rather than carried into the perpetual total.
    let opening = append(
        &mut tx,
        register,
        FiscalEventType::RegisterOpened,
        now,
        json!({
            "register_code": register.code,
            "device_id": device_id.to_string(),
            "public_key": register.public_key_hex(),
            "key_store": register.key_store.as_str(),
            "software_version": SOFTWARE_VERSION,
            "opening_perpetual_total_cents": 0,
        }),
        EntryLinks::default(),
    )
    .await?;

    tx.commit().await?;
    replicate(&state.db, device_id, register, &opening, &EntryLinks::default()).await?;
    Ok(())
}

/// Optional columns denormalised out of the payload for querying.
#[derive(Debug, Default, Clone)]
pub struct EntryLinks {
    pub document_ref: Option<String>,
    pub sale_id: Option<Uuid>,
    pub total_ttc_cents: i64,
    pub actor_id: Option<String>,
}

/// Seal one event into the register's chain.
///
/// Must run inside the caller's transaction: the journal entry and whatever
/// business row it describes have to become visible together, or a crash
/// between the two would leave a document with no fiscal trace.
pub async fn append(
    tx: &mut Transaction<'_, Sqlite>,
    register: &Register,
    event: FiscalEventType,
    occurred_at: DateTime<Utc>,
    payload: Value,
    links: EntryLinks,
) -> LocalResult<FiscalEntry> {
    // The cloud holds entries of this register that this database lacks (a restored
    // backup). Writing on would fork the chain: the cloud keeps its own and refuses
    // the contradicting entries, so they would never reach the off-site copy.
    let behind: Option<(i64,)> = sqlx::query_as("SELECT COALESCE(seq_gap, 0) FROM sync_state WHERE id = 1")
        .fetch_optional(&mut **tx)
        .await?;
    if behind.is_some_and(|(gap,)| gap > 0) {
        return Err(LocalError::BadRequest(
            "cette base est plus ancienne que ce que le cloud connaît de ce poste : récupérez d'abord les modifications manquantes (bandeau en haut de l'écran, ou Réglages › Synchronisation) avant d'émettre un document".into(),
        ));
    }

    let tip = load_tip(tx, register.id).await?;

    // A chain must not go backwards even if the system clock does. Rather than
    // refuse the sale -- which would strand a customer at the counter -- the
    // entry is stamped at the previous entry's time and the skew is recorded in
    // the payload, where an inspector can see it.
    let stamp = format_timestamp(occurred_at);
    let (stamp, clock_note) = if !tip.occurred_at.is_empty() && stamp < tip.occurred_at {
        (tip.occurred_at.clone(), Some(stamp))
    } else {
        (stamp, None)
    };
    let effective = chrono::DateTime::parse_from_rfc3339(&stamp)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(occurred_at);

    let payload = match clock_note {
        Some(reported) => merge_object(payload, json!({ "clock_skew_reported_at": reported })),
        None => payload,
    };

    let mut entry = motodesk_fiscal::seal(&tip, &register.code, event.as_str(), effective, payload)
        .map_err(|e| LocalError::Internal(anyhow::anyhow!(e)))?;
    entry.signature = register
        .key
        .sign_entry(&entry)
        .map_err(|e| LocalError::Internal(anyhow::anyhow!(e)))?;

    let entry_id = Uuid::new_v4();
    let day = business_day(effective);

    // A closed day is final. Without this a sale rung up after the Z would land
    // on a day whose totals are already sealed.
    if moves_money(event) {
        let closed: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM fiscal_closures WHERE register_id = $1 AND kind = 'day' AND period_start = $2",
        )
        .bind(register.id.to_string())
        .bind(&day)
        .fetch_optional(&mut **tx)
        .await?;
        if closed.is_some() {
            return Err(LocalError::BadRequest(format!(
                "la journée du {day} est clôturée : plus aucune vente ni aucun règlement ne peut y être enregistré. La caisse reprend demain."
            )));
        }
    }

    sqlx::query(
        r#"
        INSERT INTO fiscal_journal (
            id, register_id, seq, event_type, occurred_at, business_day,
            payload, payload_hash, prev_hash, chain_hash, signature,
            document_ref, sale_id, total_ttc_cents, actor_id, company_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
        "#,
    )
    .bind(entry_id.to_string())
    .bind(register.id.to_string())
    .bind(entry.seq)
    .bind(&entry.event_type)
    .bind(&entry.occurred_at)
    .bind(&day)
    .bind(entry.payload.to_string())
    .bind(&entry.payload_hash)
    .bind(&entry.prev_hash)
    .bind(&entry.chain_hash)
    .bind(&entry.signature)
    .bind(&links.document_ref)
    .bind(links.sale_id.map(|id| id.to_string()))
    .bind(links.total_ttc_cents)
    .bind(&links.actor_id)
    .bind(&register.company_id)
    .execute(&mut **tx)
    .await?;

    // Only documents move turnover. Counting payments too would double it.
    let turnover = if event.affects_turnover() {
        links.total_ttc_cents
    } else {
        0
    };

    sqlx::query(
        r#"
        UPDATE fiscal_counters SET
            last_seq = $2,
            last_chain_hash = $3,
            last_occurred_at = $4,
            period_grand_total_cents = period_grand_total_cents + $5,
            perpetual_total_cents = perpetual_total_cents + $5,
            updated_at = $6
        WHERE register_id = $1
        "#,
    )
    .bind(register.id.to_string())
    .bind(entry.seq)
    .bind(&entry.chain_hash)
    .bind(&entry.occurred_at)
    .bind(turnover)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **tx)
    .await?;

    Ok(entry)
}

/// Queue a sealed entry for replication. Journal rows travel as inserts only.
pub async fn replicate(
    pool: &SqlitePool,
    device_id: Uuid,
    register: &Register,
    entry: &FiscalEntry,
    links: &EntryLinks,
) -> LocalResult<()> {
    log_entry(
        pool,
        device_id,
        &register.id.to_string(),
        register.company_id.as_deref(),
        entry,
        links,
    )
    .await
}

/// A journal entry is known on the network by its chain hash, so that the same entry arriving
/// twice by different routes is recognised as one row rather than duplicated.
fn network_id(chain_hash: &str) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, chain_hash.as_bytes())
}

async fn log_entry(
    pool: &SqlitePool,
    device_id: Uuid,
    register_id: &str,
    company_id: Option<&str>,
    entry: &FiscalEntry,
    links: &EntryLinks,
) -> LocalResult<()> {
    let payload = json!({
        "register_id": register_id,
        "seq": entry.seq,
        "event_type": entry.event_type,
        "occurred_at": entry.occurred_at,
        "business_day": business_day_str(&entry.occurred_at),
        "payload": entry.payload,
        "payload_hash": entry.payload_hash,
        "prev_hash": entry.prev_hash,
        "chain_hash": entry.chain_hash,
        "signature": entry.signature,
        "document_ref": links.document_ref,
        "sale_id": links.sale_id.map(|id| id.to_string()),
        "total_ttc_cents": links.total_ttc_cents,
        "actor_id": links.actor_id,
        "company_id": company_id,
    });

    sync_log::append(
        pool,
        device_id,
        EntityType::FiscalEntry,
        network_id(&entry.chain_hash),
        OpKind::Insert,
        payload,
        None,
    )
    .await?;
    Ok(())
}

/// Send the entries of this PC's own registers that never reached the change log: the opening
/// entry used not to be sent at all, and without it the other PCs hold nothing of the journal.
pub async fn announce_unsent(state: &SharedState) -> LocalResult<usize> {
    #[derive(sqlx::FromRow)]
    struct Row {
        register_id: String,
        register_code: String,
        register_company: Option<String>,
        seq: i64,
        event_type: String,
        occurred_at: String,
        payload: String,
        payload_hash: String,
        prev_hash: String,
        chain_hash: String,
        signature: String,
        document_ref: Option<String>,
        sale_id: Option<String>,
        total_ttc_cents: i64,
        actor_id: Option<String>,
    }

    let device_id = state.device_id().await?;
    let sent: Vec<(String,)> =
        sqlx::query_as("SELECT entity_id FROM change_log WHERE entity_type = $1")
            .bind(EntityType::FiscalEntry.as_str())
            .fetch_all(&state.db)
            .await?;
    let sent: std::collections::HashSet<String> = sent.into_iter().map(|(id,)| id).collect();

    let rows: Vec<Row> = sqlx::query_as(
        r#"
        SELECT j.register_id, r.code AS register_code, r.company_id AS register_company, j.seq,
               j.event_type, j.occurred_at, j.payload, j.payload_hash, j.prev_hash, j.chain_hash,
               j.signature, j.document_ref, j.sale_id, j.total_ttc_cents, j.actor_id
          FROM fiscal_journal j
          JOIN fiscal_registers r ON r.id = j.register_id
         WHERE r.device_id = $1
         ORDER BY j.register_id, j.seq
        "#,
    )
    .bind(device_id.to_string())
    .fetch_all(&state.db)
    .await?;

    let mut announced = 0;
    for row in rows {
        if sent.contains(&network_id(&row.chain_hash).to_string()) {
            continue;
        }
        let entry = FiscalEntry {
            register_id: row.register_code,
            seq: row.seq,
            event_type: row.event_type,
            occurred_at: row.occurred_at,
            payload: serde_json::from_str(&row.payload).unwrap_or(Value::Null),
            payload_hash: row.payload_hash,
            prev_hash: row.prev_hash,
            chain_hash: row.chain_hash,
            signature: row.signature,
        };
        let links = EntryLinks {
            document_ref: row.document_ref,
            sale_id: row.sale_id.and_then(|id| Uuid::parse_str(&id).ok()),
            total_ttc_cents: row.total_ttc_cents,
            actor_id: row.actor_id,
        };
        log_entry(
            &state.db,
            device_id,
            &row.register_id,
            row.register_company.as_deref(),
            &entry,
            &links,
        )
        .await?;
        announced += 1;
    }
    Ok(announced)
}

async fn load_tip(tx: &mut Transaction<'_, Sqlite>, register_id: Uuid) -> LocalResult<ChainTip> {
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT last_seq, last_chain_hash, last_occurred_at FROM fiscal_counters WHERE register_id = $1",
    )
    .bind(register_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(match row {
        Some((seq, hash, at)) => ChainTip {
            seq,
            chain_hash: hash,
            occurred_at: at,
        },
        None => ChainTip::genesis(),
    })
}

/// Allocate the next number in a register's series for the given year.
///
/// The sequence is unbroken within the series, which is what CGI ann. II
/// art. 242 nonies A requires. It is allocated at validation and never before,
/// so an abandoned draft cannot leave a hole.
pub async fn next_number(
    tx: &mut Transaction<'_, Sqlite>,
    register: &Register,
    kind: DocumentKind,
    year: i32,
) -> LocalResult<(i64, String)> {
    sqlx::query(
        "INSERT INTO fiscal_number_series (register_id, kind, year, next_value)
         VALUES ($1, $2, $3, 1) ON CONFLICT(register_id, kind, year) DO NOTHING",
    )
    .bind(register.id.to_string())
    .bind(kind.as_str())
    .bind(year)
    .execute(&mut **tx)
    .await?;

    let (value,): (i64,) = sqlx::query_as(
        "UPDATE fiscal_number_series SET next_value = next_value + 1
         WHERE register_id = $1 AND kind = $2 AND year = $3
         RETURNING next_value - 1",
    )
    .bind(register.id.to_string())
    .bind(kind.as_str())
    .bind(year)
    .fetch_one(&mut **tx)
    .await?;

    let reference = format_number(kind, year, &register.code, value)
        .map_err(|e| LocalError::Internal(anyhow::anyhow!(e)))?;
    Ok((value, reference))
}

pub async fn info(state: &SharedState, register: &Register) -> LocalResult<RegisterInfo> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT opened_at, software_version FROM fiscal_registers WHERE id = $1")
            .bind(register.id.to_string())
            .fetch_optional(&state.db)
            .await?;
    let (opened_at, software_version) =
        row.unwrap_or_else(|| (String::new(), SOFTWARE_VERSION.to_string()));

    let counters: Option<(i64, String, i64, i64, String)> = sqlx::query_as(
        "SELECT last_seq, last_chain_hash, period_grand_total_cents, perpetual_total_cents, last_occurred_at
         FROM fiscal_counters WHERE register_id = $1",
    )
    .bind(register.id.to_string())
    .fetch_optional(&state.db)
    .await?;
    let (last_seq, last_chain_hash, period, perpetual, last_at) =
        counters.unwrap_or_else(|| (0, GENESIS_HASH.to_string(), 0, 0, String::new()));
    let journal_ahead_secs = ahead_of_clock_secs(&last_at, Utc::now());

    Ok(RegisterInfo {
        id: register.id.to_string(),
        code: register.code.clone(),
        public_key: register.public_key_hex(),
        key_store: register.key_store.as_str().to_string(),
        software_version,
        opened_at,
        last_seq,
        last_chain_hash,
        period_grand_total_cents: period,
        perpetual_total_cents: perpetual,
        journal_ahead_secs,
    })
}

/// Seconds by which `last_entry_at` is in the future of `now`, ignoring the few
/// minutes two machines can legitimately differ by.
fn ahead_of_clock_secs(last_entry_at: &str, now: DateTime<Utc>) -> i64 {
    let Ok(last) = DateTime::parse_from_rfc3339(last_entry_at) else {
        return 0;
    };
    let ahead = (last.with_timezone(&Utc) - now).num_seconds();
    if ahead > 600 {
        ahead
    } else {
        0
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;

    #[test]
    fn a_journal_ahead_of_the_clock_is_noticed() {
        let now = Utc::now();
        let hour = |h: i64| format_timestamp(now + chrono::Duration::hours(h));
        assert_eq!(ahead_of_clock_secs(&hour(-3), now), 0);
        assert_eq!(ahead_of_clock_secs(&format_timestamp(now), now), 0);
        assert!(ahead_of_clock_secs(&hour(48), now) > 47 * 3600);
        assert_eq!(ahead_of_clock_secs("", now), 0);
    }
}

/// Result of walking every chain in the database.
#[derive(Debug, Serialize)]
pub struct IntegrityReport {
    pub ok: bool,
    pub checked_at: String,
    pub registers: Vec<RegisterIntegrity>,
}

#[derive(Debug, Serialize)]
pub struct RegisterIntegrity {
    pub register_id: String,
    pub code: String,
    pub entries: i64,
    pub last_seq: i64,
    pub signatures_verified: i64,
    /// Entries carrying no signature at all. A chain whose hashes hold can still
    /// have been rewritten by anyone if its entries are not signed.
    pub unsigned: i64,
    /// Entries whose signature the register itself declares can no longer be
    /// verified (the key that made it was lost). Their hashes still chain.
    pub declared_unverifiable: i64,
    pub ok: bool,
    /// French-language description of the first break, if any.
    pub problem: Option<String>,
    pub problem_seq: Option<i64>,
    /// Everything else the cross-checks found, one sentence each.
    pub findings: Vec<String>,
}

/// The keys a register's entries are checked against: the ones its own chain
/// declares (the opening entry, then any `register_key_changed`), or, for a chain
/// whose opening entry this device does not hold, the key recorded for the register.
pub(crate) fn key_periods_of(
    entries: &[FiscalEntry],
    recorded_public_key: &str,
) -> motodesk_fiscal::chain::KeyPeriods {
    let mut periods: motodesk_fiscal::chain::KeyPeriods =
        motodesk_fiscal::chain::declared_key_periods(entries)
            .into_iter()
            .filter_map(|(from, hex)| PublicKey::from_hex(&hex).ok().map(|k| (from, k)))
            .collect();
    if periods.is_empty() {
        if let Ok(key) = PublicKey::from_hex(recorded_public_key) {
            periods.push((1, key));
        }
    }
    periods
}

/// Verify every register's chain, including replicas received from peers.
pub async fn verify_all(state: &SharedState) -> LocalResult<IntegrityReport> {
    let registers: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, code, public_key FROM fiscal_registers ORDER BY code")
            .fetch_all(&state.db)
            .await?;

    let mut out = Vec::with_capacity(registers.len());
    let mut all_ok = true;

    for (id, code, public_key) in registers {
        let entries = load_entries(&state.db, &id, 0, i64::MAX).await?;
        let periods = key_periods_of(&entries, &public_key);
        let unverifiable = motodesk_fiscal::chain::declared_unverifiable_ranges(&entries);
        let declared_unverifiable = entries
            .iter()
            .filter(|e| unverifiable.iter().any(|(from, to)| (*from..=*to).contains(&e.seq)))
            .count() as i64;
        let count = entries.len() as i64;

        let (mut ok, mut problem, mut problem_seq, last_seq, signatures, unsigned) =
            match motodesk_fiscal::chain::verify_chain_declared(&entries, &ChainTip::genesis(), &periods, &unverifiable) {
                Ok(ChainReport {
                    last_seq,
                    signatures_checked,
                    unsigned,
                    ..
                }) => (true, None, None, last_seq, signatures_checked as i64, unsigned as i64),
                Err(break_at) => {
                    let seq = break_at.seq();
                    (false, Some(describe(&break_at)), Some(seq), seq - 1, 0, 0)
                }
            };

        // Hashes alone only show that the entries still agree with each other. These
        // checks compare them with what lives outside the chain, which is how a
        // deleted tail or a swapped-in older file shows.
        let mut findings = Vec::new();
        if ok {
            findings = cross_checks(&state.db, &id, last_seq, &entries).await?;
            if unsigned > 0 {
                findings.push(format!(
                    "{unsigned} entrée(s) du journal ne portent aucune signature : leur contenu n'est protégé que par la chaîne."
                ));
            }
            if !findings.is_empty() {
                ok = false;
                problem = Some(findings[0].clone());
                problem_seq = None;
            }
        }
        if !ok {
            all_ok = false;
        }

        out.push(RegisterIntegrity {
            register_id: id,
            code,
            entries: count,
            last_seq,
            signatures_verified: signatures,
            unsigned,
            declared_unverifiable,
            ok,
            problem,
            problem_seq,
            findings,
        });
    }

    Ok(IntegrityReport {
        ok: all_ok,
        checked_at: Utc::now().to_rfc3339(),
        registers: out,
    })
}

fn describe(break_at: &ChainBreak) -> String {
    break_at.to_string()
}

/// Compare a register's journal with the rows that must agree with it.
async fn cross_checks(
    db: &SqlitePool,
    register_id: &str,
    journal_last_seq: i64,
    entries: &[FiscalEntry],
) -> LocalResult<Vec<String>> {
    let mut findings = Vec::new();

    // The counters are updated in the same transaction as every entry. A journal
    // that stops before them has lost its tail (or been replaced by an older file).
    let counters: Option<(i64, String)> = sqlx::query_as(
        "SELECT last_seq, last_chain_hash FROM fiscal_counters WHERE register_id = $1",
    )
    .bind(register_id)
    .fetch_optional(db)
    .await?;
    // A till this PC no longer writes (retired by a restore) gets its later entries from the
    // other PCs, while its counters stay where the backup left them. Behind them is still a loss.
    let retired: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM fiscal_registers r, sync_state s
          WHERE r.id = $1 AND s.id = 1 AND r.device_id IS NOT s.device_id",
    )
    .bind(register_id)
    .fetch_optional(db)
    .await?;
    if let Some((seq, hash)) = counters {
        let tip_hash = entries.last().map(|e| e.chain_hash.as_str()).unwrap_or(GENESIS_HASH);
        let grown_elsewhere = retired.is_some() && journal_last_seq > seq;
        if (seq != journal_last_seq || hash != tip_hash) && !grown_elsewhere {
            findings.push(format!(
                "le journal s'arrête à l'entrée n° {journal_last_seq} alors que la caisse en est à la n° {seq} : des écritures ont été supprimées, ou une sauvegarde plus ancienne a été restaurée."
            ));
        }
    }

    // Every validated document points at its journal entry.
    let (orphans,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM sales s
        WHERE s.fiscal_register_id = $1 AND s.locked_at IS NOT NULL
          AND NOT EXISTS (
              SELECT 1 FROM fiscal_journal j
              WHERE j.register_id = s.fiscal_register_id AND j.seq = s.fiscal_seq
                AND j.document_ref = s.reference)
        "#,
    )
    .bind(register_id)
    .fetch_one(db)
    .await?;
    if orphans > 0 {
        findings.push(format!(
            "{orphans} document(s) validé(s) n'ont plus d'entrée correspondante dans le journal."
        ));
    }

    // Every closure points at the entry that sealed it, unchanged.
    let (broken,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM fiscal_closures c
        WHERE c.register_id = $1
          AND NOT EXISTS (
              SELECT 1 FROM fiscal_journal j
              WHERE j.register_id = c.register_id AND j.seq = c.journal_seq
                AND j.chain_hash = c.chain_hash)
        "#,
    )
    .bind(register_id)
    .fetch_one(db)
    .await?;
    if broken > 0 {
        findings.push(format!(
            "{broken} clôture(s) ne correspondent plus à l'entrée du journal qui les a scellées."
        ));
    }
    Ok(findings)
}

/// Settings that change what an invoice says or how it is taxed. Changing one is
/// an event the journal has to carry.
pub fn is_fiscal_setting(key: &str) -> bool {
    key.starts_with("legal.")
        || key.starts_with("invoice.")
        || key.starts_with("vat.")
        || key.starts_with("billing.")
        || matches!(
            key,
            "company.name" | "company.legal_name" | "company.address" | "company.postal_code" | "company.city"
        )
}

/// Seal an event that has no document behind it (a verdict, a setting change, a
/// purge) into the register's chain and queue it for replication.
pub async fn journal_event(
    state: &SharedState,
    event: FiscalEventType,
    payload: Value,
) -> LocalResult<FiscalEntry> {
    let register = current(state).await?;
    let actor = current_actor(state).await;
    let device_id = state.device_id().await.map_err(LocalError::Internal)?;
    let links = EntryLinks {
        actor_id: actor,
        ..Default::default()
    };
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let entry = append(&mut tx, &register, event, Utc::now(), payload, links.clone()).await?;
    tx.commit().await?;
    replicate(&state.db, device_id, &register, &entry, &links).await?;
    Ok(entry)
}

/// Read a register's entries in sequence order, for verification and archiving.
pub async fn load_entries(
    pool: &SqlitePool,
    register_id: &str,
    from_seq: i64,
    to_seq: i64,
) -> LocalResult<Vec<FiscalEntry>> {
    // The chain hashes the register *code*, so verification has to see the code
    // rather than the row's foreign key.
    let rows: Vec<JournalRow> = sqlx::query_as(
        r#"
        SELECT r.code AS register_code, j.seq, j.event_type, j.occurred_at, j.payload,
               j.payload_hash, j.prev_hash, j.chain_hash, j.signature
        FROM fiscal_journal j
        JOIN fiscal_registers r ON r.id = j.register_id
        WHERE j.register_id = $1 AND j.seq >= $2 AND j.seq <= $3
        ORDER BY j.seq ASC
        "#,
    )
    .bind(register_id)
    .bind(from_seq)
    .bind(to_seq)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(JournalRow::into_entry).collect())
}

#[derive(sqlx::FromRow)]
struct JournalRow {
    register_code: String,
    seq: i64,
    event_type: String,
    occurred_at: String,
    payload: String,
    payload_hash: String,
    prev_hash: String,
    chain_hash: String,
    signature: String,
}

impl JournalRow {
    fn into_entry(self) -> FiscalEntry {
        FiscalEntry {
            register_id: self.register_code,
            seq: self.seq,
            event_type: self.event_type,
            occurred_at: self.occurred_at,
            payload: serde_json::from_str(&self.payload).unwrap_or(Value::Null),
            payload_hash: self.payload_hash,
            prev_hash: self.prev_hash,
            chain_hash: self.chain_hash,
            signature: self.signature,
        }
    }
}

/// The one timestamp format the journal uses, re-exported so routes building
/// payloads stamp them the same way the chain does.
pub fn format_stamp(at: DateTime<Utc>) -> String {
    format_timestamp(at)
}

/// Entries that change what was sold or collected, as opposed to ones that only
/// record that something happened (a printout, a drawer opening, a closure).
fn moves_money(event: FiscalEventType) -> bool {
    matches!(
        event,
        FiscalEventType::Invoice
            | FiscalEventType::Receipt
            | FiscalEventType::CreditNote
            | FiscalEventType::Payment
            | FiscalEventType::PaymentReversal
    )
}

/// The shop's calendar day, not UTC's: a sale at 00:30 local belongs to that
/// day's closure.
pub fn business_day(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).date_naive().to_string()
}

fn business_day_str(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|d| business_day(d.with_timezone(&Utc)))
        .unwrap_or_default()
}

pub fn current_year() -> i32 {
    Local::now().year()
}

fn merge_object(base: Value, extra: Value) -> Value {
    match (base, extra) {
        (Value::Object(mut a), Value::Object(b)) => {
            a.extend(b);
            Value::Object(a)
        }
        (base, _) => base,
    }
}

/// The signed-in user, for the `actor_id` on journal entries.
pub async fn current_actor(state: &SharedState) -> Option<String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT user_json FROM cached_session WHERE id = 1")
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let user: Value = serde_json::from_str(&row?.0).ok()?;
    user.get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}
