//! The PCs of the shop's local network, removing one, and reconnecting the others.
//!
//! A PC is a member because it holds the network secret. A lost or stolen PC keeps its copy, so
//! it can only be shut out by changing the secret:
//!
//! 1. An administrator removes it from one PC, which asks for their password and a **new** one
//!    (the removed PC holds the old one's hash, and could use it to pose as a member).
//! 2. That PC makes a new secret, one **key epoch** later, and re-wraps the shared folder's
//!    file keys under it.
//! 3. Every other PC is now on an older epoch and is told to reconnect: an administrator signs
//!    in there with the new password, over the same exchange as joining (`mesh::join`), and the
//!    PC receives the new secret. It keeps its data, which then flows both ways again.
//!
//! The new secret is never sent to anyone who has not proved the new password, and a PC only
//! accepts it from one that proves it holds that account too. What removal cannot do is take
//! back what the removed PC already holds.

use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::local_accounts;
use crate::local_server::mesh::{self, join, PeerInfo};
use crate::local_server::state::SharedState;

/// One PC, as listed and as replicated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct Device {
    pub device_id: String,
    pub name: String,
    pub first_seen: String,
    pub last_seen: String,
    pub removed_at: Option<String>,
    pub status_at: String,
}

const COLUMNS: &str = "device_id, name, first_seen, last_seen, removed_at, status_at";

/// A short public name for a secret: PCs announce it to tell whether they hold the same one.
/// The secret is 256 random bits, so its digest says nothing about it.
pub fn key_id(secret: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"motodesk-network-key-id");
    h.update(secret.as_bytes());
    hex::encode(&h.finalize()[..8])
}

pub async fn key_epoch(db: &SqlitePool) -> LocalResult<i64> {
    Ok(sqlx::query_scalar("SELECT key_epoch FROM sync_state WHERE id = 1")
        .fetch_one(db)
        .await?)
}

/// What this PC is called on the list.
fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Poste".into())
}

/// Note that this PC is here, now.
pub async fn touch_self(state: &SharedState) -> LocalResult<()> {
    let id = state.device_id().await?.to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO network_devices (device_id, name, first_seen, last_seen, removed_at, status_at)
         VALUES ($1, $2, $3, $3, NULL, $3)
         ON CONFLICT(device_id) DO UPDATE SET name = $2, last_seen = $3",
    )
    .bind(id)
    .bind(computer_name())
    .bind(now)
    .execute(&state.db)
    .await?;
    Ok(())
}

pub async fn all(db: &SqlitePool) -> LocalResult<Vec<Device>> {
    Ok(sqlx::query_as::<_, Device>(&format!(
        "SELECT {COLUMNS} FROM network_devices ORDER BY first_seen, device_id"
    ))
    .fetch_all(db)
    .await?)
}

fn when(stamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(stamp).map_or(DateTime::<Utc>::MIN_UTC, |t| t.with_timezone(&Utc))
}

