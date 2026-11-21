//! Catching up with the PCs of the network: pull what this PC lacks, in both directions.
//!
//! The mesh used to only *push*: a PC broadcast its new changes, and a PC that was off, or had
//! just arrived, had no way to ask for what it missed (the cloud used to fill that gap). Two
//! exchanges, both signed with the network's mesh secret, fix that:
//!
//! - **Changes.** The asking PC sends a *version vector*, the highest sequence number it holds
//!   per originating device. The other PC answers with the changes beyond it, in batches. A new
//!   PC sends an empty vector and receives the company's whole history; a PC that was off sends
//!   its vector and receives only what happened while it was.
//! - **Accounts.** The two PCs swap their accounts and each keeps the newest of every account.
//!
//! The vector is the highest sequence *seen*, so a change received out of order can hide an
//! earlier one that never arrived. The push path re-sends recent changes, which covers it in
//! practice; closing the gap properly would need contiguous prefixes per device.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

use motodesk_core::sync::ChangeLogEntry;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::local_accounts::{self, Account};
use crate::local_server::mesh::{self, PeerInfo};
use crate::local_server::mesh_auth;
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

const BATCH: i64 = 500;
const MAX_BATCHES_PER_PEER: usize = 400;
const TICK: Duration = Duration::from_secs(45);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// What this PC holds, as the highest sequence number per originating device.
pub async fn version_vector(state: &SharedState) -> LocalResult<HashMap<Uuid, i64>> {
    sync_log::version_vector(&state.db).await
}

fn vv_json(vv: &HashMap<Uuid, i64>) -> Value {
    Value::Object(vv.iter().map(|(d, s)| (d.to_string(), json!(s))).collect())
}

// ---- Serving ---------------------------------------------------------------------------------------

/// The changes beyond `vv`, at most `limit`, and whether there may be more.
pub async fn serve_pull(
    state: &SharedState,
    vv: &HashMap<Uuid, i64>,
    limit: i64,
) -> LocalResult<(Vec<ChangeLogEntry>, bool)> {
    let limit = limit.clamp(1, BATCH);
    let mut remaining = limit;
    let mut out = Vec::new();
    for device in sync_log::known_devices(&state.db).await? {
        if remaining <= 0 {
            break;
        }
        let after = vv.get(&device).copied().unwrap_or(0);
        let ops = sync_log::ops_after(&state.db, device, after, remaining).await?;
        remaining -= ops.len() as i64;
        out.extend(ops);
    }
    Ok((out, remaining <= 0))
}

pub async fn serve_accounts(state: &SharedState, incoming: Vec<Account>) -> LocalResult<Vec<Account>> {
    local_accounts::merge_accounts(&state.db, incoming).await?;
    local_accounts::all_accounts(&state.db).await
}

// ---- Asking -------------------------------------------------------------------------------------------

fn url(peer: &PeerInfo, path: &str) -> String {
    let host = if peer.host.contains(':') && !peer.host.starts_with('[') {
        format!("[{}]", peer.host)
    } else {
        peer.host.clone()
    };
    format!("http://{host}:{}{path}", peer.port)
}

/// A request body signed with the mesh secret: the fields, plus `nonce`, `ts` and `sig`.
fn signed(secret: &str, mut fields: serde_json::Map<String, Value>) -> Value {
    let (nonce, ts, sig) = mesh_auth::sign_value(secret, &Value::Object(fields.clone()));
    fields.insert("nonce".into(), nonce.into());
    fields.insert("ts".into(), ts.into());
    fields.insert("sig".into(), sig.into());
    Value::Object(fields)
}

/// Check a signed response and return its fields without the signature.
fn verified(secret: &str, mut body: Value) -> LocalResult<serde_json::Map<String, Value>> {
    let Some(map) = body.as_object_mut() else {
        return Err(LocalError::Unauthorized);
    };
    let nonce = map.remove("nonce").and_then(|v| v.as_str().map(str::to_string));
    let ts = map.remove("ts").and_then(|v| v.as_i64());
    let sig = map.remove("sig").and_then(|v| v.as_str().map(str::to_string));
    let (Some(nonce), Some(ts), Some(sig)) = (nonce, ts, sig) else {
        return Err(LocalError::Unauthorized);
    };
    if !mesh_auth::value_signature_ok(secret, &nonce, ts, &sig, &Value::Object(map.clone())) {
        return Err(LocalError::Unauthorized);
    }
    Ok(std::mem::take(map))
}

async fn secret_of(state: &SharedState) -> LocalResult<String> {
    state
        .mesh_secret
        .read()
        .await
        .clone()
        .filter(|s| !s.is_empty())
        .ok_or(LocalError::Unauthorized)
}

