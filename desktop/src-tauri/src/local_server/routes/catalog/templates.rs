use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::OpKind;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

use super::bool_to_int;
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

pub fn templates_router() -> Router<SharedState> {
    Router::new()
        .route("/", get(templates_list).post(templates_create))
        .route(
            "/:id",
            get(templates_get)
                .patch(templates_patch)
                .delete(templates_delete),
        )
        .route("/:id/duplicate", post(templates_duplicate))
        .route("/:id/set-default", post(templates_set_default))
}

// Document templates
// ===========================================================================

#[derive(Debug, FromRow)]
struct TemplateRow {
    id: String,
    name: String,
    kind: String,
    page_size: String,
    orientation: String,
    schema: String,
    thumbnail_data_url: Option<String>,
    is_default: i64,
    is_archived: i64,
    created_at: String,
    updated_at: String,
}

fn template_to_json(r: &TemplateRow) -> Value {
    let schema: Value = serde_json::from_str(&r.schema).unwrap_or(json!({}));
    json!({
        "id": r.id,
        "name": r.name,
        "kind": r.kind,
        "page_size": r.page_size,
        "orientation": r.orientation,
        "schema": schema,
        "thumbnail_data_url": r.thumbnail_data_url,
        "is_default": r.is_default != 0,
        "is_archived": r.is_archived != 0,
        "created_at": r.created_at,
        "updated_at": r.updated_at,
    })
}