/// Fold a peer's list into this one: the latest sighting, and the latest decision to remove or
/// reconnect.
pub async fn merge_into(tx: &mut Transaction<'_, Sqlite>, incoming: Vec<Device>) -> LocalResult<()> {
    for theirs in incoming.into_iter().take(200) {
        if uuid::Uuid::parse_str(&theirs.device_id).is_err() || theirs.name.len() > 200 {
            continue;
        }
        let mine = sqlx::query_as::<_, Device>(&format!("SELECT {COLUMNS} FROM network_devices WHERE device_id = $1"))
            .bind(&theirs.device_id)
            .fetch_optional(&mut **tx)
            .await?;
        let merged = match mine {
            None => theirs,
            Some(mine) => {
                let seen_later = when(&theirs.last_seen) > when(&mine.last_seen);
                let decided_later = when(&theirs.status_at) > when(&mine.status_at);
                Device {
                    name: if seen_later { theirs.name } else { mine.name },
                    last_seen: if seen_later { theirs.last_seen } else { mine.last_seen },
                    removed_at: if decided_later { theirs.removed_at } else { mine.removed_at },
                    status_at: if decided_later { theirs.status_at } else { mine.status_at },
                    first_seen: if when(&theirs.first_seen) < when(&mine.first_seen) {
                        theirs.first_seen
                    } else {
                        mine.first_seen
                    },
                    device_id: mine.device_id,
                }
            }
        };
        sqlx::query(
            "INSERT OR REPLACE INTO network_devices (device_id, name, first_seen, last_seen, removed_at, status_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&merged.device_id)
        .bind(&merged.name)
        .bind(&merged.first_seen)
        .bind(&merged.last_seen)
        .bind(&merged.removed_at)
        .bind(&merged.status_at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub async fn merge(db: &SqlitePool, incoming: Vec<Device>) -> LocalResult<()> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    merge_into(&mut tx, incoming).await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct Removal {
    /// The administrator's password now.
    pub password: String,
    /// The one they will have from now on.
    pub new_password: String,
    pub confirm: String,
}

/// Shut a PC out of the network: a new secret it does not get, and a new password for the
/// administrator doing it.
pub async fn remove(state: &SharedState, admin_id: &str, device_id: &str, input: Removal) -> LocalResult<()> {
    if !local_accounts::is_standalone(&state.db).await {
        return Err(LocalError::Conflict("Cette installation n'est pas un réseau local.".into()));
    }
    if device_id == state.device_id().await?.to_string() {
        return Err(LocalError::BadRequest("Vous ne pouvez pas retirer le poste que vous utilisez.".into()));
    }
    let known: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM network_devices WHERE device_id = $1 AND removed_at IS NULL")
            .bind(device_id)
            .fetch_optional(&state.db)
            .await?;
    known.ok_or(LocalError::NotFound)?;
    local_accounts::check_current_password(&state.db, admin_id, &input.password).await?;
    local_accounts::check_new_password(&input.new_password, &input.confirm)?;
    if input.new_password == input.password {
        return Err(LocalError::BadRequest("Choisissez un mot de passe différent de l'actuel.".into()));
    }

    let old = state
        .mesh_secret
        .read()
        .await
        .clone()
        .ok_or_else(|| LocalError::Conflict("Ce poste doit d'abord être reconnecté au réseau.".into()))?;
    let admin = local_accounts::with_new_password(&state.db, admin_id, &input.new_password).await?;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let secret = hex::encode(bytes);
    let epoch = key_epoch(&state.db).await? + 1;
    let now = Utc::now().to_rfc3339();

    // All of it or none: a secret changed with the shared files left under the old one would
    // lose them.
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("UPDATE sync_state SET mesh_secret = $1, key_epoch = $2 WHERE id = 1")
        .bind(&secret)
        .bind(epoch)
        .execute(&mut *tx)
        .await?;
    crate::local_server::shared_folder::rewrap_keys(&mut tx, &old, &secret).await?;
    sqlx::query("UPDATE network_devices SET removed_at = $2, status_at = $2 WHERE device_id = $1")
        .bind(device_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    local_accounts::save_account(&mut *tx, &admin).await?;
    sqlx::query(
        "INSERT INTO audit_log (id, actor_id, action, entity_type, entity_id, payload, created_at)
         VALUES ($1, $2, 'device_removed', 'network_device', $3, '{}', $4)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(admin_id)
    .bind(device_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    local_accounts::keep_own_session(&state.db, &admin).await?;
    take_up(state, &secret).await
}

/// Start using a secret that has just been written to the database.
async fn take_up(state: &SharedState, secret: &str) -> LocalResult<()> {
    local_accounts::install_mesh_secret(state, secret).await?;
    // The peers known until now held the old one.
    mesh::forget_peers(state).await;
    state.reload_company().await;
    mesh::catchup::request(state);
    Ok(())
}

/// Receive the network's current secret from one of `candidates`, with an administrator's
/// username and password.
pub async fn reconnect(
    state: &SharedState,
    username: &str,
    password: &str,
    candidates: &[PeerInfo],
) -> LocalResult<()> {
    if !local_accounts::is_standalone(&state.db).await {
        return Err(LocalError::Conflict("Cette installation n'est pas un réseau local.".into()));
    }
    let company = state.company_id.read().await.ok_or(LocalError::NotFound)?;
    let old = state.mesh_secret.read().await.clone();
    let mine = key_epoch(&state.db).await?;

    let mut refusals = join::Refusals::default();
    for peer in candidates.iter().take(join::MAX_CANDIDATES) {
        let package = match join::join_via(state, peer, username, password).await {
            Ok(package) => package,
            Err(why) => {
                refusals.note(why);
                continue;
            }
        };
        // This shop, and a key this PC lacks: newer, the same age but another (two removals
        // made out of contact), or any at all when this PC has none left.
        let newer = match &old {
            None => package.key_epoch >= mine,
            Some(old) => package.key_epoch > mine || (package.key_epoch == mine && package.mesh_secret != *old),
        };
        if package.company_id != company || !newer {
            continue;
        }

        let me = state.device_id().await?.to_string();
        let now = Utc::now().to_rfc3339();
        let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE sync_state SET mesh_secret = $1, key_epoch = $2 WHERE id = 1")
            .bind(&package.mesh_secret)
            .bind(package.key_epoch)
            .execute(&mut *tx)
            .await?;
        if let Some(old) = &old {
            crate::local_server::shared_folder::rewrap_keys(&mut tx, old, &package.mesh_secret).await?;
        }
        local_accounts::merge_into(&mut tx, package.accounts).await?;
        crate::local_server::recovery_key::merge_into(&mut tx, package.recovery).await?;
        merge_into(&mut tx, package.devices).await?;
        // Reconnecting a PC that had been removed (found again) is an administrator's say-so.
        sqlx::query("UPDATE network_devices SET removed_at = NULL, status_at = $2 WHERE device_id = $1")
            .bind(&me)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        take_up(state, &package.mesh_secret).await?;
        mesh::adopt_peer(state, peer.clone()).await;
        mesh::catchup::request(state);
        return Ok(());
    }
    Err(refusals.into_error())
}

#[cfg(test)]
mod tests;
