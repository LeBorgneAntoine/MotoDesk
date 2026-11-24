use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use motodesk_core::sync::{MeshApplyRequest, MeshApplyResponse};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::mesh;
use crate::local_server::mesh_auth::{self, timestamp_fresh};
use crate::local_server::state::SharedState;

/// LAN-facing routes (no local-token; HMAC on apply).
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

/// A request signed with the mesh secret: its fields without `nonce`, `ts` and `sig`, if the
/// signature is genuine, recent and unused.
pub(crate) async fn authenticated_fields(
    state: &SharedState,
    mut body: serde_json::Value,
) -> LocalResult<serde_json::Map<String, serde_json::Value>> {
    if !crate::local_server::local_accounts::is_standalone(&state.db).await {
        return Err(LocalError::NotFound);
    }
    let secret = state.mesh_secret.read().await.clone();
    let Some(secret) = secret.filter(|s| !s.is_empty()) else {
        return Err(LocalError::Unauthorized);
    };
    let Some(map) = body.as_object_mut() else {
        return Err(LocalError::Unauthorized);
    };
    let nonce = map.remove("nonce").and_then(|v| v.as_str().map(str::to_string));
    let ts = map.remove("ts").and_then(|v| v.as_i64());
    let sig = map.remove("sig").and_then(|v| v.as_str().map(str::to_string));
    let (Some(nonce), Some(ts), Some(sig)) = (nonce, ts, sig) else {
        return Err(LocalError::Unauthorized);
    };
    if !mesh_auth::value_signature_ok(&secret, &nonce, ts, &sig, &serde_json::Value::Object(map.clone())) {
        return Err(LocalError::Unauthorized);
    }
    if !state.mesh_nonces.accept(&nonce).await {
        return Err(LocalError::Unauthorized);
    }
    Ok(std::mem::take(map))
}

pub(crate) async fn sign_reply(state: &SharedState, fields: serde_json::Map<String, serde_json::Value>) -> LocalResult<Json<serde_json::Value>> {
    let secret = state.mesh_secret.read().await.clone().ok_or(LocalError::Unauthorized)?;
    let (nonce, ts, sig) = mesh_auth::sign_value(&secret, &serde_json::Value::Object(fields.clone()));
    let mut out = fields;
    out.insert("nonce".into(), nonce.into());
    out.insert("ts".into(), ts.into());
    out.insert("sig".into(), sig.into());
    Ok(Json(serde_json::Value::Object(out)))
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
    sign_reply(&state, out).await
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
    let mut out = serde_json::Map::new();
    out.insert("accounts".into(), serde_json::to_value(&all).map_err(|e| LocalError::Internal(e.into()))?);
    sign_reply(&state, out).await
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
    sign_reply(&state, out).await
}

async fn shared_rows(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    let fields = authenticated_fields(&state, body).await?;
    let out = crate::local_server::shared_folder::replication::serve_rows(&state, &fields).await?;
    sign_reply(&state, out).await
}

async fn shared_chunk(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    let fields = authenticated_fields(&state, body).await?;
    let out = crate::local_server::shared_folder::replication::serve_chunk(&state, &fields).await?;
    sign_reply(&state, out).await
}

async fn shared_nudge(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    authenticated_fields(&state, body).await?;
    mesh::catchup::request(&state);
    sign_reply(&state, serde_json::Map::new()).await
}

/// Loopback admin routes for mesh status / static peers.
pub fn admin_router() -> Router<SharedState> {
    Router::new().route("/mesh/status", get(status)).route(
        "/mesh/static-peers",
        get(get_static_peers).put(put_static_peers),
    )
}

