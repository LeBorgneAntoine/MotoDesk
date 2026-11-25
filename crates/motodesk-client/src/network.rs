//! The local network as this PC sees it (port of `getMeshStatus` in
//! `desktop/src/lib/api/meshAdmin.ts` and of `reconnectToNetwork` in `networkDevices.ts`).

use axum::http::Method;
use serde::Deserialize;

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
