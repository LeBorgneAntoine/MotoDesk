//! The catalogue: categories, products and their variants (port of
//! `desktop/src/lib/api/products.ts` and `productVariants.ts`).

use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client as Api;
use crate::settings::encode;

// ---- categories -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub position: i64,
    /// How many products are filed under it. Only the list says; elsewhere it is 0.
    #[serde(default)]
    pub product_count: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CategoryInput {
    pub name: String,
    /// Smaller comes first. `None` lets the service place it.
    pub position: Option<i64>,
}

pub async fn list_categories(api: &Api) -> ApiResult<Vec<Category>> {
    api.get("/api/categories").await
}

pub async fn create_category(api: &Api, input: &CategoryInput) -> ApiResult<Category> {
    api.post("/api/categories", input).await
}

pub async fn update_category(api: &Api, id: &str, input: &CategoryInput) -> ApiResult<Category> {
    api.patch(format!("/api/categories/{id}"), input).await
}

pub async fn delete_category(api: &Api, id: &str) -> ApiResult<()> {
    api.delete(format!("/api/categories/{id}")).await
}

// ---- products -------------------------------------------------------------------------------

/// What a product is counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    Piece,
    Liter,
    Meter,
    Kg,
    Hour,
    Service,
}

impl Unit {
    pub const ALL: [Unit; 6] = [Unit::Piece, Unit::Liter, Unit::Meter, Unit::Kg, Unit::Hour, Unit::Service];

    /// As written after a quantity: "12 pièce", "2,5 L".
    pub fn label(self) -> &'static str {
        match self {
            Unit::Piece => "pièce",
            Unit::Liter => "L",
            Unit::Meter => "m",
            Unit::Kg => "kg",
            Unit::Hour => "h",
            Unit::Service => "service",
        }
    }

    pub fn value(self) -> &'static str {
        match self {
            Unit::Piece => "piece",
            Unit::Liter => "liter",
            Unit::Meter => "meter",
            Unit::Kg => "kg",
            Unit::Hour => "hour",
            Unit::Service => "service",
        }
    }

    pub fn from_value(value: &str) -> Self {
        Self::ALL.into_iter().find(|unit| unit.value() == value).unwrap_or_default()
    }
}

/// (value, label) of the vehicle kinds a product can be.
pub const VEHICLE_TYPES: [(&str, &str); 3] = [("moto", "Moto"), ("bicycle", "Vélo"), ("electric", "Électrique")];

pub fn vehicle_type_label(value: &str) -> &str {
    VEHICLE_TYPES.iter().find(|(v, _)| *v == value).map(|(_, label)| *label).unwrap_or(value)
}

/// (label, hundredths of a percent) of the VAT rates the forms offer.
pub const VAT_PRESETS: [(&str, i64); 5] = [("20 %", 2000), ("10 %", 1000), ("5,5 %", 550), ("2,1 %", 210), ("0 %", 0)];

/// One characteristic of a variant: "couleur" / "bleu".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantAttr {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Product {
    pub id: String,
    #[serde(default)]
    pub category_id: Option<String>,

    pub name: String,
    #[serde(default)]
    pub sku: Option<String>,
    #[serde(default)]
    pub brand: Option<String>,
    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub unit: Unit,
    #[serde(default)]
    pub is_service: bool,

    #[serde(default)]
    pub purchase_price_ht_cents: Option<i64>,
    #[serde(default)]
    pub selling_price_ttc_cents: Option<i64>,
    /// Hundredths of a percent: 2000 is 20,00 %.
    #[serde(default)]
    pub vat_rate_bp: i64,

    /// In units; decimals allowed (2,5 L, 0,75 m).
    #[serde(default)]
    pub stock_quantity: f64,
    /// The same in thousandths of a unit, exact.
    #[serde(default)]
    pub stock_milli: i64,
    #[serde(default)]
    pub low_stock_threshold: Option<i64>,

    #[serde(default = "yes")]
    pub is_active: bool,

    #[serde(default)]
    pub vehicle_type: Option<String>,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub cylinder_capacity: Option<i64>,
    #[serde(default)]
    pub vin_or_serial: Option<String>,
    #[serde(default)]
    pub plate_number: Option<String>,
    #[serde(default)]
    pub odometer_value: Option<i64>,
    #[serde(default)]
    pub odometer_unit: Option<String>,

    #[serde(default)]
    pub notes: Option<String>,

    /// `None` on a product of its own or a model. Set on a colour or size of a model.
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub variant_label: Option<String>,
    #[serde(default)]
    pub variant_attrs: Option<Vec<VariantAttr>>,

    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn yes() -> bool {
    true
}

impl Product {
    /// Whether the record says anything about a vehicle.
    pub fn has_vehicle_data(&self) -> bool {
        self.vehicle_type.is_some()
            || self.year.is_some()
            || self.vin_or_serial.is_some()
            || self.plate_number.is_some()
            || self.odometer_value.is_some()
    }
}

