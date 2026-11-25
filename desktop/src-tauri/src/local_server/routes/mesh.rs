use axum::{
    extract::{DefaultBodyLimit, State},
    routing::{get, post},
    Json, Router,
};
use motodesk_core::sync::MeshApplyResponse;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::mesh;
use crate::local_server::mesh::transport::{self, Kind};
use crate::local_server::state::SharedState;

/// LAN-facing routes: no local token. All but joining are sealed with the network secret
/// (`mesh::transport`).
pub fn lan_router() -> Router<SharedState> {
    Router::new()
        .route("/mesh/apply", post(apply))
        .route("/mesh/join/start", post(join_start))
        .route("/mesh/join/finish", post(join_finish))
        .route("/mesh/pull", post(pull))
        .route("/mesh/accounts", post(accounts))
        .route("/mesh/shared/summary", post(shared_summary))
        .route("/mesh/shared/rows", post(shared_rows))
        .route("/mesh/shared/chunk", post(shared_chunk))
        .route("/mesh/shared/nudge", post(shared_nudge))
        // A batch of changes is well over axum's 2 MB default once it carries images.
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
}

async fn join_start(
    State(state): State<SharedState>,
    Json(body): Json<mesh::join::JoinStart>,
) -> LocalResult<Json<mesh::join::JoinStartReply>> {
    mesh::join::start(&state, body).await.map(Json)
}

async fn join_finish(
    State(state): State<SharedState>,
    Json(body): Json<mesh::join::JoinFinish>,
) -> LocalResult<Json<mesh::join::JoinFinishReply>> {
    mesh::join::finish(&state, body).await.map(Json)
}

/// The fields of a request a PC of this network sealed: recent, and not seen before.
async fn open_request(
    state: &SharedState,
    body: &serde_json::Value,
) -> LocalResult<serde_json::Map<String, serde_json::Value>> {
    let secret = state.mesh_secret.read().await.clone();
    let Some(secret) = secret.filter(|s| !s.is_empty()) else {
        return Err(LocalError::Unauthorized);
    };
    let (fields, nonce) = transport::open(&secret, Kind::Request, body)?;
    if !state.mesh_nonces.accept(&nonce).await {
        return Err(LocalError::Unauthorized);
    }
    Ok(fields)
}

/// [`open_request`] for what only a local network does.
pub(crate) async fn authenticated_fields(
    state: &SharedState,
    body: serde_json::Value,
) -> LocalResult<serde_json::Map<String, serde_json::Value>> {
    if !crate::local_server::local_accounts::is_standalone(&state.db).await {
        return Err(LocalError::NotFound);
    }
    open_request(state, &body).await
}

pub(crate) async fn sealed_reply(state: &SharedState, fields: serde_json::Map<String, serde_json::Value>) -> LocalResult<Json<serde_json::Value>> {
    let secret = state.mesh_secret.read().await.clone().ok_or(LocalError::Unauthorized)?;
    Ok(Json(transport::seal(&secret, Kind::Reply, fields)))
}

/// "What do you have that I lack?" — answered with the changes beyond the asker's version vector.
async fn pull(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    let fields = authenticated_fields(&state, body).await?;
    let vv: std::collections::HashMap<Uuid, i64> = fields
        .get("vv")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((Uuid::parse_str(k).ok()?, v.as_i64()?)))
                .collect()
        })
        .unwrap_or_default();
    let gaps: crate::local_server::sync_log::Gaps = fields
        .get("gaps")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((Uuid::parse_str(k).ok()?, serde_json::from_value(v.clone()).ok()?)))
                .collect()
        })
        .unwrap_or_default();
    let limit = fields.get("limit").and_then(|v| v.as_i64()).unwrap_or(500);
    let (ops, more) = mesh::catchup::serve_pull(&state, &vv, &gaps, limit).await?;
    let mut out = serde_json::Map::new();
    out.insert("ops".into(), serde_json::to_value(&ops).map_err(|e| LocalError::Internal(e.into()))?);
    out.insert("more".into(), more.into());
    sealed_reply(&state, out).await
}

