//! Period closures and the two counters § 170 requires.
//!
//! "Cumul du grand total de la période" is turnover since the accounting period
//! opened. "Total perpétuel" is turnover since the software was first used and
//! never resets -- § 273 is explicit that it only restarts at zero when the
//! software itself is replaced, which is why the recovered Motogest figures are
//! archived separately rather than seeded into it.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::event::ClosureKind;
use crate::vat::VatBucket;

/// A sealed closure, as stored and as written into the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosureTotals {
    pub kind: ClosureKind,
    pub register_id: String,
    /// Inclusive, `YYYY-MM-DD`.
    pub period_start: String,
    /// Inclusive, `YYYY-MM-DD`.
    pub period_end: String,
    pub document_count: i64,
    /// Journal range covered; zero when the period had no activity.
    pub first_seq: i64,
    pub last_seq: i64,
    pub total_ht_cents: i64,
    pub total_vat_cents: i64,
    pub total_ttc_cents: i64,
    pub buckets: Vec<VatBucket>,
    pub period_grand_total_cents: i64,
    pub perpetual_total_cents: i64,
}

impl ClosureTotals {
    /// The canonical journal payload for this closure.
    pub fn to_payload(&self) -> Value {
        json!({
            "kind": self.kind.as_str(),
            "register_id": self.register_id,
            "period_start": self.period_start,
            "period_end": self.period_end,
            "document_count": self.document_count,
            "first_seq": self.first_seq,
            "last_seq": self.last_seq,
            "total_ht_cents": self.total_ht_cents,
            "total_vat_cents": self.total_vat_cents,
            "total_ttc_cents": self.total_ttc_cents,
            "vat_breakdown": self
                .buckets
                .iter()
                .map(|b| json!({
                    "rate_bp": b.rate_bp,
                    "base_ht_cents": b.base_ht_cents,
                    "vat_cents": b.vat_cents,
                    "total_ttc_cents": b.total_ttc_cents,
                }))
                .collect::<Vec<_>>(),
            "period_grand_total_cents": self.period_grand_total_cents,
            "perpetual_total_cents": self.perpetual_total_cents,
        })
    }
}

/// The running counters a register carries between closures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub period_grand_total_cents: i64,
    pub perpetual_total_cents: i64,
}

impl Counters {
    /// Add a document's VAT-inclusive total. Credit notes arrive negative and
    /// subtract, which is how corrections show up in the totals at all.
    pub fn add(&mut self, ttc_cents: i64) {
        self.period_grand_total_cents = self.period_grand_total_cents.saturating_add(ttc_cents);
        self.perpetual_total_cents = self.perpetual_total_cents.saturating_add(ttc_cents);
    }

    /// Start a new accounting period. Only the period counter resets.
    pub fn open_period(&mut self) {
        self.period_grand_total_cents = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perpetual_total_survives_a_new_period() {
        let mut c = Counters::default();
        c.add(12_000);
        c.add(3_000);
        assert_eq!(c.period_grand_total_cents, 15_000);

        c.open_period();
        c.add(500);
        assert_eq!(c.period_grand_total_cents, 500);
        assert_eq!(c.perpetual_total_cents, 15_500);
    }

    #[test]
    fn credit_notes_reduce_both_counters() {
        let mut c = Counters::default();
        c.add(10_000);
        c.add(-2_500);
        assert_eq!(c.period_grand_total_cents, 7_500);
        assert_eq!(c.perpetual_total_cents, 7_500);
    }

    #[test]
    fn payload_carries_every_counter() {
        let totals = ClosureTotals {
            kind: ClosureKind::Day,
            register_id: "C1".into(),
            period_start: "2026-08-21".into(),
            period_end: "2026-08-21".into(),
            document_count: 2,
            first_seq: 10,
            last_seq: 14,
            total_ht_cents: 10_000,
            total_vat_cents: 2_000,
            total_ttc_cents: 12_000,
            buckets: vec![VatBucket {
                rate_bp: 2_000,
                base_ht_cents: 10_000,
                vat_cents: 2_000,
                total_ttc_cents: 12_000,
            }],
            period_grand_total_cents: 12_000,
            perpetual_total_cents: 987_654,
        };

        let p = totals.to_payload();
        assert_eq!(p["kind"], "day");
        assert_eq!(p["perpetual_total_cents"], 987_654);
        assert_eq!(p["vat_breakdown"][0]["rate_bp"], 2_000);
        // Must be hashable: no floats anywhere.
        assert!(crate::canonical::to_canonical_string(&p).is_ok());
    }
}
