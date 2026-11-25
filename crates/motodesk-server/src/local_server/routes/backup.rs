use axum::{
    extract::State,
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;

use crate::local_server::auth_guard::AdminUser;
use crate::local_server::error::LocalResult;
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/backup/status", get(status))
        .route("/api/backup/export", post(export))
        .route("/api/backup/restore", post(restore))
        .route("/api/backup/auto-password", put(set_auto_password).delete(clear_auto_password))
}

#[derive(Debug, Deserialize)]
struct PasswordBody {
    password: String,
}

#[derive(Debug, Deserialize)]
struct RestoreBody {
    filename: String,
    password: String,
}

async fn status(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> LocalResult<Json<crate::local_server::backup::BackupStatus>> {
    let s = crate::local_server::backup::status(&state).await?;
    Ok(Json(s))
}

async fn export(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<PasswordBody>,
) -> LocalResult<Json<crate::local_server::backup::BackupFile>> {
    let file = crate::local_server::backup::export(&state, &body.password).await?;
    Ok(Json(file))
}

async fn set_auto_password(
    _admin: AdminUser,
    Json(body): Json<PasswordBody>,
) -> LocalResult<Json<serde_json::Value>> {
    crate::local_server::backup::set_auto_password(&body.password)?;
    Ok(Json(serde_json::json!({ "auto_password_set": true })))
}

async fn clear_auto_password(_admin: AdminUser) -> LocalResult<Json<serde_json::Value>> {
    crate::local_server::backup::clear_auto_password()?;
    Ok(Json(serde_json::json!({ "auto_password_set": false })))
}

async fn restore(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<RestoreBody>,
) -> LocalResult<Json<crate::local_server::backup::RestoreResult>> {
    let result =
        crate::local_server::backup::restore_named(&state, &body.filename, &body.password).await?;
    Ok(Json(result))
}
