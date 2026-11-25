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
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::routes::sales::{self, LineInputBody, SaleInput};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/workshop-orders", get(list).post(create))
        .route(
            "/api/workshop-orders/:id",
            get(show).patch(update).delete(remove),
        )
        .route("/api/workshop-orders/:id/invoice", post(invoice))
        .route("/api/workshop-orders/from-quote/:id", post(from_quote))
}

/// Open a repair order from an accepted quote: same customer, same vehicle, same
/// lines, so the work is done and billed on what was agreed. The quote is kept
/// (and numbered if it was not yet); asking again returns the order already made.
async fn from_quote(
    State(state): State<SharedState>,
    Path(quote_id): Path<Uuid>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let quote = sales::load_sale(&state, quote_id).await?;
    if quote.get("status").and_then(|v| v.as_str()) != Some("quote") {
        return Err(LocalError::BadRequest("seul un devis peut ouvrir un ordre de réparation".into()));
    }
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM workshop_orders WHERE source_quote_id = $1 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(quote_id.to_string())
    .fetch_optional(&state.db)
    .await?;
    if let Some((order_id,)) = existing {
        let order_id = Uuid::parse_str(&order_id).map_err(|e| LocalError::Internal(e.into()))?;
        return Ok((StatusCode::OK, Json(show_payload(&state, order_id).await?)));
    }

    let Json(quote) = sales::issue_quote(State(state.clone()), Path(quote_id)).await?;
    let reference = quote.get("reference").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let lines: Vec<Value> = quote
        .get("lines")
        .and_then(|v| v.as_array())
        .map(|lines| {
            lines
                .iter()
                .map(|l| {
                    let ttc = l.get("price_basis").and_then(|v| v.as_str()) == Some("ttc");
                    json!({
                        "product_id": l.get("product_id"),
                        "label": l.get("label"),
                        "quantity_milli": l.get("quantity_milli"),
                        "unit_price_ht_cents": l.get("unit_price_ht_cents").and_then(|v| v.as_i64()).unwrap_or(0),
                        "unit_price_ttc_cents": if ttc { l.get("unit_price_ttc_cents").cloned().unwrap_or(Value::Null) } else { Value::Null },
                        "vat_rate_bp": l.get("vat_rate_bp"),
                        "discount_bp": l.get("discount_bp"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let body: OrderInput = serde_json::from_value(json!({
        "client_id": quote.get("client_id"),
        "vehicle_id": quote.get("vehicle_id"),
        "label": format!("Devis {reference}"),
        "notes": quote.get("notes"),
        "status": "pending",
        "lines": lines,
    }))
    .map_err(|e| LocalError::BadRequest(e.to_string()))?;

    let (status, Json(order)) = create(State(state.clone()), Json(body)).await?;
    let order_id = order.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    sqlx::query("UPDATE workshop_orders SET source_quote_id = $2 WHERE id = $1")
        .bind(&order_id)
        .bind(quote_id.to_string())
        .execute(&state.db)
        .await?;
    // The link was written after the order was announced to the other devices:
    // announce it again so they know which quote this work was agreed on.
    let order_uuid = Uuid::parse_str(&order_id).map_err(|e| LocalError::Internal(e.into()))?;
    let order = show_payload(&state, order_uuid).await?;
    let device_id = state.device_id().await?;
    let _ = sync_log::append(&state.db, device_id, EntityType::WorkshopOrder, order_uuid, OpKind::Update, order.clone(), None).await?;
    state.notify_sync();
    Ok((status, Json(order)))
}

#[derive(Debug, Deserialize, Default)]
struct ListQuery {
    status: Option<String>,
    client_id: Option<String>,
    #[allow(dead_code)]
    q: Option<String>,
    /// The Kanban board: every open order, plus the recently invoiced ones.
    board: Option<bool>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct LineBody {
    id: Option<String>,
    product_id: Option<String>,
    label: Option<String>,
    quantity_milli: Option<i64>,
    unit_price_ht_cents: Option<i64>,
    /// A shelf price; when set it is the price of record (HT is then a display figure).
    unit_price_ttc_cents: Option<i64>,
    vat_rate_bp: Option<i64>,
    discount_bp: Option<i64>,
}

/// Tells "field absent" (leave it alone) from "field is null" (clear it). With a
/// plain `Option` both arrive as `None`, so the update could not clear anything
/// and answered "enregistré" while keeping the old value.
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
struct OrderInput {
    #[serde(default, deserialize_with = "present")]
    client_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    vehicle_id: Option<Option<String>>,
    sale_id: Option<String>,
    #[serde(default, deserialize_with = "present")]
    label: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    promised_at: Option<Option<String>>,
    received_at: Option<String>,
    #[serde(default, deserialize_with = "present")]
    mileage: Option<Option<i64>>,
    #[serde(default, deserialize_with = "present")]
    fuel_level: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    planned_hours: Option<Option<f64>>,
    #[serde(default, deserialize_with = "present")]
    notes: Option<Option<String>>,
    status: Option<String>,
    #[serde(default, deserialize_with = "present")]
    assigned_to: Option<Option<String>>,
    #[serde(default)]
    lines: Option<Vec<LineBody>>,
}

/// The value to store: what the request says if it says anything (including
/// "clear it"), otherwise what is already there.
fn resolve<T: Clone>(sent: &Option<Option<T>>, current: Option<T>) -> Option<T> {
    match sent {
        Some(value) => value.clone(),
        None => current,
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

const STATUSES: &[&str] = &["pending", "in_progress", "ready", "invoiced"];

fn normalise_status(s: Option<&str>, fallback: &str) -> String {
    match s {
        Some(v) if STATUSES.contains(&v) => v.to_string(),
        _ => fallback.to_string(),
    }
}

fn lines_to_sale_inputs(lines: &[Value]) -> Vec<LineInputBody> {
    lines
        .iter()
        .filter_map(|l| {
            let label = l
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let qty = l.get("quantity_milli").and_then(|v| v.as_i64()).unwrap_or(0);
            if label.is_empty() || qty == 0 {
                return None;
            }
            Some(LineInputBody {
                product_id: l
                    .get("product_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty()),
                label: Some(label),
                quantity_milli: qty,
                unit_price_ht_cents: l
                    .get("unit_price_ht_cents")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                vat_rate_bp: l.get("vat_rate_bp").and_then(|v| v.as_i64()).unwrap_or(2_000),
                discount_bp: l.get("discount_bp").and_then(|v| v.as_i64()).unwrap_or(0),
                unit_price_ttc_cents: l.get("unit_price_ttc_cents").and_then(|v| v.as_i64()),
            })
        })
        .collect()
}

async fn list(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> LocalResult<Json<Value>> {
    let status = q.status.as_deref().filter(|s| STATUSES.contains(s));
    let board = q.board.unwrap_or(false);
    let limit = q.limit.unwrap_or(500).clamp(1, 2_000);
    let rows = sqlx::query_as::<_, OrderRow>(
        r#"
        SELECT w.id, w.sale_id, w.client_id, w.vehicle_id, w.received_at, w.promised_at,
               w.label, w.mileage, w.fuel_level, w.planned_hours, w.notes, w.status,
               w.assigned_to, w.created_at, w.updated_at,
               TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS client_name,
               CASE WHEN v.id IS NULL THEN NULL ELSE TRIM(v.make || ' ' || v.model) END AS vehicle_label
        FROM workshop_orders w
        LEFT JOIN clients c ON c.id = w.client_id
        LEFT JOIN vehicles v ON v.id = w.vehicle_id
        WHERE w.deleted_at IS NULL
          AND ($1 IS NULL OR w.status = $1)
          AND ($2 IS NULL OR w.client_id = $2)
          AND (
            $3 = 0
            -- The board: what is being worked on, whatever its age, and what was
            -- invoiced lately. Imported history (legacy_ref) is not a workshop
            -- queue, and 12 000 old orders loaded as "En attente" hid every new
            -- one behind the 500-row cap.
            OR (w.legacy_ref IS NULL AND (
                  w.status != 'invoiced'
                  OR w.updated_at >= strftime('%Y-%m-%dT%H:%M:%S', 'now', '-30 days')
               ))
          )
        ORDER BY
          CASE WHEN $3 = 1 AND w.status != 'invoiced' THEN 0 ELSE 1 END,
          CASE WHEN $3 = 1 AND w.status != 'invoiced' THEN COALESCE(w.promised_at, w.updated_at) END ASC,
          w.updated_at DESC
        LIMIT $4
        "#,
    )
    .bind(status)
    .bind(q.client_id.as_deref())
    .bind(i64::from(board))
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    let items: Vec<Value> = rows.into_iter().map(row_json).collect();
    Ok(Json(json!(items)))
}

#[derive(sqlx::FromRow)]
struct OrderRow {
    id: String,
    sale_id: Option<String>,
    client_id: Option<String>,
    vehicle_id: Option<String>,
    received_at: Option<String>,
    promised_at: Option<String>,
    label: Option<String>,
    mileage: Option<i64>,
    fuel_level: Option<String>,
    planned_hours: Option<f64>,
    notes: Option<String>,
    status: String,
    assigned_to: Option<String>,
    created_at: String,
    updated_at: String,
    client_name: Option<String>,
    vehicle_label: Option<String>,
}

#[derive(sqlx::FromRow)]
struct LineRow {
    id: String,
    workshop_order_id: String,
    product_id: Option<String>,
    label: Option<String>,
    quantity_milli: i64,
    unit_price_ht_cents: i64,
    unit_price_ttc_cents: Option<i64>,
    vat_rate_bp: i64,
    discount_bp: i64,
    position: i64,
}

fn row_json(r: OrderRow) -> Value {
    json!({
        "id": r.id,
        "sale_id": r.sale_id,
        "client_id": r.client_id,
        "vehicle_id": r.vehicle_id,
        "received_at": r.received_at,
        "promised_at": r.promised_at,
        "label": r.label,
        "mileage": r.mileage,
        "fuel_level": r.fuel_level,
        "planned_hours": r.planned_hours,
        "notes": r.notes,
        "status": r.status,
        "assigned_to": r.assigned_to,
        "created_at": r.created_at,
        "updated_at": r.updated_at,
        "client_name": r.client_name.filter(|s| !s.trim().is_empty()),
        "vehicle_label": r.vehicle_label.filter(|s| !s.trim().is_empty()),
    })
}

fn line_json(r: LineRow) -> Value {
    json!({
        "id": r.id,
        "workshop_order_id": r.workshop_order_id,
        "product_id": r.product_id,
        "label": r.label,
        "quantity_milli": r.quantity_milli,
        "unit_price_ht_cents": r.unit_price_ht_cents,
        "unit_price_ttc_cents": r.unit_price_ttc_cents,
        "vat_rate_bp": r.vat_rate_bp,
        "discount_bp": r.discount_bp,
        "position": r.position,
    })
}

async fn load_lines(state: &SharedState, order_id: &str) -> LocalResult<Vec<Value>> {
    let rows = sqlx::query_as::<_, LineRow>(
        r#"
        SELECT id, workshop_order_id, product_id, label, quantity_milli,
               unit_price_ht_cents, unit_price_ttc_cents, vat_rate_bp, discount_bp, position
        FROM workshop_order_lines
        WHERE workshop_order_id = $1
        ORDER BY position ASC, created_at ASC
        "#,
    )
    .bind(order_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(line_json).collect())
}

async fn replace_lines(
    state: &SharedState,
    order_id: &str,
    lines: &[LineBody],
    now: &str,
) -> LocalResult<()> {
    sqlx::query("DELETE FROM workshop_order_lines WHERE workshop_order_id = $1")
        .bind(order_id)
        .execute(&state.db)
        .await?;
    for (i, line) in lines.iter().enumerate() {
        let id = line
            .id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::new_v4);
        sqlx::query(
            r#"
            INSERT INTO workshop_order_lines (
                id, workshop_order_id, product_id, label, quantity_milli,
                unit_price_ht_cents, vat_rate_bp, discount_bp, position, created_at, updated_at,
                unit_price_ttc_cents
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10,$11)
            "#,
        )
        .bind(id.to_string())
        .bind(order_id)
        .bind(&line.product_id)
        .bind(line.label.as_deref().unwrap_or(""))
        .bind(line.quantity_milli.unwrap_or(1_000))
        .bind(line.unit_price_ht_cents.unwrap_or(0))
        .bind(line.vat_rate_bp.unwrap_or(2_000))
        .bind(line.discount_bp.unwrap_or(0))
        .bind(i as i64)
        .bind(now)
        .bind(line.unit_price_ttc_cents)
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

async fn show(State(state): State<SharedState>, Path(id): Path<Uuid>) -> LocalResult<Json<Value>> {
    let row = sqlx::query_as::<_, OrderRow>(
        r#"
        SELECT w.id, w.sale_id, w.client_id, w.vehicle_id, w.received_at, w.promised_at,
               w.label, w.mileage, w.fuel_level, w.planned_hours, w.notes, w.status,
               w.assigned_to, w.created_at, w.updated_at,
               TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS client_name,
               CASE WHEN v.id IS NULL THEN NULL ELSE TRIM(v.make || ' ' || v.model) END AS vehicle_label
        FROM workshop_orders w
        LEFT JOIN clients c ON c.id = w.client_id
        LEFT JOIN vehicles v ON v.id = w.vehicle_id
        WHERE w.id = $1 AND w.deleted_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?
    .ok_or(LocalError::NotFound)?;
    let mut payload = row_json(row);
    let lines = load_lines(&state, &id.to_string()).await?;
    let source_quote: Option<(Option<String>,)> =
        sqlx::query_as("SELECT source_quote_id FROM workshop_orders WHERE id = $1")
            .bind(id.to_string())
            .fetch_optional(&state.db)
            .await?;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("lines".into(), json!(lines));
        // Which quote the work was agreed on: it travels with the order.
        obj.insert("source_quote_id".into(), json!(source_quote.and_then(|(q,)| q)));
    }
    Ok(Json(payload))
}

async fn create(
    State(state): State<SharedState>,
    Json(body): Json<OrderInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    // "Facturé" is a fact about an invoice, never something to start from.
    let status = match normalise_status(body.status.as_deref(), "pending").as_str() {
        "invoiced" => "pending".to_string(),
        other => other.to_string(),
    };
    sqlx::query(
        r#"
        INSERT INTO workshop_orders (
            id, sale_id, client_id, vehicle_id, received_at, promised_at, label,
            mileage, fuel_level, planned_hours, notes, status, assigned_to,
            created_at, updated_at, row_version, field_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$14,1,'{}')
        "#,
    )
    .bind(id.to_string())
    .bind(&body.sale_id)
    .bind(body.client_id.clone().flatten())
    .bind(body.vehicle_id.clone().flatten())
    .bind(body.received_at.as_deref().unwrap_or(&now))
    .bind(body.promised_at.clone().flatten())
    .bind(
        body.label
            .clone()
            .flatten()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| "Ordre de réparation".to_string()),
    )
    .bind(body.mileage.flatten())
    .bind(body.fuel_level.clone().flatten())
    .bind(body.planned_hours.flatten())
    .bind(body.notes.clone().flatten())
    .bind(&status)
    .bind(body.assigned_to.clone().flatten())
    .bind(&now)
    .execute(&state.db)
    .await?;

    if let Some(lines) = body.lines.as_deref() {
        replace_lines(&state, &id.to_string(), lines, &now).await?;
    }

    let payload = show_payload(&state, id).await?;
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::WorkshopOrder,
        id,
        OpKind::Insert,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok((StatusCode::CREATED, Json(payload)))
}

async fn update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<OrderInput>,
) -> LocalResult<Json<Value>> {
    let now = Utc::now().to_rfc3339();
    let existing = show_payload(&state, id).await?;
    let current_status = existing["status"].as_str().unwrap_or("pending").to_string();

    // An invoiced order is part of a sealed sale's story: it no longer changes.
    if current_status == "invoiced" {
        return Err(LocalError::BadRequest(
            "cet ordre est facturé et ne peut plus être modifié ; pour corriger, établissez un avoir sur sa facture".into(),
        ));
    }
    let status = normalise_status(body.status.as_deref(), &current_status);
    if status == "invoiced" {
        // Only sealing the invoice makes an order "Facturé" (`after_sale_sealed`).
        // Setting it by hand marked orders invoiced that had no invoice at all.
        return Err(LocalError::BadRequest(
            "un ordre ne passe à « Facturé » que lorsque sa facture est validée ; utilisez « Facturer »".into(),
        ));
    }

    // Absent fields keep their value; `null` clears them.
    let client_id = resolve(&body.client_id, text(&existing, "client_id"));
    let mut vehicle_id = resolve(&body.vehicle_id, text(&existing, "vehicle_id"));
    if body.client_id.is_some() && client_id != text(&existing, "client_id") && body.vehicle_id.is_none() {
        // A vehicle belongs to a customer: changing the customer drops the link.
        vehicle_id = None;
    }
    let promised_at = resolve(&body.promised_at, text(&existing, "promised_at"));
    let label = resolve(&body.label, text(&existing, "label"))
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| "Ordre de réparation".to_string());
    let mileage = resolve(&body.mileage, existing["mileage"].as_i64());
    let fuel_level = resolve(&body.fuel_level, text(&existing, "fuel_level"));
    let planned_hours = resolve(&body.planned_hours, existing["planned_hours"].as_f64());
    let notes = resolve(&body.notes, text(&existing, "notes"));
    let assigned_to = resolve(&body.assigned_to, text(&existing, "assigned_to"));

    sqlx::query(
        r#"
        UPDATE workshop_orders SET
            client_id = $2, vehicle_id = $3, promised_at = $4, label = $5,
            mileage = $6, fuel_level = $7, planned_hours = $8, notes = $9,
            status = $10, assigned_to = $11,
            updated_at = $12, row_version = row_version + 1
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .bind(&client_id)
    .bind(&vehicle_id)
    .bind(&promised_at)
    .bind(&label)
    .bind(mileage)
    .bind(&fuel_level)
    .bind(planned_hours)
    .bind(&notes)
    .bind(&status)
    .bind(&assigned_to)
    .bind(&now)
    .execute(&state.db)
    .await?;

    if let Some(lines) = body.lines.as_deref() {
        replace_lines(&state, &id.to_string(), lines, &now).await?;
    }

    let payload = show_payload(&state, id).await?;
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::WorkshopOrder,
        id,
        OpKind::Update,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(Json(payload))
}

async fn remove(State(state): State<SharedState>, Path(id): Path<Uuid>) -> LocalResult<StatusCode> {
    let now = Utc::now().to_rfc3339();
    let r = sqlx::query(
        "UPDATE workshop_orders SET deleted_at = $2, updated_at = $2 WHERE id = $1 AND deleted_at IS NULL",
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
        EntityType::WorkshopOrder,
        id,
        OpKind::Delete,
        json!({ "id": id.to_string() }),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

/// What a repair order becomes on an invoice.
fn invoice_input(order: &Value, lines: Vec<LineInputBody>) -> SaleInput {
    let notes = match order.get("label").and_then(|v| v.as_str()) {
        Some(label) if !label.is_empty() => Some(format!("OR {label}")),
        _ => Some("Ordre de réparation".into()),
    };
    SaleInput {
        client_id: text(order, "client_id"),
        vehicle_id: text(order, "vehicle_id"),
        sale_date: None,
        notes,
        status: Some("draft".into()),
        document_kind: Some("invoice".into()),
        lines,
    }
}

/// New V2 draft only. An imported Motogest piece (`legacy_ref`) is never reused.
async fn invoice(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let order = show_payload(&state, id).await?;
    let empty: Vec<Value> = Vec::new();
    let raw_lines = order.get("lines").and_then(|v| v.as_array()).unwrap_or(&empty);
    let sale_lines = lines_to_sale_inputs(raw_lines);
    if sale_lines.is_empty() {
        return Err(LocalError::BadRequest(
            "un ordre sans ligne ne peut pas être facturé".into(),
        ));
    }

    if order.get("status").and_then(|v| v.as_str()) == Some("invoiced") {
        return Err(LocalError::BadRequest(
            "cet ordre est déjà facturé ; pour le corriger, établissez un avoir sur sa facture".into(),
        ));
    }

    if let Some(sid) = order.get("sale_id").and_then(|v| v.as_str()) {
        if let Ok(existing_id) = Uuid::parse_str(sid) {
            let imported: Option<(Option<String>,)> = sqlx::query_as(
                "SELECT legacy_ref FROM sales WHERE id = $1 AND deleted_at IS NULL",
            )
            .bind(sid)
            .fetch_optional(&state.db)
            .await?;
            let imported = imported
                .and_then(|r| r.0)
                .is_some_and(|s| !s.is_empty());
            if let Ok(sale) = sales::load_sale(&state, existing_id).await {
                let locked = sale.get("locked_at").and_then(|v| v.as_str()).is_some();
                if locked && !imported {
                    // Sealed already: a second invoice for the same work is a
                    // double billing, whatever the order says.
                    return Err(LocalError::BadRequest(format!(
                        "la facture {} de cet ordre est déjà validée ; pour la corriger, établissez un avoir",
                        sale.get("reference").and_then(|v| v.as_str()).unwrap_or("")
                    )));
                }
                if !locked && !imported {
                    // The draft is still open: bring it up to date with the order
                    // (parts added since it was made) instead of returning it as it
                    // was and announcing a new one.
                    let Json(mut refreshed) = sales::update_draft(
                        &state,
                        existing_id,
                        invoice_input(&order, sale_lines.clone()),
                    )
                    .await?;
                    if let Some(obj) = refreshed.as_object_mut() {
                        obj.insert("refreshed_from_order".into(), json!(true));
                    }
                    return Ok((StatusCode::OK, Json(refreshed)));
                }
            }
        }
    }

    let (status, Json(sale)) =
        sales::create(State(state.clone()), Json(invoice_input(&order, sale_lines))).await?;

    let sale_id = sale
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LocalError::Internal(anyhow::anyhow!("facture créée sans id")))?;
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE workshop_orders
        SET sale_id = $2, updated_at = $3, row_version = row_version + 1
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .bind(sale_id)
    .bind(&now)
    .execute(&state.db)
    .await?;

    // The link to the invoice is data like any other: without a change-log entry
    // another PC did not know the order had an invoice and billed it again.
    let payload = show_payload(&state, id).await?;
    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::WorkshopOrder,
        id,
        OpKind::Update,
        payload,
        None,
    )
    .await?;
    state.notify_sync();

    Ok((status, Json(sale)))
}

pub(crate) async fn show_payload(state: &SharedState, id: Uuid) -> LocalResult<Value> {
    let Json(v) = show(State(state.clone()), Path(id)).await?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::routes::sales::tests as sales_tests;
    use crate::local_server::test_support::shop;

    fn part_line() -> LineBody {
        LineBody {
            id: None,
            product_id: None,
            label: Some("Filtre à huile".into()),
            quantity_milli: Some(1_000),
            unit_price_ht_cents: Some(1_200),
            unit_price_ttc_cents: None,
            vat_rate_bp: Some(2_000),
            discount_bp: Some(0),
        }
    }

    /// A part from the catalogue keeps its shelf price from the repair order to
    /// the invoice: 9,99 EUR must not become 10,00.
    #[tokio::test]
    async fn a_shelf_priced_part_reaches_the_invoice_at_its_label_price() {
        let state = shop().await;
        let lines = vec![
            LineBody {
                label: Some("Valve".into()),
                unit_price_ht_cents: Some(833),
                unit_price_ttc_cents: Some(999),
                quantity_milli: Some(3_000),
                ..part_line()
            },
            part_line(),
        ];
        let order_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO workshop_orders (id, status, created_at, updated_at, row_version, field_meta)
             VALUES ($1, 'pending', $2, $2, 1, '{}')",
        )
        .bind(&order_id)
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();
        replace_lines(&state, &order_id, &lines, &now).await.unwrap();

        let stored = load_lines(&state, &order_id).await.unwrap();
        assert_eq!(stored[0]["unit_price_ttc_cents"], 999);
        assert!(stored[1]["unit_price_ttc_cents"].is_null());

        let inputs = lines_to_sale_inputs(&stored);
        let totals = sales_tests::totals_of(&inputs);
        assert_eq!(totals.total_ttc_cents, 2_997 + 1_440, "3 x 9,99 + 12,00 HT labour part");
    }

    #[test]
    fn empty_lines_cannot_become_a_sale() {
        assert!(lines_to_sale_inputs(&[]).is_empty());
        assert!(lines_to_sale_inputs(&[json!({"label": "  ", "quantity_milli": 1000})]).is_empty());
    }

    #[test]
    fn maps_or_lines_onto_sale_inputs() {
        let mapped = lines_to_sale_inputs(&[json!({
            "product_id": "p1",
            "label": "  Filtre  ",
            "quantity_milli": 1000,
            "unit_price_ht_cents": 1200,
            "vat_rate_bp": 2000,
            "discount_bp": 0
        })]);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].label.as_deref(), Some("Filtre"));
        assert_eq!(mapped[0].unit_price_ht_cents, 1_200);
    }

    /// Requests arrive as JSON, so tests build them the same way: that is what
    /// tells a field that is absent from one that is explicitly `null`.
    fn input(v: Value) -> Json<OrderInput> {
        Json(serde_json::from_value(v).expect("a valid order body"))
    }

    fn line_json() -> Value {
        json!({
            "label": "Filtre à huile",
            "quantity_milli": 1_000,
            "unit_price_ht_cents": 1_200,
            "vat_rate_bp": 2_000,
            "discount_bp": 0
        })
    }

    async fn new_order(state: &SharedState, body: Value) -> Uuid {
        let (_, Json(order)) = create(State(state.clone()), input(body)).await.unwrap();
        Uuid::parse_str(order["id"].as_str().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn invoice_refuses_zero_lines() {
        let state = shop().await;
        let (_, Json(order)) = create(
            State(state.clone()),
            input(json!({ "label": "Révision", "lines": [] })),
        )
        .await
        .unwrap();
        let id = Uuid::parse_str(order["id"].as_str().unwrap()).unwrap();
        let err = invoice(State(state), Path(id)).await.unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(msg) if msg.contains("sans ligne")));
    }

    #[tokio::test]
    async fn invoice_opens_a_draft_and_seal_marks_invoiced() {
        let state = shop().await;
        let (_, Json(order)) = create(
            State(state.clone()),
            input(json!({
                "label": "Révision", "mileage": 24_000, "fuel_level": "1/2",
                "status": "ready", "lines": [line_json()]
            })),
        )
        .await
        .unwrap();
        let id = Uuid::parse_str(order["id"].as_str().unwrap()).unwrap();

        let (_, Json(sale)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(sale["status"], "draft");
        assert!(sale["locked_at"].is_null());

        let Json(before) = show(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(before["status"], "ready");
        assert_eq!(before["sale_id"], sale["id"]);

        let sale_id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();
        sales_tests::seal(&state, sale_id).await;

        let Json(after) = show(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(after["status"], "invoiced");

        // The status change and the link were journalled for the other PCs.
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM change_log WHERE entity_type = 'workshop_order' AND entity_id = $1 AND op = 'update'",
        )
        .bind(id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert!(n >= 2, "link and status must both reach the change log, found {n}");
    }

    #[tokio::test]
    async fn a_sealed_invoice_cannot_be_billed_a_second_time() {
        let state = shop().await;
        let id = new_order(&state, json!({ "status": "ready", "lines": [line_json()] })).await;
        let (_, Json(sale)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        let sale_id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();
        sales_tests::seal(&state, sale_id).await;

        let again = invoice(State(state.clone()), Path(id)).await.unwrap_err();
        assert!(matches!(&again, LocalError::BadRequest(m) if m.contains("déjà facturé")), "{again:?}");

        // Even if the order were pulled back to an open status, the sealed invoice
        // is still found and still refuses a second one.
        sqlx::query("UPDATE workshop_orders SET status = 'ready' WHERE id = $1")
            .bind(id.to_string())
            .execute(&state.db)
            .await
            .unwrap();
        let again = invoice(State(state.clone()), Path(id)).await.unwrap_err();
        assert!(matches!(&again, LocalError::BadRequest(m) if m.contains("déjà validée")), "{again:?}");
        let (drafts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sales WHERE locked_at IS NULL")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(drafts, 0, "no second draft was created");
    }

    #[tokio::test]
    async fn invoicing_again_brings_the_open_draft_up_to_date() {
        let state = shop().await;
        let id = new_order(&state, json!({ "status": "ready", "lines": [line_json()] })).await;
        let (_, Json(first)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(first["lines"].as_array().unwrap().len(), 1);

        // A part is added to the order after the draft was made.
        let _ = update(
            State(state.clone()),
            Path(id),
            input(json!({ "lines": [line_json(), { "label": "Plaquettes", "quantity_milli": 1000,
                "unit_price_ht_cents": 4_000, "vat_rate_bp": 2000 }] })),
        )
        .await
        .unwrap();

        let (status, Json(second)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(second["id"], first["id"], "the same draft, not a second invoice");
        assert_eq!(second["lines"].as_array().unwrap().len(), 2, "with the new part on it");
        assert_eq!(second["refreshed_from_order"], true);
        assert_eq!(second["total_ttc_cents"], (1_200 + 4_000) * 12 / 10);
    }

    /// A repair order knows nothing about the invoice date, so bringing its draft
    /// up to date sends none. That used to erase the date the draft had, and the
    /// invoice was then validated, numbered and sealed with an empty date.
    #[tokio::test]
    async fn invoicing_again_leaves_the_draft_dated() {
        let state = shop().await;
        let id = new_order(&state, json!({ "status": "ready", "lines": [line_json()] })).await;
        let (_, Json(first)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        assert!(first["sale_date"].is_string(), "a new draft is dated");

        let (_, Json(second)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(second["id"], first["id"]);
        assert_eq!(second["sale_date"], first["sale_date"], "still dated after the refresh");

        // A date the user chose on the draft survives the refresh as well.
        sqlx::query("UPDATE sales SET sale_date = '2026-10-03' WHERE id = $1")
            .bind(first["id"].as_str().unwrap())
            .execute(&state.db)
            .await
            .unwrap();
        let (_, Json(third)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(third["sale_date"], "2026-10-03");

        let sale_id = Uuid::parse_str(first["id"].as_str().unwrap()).unwrap();
        let sealed = sales_tests::seal(&state, sale_id).await;
        assert!(sealed["locked_at"].is_string());
        assert_eq!(sealed["sale_date"], "2026-10-03", "and the invoice is sealed with it");
    }

    #[tokio::test]
    async fn invoiced_is_only_reached_by_sealing_an_invoice() {
        let state = shop().await;
        let id = new_order(&state, json!({ "status": "ready", "lines": [line_json()] })).await;

        let by_hand = update(State(state.clone()), Path(id), input(json!({ "status": "invoiced" })))
            .await
            .unwrap_err();
        assert!(matches!(&by_hand, LocalError::BadRequest(m) if m.contains("Facturer")), "{by_hand:?}");
        let born = new_order(&state, json!({ "status": "invoiced" })).await;
        let Json(row) = show(State(state.clone()), Path(born)).await.unwrap();
        assert_eq!(row["status"], "pending", "an order cannot be created as invoiced");

        // Once invoiced for real, the order is frozen.
        let (_, Json(sale)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        sales_tests::seal(&state, Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap()).await;
        let back = update(State(state.clone()), Path(id), input(json!({ "status": "ready" })))
            .await
            .unwrap_err();
        assert!(matches!(&back, LocalError::BadRequest(m) if m.contains("facturé")), "{back:?}");
        let edit = update(State(state), Path(id), input(json!({ "notes": "x" }))).await.unwrap_err();
        assert!(matches!(edit, LocalError::BadRequest(_)));
    }

    #[tokio::test]
    async fn a_cleared_field_is_cleared_and_a_missing_one_is_left_alone() {
        let state = shop().await;
        let id = new_order(
            &state,
            json!({ "label": "Révision", "notes": "Bruit de chaîne", "mileage": 24_000,
                    "promised_at": "2026-12-01", "assigned_to": "mecano-1", "fuel_level": "1/2" }),
        )
        .await;

        // The kanban moves a card by sending nothing but the status.
        let Json(moved) = update(State(state.clone()), Path(id), input(json!({ "status": "in_progress" })))
            .await
            .unwrap();
        assert_eq!(moved["status"], "in_progress");
        assert_eq!(moved["notes"], "Bruit de chaîne");
        assert_eq!(moved["mileage"], 24_000);
        assert_eq!(moved["assigned_to"], "mecano-1");

        // The form sends null for a field the user emptied.
        let Json(cleared) = update(
            State(state.clone()),
            Path(id),
            input(json!({ "notes": null, "assigned_to": null, "promised_at": null, "mileage": null })),
        )
        .await
        .unwrap();
        assert!(cleared["notes"].is_null());
        assert!(cleared["assigned_to"].is_null());
        assert!(cleared["promised_at"].is_null());
        assert!(cleared["mileage"].is_null());
        assert_eq!(cleared["label"], "Révision", "untouched fields stay");
        assert_eq!(cleared["fuel_level"], "1/2");
        assert_eq!(cleared["status"], "in_progress");
    }

    #[tokio::test]
    async fn the_board_shows_every_open_order_but_not_imported_history() {
        let state = shop().await;
        let now = chrono::Utc::now().to_rfc3339();
        // 600 imported, "pending" by default after the bulk load; far more than the
        // old 500-row cap, and older than anything real.
        for i in 0..600 {
            sqlx::query(
                "INSERT INTO workshop_orders (id, status, legacy_ref, created_at, updated_at, row_version, field_meta)
                 VALUES ($1, 'pending', $2, '2015-01-01T00:00:00Z', '2015-01-01T00:00:00Z', 1, '{}')",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(format!("OR-{i}"))
            .execute(&state.db)
            .await
            .unwrap();
        }
        let fresh = new_order(&state, json!({ "label": "Nouvel OR", "lines": [] })).await;
        sqlx::query(
            "INSERT INTO workshop_orders (id, status, created_at, updated_at, row_version, field_meta)
             VALUES ($1, 'invoiced', $2, '2020-01-01T00:00:00Z', 1, '{}')",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();

        let Json(board) = list(
            State(state.clone()),
            Query(ListQuery { board: Some(true), ..Default::default() }),
        )
        .await
        .unwrap();
        let ids: Vec<&str> = board.as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec![fresh.to_string().as_str()], "the new order is on the board, history and old invoices are not");

        // Asking for everything still works (client history, search).
        let Json(all) = list(
            State(state),
            Query(ListQuery { limit: Some(2_000), ..Default::default() }),
        )
        .await
        .unwrap();
        assert!(all.as_array().unwrap().len() > 600);
    }

    #[tokio::test]
    async fn crediting_the_whole_invoice_reopens_the_order_for_billing() {
        use crate::local_server::routes::sales::{credit_note, CreditNoteBody};
        let state = shop().await;
        let id = new_order(&state, json!({ "status": "ready", "lines": [line_json()] })).await;
        let (_, Json(sale)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        let sale_id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();
        sales_tests::seal(&state, sale_id).await;
        let Json(billed) = show(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(billed["status"], "invoiced");

        let (_, Json(note)) = credit_note(State(state.clone()), Path(sale_id), Json(CreditNoteBody::default()))
            .await
            .unwrap();
        assert_eq!(note["status"], "credit_note");
        assert!(note["locked_at"].is_string(), "the avoir is sealed in the same step");

        let Json(after) = show(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(after["status"], "ready", "the cancelled invoice no longer bills the order");
        assert!(after["sale_id"].is_null());
        // And it can be billed again.
        let (_, Json(second)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        assert_ne!(second["id"], sale["id"]);
    }

    #[tokio::test]
    async fn a_partial_credit_leaves_the_order_invoiced() {
        use crate::local_server::routes::sales::{credit_note, CreditNoteBody};
        let state = shop().await;
        let two = json!({ "label": "Plaquettes", "quantity_milli": 2_000, "unit_price_ht_cents": 4_000, "vat_rate_bp": 2_000 });
        let id = new_order(&state, json!({ "status": "ready", "lines": [two.clone()] })).await;
        let (_, Json(sale)) = invoice(State(state.clone()), Path(id)).await.unwrap();
        let sale_id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();
        sales_tests::seal(&state, sale_id).await;

        let one: crate::local_server::routes::sales::LineInputBody = serde_json::from_value(json!({
            "label": "Plaquettes", "quantity_milli": 1_000, "unit_price_ht_cents": 4_000, "vat_rate_bp": 2_000
        }))
        .unwrap();
        let _ = credit_note(
            State(state.clone()),
            Path(sale_id),
            Json(CreditNoteBody { lines: vec![one], reason: None }),
        )
        .await
        .unwrap();
        let Json(after) = show(State(state), Path(id)).await.unwrap();
        assert_eq!(after["status"], "invoiced");
        assert_eq!(after["sale_id"], sale["id"]);
    }

    async fn quote_with_lines(state: &SharedState) -> Uuid {
        use crate::local_server::routes::sales as s;
        let (_, Json(quote)) = s::create(
            State(state.clone()),
            Json(serde_json::from_value(json!({
                "status": "quote",
                "notes": "Valable 30 jours",
                "lines": [{
                    "label": "Filtre à huile", "quantity_milli": 2_000,
                    "unit_price_ttc_cents": 999, "unit_price_ht_cents": 833, "vat_rate_bp": 2_000
                }, {
                    "label": "Main d'oeuvre", "quantity_milli": 1_500,
                    "unit_price_ht_cents": 5_500, "vat_rate_bp": 2_000
                }]
            }))
            .unwrap()),
        )
        .await
        .unwrap();
        Uuid::parse_str(quote["id"].as_str().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn a_quote_is_numbered_once_and_kept_when_invoiced() {
        use crate::local_server::routes::sales as s;
        let state = shop().await;
        let quote_id = quote_with_lines(&state).await;

        let Json(shown) = s::show(State(state.clone()), Path(quote_id)).await.unwrap();
        assert!(shown["reference"].is_null(), "a draft quote has no number yet");

        // It cannot be sealed as it is.
        let refused = s::validate(State(state.clone()), Path(quote_id)).await.unwrap_err();
        assert!(matches!(&refused, LocalError::BadRequest(m) if m.contains("devis")), "{refused:?}");

        let Json(issued) = s::issue_quote(State(state.clone()), Path(quote_id)).await.unwrap();
        let number = issued["reference"].as_str().unwrap().to_string();
        assert!(number.starts_with("DE-"), "{number}");
        let Json(again) = s::issue_quote(State(state.clone()), Path(quote_id)).await.unwrap();
        assert_eq!(again["reference"], number.as_str(), "issuing twice keeps the number");

        let (status, Json(draft)) = s::convert_quote(State(state.clone()), Path(quote_id)).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(draft["status"], "draft");
        assert_eq!(draft["source_quote_id"], quote_id.to_string());
        let announced: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM change_log WHERE entity_type = 'sale' AND entity_id = $1
               AND op = 'update' AND payload LIKE '%' || $2 || '%'",
        )
        .bind(draft["id"].as_str().unwrap())
        .bind(quote_id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(announced, 1, "the other devices are told which quote this invoice came from");
        assert_eq!(draft["lines"].as_array().unwrap().len(), 2);
        assert!(draft["notes"].as_str().unwrap().contains(&number));
        assert_eq!(draft["total_ttc_cents"], shown["total_ttc_cents"], "same money as the quote");

        // The quote is still there, still a quote, now closed to edits.
        let Json(quote) = s::show(State(state.clone()), Path(quote_id)).await.unwrap();
        assert_eq!(quote["status"], "quote");
        assert_eq!(quote["converted_to_sale_id"], draft["id"]);
        let edit = s::update_draft(
            &state,
            quote_id,
            serde_json::from_value(json!({ "status": "quote", "lines": [] })).unwrap(),
        )
        .await
        .unwrap_err();
        assert!(matches!(&edit, LocalError::BadRequest(m) if m.contains("transformé")), "{edit:?}");

        // Converting again hands back the same open draft rather than a second one.
        let (status, Json(second)) = s::convert_quote(State(state.clone()), Path(quote_id)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(second["id"], draft["id"]);

        // Once that invoice is validated the quote is done.
        sales_tests::seal(&state, Uuid::parse_str(draft["id"].as_str().unwrap()).unwrap()).await;
        let done = s::convert_quote(State(state), Path(quote_id)).await.unwrap_err();
        assert!(matches!(&done, LocalError::BadRequest(m) if m.contains("déjà été facturé")), "{done:?}");
    }

    #[tokio::test]
    async fn a_quote_opens_a_repair_order_with_its_lines() {
        let state = shop().await;
        let quote_id = quote_with_lines(&state).await;

        let (status, Json(order)) = from_quote(State(state.clone()), Path(quote_id)).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(order["status"], "pending");
        assert!(order["label"].as_str().unwrap().starts_with("Devis DE-"));
        let lines = order["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["unit_price_ttc_cents"], 999, "a shelf price stays a shelf price");
        assert_eq!(order["source_quote_id"], quote_id.to_string());
        // The other devices are told which quote the order came from.
        let announced: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM change_log WHERE entity_type = 'workshop_order' AND entity_id = $1
               AND op = 'update' AND payload LIKE '%' || $2 || '%'",
        )
        .bind(order["id"].as_str().unwrap())
        .bind(quote_id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(announced, 1);
        assert!(lines[1]["unit_price_ttc_cents"].is_null(), "labour stays priced HT");

        let (status, Json(again)) = from_quote(State(state), Path(quote_id)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(again["id"], order["id"], "asking twice does not open a second order");
    }
}

