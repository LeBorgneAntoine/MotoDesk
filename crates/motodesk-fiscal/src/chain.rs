//! Hash chaining for the permanent fiscal journal.
//!
//! BOI-TVA-DECLA-30-10-30 § 120 requires a reliable technique proving that
//! recorded payment data has not changed since it was written, and § 140 names
//! record chaining as an accepted one. Each entry carries the digest of its own
//! payload and the digest of the entry before it, so altering any row
//! invalidates every row after it and the break is locatable.
//!
//! One chain exists per register (per device). Chains are never interleaved,
//! which is what makes offline operation and later replication safe.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::canonical::to_canonical_bytes;
use crate::error::{ChainBreak, FiscalError};

/// `prev_hash` of the first entry in a register's chain.
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Field separator inside the hashed preimage. ASCII record separator cannot
/// occur in any of the fields, so no field can be made to impersonate another.
const SEP: u8 = 0x1e;

/// One row of the permanent fiscal journal, exactly as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiscalEntry {
    pub register_id: String,
    pub seq: i64,
    pub event_type: String,
    /// RFC 3339 UTC, second precision. Always produced by [`format_timestamp`].
    pub occurred_at: String,
    pub payload: Value,
    pub payload_hash: String,
    pub prev_hash: String,
    pub chain_hash: String,
    /// Hex Ed25519 signature over the raw chain digest. Empty when the register
    /// has no key yet, which verification treats as unsigned rather than bad.
    #[serde(default)]
    pub signature: String,
}

/// The tail of a chain, everything needed to append the next entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainTip {
    pub seq: i64,
    pub chain_hash: String,
    pub occurred_at: String,
}

impl ChainTip {
    /// The state of a register that has never recorded anything.
    pub fn genesis() -> Self {
        Self {
            seq: 0,
            chain_hash: GENESIS_HASH.to_string(),
            occurred_at: String::new(),
        }
    }
}

/// Format an instant the one way the journal accepts.
pub fn format_timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// SHA-256 of the canonical form of `payload`, hex encoded.
pub fn hash_payload(payload: &Value) -> Result<String, FiscalError> {
    let bytes = to_canonical_bytes(payload)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// The digest binding an entry to its position, its content and its ancestor.
pub fn compute_chain_hash(
    register_id: &str,
    seq: i64,
    occurred_at: &str,
    event_type: &str,
    payload_hash: &str,
    prev_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    for field in [
        register_id,
        &seq.to_string(),
        occurred_at,
        event_type,
        payload_hash,
        prev_hash,
    ] {
        hasher.update(field.as_bytes());
        hasher.update([SEP]);
    }
    hex::encode(hasher.finalize())
}

/// Build the next entry for a register. The caller persists it and advances the
/// stored tip in the same transaction.
pub fn seal(
    tip: &ChainTip,
    register_id: &str,
    event_type: &str,
    occurred_at: DateTime<Utc>,
    payload: Value,
) -> Result<FiscalEntry, FiscalError> {
    let seq = tip.seq + 1;
    let stamp = format_timestamp(occurred_at);
    let payload_hash = hash_payload(&payload)?;
    let chain_hash = compute_chain_hash(
        register_id,
        seq,
        &stamp,
        event_type,
        &payload_hash,
        &tip.chain_hash,
    );

    Ok(FiscalEntry {
        register_id: register_id.to_string(),
        seq,
        event_type: event_type.to_string(),
        occurred_at: stamp,
        payload,
        payload_hash,
        prev_hash: tip.chain_hash.clone(),
        chain_hash,
        signature: String::new(),
    })
}

impl FiscalEntry {
    /// Whether the digests still match the content: the entry is as it was sealed. Says nothing
    /// of who sealed it; that is the signature's part.
    pub fn is_intact(&self) -> bool {
        hash_payload(&self.payload).is_ok_and(|hash| hash == self.payload_hash)
            && compute_chain_hash(&self.register_id, self.seq, &self.occurred_at, &self.event_type, &self.payload_hash, &self.prev_hash)
                == self.chain_hash
    }

    /// The tip this entry leaves behind.
    pub fn tip(&self) -> ChainTip {
        ChainTip {
            seq: self.seq,
            chain_hash: self.chain_hash.clone(),
            occurred_at: self.occurred_at.clone(),
        }
    }

    /// Raw digest bytes, the message that gets signed.
    pub fn chain_digest_bytes(&self) -> Result<Vec<u8>, FiscalError> {
        hex::decode(&self.chain_hash).map_err(|e| FiscalError::BadDigest(e.to_string()))
    }
}

/// Outcome of walking a whole chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainReport {
    pub register_id: String,
    pub entries_checked: usize,
    pub first_seq: i64,
    pub last_seq: i64,
    pub last_chain_hash: String,
    pub signatures_checked: usize,
    pub unsigned: usize,
}

