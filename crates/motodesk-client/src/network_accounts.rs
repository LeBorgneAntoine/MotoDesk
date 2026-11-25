//! The accounts of a local network (port of `desktop/src/lib/api/networkAccounts.ts`).
//!
//! They open a session on every PC of the shop's network. An online company manages its team
//! with [`crate::employees`] instead.

use axum::http::Method;
use serde::{Deserialize, Serialize};

use crate::employees::Role;
use crate::error::ApiResult;
use crate::session::Client;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NetworkAccount {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub full_name: Option<String>,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl NetworkAccount {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }

    /// The name to show: the display name, or the username when there is none.
    pub fn display_name(&self) -> &str {
        self.full_name.as_deref().map(str::trim).filter(|name| !name.is_empty()).unwrap_or(&self.username)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewNetworkAccount {
    pub username: String,
    pub password: String,
    pub confirm: String,
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
}

/// Only the fields that are set are changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NetworkAccountPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
}

pub async fn list(client: &Client) -> ApiResult<Vec<NetworkAccount>> {
    client.call(Method::GET, "/api/network/accounts").public().send().await
}

pub async fn create(client: &Client, account: &NewNetworkAccount) -> ApiResult<NetworkAccount> {
    client.call(Method::POST, "/api/network/accounts").public().json(account).send().await
}

pub async fn update(client: &Client, id: &str, patch: &NetworkAccountPatch) -> ApiResult<NetworkAccount> {
    client.call(Method::PATCH, format!("/api/network/accounts/{id}")).public().json(patch).send().await
}

pub async fn delete(client: &Client, id: &str) -> ApiResult<()> {
    client.call(Method::DELETE, format!("/api/network/accounts/{id}")).public().done().await
}
