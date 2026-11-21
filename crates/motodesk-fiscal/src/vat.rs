//! VAT and document totals.
//!
//! Nothing in the codebase computed these before: `sales.total_ht_cents` was a
//! stored field that no code ever recalculated. An invoice is a legal document,
//! so the arithmetic has to be one shared, tested function rather than something
//! each screen works out for itself.
//!
//! Rounding follows French practice: VAT is worked out per rate on the sum of
//! the net amounts at that rate, not line by line, then rounded once. Totals are
//! therefore exact by construction -- HT plus VAT always equals TTC, with no
//! rounding residue to explain to an inspector.
//!
//! A line is priced either HT (a workshop quote: 83,33 EUR + VAT) or TTC (a shelf
//! price: 9,99 EUR, which is what the customer pays). They cannot be converted
//! into each other without loss in whole cents -- at 20 % no net amount in cents
//! gives 9,99 EUR once VAT is added -- so a TTC line keeps its TTC amount as the
//! authority and the net and VAT are derived from the *sum of the TTC amounts at
//! that rate*. Three shelf items at 9,99 EUR ring up 29,97 EUR, exactly.

use serde::{Deserialize, Serialize};

use crate::error::FiscalError;

/// 100.00% expressed in basis points.
pub const FULL_BP: i64 = 10_000;
/// One whole unit expressed in thousandths.
pub const UNIT_MILLI: i64 = 1_000;

/// A line as captured on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineInput {
    /// 1000 = one unit. Negative on a credit note.
    pub quantity_milli: i64,
    /// Used when `unit_price_ttc_cents` is `None`; otherwise only a display value.
    pub unit_price_ht_cents: i64,
    /// Set for a line priced VAT-inclusive. It is then the price of record.
    #[serde(default)]
    pub unit_price_ttc_cents: Option<i64>,
    /// 0..=10000, so 500 is a 5% discount.
    pub discount_bp: i64,
    /// 0..=10000, so 2000 is 20% VAT.
    pub vat_rate_bp: i64,
}

impl LineInput {
    pub fn new(quantity_milli: i64, unit_price_ht_cents: i64, vat_rate_bp: i64) -> Self {
        Self {
            quantity_milli,
            unit_price_ht_cents,
            unit_price_ttc_cents: None,
            discount_bp: 0,
            vat_rate_bp,
        }
    }

    /// A line priced VAT-inclusive, as on a shelf label.
    pub fn new_ttc(quantity_milli: i64, unit_price_ttc_cents: i64, vat_rate_bp: i64) -> Self {
        Self {
            quantity_milli,
            // Display value only; the engine never reads it for this line.
            unit_price_ht_cents: ht_from_ttc(unit_price_ttc_cents, vat_rate_bp).unwrap_or(0),
            unit_price_ttc_cents: Some(unit_price_ttc_cents),
            discount_bp: 0,
            vat_rate_bp,
        }
    }

