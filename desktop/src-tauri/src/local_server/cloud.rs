use crate::local_server::state::SharedState;

/// Resolve cloud API base URL (always http://127.0.0.1:8080 unless configured).
pub async fn cloud_base_url(state: &SharedState) -> String {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT cloud_url FROM sync_state WHERE id = 1")
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

    row.and_then(|(u,)| u)
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        .unwrap_or_else(|| state.cloud_url.clone())
}