/// A line of the catalogue.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProductListItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub sku: Option<String>,
    #[serde(default)]
    pub brand: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub category_name: Option<String>,
    #[serde(default)]
    pub unit: Unit,
    #[serde(default)]
    pub is_service: bool,
    #[serde(default)]
    pub stock_quantity: f64,
    #[serde(default)]
    pub stock_milli: i64,
    #[serde(default)]
    pub low_stock_threshold: Option<i64>,
    #[serde(default)]
    pub selling_price_ttc_cents: Option<i64>,
    #[serde(default)]
    pub vat_rate_bp: i64,
    #[serde(default = "yes")]
    pub is_active: bool,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub variant_label: Option<String>,
    #[serde(default)]
    pub variant_count: i64,
    #[serde(default)]
    pub updated_at: String,
}

/// What a product form sends. Every field is sent, an empty one as `null` (it clears the
/// stored value), except the stock: see [`ProductInput::stock_quantity`].
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProductInput {
    pub name: String,
    pub category_id: Option<String>,
    pub sku: Option<String>,
    pub brand: Option<String>,
    pub description: Option<String>,
    pub unit: Unit,
    pub is_service: bool,

    pub purchase_price_ht_cents: Option<i64>,
    pub selling_price_ttc_cents: Option<i64>,
    pub vat_rate_bp: i64,

    /// Left out when the form did not touch it: the figure on screen may be older than the
    /// shelf (sales and receptions since the form opened), and sending it back would undo them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock_quantity: Option<f64>,
    pub low_stock_threshold: Option<i64>,

    pub is_active: bool,

    pub vehicle_type: Option<String>,
    pub year: Option<i64>,
    pub cylinder_capacity: Option<i64>,
    pub vin_or_serial: Option<String>,
    pub plate_number: Option<String>,
    pub odometer_value: Option<i64>,
    pub odometer_unit: Option<String>,

    pub notes: Option<String>,

    pub parent_id: Option<String>,
    pub variant_label: Option<String>,
    pub variant_attrs: Option<Vec<VariantAttr>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct VariantInput {
    pub attrs: Vec<VariantAttr>,
    /// The catalogue name. Empty lets the service compose it from the model and the attributes.
    pub name: Option<String>,
    pub sku: Option<String>,
    pub stock_quantity: Option<f64>,
    pub selling_price_ttc_cents: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListQuery {
    pub q: String,
    pub category_id: Option<String>,
    pub low_stock: bool,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

pub async fn list(api: &Api, query: &ListQuery) -> ApiResult<Vec<ProductListItem>> {
    let mut params = Vec::new();
    let q = query.q.trim();
    if !q.is_empty() {
        params.push(format!("q={}", encode(q)));
    }
    if let Some(category) = query.category_id.as_deref().filter(|id| !id.is_empty()) {
        params.push(format!("category_id={}", encode(category)));
    }
    if query.low_stock {
        params.push("low_stock=true".to_string());
    }
    if let Some(limit) = query.limit {
        params.push(format!("limit={limit}"));
    }
    if let Some(offset) = query.offset {
        params.push(format!("offset={offset}"));
    }
    let qs = if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) };
    api.get(format!("/api/products{qs}")).await
}

pub async fn get(api: &Api, id: &str) -> ApiResult<Product> {
    api.get(format!("/api/products/{id}")).await
}

pub async fn create(api: &Api, input: &ProductInput) -> ApiResult<Product> {
    api.post("/api/products", input).await
}

pub async fn update(api: &Api, id: &str, input: &ProductInput) -> ApiResult<Product> {
    api.patch(format!("/api/products/{id}"), input).await
}

pub async fn delete(api: &Api, id: &str) -> ApiResult<()> {
    api.delete(format!("/api/products/{id}")).await
}

/// The colours and sizes of a model. The service may list the model itself among them.
pub async fn list_variants(api: &Api, product_id: &str) -> ApiResult<Vec<ProductListItem>> {
    api.get(format!("/api/products/{product_id}/variants")).await
}

pub async fn create_variant(api: &Api, product_id: &str, input: &VariantInput) -> ApiResult<Product> {
    api.post(format!("/api/products/{product_id}/variants"), input).await
}

// ---- rules ----------------------------------------------------------------------------------

/// Whether a product is at or under its alert threshold. A service has no stock; a product
/// without a threshold never alerts.
pub fn is_low_stock(is_service: bool, stock_quantity: f64, low_stock_threshold: Option<i64>) -> bool {
    !is_service && low_stock_threshold.is_some_and(|threshold| stock_quantity <= threshold as f64)
}

/// The pairs worth keeping: trimmed, with both a name and a value.
pub fn sanitize_variant_attrs(attrs: &[VariantAttr]) -> Vec<VariantAttr> {
    attrs
        .iter()
        .map(|a| VariantAttr { name: a.name.trim().to_string(), value: a.value.trim().to_string() })
        .filter(|a| !a.name.is_empty() && !a.value.is_empty())
        .collect()
}

/// "couleur bleu · taille 10": what lists, the till and the composed name show.
pub fn format_variant_label(attrs: &[VariantAttr]) -> String {
    sanitize_variant_attrs(attrs).iter().map(|a| format!("{} {}", a.name, a.value)).collect::<Vec<_>>().join(" · ")
}

/// The catalogue name of a variant: a name typed by hand wins, otherwise
/// "BIKE BMW — couleur bleu", or the model's name alone without attributes.
pub fn variant_product_name(parent_name: &str, attrs: &[VariantAttr], custom_name: Option<&str>) -> String {
    if let Some(custom) = custom_name.map(str::trim).filter(|name| !name.is_empty()) {
        return custom.to_string();
    }
    let label = format_variant_label(attrs);
    let base = parent_name.trim();
    if label.is_empty() { base.to_string() } else { format!("{base} — {label}") }
}

/// The model's name, even when the product is itself a variant of it.
pub fn variant_root_name(name: &str, parent_id: Option<&str>, variant_label: Option<&str>) -> String {
    let base = name.trim();
    if let (Some(_), Some(label)) = (parent_id, variant_label.filter(|label| !label.is_empty())) {
        let stripped = base.strip_suffix(&format!(" — {label}")).map(str::trim).filter(|s| !s.is_empty());
        if let Some(stripped) = stripped {
            return stripped.to_string();
        }
    }
    base.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &str, value: &str) -> VariantAttr {
        VariantAttr { name: name.into(), value: value.into() }
    }

    #[test]
    fn blank_pairs_are_dropped_and_the_rest_trimmed() {
        assert_eq!(
            sanitize_variant_attrs(&[attr(" couleur ", " bleu "), attr("taille", ""), attr("", "10")]),
            vec![attr("couleur", "bleu")]
        );
    }

    #[test]
    fn several_axes_read_the_way_a_till_ticket_would() {
        assert_eq!(format_variant_label(&[attr("couleur", "bleu"), attr("taille", "10")]), "couleur bleu · taille 10");
    }

    #[test]
    fn a_variant_is_named_after_its_model() {
        assert_eq!(variant_product_name("BIKE BMW", &[], None), "BIKE BMW");
        assert_eq!(variant_product_name("BIKE BMW", &[attr("couleur", "bleu")], None), "BIKE BMW — couleur bleu");
        assert_eq!(variant_product_name("BIKE BMW", &[attr("couleur", "bleu")], Some("S1000RR Bleu")), "S1000RR Bleu");
        assert_eq!(variant_product_name("BIKE BMW", &[attr("couleur", "bleu")], Some("  ")), "BIKE BMW — couleur bleu");
    }

    #[test]
    fn the_models_name_is_found_back_from_a_variant() {
        assert_eq!(variant_root_name("BIKE BMW — couleur bleu", Some("p1"), Some("couleur bleu")), "BIKE BMW");
        assert_eq!(variant_root_name("BIKE BMW", None, None), "BIKE BMW");
        assert_eq!(variant_root_name("S1000RR Bleu", Some("p1"), Some("couleur bleu")), "S1000RR Bleu");
    }

    #[test]
    fn a_product_alerts_at_or_under_its_threshold_only() {
        assert!(is_low_stock(false, 5., Some(5)));
        assert!(is_low_stock(false, 0.5, Some(1)));
        assert!(!is_low_stock(false, 6., Some(5)));
        assert!(!is_low_stock(false, 0., None), "no threshold, no alert");
        assert!(!is_low_stock(true, 0., Some(5)), "a service has no stock");
    }

    #[test]
    fn a_form_that_left_the_stock_alone_does_not_send_it() {
        let untouched = serde_json::to_value(ProductInput { name: "Filtre".into(), ..Default::default() }).unwrap();
        assert!(untouched.get("stock_quantity").is_none());
        assert_eq!(untouched["sku"], serde_json::Value::Null, "an empty field is sent, to clear what was stored");
        assert_eq!(untouched["unit"], "piece");

        let corrected =
            serde_json::to_value(ProductInput { name: "Filtre".into(), stock_quantity: Some(2.5), ..Default::default() })
                .unwrap();
        assert_eq!(corrected["stock_quantity"], 2.5);
    }

    #[test]
    fn units_go_through_their_stored_names() {
        for unit in Unit::ALL {
            assert_eq!(Unit::from_value(unit.value()), unit);
            assert_eq!(serde_json::to_value(unit).unwrap(), unit.value());
        }
        assert_eq!(Unit::from_value("carton"), Unit::Piece);
    }
}