fn template_list_item(r: &TemplateRow) -> Value {
    json!({
        "id": r.id,
        "name": r.name,
        "kind": r.kind,
        "page_size": r.page_size,
        "orientation": r.orientation,
        "thumbnail_data_url": r.thumbnail_data_url,
        "is_default": r.is_default != 0,
        "is_archived": r.is_archived != 0,
        "updated_at": r.updated_at,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct TemplateListQuery {
    pub kind: Option<String>,
    pub include_archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct TemplateInput {
    pub name: String,
    pub kind: String,
    pub page_size: Option<String>,
    pub orientation: Option<String>,
    pub schema: Value,
    pub thumbnail_data_url: Option<String>,
    pub is_default: Option<bool>,
    pub is_archived: Option<bool>,
}

const TEMPLATE_COLS: &str = "id, name, kind, page_size, orientation, schema, thumbnail_data_url, \
     is_default, is_archived, created_at, updated_at";

async fn templates_list(
    State(state): State<SharedState>,
    Query(q): Query<TemplateListQuery>,
) -> LocalResult<Json<Vec<Value>>> {
    let include_archived = q.include_archived.unwrap_or(false);
    let mut sql = format!("SELECT {TEMPLATE_COLS} FROM doc_templates WHERE deleted_at IS NULL");

    let mut bind_kind: Option<String> = None;
    let mut idx: u32 = 0;
    if let Some(k) = q.kind.as_ref().filter(|s| !s.trim().is_empty()) {
        idx += 1;
        sql.push_str(&format!(" AND kind = ${idx}"));
        bind_kind = Some(k.clone());
    }
    if !include_archived {
        sql.push_str(" AND is_archived = 0");
    }
    sql.push_str(" ORDER BY is_default DESC, kind ASC, updated_at DESC");

    let mut query = sqlx::query_as::<_, TemplateRow>(&sql);
    if let Some(k) = bind_kind {
        query = query.bind(k);
    }
    let rows = query.fetch_all(&state.db).await?;
    Ok(Json(rows.iter().map(template_list_item).collect()))
}

async fn templates_get(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    let row = find_template(&state, &id.to_string()).await?;
    Ok(Json(template_to_json(&row)))
}

async fn find_template(state: &SharedState, id: &str) -> LocalResult<TemplateRow> {
    let sql =
        format!("SELECT {TEMPLATE_COLS} FROM doc_templates WHERE id = $1 AND deleted_at IS NULL");
    let row: TemplateRow = sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(LocalError::NotFound)?;
    Ok(row)
}

async fn templates_create(
    State(state): State<SharedState>,
    Json(body): Json<TemplateInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let schema_str = body.schema.to_string();

    sqlx::query(
        r#"INSERT INTO doc_templates (
            id, name, kind, page_size, orientation, schema, thumbnail_data_url,
            is_default, is_archived, created_at, updated_at, row_version, field_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11, 1, '{}')"#,
    )
    .bind(id.to_string())
    .bind(body.name.trim())
    .bind(&body.kind)
    .bind(body.page_size.as_deref().unwrap_or("A4"))
    .bind(body.orientation.as_deref().unwrap_or("portrait"))
    .bind(&schema_str)
    .bind(body.thumbnail_data_url.as_deref())
    .bind(bool_to_int(body.is_default, false))
    .bind(bool_to_int(body.is_archived, false))
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    let row = find_template(&state, &id.to_string()).await?;
    let payload = template_to_json(&row);
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::DocTemplate,
        id,
        OpKind::Insert,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok((StatusCode::CREATED, Json(payload)))
}

async fn templates_patch(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<TemplateInput>,
) -> LocalResult<Json<Value>> {
    let existing = find_template(&state, &id.to_string()).await?;
    let now = Utc::now().to_rfc3339();
    let schema_str = body.schema.to_string();

    sqlx::query(
        r#"UPDATE doc_templates SET
            name               = $2,
            kind               = $3,
            page_size          = $4,
            orientation        = $5,
            schema             = $6,
            thumbnail_data_url = $7,
            is_default         = $8,
            is_archived        = $9,
            updated_at         = $10,
            row_version        = row_version + 1
        WHERE id = $1"#,
    )
    .bind(id.to_string())
    .bind(body.name.trim())
    .bind(&body.kind)
    .bind(body.page_size.as_deref().unwrap_or(&existing.page_size))
    .bind(body.orientation.as_deref().unwrap_or(&existing.orientation))
    .bind(&schema_str)
    .bind(
        body.thumbnail_data_url
            .as_deref()
            .or(existing.thumbnail_data_url.as_deref()),
    )
    .bind(bool_to_int(body.is_default, existing.is_default != 0))
    .bind(bool_to_int(body.is_archived, existing.is_archived != 0))
    .bind(&now)
    .execute(&state.db)
    .await?;

    let row = find_template(&state, &id.to_string()).await?;
    let payload = template_to_json(&row);
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::DocTemplate,
        id,
        OpKind::Update,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(Json(payload))
}

async fn templates_delete(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<StatusCode> {
    let now = Utc::now().to_rfc3339();
    let r = sqlx::query(
        "UPDATE doc_templates SET deleted_at = $2 WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id.to_string())
    .bind(&now)
    .execute(&state.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(LocalError::NotFound);
    }
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::DocTemplate,
        id,
        OpKind::Delete,
        json!({ "id": id }),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

async fn templates_duplicate(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let src = find_template(&state, &id.to_string()).await?;
    let new_id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let new_name = format!("{} (copie)", src.name);

    sqlx::query(
        r#"INSERT INTO doc_templates (
            id, name, kind, page_size, orientation, schema, thumbnail_data_url,
            is_default, is_archived, created_at, updated_at, row_version, field_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7, 0, 0, $8, $9, 1, '{}')"#,
    )
    .bind(new_id.to_string())
    .bind(&new_name)
    .bind(&src.kind)
    .bind(&src.page_size)
    .bind(&src.orientation)
    .bind(&src.schema)
    .bind(src.thumbnail_data_url.as_deref())
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    let row = find_template(&state, &new_id.to_string()).await?;
    let payload = template_to_json(&row);
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::DocTemplate,
        new_id,
        OpKind::Insert,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok((StatusCode::CREATED, Json(payload)))
}

async fn templates_set_default(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    let target = find_template(&state, &id.to_string()).await?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        "UPDATE doc_templates SET is_default = 0 WHERE kind = $1 AND is_default = 1 AND deleted_at IS NULL",
    )
    .bind(&target.kind)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE doc_templates SET is_default = 1, is_archived = 0, updated_at = $2,
         row_version = row_version + 1 WHERE id = $1",
    )
    .bind(id.to_string())
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let row = find_template(&state, &id.to_string()).await?;
    let payload = template_to_json(&row);
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::DocTemplate,
        id,
        OpKind::Update,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(Json(payload))
}