/// Verify one register's chain from `start_tip` forwards.
///
/// `entries` must be the register's rows in ascending sequence. Pass the
/// register's public key to also verify signatures; without it the structural
/// checks still run.
pub fn verify_chain(
    entries: &[FiscalEntry],
    start_tip: &ChainTip,
    verifier: Option<&crate::signature::PublicKey>,
) -> Result<ChainReport, ChainBreak> {
    verify_chain_with(entries, start_tip, &|_| verifier)
}

/// A register's signing keys over time: from which entry on each one signs, oldest
/// first.
pub type KeyPeriods = Vec<(i64, crate::signature::PublicKey)>;

/// The key that signs entry `seq`: the latest period that has begun by then.
pub fn key_at(periods: &KeyPeriods, seq: i64) -> Option<&crate::signature::PublicKey> {
    periods.iter().rev().find(|(from, _)| *from <= seq).map(|(_, k)| k)
}

/// Verify a chain whose signing key changed during its life (a lost key replaced).
pub fn verify_chain_periods(
    entries: &[FiscalEntry],
    start_tip: &ChainTip,
    periods: &KeyPeriods,
) -> Result<ChainReport, ChainBreak> {
    verify_chain_declared(entries, start_tip, periods, &[])
}

/// Like [`verify_chain_periods`], for a chain that has also declared ranges of entries
/// whose signatures can no longer be verified (the key that made them is lost). Their
/// hashes are still checked and still have to chain; only the signature is not.
pub fn verify_chain_declared(
    entries: &[FiscalEntry],
    start_tip: &ChainTip,
    periods: &KeyPeriods,
    unverifiable: &[(i64, i64)],
) -> Result<ChainReport, ChainBreak> {
    verify_chain_with(entries, start_tip, &|seq| {
        if unverifiable.iter().any(|(from, to)| (*from..=*to).contains(&seq)) {
            None
        } else {
            key_at(periods, seq)
        }
    })
}

/// The ranges of entries a chain declares as no longer verifiable, from its own
/// `register_key_changed` entries. A range can only describe entries before the
/// declaration that makes it: it can never excuse an entry written afterwards.
pub fn declared_unverifiable_ranges(entries: &[FiscalEntry]) -> Vec<(i64, i64)> {
    entries
        .iter()
        .filter(|e| e.event_type == "register_key_changed")
        .filter_map(|e| {
            let from = e.payload.get("unverifiable_from_seq")?.as_i64()?;
            let to = e.payload.get("unverifiable_to_seq")?.as_i64()?;
            (1 <= from && from <= to && to < e.seq).then_some((from, to))
        })
        .collect()
}

/// The keys a chain declares for itself, from its own entries: the opening entry
/// gives the first, and each `register_key_changed` entry one more, effective from
/// the entry it names (never later than itself, never before the period it replaces).
///
/// Whether a declaration deserves to be believed is not decided here: the chain
/// itself only proves that the declaring entry was signed by the key it declares.
pub fn declared_key_periods(entries: &[FiscalEntry]) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    for entry in entries {
        let declared = entry.payload.get("public_key").and_then(|v| v.as_str());
        match (entry.event_type.as_str(), declared) {
            ("register_opened", Some(key)) if out.is_empty() => out.push((entry.seq.max(1), key.to_string())),
            ("register_key_changed", Some(key)) => {
                let effective = entry
                    .payload
                    .get("effective_from_seq")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(entry.seq);
                let floor = out.last().map(|(from, _)| *from + 1).unwrap_or(1);
                if (floor..=entry.seq).contains(&effective) {
                    out.push((effective, key.to_string()));
                }
            }
            _ => {}
        }
    }
    out
}