async fn post(state: &SharedState, peer: &PeerInfo, path: &str, body: Value) -> LocalResult<Value> {
    let res = state
        .http
        .post(url(peer, path))
        .timeout(REQUEST_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|e| LocalError::Internal(anyhow::anyhow!("peer unreachable: {e}")))?;
    if !res.status().is_success() {
        return Err(LocalError::Internal(anyhow::anyhow!("peer answered {}", res.status())));
    }
    res.json().await.map_err(|e| LocalError::Internal(e.into()))
}

/// Swap accounts with a peer. Returns how many changed here.
pub async fn exchange_accounts(state: &SharedState, peer: &PeerInfo) -> LocalResult<usize> {
    let secret = secret_of(state).await?;
    let mine = local_accounts::all_accounts(&state.db).await?;
    let mut fields = serde_json::Map::new();
    fields.insert("accounts".into(), serde_json::to_value(&mine).map_err(|e| LocalError::Internal(e.into()))?);
    let reply = verified(&secret, post(state, peer, "/mesh/accounts", signed(&secret, fields)).await?)?;
    let theirs: Vec<Account> = serde_json::from_value(reply.get("accounts").cloned().unwrap_or(Value::Null))
        .map_err(|e| LocalError::Internal(e.into()))?;
    local_accounts::merge_accounts(&state.db, theirs).await
}

/// Pull every change a peer has that this PC lacks. Returns how many were received.
pub async fn pull_from(state: &SharedState, peer: &PeerInfo) -> LocalResult<usize> {
    let secret = secret_of(state).await?;
    let mut received = 0;
    for _ in 0..MAX_BATCHES_PER_PEER {
        let vv = version_vector(state).await?;
        let mut fields = serde_json::Map::new();
        fields.insert("vv".into(), vv_json(&vv));
        fields.insert("limit".into(), json!(BATCH));
        let reply = verified(&secret, post(state, peer, "/mesh/pull", signed(&secret, fields)).await?)?;
        let ops: Vec<ChangeLogEntry> = serde_json::from_value(reply.get("ops").cloned().unwrap_or(Value::Null))
            .map_err(|e| LocalError::Internal(e.into()))?;
        let more = reply.get("more").and_then(|m| m.as_bool()).unwrap_or(false);
        if ops.is_empty() {
            break;
        }
        received += ops.len();
        mesh::apply_batch(state, ops).await?;
        // Nothing new after applying means asking again would get the same batch.
        if !more || version_vector(state).await? == vv {
            break;
        }
    }
    Ok(received)
}

// ---- Keeping up -----------------------------------------------------------------------------------------

/// Catch up with every PC currently in view. Only in network mode; one run at a time.
pub async fn catch_up_all(state: &SharedState) {
    if !local_accounts::is_standalone(&state.db).await {
        return;
    }
    let Ok(_running) = state.catchup_lock.try_lock() else {
        return;
    };
    let peers = state.peers.read().await.clone();
    for peer in peers {
        match exchange_accounts(state, &peer).await {
            Ok(n) if n > 0 => tracing::info!(peer = %peer.host, accounts = n, "accounts updated from a peer"),
            Ok(_) => {}
            Err(e) => tracing::debug!(peer = %peer.host, error = %e, "account exchange failed"),
        }
        match pull_from(state, &peer).await {
            Ok(n) if n > 0 => {
                tracing::info!(peer = %peer.host, changes = n, "caught up with a peer");
                state.notify_sync();
            }
            Ok(_) => {}
            Err(e) => tracing::debug!(peer = %peer.host, error = %e, "catch-up failed"),
        }
    }
}

/// Ask for a catch-up now (a PC joined, or a peer appeared).
pub fn request(state: &SharedState) {
    state.catchup_signal.notify_one();
}

pub fn start(state: SharedState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = state.catchup_signal.notified() => {}
            }
            catch_up_all(&state).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_body_verifies_and_a_tampered_one_does_not() {
        let secret = "network-secret";
        let mut fields = serde_json::Map::new();
        fields.insert("vv".into(), json!({"a": 3}));
        fields.insert("limit".into(), json!(500));
        let body = signed(secret, fields);
        assert!(verified(secret, body.clone()).is_ok());
        assert!(verified("another-secret", body.clone()).is_err());

        let mut forged = body;
        forged["limit"] = json!(1);
        assert!(verified(secret, forged).is_err());
    }

    #[test]
    fn an_unsigned_body_is_refused() {
        assert!(verified("s", json!({"ops": []})).is_err());
        assert!(verified("s", json!(null)).is_err());
    }
}
