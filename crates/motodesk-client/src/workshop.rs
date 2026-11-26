//! Repair orders (port of `desktop/src/lib/api/workshop.ts`).
//!
//! An order goes through the workshop by hand: waiting, in progress, ready. It becomes
//! "invoiced" only when the invoice made from it is validated, never by hand.

use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client;

/// Where an order stands in the workshop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    #[default]
    Pending,
    InProgress,
    Ready,
    Invoiced,
}

impl Step {
    /// The columns of the board, in order.
    pub const ALL: [Step; 4] = [Step::Pending, Step::InProgress, Step::Ready, Step::Invoiced];

    pub fn label(self) -> &'static str {
        match self {
            Step::Pending => "En attente",
            Step::InProgress => "En cours",
            Step::Ready => "Prêt",
            Step::Invoiced => "Facturé",
        }
    }

    /// The step a card moves forward to by hand. "Facturé" is not one: an order is invoiced
    /// when its invoice is validated, so "Prêt" leads nowhere by hand.
    pub fn next(self) -> Option<Step> {
        match self {
            Step::Pending => Some(Step::InProgress),
            Step::InProgress => Some(Step::Ready),
            Step::Ready | Step::Invoiced => None,
        }
    }

    /// The step a card moves back to by hand. An invoiced order does not move.
    pub fn previous(self) -> Option<Step> {
        match self {
            Step::InProgress => Some(Step::Pending),
            Step::Ready => Some(Step::InProgress),
            Step::Pending | Step::Invoiced => None,
        }
    }
}

/// A part or an hour of labour on an order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WorkshopLine {
    pub id: String,
    #[serde(default)]
    pub product_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    /// Thousandths of a unit: 1500 is 1,5.
    #[serde(default)]
    pub quantity_milli: i64,
    #[serde(default)]
    pub unit_price_ht_cents: i64,
    /// A shelf price (a part from the catalogue); `None` for a line priced HT, as labour is.
    #[serde(default)]
    pub unit_price_ttc_cents: Option<i64>,
    #[serde(default)]
    pub vat_rate_bp: i64,
    #[serde(default)]
    pub discount_bp: i64,
    #[serde(default)]
    pub position: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WorkshopOrder {
    pub id: String,
    /// The invoice made from the order, once there is one.
    #[serde(default)]
    pub sale_id: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub vehicle_id: Option<String>,
    #[serde(default)]
    pub promised_at: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub mileage: Option<i64>,
    #[serde(default)]
    pub fuel_level: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub status: Step,
    /// The technician in charge: an employee's id.
    #[serde(default)]
    pub assigned_to: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub vehicle_label: Option<String>,
    /// Only a single order carries its lines; the board's cards do not.
    #[serde(default)]
    pub lines: Vec<WorkshopLine>,
}

impl WorkshopOrder {
    /// What the order is called, on its card and its panel.
    pub fn title(&self) -> String {
        self.label.clone().filter(|label| !label.trim().is_empty()).unwrap_or_else(|| "Ordre de réparation".to_string())
    }
}

/// What the "Nouvel ordre" dialog sends: an order starts waiting, without lines.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NewOrder {
    pub label: String,
    pub client_id: Option<String>,
    /// `2026-11-05`.
    pub promised_at: Option<String>,
    pub notes: Option<String>,
    pub status: Step,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct WorkshopLineInput {
    /// The line being kept; `None` for a new one.
    pub id: Option<String>,
    pub product_id: Option<String>,
    pub label: Option<String>,
    pub quantity_milli: i64,
    pub unit_price_ht_cents: i64,
    pub unit_price_ttc_cents: Option<i64>,
    pub vat_rate_bp: i64,
    pub discount_bp: i64,
}

/// What the order's form sends. Every field is sent, an empty one as `null`: on an edit the
/// service keeps what is absent and clears what is `null`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OrderEdit {
    pub label: String,
    pub client_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub assigned_to: Option<String>,
    pub mileage: Option<i64>,
    pub fuel_level: Option<String>,
    pub promised_at: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<WorkshopLineInput>,
}