fn verify_chain_with<'k>(
    entries: &[FiscalEntry],
    start_tip: &ChainTip,
    key_for: &dyn Fn(i64) -> Option<&'k crate::signature::PublicKey>,
) -> Result<ChainReport, ChainBreak> {
    let mut prev = start_tip.clone();
    let mut signatures_checked = 0usize;
    let mut unsigned = 0usize;

    for entry in entries {
        if entry.seq != prev.seq + 1 {
            return Err(ChainBreak::SequenceGap {
                expected: prev.seq + 1,
                found: entry.seq,
            });
        }

        if entry.prev_hash != prev.chain_hash {
            return Err(ChainBreak::PrevHashMismatch {
                seq: entry.seq,
                expected: prev.chain_hash.clone(),
                found: entry.prev_hash.clone(),
            });
        }

        let recomputed_payload =
            hash_payload(&entry.payload).map_err(|_| ChainBreak::PayloadTampered {
                seq: entry.seq,
                expected: entry.payload_hash.clone(),
                found: "<non-canonicalisable>".into(),
            })?;
        if recomputed_payload != entry.payload_hash {
            return Err(ChainBreak::PayloadTampered {
                seq: entry.seq,
                expected: entry.payload_hash.clone(),
                found: recomputed_payload,
            });
        }

        let recomputed_chain = compute_chain_hash(
            &entry.register_id,
            entry.seq,
            &entry.occurred_at,
            &entry.event_type,
            &entry.payload_hash,
            &entry.prev_hash,
        );
        if recomputed_chain != entry.chain_hash {
            return Err(ChainBreak::ChainHashMismatch {
                seq: entry.seq,
                expected: entry.chain_hash.clone(),
                found: recomputed_chain,
            });
        }

        // Within one register the clock must not run backwards. Equal stamps are
        // fine: several documents can be sealed inside the same second.
        if !prev.occurred_at.is_empty() && entry.occurred_at < prev.occurred_at {
            return Err(ChainBreak::TimeWentBackwards {
                seq: entry.seq,
                previous: prev.occurred_at.clone(),
                found: entry.occurred_at.clone(),
            });
        }

        if entry.signature.is_empty() {
            unsigned += 1;
        } else if let Some(key) = key_for(entry.seq) {
            key.verify_entry(entry)
                .map_err(|_| ChainBreak::BadSignature { seq: entry.seq })?;
            signatures_checked += 1;
        }

        prev = entry.tip();
    }

    Ok(ChainReport {
        register_id: entries
            .first()
            .map(|e| e.register_id.clone())
            .unwrap_or_default(),
        entries_checked: entries.len(),
        first_seq: entries.first().map(|e| e.seq).unwrap_or(0),
        last_seq: prev.seq,
        last_chain_hash: prev.chain_hash,
        signatures_checked,
        unsigned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_760_000_000 + secs, 0).unwrap()
    }

    fn build(n: i64) -> Vec<FiscalEntry> {
        let mut tip = ChainTip::genesis();
        let mut out = Vec::new();
        for i in 0..n {
            let e = seal(&tip, "C1", "sale", at(i * 60), json!({ "n": i })).unwrap();
            tip = e.tip();
            out.push(e);
        }
        out
    }

    #[test]
    fn first_entry_starts_from_genesis() {
        let entries = build(1);
        assert_eq!(entries[0].seq, 1);
        assert_eq!(entries[0].prev_hash, GENESIS_HASH);
    }

    #[test]
    fn a_clean_chain_verifies() {
        let entries = build(25);
        let report = verify_chain(&entries, &ChainTip::genesis(), None).unwrap();
        assert_eq!(report.entries_checked, 25);
        assert_eq!(report.last_seq, 25);
        assert_eq!(report.unsigned, 25);
    }

    #[test]
    fn editing_a_payload_is_detected_at_that_row() {
        let mut entries = build(10);
        entries[4].payload = json!({ "n": 999 });
        let err = verify_chain(&entries, &ChainTip::genesis(), None).unwrap_err();
        assert!(matches!(err, ChainBreak::PayloadTampered { seq: 5, .. }));
    }

    #[test]
    fn removing_a_row_is_detected() {
        let mut entries = build(10);
        entries.remove(6);
        let err = verify_chain(&entries, &ChainTip::genesis(), None).unwrap_err();
        assert!(matches!(
            err,
            ChainBreak::SequenceGap {
                expected: 7,
                found: 8
            }
        ));
    }

    #[test]
    fn re_signing_a_forged_row_still_breaks_the_link_to_the_next() {
        // The realistic attack: rewrite a row *and* recompute its own hashes.
        let mut entries = build(10);
        let tampered = seal(
            &ChainTip {
                seq: 4,
                chain_hash: entries[3].chain_hash.clone(),
                occurred_at: entries[3].occurred_at.clone(),
            },
            "C1",
            "sale",
            at(4 * 60),
            json!({ "n": 4242 }),
        )
        .unwrap();
        entries[4] = tampered;
        let err = verify_chain(&entries, &ChainTip::genesis(), None).unwrap_err();
        assert!(matches!(err, ChainBreak::PrevHashMismatch { seq: 6, .. }));
    }

    #[test]
    fn backdating_within_a_register_is_rejected() {
        let mut tip = ChainTip::genesis();
        let a = seal(&tip, "C1", "sale", at(100), json!({})).unwrap();
        tip = a.tip();
        let b = seal(&tip, "C1", "sale", at(10), json!({})).unwrap();
        let err = verify_chain(&[a, b], &ChainTip::genesis(), None).unwrap_err();
        assert!(matches!(err, ChainBreak::TimeWentBackwards { seq: 2, .. }));
    }

    #[test]
    fn several_entries_may_share_one_second() {
        let mut tip = ChainTip::genesis();
        let mut all = Vec::new();
        for i in 0..3 {
            let e = seal(&tip, "C1", "sale", at(7), json!({ "i": i })).unwrap();
            tip = e.tip();
            all.push(e);
        }
        assert!(verify_chain(&all, &ChainTip::genesis(), None).is_ok());
    }

    #[test]
    fn verification_can_resume_from_a_stored_tip() {
        let entries = build(10);
        let (head, tail) = entries.split_at(6);
        let tip = head.last().unwrap().tip();
        let report = verify_chain(tail, &tip, None).unwrap();
        assert_eq!(report.first_seq, 7);
        assert_eq!(report.last_seq, 10);
    }

    #[test]
    fn registers_with_the_same_content_produce_different_hashes() {
        let a = seal(&ChainTip::genesis(), "C1", "sale", at(0), json!({ "n": 1 })).unwrap();
        let b = seal(&ChainTip::genesis(), "C2", "sale", at(0), json!({ "n": 1 })).unwrap();
        assert_ne!(a.chain_hash, b.chain_hash);
    }

    #[test]
    fn hash_is_stable_across_key_insertion_order() {
        let one: Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
        let two: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
        assert_eq!(hash_payload(&one).unwrap(), hash_payload(&two).unwrap());
    }
}

