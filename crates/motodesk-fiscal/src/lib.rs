//! Fiscal primitives for the French anti-fraud rules on payment software.
//!
//! Article 286 I-3° bis of the CGI requires software that records customer
//! payments to satisfy four conditions, known as ISCA: inaltérabilité,
//! sécurisation, conservation, archivage. The administrative doctrine is
//! BOI-TVA-DECLA-30-10-30, whose paragraph numbers are cited throughout this
//! crate so that each rule can be traced back to its source.
//!
//! Everything here is pure: no database, no clock, no filesystem. That is
//! deliberate. These are the parts an inspector would want to reason about, and
//! they are far easier to trust when they can be exercised in isolation.
//!
//! The pieces:
//!
//! - [`canonical`] turns a payload into bytes that hash identically everywhere.
//! - [`chain`] links entries so a change to one invalidates all that follow.
//! - [`signature`] proves the entries came from this register.
//! - [`vat`] computes document totals, once, correctly.
//! - [`numbering`] issues unbroken per-register document numbers.
//! - [`closure`] holds the daily, monthly and annual totals.
//! - [`event`] enumerates what the journal records.

pub mod canonical;
pub mod chain;
pub mod closure;
pub mod error;
pub mod event;
pub mod numbering;
pub mod signature;
pub mod vat;

pub use chain::{
    compute_chain_hash, format_timestamp, hash_payload, seal, verify_chain, ChainReport, ChainTip,
    FiscalEntry, GENESIS_HASH,
};
pub use closure::{ClosureTotals, Counters};
pub use error::{ChainBreak, FiscalError};
pub use event::{ClosureKind, FiscalEventType};
pub use numbering::{format_number, parse_number, DocumentKind};
pub use signature::{PublicKey, SecretKey};
pub use vat::{document_totals, DocumentTotals, LineInput, LineTotals, VatBucket};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    /// A day at the counter, end to end: two invoices, one correction, one
    /// closure, then a full verification of what was written.
    #[test]
    fn a_trading_day_seals_and_verifies() {
        let key = SecretKey::generate();
        let mut tip = ChainTip::genesis();
        let mut counters = Counters::default();
        let mut entries = Vec::new();

        let append = |tip: &mut ChainTip,
                      entries: &mut Vec<FiscalEntry>,
                      kind: FiscalEventType,
                      secs: i64,
                      payload: serde_json::Value| {
            let at = Utc.timestamp_opt(1_787_000_000 + secs, 0).unwrap();
            let mut entry = seal(tip, "C1", kind.as_str(), at, payload).unwrap();
            entry.signature = key.sign_entry(&entry).unwrap();
            *tip = entry.tip();
            entries.push(entry);
        };

        // Helmet and a service, sold together.
        let sale = document_totals(&[
            LineInput::new(1_000, 12_000, 2_000),
            LineInput::new(1_500, 6_000, 2_000),
        ])
        .unwrap();
        counters.add(sale.total_ttc_cents);
        append(
            &mut tip,
            &mut entries,
            FiscalEventType::Invoice,
            0,
            json!({
                "reference": format_number(DocumentKind::Invoice, 2026, "C1", 1).unwrap(),
                "total_ht_cents": sale.total_ht_cents,
                "total_vat_cents": sale.total_vat_cents,
                "total_ttc_cents": sale.total_ttc_cents,
            }),
        );

        append(
            &mut tip,
            &mut entries,
            FiscalEventType::Payment,
            30,
            json!({ "method": "card", "amount_cents": sale.total_ttc_cents }),
        );

        // The service was billed twice; corrected by a credit note, never edited.
        let credit = document_totals(&vat::negate(&[LineInput::new(1_500, 6_000, 2_000)])).unwrap();
        counters.add(credit.total_ttc_cents);
        append(
            &mut tip,
            &mut entries,
            FiscalEventType::CreditNote,
            600,
            json!({
                "reference": format_number(DocumentKind::CreditNote, 2026, "C1", 1).unwrap(),
                "corrects": format_number(DocumentKind::Invoice, 2026, "C1", 1).unwrap(),
                "total_ttc_cents": credit.total_ttc_cents,
            }),
        );

        let closure = ClosureTotals {
            kind: ClosureKind::Day,
            register_id: "C1".into(),
            period_start: "2026-08-21".into(),
            period_end: "2026-08-21".into(),
            document_count: 2,
            first_seq: 1,
            last_seq: 3,
            total_ht_cents: sale.total_ht_cents + credit.total_ht_cents,
            total_vat_cents: sale.total_vat_cents + credit.total_vat_cents,
            total_ttc_cents: counters.period_grand_total_cents,
            buckets: sale.buckets.clone(),
            period_grand_total_cents: counters.period_grand_total_cents,
            perpetual_total_cents: counters.perpetual_total_cents,
        };
        append(
            &mut tip,
            &mut entries,
            FiscalEventType::ClosureDay,
            3_600,
            closure.to_payload(),
        );

        let report = verify_chain(&entries, &ChainTip::genesis(), Some(&key.public())).unwrap();
        assert_eq!(report.entries_checked, 4);
        assert_eq!(report.signatures_checked, 4);
        assert_eq!(report.last_seq, 4);

        // 120.00 helmet + 90.00 service, less the 90.00 credited back.
        assert_eq!(counters.perpetual_total_cents, 14_400);
    }

    /// Deleting an inconvenient invoice must be visible even if every later row
    /// is renumbered to hide the hole.
    #[test]
    fn quietly_dropping_an_invoice_is_detectable() {
        let key = SecretKey::generate();
        let mut tip = ChainTip::genesis();
        let mut entries = Vec::new();
        for i in 0..6 {
            let at = Utc.timestamp_opt(1_787_000_000 + i * 60, 0).unwrap();
            let mut e = seal(
                &tip,
                "C1",
                "invoice",
                at,
                json!({ "amount": 1_000 * (i + 1) }),
            )
            .unwrap();
            e.signature = key.sign_entry(&e).unwrap();
            tip = e.tip();
            entries.push(e);
        }

        entries.remove(2);
        for (index, entry) in entries.iter_mut().enumerate() {
            entry.seq = index as i64 + 1;
        }

        let err = verify_chain(&entries, &ChainTip::genesis(), Some(&key.public())).unwrap_err();
        assert_eq!(err.seq(), 3);
    }
}
