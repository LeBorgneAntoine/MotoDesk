//! Repairing a register whose signing key is gone.
//!
//! A register's journal is signed with a key kept in the OS credential store. If the
//! store is emptied, or the database is copied to another computer, the key is gone
//! and every fiscal write is refused (it would otherwise be signed with a key nobody
//! can verify against the public key on record). Older versions did not refuse: they
//! quietly made a new key, so some registers hold entries signed by two keys while
//! claiming one.
//!
//! Neither situation touches what the journal proves. Its hash chain is intact; only
//! its signatures lack a key. So the repair never rewrites anything: it appends one
//! entry, `register_key_changed`, signed with the key that now signs, which says from
//! which entry on it does. The register keeps its code, its chain, its totals and its
//! numbering. An auditor reads the declaration in the journal like any other entry.
//!
//! What cannot be repaired is refused rather than papered over: a chain whose hashes
//! do not link (that is tampering, not a lost key), and signatures that no key
//! available here can account for.
//!
//! A register is healthy when the till can sign for it, and the till signs with the
//! key at hand that the journal names, wherever it is kept (`keys::named_by`, the same
//! choice `fiscal::current` makes). A key kept here that the journal does not name --
//! one a refused repair left in the keychain, or the key of the data a restore
//! replaced -- is neither a fault nor a key: it is listed, signs nothing, and nothing
//! removes it.

use std::sync::Arc;

use chrono::Utc;
use motodesk_fiscal::chain::{declared_unverifiable_ranges, key_at, verify_chain, ChainTip};
use motodesk_fiscal::{FiscalEventType, SecretKey};
use serde::Serialize;
use serde_json::json;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::keys::{self, KeyStore, SigningKeychain, SystemKeychain};
use super::{append, key_periods_of, load_entries, replicate, EntryLinks, Register};
use crate::local_server::error::{LocalError, LocalResult};

/// Where a secret key was found.
pub type Source = KeyStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Verdict {
    /// The chain verifies and a key that can sign for it is at hand.
    Healthy,
    /// The chain verifies, but no key at hand can sign for it. Nothing was altered;
    /// the key is simply lost.
    KeyLost,
    /// Entries from `from_seq` on were signed by a key other than the one on record,
    /// and that key is at hand. (A replacement an older version made on its own.)
    TailSignedByOtherKey { from_seq: i64, public_key: String },
    /// Entries `from_seq` to `to_seq` fail their signature and no key at hand explains
    /// them. Their hashes still chain, but the signatures cannot be accounted for.
    TailUnverifiable { from_seq: i64, to_seq: i64 },
    /// The hash chain itself does not link. This is not a lost key.
    ChainBroken { seq: i64, detail: String },
}

#[derive(Debug, Serialize)]
pub struct Diagnosis {
    pub register_code: String,
    pub register_id: String,
    pub recorded_public_key: String,
    pub entries: i64,
    pub last_seq: i64,
    /// The keys found at hand, by where they were found (public halves only).
    pub keys_at_hand: Vec<(Source, String)>,
    /// Where the key the till signs with is kept: the key at hand that the journal
    /// names. `None` when no key at hand is that one, whatever else is kept here.
    pub signing_key_in: Option<Source>,
    pub verdict: Verdict,
}

#[derive(Debug, Serialize)]
pub struct RepairReport {
    pub register_code: String,
    pub before: Verdict,
    pub new_public_key: String,
    pub effective_from_seq: i64,
    pub entry_seq: i64,
    pub key_store: String,
}

fn bad(msg: impl Into<String>) -> LocalError {
    LocalError::BadRequest(msg.into())
}

struct Loaded {
    id: String,
    code: String,
    recorded_key: String,
    company_id: Option<String>,
    entries: Vec<motodesk_fiscal::FiscalEntry>,
    at_hand: Vec<(Source, SecretKey)>,
}

