//! Suppliers and stock ledger (receptions / adjustments).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::OpKind;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/suppliers", get(list_suppliers).post(create_supplier))
        .route(
            "/api/suppliers/:id",
            get(show_supplier).patch(update_supplier).delete(remove_supplier),
        )
        .route("/api/stock-movements", get(list_movements).post(create_movement))
}

#[derive(Debug, Deserialize)]
struct SupplierInput {
    name: String,
    contact: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    address: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MovementInput {
    product_id: String,
    supplier_id: Option<String>,
    kind: Option<String>,
    /// Signed thousandths: positive receives stock, negative is an issue.
    quantity_milli: i64,
    unit_cost_ht_cents: Option<i64>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MovementQuery {
    product_id: Option<String>,
}

async fn list_suppliers(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            r#"SELECT id, name, contact, phone, email, updated_at
               FROM suppliers WHERE deleted_at IS NULL ORDER BY name"#,
        )
        .fetch_all(&state.db)
        .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, name, contact, phone, email, updated_at)| json!({
            "id": id, "name": name, "contact": contact, "phone": phone,
            "email": email, "updated_at": updated_at
        }))
        .collect::<Vec<_>>())))
}

async fn show_supplier(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    let row: Option<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
    )> = sqlx::query_as(
        r#"SELECT id, name, contact, phone, email, address, notes, created_at, updated_at
           FROM suppliers WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?;
    let Some(r) = row else {
        return Err(LocalError::NotFound);
    };
    Ok(Json(json!({
        "id": r.0, "name": r.1, "contact": r.2, "phone": r.3, "email": r.4,
        "address": r.5, "notes": r.6, "created_at": r.7, "updated_at": r.8
    })))
}

async fn create_supplier(
    State(state): State<SharedState>,
    Json(body): Json<SupplierInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(LocalError::BadRequest("le nom du fournisseur est requis".into()));
    }
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"INSERT INTO suppliers (id, name, contact, phone, email, address, notes, created_at, updated_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$8)"#,
    )
    .bind(id.to_string())
    .bind(name)
    .bind(&body.contact)
    .bind(&body.phone)
    .bind(&body.email)
    .bind(&body.address)
    .bind(&body.notes)
    .bind(&now)
    .execute(&state.db)
    .await?;
    let payload = json!({
        "id": id, "name": name, "contact": body.contact, "phone": body.phone,
        "email": body.email, "address": body.address, "notes": body.notes,
        "created_at": now, "updated_at": now
    });
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Supplier,
        id,
        OpKind::Insert,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok((StatusCode::CREATED, Json(payload)))
}

async fn update_supplier(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SupplierInput>,
) -> LocalResult<Json<Value>> {
    let now = Utc::now().to_rfc3339();
    let r = sqlx::query(
        r#"UPDATE suppliers SET name=$2, contact=$3, phone=$4, email=$5, address=$6, notes=$7,
           updated_at=$8, row_version = row_version + 1
           WHERE id=$1 AND deleted_at IS NULL"#,
    )
    .bind(id.to_string())
    .bind(body.name.trim())
    .bind(&body.contact)
    .bind(&body.phone)
    .bind(&body.email)
    .bind(&body.address)
    .bind(&body.notes)
    .bind(&now)
    .execute(&state.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(LocalError::NotFound);
    }
    show_supplier(State(state), Path(id)).await
}

async fn remove_supplier(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<StatusCode> {
    let now = Utc::now().to_rfc3339();
    let r = sqlx::query(
        "UPDATE suppliers SET deleted_at=$2, updated_at=$2 WHERE id=$1 AND deleted_at IS NULL",
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
        EntityType::Supplier,
        id,
        OpKind::Delete,
        json!({ "id": id }),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

async fn list_movements(
    State(state): State<SharedState>,
    Query(q): Query<MovementQuery>,
) -> LocalResult<Json<Value>> {
    let rows: Vec<(
        String,
        String,
        Option<String>,
        String,
        i64,
        Option<i64>,
        Option<String>,
        String,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT m.id, m.product_id, m.supplier_id, m.kind, m.quantity_milli,
               m.unit_cost_ht_cents, m.reason, m.created_at, p.name
        FROM stock_movements m
        LEFT JOIN products p ON p.id = m.product_id
        WHERE m.deleted_at IS NULL AND ($1 IS NULL OR m.product_id = $1)
        ORDER BY m.created_at DESC
        LIMIT 200
        "#,
    )
    .bind(q.product_id.as_deref())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, product_id, supplier_id, kind, qty, cost, reason, created_at, product_name)| {
            json!({
                "id": id, "product_id": product_id, "supplier_id": supplier_id,
                "kind": kind, "quantity_milli": qty, "unit_cost_ht_cents": cost,
                "reason": reason, "created_at": created_at, "product_name": product_name
            })
        })
        .collect::<Vec<_>>())))
}

async fn create_movement(
    State(state): State<SharedState>,
    Json(body): Json<MovementInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    if body.quantity_milli == 0 {
        return Err(LocalError::BadRequest("quantité nulle".into()));
    }
    let kind = match body.kind.as_deref() {
        Some("reception") | Some("adjustment") | Some("inventory") | Some("sale") | Some("credit") => {
            body.kind.clone().unwrap()
        }
        _ => "reception".to_string(),
    };
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        r#"INSERT INTO stock_movements (
            id, product_id, supplier_id, kind, quantity_milli, unit_cost_ht_cents,
            reason, created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$8)"#,
    )
    .bind(id.to_string())
    .bind(&body.product_id)
    .bind(&body.supplier_id)
    .bind(&kind)
    .bind(body.quantity_milli)
    .bind(body.unit_cost_ht_cents)
    .bind(&body.reason)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"UPDATE products SET stock_milli = stock_milli + $2, updated_at = $3,
           row_version = row_version + 1
           WHERE id = $1 AND is_service = 0 AND deleted_at IS NULL"#,
    )
    .bind(&body.product_id)
    .bind(body.quantity_milli)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let payload = json!({
        "id": id, "product_id": body.product_id, "supplier_id": body.supplier_id,
        "kind": kind, "quantity_milli": body.quantity_milli,
        "unit_cost_ht_cents": body.unit_cost_ht_cents, "reason": body.reason,
        "created_at": now, "updated_at": now
    });
    if let Ok(pid) = Uuid::parse_str(&id.to_string()) {
        let device_id = state.device_id().await?;
        let _ = sync_log::append(
            &state.db,
            device_id,
            EntityType::StockMovement,
            pid,
            OpKind::Insert,
            payload.clone(),
            None,
        )
        .await?;
    }
    state.notify_sync();
    Ok((StatusCode::CREATED, Json(payload)))
}

/// Ledger row for a sealed sale / credit note. Does not change quantity —
/// the caller already applied `apply_stock_movements`.
pub async fn record_sale_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    lines: &[crate::local_server::routes::sales::LineInputBody],
    sale_id: &str,
    kind: &str,
    now: &str,
) -> LocalResult<()> {
    for line in lines {
        let Some(pid) = line.product_id.as_deref() else {
            continue;
        };
        if line.quantity_milli == 0 {
            continue;
        }
        sqlx::query(
            r#"INSERT INTO stock_movements (
                id, product_id, kind, quantity_milli, sale_id, created_at, updated_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$6)"#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(pid)
        .bind(kind)
        .bind(-line.quantity_milli)
        .bind(sale_id)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
