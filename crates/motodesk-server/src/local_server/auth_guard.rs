//! Admin-only access for sensitive local API routes (data manager).

use axum::{async_trait, extract::FromRequestParts, http::request::Parts};
use serde_json::Value;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;

/// Authenticated admin bound to a company install.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AdminUser {
    pub user_id: String,
    pub email: String,
    pub full_name: String,
}

/// True when the cached session belongs to an admin (company manager).
pub async fn is_admin(state: &SharedState) -> bool {
    require_admin(state).await.is_ok()
}

pub async fn require_admin(state: &SharedState) -> LocalResult<AdminUser> {
    let user: Value = crate::local_server::local_accounts::session_user(&state.db)
        .await?
        .ok_or(LocalError::Unauthorized)?;

    let role = user.get("role").and_then(|v| v.as_str()).unwrap_or("");
    if role != "admin" && role != "staff" {
        return Err(LocalError::Forbidden);
    }

    let session_company = user
        .get("company_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let install_company = state.company_id.read().await.map(|id| id.to_string());

    match (session_company, install_company.as_deref()) {
        (Some(a), Some(b)) if a == b => {}
        (None, None) => {}
        (Some(_), None) => {}
        _ => return Err(LocalError::Forbidden),
    }

    Ok(AdminUser {
        user_id: user
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        email: user
            .get("email")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        full_name: user
            .get("full_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

#[async_trait]
impl FromRequestParts<SharedState> for AdminUser {
    type Rejection = LocalError;

    async fn from_request_parts(
        _parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        // Local-token middleware already authenticated the caller as the
        // webview; we still re-check the cached role + company binding.
        require_admin(state).await
    }
}
