//! Online accounts (port of `desktop/src/lib/api/auth.ts`).

use axum::http::Method;
use serde::Serialize;

use crate::error::ApiResult;
use crate::session::Client;
use crate::sync::BootstrapResult;
use crate::types::{PublicUser, TokenBundle};

#[derive(Debug, Clone, Serialize)]
pub struct RegisterPayload {
    pub email: String,
    pub password: String,
    pub full_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_code: Option<String>,
}

/// Hand the session to the local server, which keeps it for offline `/me` and for its own
/// cloud sync. Best effort: the local server may still be starting.
pub async fn sync_session_to_local(client: &Client, bundle: &TokenBundle) {
    let _ = client.call(Method::POST, "/api/auth/sync-session").public().json(bundle).done().await;
}

/// Hand a fresh sign-in to the local server. Unlike the quiet one above, this one can refuse:
/// an account from another company than this PC's data must not sign in, and the cloud session
/// it just opened is closed again rather than left half-open.
async fn adopt_sign_in(client: &Client, bundle: &TokenBundle) -> ApiResult<()> {
    match client.call(Method::POST, "/api/auth/sync-session").public().json(bundle).done().await {
        Err(e) if e.code == "company_mismatch" => {
            client.cloud().logout(&bundle.refresh_token).await;
            client.set_tokens(None);
            Err(e)
        }
        // Anything else: the local server may still be starting.
        _ => Ok(()),
    }
}

/// Import shop data from the cloud into the local database (first run, or an empty one).
pub async fn bootstrap_from_cloud(client: &Client) -> Option<BootstrapResult> {
    client.call(Method::POST, "/api/sync/bootstrap-from-cloud").public().send().await.ok()
}

pub async fn login(client: &Client, identifier: &str, password: &str) -> ApiResult<TokenBundle> {
    let bundle = client.cloud().login(identifier, password).await?;
    client.set_tokens(Some(bundle.clone()));
    adopt_sign_in(client, &bundle).await?;
    Ok(bundle)
}

pub async fn register(client: &Client, payload: &RegisterPayload) -> ApiResult<TokenBundle> {
    let bundle = client.cloud().register(payload).await?;
    client.set_tokens(Some(bundle.clone()));
    adopt_sign_in(client, &bundle).await?;
    Ok(bundle)
}

/// Sign out of the cloud (when there is one) and of this PC.
pub async fn logout(client: &Client) {
    if let Some(tokens) = client.tokens() {
        if !tokens.refresh_token.is_empty() && !tokens.user.is_local() {
            client.cloud().logout(&tokens.refresh_token).await;
        }
    }
    client.set_tokens(None);
    let _ = client.call(Method::POST, "/api/auth/logout").public().done().await;
}

/// Who the local server says is signed in on this PC.
pub async fn local_me(client: &Client) -> ApiResult<PublicUser> {
    client.call(Method::GET, "/api/auth/me").public().send().await
}
