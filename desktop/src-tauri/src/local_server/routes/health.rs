use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde_json::json;

use crate::local_server::error::LocalResult;
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/health", get(health))
}

async fn health(
    State(state): State<SharedState>,
) -> LocalResult<(StatusCode, Json<serde_json::Value>)> {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    Ok((
        status,
        Json(json!({
            "status": if db_ok { "ok" } else { "degraded" },
            "db": db_ok,
            "mode": "local-first",
            "version": env!("CARGO_PKG_VERSION"),
        })),
    ))
}
