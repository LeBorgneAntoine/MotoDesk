//! The "keep running in the background" preference. See `crate::background`.

use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_server::auth_guard::AdminUser;
use crate::local_server::error::LocalResult;
use crate::local_server::local_accounts;
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/api/background", get(show).put(set))
}

async fn stored(state: &SharedState) -> LocalResult<Option<bool>> {
    let value: Option<Option<i64>> = sqlx::query_scalar("SELECT background_enabled FROM sync_state WHERE id = 1")
        .fetch_optional(&state.db)
        .await?;
    Ok(value.flatten().map(|n| n != 0))
}

/// Recompute the effective preference and apply it. Called at start, and whenever the install
/// becomes (or stops being) a network PC.
pub async fn refresh(state: &SharedState) {
    let local_network = local_accounts::is_standalone(&state.db).await;
    let stored = stored(state).await.unwrap_or(None);
    crate::background::apply(crate::background::effective(stored, local_network));
}

async fn current(state: &SharedState) -> LocalResult<Value> {
    let local_network = local_accounts::is_standalone(&state.db).await;
    let stored = stored(state).await?;
    Ok(json!({
        "enabled": crate::background::effective(stored, local_network),
        "stored": stored,
        "default_enabled": local_network,
    }))
}

async fn show(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    current(&state).await.map(Json)
}

#[derive(Debug, Deserialize)]
struct SetBackground {
    enabled: bool,
}

async fn set(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<SetBackground>,
) -> LocalResult<Json<Value>> {
    sqlx::query("UPDATE sync_state SET background_enabled = $1 WHERE id = 1")
        .bind(body.enabled)
        .execute(&state.db)
        .await?;
    refresh(&state).await;
    current(&state).await.map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::shop;

    #[tokio::test]
    async fn a_network_pc_defaults_on_and_the_owners_choice_is_kept() {
        let state = shop().await;
        let fresh = current(&state).await.unwrap();
        assert_eq!(fresh["enabled"], false, "an installation that is not a network PC");
        assert_eq!(fresh["stored"], Value::Null);

        local_accounts::setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        let network = current(&state).await.unwrap();
        assert_eq!(network["enabled"], true, "a network PC stays on by default");
        assert_eq!(network["stored"], Value::Null);

        sqlx::query("UPDATE sync_state SET background_enabled = 0 WHERE id = 1").execute(&state.db).await.unwrap();
        let off = current(&state).await.unwrap();
        assert_eq!(off["enabled"], false, "the owner turned it off");
        assert_eq!(off["stored"], false);
        assert_eq!(off["default_enabled"], true);
    }
}
