//! LAN mesh: mDNS discovery, peer table, and broadcast/apply primitives.
//!
//! The mDNS event loop runs forever, consuming every `ServiceEvent` variant.
//! A peer entry stays alive as long as it re-announces within `PEER_TTL`; on
//! `ServiceRemoved` we drop it immediately. A second task purges stale entries.
//! Static peers (env var + sync_state.lan_peers_static) are merged in so the
//! mesh works even on networks where multicast is blocked.

pub mod catchup;
mod deferred;
pub mod join;
#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod two_pc_tests;
pub mod transport;

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use motodesk_core::sync::{ChangeLogEntry, MeshApplyResponse};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;

const SERVICE_TYPE: &str = "_motodesk._tcp.local.";
const PEER_TTL: Duration = Duration::from_secs(60);
const PRUNE_EVERY: Duration = Duration::from_secs(15);
const PER_PEER_TIMEOUT: Duration = Duration::from_secs(3);

static MDNS_DAEMON: OnceLock<mdns_sd::ServiceDaemon> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub device_id: Uuid,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
struct PeerEntry {
    info: PeerInfo,
    fullname: Option<String>,
    last_seen: Instant,
    is_static: bool,
}

/// Process-wide peer table keyed by device_id so flapping advertisements don't
/// reset the list. Visible via [`SharedState::peers`].
static PEER_TABLE: OnceLock<RwLock<HashMap<Uuid, PeerEntry>>> = OnceLock::new();

fn table() -> &'static RwLock<HashMap<Uuid, PeerEntry>> {
    PEER_TABLE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Every PC seen advertising a company on the LAN, whatever the company: where a PC that has
/// no network yet looks for one to join. Not trusted: joining proves the password to each.
static CANDIDATES: OnceLock<RwLock<HashMap<Uuid, (PeerInfo, Instant)>>> = OnceLock::new();

/// PCs of this company seen speaking another version of the exchanges (`transport::PROTOCOL`):
/// they cannot be talked to until both are up to date, and the screens say so.
static OTHER_VERSION: OnceLock<RwLock<HashMap<Uuid, Instant>>> = OnceLock::new();

fn other_version() -> &'static RwLock<HashMap<Uuid, Instant>> {
    OTHER_VERSION.get_or_init(|| RwLock::new(HashMap::new()))
}

/// How many of the shop's PCs were seen lately with another version.
pub async fn other_version_count() -> usize {
    other_version().read().await.values().filter(|seen| seen.elapsed() < PEER_TTL * 2).count()
}

/// Whether an announced `version` is the one this build speaks.
fn speaks_our_version(announced: Option<&str>) -> bool {
    announced.and_then(|v| v.parse::<u32>().ok()) == Some(transport::PROTOCOL)
}

/// PCs of this company seen holding another network key: their epoch, and when.
static OTHER_KEY: OnceLock<RwLock<HashMap<Uuid, (i64, Instant)>>> = OnceLock::new();

fn other_key() -> &'static RwLock<HashMap<Uuid, (i64, Instant)>> {
    OTHER_KEY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// This PC's key identifier and epoch.
async fn our_key(state: &SharedState) -> (Option<String>, i64) {
    let kid = state.mesh_secret.read().await.as_deref().map(crate::local_server::network_devices::key_id);
    let epoch = crate::local_server::network_devices::key_epoch(&state.db).await.unwrap_or(1);
    (kid, epoch)
}

/// Whether this PC is the one left behind: it has no key at all, or a PC of the shop holds
/// another one at least as recent. (Anyone on the network can claim so; all it does is show the
/// reconnect screen, which proves the other side before taking anything from it.)
pub async fn needs_reconnect(state: &SharedState) -> bool {
    if !crate::local_server::local_accounts::is_standalone(&state.db).await {
        return false;
    }
    let (kid, epoch) = our_key(state).await;
    kid.is_none()
        || other_key()
            .read()
            .await
            .values()
            .any(|(theirs, seen)| *theirs >= epoch && seen.elapsed() < PEER_TTL * 2)
}

/// How many PCs were seen still holding an older key: they wait to be reconnected (a removed
/// PC that is switched on near the shop counts too).
pub async fn behind_count(state: &SharedState) -> usize {
    let (_, epoch) = our_key(state).await;
    other_key()
        .read()
        .await
        .values()
        .filter(|(theirs, seen)| *theirs < epoch && seen.elapsed() < PEER_TTL * 2)
        .count()
}

/// Drop every known peer: after the key changes they have to be seen again.
pub async fn forget_peers(state: &SharedState) {
    let mut tbl = table().write().await;
    tbl.clear();
    refresh_visible_peers(state, &tbl).await;
    drop(tbl);
    state.peers.write().await.clear();
}

fn candidates() -> &'static RwLock<HashMap<Uuid, (PeerInfo, Instant)>> {
    CANDIDATES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// The PCs a new PC may try to join: the ones seen on the LAN lately, then the static peers
