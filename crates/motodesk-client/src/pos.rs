//! The till (port of `desktop/src/lib/api/pos.ts` and of the device half of
//! `desktop/src/lib/pos/store.svelte.ts`): the cash session, the checkout that seals a ticket
//! with its payments, the drawer, the printer, the card terminal, and the tickets put aside.
//!
//! Whether a PC is a till is a choice of that PC. It is kept in a file of its own, never in
//! the shop's settings: those travel to every PC, and would turn each of them into a till.

use std::path::Path;

use axum::http::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::device_store::DeviceFile;
use crate::error::ApiResult;
use crate::products::Product;
use crate::sales::{Sale, SaleLineInput};
use crate::session::Client;
use crate::settings::encode;

// ---- this PC as a till -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TpeMode {
    /// No terminal: every card payment is accepted. For trying the till out.
    #[default]
    Simulator,
    /// A terminal that is not wired to the PC: the cashier confirms what it said.
    Manual,
}

impl TpeMode {
    pub const ALL: [TpeMode; 2] = [TpeMode::Simulator, TpeMode::Manual];

    pub fn id(self) -> &'static str {
        match self {
            TpeMode::Simulator => "simulator",
            TpeMode::Manual => "manual",
        }
    }

    pub fn from_id(id: &str) -> TpeMode {
        if id == "manual" {
            TpeMode::Manual
        } else {
            TpeMode::Simulator
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TpeMode::Simulator => "Simulateur (tests)",
            TpeMode::Manual => "TPE autonome — confirmation manuelle",
        }
    }

    /// As the card dialog names it.
    pub fn short(self) -> &'static str {
        match self {
            TpeMode::Simulator => "simulateur",
            TpeMode::Manual => "saisie manuelle",
        }
    }
}

/// What the till issues for a sale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PosDocument {
    #[default]
    Receipt,
    /// An invoice names its customer: the till then asks for one.
    Invoice,
}

impl PosDocument {
    pub const ALL: [PosDocument; 2] = [PosDocument::Receipt, PosDocument::Invoice];

    pub fn id(self) -> &'static str {
        match self {
            PosDocument::Receipt => "receipt",
            PosDocument::Invoice => "invoice",
        }
    }

    pub fn from_id(id: &str) -> PosDocument {
        if id == "invoice" {
            PosDocument::Invoice
        } else {
            PosDocument::Receipt
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PosDocument::Receipt => "Ticket de caisse (RC)",
            PosDocument::Invoice => "Facture (FA) — client obligatoire",
        }
    }
}

/// The till role of this PC. Same fields, same file as the Svelte interface, so a PC that was
/// a till stays one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PosConfig {
    /// This PC opens on the till.
    pub enabled: bool,
    /// A COM port (`COM3`) or a raw Windows printer path. Empty: nothing is printed.
    pub printer_target: String,
    pub drawer_kick_on_cash: bool,
    pub tpe_mode: TpeMode,
    /// Shown in the quick strip while nothing has been sold yet.
    pub favorite_product_ids: Vec<String>,
    pub default_document: PosDocument,
}

impl Default for PosConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            printer_target: String::new(),
            drawer_kick_on_cash: true,
            tpe_mode: TpeMode::Simulator,
            favorite_product_ids: Vec::new(),
            default_document: PosDocument::Receipt,
        }
    }
}

impl PosConfig {
    /// The printer to send to, when there is one.
    pub fn printer(&self) -> Option<&str> {
        Some(self.printer_target.trim()).filter(|target| !target.is_empty())
    }
}

/// What is typed in the favourites field: identifiers separated by commas, semicolons or spaces.
pub fn parse_favorites(typed: &str) -> Vec<String> {
    typed.split([',', ';', ' ', '\t', '\n']).map(str::trim).filter(|id| !id.is_empty()).map(str::to_string).collect()
}

const CONFIG_FILE: &str = "pos.store.json";
const CONFIG_KEY: &str = "pos.config.v1";

/// Where the till role of this PC is kept between two runs.
#[derive(Debug, Clone)]
pub struct PosConfigStore {
    file: DeviceFile,
}

impl PosConfigStore {
    /// `data_dir` is the app's data folder.
    pub fn in_dir(data_dir: &Path) -> Self {
        Self { file: DeviceFile::in_dir(data_dir, CONFIG_FILE) }
    }

    /// The stored role, or the defaults (not a till) when nothing readable is stored.
    pub fn load(&self) -> PosConfig {
        self.file.get(CONFIG_KEY).unwrap_or_default()
    }

    pub fn save(&self, config: &PosConfig) -> std::io::Result<()> {
        self.file.set(CONFIG_KEY, config)
    }
}

