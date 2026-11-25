//! The network's recovery key (port of `desktop/src/lib/api/recovery.ts`;
//! see `local_server/recovery_key.rs`).

use axum::http::Method;
use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::local_account::SessionWithKey;
use crate::session::Client;

#[derive(Debug, Clone, Serialize)]
pub struct RecoverPayload {
    pub recovery_key: String,
    pub username: String,
    pub password: String,
    pub confirm: String,
}

/// "Password lost": a session, with the key that replaces the one just used.
pub async fn recover_with_key(client: &Client, payload: &RecoverPayload) -> ApiResult<SessionWithKey> {
    client.call(Method::POST, "/api/auth/local/recover").public().json(payload).send().await
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecoveryStatus {
    pub exists: bool,
    /// Someone typed it back: it is written down.
    pub confirmed: bool,
    #[serde(default)]
    pub updated_at: Option<String>,
}

pub async fn status(client: &Client) -> ApiResult<RecoveryStatus> {
    client.get("/api/network/recovery").await
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssuedKey {
    pub recovery_key: String,
}

pub async fn issue(client: &Client, password: &str) -> ApiResult<IssuedKey> {
    client.post("/api/network/recovery", &serde_json::json!({ "password": password })).await
}

pub async fn confirm(client: &Client, recovery_key: &str) -> ApiResult<()> {
    client
        .call(Method::POST, "/api/network/recovery/confirm")
        .json(&serde_json::json!({ "recovery_key": recovery_key }))
        .done()
        .await
}
