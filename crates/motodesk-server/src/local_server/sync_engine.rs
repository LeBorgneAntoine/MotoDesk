//! Background sync engine: drives the outbox to LAN peers and the cloud.
//!
//! Design:
//! - The local API writes are durable as soon as they hit SQLite. The engine
//!   never sits on the request path; instead, every write calls
//!   [`LocalState::notify_sync`] which wakes the engine.
//! - The engine is a single long-running task that selects between three
//!   signals: a `Notify` from local writes (debounced ~1.5 s), a 60 s safety
//!   tick, and a 30 s cloud pull tick.
//! - LAN broadcast and cloud push run **concurrently and independently**: a
//!   row stays `synced_to_cloud = 0` until the cloud ACKs, so even if the
//!   LAN peer accepted it we eventually relay to the cloud as well.
//! - Cloud failures use a persisted exponential backoff window so the engine
//!   stops hammering an unreachable backend.

use std::time::Duration;

use chrono::{DateTime, Utc};
use motodesk_core::sync::{SyncPullResponse, SyncPushRequest};
use tokio::time::{sleep, timeout, Instant};
use uuid::Uuid;

use crate::local_server::mesh;
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

/// How long to wait after a local write before flushing (coalesces bursts).
const DEBOUNCE: Duration = Duration::from_millis(1_500);
/// Safety tick: even if no writes happened, drain anything that previously
/// failed to push (covers offline → online recovery).
const SAFETY_TICK: Duration = Duration::from_secs(60);
/// How often to poll the cloud for changes pushed by other devices.
const PULL_TICK: Duration = Duration::from_secs(30);
/// Max ops per push batch.
const PUSH_BATCH: i64 = 200;
/// Most batches one drain sends: 20 000 changes. More waits for the next tick.
const MAX_BATCHES_PER_DRAIN: usize = 100;
/// Backoff ladder for repeated cloud failures (seconds).
const BACKOFF_LADDER_SECS: &[u64] = &[1, 2, 4, 8, 30, 60, 300];

pub fn spawn(state: SharedState) {
    tokio::spawn(async move {
        run(state).await;
    });
}

async fn run(state: SharedState) {
    let mut safety = tokio::time::interval(SAFETY_TICK);
    safety.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    safety.tick().await; // skip first immediate tick

    let mut pull = tokio::time::interval(PULL_TICK);
    pull.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    pull.tick().await;

    // Kick once on boot to drain anything left over from last run.
    state.sync_signal.notify_one();

    let mut consecutive_failures: u32 = 0;

    loop {
        tokio::select! {
            _ = state.sync_signal.notified() => {
                // Debounce: keep absorbing notifies for up to DEBOUNCE total.
                let deadline = Instant::now() + DEBOUNCE;
                loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() { break; }
                    if timeout(remaining, state.sync_signal.notified()).await.is_err() {
                        break;
                    }
                }
                drain(&state, &mut consecutive_failures).await;
            }
            _ = safety.tick() => {
                drain(&state, &mut consecutive_failures).await;
            }
            _ = pull.tick() => {
                if let Err(e) = pull_remote(&state).await {
                    tracing::debug!(error = %e, "cloud pull failed");
                }
            }
        }
    }
}

async fn drain(state: &SharedState, consecutive_failures: &mut u32) {
    let lan_fut = drain_lan(state);
    let cloud_fut = drain_cloud(state, consecutive_failures);
    let (_lan, _cloud) = tokio::join!(lan_fut, cloud_fut);
    prune_change_log(state).await;
}

async fn prune_change_log(state: &SharedState) {
    let cutoff = (Utc::now() - chrono::Duration::days(90)).to_rfc3339();
    let _ = sqlx::query("DELETE FROM change_log WHERE synced_to_cloud = 1 AND created_at < $1")
        .bind(cutoff)
        .execute(&state.db)
        .await;
}

async fn drain_lan(state: &SharedState) {
    let peers = state.peers.read().await.clone();
    if peers.is_empty() {
        return;
    }
    // Without a cloud nothing is ever acknowledged, so "not synced yet" never shrinks and the
    // same oldest changes would be sent forever. A network PC pushes from a cursor instead;
    // what a peer misses it pulls (`mesh::catchup`).
    if crate::local_server::local_accounts::is_standalone(&state.db).await {
        match sync_log::unpushed_lan_ops(&state.db, PUSH_BATCH).await {
            Ok((ops, last)) if !ops.is_empty() => {
                mesh::broadcast_ops_to_peers(state, &peers, ops).await;
                let _ = sync_log::mark_lan_pushed(&state.db, last).await;
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "lan drain: read outbox failed"),
        }
        return;
    }
    // Mesh broadcast is best-effort; we use ops that haven't been cloud-acked
    // yet so newly-joined peers can catch up on previously-missed writes.
    let ops = match sync_log::unsynced_ops(&state.db, PUSH_BATCH).await {
        Ok(ops) => ops,
        Err(e) => {
            tracing::warn!(error = %e, "lan drain: read outbox failed");
            return;
        }
    };
    if ops.is_empty() {
        return;
    }
    mesh::broadcast_ops_to_peers(state, &peers, ops).await;
    let _ = sqlx::query("UPDATE sync_state SET last_lan_push_at = $1 WHERE id = 1")
        .bind(Utc::now().to_rfc3339())
        .execute(&state.db)
        .await;
}