// ---- the cash session --------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PosSession {
    pub id: String,
    #[serde(default)]
    pub opened_at: String,
    #[serde(default)]
    pub opening_float_cents: i64,
}

/// The open session of this till, if there is one.
pub async fn session(client: &Client) -> ApiResult<Option<PosSession>> {
    client.get("/api/pos/session").await
}

pub async fn open_session(client: &Client, opening_float_cents: i64) -> ApiResult<PosSession> {
    client.post("/api/pos/session/open", &serde_json::json!({ "opening_float_cents": opening_float_cents })).await
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct SessionClosed {
    pub expected_cash_cents: i64,
    pub counted_cents: i64,
    pub variance_cents: i64,
    /// The day's closure, when it was asked for and made.
    pub day_closure: Option<Value>,
    /// Why the day could not be closed along with the session (the session is closed either way).
    pub day_closure_error: Option<String>,
}

impl SessionClosed {
    /// "conforme", "excédent" or "manquant".
    pub fn verdict(&self) -> &'static str {
        match self.variance_cents {
            0 => "conforme",
            over if over > 0 => "excédent",
            _ => "manquant",
        }
    }
}

/// Close the session on what was counted in the drawer; `close_day` also closes the day (Z).
pub async fn close_session(client: &Client, counted_cents: i64, close_day: bool) -> ApiResult<SessionClosed> {
    client.post("/api/pos/session/close", &serde_json::json!({ "counted_cents": counted_cents, "close_day": close_day })).await
}

