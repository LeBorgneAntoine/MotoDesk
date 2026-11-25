//! Sync with the other PCs and the cloud (port of `desktop/src/lib/api/sync.ts`).

use axum::http::Method;
use serde::Deserialize;

use crate::error::ApiResult;
use crate::session::Client;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SyncStatus {
    pub device_id: String,
    #[serde(default)]
    pub pending_cloud_ops: i64,
    #[serde(default)]
    pub last_cloud_cursor: Option<String>,
    #[serde(default)]
    pub lan_peer_count: i64,
    #[serde(default)]
    pub cloud_reachable: bool,
}

/// What an import from the cloud brought in.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct BootstrapResult {
    pub imported: bool,
    pub clients: i64,
    pub vehicles: i64,
    pub products: i64,
    pub categories: i64,
    pub templates: i64,
    pub sales: i64,
    /// Rows the cloud sent that this device could not store (logged on the device).
    pub skipped: i64,
}

impl BootstrapResult {
    /// One line telling the user what the sign-in brought in.
    pub fn message(result: Option<&Self>) -> String {
        let Some(b) = result else {
            return "Synchronisation cloud indisponible — les données locales restent utilisables.".into();
        };
        if b.imported {
            let parts: Vec<String> = [(b.clients, "client(s)"), (b.products, "produit(s)"), (b.sales, "vente(s)")]
                .into_iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, what)| format!("{n} {what}"))
                .collect();
            return if parts.is_empty() {
                "Données importées du cloud.".into()
            } else {
                format!("Données importées du cloud : {}.", parts.join(", "))
            };
        }
        if b.clients > 0 {
            return "Données locales à jour.".into();
        }
        "Aucune donnée à importer. Créez vos premiers clients ou restaurez une sauvegarde.".into()
    }
}

pub async fn status(client: &Client) -> ApiResult<SyncStatus> {
    client.get("/api/sync/status").await
}

#[derive(Debug, Clone, Deserialize)]
pub struct SyncNowResult {
    #[serde(default)]
    pub cloud_sync_ok: bool,
}

/// Fast push/pull with the cloud (local reads stay instant).
pub async fn sync_now(client: &Client) -> ApiResult<SyncNowResult> {
    client.call(Method::POST, "/api/sync/now").public().send().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_in_message_names_what_came_in() {
        assert!(BootstrapResult::message(None).contains("indisponible"));
        let imported = BootstrapResult { imported: true, clients: 12, sales: 3, ..Default::default() };
        assert_eq!(
            BootstrapResult::message(Some(&imported)),
            "Données importées du cloud : 12 client(s), 3 vente(s)."
        );
        let nothing = BootstrapResult::default();
        assert!(BootstrapResult::message(Some(&nothing)).starts_with("Aucune donnée"));
    }
}