/// The draft invoice an order was turned into.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InvoicedSale {
    pub id: String,
    /// The order already had an open draft: it was brought up to date rather than doubled.
    #[serde(default)]
    pub refreshed_from_order: bool,
}

/// The board: every open order and the recently invoiced ones, without the imported history.
pub async fn list_board(client: &Client) -> ApiResult<Vec<WorkshopOrder>> {
    client.get("/api/workshop-orders?board=true").await
}

/// The repair orders of one customer, the latest first.
pub async fn list_for_client(client: &Client, client_id: &str) -> ApiResult<Vec<WorkshopOrder>> {
    client.get(format!("/api/workshop-orders?client_id={}", crate::settings::encode(client_id))).await
}

pub async fn get(client: &Client, id: &str) -> ApiResult<WorkshopOrder> {
    client.get(format!("/api/workshop-orders/{id}")).await
}

pub async fn create(client: &Client, input: &NewOrder) -> ApiResult<WorkshopOrder> {
    client.post("/api/workshop-orders", input).await
}

pub async fn update(client: &Client, id: &str, input: &OrderEdit) -> ApiResult<WorkshopOrder> {
    client.patch(format!("/api/workshop-orders/{id}"), input).await
}

/// Move a card to another step, touching nothing else of the order.
pub async fn set_step(client: &Client, id: &str, step: Step) -> ApiResult<WorkshopOrder> {
    client.patch(format!("/api/workshop-orders/{id}"), &serde_json::json!({ "status": step })).await
}

pub async fn delete(client: &Client, id: &str) -> ApiResult<()> {
    client.delete(format!("/api/workshop-orders/{id}")).await
}

/// Open the draft invoice for the order's lines. When the order already has an open draft,
/// that draft is brought up to date and returned instead of a second invoice being made.
pub async fn invoice(client: &Client, id: &str) -> ApiResult<InvoicedSale> {
    client.post(format!("/api/workshop-orders/{id}/invoice"), &serde_json::json!({})).await
}

/// Open a repair order from a quote, with its customer, vehicle and lines. The quote takes
/// its number on the way; asking again returns the order that already exists.
pub async fn create_order_from_quote(client: &Client, quote_id: &str) -> ApiResult<WorkshopOrder> {
    client.post(format!("/api/workshop-orders/from-quote/{quote_id}"), &serde_json::json!({})).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_moves_by_hand_between_the_three_workshop_steps_only() {
        assert_eq!(Step::Pending.next(), Some(Step::InProgress));
        assert_eq!(Step::InProgress.next(), Some(Step::Ready));
        assert_eq!(Step::Ready.next(), None, "invoicing is not a move by hand");
        assert_eq!(Step::Invoiced.next(), None);
        assert_eq!(Step::Pending.previous(), None);
        assert_eq!(Step::Ready.previous(), Some(Step::InProgress));
        assert_eq!(Step::Invoiced.previous(), None, "an invoiced order does not move");
    }

    #[test]
    fn steps_go_through_their_stored_names() {
        assert_eq!(serde_json::to_value(Step::InProgress).unwrap(), "in_progress");
        assert_eq!(serde_json::from_str::<Step>("\"invoiced\"").unwrap(), Step::Invoiced);
        assert_eq!(Step::Ready.label(), "Prêt");
    }

    #[test]
    fn an_edit_sends_empty_fields_as_null_so_that_they_are_cleared() {
        let sent = serde_json::to_value(OrderEdit { label: "Révision".into(), ..Default::default() }).unwrap();
        for field in ["client_id", "vehicle_id", "assigned_to", "mileage", "fuel_level", "promised_at", "notes"] {
            assert_eq!(sent.get(field), Some(&serde_json::Value::Null), "{field}");
        }
        assert_eq!(sent["lines"], serde_json::json!([]));
    }

    #[test]
    fn an_order_without_a_name_has_the_usual_one() {
        let order: WorkshopOrder = serde_json::from_str(r#"{"id":"o1","status":"ready","label":"  "}"#).unwrap();
        assert_eq!((order.title().as_str(), order.status), ("Ordre de réparation", Step::Ready));
    }
}