// ---- the checkout ------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PosPayment {
    /// For cash, what the customer handed over: the service works out the change.
    pub amount_cents: i64,
    pub method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cheque_drawer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_ref: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Checkout {
    pub client_id: Option<String>,
    pub kind: PosDocument,
    pub lines: Vec<SaleLineInput>,
    pub payments: Vec<PosPayment>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CheckedOut {
    pub sale: Sale,
    #[serde(default)]
    pub change_cents: i64,
    /// A cash payment was taken: the drawer is to be opened.
    #[serde(default)]
    pub kick_drawer: bool,
}

/// Seal the cart as a ticket (or an invoice) with its payments, in one step.
pub async fn checkout(client: &Client, input: &Checkout) -> ApiResult<CheckedOut> {
    client.post("/api/pos/checkout", input).await
}

// ---- the drawer, the printer, the terminal -----------------------------------------------------

/// Record that the drawer was opened outside a sale. The opening is written to the journal.
pub async fn open_drawer(client: &Client) -> ApiResult<()> {
    client.call(Method::POST, "/api/pos/drawer/open").json(&serde_json::json!({})).done().await
}

/// Pulse the cash drawer through the printer it is plugged into.
pub async fn kick_drawer(client: &Client, target: &str) -> ApiResult<()> {
    client.call(Method::POST, "/api/pos/drawer/kick").json(&serde_json::json!({ "target": target, "kick_drawer": true })).done().await
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Printed {
    /// The ticket as plain text: what was sent to the printer, or what would have been.
    pub text: String,
    pub bytes_len: i64,
}

/// Print the ticket of a sale. Without a `target` nothing leaves the PC: the text comes back.
pub async fn print_ticket(client: &Client, sale_id: &str, target: Option<&str>) -> ApiResult<Printed> {
    let body = serde_json::json!({ "target": target, "kick_drawer": false });
    client.post(format!("/api/pos/print/{}", encode(sale_id)), &body).await
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Charge {
    /// `accepted`, `refused` or `timeout`.
    pub status: String,
    pub auth_ref: Option<String>,
    pub reason: Option<String>,
}

impl Charge {
    /// The authorisation to record with the payment, when the card went through.
    pub fn accepted(&self) -> Option<&str> {
        self.auth_ref.as_deref().filter(|_| self.status == "accepted")
    }
}

/// Ask the terminal for an amount. The cashier confirming it is what `accept` says.
pub async fn charge_card(client: &Client, amount_cents: i64, mode: TpeMode) -> ApiResult<Charge> {
    let body = serde_json::json!({ "amount_cents": amount_cents, "mode": mode, "force": "accept" });
    client.post("/api/pos/terminal/charge", &body).await
}

// ---- tickets put aside, and what sells ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HeldTicket {
    pub id: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub total_ttc_cents: i64,
    #[serde(default)]
    pub updated_at: String,
}

pub async fn held(client: &Client) -> ApiResult<Vec<HeldTicket>> {
    client.get("/api/pos/held").await
}

/// Put a cart aside as an unsealed ticket, to take it up again later.
pub async fn park(client: &Client, lines: &[SaleLineInput]) -> ApiResult<Sale> {
    let body = serde_json::json!({ "document_kind": "receipt", "notes": "Caisse — en attente", "lines": lines });
    client.post("/api/sales", &body).await
}

/// What the till sold most, for its quick strip.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FrequentItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub sku: Option<String>,
    #[serde(default)]
    pub brand: Option<String>,
    #[serde(default)]
    pub is_service: bool,
    #[serde(default)]
    pub selling_price_ttc_cents: Option<i64>,
    #[serde(default)]
    pub vat_rate_bp: i64,
    #[serde(default)]
    pub stock_quantity: f64,
    #[serde(default)]
    pub low_stock_threshold: Option<i64>,
}

pub async fn frequents(client: &Client) -> ApiResult<Vec<FrequentItem>> {
    client.get("/api/pos/frequents").await
}

/// The product a scanned code is, or `None` when no product has that reference.
pub async fn product_by_sku(client: &Client, sku: &str) -> ApiResult<Option<Product>> {
    match client.get(format!("/api/products/by-sku/{}", encode(sku))).await {
        Ok(product) => Ok(Some(product)),
        Err(error) if error.is_not_found() => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_pc_is_not_a_till_until_told_and_keeps_what_it_was_told() {
        let dir = std::env::temp_dir().join(format!("motodesk-pos-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = PosConfigStore::in_dir(&dir);
        assert_eq!(store.load(), PosConfig::default());
        assert!(!store.load().enabled);

        let config = PosConfig {
            enabled: true,
            printer_target: " COM3 ".into(),
            tpe_mode: TpeMode::Manual,
            favorite_product_ids: vec!["a".into(), "b".into()],
            default_document: PosDocument::Invoice,
            ..Default::default()
        };
        store.save(&config).unwrap();
        assert_eq!(store.load(), config);
        assert_eq!(config.printer(), Some("COM3"));
        assert_eq!(PosConfig::default().printer(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_is_the_one_the_svelte_interface_wrote() {
        let dir = std::env::temp_dir().join(format!("motodesk-pos-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).unwrap();
        // As Tauri's store plugin writes it; a field this version does not know is left alone.
        let written = json!({ "pos.config.v1": {
            "enabled": true, "printerTarget": "COM4", "drawerKickOnCash": false, "tpeMode": "manual",
            "favoriteProductIds": ["p1"], "defaultDocument": "invoice", "somethingNew": 1
        }, "other.key": "kept" });
        std::fs::write(dir.join("pos.store.json"), serde_json::to_vec(&written).unwrap()).unwrap();
        let store = PosConfigStore::in_dir(&dir);
        let config = store.load();
        assert!(config.enabled && !config.drawer_kick_on_cash);
        assert_eq!((config.tpe_mode, config.default_document), (TpeMode::Manual, PosDocument::Invoice));
        assert_eq!(config.favorite_product_ids, ["p1"]);

        // Saving keeps what else the file holds.
        store.save(&config).unwrap();
        let after: Value = serde_json::from_slice(&std::fs::read(dir.join("pos.store.json")).unwrap()).unwrap();
        assert_eq!(after["other.key"], "kept");
        assert_eq!(after["pos.config.v1"]["printerTarget"], "COM4");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn favourites_are_typed_however_is_convenient() {
        assert_eq!(parse_favorites(" a, b;c  d\n"), ["a", "b", "c", "d"]);
        assert!(parse_favorites("  ").is_empty());
    }

    #[test]
    fn a_payment_is_sent_without_what_it_does_not_have() {
        let cash = PosPayment { amount_cents: 2000, method: "cash", ..Default::default() };
        assert_eq!(serde_json::to_value(&cash).unwrap(), json!({ "amount_cents": 2000, "method": "cash" }));
        let card = PosPayment { amount_cents: 1290, method: "card", auth_ref: Some("A1".into()), ..Default::default() };
        assert_eq!(serde_json::to_value(&card).unwrap()["auth_ref"], "A1");
        assert_eq!(serde_json::to_value(PosDocument::Invoice).unwrap(), "invoice");
        assert_eq!(serde_json::to_value(TpeMode::Manual).unwrap(), "manual");
    }

    #[test]
    fn a_card_counts_only_when_it_went_through_and_a_count_has_a_verdict() {
        let accepted = Charge { status: "accepted".into(), auth_ref: Some("SIM-1".into()), reason: None };
        assert_eq!(accepted.accepted(), Some("SIM-1"));
        let refused = Charge { status: "refused".into(), auth_ref: Some("x".into()), reason: Some("Refusée".into()) };
        assert_eq!(refused.accepted(), None);
        let closed = |variance_cents| SessionClosed { variance_cents, ..Default::default() };
        assert_eq!((closed(0).verdict(), closed(250).verdict(), closed(-100).verdict()), ("conforme", "excédent", "manquant"));
    }
}