/// (for networks that block multicast).
pub async fn join_candidates(state: &SharedState) -> Vec<PeerInfo> {
    let mut out: Vec<PeerInfo> = candidates()
        .read()
        .await
        .values()
        .filter(|(_, seen)| seen.elapsed() < PEER_TTL * 2)
        .map(|(p, _)| p.clone())
        .collect();
    for raw in load_static_peers(&state.db).await {
        if let Some((host, port)) = parse_host_port(&raw) {
            out.push(PeerInfo { device_id: synth_static_id(&host, port), host, port });
        }
    }
    out
}

/// Make a PC a peer right away (the one this PC just joined), without waiting for its next
/// announcement.
pub async fn adopt_peer(state: &SharedState, peer: PeerInfo) {
    let mut tbl = table().write().await;
    tbl.insert(
        peer.device_id,
        PeerEntry { info: peer, fullname: None, last_seen: Instant::now(), is_static: false },
    );
    refresh_visible_peers(state, &tbl).await;
}

/// Boot mDNS discovery + the prune loop + reload static peers.
pub fn start_discovery(state: SharedState, port: u16) {
    catchup::start(state.clone());
    crate::local_server::shared_folder::replication::start(state.clone());
    tokio::spawn(async move {
        if let Err(e) = run_discovery(state.clone(), port).await {
            tracing::warn!(error = %e, "mdns discovery exited");
        }
    });
}

async fn run_discovery(state: SharedState, port: u16) -> anyhow::Result<()> {
    let device_id = state.device_id().await?;

    let daemon = MDNS_DAEMON.get_or_init(|| mdns_sd::ServiceDaemon::new().expect("mdns daemon"));

    // Advertise immediately (with the current company fingerprint, if known)
    // and re-advertise whenever the company changes (login/logout).
    advertise(daemon, &state, device_id, port).await;

    let adv_state = state.clone();
    tokio::spawn(async move {
        loop {
            adv_state.mesh_reload.notified().await;
            advertise(daemon, &adv_state, device_id, port).await;
        }
    });

    // Browse loop: react to every event variant.
    let browse_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = browse_loop(browse_state, daemon, device_id).await {
            tracing::warn!(error = %e, "mdns browse loop exited");
        }
    });

    // Static-peer reload + TTL prune loop.
    let prune_state = state.clone();
    tokio::spawn(async move {
        prune_loop(prune_state).await;
    });

    Ok(())
}

/// (Re)register the mDNS service, embedding a non-secret company fingerprint so
/// peers can filter to same-company instances before any handshake.
async fn advertise(
    daemon: &mdns_sd::ServiceDaemon,
    state: &SharedState,
    device_id: Uuid,
    port: u16,
) {
    let service_name = format!("Motodesk-{}", &device_id.to_string()[..8]);
    let fingerprint = state.company_fingerprint().await.unwrap_or_default();
    let (kid, epoch) = our_key(state).await;
    let properties = [
        ("device_id".to_string(), device_id.to_string()),
        ("company".to_string(), fingerprint.clone()),
        ("version".to_string(), transport::PROTOCOL.to_string()),
        ("kid".to_string(), kid.unwrap_or_default()),
        ("epoch".to_string(), epoch.to_string()),
    ];
    // No address was ever given, and the daemon does not announce a service
    // without one, so no peer could be discovered. `enable_addr_auto` fills in the
    // machine's addresses and keeps them current. The host name is per device: a
    // shared one made every PC claim the same name.
    if let Ok(info) = mdns_sd::ServiceInfo::new(
        SERVICE_TYPE,
        &service_name,
        &format!("{service_name}.local."),
        "",
        port,
        &properties[..],
    )
    .map(|i| i.enable_addr_auto())
    {
        if let Err(e) = daemon.register(info) {
            tracing::warn!(error = %e, "mdns register failed");
        } else {
            tracing::info!(service = %service_name, company = %fingerprint, "mdns advertised");
        }
    }
}

