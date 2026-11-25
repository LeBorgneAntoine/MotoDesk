//! The accounting reports (port of `desktop/src/lib/api/accounting.ts`).
//!
//! Every figure comes from documents already sealed into the fiscal journal, so these reports
//! cannot disagree with the invoices. Nothing here writes.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::ApiResult;
use crate::session::Client;
use crate::settings::encode;

/// `?from=…&to=…`, each bound left out when it is empty.
fn period(from: &str, to: &str) -> String {
    let bounds: Vec<String> = [("from", from), ("to", to)]
        .into_iter()
        .filter(|(_, day)| !day.trim().is_empty())
        .map(|(name, day)| format!("{name}={}", encode(day.trim())))
        .collect();
    if bounds.is_empty() { String::new() } else { format!("?{}", bounds.join("&")) }
}

// ---- journal des ventes ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SalesJournalRow {
    #[serde(default)]
    pub reference: Option<String>,
    /// `invoice`, `credit_note` or `receipt`.
    #[serde(default)]
    pub document_kind: String,
    #[serde(default)]
    pub issued_on: String,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub total_ht_cents: i64,
    #[serde(default)]
    pub total_vat_cents: i64,
    #[serde(default)]
    pub total_ttc_cents: i64,
    #[serde(default)]
    pub paid_cents: i64,
}

impl SalesJournalRow {
    pub fn kind_label(&self) -> &'static str {
        match self.document_kind.as_str() {
            "credit_note" => "Avoir",
            "receipt" => "Ticket",
            _ => "Facture",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct SalesJournal {
    #[serde(default)]
    pub rows: Vec<SalesJournalRow>,
    #[serde(default)]
    pub total_ht_cents: i64,
    #[serde(default)]
    pub total_vat_cents: i64,
    #[serde(default)]
    pub total_ttc_cents: i64,
}

/// Every sealed document of the period, in the order it was sealed. Drafts and quotes never
/// appear: they have no accounting value.
pub async fn sales_journal(client: &Client, from: &str, to: &str) -> ApiResult<SalesJournal> {
    client.get(format!("/api/accounting/sales-journal{}", period(from, to))).await
}

// ---- journal de caisse -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CashJournalRow {
    pub id: String,
    #[serde(default)]
    pub paid_on: String,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    /// Negative on a reversal.
    #[serde(default)]
    pub amount_cents: i64,
    #[serde(default)]
    pub reverses_payment_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CashJournal {
    #[serde(default)]
    pub rows: Vec<CashJournalRow>,
    #[serde(default)]
    pub total_cents: i64,
    /// What came in by each way of paying.
    #[serde(default)]
    pub by_method: BTreeMap<String, i64>,
}

impl CashJournal {
    /// The ways of paying, the one that brought in the most first.
    pub fn methods_by_amount(&self) -> Vec<(&str, i64)> {
        let mut methods: Vec<(&str, i64)> = self.by_method.iter().map(|(method, cents)| (method.as_str(), *cents)).collect();
        methods.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        methods
    }
}

/// Every receipt of the period, with the split per way of paying, so that the drawer can be
/// counted against what the software says it holds.
pub async fn cash_journal(client: &Client, from: &str, to: &str) -> ApiResult<CashJournal> {
    client.get(format!("/api/accounting/cash-journal{}", period(from, to))).await
}

// ---- TVA ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VatRecapRate {
    pub rate_bp: i64,
    #[serde(default)]
    pub base_ht_cents: i64,
    #[serde(default)]
    pub vat_cents: i64,
    #[serde(default)]
    pub total_ttc_cents: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct VatRecap {
    #[serde(default)]
    pub document_count: i64,
    #[serde(default)]
    pub rates: Vec<VatRecapRate>,
    #[serde(default)]
    pub total_base_ht_cents: i64,
    #[serde(default)]
    pub total_vat_cents: i64,
}

/// VAT collected per rate: the figures that fill the boxes of the CA3 return.
pub async fn vat_recap(client: &Client, from: &str, to: &str) -> ApiResult<VatRecap> {
    client.get(format!("/api/accounting/vat-recap{}", period(from, to))).await
}

// ---- balance âgée ------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AgeingRow {
    pub sale_id: String,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub issued_on: String,
    #[serde(default)]
    pub total_ttc_cents: i64,
    #[serde(default)]
    pub outstanding_cents: i64,
    /// Negative while still within the payment terms.
    #[serde(default)]
    pub days_overdue: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AgeingBuckets {
    #[serde(default)]
    pub not_due_cents: i64,
    #[serde(default)]
    pub overdue_0_30_cents: i64,
    #[serde(default)]
    pub overdue_31_60_cents: i64,
    #[serde(default)]
    pub overdue_61_90_cents: i64,
    #[serde(default)]
    pub overdue_90_plus_cents: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Ageing {
    #[serde(default)]
    pub payment_terms_days: i64,
    #[serde(default)]
    pub rows: Vec<AgeingRow>,
    #[serde(default)]
    pub total_outstanding_cents: i64,
    #[serde(default)]
    pub buckets: AgeingBuckets,
}

/// What is still owed today, by how late it is.
pub async fn ageing(client: &Client) -> ApiResult<Ageing> {
    client.get("/api/accounting/ageing").await
}

// ---- FEC ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FecExport {
    /// SIREN + "FEC" + closing date, as LPF art. L47 A I requires.
    pub file_name: String,
    #[serde(default)]
    pub line_count: i64,
    pub content: String,
}

impl FecExport {
    /// The first lines of the file, to look at before saving it.
    pub fn preview(&self, lines: usize) -> Vec<&str> {
        self.content.lines().take(lines).collect()
    }

    /// The file as it is written to disk: plain text, with the mark that keeps accented names
    /// readable when the accountant opens it in a spreadsheet.
    pub fn bytes(&self) -> Vec<u8> {
        let mut bytes = "\u{feff}".as_bytes().to_vec();
        bytes.extend_from_slice(self.content.as_bytes());
        bytes
    }
}

/// The accounting entries of a year, in the format the tax administration imposes.
pub async fn fec_export(client: &Client, year: i32) -> ApiResult<FecExport> {
    client.get(format!("/api/accounting/fec?year={year}")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_period_names_only_the_bounds_it_has() {
        assert_eq!(period("2026-01-01", "2026-12-31"), "?from=2026-01-01&to=2026-12-31");
        assert_eq!(period("", " 2026-12-31 "), "?to=2026-12-31");
        assert_eq!(period("", ""), "");
    }

    #[test]
    fn the_ways_of_paying_come_biggest_first() {
        let journal: CashJournal =
            serde_json::from_str(r#"{"rows":[],"total_cents":900,"by_method":{"card":500,"cash":300,"cheque":500}}"#).unwrap();
        assert_eq!(journal.methods_by_amount(), [("card", 500), ("cheque", 500), ("cash", 300)]);
    }

    #[test]
    fn the_fec_is_saved_with_its_mark_and_previewed_by_its_first_lines() {
        let fec = FecExport { file_name: "123456789FEC20251231.txt".into(), line_count: 2, content: "en-tête\nligne 1\nligne 2\n".into() };
        assert_eq!(fec.preview(2), ["en-tête", "ligne 1"]);
        assert!(fec.bytes().starts_with(&[0xEF, 0xBB, 0xBF]));
        assert!(fec.bytes().ends_with("ligne 2\n".as_bytes()));
    }
}