async fn drain_cloud(state: &SharedState, consecutive_failures: &mut u32) {
    if let Some(until) = read_backoff_until(state).await {
        if Utc::now() < until {
            tracing::trace!(?until, "cloud drain skipped (backoff)");
            return;
        }
    }

    let token = match session_token(state).await {
        Ok(Some(t)) => t,
        _ => {
            tracing::trace!("cloud drain skipped (no session token)");
            return;
        }
    };

    let device_id = match state.device_id().await {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(error = %e, "cloud drain: missing device id");
            return;
        }
    };

    let base = crate::local_server::cloud::cloud_base_url(state).await;

    // Once per run, before the first push: make sure this PC's numbering starts past
    // what the cloud already holds from it (a restored backup does not).
    if !SEQUENCE_ALIGNED.load(std::sync::atomic::Ordering::Relaxed) {
        match align_with_cloud(state, &base, &token, device_id).await {
            Ok(()) => SEQUENCE_ALIGNED.store(true, std::sync::atomic::Ordering::Relaxed),
            Err(msg) => {
                record_failure(state, &msg, consecutive_failures).await;
                return;
            }
        }
    }

    // Bounded, so a refusal that could not be recorded cannot make this spin.
    for _ in 0..MAX_BATCHES_PER_DRAIN {
        let ops = match sync_log::due_ops(&state.db, PUSH_BATCH).await {
            Ok(ops) => ops,
            Err(e) => {
                tracing::warn!(error = %e, "cloud drain: read outbox failed");
                record_failure(state, &format!("read outbox: {e}"), consecutive_failures).await;
                return;
            }
        };
        if ops.is_empty() {
            record_success(state, consecutive_failures).await;
            // Cloud is healthy: clear cached reachability flag too.
            let _ = sqlx::query(
                "UPDATE sync_state SET cloud_reachable = 1, cloud_checked_at = $1 WHERE id = 1",
            )
            .bind(Utc::now().to_rfc3339())
            .execute(&state.db)
            .await;
            return;
        }

        let ids: Vec<Uuid> = ops.iter().map(|o| o.id).collect();
        let req = SyncPushRequest {
            device_id,
            ops,
            relay: false,
        };
        let url = format!("{base}/api/sync/push");

        let res = state
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&req)
            .timeout(Duration::from_secs(15))
            .send()
            .await;

        match res {
            Ok(r) if r.status().is_success() => {
                // The cloud says which operations it could not apply. Those stay
                // in the outbox: marking them as delivered lost them for good.
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let rejected: std::collections::HashSet<Uuid> = body
                    .get("rejected")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                            .collect()
                    })
                    .unwrap_or_default();
                let accepted: Vec<Uuid> =
                    ids.iter().filter(|id| !rejected.contains(id)).copied().collect();
                if let Err(e) = sync_log::mark_synced(&state.db, &accepted).await {
                    tracing::warn!(error = %e, "mark_synced failed");
                }
                record_success(state, consecutive_failures).await;
                if !rejected.is_empty() {
                    // A refused change steps aside, with its reason, and is tried again
                    // later; the rest of the queue is not held up behind it.
                    let reasons = body.get("rejected_reasons");
                    let refusals: Vec<(Uuid, String)> = rejected
                        .iter()
                        .map(|id| {
                            let why = reasons
                                .and_then(|r| r.get(id.to_string()))
                                .and_then(|v| v.as_str())
                                .unwrap_or("refusé par le cloud")
                                .to_string();
                            (*id, why)
                        })
                        .collect();
                    tracing::warn!(
                        rejected = refusals.len(),
                        first = %refusals[0].1,
                        "cloud refused some changes; they wait and are tried again later"
                    );
                    if let Err(e) = sync_log::record_refusals(&state.db, &refusals).await {
                        tracing::warn!(error = %e, "could not record the refusals");
                        return;
                    }
                }
                if ids.len() < PUSH_BATCH as usize {
                    return;
                }
                // More to send: loop and drain next batch.
                continue;
            }
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                let msg = format!("cloud push HTTP {status}: {body}");
                tracing::warn!(%msg, "cloud push rejected");
                record_failure(state, &msg, consecutive_failures).await;
                return;
            }
            Err(e) => {
                let msg = format!("cloud push transport: {e}");
                tracing::debug!(%msg, "cloud push transport error");
                record_failure(state, &msg, consecutive_failures).await;
                return;
            }
        }
    }
}