/// Stable, non-secret fingerprint derived from the company id. Safe to broadcast
/// on the LAN; knowing it does not grant mesh access (that needs the secret).
pub fn fingerprint_for_company(company_id: Uuid) -> String {
    let digest = md5_like(&format!("company:{company_id}"));
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

async fn browse_loop(
    state: SharedState,
    daemon: &mdns_sd::ServiceDaemon,
    own_id: Uuid,
) -> anyhow::Result<()> {
    let receiver = daemon.browse(SERVICE_TYPE)?;

    loop {
        let event = match receiver.recv_async().await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "mdns receiver closed");
                return Ok(());
            }
        };

        match event {
            mdns_sd::ServiceEvent::ServiceResolved(info) => {
                let Some(peer_id) = info
                    .get_property_val_str("device_id")
                    .and_then(|s| Uuid::parse_str(s).ok())
                else {
                    continue;
                };
                if peer_id == own_id {
                    continue;
                }
                // Only pair with peers advertising our company fingerprint. If
                // we aren't logged in yet (no fingerprint), pair with no one.
                let our_fp = state.company_fingerprint().await;
                let peer_fp = info.get_property_val_str("company").map(|s| s.to_string());
                // Remember it as somewhere to try to join, whatever its company.
                if peer_fp.as_deref().is_some_and(|f| !f.is_empty()) {
                    // IPv4 first: link-local IPv6 addresses do not reach across a plain URL.
                    let mut addrs: Vec<_> = info.get_addresses().iter().copied().collect();
                    addrs.sort_by_key(|a| a.is_ipv6());
                    if let Some(ip) = addrs.first() {
                        candidates().write().await.insert(
                            peer_id,
                            (
                                PeerInfo { device_id: peer_id, host: ip.to_string(), port: info.get_port() },
                                Instant::now(),
                            ),
                        );
                    }
                }
                match (&our_fp, &peer_fp) {
                    (Some(ours), Some(theirs)) if ours == theirs => {}
                    _ => {
                        tracing::trace!(peer = %peer_id, "skipping peer: different/unknown company");
                        continue;
                    }
                }
                if !speaks_our_version(info.get_property_val_str("version")) {
                    tracing::debug!(peer = %peer_id, "skipping peer: another version of Motodesk");
                    other_version().write().await.insert(peer_id, Instant::now());
                    table().write().await.remove(&peer_id);
                    continue;
                }
                other_version().write().await.remove(&peer_id);
                let (kid, _) = our_key(&state).await;
                let theirs = info.get_property_val_str("kid").filter(|k| !k.is_empty());
                if theirs.is_some() && theirs != kid.as_deref() {
                    let epoch = info.get_property_val_str("epoch").and_then(|e| e.parse().ok()).unwrap_or(1);
                    tracing::debug!(peer = %peer_id, epoch, "skipping peer: another network key");
                    other_key().write().await.insert(peer_id, (epoch, Instant::now()));
                    table().write().await.remove(&peer_id);
                    continue;
                }
                other_key().write().await.remove(&peer_id);
                let addrs: Vec<_> = info.get_addresses().iter().copied().collect();
                let Some(ip) = addrs.first() else { continue };
                let entry = PeerEntry {
                    info: PeerInfo {
                        device_id: peer_id,
                        host: ip.to_string(),
                        port: info.get_port(),
                    },
                    fullname: Some(info.get_fullname().to_string()),
                    last_seen: Instant::now(),
                    is_static: false,
                };
                let mut tbl = table().write().await;
                tbl.insert(peer_id, entry);
                refresh_visible_peers(&state, &tbl).await;
                tracing::debug!(peer = %peer_id, ip = %ip, "mdns peer resolved");
            }
            mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                let mut tbl = table().write().await;
                let drop_ids: Vec<Uuid> = tbl
                    .iter()
                    .filter(|(_, e)| e.fullname.as_deref() == Some(fullname.as_str()))
                    .map(|(id, _)| *id)
                    .collect();
                for id in drop_ids {
                    tbl.remove(&id);
                    tracing::debug!(peer = %id, "mdns peer removed");
                }
                refresh_visible_peers(&state, &tbl).await;
            }
            other => {
                tracing::trace!(?other, "mdns event");
            }
        }
    }
}

async fn prune_loop(state: SharedState) {
    let mut tick = tokio::time::interval(PRUNE_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;

        // Reload static peers from env + sync_state.lan_peers_static.
        let static_peers = load_static_peers(&state.db).await;

        let mut tbl = table().write().await;

        // Drop expired dynamic peers.
        let now = Instant::now();
        tbl.retain(|_, e| e.is_static || now.duration_since(e.last_seen) < PEER_TTL);

        // Reinsert static peers (synthesize a deterministic UUID per host:port).
        for raw in static_peers {
            if let Some((host, port)) = parse_host_port(&raw) {
                let synth_id = synth_static_id(&host, port);
                let entry = PeerEntry {
                    info: PeerInfo {
                        device_id: synth_id,
                        host,
                        port,
                    },
                    fullname: None,
                    last_seen: now,
                    is_static: true,
                };
                tbl.insert(synth_id, entry);
            }
        }

        refresh_visible_peers(&state, &tbl).await;
    }
}

fn parse_host_port(raw: &str) -> Option<(String, u16)> {
    let raw = raw.trim();
    let (host, port) = raw.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    Some((host.to_string(), port))
}

fn synth_static_id(host: &str, port: u16) -> Uuid {
    // Stable namespaced UUID: derive from host+port so reload doesn't churn.
    let bytes = format!("static:{host}:{port}");
    let digest = md5_like(&bytes);
    Uuid::from_bytes(digest)
}

/// Tiny deterministic 16-byte hash so we don't pull md5/blake3 just for IDs.
fn md5_like(s: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let mut h2: u64 = h.rotate_left(13).wrapping_mul(0x9e3779b97f4a7c15);
    for b in s.bytes().rev() {
        h2 ^= b as u64;
        h2 = h2.wrapping_mul(0x100000001b3);
    }
    out[..8].copy_from_slice(&h.to_le_bytes());
    out[8..].copy_from_slice(&h2.to_le_bytes());
    out
}