    pub fn with_discount(mut self, discount_bp: i64) -> Self {
        self.discount_bp = discount_bp;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineTotals {
    /// Quantity times unit price, before any discount.
    pub gross_ht_cents: i64,
    pub discount_cents: i64,
    pub net_ht_cents: i64,
    pub vat_rate_bp: i64,
    /// What the customer pays for this line, for a line priced TTC. `None` for an
    /// HT line, whose VAT only exists per rate, not per line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_ttc_cents: Option<i64>,
}

/// Everything charged at one VAT rate. An invoice needs this breakdown printed
/// on it (CGI ann. II art. 242 nonies A) and the closures aggregate it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VatBucket {
    pub rate_bp: i64,
    pub base_ht_cents: i64,
    pub vat_cents: i64,
    pub total_ttc_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentTotals {
    pub lines: Vec<LineTotals>,
    /// Ascending by rate, one entry per rate actually used.
    pub buckets: Vec<VatBucket>,
    pub total_ht_cents: i64,
    pub total_vat_cents: i64,
    pub total_ttc_cents: i64,
}

/// Integer division rounded half away from zero.
///
/// Away from zero rather than to even so that negating every input negates
/// every output exactly: a credit note built from an invoice's lines must
/// cancel it to the cent.
fn div_round(numerator: i128, denominator: i128) -> Result<i64, FiscalError> {
    debug_assert!(denominator > 0);
    let negative = numerator < 0;
    let magnitude = if negative { -numerator } else { numerator };
    let quotient = (magnitude * 2 + denominator) / (denominator * 2);
    let signed = if negative { -quotient } else { quotient };
    i64::try_from(signed).map_err(|_| FiscalError::Overflow("rounded amount"))
}

fn check_rates(line: &LineInput) -> Result<(), FiscalError> {
    if !(0..=FULL_BP).contains(&line.vat_rate_bp) {
        return Err(FiscalError::VatRateOutOfRange(line.vat_rate_bp));
    }
    if !(0..=FULL_BP).contains(&line.discount_bp) {
        return Err(FiscalError::DiscountOutOfRange(line.discount_bp));
    }
    Ok(())
}

/// Totals for a single line.
pub fn line_totals(line: &LineInput) -> Result<LineTotals, FiscalError> {
    check_rates(line)?;

    let quantity = line.quantity_milli as i128;

    if let Some(unit_ttc) = line.unit_price_ttc_cents {
        // The customer's amount is the authority; the net is provisional here and
        // is settled per rate in `document_totals`.
        let unit = unit_ttc as i128;
        let gross_ttc = div_round(quantity * unit, UNIT_MILLI as i128)?;
        let net_ttc = div_round(
            quantity * unit * (FULL_BP - line.discount_bp) as i128,
            UNIT_MILLI as i128 * FULL_BP as i128,
        )?;
        let divisor = (FULL_BP + line.vat_rate_bp) as i128;
        let gross_ht = div_round(gross_ttc as i128 * FULL_BP as i128, divisor)?;
        let net_ht = div_round(net_ttc as i128 * FULL_BP as i128, divisor)?;
        return Ok(LineTotals {
            gross_ht_cents: gross_ht,
            discount_cents: gross_ht - net_ht,
            net_ht_cents: net_ht,
            vat_rate_bp: line.vat_rate_bp,
            net_ttc_cents: Some(net_ttc),
        });
    }

    let unit = line.unit_price_ht_cents as i128;
    let gross = div_round(quantity * unit, UNIT_MILLI as i128)?;
    let net = div_round(
        quantity * unit * (FULL_BP - line.discount_bp) as i128,
        UNIT_MILLI as i128 * FULL_BP as i128,
    )?;

    Ok(LineTotals {
        gross_ht_cents: gross,
        // Derived rather than rounded separately, so the three always reconcile.
        discount_cents: gross - net,
        net_ht_cents: net,
        vat_rate_bp: line.vat_rate_bp,
        net_ttc_cents: None,
    })
}

/// Totals for a whole document.
pub fn document_totals(lines: &[LineInput]) -> Result<DocumentTotals, FiscalError> {
    let mut computed = Vec::with_capacity(lines.len());
    for line in lines {
        computed.push(line_totals(line)?);
    }

    // Accumulate per rate first: rounding once per rate is what the tax
    // authority expects and it keeps HT + VAT == TTC exact.
    let mut rates: Vec<i64> = computed.iter().map(|l| l.vat_rate_bp).collect();
    rates.sort_unstable();
    rates.dedup();

    let mut buckets = Vec::with_capacity(rates.len());
    for rate in rates {
        // HT lines: net summed, VAT rounded once on the sum.
        let ht_sum: i128 = computed
            .iter()
            .filter(|l| l.vat_rate_bp == rate && l.net_ttc_cents.is_none())
            .map(|l| l.net_ht_cents as i128)
            .sum();
        let ht_vat = div_round(ht_sum * rate as i128, FULL_BP as i128)?;

        // TTC lines: the sum of what customers pay is exact; the net is backed out
        // of it once, and the VAT is whatever is left of the TTC.
        let ttc_sum: i128 = computed
            .iter()
            .filter(|l| l.vat_rate_bp == rate)
            .filter_map(|l| l.net_ttc_cents)
            .map(i128::from)
            .sum();
        let ttc_net = div_round(ttc_sum * FULL_BP as i128, (FULL_BP + rate) as i128)?;
        let ttc_vat = i64::try_from(ttc_sum).map_err(|_| FiscalError::Overflow("TTC sum"))? - ttc_net;

        // Spread the rounding of the one back-out over the lines it came from, so
        // the lines still add up to the document net.
        let line_net_sum: i64 = computed
            .iter()
            .filter(|l| l.vat_rate_bp == rate && l.net_ttc_cents.is_some())
            .map(|l| l.net_ht_cents)
            .sum();
        let residue = ttc_net - line_net_sum;
        if residue != 0 {
            let target = computed
                .iter()
                .enumerate()
                .filter(|(_, l)| l.vat_rate_bp == rate && l.net_ttc_cents.is_some())
                .max_by_key(|(i, l)| (l.net_ttc_cents.unwrap_or(0).unsigned_abs(), std::cmp::Reverse(*i)))
                .map(|(i, _)| i);
            if let Some(i) = target {
                computed[i].net_ht_cents += residue;
                computed[i].discount_cents -= residue;
            }
        }

        let base = ht_sum + ttc_net as i128;
        let base_i64 = i64::try_from(base).map_err(|_| FiscalError::Overflow("VAT base"))?;
        let vat = ht_vat
            .checked_add(ttc_vat)
            .ok_or(FiscalError::Overflow("bucket VAT"))?;
        buckets.push(VatBucket {
            rate_bp: rate,
            base_ht_cents: base_i64,
            vat_cents: vat,
            total_ttc_cents: base_i64
                .checked_add(vat)
                .ok_or(FiscalError::Overflow("bucket TTC"))?,
        });
    }

    let mut total_ht: i64 = 0;
    let mut total_vat: i64 = 0;
    for bucket in &buckets {
        total_ht = total_ht
            .checked_add(bucket.base_ht_cents)
            .ok_or(FiscalError::Overflow("document HT"))?;
        total_vat = total_vat
            .checked_add(bucket.vat_cents)
            .ok_or(FiscalError::Overflow("document VAT"))?;
    }

    Ok(DocumentTotals {
        lines: computed,
        buckets,
        total_ht_cents: total_ht,
        total_vat_cents: total_vat,
        total_ttc_cents: total_ht
            .checked_add(total_vat)
            .ok_or(FiscalError::Overflow("document TTC"))?,
    })
}

/// Net of VAT from a VAT-inclusive price, for counter sales priced TTC.
pub fn ht_from_ttc(ttc_cents: i64, vat_rate_bp: i64) -> Result<i64, FiscalError> {
    if !(0..=FULL_BP).contains(&vat_rate_bp) {
        return Err(FiscalError::VatRateOutOfRange(vat_rate_bp));
    }
    div_round(
        ttc_cents as i128 * FULL_BP as i128,
        (FULL_BP + vat_rate_bp) as i128,
    )
}

/// VAT-inclusive price from a net one.
pub fn ttc_from_ht(ht_cents: i64, vat_rate_bp: i64) -> Result<i64, FiscalError> {
    if !(0..=FULL_BP).contains(&vat_rate_bp) {
        return Err(FiscalError::VatRateOutOfRange(vat_rate_bp));
    }
    let vat = div_round(ht_cents as i128 * vat_rate_bp as i128, FULL_BP as i128)?;
    ht_cents
        .checked_add(vat)
        .ok_or(FiscalError::Overflow("TTC from HT"))
}

/// Flip every quantity, turning an invoice's lines into a credit note's.
pub fn negate(lines: &[LineInput]) -> Vec<LineInput> {
    lines
        .iter()
        .map(|l| LineInput {
            quantity_milli: -l.quantity_milli,
            ..*l
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDARD: i64 = 2000;
    const REDUCED: i64 = 550;

    #[test]
    fn one_line_at_the_standard_rate() {
        let t = document_totals(&[LineInput::new(UNIT_MILLI, 10_000, STANDARD)]).unwrap();
        assert_eq!(t.total_ht_cents, 10_000);
        assert_eq!(t.total_vat_cents, 2_000);
        assert_eq!(t.total_ttc_cents, 12_000);
    }

    #[test]
    fn fractional_quantities() {
        // 2.5 litres at 13.37 EUR
        let t = document_totals(&[LineInput::new(2_500, 1_337, STANDARD)]).unwrap();
        assert_eq!(t.total_ht_cents, 3_343); // 3342.5 rounds away from zero
        assert_eq!(t.total_vat_cents, 669); // 668.6
        assert_eq!(t.total_ttc_cents, 4_012);
    }

    #[test]
    fn discount_reconciles_with_the_gross() {
        let line = LineInput::new(3 * UNIT_MILLI, 999, STANDARD).with_discount(1_500);
        let t = line_totals(&line).unwrap();
        assert_eq!(t.gross_ht_cents, 2_997);
        assert_eq!(t.net_ht_cents, 2_547); // 2547.45
        assert_eq!(t.discount_cents, t.gross_ht_cents - t.net_ht_cents);
    }

    #[test]
    fn mixed_rates_are_bucketed_separately() {
        let t = document_totals(&[
            LineInput::new(UNIT_MILLI, 10_000, STANDARD),
            LineInput::new(UNIT_MILLI, 5_000, REDUCED),
            LineInput::new(2 * UNIT_MILLI, 2_500, STANDARD),
        ])
        .unwrap();

        assert_eq!(t.buckets.len(), 2);
        assert_eq!(t.buckets[0].rate_bp, REDUCED);
        assert_eq!(t.buckets[0].base_ht_cents, 5_000);
        assert_eq!(t.buckets[0].vat_cents, 275);
        assert_eq!(t.buckets[1].rate_bp, STANDARD);
        assert_eq!(t.buckets[1].base_ht_cents, 15_000);
        assert_eq!(t.buckets[1].vat_cents, 3_000);
        assert_eq!(t.total_ht_cents, 20_000);
        assert_eq!(t.total_vat_cents, 3_275);
        assert_eq!(t.total_ttc_cents, 23_275);
    }

    #[test]
    fn vat_is_rounded_per_rate_not_per_line() {
        // Three lines of 0.01 EUR: per-line rounding would give 3 x 0 = 0,
        // per-rate rounding gives round(0.006) = 0.01.
        let lines = vec![LineInput::new(UNIT_MILLI, 1, STANDARD); 3];
        let t = document_totals(&lines).unwrap();
        assert_eq!(t.total_ht_cents, 3);
        assert_eq!(t.total_vat_cents, 1);
    }

    #[test]
    fn zero_rate_produces_no_vat() {
        let t = document_totals(&[LineInput::new(UNIT_MILLI, 45_000, 0)]).unwrap();
        assert_eq!(t.total_vat_cents, 0);
        assert_eq!(t.total_ttc_cents, t.total_ht_cents);
    }

    #[test]
    fn a_credit_note_cancels_its_invoice_exactly() {
        let lines = vec![
            LineInput::new(3_333, 1_777, STANDARD).with_discount(777),
            LineInput::new(UNIT_MILLI, 4_999, REDUCED),
            LineInput::new(7 * UNIT_MILLI, 33, 1_000),
        ];
        let invoice = document_totals(&lines).unwrap();
        let credit = document_totals(&negate(&lines)).unwrap();

        assert_eq!(invoice.total_ht_cents, -credit.total_ht_cents);
        assert_eq!(invoice.total_vat_cents, -credit.total_vat_cents);
        assert_eq!(invoice.total_ttc_cents, -credit.total_ttc_cents);
    }

    #[test]
    fn ttc_and_ht_round_trip_for_every_preset_rate() {
        for rate in [0, 210, 550, 1_000, 2_000] {
            for ttc in [1i64, 99, 1_200, 45_678, 1_999_999] {
                let ht = ht_from_ttc(ttc, rate).unwrap();
                let back = ttc_from_ht(ht, rate).unwrap();
                assert!(
                    (back - ttc).abs() <= 1,
                    "rate {rate}, ttc {ttc}: got {back} back"
                );
            }
        }
    }

    #[test]
    fn a_shelf_price_rings_up_exactly() {
        // 9,99 EUR TTC at 20 %: no net amount in cents gives 9,99 with VAT added,
        // so pricing it HT charged 10,00. Priced TTC it is exact.
        for (ttc, qty, expected) in [(999, 1, 999), (999, 3, 2_997), (1_299, 2, 2_598), (495, 1, 495), (1, 1, 1)] {
            let t = document_totals(&[LineInput::new_ttc(qty * UNIT_MILLI, ttc, STANDARD)]).unwrap();
            assert_eq!(t.total_ttc_cents, expected, "{ttc} x {qty}");
            assert_eq!(t.total_ht_cents + t.total_vat_cents, t.total_ttc_cents);
            assert_eq!(t.lines[0].net_ttc_cents, Some(expected));
        }
        let hit_by_the_old_way = document_totals(&[LineInput::new(UNIT_MILLI, 833, STANDARD)]).unwrap();
        assert_eq!(hit_by_the_old_way.total_ttc_cents, 1_000, "the HT route cannot reach 9,99");
    }

    #[test]
    fn every_ttc_price_up_to_a_thousand_euros_is_reproduced_at_every_rate() {
        for rate in [0, 210, 550, 1_000, 2_000] {
            for ttc in 1..=100_000i64 {
                let t = document_totals(&[LineInput::new_ttc(UNIT_MILLI, ttc, rate)]).unwrap();
                assert_eq!(t.total_ttc_cents, ttc, "rate {rate}, price {ttc}");
            }
        }
    }

    #[test]
    fn ttc_lines_at_one_rate_share_a_single_rounding() {
        // Three 0,10 EUR TTC lines: backing the net out of each gives 3 x 0,08 =
        // 0,24 HT; from the sum 0,30 it is 0,25. The sum is what is taxed.
        let lines = vec![LineInput::new_ttc(UNIT_MILLI, 10, STANDARD); 3];
        let t = document_totals(&lines).unwrap();
        assert_eq!(t.total_ttc_cents, 30);
        assert_eq!(t.total_ht_cents, 25);
        assert_eq!(t.total_vat_cents, 5);
        assert_eq!(t.lines.iter().map(|l| l.net_ht_cents).sum::<i64>(), 25, "lines still add up");
    }

    #[test]
    fn ttc_and_ht_lines_can_share_a_rate_and_a_document() {
        let t = document_totals(&[
            LineInput::new(UNIT_MILLI, 10_000, STANDARD),    // 100,00 HT -> 120,00
            LineInput::new_ttc(UNIT_MILLI, 999, STANDARD),   // 9,99 TTC
            LineInput::new_ttc(UNIT_MILLI, 1_050, REDUCED),  // 10,50 TTC at 5,5 %
        ])
        .unwrap();
        assert_eq!(t.total_ttc_cents, 12_000 + 999 + 1_050);
        assert_eq!(t.total_ht_cents + t.total_vat_cents, t.total_ttc_cents);
        assert_eq!(t.buckets.len(), 2);
    }

    #[test]
    fn a_ttc_discount_is_taken_off_what_the_customer_pays() {
        let t = document_totals(&[LineInput::new_ttc(2 * UNIT_MILLI, 1_999, STANDARD).with_discount(1_000)]).unwrap();
        // 2 x 19,99 = 39,98; 10 % off = 35,98 (35,982 rounded)
        assert_eq!(t.total_ttc_cents, 3_598);
    }

    #[test]
    fn a_credit_note_cancels_a_ttc_invoice_exactly() {
        let lines = vec![
            LineInput::new_ttc(3_000, 999, STANDARD),
            LineInput::new_ttc(1_500, 1_299, STANDARD).with_discount(777),
            LineInput::new_ttc(UNIT_MILLI, 4_990, REDUCED),
            LineInput::new(2 * UNIT_MILLI, 3_333, STANDARD),
        ];
        let invoice = document_totals(&lines).unwrap();
        let credit = document_totals(&negate(&lines)).unwrap();
        assert_eq!(invoice.total_ht_cents, -credit.total_ht_cents);
        assert_eq!(invoice.total_vat_cents, -credit.total_vat_cents);
        assert_eq!(invoice.total_ttc_cents, -credit.total_ttc_cents);
        for (a, b) in invoice.lines.iter().zip(credit.lines.iter()) {
            assert_eq!(a.net_ht_cents, -b.net_ht_cents);
        }
    }

    #[test]
    fn rejects_impossible_rates() {
        assert!(document_totals(&[LineInput::new(UNIT_MILLI, 100, 10_001)]).is_err());
        assert!(
            document_totals(&[LineInput::new(UNIT_MILLI, 100, STANDARD).with_discount(-1)])
                .is_err()
        );
    }

    #[test]
    fn empty_document_is_all_zero() {
        let t = document_totals(&[]).unwrap();
        assert_eq!(t.total_ttc_cents, 0);
        assert!(t.buckets.is_empty());
    }

    // Deterministic pseudo-random sweep, kept dependency-free.
    fn xorshift(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn totals_always_reconcile_over_random_documents() {
        let rates = [0i64, 210, 550, 1_000, 2_000];
        let mut state = 0x2026_0821_1200_0001u64;

        for _ in 0..2_000 {
            let count = (xorshift(&mut state) % 8) as usize + 1;
            let mut lines = Vec::with_capacity(count);
            for _ in 0..count {
                let qty = (xorshift(&mut state) % 20_000) as i64 + 1;
                let price = (xorshift(&mut state) % 500_000) as i64;
                let discount = (xorshift(&mut state) % 5_001) as i64;
                let rate = rates[(xorshift(&mut state) % rates.len() as u64) as usize];
                let line = if xorshift(&mut state) % 2 == 0 {
                    LineInput::new(qty, price, rate)
                } else {
                    LineInput::new_ttc(qty, price, rate)
                };
                lines.push(line.with_discount(discount));
            }

            let t = document_totals(&lines).unwrap();

            assert_eq!(
                t.total_ht_cents + t.total_vat_cents,
                t.total_ttc_cents,
                "HT + VAT must equal TTC"
            );
            assert_eq!(
                t.total_ht_cents,
                t.buckets.iter().map(|b| b.base_ht_cents).sum::<i64>(),
                "buckets must sum to the document net"
            );
            assert_eq!(
                t.total_ht_cents,
                t.lines.iter().map(|l| l.net_ht_cents).sum::<i64>(),
                "lines must sum to the document net"
            );
            for bucket in &t.buckets {
                assert_eq!(
                    bucket.base_ht_cents + bucket.vat_cents,
                    bucket.total_ttc_cents
                );
            }

            let credit = document_totals(&negate(&lines)).unwrap();
            assert_eq!(t.total_ttc_cents, -credit.total_ttc_cents);
        }
    }
}
