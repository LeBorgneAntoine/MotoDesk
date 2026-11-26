//! Suppliers and what moves the stock.

use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client as Api;
use crate::settings::encode;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Supplier {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub contact: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SupplierInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// One entry or exit of stock.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StockMovement {
    pub id: String,
    pub product_id: String,
    #[serde(default)]
    pub supplier_id: Option<String>,
    /// `reception`, `adjustment`, `inventory`, `sale`, `credit`.
    pub kind: String,
    /// Signed thousandths of a unit: positive comes in, negative goes out.
    pub quantity_milli: i64,
    #[serde(default)]
    pub unit_cost_ht_cents: Option<i64>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub product_name: Option<String>,
}

/// The movements a person records by hand. Sales and credit notes write their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MovementKind {
    #[default]
    Reception,
    Adjustment,
    Inventory,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MovementInput {
    pub product_id: String,
    pub supplier_id: Option<String>,
    pub kind: MovementKind,
    /// Signed thousandths of a unit.
    pub quantity_milli: i64,
    pub unit_cost_ht_cents: Option<i64>,
    pub reason: Option<String>,
}

/// What a movement's kind is called on screen. An unknown kind is shown as it came.
pub fn movement_kind_label(kind: &str) -> &str {
    match kind {
        "reception" => "Réception",
        "adjustment" => "Ajustement",
        "inventory" => "Inventaire",
        "sale" => "Vente",
        "credit" => "Avoir",
        other => other,
    }
}

pub async fn list_suppliers(api: &Api) -> ApiResult<Vec<Supplier>> {
    api.get("/api/suppliers").await
}

pub async fn create_supplier(api: &Api, input: &SupplierInput) -> ApiResult<Supplier> {
    api.post("/api/suppliers", input).await
}

/// The movements of one product, or of every product, most recent first.
pub async fn list_movements(api: &Api, product_id: Option<&str>) -> ApiResult<Vec<StockMovement>> {
    let qs = product_id.map(|id| format!("?product_id={}", encode(id))).unwrap_or_default();
    api.get(format!("/api/stock-movements{qs}")).await
}

pub async fn create_movement(api: &Api, input: &MovementInput) -> ApiResult<StockMovement> {
    api.post("/api/stock-movements", input).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reception_is_sent_in_the_shape_the_service_reads() {
        let sent = serde_json::to_value(MovementInput {
            product_id: "p1".into(),
            quantity_milli: 2_500,
            unit_cost_ht_cents: Some(1_250),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "product_id": "p1", "supplier_id": null, "kind": "reception",
                "quantity_milli": 2500, "unit_cost_ht_cents": 1250, "reason": null
            })
        );
    }

    #[test]
    fn a_kind_has_its_french_name() {
        assert_eq!(movement_kind_label("reception"), "Réception");
        assert_eq!(movement_kind_label("credit"), "Avoir");
        assert_eq!(movement_kind_label("transfert"), "transfert");
    }
}
