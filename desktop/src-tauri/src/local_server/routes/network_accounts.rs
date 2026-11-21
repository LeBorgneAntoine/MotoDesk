//! The network's accounts, managed by an administrator. See `local_accounts` for the rules.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
    Json, Router,
};

use crate::local_server::auth_guard::AdminUser;
use crate::local_server::error::LocalResult;
use crate::local_server::local_accounts::{self, AccountPatch, AccountView, NewAccount};
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/network/accounts", get(list).post(create))
        .route("/api/network/accounts/:id", patch(update).delete(remove))
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
