//! The shared folder's HTTP routes. The rules live in `shared_folder`.

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::Response,
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_stream::wrappers::ReceiverStream;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::shared_folder::{self as shared, FilePatch, FolderPatch};
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/shared/status", get(status))
        .route("/api/shared/browse", get(browse))
        .route("/api/shared/folders", post(create_folder))
        .route("/api/shared/folders/:id", patch(update_folder).delete(delete_folder))
        .route("/api/shared/files", post(begin_upload))
        .route("/api/shared/files/:id", patch(update_file).delete(delete_file))
        .route("/api/shared/files/:id/chunks/:index", put(put_chunk))
        .route("/api/shared/files/:id/finish", post(finish_upload))
        .route("/api/shared/files/:id/content", get(download))
        .route("/api/shared/audit", get(audit))
}

async fn status(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let actor = shared::actor(&state).await?;
    let available = shared::available(&state).await;
    let usage = if available { serde_json::to_value(shared::usage(&state).await?).ok() } else { None };
    // Files are copied between PCs only on a shop's local network for now.
    let replicated = crate::local_server::local_accounts::is_standalone(&state.db).await;
    Ok(Json(json!({
        "available": available,
        "is_admin": actor.is_admin,
        "replicated": replicated,
        "usage": usage,
    })))
}

#[derive(Debug, Deserialize)]
struct BrowseQuery {
    folder: Option<String>,
}

async fn browse(
    State(state): State<SharedState>,
    Query(q): Query<BrowseQuery>,
) -> LocalResult<Json<shared::Listing>> {
    let actor = shared::actor(&state).await?;
    let folder = q.folder.as_deref().filter(|f| !f.is_empty());
    Ok(Json(shared::browse(&state, &actor, folder).await?))
}

#[derive(Debug, Deserialize)]
struct NewFolder {
    parent_id: Option<String>,
    name: String,
    #[serde(default = "admins_only")]
    visibility: String,
}

fn admins_only() -> String {
    "admins".into()
}

async fn create_folder(
    State(state): State<SharedState>,
    Json(body): Json<NewFolder>,
) -> LocalResult<(StatusCode, Json<shared::FolderView>)> {
    let actor = shared::actor(&state).await?;
    let folder =
        shared::create_folder(&state, &actor, body.parent_id.as_deref(), &body.name, &body.visibility).await?;
    Ok((StatusCode::CREATED, Json(folder)))
}

async fn update_folder(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(patch): Json<FolderPatch>,
) -> LocalResult<Json<shared::FolderView>> {
    let actor = shared::actor(&state).await?;
    Ok(Json(shared::update_folder(&state, &actor, &id, patch).await?))
}

async fn delete_folder(State(state): State<SharedState>, Path(id): Path<String>) -> LocalResult<StatusCode> {
    let actor = shared::actor(&state).await?;
    shared::delete_folder(&state, &actor, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct NewFile {
    folder_id: String,
    name: String,
    size_bytes: u64,
    mime: Option<String>,
}

async fn begin_upload(
    State(state): State<SharedState>,
    Json(body): Json<NewFile>,
) -> LocalResult<(StatusCode, Json<shared::UploadStart>)> {
    let actor = shared::actor(&state).await?;
    let start =
        shared::begin_upload(&state, &actor, &body.folder_id, &body.name, body.size_bytes, body.mime.as_deref())
            .await?;
    Ok((StatusCode::CREATED, Json(start)))
}

async fn put_chunk(
    State(state): State<SharedState>,
    Path((id, index)): Path<(String, u32)>,
    body: Bytes,
) -> LocalResult<StatusCode> {
    let actor = shared::actor(&state).await?;
    shared::put_chunk(&state, &actor, &id, index, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn finish_upload(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> LocalResult<Json<shared::FileView>> {
    let actor = shared::actor(&state).await?;
    Ok(Json(shared::finish_upload(&state, &actor, &id).await?))
}

async fn update_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(patch): Json<FilePatch>,
) -> LocalResult<Json<shared::FileView>> {
    let actor = shared::actor(&state).await?;
    Ok(Json(shared::update_file(&state, &actor, &id, patch).await?))
}

async fn delete_file(State(state): State<SharedState>, Path(id): Path<String>) -> LocalResult<StatusCode> {
    let actor = shared::actor(&state).await?;
    shared::delete_file(&state, &actor, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Percent-encode a file name for `Content-Disposition: filename*=UTF-8''…` (RFC 5987).
fn encode_filename(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The decrypted file, streamed chunk by chunk so a big file is never held whole in memory.
async fn download(State(state): State<SharedState>, Path(id): Path<String>) -> LocalResult<Response> {
    let actor = shared::actor(&state).await?;
    let file = shared::open_download(&state, &actor, &id).await?;

    let mime = file.mime.clone().unwrap_or_else(|| "application/octet-stream".into());
    let disposition = format!("attachment; filename*=UTF-8''{}", encode_filename(&file.name));
    let length = file.size_bytes;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    let reader_state = state.clone();
    tokio::spawn(async move {
        for index in 0..file.chunk_count() {
            let item = file
                .read_chunk(&reader_state, index)
                .await
                .map(Bytes::from)
                .map_err(|e| std::io::Error::other(e.to_string()));
            let failed = item.is_err();
            if tx.send(item).await.is_err() || failed {
                break;
            }
        }
    });

    let mut response = Response::new(Body::from_stream(ReceiverStream::new(rx)));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(|e| LocalError::Internal(e.into()))?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(response)
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

async fn audit(
    State(state): State<SharedState>,
    Query(q): Query<AuditQuery>,
) -> LocalResult<Json<Vec<shared::AuditEntry>>> {
    let actor = shared::actor(&state).await?;
    Ok(Json(shared::recent_audit(&state, &actor, q.limit.unwrap_or(100)).await?))
}

#[cfg(test)]
mod tests {
    use super::encode_filename;

    #[test]
    fn file_names_are_encoded_for_the_download_header() {
        assert_eq!(encode_filename("Bilan 2025.pdf"), "Bilan%202025.pdf");
        assert_eq!(encode_filename("é.txt"), "%C3%A9.txt");
        assert_eq!(encode_filename("a\"b\r\nc"), "a%22b%0D%0Ac");
    }
}
