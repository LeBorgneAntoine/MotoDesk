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

use std::sync::Arc;

use chrono::Utc;
use motodesk_fiscal::chain::{declared_unverifiable_ranges, key_at, verify_chain, ChainTip};
use motodesk_fiscal::{FiscalEventType, SecretKey};
use serde::Serialize;
use serde_json::json;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{append, key_periods_of, load_entries, replicate, EntryLinks, Register};
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::secrets;

/// Where a secret key was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Keychain,
    Database,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Keychain => "keychain",
            Source::Database => "database",
        }
    }
}

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

/// Secret keys available for a register: the credential store (unless opted out) and
/// the fallback table.
async fn secrets_at_hand(pool: &SqlitePool, code: &str) -> LocalResult<Vec<(Source, SecretKey)>> {
    let mut found = Vec::new();
    if !matches!(
        std::env::var("MOTODESK_FISCAL_NO_KEYCHAIN").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    ) {
        if let Ok(Some(hex)) = secrets::load_fiscal_key(code) {
            if let Ok(key) = SecretKey::from_hex(&hex) {
                found.push((Source::Keychain, key));
            }
        }
    }
    let row: Option<(String,)> =
        sqlx::query_as("SELECT secret_key FROM fiscal_key_fallback WHERE register_code = $1")
            .bind(code)
            .fetch_optional(pool)
            .await?;
    if let Some((hex,)) = row {
        if let Ok(key) = SecretKey::from_hex(&hex) {
            found.push((Source::Database, key));
        }
    }
    Ok(found)
}

struct Loaded {
    id: String,
    code: String,
    recorded_key: String,
    company_id: Option<String>,
    entries: Vec<motodesk_fiscal::FiscalEntry>,
    at_hand: Vec<(Source, SecretKey)>,
}

async fn load(pool: &SqlitePool, code: Option<&str>) -> LocalResult<Vec<Loaded>> {
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
        let at_hand = secrets_at_hand(pool, &code).await?;
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
        let current = periods.last().map(|(_, k)| k.to_hex());
        let signs = l
            .at_hand
            .iter()
            .any(|(_, k)| Some(k.public().to_hex()) == current);
        return if signs { Verdict::Healthy } else { Verdict::KeyLost };
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
    Ok(load(pool, code)
        .await?
        .into_iter()
        .map(|l| {
            let verdict = judge(&l);
            Diagnosis {
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
    let l = load(pool, Some(code))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| bad("caisse introuvable"))?;
    let before = judge(&l);

    // The key that will sign the declaration, where it lives, and from which entry
    // the register's signatures are to be read with it.
    // A fresh key, kept where the caller asked.
    let fresh_key = |pool: &SqlitePool| {
        let pool = pool.clone();
        let code = code.to_string();
        async move {
            let key = SecretKey::generate();
            let hex = key.to_hex();
            let src = match keep_new_key_in {
                KeepKeyIn::Keychain => {
                    secrets::store_fiscal_key(&code, &hex)
                        .map_err(|e| bad(format!("le trousseau du système refuse la nouvelle clé : {e:#}")))?;
                    Source::Keychain
                }
                KeepKeyIn::Database => {
                    sqlx::query(
                        "INSERT INTO fiscal_key_fallback (register_code, secret_key, created_at) VALUES ($1, $2, $3)
                         ON CONFLICT(register_code) DO UPDATE SET secret_key = excluded.secret_key, created_at = excluded.created_at",
                    )
                    .bind(&code)
                    .bind(&hex)
                    .bind(Utc::now().to_rfc3339())
                    .execute(&pool)
                    .await?;
                    Source::Database
                }
            };
            LocalResult::Ok((key, src))
        }
    };
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
            let (key, src) = fresh_key(pool).await?;
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
            let (key, src) = fresh_key(pool).await?;
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
        key_store: match store {
            Source::Keychain => super::keys::KeyStore::Keychain,
            Source::Database => super::keys::KeyStore::Database,
        },
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
    use crate::local_server::test_support::shop;
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
}