async fn load_static_peers(db: &SqlitePool) -> Vec<String> {
    let mut out: Vec<String> = std::env::var("MOTODESK_LAN_PEERS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let stored: Option<(Option<String>,)> =
        sqlx::query_as("SELECT lan_peers_static FROM sync_state WHERE id = 1")
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
    if let Some((Some(json),)) = stored {
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(&json) {
            for p in arr {
                if !p.is_empty() {
                    out.push(p);
                }
            }
        }
    }
    out
}

async fn refresh_visible_peers(state: &SharedState, tbl: &HashMap<Uuid, PeerEntry>) {
    let list: Vec<PeerInfo> = tbl.values().map(|e| e.info.clone()).collect();
    state.set_peer_count(list.len() as u32);
    *state.peers.write().await = list;
}

/// Concurrent broadcast to a known set of peers.
///
/// Each peer gets a short, isolated request so one slow peer never holds up
/// the others. The PSK is stamped on the wire so an attacker on the LAN that
/// doesn't know the shop's pairing code can't inject ops.
pub async fn broadcast_ops_to_peers(
    state: &SharedState,
    peers: &[PeerInfo],
    ops: Vec<ChangeLogEntry>,
) {
    if peers.is_empty() || ops.is_empty() {
        return;
    }
    // Without a company mesh secret we can't authenticate to peers; stay quiet.
    let secret = state.mesh_secret.read().await.clone();
    let Some(secret) = secret else {
        return;
    };
    let body = crate::local_server::routes::mesh::apply_request(&secret, ops);
    let body = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(_) => return,
    };

    let mut set: JoinSet<()> = JoinSet::new();
    for peer in peers {
        let client = state.http.clone();
        let url = format!("http://{}:{}/mesh/apply", peer.host, peer.port);
        let body = body.clone();
        let peer_id = peer.device_id;
        set.spawn(async move {
            let res = client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(body)
                .timeout(PER_PEER_TIMEOUT)
                .send()
                .await;
            match res {
                Ok(r) if r.status().is_success() => {
                    tracing::trace!(peer = %peer_id, "mesh ack");
                }
                Ok(r) => {
                    tracing::debug!(peer = %peer_id, status = %r.status(), "mesh non-ack");
                }
                Err(e) => {
                    tracing::debug!(peer = %peer_id, error = %e, "mesh send failed");
                }
            }
        });
    }
    while set.join_next().await.is_some() {}
}

