//! The local network as this PC sees it.

use axum::http::Method;
use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client;

/// Where this PC stands among the PCs of the shop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MeshStatus {
    pub peer_count: i64,
    /// PCs of the shop seen running another version: they cannot exchange with this one.
    pub other_version_count: i64,
    /// This PC holds an old network key, or none: it has to be reconnected.
    pub needs_reconnect: bool,
    /// PCs seen still holding an older key.
    pub behind_count: i64,
}

pub async fn mesh_status(client: &Client) -> ApiResult<MeshStatus> {
    client.call(Method::GET, "/mesh/status").public().send().await
}

/// One PC of the local network (see `local_server/network_devices.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NetworkDevice {
    pub device_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub first_seen: String,
    #[serde(default)]
    pub last_seen: String,
    /// Set once an administrator has removed it.
    #[serde(default)]
    pub removed_at: Option<String>,
    #[serde(default)]
    pub this_pc: bool,
}

pub async fn list_devices(client: &Client) -> ApiResult<Vec<NetworkDevice>> {
    client.get("/api/network/devices").await
}

/// What removing a PC asks of the administrator: their password, and a new one, because the PC
/// that is shut out knows the fingerprint of the old one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoveDevice {
    pub password: String,
    pub new_password: String,
    pub confirm: String,
}

/// Shut a PC out: the network gets a new key and the administrator a new password. Every other
/// PC then has to be reconnected.
pub async fn remove_device(client: &Client, device_id: &str, payload: &RemoveDevice) -> ApiResult<()> {
    client.call(Method::POST, format!("/api/network/devices/{device_id}/remove")).json(payload).done().await
}

/// On a PC left with an old key: get the current one with an administrator's password. Another
/// PC of the network must be switched on. What this PC recorded meanwhile is kept.
pub async fn reconnect(client: &Client, username: &str, password: &str) -> ApiResult<()> {
    client
        .call(Method::POST, "/api/auth/local/reconnect")
        .public()
        .json(&serde_json::json!({ "username": username, "password": password }))
        .done()
        .await
}