async fn pull_remote(state: &SharedState) -> anyhow::Result<()> {
    let token = match session_token(state).await? {
        Some(t) => t,
        None => return Ok(()),
    };
    let device_id = state.device_id().await?;
    let base = crate::local_server::cloud::cloud_base_url(state).await;

    let mut cursor: Option<String> =
        sqlx::query_scalar("SELECT last_cloud_cursor FROM sync_state WHERE id = 1")
            .fetch_optional(&state.db)
            .await?
            .flatten();

    // Drain has_more=true so we never silently lag.
    for _ in 0..50 {
        let mut url = format!("{base}/api/sync/pull?device_id={device_id}");
        if let Some(c) = &cursor {
            url.push_str(&format!("&since={}", url_encode(c)));
        }
        let res = state
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;

        if !res.status().is_success() {
            anyhow::bail!("cloud pull HTTP {}", res.status());
        }
        let body: SyncPullResponse = res.json().await?;

        if !body.ops.is_empty() {
            mesh::apply_batch(state, body.ops).await?;
        }

        if !body.cursor.is_empty() {
            sqlx::query("UPDATE sync_state SET last_cloud_cursor = $1 WHERE id = 1")
                .bind(&body.cursor)
                .execute(&state.db)
                .await?;
            cursor = Some(body.cursor);
        }

        if !body.has_more {
            break;
        }
    }

    Ok(())
}

/// Manual / on-demand sync trigger (Settings > "Sync now").
pub async fn sync_once(state: &SharedState) -> anyhow::Result<()> {
    // Bypass any backoff window: a manual click should always try.
    sqlx::query("UPDATE sync_state SET cloud_backoff_until = NULL WHERE id = 1")
        .execute(&state.db)
        .await
        .ok();
    let mut failures: u32 = 0;
    let _ = pull_remote(state).await;
    drain(state, &mut failures).await;
    // If we still have unsynced ops, surface as error to caller.
    let (pending,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM change_log WHERE synced_to_cloud = 0")
            .fetch_one(&state.db)
            .await?;
    if pending > 0 {
        anyhow::bail!("{pending} op(s) still pending after sync");
    }
    Ok(())
}

async fn read_backoff_until(state: &SharedState) -> Option<DateTime<Utc>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT cloud_backoff_until FROM sync_state WHERE id = 1")
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    row.and_then(|(s,)| s)
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
}

async fn record_failure(state: &SharedState, msg: &str, failures: &mut u32) {
    *failures = failures.saturating_add(1);
    let idx = (*failures as usize)
        .saturating_sub(1)
        .min(BACKOFF_LADDER_SECS.len() - 1);
    let wait = BACKOFF_LADDER_SECS[idx];
    let until = Utc::now() + chrono::Duration::seconds(wait as i64);
    let _ = sqlx::query(
        r#"
        UPDATE sync_state
        SET last_cloud_push_error = $1,
            cloud_backoff_until = $2,
            cloud_reachable = 0,
            cloud_checked_at = $3
        WHERE id = 1
        "#,
    )
    .bind(msg)
    .bind(until.to_rfc3339())
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;
}

async fn record_success(state: &SharedState, failures: &mut u32) {
    *failures = 0;
    let _ = sqlx::query(
        r#"
        UPDATE sync_state
        SET last_cloud_push_at = $1,
            last_cloud_push_error = NULL,
            cloud_backoff_until = NULL,
            cloud_reachable = 1,
            cloud_checked_at = $1
        WHERE id = 1
        "#,
    )
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;
}

static SEQUENCE_ALIGNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) async fn align_with_cloud(
    state: &SharedState,
    base: &str,
    token: &str,
    device_id: Uuid,
) -> Result<(), String> {
    let res = state
        .http
        .get(format!("{base}/api/sync/device-seq?device_id={device_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("cloud sequence check transport: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("cloud sequence check HTTP {}", res.status()));
    }
    let body: serde_json::Value = res.json().await.unwrap_or_default();
    let cloud_last = body.get("last_seq").and_then(|v| v.as_i64()).unwrap_or(0);
    match crate::local_server::db::align_sequence(&state.db, device_id, cloud_last).await {
        Ok(Some(done)) => {
            tracing::warn!(
                missing = done.missing,
                renumbered = done.renumbered,
                "this database is behind the cloud's record of this PC (restored backup?); numbering moved past it"
            );
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(e) => Err(format!("sequence alignment: {e}")),
    }
}

async fn session_token(state: &SharedState) -> anyhow::Result<Option<String>> {
    crate::local_server::secrets::session_access_token(&state.db).await
}

fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

#[allow(dead_code)]
async fn jitter_sleep(base_ms: u64) {
    let jitter = (Instant::now().elapsed().as_nanos() % 250) as u64;
    sleep(Duration::from_millis(base_ms.saturating_add(jitter))).await;
}
