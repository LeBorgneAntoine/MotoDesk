//! Quotes, invoices, credit notes and payments.
//!
//! A document is freely editable until [`validate`]. After that it is sealed into the fiscal
//! journal and the service refuses every edit and deletion: the interface offers a credit note
//! instead of a form.

use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client as Api;
use crate::settings::encode;

/// A line of a document, as stored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SaleLine {
    pub id: String,
    #[serde(default)]
    pub product_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    /// Thousandths of a unit: 1500 is 1,5.
    #[serde(default)]
    pub quantity_milli: i64,
    #[serde(default)]
    pub unit_price_ht_cents: Option<i64>,
    #[serde(default)]
    pub unit_price_ttc_cents: Option<i64>,
    /// `ttc`: the TTC price is the price of record (a shelf price); `ht`: it is derived.
    #[serde(default)]
    pub price_basis: String,
    /// Hundredths of a percent: 2000 is 20,00 %.
    #[serde(default)]
    pub vat_rate_bp: i64,
    #[serde(default)]
    pub discount_bp: i64,
    #[serde(default)]
    pub line_total_ttc_cents: Option<i64>,
    #[serde(default)]
    pub position: i64,
}

impl SaleLine {
    /// The shelf price, when the line is priced VAT-inclusive.
    pub fn shelf_price_cents(&self) -> Option<i64> {
        if self.price_basis == "ttc" { self.unit_price_ttc_cents } else { None }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Payment {
    pub id: String,
    #[serde(default)]
    pub sale_id: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub paid_at: Option<String>,
    /// Negative on a reversal.
    pub amount_cents: i64,
    #[serde(default)]
    pub outstanding_cents: Option<i64>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub bank: Option<String>,
    #[serde(default)]
    pub cheque_drawer: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub reverses_payment_id: Option<String>,
    #[serde(default)]
    pub fiscal_seq: Option<i64>,
    #[serde(default)]
    pub reference: Option<String>,
}

impl Payment {
    /// A receipt that can still be taken back: positive, and not itself a reversal.
    pub fn is_reversible(&self) -> bool {
        self.amount_cents > 0 && self.reverses_payment_id.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Sale {
    pub id: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub vehicle_id: Option<String>,
    #[serde(default)]
    pub reference: Option<String>,
    /// `draft`, `quote`, `invoice`, `credit_note`, `paid`, `receipt`.
    pub status: String,
    /// `invoice`, `credit_note`, `quote`, `receipt`.
    #[serde(default)]
    pub document_kind: String,
    #[serde(default)]
    pub sale_date: Option<String>,
    #[serde(default)]
    pub total_ht_cents: Option<i64>,
    #[serde(default)]
    pub total_vat_cents: Option<i64>,
    #[serde(default)]
    pub total_ttc_cents: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub fiscal_seq: Option<i64>,
    /// Set once sealed. Its presence is what makes the document read-only.
    #[serde(default)]
    pub locked_at: Option<String>,
    /// On a credit note: the document it corrects.
    #[serde(default)]
    pub corrects_sale_id: Option<String>,
    /// On an invoice made from a quote: the quote.
    #[serde(default)]
    pub source_quote_id: Option<String>,
    /// On a quote: the invoice made from it, while that invoice exists.
    #[serde(default)]
    pub converted_to_sale_id: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub lines: Vec<SaleLine>,
    #[serde(default)]
    pub payments: Vec<Payment>,
    #[serde(default)]
    pub paid_cents: i64,
    #[serde(default)]
    pub outstanding_cents: i64,
}

impl Sale {
    /// A sealed document is read-only for good: the interface must not offer an edit.
    pub fn is_sealed(&self) -> bool {
        self.locked_at.is_some()
    }

    pub fn is_quote(&self) -> bool {
        self.status == "quote"
    }

    pub fn is_credit_note(&self) -> bool {
        self.document_kind == "credit_note"
    }

    /// A quote that has become an invoice is closed: the invoice is what is edited.
    pub fn is_closed_quote(&self) -> bool {
        self.is_quote() && self.converted_to_sale_id.is_some()
    }

    /// Its number, or what it is while it has none.
    pub fn title(&self) -> String {
        self.reference.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| status_label(&self.status).to_string())
    }
}

/// A line of the list of documents.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SaleListItem {
    pub id: String,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub document_kind: String,
    pub status: String,
    #[serde(default)]
    pub sale_date: Option<String>,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub total_ht_cents: Option<i64>,
    #[serde(default)]
    pub total_ttc_cents: Option<i64>,
    #[serde(default)]
    pub paid_cents: i64,
    #[serde(default)]
    pub locked_at: Option<String>,
    #[serde(default)]
    pub updated_at: String,
}

impl SaleListItem {
    /// What is still owed on a sealed document; nothing is owed on a draft.
    pub fn due_cents(&self) -> Option<i64> {
        let due = self.total_ttc_cents.unwrap_or(0) - self.paid_cents;
        (self.locked_at.is_some() && due > 0).then_some(due)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SaleLineInput {
    pub product_id: Option<String>,
    pub label: Option<String>,
    pub quantity_milli: i64,
    pub unit_price_ht_cents: i64,
    /// A shelf price. When present it is the price of record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_price_ttc_cents: Option<i64>,
    pub vat_rate_bp: i64,
    pub discount_bp: i64,
}

/// What a document starts as. It is never created already validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftStatus {
    #[default]
    Draft,
    Quote,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SaleInput {
    pub client_id: Option<String>,
    pub vehicle_id: Option<String>,
    /// `2026-11-05`.
    pub sale_date: Option<String>,
    pub notes: Option<String>,
    pub status: DraftStatus,
    pub lines: Vec<SaleLineInput>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListQuery {
    pub q: String,
    /// One of the statuses, or empty for all.
    pub status: String,
    pub client_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

pub async fn list(api: &Api, query: &ListQuery) -> ApiResult<Vec<SaleListItem>> {
    let mut params = Vec::new();
    let q = query.q.trim();
    if !q.is_empty() {
        params.push(format!("q={}", encode(q)));
    }
    if !query.status.is_empty() {
        params.push(format!("status={}", encode(&query.status)));
    }
    if let Some(client) = query.client_id.as_deref().filter(|id| !id.is_empty()) {
        params.push(format!("client_id={}", encode(client)));
    }
    if let Some(limit) = query.limit {
        params.push(format!("limit={limit}"));
    }
    if let Some(offset) = query.offset {
        params.push(format!("offset={offset}"));
    }
    let qs = if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) };
    api.get(format!("/api/sales{qs}")).await
}

pub async fn get(api: &Api, id: &str) -> ApiResult<Sale> {
    api.get(format!("/api/sales/{id}")).await
}

pub async fn create(api: &Api, input: &SaleInput) -> ApiResult<Sale> {
    api.post("/api/sales", input).await
}

pub async fn update(api: &Api, id: &str, input: &SaleInput) -> ApiResult<Sale> {
    api.patch(format!("/api/sales/{id}"), input).await
}

/// Delete a draft or a quote. A sealed document is refused.
pub async fn delete(api: &Api, id: &str) -> ApiResult<()> {
    api.delete(format!("/api/sales/{id}")).await
}

/// Seal the document. Irreversible: it takes its number, is signed into the journal and can
/// afterwards only be corrected by a credit note.
pub async fn validate(api: &Api, id: &str) -> ApiResult<Sale> {
    api.post(format!("/api/sales/{id}/validate"), &serde_json::json!({})).await
}

/// Give a quote its number. It takes one the first time it is handed to a customer (printed,
/// or turned into an invoice or a repair order), not when its draft is created; asking again
/// changes nothing.
pub async fn issue_quote(api: &Api, id: &str) -> ApiResult<Sale> {
    api.post(format!("/api/sales/{id}/issue-quote"), &serde_json::json!({})).await
}

/// Make a draft invoice from a quote. The quote is kept; asking again returns the draft that
/// already exists.
pub async fn convert_quote(api: &Api, id: &str) -> ApiResult<Sale> {
    api.post(format!("/api/sales/{id}/convert-quote"), &serde_json::json!({})).await
}

/// A sealed credit note cancelling the whole document.
pub async fn create_credit_note(api: &Api, id: &str) -> ApiResult<Sale> {
    api.post(format!("/api/sales/{id}/credit-note"), &serde_json::json!({ "reason": null })).await
}

/// What the journal says of a print.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Reprint {
    /// False for the original, true from the second copy onwards.
    pub duplicata: bool,
    pub copy_number: i64,
    /// `DUPLICATA` on a copy, nothing on the original.
    #[serde(default)]
    pub mention: Option<String>,
    pub journal_seq: i64,
}

/// Write a print of a sealed document into the journal, before the page is drawn: the answer
/// says whether this one is the original or a copy, which must then carry DUPLICATA. Never
/// undone: a print that fails afterwards has still been counted.
pub async fn log_reprint(api: &Api, sale_id: &str) -> ApiResult<Reprint> {
    api.post(format!("/api/sales/{sale_id}/reprint"), &serde_json::json!({})).await
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PaymentInput {
    pub amount_cents: i64,
    /// `cash`, `card`, `cheque`, `transfer`.
    pub method: String,
    pub bank: Option<String>,
    pub cheque_drawer: Option<String>,
    pub notes: Option<String>,
}

pub async fn add_payment(api: &Api, sale_id: &str, input: &PaymentInput) -> ApiResult<Payment> {
    api.post(format!("/api/sales/{sale_id}/payments"), input).await
}

/// Records the opposite movement; the original receipt stays visible.
pub async fn reverse_payment(api: &Api, payment_id: &str) -> ApiResult<Payment> {
    api.post(format!("/api/payments/{payment_id}/reverse"), &serde_json::json!({})).await
}

// ---- words ----------------------------------------------------------------------------------

/// (status, what the list filter calls it).
pub const STATUS_FILTERS: [(&str, &str); 6] = [
    ("draft", "Brouillons"),
    ("quote", "Devis"),
    ("invoice", "Factures"),
    ("receipt", "Tickets"),
    ("paid", "Payées"),
    ("credit_note", "Avoirs"),
];

/// What a status is called on screen. An unknown status is shown as it came.
pub fn status_label(status: &str) -> &str {
    match status {
        "draft" => "Brouillon",
        "quote" => "Devis",
        "invoice" => "Facture",
        "credit_note" => "Avoir",
        "paid" => "Payée",
        "receipt" => "Ticket",
        other => other,
    }
}

/// (value, label) of the ways a payment can be made.
pub const PAYMENT_METHODS: [(&str, &str); 4] =
    [("cash", "Espèces"), ("card", "Carte bancaire"), ("cheque", "Chèque"), ("transfer", "Virement")];

/// What a payment method is called. "À crédit" is not offered any more, but old payments
/// carry it and must still read well.
pub fn payment_method_label(method: Option<&str>) -> &str {
    match method {
        None | Some("") => "—",
        Some("credit") => "À crédit",
        Some(method) => PAYMENT_METHODS.iter().find(|(value, _)| *value == method).map(|(_, label)| *label).unwrap_or(method),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_with_a_shelf_price_sends_it_and_a_free_line_does_not() {
        let free = serde_json::to_value(SaleLineInput {
            label: Some("Main d'œuvre".into()),
            quantity_milli: 1_500,
            unit_price_ht_cents: 6_000,
            vat_rate_bp: 2000,
            ..Default::default()
        })
        .unwrap();
        assert!(free.get("unit_price_ttc_cents").is_none());
        assert_eq!(free["product_id"], serde_json::Value::Null);

        let shelf = serde_json::to_value(SaleLineInput { unit_price_ttc_cents: Some(999), ..Default::default() }).unwrap();
        assert_eq!(shelf["unit_price_ttc_cents"], 999);
    }

    #[test]
    fn a_document_is_sent_as_a_draft_or_a_quote() {
        let sent = serde_json::to_value(SaleInput { status: DraftStatus::Quote, ..Default::default() }).unwrap();
        assert_eq!(sent["status"], "quote");
        assert_eq!(serde_json::to_value(SaleInput::default()).unwrap()["status"], "draft");
    }

    #[test]
    fn only_a_sealed_document_owes_something() {
        let line = |locked: Option<&str>, total: i64, paid: i64| -> SaleListItem {
            serde_json::from_value(serde_json::json!({
                "id": "s", "status": "invoice", "locked_at": locked, "total_ttc_cents": total, "paid_cents": paid
            }))
            .unwrap()
        };
        assert_eq!(line(Some("2026-11-05T10:00:00Z"), 10_000, 4_000).due_cents(), Some(6_000));
        assert_eq!(line(Some("2026-11-05T10:00:00Z"), 10_000, 10_000).due_cents(), None);
        assert_eq!(line(None, 10_000, 0).due_cents(), None, "a draft is not owed yet");
    }

    #[test]
    fn statuses_and_methods_have_their_french_names() {
        assert_eq!(status_label("paid"), "Payée");
        assert_eq!(status_label("parked"), "parked");
        assert_eq!(payment_method_label(Some("cheque")), "Chèque");
        assert_eq!(payment_method_label(Some("credit")), "À crédit");
        assert_eq!(payment_method_label(Some("troc")), "troc");
        assert_eq!(payment_method_label(None), "—");
    }
}