async fn apply(
    State(state): State<SharedState>,
    Json(body): Json<MeshApplyRequest>,
) -> LocalResult<Json<MeshApplyResponse>> {
    let secret = state.mesh_secret.read().await.clone();
    let Some(secret) = secret.filter(|s| !s.is_empty()) else {
        return Err(LocalError::Unauthorized);
    };

    let nonce = body.nonce.as_deref().ok_or(LocalError::Unauthorized)?;
    let ts = body.ts.ok_or(LocalError::Unauthorized)?;
    let sig = body.sig.as_deref().ok_or(LocalError::Unauthorized)?;
    let now = Utc::now().timestamp();
    if !timestamp_fresh(ts, now) {
        return Err(LocalError::Unauthorized);
    }
    if !state.mesh_nonces.accept(nonce).await {
        return Err(LocalError::Unauthorized);
    }

    let ops_body = mesh_auth::canonical_ops_bytes(&body.ops);
    let expected = mesh_auth::sign(&secret, nonce, ts, &ops_body);
    if !mesh_auth::constant_time_eq(sig, &expected) {
        return Err(LocalError::Unauthorized);
    }

    let resp = mesh::apply_batch(&state, body.ops).await?;
    Ok(Json(resp))
}

async fn status(State(state): State<SharedState>) -> LocalResult<Json<serde_json::Value>> {
    let peers = state.peers.read().await;
    Ok(Json(json!({
        "peer_count": peers.len(),
        "peers": peers.clone(),
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

/// Build a signed mesh apply body.
pub fn signed_apply_request(
    secret: &str,
    ops: Vec<motodesk_core::sync::ChangeLogEntry>,
) -> MeshApplyRequest {
    let nonce = Uuid::new_v4().to_string();
    let ts = Utc::now().timestamp();
    let ops_body = mesh_auth::canonical_ops_bytes(&ops);
    let sig = mesh_auth::sign(secret, &nonce, ts, &ops_body);
    MeshApplyRequest {
        ops,
        mesh_psk: None,
        nonce: Some(nonce),
        ts: Some(ts),
        sig: Some(sig),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use motodesk_core::entity::EntityType;
    use motodesk_core::sync::{ChangeLogEntry, OpKind};

    fn op(seq: i64) -> ChangeLogEntry {
        let device = Uuid::new_v4();
        let payload = json!({ "id": "x", "last_name": "Martin", "credit_limit_cents": 5000, "notes": null });
        ChangeLogEntry {
            id: Uuid::new_v4(),
            device_id: device,
            seq,
            entity_type: EntityType::Client,
            entity_id: Uuid::new_v4(),
            op: OpKind::Update,
            // Several keys: this is what made the order of a HashMap matter.
            field_meta: crate::local_server::sync_log::field_meta_for_patch(
                &Default::default(),
                &payload,
                device,
            ),
            payload,
            created_at: Utc::now(),
            synced_to_cloud: false,
            relayed: false,
        }
    }

    #[test]
    fn a_signed_batch_still_verifies_after_a_trip_over_the_wire() {
        let secret = "shared-company-secret-0123456789";
        let sent = signed_apply_request(secret, vec![op(1), op(2), op(3)]);

        // Serialise, deserialise, as the two machines do.
        let wire = serde_json::to_vec(&sent).unwrap();
        let received: MeshApplyRequest = serde_json::from_slice(&wire).unwrap();

        let expected = mesh_auth::sign(
            secret,
            received.nonce.as_deref().unwrap(),
            received.ts.unwrap(),
            &mesh_auth::canonical_ops_bytes(&received.ops),
        );
        assert!(mesh_auth::constant_time_eq(received.sig.as_deref().unwrap(), &expected));
    }

    #[test]
    fn tampering_with_an_op_breaks_the_signature() {
        let secret = "shared-company-secret-0123456789";
        let sent = signed_apply_request(secret, vec![op(1)]);
        let wire = serde_json::to_vec(&sent).unwrap();
        let mut received: MeshApplyRequest = serde_json::from_slice(&wire).unwrap();
        received.ops[0].payload = json!({ "last_name": "Pirate" });
        let expected = mesh_auth::sign(
            secret,
            received.nonce.as_deref().unwrap(),
            received.ts.unwrap(),
            &mesh_auth::canonical_ops_bytes(&received.ops),
        );
        assert!(!mesh_auth::constant_time_eq(received.sig.as_deref().unwrap(), &expected));
    }
}