/// Swap accounts: keep the asker's newer ones, answer with all of this PC's.
async fn accounts(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    let fields = authenticated_fields(&state, body).await?;
    let incoming: Vec<crate::local_server::local_accounts::Account> =
        serde_json::from_value(fields.get("accounts").cloned().unwrap_or(serde_json::Value::Null))
            .map_err(|_| LocalError::BadRequest("comptes invalides".into()))?;
    let all = mesh::catchup::serve_accounts(&state, incoming).await?;
    // The recovery key's hash travels with the accounts.
    let theirs = fields.get("recovery").and_then(|v| serde_json::from_value(v.clone()).ok());
    crate::local_server::recovery_key::merge(&state.db, theirs).await?;
    let recovery = crate::local_server::recovery_key::stored(&state.db).await?;
    let mut out = serde_json::Map::new();
    out.insert("recovery".into(), serde_json::to_value(&recovery).map_err(|e| LocalError::Internal(e.into()))?);
    out.insert("accounts".into(), serde_json::to_value(&all).map_err(|e| LocalError::Internal(e.into()))?);
    sealed_reply(&state, out).await
}

/// The shared folder: what this PC holds (or just that it matches the asker), the full rows
/// asked for, one encrypted chunk, and a tap on the shoulder saying "something changed".
async fn shared_summary(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    let fields = authenticated_fields(&state, body).await?;
    let digest = fields.get("digest").and_then(|v| v.as_str()).unwrap_or("");
    let out = crate::local_server::shared_folder::replication::serve_summary(&state, digest).await?;
    sealed_reply(&state, out).await
}

async fn shared_rows(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    let fields = authenticated_fields(&state, body).await?;
    let out = crate::local_server::shared_folder::replication::serve_rows(&state, &fields).await?;
    sealed_reply(&state, out).await
}

async fn shared_chunk(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    let fields = authenticated_fields(&state, body).await?;
    let out = crate::local_server::shared_folder::replication::serve_chunk(&state, &fields).await?;
    sealed_reply(&state, out).await
}

async fn shared_nudge(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    authenticated_fields(&state, body).await?;
    mesh::catchup::request(&state);
    sealed_reply(&state, serde_json::Map::new()).await
}

/// Loopback admin routes for mesh status / static peers.
pub fn admin_router() -> Router<SharedState> {
    Router::new().route("/mesh/status", get(status)).route(
        "/mesh/static-peers",
        get(get_static_peers).put(put_static_peers),
    )
}

/// Changes pushed by a peer as they happen.
async fn apply(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<MeshApplyResponse>> {
    let mut fields = open_request(&state, &body).await?;
    let ops = serde_json::from_value(fields.remove("ops").unwrap_or_default())
        .map_err(|_| LocalError::BadRequest("modifications invalides".into()))?;
    Ok(Json(mesh::apply_batch(&state, ops).await?))
}

async fn status(State(state): State<SharedState>) -> LocalResult<Json<serde_json::Value>> {
    let peers = state.peers.read().await;
    Ok(Json(json!({
        "peer_count": peers.len(),
        "peers": peers.clone(),
        "other_version_count": mesh::other_version_count().await,
    })))
}

#[derive(Debug, Deserialize)]
struct StaticPeers {
    peers: Vec<String>,
}

async fn get_static_peers(
    State(state): State<SharedState>,
) -> LocalResult<Json<serde_json::Value>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT lan_peers_static FROM sync_state WHERE id = 1")
            .fetch_optional(&state.db)
            .await?;
    let peers: Vec<String> = row
        .and_then(|(s,)| s)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(Json(json!({ "peers": peers })))
}

async fn put_static_peers(
    State(state): State<SharedState>,
    Json(body): Json<StaticPeers>,
) -> LocalResult<Json<serde_json::Value>> {
    let cleaned: Vec<String> = body
        .peers
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    let json_payload = serde_json::to_string(&cleaned).unwrap_or_else(|_| "[]".into());
    sqlx::query("UPDATE sync_state SET lan_peers_static = $1 WHERE id = 1")
        .bind(&json_payload)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "peers": cleaned })))
}

/// The changes to push to a peer, sealed.
pub fn apply_request(secret: &str, ops: Vec<motodesk_core::sync::ChangeLogEntry>) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    fields.insert("ops".into(), serde_json::to_value(ops).unwrap_or_default());
    transport::seal(secret, Kind::Request, fields)
}
