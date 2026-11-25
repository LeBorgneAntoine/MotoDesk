//! The network's accounts, managed by an administrator. See `local_accounts` for the rules.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
    Json, Router,
};

use serde::{Deserialize, Serialize};

use crate::local_server::auth_guard::AdminUser;
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::{network_devices, recovery_key};
use crate::local_server::local_accounts::{self, AccountPatch, AccountView, NewAccount};
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/network/accounts", get(list).post(create))
        .route("/api/network/accounts/:id", patch(update).delete(remove))
        .route("/api/network/recovery", get(recovery_status).post(recovery_issue))
        .route("/api/network/recovery/confirm", post(recovery_confirm))
        .route("/api/network/devices", get(devices))
        .route("/api/network/devices/:id/remove", post(remove_device))
}

#[derive(Debug, Serialize)]
struct DeviceView {
    #[serde(flatten)]
    device: network_devices::Device,
    this_pc: bool,
}

async fn devices(State(state): State<SharedState>, _admin: AdminUser) -> LocalResult<Json<Vec<DeviceView>>> {
    network_devices::touch_self(&state).await?;
    let me = state.device_id().await?.to_string();
    Ok(Json(
        network_devices::all(&state.db)
            .await?
            .into_iter()
            .map(|device| DeviceView { this_pc: device.device_id == me, device })
            .collect(),
    ))
}

async fn remove_device(
    State(state): State<SharedState>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(input): Json<network_devices::Removal>,
) -> LocalResult<StatusCode> {
    network_devices::remove(&state, &admin.user_id, &id, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
struct RecoveryStatus {
    exists: bool,
    confirmed: bool,
    updated_at: Option<String>,
}

async fn recovery_status(State(state): State<SharedState>, _admin: AdminUser) -> LocalResult<Json<RecoveryStatus>> {
    let record = recovery_key::stored(&state.db).await?;
    Ok(Json(RecoveryStatus {
        exists: record.is_some(),
        confirmed: record.as_ref().is_some_and(|r| r.confirmed),
        updated_at: record.map(|r| r.updated_at),
    }))
}

#[derive(Debug, Deserialize)]
struct IssueInput {
    /// The administrator's own password, asked again: this replaces the key in use.
    password: String,
}

async fn recovery_issue(
    State(state): State<SharedState>,
    admin: AdminUser,
    Json(input): Json<IssueInput>,
) -> LocalResult<Json<serde_json::Value>> {
    if !local_accounts::is_standalone(&state.db).await {
        return Err(LocalError::Conflict("La clé de récupération n'existe que sans cloud.".into()));
    }
    local_accounts::check_current_password(&state.db, &admin.user_id, &input.password).await?;
    let key = recovery_key::issue(&state).await?;
    Ok(Json(serde_json::json!({ "recovery_key": key })))
}

#[derive(Debug, Deserialize)]
struct ConfirmInput {
    recovery_key: String,
}

async fn recovery_confirm(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(input): Json<ConfirmInput>,
) -> LocalResult<StatusCode> {
    recovery_key::confirm(&state, &input.recovery_key).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list(State(state): State<SharedState>, _admin: AdminUser) -> LocalResult<Json<Vec<AccountView>>> {
    Ok(Json(local_accounts::list_views(&state.db).await?))
}

async fn create(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(input): Json<NewAccount>,
) -> LocalResult<(StatusCode, Json<AccountView>)> {
    Ok((StatusCode::CREATED, Json(local_accounts::create_account(&state, input).await?)))
}

async fn update(
    State(state): State<SharedState>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(patch): Json<AccountPatch>,
) -> LocalResult<Json<AccountView>> {
    Ok(Json(local_accounts::update_account(&state, &admin.user_id, &id, patch).await?))
}

async fn remove(
    State(state): State<SharedState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> LocalResult<StatusCode> {
    local_accounts::delete_account(&state, &admin.user_id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