#[cfg(test)]
mod key_period_tests {
    use super::*;
    use crate::signature::SecretKey;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn push(chain: &mut Vec<FiscalEntry>, key: &SecretKey, event: &str, payload: Value) {
        let tip = chain.last().map(|e| e.tip()).unwrap_or_else(ChainTip::genesis);
        let at = Utc.timestamp_opt(1_760_000_000 + chain.len() as i64 * 30, 0).unwrap();
        let mut e = seal(&tip, "C1", event, at, payload).unwrap();
        e.signature = key.sign_entry(&e).unwrap();
        chain.push(e);
    }

    fn periods_of(chain: &[FiscalEntry]) -> KeyPeriods {
        declared_key_periods(chain)
            .into_iter()
            .map(|(from, hex)| (from, crate::signature::PublicKey::from_hex(&hex).unwrap()))
            .collect()
    }

    /// Entries 1-2 signed by A, then B declares itself at entry 3.
    fn replaced(declare: Value) -> Vec<FiscalEntry> {
        let (a, b) = (SecretKey::generate(), SecretKey::generate());
        let mut chain = Vec::new();
        push(&mut chain, &a, "register_opened", json!({ "public_key": a.public().to_hex() }));
        push(&mut chain, &a, "invoice", json!({ "n": 1 }));
        let mut payload = json!({ "public_key": b.public().to_hex(), "effective_from_seq": 3 });
        if let (Some(extra), Some(obj)) = (declare.as_object(), payload.as_object_mut()) {
            obj.extend(extra.clone());
        }
        push(&mut chain, &b, "register_key_changed", payload);
        push(&mut chain, &b, "invoice", json!({ "n": 2 }));
        chain
    }