async fn load(
    keychain: &dyn SigningKeychain,
    pool: &SqlitePool,
    code: Option<&str>,
) -> LocalResult<Vec<Loaded>> {
    let registers: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT r.id, r.code, r.public_key, r.company_id
           FROM fiscal_registers r JOIN fiscal_counters c ON c.register_id = r.id
          WHERE $1 IS NULL OR r.code = $1 ORDER BY r.code",
    )
    .bind(code)
    .fetch_all(pool)
    .await?;
    if registers.is_empty() {
        return Err(bad(match code {
            Some(c) => format!("aucune caisse « {c} » dans cette base"),
            None => "aucune caisse dans cette base".to_string(),
        }));
    }
    let mut out = Vec::new();
    for (id, code, recorded_key, company_id) in registers {
        let entries = load_entries(pool, &id, 0, i64::MAX).await?;
        let at_hand = keys::at_hand(keychain, pool, &code).await?;
        out.push(Loaded {
            id,
            code,
            recorded_key,
            company_id,
            entries,
            at_hand,
        });
    }
    Ok(out)
}

/// The public key the journal names for what is signed now: its last declaration, or
/// the key recorded for the register when the chain declares none.
fn named_key(l: &Loaded) -> Option<String> {
    key_periods_of(&l.entries, &l.recorded_key).last().map(|(_, k)| k.to_hex())
}

/// Where the key the till signs with is kept, if it is at hand.
fn signing_key_in(l: &Loaded) -> Option<Source> {
    let named = named_key(l)?;
    keys::named_by(&named, &l.at_hand).map(|(store, _)| *store)
}

fn judge(l: &Loaded) -> Verdict {
    // The hashes first, with no key involved: a chain that does not link is not a lost
    // key, whatever its signatures say.
    if let Err(broken) = verify_chain(&l.entries, &ChainTip::genesis(), None) {
        return Verdict::ChainBroken {
            seq: broken.seq(),
            detail: format!("{broken:?}"),
        };
    }

    let periods = key_periods_of(&l.entries, &l.recorded_key);
    let declared = declared_unverifiable_ranges(&l.entries);
    let covered = |seq: i64| declared.iter().any(|(from, to)| (*from..=*to).contains(&seq));
    let failing: Vec<i64> = l
        .entries
        .iter()
        .filter(|e| !e.signature.is_empty() && !covered(e.seq))
        .filter(|e| key_at(&periods, e.seq).is_some_and(|k| k.verify_entry(e).is_err()))
        .map(|e| e.seq)
        .collect();

    let (Some(&first), Some(&last)) = (failing.first(), failing.last()) else {
        // Healthy means the till can sign: the same choice of key as `fiscal::current`.
        return if signing_key_in(l).is_some() { Verdict::Healthy } else { Verdict::KeyLost };
    };

    // Does a key at hand account for every entry from the first failure on?
    let tail: Vec<&motodesk_fiscal::FiscalEntry> =
        l.entries.iter().filter(|e| e.seq >= first && !covered(e.seq)).collect();
    for (_, secret) in &l.at_hand {
        let public = secret.public();
        if tail.iter().all(|e| public.verify_entry(e).is_ok()) {
            return Verdict::TailSignedByOtherKey {
                from_seq: first,
                public_key: public.to_hex(),
            };
        }
    }
    Verdict::TailUnverifiable {
        from_seq: first,
        to_seq: last,
    }
}

/// What state each register is in. Read-only.
pub async fn diagnose(pool: &SqlitePool, code: Option<&str>) -> LocalResult<Vec<Diagnosis>> {
    diagnose_with(&SystemKeychain, pool, code).await
}

/// [`diagnose`], with the keychain given by the caller (tests give one of their own).
pub(crate) async fn diagnose_with(
    keychain: &dyn SigningKeychain,
    pool: &SqlitePool,
    code: Option<&str>,
) -> LocalResult<Vec<Diagnosis>> {
    Ok(load(keychain, pool, code)
        .await?
        .into_iter()
        .map(|l| {
            let verdict = judge(&l);
            Diagnosis {
                signing_key_in: signing_key_in(&l),
                entries: l.entries.len() as i64,
                last_seq: l.entries.last().map(|e| e.seq).unwrap_or(0),
                keys_at_hand: l
                    .at_hand
                    .iter()
                    .map(|(s, k)| (*s, k.public().to_hex()))
                    .collect(),
                register_code: l.code,
                register_id: l.id,
                recorded_public_key: l.recorded_key,
                verdict,
            }
        })
        .collect())
}