/// Apply a batch of change-log entries to the local SQLite mirror.
///
/// We keep them in `change_log` with `synced_to_cloud = 0` on purpose: any peer
/// that ends up online with the cloud first will relay these ops upstream. The
/// cloud `INSERT ... ON CONFLICT (id) DO NOTHING` makes that idempotent.
pub async fn apply_batch(
    state: &SharedState,
    ops: Vec<ChangeLogEntry>,
) -> LocalResult<MeshApplyResponse> {
    let mut applied = 0u32;
    let mut skipped = 0u32;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;

    for op in ops {
        let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM change_log WHERE id = $1")
            .bind(op.id.to_string())
            .fetch_optional(&mut *tx)
            .await?;

        if exists.is_some() {
            skipped += 1;
            continue;
        }

        // Each op in its own savepoint. One that cannot be applied (a UNIQUE clash,
        // a missing parent row) used to abort the whole batch and the cursor never
        // moved: the same batch was fetched and failed again every 30 seconds, and
        // nothing after it was ever applied. It is set aside, recorded for review,
        // tried again later (`deferred`), and the rest goes through.
        let mut waits = false;
        let mut failure = None;
        if deferred::is_waiting(&mut tx, op.entity_type, op.entity_id).await? {
            waits = true;
        } else {
            let mut savepoint = sqlx::Acquire::begin(&mut tx).await?;
            match apply_op_to_sqlite(&mut savepoint, &op).await {
                Ok(Applied::Done) => savepoint.commit().await?,
                Ok(Applied::WaitsForRow) => {
                    waits = true;
                    savepoint.rollback().await?;
                }
                Err(err) => {
                    waits = true;
                    failure = Some(err.to_string());
                    savepoint.rollback().await?;
                tracing::warn!(
                    entity = %op.entity_type.as_str(),
                    entity_id = %op.entity_id,
                    error = %err,
                    "sync: an incoming change could not be applied yet and was set aside"
                );
                sqlx::query(
                    "INSERT INTO audit_log (id, action, entity_type, entity_id, payload, created_at)
                     VALUES ($1, 'sync_op_rejected', $2, $3, $4, $5)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(op.entity_type.as_str())
                .bind(op.entity_id.to_string())
                .bind(
                    serde_json::json!({
                        "op": op.op.as_str(),
                        "device_id": op.device_id.to_string(),
                        "seq": op.seq,
                        "error": err.to_string(),
                        "payload": op.payload,
                    })
                    .to_string(),
                )
                .bind(chrono::Utc::now().to_rfc3339())
                .execute(&mut *tx)
                .await?;
                }
            }
        }
        if waits {
            deferred::keep(&mut tx, &op, failure.as_deref()).await?;
        }

        sqlx::query(
            r#"
            INSERT INTO change_log (id, device_id, seq, entity_type, entity_id, op, payload, field_meta, created_at, synced_to_cloud, relayed)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0, 0)
            "#,
        )
        .bind(op.id.to_string())
        .bind(op.device_id.to_string())
        .bind(op.seq)
        .bind(op.entity_type.as_str())
        .bind(op.entity_id.to_string())
        .bind(op.op.as_str())
        .bind(serde_json::to_string(&op.payload).unwrap_or_else(|_| "{}".into()))
        .bind(serde_json::to_string(&op.field_meta).unwrap_or_else(|_| "{}".into()))
        .bind(op.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        if waits {
            skipped += 1;
        } else {
            applied += 1;
        }
    }

    // What this batch brought may be what earlier changes were waiting for.
    applied += deferred::retry(&mut tx).await?;

    tx.commit().await?;

    if applied > 0 {
        state.notify_sync();
    }

    Ok(MeshApplyResponse { applied, skipped })
}

/// What became of a change that raised no error.
enum Applied {
    Done,
    /// A deletion of a row this PC has not received yet. Forgetting it would bring the row
    /// back when its creation arrives.
    WaitsForRow,
}

async fn apply_op_to_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    op: &ChangeLogEntry,
) -> LocalResult<Applied> {
    use motodesk_core::sync::OpKind;
    use motodesk_sync::{apply_operation, ApplyOutcome};

    let snapshot = load_snapshot(tx, op.entity_type, op.entity_id).await?;
    let result = apply_operation(snapshot.as_ref(), op)
        .map_err(|e| LocalError::Internal(anyhow::anyhow!(e)))?;

    if !matches!(result.outcome, ApplyOutcome::Applied) {
        return Ok(Applied::Done);
    }

    let known_locally = snapshot.is_some();
    match result.snapshot {
        // A delete is a tombstone, not a row to write back: persisting the
        // snapshot would set `deleted_at` to NULL again and the record would
        // come back to life. A delete for a row we never had has nothing to
        // delete and must not invent an empty one.
        Some(snap) if snap.deleted_at.is_some() => {
            if !known_locally {
                return Ok(Applied::WaitsForRow);
            }
            soft_delete_entity(tx, op.entity_type, op.entity_id, snap.deleted_at.unwrap_or(op.created_at))
                .await?;
        }
        Some(snap) => persist_snapshot(tx, &snap).await?,
        None if op.op == OpKind::Delete => {
            soft_delete_entity(tx, op.entity_type, op.entity_id, op.created_at).await?;
        }
        None => {}
    }

    Ok(Applied::Done)
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::local_server::sync_log::field_meta_for_patch;
    use crate::local_server::test_support::shop;
    use chrono::Duration;
    use motodesk_core::entity::EntityType;
    use motodesk_core::sync::OpKind;
    use serde_json::json;
    use uuid::Uuid;

    fn remote_op(
        entity_type: EntityType,
        entity_id: Uuid,
        kind: OpKind,
        payload: serde_json::Value,
        seq: i64,
    ) -> ChangeLogEntry {
        let device = Uuid::new_v4();
        ChangeLogEntry {
            id: Uuid::new_v4(),
            device_id: device,
            seq,
            entity_type,
            entity_id,
            op: kind,
            field_meta: field_meta_for_patch(&Default::default(), &payload, device),
            payload,
            // Newer than anything written by the fixture, so the remote edit wins.
            created_at: chrono::Utc::now() + Duration::minutes(5),
            synced_to_cloud: false,
            relayed: false,
        }
    }

    #[tokio::test]
    async fn a_remote_edit_of_one_field_leaves_the_numeric_fields_alone() {
        let state = shop().await;
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO products (id, name, unit, selling_price_ttc_cents, purchase_price_ht_cents,
                                   vat_rate_bp, stock_milli, low_stock_threshold,
                                   created_at, updated_at, row_version, field_meta)
             VALUES ($1,'Plaquettes','piece',1999,800,550,7500,2,$2,$2,1,'{}')",
        )
        .bind(id.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();

        let op = remote_op(EntityType::Product, id, OpKind::Update, json!({ "id": id.to_string(), "notes": "Compatible MT-07" }), 1);
        apply_batch(&state, vec![op]).await.unwrap();

        let row: (Option<String>, i64, i64, i64, Option<i64>) = sqlx::query_as(
            "SELECT notes, stock_milli, selling_price_ttc_cents, vat_rate_bp, low_stock_threshold
             FROM products WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(row.0.as_deref(), Some("Compatible MT-07"), "the remote edit is applied");
        assert_eq!(row.1, 7_500, "stock (7,5) must not be reset to 0");
        assert_eq!(row.2, 1999, "price must not be wiped");
        assert_eq!(row.3, 550, "VAT rate must not fall back to 20 %");
        assert_eq!(row.4, Some(2));
    }

    #[tokio::test]
    async fn one_change_that_cannot_be_applied_does_not_block_the_others() {
        let state = shop().await;
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO products (id, name, sku, unit, created_at, updated_at, row_version, field_meta)
             VALUES ($1,'Existant','SKU-1','piece',$2,$2,1,'{}')",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();

        let good_before = Uuid::new_v4();
        let clashing = Uuid::new_v4();
        let good_after = Uuid::new_v4();
        let ops = vec![
            remote_op(EntityType::Client, good_before, OpKind::Insert,
                json!({ "id": good_before.to_string(), "last_name": "Avant" }), 1),
            // Same SKU as an existing, different product: UNIQUE violation.
            remote_op(EntityType::Product, clashing, OpKind::Insert,
                json!({ "id": clashing.to_string(), "name": "Doublon", "sku": "SKU-1", "unit": "piece" }), 2),
            remote_op(EntityType::Client, good_after, OpKind::Insert,
                json!({ "id": good_after.to_string(), "last_name": "Apres" }), 3),
        ];

        let resp = apply_batch(&state, ops).await.expect("the batch as a whole must succeed");
        assert_eq!(resp.applied, 2);
        assert_eq!(resp.skipped, 1);

        let (clients,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clients WHERE last_name IN ('Avant','Apres')")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(clients, 2, "the changes around the bad one were applied");
        let (rejected,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_log WHERE action = 'sync_op_rejected'")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(rejected, 1, "the bad one is recorded for review");
    }

    #[tokio::test]
    async fn a_remote_delete_deletes_and_does_not_resurrect() {
        let state = shop().await;
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO clients (id, last_name, created_at, updated_at, row_version, field_meta)
             VALUES ($1,'Martin',$2,$2,1,'{}')",
        )
        .bind(id.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();

        let del = remote_op(EntityType::Client, id, OpKind::Delete, json!({ "id": id.to_string() }), 1);
        apply_batch(&state, vec![del]).await.unwrap();

        let (deleted,): (Option<String>,) = sqlx::query_as("SELECT deleted_at FROM clients WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert!(deleted.is_some(), "the record is deleted here too");
    }

    #[tokio::test]
    async fn a_remote_delete_of_an_unknown_row_creates_nothing() {
        let state = shop().await;
        let id = Uuid::new_v4();
        let del = remote_op(EntityType::Client, id, OpKind::Delete, json!({ "id": id.to_string() }), 1);
        apply_batch(&state, vec![del]).await.unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clients WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(n, 0, "no ghost row");
    }

    fn sale_op(id: Uuid, kind: OpKind, lines: serde_json::Value, locked: bool, seq: i64) -> ChangeLogEntry {
        let mut payload = json!({
            "id": id.to_string(), "status": if locked { "invoice" } else { "draft" },
            "document_kind": "invoice", "total_ht_cents": 2_500, "total_vat_cents": 500,
            "total_ttc_cents": 3_000, "lines": lines,
        });
        if locked {
            payload["reference"] = json!("FA-2026-C1-00001");
            payload["locked_at"] = json!(chrono::Utc::now().to_rfc3339());
            payload["fiscal_seq"] = json!(2);
        }
        remote_op(EntityType::Sale, id, kind, payload, seq)
    }

    fn line(id: Uuid, label: &str, qty: i64, ttc: Option<i64>) -> serde_json::Value {
        json!({
            "id": id.to_string(), "label": label, "quantity_milli": qty,
            "unit_price_ht_cents": 1_000, "unit_price_ttc_cents": ttc,
            "price_basis": if ttc.is_some() { "ttc" } else { "ht" },
            "vat_rate_bp": 2_000, "discount_bp": 0, "line_total_ttc_cents": 1_200, "position": 0,
        })
    }

    async fn lines_of(state: &SharedState, sale: Uuid) -> Vec<(String, String)> {
        sqlx::query_as("SELECT label, price_basis FROM sale_lines WHERE sale_id = $1 AND deleted_at IS NULL ORDER BY position, label")
            .bind(sale.to_string())
            .fetch_all(&state.db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_received_sale_carries_its_lines() {
        let state = shop().await;
        let sale = Uuid::new_v4();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        apply_batch(&state, vec![sale_op(sale, OpKind::Insert, json!([line(a, "Filtre", 1_000, Some(1_200)), line(b, "Main d'oeuvre", 1_500, None)]), false, 1)])
            .await
            .unwrap();
        let lines = lines_of(&state, sale).await;
        assert_eq!(lines.len(), 2, "the invoice is not empty on this PC");
        assert!(lines.contains(&("Filtre".into(), "ttc".into())), "a shelf price stays a shelf price: {lines:?}");
        assert!(lines.contains(&("Main d'oeuvre".into(), "ht".into())));
    }

    #[tokio::test]
    async fn an_edited_draft_replaces_its_lines_and_a_sealed_one_does_not_change() {
        let state = shop().await;
        let sale = Uuid::new_v4();
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        apply_batch(&state, vec![sale_op(sale, OpKind::Insert, json!([line(a, "Filtre", 1_000, None)]), false, 1)])
            .await
            .unwrap();

        // The draft is edited on the other PC: one line replaced, one added.
        apply_batch(&state, vec![sale_op(sale, OpKind::Update, json!([line(b, "Plaquettes", 2_000, None), line(c, "Huile", 1_000, None)]), false, 2)])
            .await
            .unwrap();
        let labels: Vec<String> = lines_of(&state, sale).await.into_iter().map(|l| l.0).collect();
        assert_eq!(labels.len(), 2, "{labels:?}");
        assert!(!labels.contains(&"Filtre".to_string()), "the removed line is gone");

        // It is then validated there, and that arrives here.
        apply_batch(&state, vec![sale_op(sale, OpKind::Update, json!([line(b, "Plaquettes", 2_000, None), line(c, "Huile", 1_000, None)]), true, 3)])
            .await
            .unwrap();
        let (locked,): (Option<String>,) = sqlx::query_as("SELECT locked_at FROM sales WHERE id = $1")
            .bind(sale.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert!(locked.is_some());
        assert_eq!(lines_of(&state, sale).await.len(), 2);

        // A later, different set of lines for the sealed document is ignored.
        apply_batch(&state, vec![sale_op(sale, OpKind::Update, json!([line(a, "Filtre", 9_000, None)]), true, 4)])
            .await
            .unwrap();
        let labels: Vec<String> = lines_of(&state, sale).await.into_iter().map(|l| l.0).collect();
        assert_eq!(labels.len(), 2, "sealed lines are never replaced: {labels:?}");
        assert!(!labels.contains(&"Filtre".to_string()));
    }

    async fn stock_of(state: &SharedState, product: Uuid) -> i64 {
        let (n,): (i64,) = sqlx::query_as("SELECT stock_milli FROM products WHERE id = $1")
            .bind(product.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        n
    }

    async fn product_with_stock(state: &SharedState, stock: i64) -> Uuid {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO products (id, name, unit, stock_milli, created_at, updated_at, row_version, field_meta)
             VALUES ($1,'Huile','piece',$2,$3,$3,1,'{}')",
        )
        .bind(id.to_string())
        .bind(stock * 1000)
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();
        id
    }

    fn movement_op(id: Uuid, product: Uuid, qty_milli: i64, seq: i64) -> ChangeLogEntry {
        remote_op(
            EntityType::StockMovement,
            id,
            OpKind::Insert,
            json!({ "id": id.to_string(), "product_id": product.to_string(), "kind": "sale", "quantity_milli": qty_milli }),
            seq,
        )
    }

    #[tokio::test]
    async fn a_sale_made_elsewhere_takes_the_stock_down_here_once() {
        let state = shop().await;
        let product = product_with_stock(&state, 10).await;
        let sold = Uuid::new_v4();

        apply_batch(&state, vec![movement_op(sold, product, -2_000, 1)]).await.unwrap();
        assert_eq!(stock_of(&state, product).await, 8_000);

        // The same change arriving again (a retry, or by a second route) counts once.
        apply_batch(&state, vec![movement_op(sold, product, -2_000, 2)]).await.unwrap();
        assert_eq!(stock_of(&state, product).await, 8_000);

        // A reception elsewhere adds.
        apply_batch(&state, vec![movement_op(Uuid::new_v4(), product, 5_000, 3)]).await.unwrap();
        assert_eq!(stock_of(&state, product).await, 13_000);
    }

    /// 2,5 L sold elsewhere is 2,5 L off the shelf here, and 0,5 L is not nothing.
    #[tokio::test]
    async fn a_fraction_sold_elsewhere_takes_exactly_that_fraction_off() {
        let state = shop().await;
        let product = product_with_stock(&state, 10).await;
        apply_batch(&state, vec![movement_op(Uuid::new_v4(), product, -2_500, 1)]).await.unwrap();
        apply_batch(&state, vec![movement_op(Uuid::new_v4(), product, -500, 2)]).await.unwrap();
        assert_eq!(stock_of(&state, product).await, 7_000);
    }

    /// A PC that has not been updated still sends whole units as `stock_quantity`.
    #[tokio::test]
    async fn a_product_from_an_older_device_arrives_with_its_stock_in_units() {
        let state = shop().await;
        let id = Uuid::new_v4();
        let op = remote_op(
            EntityType::Product,
            id,
            OpKind::Insert,
            json!({ "id": id.to_string(), "name": "Bougie", "unit": "piece", "stock_quantity": 12 }),
            1,
        );
        apply_batch(&state, vec![op]).await.unwrap();
        assert_eq!(stock_of(&state, id).await, 12_000);

        let id = Uuid::new_v4();
        let op = remote_op(
            EntityType::Product,
            id,
            OpKind::Insert,
            json!({ "id": id.to_string(), "name": "Câble", "unit": "m", "stock_milli": 2_500 }),
            2,
        );
        apply_batch(&state, vec![op]).await.unwrap();
        assert_eq!(stock_of(&state, id).await, 2_500);
    }

    #[tokio::test]
    async fn an_edited_product_does_not_overwrite_the_stock_it_sees() {
        let state = shop().await;
        let product = product_with_stock(&state, 10).await;
        // Sold here since the other PC last looked: 10 -> 7.
        sqlx::query("UPDATE products SET stock_milli = 7000 WHERE id = $1")
            .bind(product.to_string())
            .execute(&state.db)
            .await
            .unwrap();

        // The other PC renames the product; its payload still says stock 10.
        let op = remote_op(
            EntityType::Product,
            product,
            OpKind::Update,
            json!({ "id": product.to_string(), "name": "Huile 10W40", "unit": "piece", "stock_quantity": 10 }),
            1,
        );
        apply_batch(&state, vec![op]).await.unwrap();
        let (name,): (String,) = sqlx::query_as("SELECT name FROM products WHERE id = $1")
            .bind(product.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(name, "Huile 10W40", "the edit itself is applied");
        assert_eq!(stock_of(&state, product).await, 7_000, "but the sale made here is still counted");
    }

    #[tokio::test]
    async fn another_pcs_journal_is_stored_with_a_register_built_from_its_opening_entry() {
        let state = shop().await;
        let register = Uuid::new_v4().to_string();
        let entry = |event: &str, seq: i64, payload: serde_json::Value| {
            let id = Uuid::new_v4();
            remote_op(
                EntityType::FiscalEntry,
                id,
                OpKind::Insert,
                json!({
                    "id": id.to_string(), "register_id": register, "seq": seq, "event_type": event,
                    "occurred_at": "2026-11-10T09:00:00+00:00", "business_day": "2026-11-10",
                    "payload": payload, "payload_hash": "h", "prev_hash": "p", "chain_hash": format!("c{seq}"),
                    "signature": "s", "total_ttc_cents": 0,
                }),
                seq,
            )
        };
        apply_batch(
            &state,
            vec![
                entry(
                    "register_opened",
                    1,
                    json!({ "register_code": "C7", "public_key": "ab12", "device_id": Uuid::new_v4().to_string() }),
                ),
                entry("document_sealed", 2, json!({ "reference": "FA-2026-C7-00001" })),
            ],
        )
        .await
        .unwrap();

        let (code, key): (String, String) =
            sqlx::query_as("SELECT code, public_key FROM fiscal_registers WHERE id = $1")
                .bind(&register)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!((code.as_str(), key.as_str()), ("C7", "ab12"));
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fiscal_journal WHERE register_id = $1")
            .bind(&register)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(n, 2, "both entries of the other register are kept");
    }

    #[tokio::test]
    async fn an_invoice_and_an_order_remember_the_quote_they_came_from() {
        let state = shop().await;
        let quote = Uuid::new_v4();
        let (invoice, order) = (Uuid::new_v4(), Uuid::new_v4());

        let mut sale = json!({
            "id": invoice.to_string(), "status": "draft", "document_kind": "invoice",
            "total_ttc_cents": 1_200, "source_quote_id": quote.to_string(), "lines": [],
        });
        apply_batch(&state, vec![remote_op(EntityType::Sale, invoice, OpKind::Insert, sale.clone(), 1)])
            .await
            .unwrap();
        // A later change that does not repeat the link must not lose it.
        sale.as_object_mut().unwrap().remove("source_quote_id");
        sale["notes"] = json!("edited");
        apply_batch(&state, vec![remote_op(EntityType::Sale, invoice, OpKind::Update, sale, 2)])
            .await
            .unwrap();
        let (from,): (Option<String>,) = sqlx::query_as("SELECT source_quote_id FROM sales WHERE id = $1")
            .bind(invoice.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(from.as_deref(), Some(quote.to_string().as_str()));

        let wo = json!({
            "id": order.to_string(), "status": "pending", "label": "Devis",
            "source_quote_id": quote.to_string(),
            "lines": [{
                "id": Uuid::new_v4().to_string(), "label": "Filtre", "quantity_milli": 1_000,
                "unit_price_ht_cents": 833, "unit_price_ttc_cents": 999, "vat_rate_bp": 2_000,
            }],
        });
        apply_batch(&state, vec![remote_op(EntityType::WorkshopOrder, order, OpKind::Insert, wo, 3)])
            .await
            .unwrap();
        let (from,): (Option<String>,) =
            sqlx::query_as("SELECT source_quote_id FROM workshop_orders WHERE id = $1")
                .bind(order.to_string())
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(from.as_deref(), Some(quote.to_string().as_str()));
        let (ttc,): (Option<i64>,) =
            sqlx::query_as("SELECT unit_price_ttc_cents FROM workshop_order_lines WHERE workshop_order_id = $1")
                .bind(order.to_string())
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(ttc, Some(999));
    }
}

pub(crate) mod persist;
use persist::{load_snapshot, persist_snapshot, soft_delete_entity};

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn only_the_same_version_of_the_exchanges_is_talked_to() {
        assert!(speaks_our_version(Some(&transport::PROTOCOL.to_string())));
        for other in [None, Some("2"), Some("999"), Some("x")] {
            assert!(!speaks_our_version(other), "{other:?}");
        }
    }
}
