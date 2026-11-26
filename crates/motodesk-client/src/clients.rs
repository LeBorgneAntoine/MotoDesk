//! Customers and their vehicles.

use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client as Api;
use crate::settings::encode;

// ---- types ----------------------------------------------------------------------------------

/// A row of the customer list.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClientListItem {
    pub id: String,
    #[serde(default)]
    pub first_name: Option<String>,
    pub last_name: String,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub vehicle_count: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Client {
    pub id: String,
    #[serde(default)]
    pub first_name: Option<String>,
    pub last_name: String,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub credit_limit_cents: Option<i64>,
    #[serde(default)]
    pub credit_blocked: bool,
    /// What the customer still owes; only on a single customer's page.
    #[serde(default)]
    pub outstanding_cents: Option<i64>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VehicleType {
    #[default]
    Moto,
    Bicycle,
    Electric,
}

impl VehicleType {
    pub const ALL: [VehicleType; 3] = [VehicleType::Moto, VehicleType::Bicycle, VehicleType::Electric];

    pub fn label(self) -> &'static str {
        match self {
            VehicleType::Moto => "Moto",
            VehicleType::Bicycle => "Vélo",
            VehicleType::Electric => "Électrique",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OdometerUnit {
    #[default]
    Km,
    Miles,
    Hours,
}

impl OdometerUnit {
    pub const ALL: [OdometerUnit; 3] = [OdometerUnit::Km, OdometerUnit::Miles, OdometerUnit::Hours];

    pub fn label(self) -> &'static str {
        match self {
            OdometerUnit::Km => "km",
            OdometerUnit::Miles => "mi",
            OdometerUnit::Hours => "h",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Vehicle {
    pub id: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub vehicle_type: VehicleType,
    pub make: String,
    pub model: String,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub cylinder_capacity: Option<i32>,
    #[serde(default)]
    pub vin_or_serial: Option<String>,
    #[serde(default)]
    pub plate_number: Option<String>,
    #[serde(default)]
    pub odometer_value: i64,
    #[serde(default)]
    pub odometer_unit: OdometerUnit,
    #[serde(default)]
    pub is_stock: bool,
    #[serde(default)]
    pub purchase_price_ht_cents: Option<i64>,
    #[serde(default)]
    pub selling_price_ttc_cents: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// A customer with their vehicles.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClientDetail {
    #[serde(flatten)]
    pub client: Client,
    #[serde(default)]
    pub vehicles: Vec<Vehicle>,
}

/// What a customer form sends. `None` clears the field.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ClientInput {
    pub first_name: Option<String>,
    pub last_name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub credit_limit_cents: Option<i64>,
    pub credit_blocked: Option<bool>,
}

/// What a vehicle form sends.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct VehicleInput {
    pub vehicle_type: VehicleType,
    pub make: String,
    pub model: String,
    pub year: Option<i32>,
    pub cylinder_capacity: Option<i32>,
    pub vin_or_serial: Option<String>,
    pub plate_number: Option<String>,
    pub odometer_value: Option<i64>,
    pub odometer_unit: Option<OdometerUnit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_stock: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purchase_price_ht_cents: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selling_price_ttc_cents: Option<i64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListQuery {
    pub q: String,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClientListPage {
    pub items: Vec<ClientListItem>,
    pub total: i64,
}

// ---- calls ----------------------------------------------------------------------------------

pub async fn list(api: &Api, query: &ListQuery) -> ApiResult<ClientListPage> {
    let mut params = Vec::new();
    let q = query.q.trim();
    if !q.is_empty() {
        params.push(format!("q={}", encode(q)));
    }
    if let Some(limit) = query.limit {
        params.push(format!("limit={limit}"));
    }
    if let Some(offset) = query.offset {
        params.push(format!("offset={offset}"));
    }
    let qs = if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) };
    api.get(format!("/api/clients{qs}")).await
}

pub async fn get(api: &Api, id: &str) -> ApiResult<ClientDetail> {
    api.get(format!("/api/clients/{id}")).await
}

pub async fn create(api: &Api, input: &ClientInput) -> ApiResult<Client> {
    api.post("/api/clients", input).await
}

pub async fn update(api: &Api, id: &str, input: &ClientInput) -> ApiResult<Client> {
    api.patch(format!("/api/clients/{id}"), input).await
}

pub async fn delete(api: &Api, id: &str) -> ApiResult<()> {
    api.delete(format!("/api/clients/{id}")).await
}

pub async fn list_vehicles(api: &Api, client_id: &str) -> ApiResult<Vec<Vehicle>> {
    api.get(format!("/api/clients/{client_id}/vehicles")).await
}

pub async fn create_vehicle(api: &Api, client_id: &str, input: &VehicleInput) -> ApiResult<Vehicle> {
    api.post(format!("/api/clients/{client_id}/vehicles"), input).await
}

pub async fn get_vehicle(api: &Api, vehicle_id: &str) -> ApiResult<Vehicle> {
    api.get(format!("/api/vehicles/{vehicle_id}")).await
}

pub async fn update_vehicle(api: &Api, vehicle_id: &str, input: &VehicleInput) -> ApiResult<Vehicle> {
    api.patch(format!("/api/vehicles/{vehicle_id}"), input).await
}

pub async fn delete_vehicle(api: &Api, vehicle_id: &str) -> ApiResult<()> {
    api.delete(format!("/api/vehicles/{vehicle_id}")).await
}

// ---- stock vehicles (Parc VN/VO) ------------------------------------------------------------

/// The vehicles for sale, optionally narrowed by VIN, plate or make.
pub async fn list_stock_vehicles(api: &Api, q: &str) -> ApiResult<Vec<Vehicle>> {
    let q = q.trim();
    let search = if q.is_empty() { String::new() } else { format!("&q={}", encode(q)) };
    api.get(format!("/api/vehicles?is_stock=true{search}")).await
}

/// Add a vehicle to the stock. It has no owner until it is sold.
pub async fn create_stock_vehicle(api: &Api, input: &VehicleInput) -> ApiResult<Vehicle> {
    let input = VehicleInput { is_stock: Some(true), ..input.clone() };
    api.post("/api/vehicles", &input).await
}

/// A vehicle the buyer hands over: a negative line on the invoice, and an entry in the stock.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Reprise {
    pub make: String,
    pub model: String,
    pub vin_or_serial: Option<String>,
    pub plate_number: Option<String>,
    pub purchase_price_ht_cents: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct VehicleSellInput {
    pub client_id: String,
    pub reprise: Option<Reprise>,
}

/// The draft invoice a sale created. The Sales screen holds the rest of it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SaleRef {
    pub id: String,
}

/// Sell a stock vehicle: creates a draft invoice for the buyer and returns it.
pub async fn sell_stock_vehicle(api: &Api, vehicle_id: &str, input: &VehicleSellInput) -> ApiResult<SaleRef> {
    api.post(format!("/api/vehicles/{vehicle_id}/sell"), input).await
}

/// VN/VO margin: selling price TTC minus purchase price HT, in cents. `None` when a price is
/// missing.
pub fn vehicle_margin_cents(purchase_ht_cents: Option<i64>, selling_ttc_cents: Option<i64>) -> Option<i64> {
    Some(selling_ttc_cents? - purchase_ht_cents?)
}

/// Stock prices are TTC; sale lines are HT. `vat_rate_bp` is in basis points (2000 = 20 %).
pub fn ht_cents_from_ttc(ttc_cents: i64, vat_rate_bp: i64) -> i64 {
    if vat_rate_bp <= 0 {
        return ttc_cents;
    }
    (ttc_cents * 10_000 + (10_000 + vat_rate_bp) / 2) / (10_000 + vat_rate_bp)
}

// ---- labels ---------------------------------------------------------------------------------

/// "Jean Dupont", or "Dupont" when there is no first name.
pub fn format_client_name(first_name: Option<&str>, last_name: &str) -> String {
    match first_name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(first) => format!("{first} {last_name}"),
        None => last_name.to_string(),
    }
}

/// "Honda CBR 600 (2018)", or "Honda CBR 600" when the year is unknown.
pub fn format_vehicle_label(make: &str, model: &str, year: Option<i32>) -> String {
    match year {
        Some(year) => format!("{make} {model} ({year})"),
        None => format!("{make} {model}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_vehicle_labels() {
        assert_eq!(format_client_name(Some("Jean"), "Dupont"), "Jean Dupont");
        assert_eq!(format_client_name(None, "Dupont"), "Dupont");
        assert_eq!(format_client_name(Some("  "), "Dupont"), "Dupont");
        assert_eq!(format_vehicle_label("Honda", "CBR 600", Some(2018)), "Honda CBR 600 (2018)");
        assert_eq!(format_vehicle_label("Honda", "CBR 600", None), "Honda CBR 600");
    }

    #[test]
    fn the_margin_is_sell_ttc_minus_buy_ht() {
        assert_eq!(vehicle_margin_cents(Some(800_000), Some(1_200_000)), Some(400_000));
        assert_eq!(vehicle_margin_cents(None, Some(1_200_000)), None);
        assert_eq!(vehicle_margin_cents(Some(100), None), None);
    }

    #[test]
    fn vat_is_backed_out_of_a_ttc_price() {
        assert_eq!(ht_cents_from_ttc(12_000, 2_000), 10_000);
        assert_eq!(ht_cents_from_ttc(12_000, 0), 12_000);
    }
}
