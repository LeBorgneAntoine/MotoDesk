use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use crate::local_server::assistant::OPENROUTER_API;
use crate::local_server::mesh::PeerInfo;
use crate::local_server::mesh_auth::NonceCache;

pub struct LocalState {
    pub db: SqlitePool,
    /// Where the local database lives. Fiscal archives are written alongside it
    /// so that a backup of the app folder carries the archives too.
    pub data_dir: PathBuf,
    pub cloud_url: String,
    /// Where the assistant reaches OpenRouter. Always [`OPENROUTER_API`] in the app: no setting,
    /// file or variable changes it. Tests point it at a stand-in on this PC before the state is
    /// shared, so the assistant's routes run without the network.
    pub openrouter_url: String,
    pub local_token: String,
    pub mesh_nonces: NonceCache,
    /// Cloud-issued company mesh secret (LAN trust root). `None` until login.
    pub mesh_secret: RwLock<Option<String>>,
    /// Company this install belongs to. `None` until login.
    pub company_id: RwLock<Option<Uuid>>,
    pub peers: RwLock<Vec<PeerInfo>>,
    pub lan_peer_count: AtomicU32,
    pub local_port: AtomicU16,
    pub mesh_port: AtomicU16,
    pub http: reqwest::Client,
    /// Notified after every local mutation so the sync engine wakes up
    /// (debounced) to drain the outbox to LAN peers and the cloud.
    pub sync_signal: Notify,
    /// Notified when the company identity / mesh secret changes so the mDNS
    /// task can re-advertise the company fingerprint.
    pub mesh_reload: Notify,
    /// Handshakes in flight and failed attempts, for PCs joining this one's network.
    pub join: crate::local_server::mesh::join::JoinState,
    /// Asks the catch-up loop to run now.
    pub catchup_signal: Notify,
    /// Keeps catch-up runs from overlapping.
    pub catchup_lock: tokio::sync::Mutex<()>,
    /// Notified when the shared folder changes here, so the other PCs are told to come and look.
    pub shared_signal: Notify,
}

impl LocalState {
    pub fn new(
        db: SqlitePool,
        data_dir: PathBuf,
        cloud_url: String,
        mesh_secret: Option<String>,
        company_id: Option<Uuid>,
        local_token: String,
    ) -> Self {
        Self {
            db,
            data_dir,
            cloud_url,
            openrouter_url: OPENROUTER_API.into(),
            local_token,
            mesh_nonces: NonceCache::new(),
            mesh_secret: RwLock::new(mesh_secret),
            company_id: RwLock::new(company_id),
            peers: RwLock::new(Vec::new()),
            lan_peer_count: AtomicU32::new(0),
            local_port: AtomicU16::new(0),
            mesh_port: AtomicU16::new(0),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .pool_idle_timeout(std::time::Duration::from_secs(30))
                .tcp_keepalive(std::time::Duration::from_secs(60))
                .build()
                .expect("http client"),
            sync_signal: Notify::new(),
            mesh_reload: Notify::new(),
            join: Default::default(),
            catchup_signal: Notify::new(),
            catchup_lock: tokio::sync::Mutex::new(()),
            shared_signal: Notify::new(),
        }
    }

    pub async fn device_id(&self) -> anyhow::Result<Uuid> {
        crate::local_server::db::device_id(&self.db).await
    }

    pub fn set_peer_count(&self, n: u32) {
        self.lan_peer_count.store(n, Ordering::Relaxed);
    }

    /// Wake the sync engine so it drains the outbox after a local write.
    /// The shared folder changed on this PC.
    pub fn shared_changed(&self) {
        self.shared_signal.notify_one();
    }

    pub fn notify_sync(&self) {
        self.sync_signal.notify_one();
    }

    /// Non-secret company fingerprint advertised on the LAN so instances only
    /// pair with same-company peers. Derived from the company id (a public
    /// value) — never the mesh secret.
    pub async fn company_fingerprint(&self) -> Option<String> {
        self.company_id
            .read()
            .await
            .map(|id| crate::local_server::mesh::fingerprint_for_company(id))
    }

    /// Reload the company id + mesh secret from the DB (after login / sync) and
    /// signal the mDNS task to re-advertise.
    pub async fn reload_company(&self) {
        let secret = crate::local_server::db::current_mesh_secret(&self.db)
            .await
            .ok()
            .flatten();
        *self.mesh_secret.write().await = secret;
        if let Ok(cid) = crate::local_server::db::load_company_id(&self.db).await {
            *self.company_id.write().await = cid;
        }
        self.mesh_reload.notify_waiters();
    }
}

pub type SharedState = Arc<LocalState>;