    #[test]
    fn a_chain_is_checked_against_the_key_that_signs_each_entry() {
        let chain = replaced(json!({}));
        let periods = periods_of(&chain);
        assert_eq!(periods.iter().map(|(from, _)| *from).collect::<Vec<_>>(), vec![1, 3]);
        let report = verify_chain_periods(&chain, &ChainTip::genesis(), &periods).unwrap();
        assert_eq!(report.signatures_checked, 4);

        // Against one key alone, the later half fails: this is what a single recorded key did.
        let only_first = vec![periods[0].clone()];
        assert!(matches!(
            verify_chain_periods(&chain, &ChainTip::genesis(), &only_first),
            Err(ChainBreak::BadSignature { seq: 3 })
        ));
    }

    #[test]
    fn a_declaration_cannot_start_in_the_future_or_before_the_key_it_replaces() {
        // Effective at 9 although declared at 3: ignored, so entry 3 is not covered by it.
        let chain = replaced(json!({ "effective_from_seq": 9 }));
        assert_eq!(declared_key_periods(&chain).len(), 1);
        // Effective at 1, which the first key already holds: ignored too.
        let chain = replaced(json!({ "effective_from_seq": 1 }));
        assert_eq!(declared_key_periods(&chain).len(), 1);
    }

    #[test]
    fn declared_unverifiable_signatures_are_skipped_but_hashes_still_chain() {
        let (a, b, stranger) = (SecretKey::generate(), SecretKey::generate(), SecretKey::generate());
        let mut chain = Vec::new();
        push(&mut chain, &a, "register_opened", json!({ "public_key": a.public().to_hex() }));
        push(&mut chain, &stranger, "session_opened", json!({}));
        push(
            &mut chain,
            &b,
            "register_key_changed",
            json!({
                "public_key": b.public().to_hex(), "effective_from_seq": 3,
                "unverifiable_from_seq": 2, "unverifiable_to_seq": 2,
            }),
        );
        let periods = periods_of(&chain);
        assert!(verify_chain_periods(&chain, &ChainTip::genesis(), &periods).is_err());
        let ranges = declared_unverifiable_ranges(&chain);
        assert_eq!(ranges, vec![(2, 2)]);
        verify_chain_declared(&chain, &ChainTip::genesis(), &periods, &ranges).unwrap();

        // The hashes are not excused: change entry 2 and the chain breaks.
        let mut tampered = chain.clone();
        tampered[1].payload = json!({ "forged": true });
        assert!(matches!(
            verify_chain_declared(&tampered, &ChainTip::genesis(), &periods, &ranges),
            Err(ChainBreak::PayloadTampered { seq: 2, .. })
        ));
    }

    #[test]
    fn a_range_cannot_excuse_the_declaration_itself_or_what_follows() {
        let (a, b) = (SecretKey::generate(), SecretKey::generate());
        let mut chain = Vec::new();
        push(&mut chain, &a, "register_opened", json!({ "public_key": a.public().to_hex() }));
        push(
            &mut chain,
            &b,
            "register_key_changed",
            json!({
                "public_key": b.public().to_hex(), "effective_from_seq": 2,
                "unverifiable_from_seq": 1, "unverifiable_to_seq": 5,
            }),
        );
        assert!(declared_unverifiable_ranges(&chain).is_empty(), "to_seq must precede the declaration");
    }
}