/// Where a new key is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepKeyIn {
    Keychain,
    Database,
}

/// Declare the key that signs from now on (or from `from_seq` for entries already
/// signed by it), in the journal. Returns what was done; refuses what cannot be
/// repaired.
///
/// `accept_unverifiable` is the owner's explicit consent to the one repair that is a
/// statement rather than a fix: entries whose signatures no key can account for are
/// declared, in the journal, as no longer verifiable. Their hashes still chain; the
/// register just stops claiming it can prove who signed them.
pub async fn repair(
    pool: &SqlitePool,
    code: &str,
    keep_new_key_in: KeepKeyIn,
    accept_unverifiable: bool,
) -> LocalResult<RepairReport> {
    repair_with(&SystemKeychain, pool, code, keep_new_key_in, accept_unverifiable).await
}

/// [`repair`], with the keychain given by the caller (tests give one of their own).
pub(crate) async fn repair_with(
    keychain: &dyn SigningKeychain,
    pool: &SqlitePool,
    code: &str,
    keep_new_key_in: KeepKeyIn,
    accept_unverifiable: bool,
) -> LocalResult<RepairReport> {
    let l = load(keychain, pool, Some(code))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| bad("caisse introuvable"))?;
    let before = judge(&l);

    // The key that will sign the declaration, where it lives, and from which entry
    // the register's signatures are to be read with it.
    // A fresh key is only made here. It is kept (where the caller asked) further down,
    // once its declaration is written and about to be committed: a key stored first
    // would go over the one already kept under the register's name, which may be the
    // very key the journal still names, before the journal says another one signs.
    let fresh_key = || {
        let store = match keep_new_key_in {
            KeepKeyIn::Keychain => Source::Keychain,
            KeepKeyIn::Database => Source::Database,
        };
        (SecretKey::generate(), store)
    };
    let mut to_keep: Option<String> = None;
    let mut unverifiable: Option<(i64, i64)> = None;

    let (secret, store, effective_from, reason): (SecretKey, Source, Option<i64>, &str) = match &before {
        Verdict::Healthy => return Err(bad(format!("la caisse {code} est saine : rien à réparer"))),
        Verdict::ChainBroken { seq, .. } => {
            return Err(bad(format!(
                "la chaîne de la caisse {code} est rompue à l'entrée {seq} : ce n'est pas une clé perdue, c'est une altération du journal. Aucune réparation n'est faite."
            )))
        }
        Verdict::TailUnverifiable { from_seq, to_seq } if !accept_unverifiable => {
            return Err(bad(format!(
                "les signatures de la caisse {code} des entrées {from_seq} à {to_seq} ne correspondent à aucune clé disponible ici. Retrouvez la clé qui les a produites (autre ordinateur, ancien trousseau) ; sinon, relancez avec --accept-unverifiable pour que le journal déclare ces signatures invérifiables (leurs empreintes restent chaînées)."
            )))
        }
        Verdict::TailUnverifiable { from_seq, to_seq } => {
            let (key, src) = fresh_key();
            to_keep = Some(key.to_hex());
            unverifiable = Some((*from_seq, *to_seq));
            (key, src, None, "signatures_unverifiable")
        }
        Verdict::TailSignedByOtherKey { from_seq, public_key } => {
            let (src, key) = l
                .at_hand
                .iter()
                .find(|(_, k)| &k.public().to_hex() == public_key)
                .ok_or_else(|| bad("la clé du journal a disparu entre le diagnostic et la réparation"))?;
            (SecretKey::from_hex(&key.to_hex()).map_err(|e| LocalError::Internal(e.into()))?, *src, Some(*from_seq), "signed_with_replacement_key")
        }
        Verdict::KeyLost => {
            let (key, src) = fresh_key();
            to_keep = Some(key.to_hex());
            (key, src, None, "key_lost")
        }
    };

    let public_key = secret.public().to_hex();
    let device_id: (String,) = sqlx::query_as("SELECT device_id FROM sync_state WHERE id = 1")
        .fetch_one(pool)
        .await?;
    let device_id = Uuid::parse_str(&device_id.0).map_err(|e| LocalError::Internal(e.into()))?;

    let register = Register {
        id: Uuid::parse_str(&l.id).map_err(|e| LocalError::Internal(e.into()))?,
        code: l.code.clone(),
        company_id: l.company_id.clone(),
        key: Arc::new(secret),
        key_store: store,
    };

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (tip_seq,): (i64,) = sqlx::query_as("SELECT last_seq FROM fiscal_counters WHERE register_id = $1")
        .bind(&l.id)
        .fetch_one(&mut *tx)
        .await?;
    let entry_seq = tip_seq + 1;
    let effective = effective_from.unwrap_or(entry_seq);
    let entry = append(
        &mut tx,
        &register,
        FiscalEventType::RegisterKeyChanged,
        Utc::now(),
        {
            let mut payload = json!({
                "register_code": l.code,
                "previous_public_key": l.recorded_key,
                "public_key": public_key,
                "effective_from_seq": effective,
                "reason": reason,
                "key_store": store.as_str(),
            });
            if let Some((from, to)) = unverifiable {
                payload["unverifiable_from_seq"] = json!(from);
                payload["unverifiable_to_seq"] = json!(to);
            }
            payload
        },
        EntryLinks::default(),
    )
    .await?;
    sqlx::query("UPDATE fiscal_registers SET public_key = $2, key_store = $3 WHERE id = $1")
        .bind(&l.id)
        .bind(&public_key)
        .bind(store.as_str())
        .execute(&mut *tx)
        .await?;
    // The declaration is written, not yet committed: now the new key is kept. Any
    // failure from here on leaves the transaction to be rolled back, the journal as it
    // was.
    if let Some(hex) = &to_keep {
        match store {
            // In the same transaction: the key and the entry that names it land together.
            Source::Database => {
                sqlx::query(
                    "INSERT INTO fiscal_key_fallback (register_code, secret_key, created_at) VALUES ($1, $2, $3)
                     ON CONFLICT(register_code) DO UPDATE SET secret_key = excluded.secret_key, created_at = excluded.created_at",
                )
                .bind(&l.code)
                .bind(hex)
                .bind(Utc::now().to_rfc3339())
                .execute(&mut *tx)
                .await?;
            }
            // A key the keychain will not vouch for is not declared. If the keychain kept
            // it all the same (Windows can show a write after having answered that it was
            // not kept), it stays there undeclared: no journal names it, so no till signs
            // with it, and a later repair is free to declare another key.
            Source::Keychain => keychain.store(&l.code, hex).map_err(|e| {
                bad(format!(
                    "le trousseau du système refuse la nouvelle clé : {e:#}. Rien n'a été écrit dans le journal ; \
                     si le trousseau a malgré tout gardé cette clé, elle n'est déclarée nulle part et ne signe rien."
                ))
            })?,
        }
    }
    tx.commit().await?;

    // The journal is replicated: the declaration has to reach the other devices and
    // the cloud, or the entries signed with the new key would fail there.
    replicate(pool, device_id, &register, &entry, &EntryLinks::default()).await?;

    Ok(RepairReport {
        register_code: l.code,
        before,
        new_public_key: public_key,
        effective_from_seq: effective,
        entry_seq: entry.seq,
        key_store: store.as_str().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::fiscal;
    use crate::local_server::test_support::{shop, MemoryKeychain, OnStore};
    use motodesk_fiscal::FiscalEventType as Ev;

    async fn note(pool: &SqlitePool, register: &Register) {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        append(
            &mut tx,
            register,
            Ev::IntegrityChecked,
            Utc::now(),
            json!({ "ok": true }),
            EntryLinks::default(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    fn code_of(d: &[Diagnosis]) -> &Verdict {
        &d[0].verdict
    }

    async fn ready() -> (crate::local_server::state::SharedState, Register) {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        note(&state.db, &register).await;
        note(&state.db, &register).await;
        (state, register)
    }

    #[tokio::test]
    async fn a_lost_key_is_replaced_by_a_declared_one_and_the_chain_carries_on() {
        let (state, register) = ready().await;
        let before = fiscal::verify_all(&state).await.unwrap();
        assert!(before.registers[0].ok);

        // The credential store is emptied: nothing can sign any more.
        sqlx::query("DELETE FROM fiscal_key_fallback").execute(&state.db).await.unwrap();
        assert!(fiscal::current(&state).await.is_err(), "fiscal writes are blocked");
        let d = diagnose(&state.db, None).await.unwrap();
        assert_eq!(code_of(&d), &Verdict::KeyLost);
        assert!(d[0].keys_at_hand.is_empty());

        let report = repair(&state.db, &register.code, KeepKeyIn::Database, false).await.unwrap();
        assert_eq!(report.before, Verdict::KeyLost);
        assert_eq!(report.effective_from_seq, report.entry_seq, "the new key signs from the declaration on");

        // The register works again, under the new key, and the whole chain verifies:
        // the early entries against the old key, the rest against the new one.
        let register = fiscal::current(&state).await.unwrap();
        assert_eq!(register.public_key_hex(), report.new_public_key);
        note(&state.db, &register).await;
        let after = fiscal::verify_all(&state).await.unwrap();
        assert!(after.registers[0].ok, "{:?}", after.registers[0].problem);
        assert_eq!(after.registers[0].last_seq, report.entry_seq + 1);
        assert_eq!(code_of(&diagnose(&state.db, None).await.unwrap()), &Verdict::Healthy);

        // The declaration is in the journal, to be replicated.
        let queued: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM change_log WHERE entity_type = 'fiscal_entry' AND payload LIKE '%register_key_changed%'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(queued, 1);

        // Nothing is repaired twice.
        assert!(repair(&state.db, &register.code, KeepKeyIn::Database, false).await.is_err());
    }

    /// An older version replaced a lost key without saying so: entries after the loss
    /// are signed by a key the register does not name.
    #[tokio::test]
    async fn a_key_an_older_version_replaced_silently_is_declared_from_where_it_began() {
        let (state, register) = ready().await;
        let replacement = SecretKey::generate();
        let impostor = Register {
            id: register.id,
            code: register.code.clone(),
            company_id: register.company_id.clone(),
            key: Arc::new(SecretKey::from_hex(&replacement.to_hex()).unwrap()),
            key_store: super::super::keys::KeyStore::Database,
        };
        note(&state.db, &impostor).await;
        note(&state.db, &impostor).await;
        // The replacement is what the credential store holds now.
        sqlx::query("UPDATE fiscal_key_fallback SET secret_key = $1")
            .bind(replacement.to_hex())
            .execute(&state.db)
            .await
            .unwrap();

        assert!(!fiscal::verify_all(&state).await.unwrap().registers[0].ok, "the break shows");
        let d = diagnose(&state.db, None).await.unwrap();
        // Entries: 1 opened, 2-3 by the original key, 4-5 by the replacement.
        assert_eq!(
            code_of(&d),
            &Verdict::TailSignedByOtherKey { from_seq: 4, public_key: replacement.public().to_hex() }
        );

        let report = repair(&state.db, &register.code, KeepKeyIn::Database, false).await.unwrap();
        assert_eq!(report.effective_from_seq, 4, "declared from the first entry it signed");
        assert_eq!(report.new_public_key, replacement.public().to_hex());

        let after = fiscal::verify_all(&state).await.unwrap();
        assert!(after.registers[0].ok, "{:?}", after.registers[0].problem);
        let register = fiscal::current(&state).await.unwrap();
        note(&state.db, &register).await;
        assert!(fiscal::verify_all(&state).await.unwrap().registers[0].ok);
    }

    #[tokio::test]
    async fn signatures_no_key_can_account_for_are_not_papered_over() {
        let (state, register) = ready().await;
        let stranger = Register {
            id: register.id,
            code: register.code.clone(),
            company_id: register.company_id.clone(),
            key: Arc::new(SecretKey::generate()),
            key_store: super::super::keys::KeyStore::Database,
        };
        note(&state.db, &stranger).await;
        let d = diagnose(&state.db, None).await.unwrap();
        assert!(
            matches!(code_of(&d), Verdict::TailUnverifiable { from_seq: 4, to_seq: 4 }),
            "{:?}",
            code_of(&d)
        );
        let refused = repair(&state.db, &register.code, KeepKeyIn::Database, false).await.unwrap_err();
        assert!(matches!(&refused, LocalError::BadRequest(m) if m.contains("ne correspondent")), "{refused:?}");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'register_key_changed'")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            0,
            "nothing was written without consent"
        );

        // With the owner's explicit consent the journal declares those signatures
        // unverifiable, and the register goes on under a new key.
        let report = repair(&state.db, &register.code, KeepKeyIn::Database, true).await.unwrap();
        assert_eq!(report.effective_from_seq, report.entry_seq);
        let after = fiscal::verify_all(&state).await.unwrap();
        assert!(after.registers[0].ok, "{:?}", after.registers[0].problem);
        assert_eq!(after.registers[0].declared_unverifiable, 1, "and the check says so");
        let register = fiscal::current(&state).await.unwrap();
        note(&state.db, &register).await;
        assert!(fiscal::verify_all(&state).await.unwrap().registers[0].ok);
        assert_eq!(code_of(&diagnose(&state.db, None).await.unwrap()), &Verdict::Healthy);
    }

    /// The declaration covers what existed when it was written: it cannot excuse a bad
    /// signature on an entry that comes after it.
    #[tokio::test]
    async fn a_declaration_does_not_excuse_later_entries() {
        let (state, register) = ready().await;
        let stranger = Register {
            id: register.id,
            code: register.code.clone(),
            company_id: register.company_id.clone(),
            key: Arc::new(SecretKey::generate()),
            key_store: super::super::keys::KeyStore::Database,
        };
        note(&state.db, &stranger).await;
        repair(&state.db, &register.code, KeepKeyIn::Database, true).await.unwrap();
        // Someone signs a later entry with yet another key.
        note(&state.db, &stranger).await;
        let after = fiscal::verify_all(&state).await.unwrap();
        assert!(!after.registers[0].ok, "the later entry is still caught");
    }

    #[tokio::test]
    async fn a_tampered_chain_is_not_a_lost_key_and_is_not_repaired() {
        let (state, register) = ready().await;
        sqlx::query("DROP TRIGGER fiscal_journal_no_update").execute(&state.db).await.unwrap();
        sqlx::query("UPDATE fiscal_journal SET payload = '{\"ok\":false}' WHERE seq = 2")
            .execute(&state.db)
            .await
            .unwrap();
        let d = diagnose(&state.db, None).await.unwrap();
        assert!(matches!(code_of(&d), Verdict::ChainBroken { seq: 2, .. }), "{:?}", code_of(&d));
        let refused = repair(&state.db, &register.code, KeepKeyIn::Database, false).await.unwrap_err();
        assert!(matches!(&refused, LocalError::BadRequest(m) if m.contains("altération")), "{refused:?}");
    }

    async fn key_changes(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'register_key_changed'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn message(e: &LocalError) -> &str {
        match e {
            LocalError::BadRequest(m) => m,
            _ => "",
        }
    }

    /// The keychain holds a key the journal does not name, the database holds the one it
    /// does. The till signs with the named key, the diagnosis says the register is healthy
    /// and where its key is, the repair has nothing to do: the three say the same thing.
    #[tokio::test]
    async fn a_till_signs_with_the_key_its_journal_names_when_the_keychain_holds_another() {
        let (state, register) = ready().await;
        let stray = SecretKey::generate();
        let keychain = MemoryKeychain::new();
        keychain.put(&register.code, &stray.to_hex());

        let till = fiscal::current_with(&state, &keychain).await.unwrap();
        assert_eq!(till.public_key_hex(), register.public_key_hex(), "the key on record, not the first found");
        note(&state.db, &till).await;
        let checked = fiscal::verify_all(&state).await.unwrap();
        assert!(checked.registers[0].ok, "{:?}", checked.registers[0].problem);

        let d = diagnose_with(&keychain, &state.db, None).await.unwrap();
        assert_eq!(code_of(&d), &Verdict::Healthy);
        assert_eq!(d[0].signing_key_in, Some(Source::Database));
        assert_eq!(
            d[0].keys_at_hand,
            vec![
                (Source::Keychain, stray.public().to_hex()),
                (Source::Database, register.public_key_hex()),
            ],
            "the other key is still shown"
        );

        let refused = repair_with(&keychain, &state.db, &register.code, KeepKeyIn::Keychain, false)
            .await
            .unwrap_err();
        assert!(message(&refused).contains("rien à réparer"), "{refused:?}");
        assert_eq!(key_changes(&state.db).await, 0);
        assert_eq!(keychain.get(&register.code), Some(stray.to_hex()), "and nothing touched it");
    }

    /// The situation as it arises. A repair is asked to keep the new key in the keychain;
    /// the keychain answers that it did not keep it, and keeps it. The repair is refused
    /// with nothing declared. The operator then repairs into the database.
    #[tokio::test]
    async fn a_repair_the_keychain_refused_yet_kept_does_not_block_the_next_one() {
        let (state, register) = ready().await;
        sqlx::query("DELETE FROM fiscal_key_fallback").execute(&state.db).await.unwrap();
        let keychain = MemoryKeychain::that(OnStore::KeepsButSaysNo);

        let refused = repair_with(&keychain, &state.db, &register.code, KeepKeyIn::Keychain, false)
            .await
            .unwrap_err();
        assert!(message(&refused).contains("refuse la nouvelle clé"), "{refused:?}");
        assert_eq!(key_changes(&state.db).await, 0, "a refused repair declares nothing");
        let left = keychain.get(&register.code).expect("the keychain kept the key it said it refused");
        let left_public = SecretKey::from_hex(&left).unwrap().public().to_hex();

        // A key is at hand and no journal names it: the register's key is still lost, and
        // the till does not sign with what happens to be there.
        let d = diagnose_with(&keychain, &state.db, None).await.unwrap();
        assert_eq!(code_of(&d), &Verdict::KeyLost);
        assert_eq!(d[0].signing_key_in, None);
        assert_eq!(d[0].keys_at_hand, vec![(Source::Keychain, left_public.clone())]);
        let blocked = fiscal::current_with(&state, &keychain).await.err().unwrap();
        assert!(message(&blocked).contains("ne correspond pas"), "{blocked:?}");

        let report = repair_with(&keychain, &state.db, &register.code, KeepKeyIn::Database, false)
            .await
            .unwrap();
        assert_eq!(report.key_store, "database");
        assert_ne!(report.new_public_key, left_public);

        // The till signs again, with the key the journal now names, although the keychain
        // is read first and still holds the other one.
        let till = fiscal::current_with(&state, &keychain).await.unwrap();
        assert_eq!(till.public_key_hex(), report.new_public_key);
        note(&state.db, &till).await;
        let checked = fiscal::verify_all(&state).await.unwrap();
        assert!(checked.registers[0].ok, "{:?}", checked.registers[0].problem);

        let d = diagnose_with(&keychain, &state.db, None).await.unwrap();
        assert_eq!(code_of(&d), &Verdict::Healthy);
        assert_eq!(d[0].signing_key_in, Some(Source::Database));
        let nothing = repair_with(&keychain, &state.db, &register.code, KeepKeyIn::Database, false)
            .await
            .unwrap_err();
        assert!(message(&nothing).contains("rien à réparer"), "{nothing:?}");
        assert_eq!(key_changes(&state.db).await, 1, "one declaration, appended; nothing rewritten");
        assert_eq!(keychain.get(&register.code), Some(left), "the undeclared key was neither removed nor replaced");
    }

    /// The other way round: the journal names the key kept in the keychain, and the
    /// database holds another. When the named key goes, the other does not stand in.
    #[tokio::test]
    async fn a_key_left_in_the_database_never_stands_in_for_the_one_the_journal_names() {
        let state = shop().await;
        let keychain = MemoryKeychain::new();
        let register = fiscal::current_with(&state, &keychain).await.unwrap();
        assert_eq!(register.key_store, Source::Keychain);
        note(&state.db, &register).await;
        let stray = SecretKey::generate();
        sqlx::query("INSERT INTO fiscal_key_fallback (register_code, secret_key, created_at) VALUES ($1, $2, $3)")
            .bind(&register.code)
            .bind(stray.to_hex())
            .bind(Utc::now().to_rfc3339())
            .execute(&state.db)
            .await
            .unwrap();

        let till = fiscal::current_with(&state, &keychain).await.unwrap();
        assert_eq!(till.public_key_hex(), register.public_key_hex());
        let d = diagnose_with(&keychain, &state.db, None).await.unwrap();
        assert_eq!(code_of(&d), &Verdict::Healthy);
        assert_eq!(d[0].signing_key_in, Some(Source::Keychain));

        keychain.remove(&register.code);
        let blocked = fiscal::current_with(&state, &keychain).await.err().unwrap();
        assert!(message(&blocked).contains("ne correspond pas"), "{blocked:?}");
        let d = diagnose_with(&keychain, &state.db, None).await.unwrap();
        assert_eq!(code_of(&d), &Verdict::KeyLost);
        assert_eq!(d[0].signing_key_in, None);
    }

    #[tokio::test]
    async fn a_new_key_kept_in_the_keychain_signs_from_its_declaration_on() {
        let (state, register) = ready().await;
        sqlx::query("DELETE FROM fiscal_key_fallback").execute(&state.db).await.unwrap();
        let keychain = MemoryKeychain::new();

        let report = repair_with(&keychain, &state.db, &register.code, KeepKeyIn::Keychain, false)
            .await
            .unwrap();
        assert_eq!(report.key_store, "keychain");
        let kept = keychain.get(&register.code).expect("the key is in the keychain");
        assert_eq!(SecretKey::from_hex(&kept).unwrap().public().to_hex(), report.new_public_key);
        let in_database: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fiscal_key_fallback")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(in_database, 0, "and nowhere else");

        let till = fiscal::current_with(&state, &keychain).await.unwrap();
        assert_eq!(till.public_key_hex(), report.new_public_key);
        assert_eq!(till.key_store, Source::Keychain);
        note(&state.db, &till).await;
        let checked = fiscal::verify_all(&state).await.unwrap();
        assert!(checked.registers[0].ok, "{:?}", checked.registers[0].problem);
        let d = diagnose_with(&keychain, &state.db, None).await.unwrap();
        assert_eq!(code_of(&d), &Verdict::Healthy);
        assert_eq!(d[0].signing_key_in, Some(Source::Keychain));
    }

    /// With the keychain opted out of (as every test run is), a repair asked to use it is
    /// refused: it must not reach the operating system's store behind the opt-out.
    #[tokio::test]
    async fn a_repair_does_not_reach_a_keychain_that_is_opted_out_of() {
        let (state, register) = ready().await;
        sqlx::query("DELETE FROM fiscal_key_fallback").execute(&state.db).await.unwrap();
        let refused = repair(&state.db, &register.code, KeepKeyIn::Keychain, false).await.unwrap_err();
        assert!(message(&refused).contains("refuse la nouvelle clé"), "{refused:?}");
        assert_eq!(key_changes(&state.db).await, 0);
    }
}
