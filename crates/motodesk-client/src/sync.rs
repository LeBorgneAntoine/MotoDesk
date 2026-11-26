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
    pub workshop_orders: i64,
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

#[derive(Debug, Clone, Deserialize)]
pub struct ForceResyncResult {
    #[serde(default)]
    pub bootstrap: BootstrapResult,
    #[serde(default)]
    pub cloud_sync_ok: bool,
}

impl ForceResyncResult {
    /// One line telling what the import brought in.
    pub fn message(&self) -> String {
        let b = &self.bootstrap;
        let mut line = format!(
            "Import cloud : {} client(s) · {} produit(s) · {} véhicule(s) · {} document(s) · {} OR",
            b.clients, b.products, b.vehicles, b.sales, b.workshop_orders
        );
        if b.skipped > 0 {
            line.push_str(&format!(" · {} ligne(s) ignorée(s)", b.skipped));
        }
        line
    }
}

/// Full re-import from the cloud: slow, for a local database that is empty or damaged.
pub async fn force_import_from_cloud(client: &Client) -> ApiResult<ForceResyncResult> {
    client.call(Method::POST, "/api/sync/force-resync").public().send().await
}

/// Send the changes the cloud refused again now, rather than when their back-off ends.
/// Answers how many are being sent.
pub async fn retry_refused(client: &Client) -> ApiResult<i64> {
    #[derive(Deserialize)]
    struct Retrying {
        #[serde(default)]
        retrying: i64,
    }
    let answer: Retrying = client.call(Method::POST, "/api/sync/retry-refused").send().await?;
    Ok(answer.retrying)
}

/// Another PC of the shop, seen on the local network.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LanPeer {
    pub device_id: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: i64,
}

/// Where the sync of this PC stands, in detail (port of `SyncDiagnostics` in `meshAdmin.ts`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Diagnostics {
    pub device_id: String,
    pub lan_peers: Vec<LanPeer>,
    pub pending_cloud_ops: i64,
    pub last_cloud_push_at: Option<String>,
    pub last_cloud_push_error: Option<String>,
    pub cloud_backoff_until: Option<String>,
    pub last_lan_push_at: Option<String>,
    pub cloud_reachable: bool,
    pub cloud_url: String,
    /// A release build pointing at its own computer: it can sync with nothing.
    pub cloud_url_misconfigured: bool,
    /// Changes the cloud refused, waiting to be tried again; they do not block the others.
    pub refused_ops: i64,
    pub refused_latest: Option<String>,
    /// This installation has no cloud: it is run standalone and can be put online.
    pub standalone: bool,
    /// Whether the PCs of the shop share their key (signed in and synced once).
    pub mesh_secret_set: bool,
}

impl Diagnostics {
    /// The identifier of this PC, cut to what fits a tile.
    pub fn short_device_id(&self) -> String {
        short_id(&self.device_id)
    }
}

/// The first characters of an identifier, with a mark that there is more.
pub fn short_id(id: &str) -> String {
    let start: String = id.chars().take(8).collect();
    if id.chars().count() > 8 { format!("{start}…") } else { start }
}

pub async fn diagnostics(client: &Client) -> ApiResult<Diagnostics> {
    client.call(Method::GET, "/api/sync/diagnostics").public().send().await
}

#[derive(Deserialize)]
struct StaticPeers {
    #[serde(default)]
    peers: Vec<String>,
}

/// The PCs to reach by address, for a network that hides them from one another.
pub async fn static_peers(client: &Client) -> ApiResult<Vec<String>> {
    let answer: StaticPeers = client.call(Method::GET, "/mesh/static-peers").public().send().await?;
    Ok(answer.peers)
}

pub async fn set_static_peers(client: &Client, peers: &[String]) -> ApiResult<Vec<String>> {
    let body = serde_json::json!({ "peers": peers });
    let answer: StaticPeers = client.call(Method::PUT, "/mesh/static-peers").public().json(&body).send().await?;
    Ok(answer.peers)
}

/// What was typed in the list of addresses, one per line or separated by commas.
pub fn parse_static_peers(typed: &str) -> Vec<String> {
    typed.split(['\n', '\r', ',']).map(str::trim).filter(|peer| !peer.is_empty()).map(str::to_string).collect()
}

// ---- a base older than what the cloud holds ---------------------------------------------------

#[derive(Debug, Deserialize)]
struct RestoreGap {
    #[serde(default)]
    missing: i64,
}

/// Changes the cloud holds from this PC that this database lacks: what a restored backup
/// looks like from the inside. 0 when there is nothing to report.
pub async fn restore_gap(client: &Client) -> ApiResult<i64> {
    client.get::<RestoreGap>("/api/sync/restore-gap").await.map(|gap| gap.missing)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RecoveryResult {
    pub recovered: bool,
    pub applied: i64,
    pub closures_restored: i64,
}

impl RecoveryResult {
    /// What to tell the user once it is done.
    pub fn message(&self) -> String {
        if !self.recovered {
            return "Rien à récupérer.".into();
        }
        let closures = if self.closures_restored > 0 { format!(", {} clôture(s) rétablie(s)", self.closures_restored) } else { String::new() };
        format!("{} modification(s) récupérée(s) du cloud{closures}. Vérifiez le registre fiscal (Facturation › Registre).", self.applied)
    }
}

/// Fetch this PC's own changes back from the cloud and rebuild the fiscal register from them.
pub async fn recover_from_cloud(client: &Client) -> ApiResult<RecoveryResult> {
    client.post("/api/sync/recover", &serde_json::json!({})).await
}

/// Go on without recovering: the notice is not shown again for this gap.
pub async fn dismiss_restore_gap(client: &Client) -> ApiResult<()> {
    client.call(Method::POST, "/api/sync/restore-gap/dismiss").done().await
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
