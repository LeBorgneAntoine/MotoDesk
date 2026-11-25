//! The figures and the recent documents of the home screen (port of
//! `desktop/src/lib/api/dashboard.ts`).

use serde::Deserialize;

use crate::error::ApiResult;
use crate::session::Client;

/// The four headline figures.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct DashboardStats {
    pub active_clients: i64,
    pub vehicles_in_workshop: i64,
    pub stock_available: i64,
    /// Sealed documents of the month, credit notes taken off, in cents.
    pub revenue_this_month_cents: i64,

    /// How each figure moved since the previous period. The service may leave them out.
    #[serde(default)]
    pub active_clients_delta: Option<i64>,
    #[serde(default)]
    pub vehicles_in_workshop_delta: Option<i64>,
    #[serde(default)]
    pub stock_available_delta: Option<i64>,
    /// In percent: `8.2` is +8,2 %.
    #[serde(default)]
    pub revenue_this_month_delta_pct: Option<f64>,
}

/// A line of "Activité récente": one sales document.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RecentActivity {
    pub id: String,
    /// The document's number, or the start of its id while it has none (a draft).
    pub doc_number: String,
    /// `DEVIS`, `FACTURE`, `AVOIR` or `TICKET`.
    pub doc_type: String,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub vehicle_summary: Option<String>,
    /// The sale's status as the service stores it: `draft`, `paid`...
    pub status: String,
    /// The day of the sale (`2026-11-05`), or the moment the document was created.
    pub created_at: String,
}

/// What the sale's status is called on screen. An unknown status is shown as it came.
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

pub async fn stats(client: &Client) -> ApiResult<DashboardStats> {
    client.get("/api/dashboard/stats").await
}

/// The documents touched last, most recent first. The service keeps `limit` between 1 and 50.
pub async fn recent(client: &Client, limit: u32) -> ApiResult<Vec<RecentActivity>> {
    client.get(&format!("/api/dashboard/recent?limit={limit}")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figures_are_read_with_or_without_their_trends() {
        let bare: DashboardStats = serde_json::from_str(
            r#"{"active_clients":137,"vehicles_in_workshop":4,"stock_available":812,"revenue_this_month_cents":1284550}"#,
        )
        .unwrap();
        assert_eq!((bare.active_clients, bare.revenue_this_month_cents), (137, 1_284_550));
        assert_eq!((bare.active_clients_delta, bare.revenue_this_month_delta_pct), (None, None));

        let with_trends: DashboardStats = serde_json::from_str(
            r#"{"active_clients":137,"vehicles_in_workshop":4,"stock_available":812,"revenue_this_month_cents":0,
                "active_clients_delta":3,"stock_available_delta":-12,"revenue_this_month_delta_pct":8.2}"#,
        )
        .unwrap();
        assert_eq!(with_trends.active_clients_delta, Some(3));
        assert_eq!(with_trends.stock_available_delta, Some(-12));
        assert_eq!(with_trends.revenue_this_month_delta_pct, Some(8.2));
    }

    #[test]
    fn a_status_has_its_french_name() {
        assert_eq!(status_label("draft"), "Brouillon");
        assert_eq!(status_label("paid"), "Payée");
        assert_eq!(status_label("en attente"), "en attente");
    }
}
