//! Local mirrors of `/api/categories`, `/api/products`, `/api/settings`, and
//! `/api/templates`. Each endpoint mirrors the cloud's wire format
//! (the cloud's routes) so the frontend can talk to either transparently.
//!
//! Writes are persisted to SQLite and appended to the `change_log` so the
//! sync engine fans them out to LAN peers and the cloud.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use chrono::{Datelike, Utc};
use motodesk_core::entity::EntityType;
use motodesk_core::sync::OpKind;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

pub fn router() -> Router<SharedState> {
    Router::new()
        .nest("/api/categories", categories_router())
        .nest("/api/products", products_router())
}

pub fn settings_router() -> Router<SharedState> {
    Router::new()
        .route("/", get(settings_list).put(settings_put))
        .route(
            "/:key",
            get(settings_get)
                .put(settings_put_key)
                .delete(settings_delete),
        )
}

fn categories_router() -> Router<SharedState> {
    Router::new()
        .route("/", get(categories_list).post(categories_create))
        .route(
            "/:id",
            get(categories_show)
                .patch(categories_update)
                .delete(categories_delete),
        )
}

fn products_router() -> Router<SharedState> {
    Router::new()
        .route("/", get(products_list).post(products_create))
        .route("/by-sku/:sku", get(products_by_sku))
        .route("/:id/variants", get(products_variants_list).post(products_create_variant))
        .route(
            "/:id",
            get(products_show)
                .patch(products_update)
                .delete(products_delete),
        )
}

// ===========================================================================
// Categories
// ===========================================================================

#[derive(Debug, FromRow)]
struct CategoryRow {
    id: String,
    name: String,
    slug: String,
    position: i64,
    created_at: String,
    updated_at: String,
}

fn category_to_json(r: &CategoryRow) -> Value {
    json!({
        "id": r.id,
        "name": r.name,
        "slug": r.slug,
        "position": r.position,
        "created_at": r.created_at,
        "updated_at": r.updated_at,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct CategoryInput {
    pub name: String,
    pub position: Option<i64>,
}

async fn categories_list(State(state): State<SharedState>) -> LocalResult<Json<Vec<Value>>> {
    let rows: Vec<(String, String, String, i64, Option<i64>, String, String)> = sqlx::query_as(
        r#"
        SELECT c.id, c.name, c.slug, c.position,
               (SELECT COUNT(*) FROM products p
                WHERE p.category_id = c.id AND p.deleted_at IS NULL) AS product_count,
               c.created_at, c.updated_at
        FROM categories c
        WHERE c.deleted_at IS NULL
        ORDER BY c.position ASC, c.name ASC
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let out = rows
        .into_iter()
        .map(
            |(id, name, slug, position, product_count, created_at, updated_at)| {
                json!({
                    "id": id,
                    "name": name,
                    "slug": slug,
                    "position": position,
                    "product_count": product_count.unwrap_or(0),
                    "created_at": created_at,
                    "updated_at": updated_at,
                })
            },
        )
        .collect();
    Ok(Json(out))
}

async fn categories_show(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    let row = find_category(&state, &id.to_string()).await?;
    Ok(Json(category_to_json(&row)))
}

async fn find_category(state: &SharedState, id: &str) -> LocalResult<CategoryRow> {
    let row: CategoryRow = sqlx::query_as(
        r#"SELECT id, name, slug, position, created_at, updated_at
           FROM categories WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(LocalError::NotFound)?;
    Ok(row)
}

async fn categories_create(
    State(state): State<SharedState>,
    Json(body): Json<CategoryInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let name = body.name.trim().to_string();
    let slug = slugify(&name);
    let position = body.position.unwrap_or(100);

    sqlx::query(
        r#"INSERT INTO categories (id, name, slug, position, created_at, updated_at, row_version, field_meta)
           VALUES ($1, $2, $3, $4, $5, $6, 1, '{}')"#,
    )
    .bind(id.to_string())
    .bind(&name)
    .bind(&slug)
    .bind(position)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    let row = find_category(&state, &id.to_string()).await?;
    let payload = category_to_json(&row);

    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Category,
        id,
        OpKind::Insert,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();

    Ok((StatusCode::CREATED, Json(payload)))
}

async fn categories_update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CategoryInput>,
) -> LocalResult<Json<Value>> {
    let existing = find_category(&state, &id.to_string()).await?;
    let name = body.name.trim().to_string();
    let slug = slugify(&name);
    let position = body.position.unwrap_or(existing.position);
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"UPDATE categories
           SET name = $2, slug = $3, position = $4, updated_at = $5,
               row_version = row_version + 1
           WHERE id = $1"#,
    )
    .bind(id.to_string())
    .bind(&name)
    .bind(&slug)
    .bind(position)
    .bind(&now)
    .execute(&state.db)
    .await?;

    let row = find_category(&state, &id.to_string()).await?;
    let payload = category_to_json(&row);

    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Category,
        id,
        OpKind::Update,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(Json(payload))
}

async fn categories_delete(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<StatusCode> {
    let now = Utc::now().to_rfc3339();
    // The name and slug are UNIQUE across deleted rows too: free them so the
    // category can be created again (the suffix keeps the old row identifiable).
    let r = sqlx::query(
        "UPDATE categories SET deleted_at = $2,
                name = name || ' ~' || substr(id, 1, 8),
                slug = slug || '~' || substr(id, 1, 8)
          WHERE id = $1 AND deleted_at IS NULL",
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
        EntityType::Category,
        id,
        OpKind::Delete,
        json!({ "id": id }),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.chars() {
        let lower = ch.to_ascii_lowercase();
        let mapped = match lower {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ÿ' | 'ý' => 'y',
            'ñ' => 'n',
            _ => lower,
        };
        if mapped.is_ascii_alphanumeric() {
            out.push(mapped);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("categorie");
    }
    out
}

// ===========================================================================
// Products
// ===========================================================================

#[derive(Debug, Clone, FromRow)]
struct ProductRow {
    id: String,
    category_id: Option<String>,
    name: String,
    sku: Option<String>,
    brand: Option<String>,
    description: Option<String>,
    unit: String,
    is_service: i64,
    purchase_price_ht_cents: Option<i64>,
    selling_price_ttc_cents: Option<i64>,
    vat_rate_bp: i64,
    stock_milli: i64,
    low_stock_threshold: Option<i64>,
    is_active: i64,
    vehicle_type: Option<String>,
    year: Option<i64>,
    cylinder_capacity: Option<i64>,
    vin_or_serial: Option<String>,
    plate_number: Option<String>,
    odometer_value: Option<i64>,
    odometer_unit: Option<String>,
    notes: Option<String>,
    parent_id: Option<String>,
    variant_label: Option<String>,
    variant_attrs: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, FromRow)]
struct ProductListRow {
    id: String,
    name: String,
    sku: Option<String>,
    brand: Option<String>,
    category_id: Option<String>,
    category_name: Option<String>,
    unit: String,
    is_service: i64,
    stock_milli: i64,
    low_stock_threshold: Option<i64>,
    selling_price_ttc_cents: Option<i64>,
    vat_rate_bp: i64,
    is_active: i64,
    updated_at: String,
    parent_id: Option<String>,
    variant_label: Option<String>,
    variant_count: i64,
}

const PRODUCT_COLS: &str = "id, category_id, name, sku, brand, description, unit, is_service, \
    purchase_price_ht_cents, selling_price_ttc_cents, vat_rate_bp, stock_milli, \
    low_stock_threshold, is_active, vehicle_type, year, cylinder_capacity, vin_or_serial, \
    plate_number, odometer_value, odometer_unit, notes, parent_id, variant_label, variant_attrs, \
    created_at, updated_at";

fn parse_variant_attrs(raw: &Option<String>) -> Value {
    raw.as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null)
}

fn encode_variant_attrs(v: &Option<Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(val) => Some(val.to_string()),
    }
}

fn variant_label_from(attrs: &Value) -> Option<String> {
    let arr = attrs.as_array()?;
    let parts: Vec<String> = arr
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.trim();
            let value = item.get("value")?.as_str()?.trim();
            if name.is_empty() || value.is_empty() {
                None
            } else {
                Some(format!("{name} {value}"))
            }
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn product_to_json(r: &ProductRow) -> Value {
    json!({
        "id": r.id,
        "category_id": r.category_id,
        "name": r.name,
        "sku": r.sku,
        "brand": r.brand,
        "description": r.description,
        "unit": r.unit,
        "is_service": r.is_service != 0,
        "purchase_price_ht_cents": r.purchase_price_ht_cents,
        "selling_price_ttc_cents": r.selling_price_ttc_cents,
        "vat_rate_bp": r.vat_rate_bp,
        "stock_quantity": r.stock_milli as f64 / 1000.0,
        "stock_milli": r.stock_milli,
        "low_stock_threshold": r.low_stock_threshold,
        "is_active": r.is_active != 0,
        "vehicle_type": r.vehicle_type,
        "year": r.year,
        "cylinder_capacity": r.cylinder_capacity,
        "vin_or_serial": r.vin_or_serial,
        "plate_number": r.plate_number,
        "odometer_value": r.odometer_value,
        "odometer_unit": r.odometer_unit,
        "notes": r.notes,
        "parent_id": r.parent_id,
        "variant_label": r.variant_label,
        "variant_attrs": parse_variant_attrs(&r.variant_attrs),
        "created_at": r.created_at,
        "updated_at": r.updated_at,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct ProductListQuery {
    pub q: Option<String>,
    pub category_id: Option<String>,
    pub low_stock: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ProductInput {
    pub name: String,
    pub category_id: Option<String>,
    pub sku: Option<String>,
    pub brand: Option<String>,
    pub description: Option<String>,
    pub unit: String,
    pub is_service: Option<bool>,
    pub purchase_price_ht_cents: Option<i64>,
    pub selling_price_ttc_cents: Option<i64>,
    pub vat_rate_bp: Option<i64>,
    /// In units, decimals allowed (2,5 L of oil). Stored in thousandths.
    pub stock_quantity: Option<f64>,
    pub low_stock_threshold: Option<i64>,
    pub is_active: Option<bool>,
    pub vehicle_type: Option<String>,
    pub year: Option<i64>,
    pub cylinder_capacity: Option<i64>,
    pub vin_or_serial: Option<String>,
    pub plate_number: Option<String>,
    pub odometer_value: Option<i64>,
    pub odometer_unit: Option<String>,
    pub notes: Option<String>,
    pub parent_id: Option<String>,
    pub variant_label: Option<String>,
    pub variant_attrs: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub struct VariantInput {
    pub attrs: Vec<VariantAttr>,
    pub name: Option<String>,
    pub sku: Option<String>,
    pub stock_quantity: Option<f64>,
    pub selling_price_ttc_cents: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct VariantAttr {
    pub name: String,
    pub value: String,
}

async fn products_list(
    State(state): State<SharedState>,
    Query(q): Query<ProductListQuery>,
) -> LocalResult<Json<Vec<Value>>> {
    let limit = q.limit.unwrap_or(500).clamp(1, 2000);
    let offset = q.offset.unwrap_or(0).max(0);

    let q_like =
        q.q.as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"));

    let mut sql = String::from(
        r#"
        SELECT p.id, p.name, p.sku, p.brand, p.category_id, c.name AS category_name,
               p.unit, p.is_service, p.stock_milli, p.low_stock_threshold,
               p.selling_price_ttc_cents, p.vat_rate_bp, p.is_active, p.updated_at,
               p.parent_id, p.variant_label,
               (SELECT COUNT(*) FROM products v
                WHERE v.parent_id = p.id AND v.deleted_at IS NULL) AS variant_count
        FROM products p
        LEFT JOIN categories c ON c.id = p.category_id AND c.deleted_at IS NULL
        WHERE p.deleted_at IS NULL
        "#,
    );

    let mut idx: u32 = 0;
    let mut want_q = false;
    let mut want_cat = false;
    let mut want_parent = false;

    if q_like.is_some() {
        idx += 1;
        sql.push_str(&format!(
            " AND (p.name LIKE ${idx} OR p.sku LIKE ${idx} OR p.brand LIKE ${idx} OR p.variant_label LIKE ${idx})"
        ));
        want_q = true;
    }
    if q.category_id.is_some() {
        idx += 1;
        sql.push_str(&format!(" AND p.category_id = ${idx}"));
        want_cat = true;
    }
    if q.parent_id.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
        idx += 1;
        sql.push_str(&format!(" AND p.parent_id = ${idx}"));
        want_parent = true;
    }
    if q.low_stock == Some(true) {
        sql.push_str(
            " AND p.low_stock_threshold IS NOT NULL AND p.stock_milli <= p.low_stock_threshold * 1000",
        );
    }
    sql.push_str(" ORDER BY COALESCE(c.position, 99999) ASC, p.name ASC");

    idx += 1;
    sql.push_str(&format!(" LIMIT ${idx}"));
    idx += 1;
    sql.push_str(&format!(" OFFSET ${idx}"));

    let mut query = sqlx::query_as::<_, ProductListRow>(&sql);
    if want_q {
        query = query.bind(q_like.unwrap());
    }
    if want_cat {
        query = query.bind(q.category_id.unwrap());
    }
    if want_parent {
        query = query.bind(q.parent_id.unwrap());
    }
    query = query.bind(limit).bind(offset);

    let rows: Vec<ProductListRow> = query.fetch_all(&state.db).await?;
    let out = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "name": r.name,
                "sku": r.sku,
                "brand": r.brand,
                "category_id": r.category_id,
                "category_name": r.category_name,
                "unit": r.unit,
                "is_service": r.is_service != 0,
                "stock_quantity": r.stock_milli as f64 / 1000.0,
                "stock_milli": r.stock_milli,
                "low_stock_threshold": r.low_stock_threshold,
                "selling_price_ttc_cents": r.selling_price_ttc_cents,
                "vat_rate_bp": r.vat_rate_bp,
                "is_active": r.is_active != 0,
                "updated_at": r.updated_at,
                "parent_id": r.parent_id,
                "variant_label": r.variant_label,
                "variant_count": r.variant_count,
            })
        })
        .collect();
    Ok(Json(out))
}

async fn products_show(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    let row = find_product(&state, &id.to_string()).await?;
    Ok(Json(product_to_json(&row)))
}

pub(crate) async fn products_by_sku(
    State(state): State<SharedState>,
    Path(sku): Path<String>,
) -> LocalResult<Json<Value>> {
    let needle = sku.trim();
    if needle.is_empty() {
        return Err(LocalError::BadRequest("code article vide".into()));
    }
    let sql = format!("SELECT {PRODUCT_COLS} FROM products WHERE sku = $1 AND deleted_at IS NULL");
    let row: ProductRow = sqlx::query_as(&sql)
        .bind(needle)
        .fetch_optional(&state.db)
        .await?
        .ok_or(LocalError::NotFound)?;
    Ok(Json(product_to_json(&row)))
}

async fn find_product(state: &SharedState, id: &str) -> LocalResult<ProductRow> {
    let sql = format!("SELECT {PRODUCT_COLS} FROM products WHERE id = $1 AND deleted_at IS NULL");
    let row: ProductRow = sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(LocalError::NotFound)?;
    Ok(row)
}

async fn products_variants_list(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Vec<Value>>> {
    let source = find_product(&state, &id.to_string()).await?;
    let parent_id = source.parent_id.clone().unwrap_or_else(|| source.id.clone());
    let q = ProductListQuery {
        parent_id: Some(parent_id),
        limit: Some(500),
        ..Default::default()
    };
    products_list(State(state), Query(q)).await
}

async fn products_create_variant(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<VariantInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let source = find_product(&state, &id.to_string()).await?;
    let parent_id = source.parent_id.clone().unwrap_or_else(|| source.id.clone());
    let parent = if parent_id == source.id {
        source.clone()
    } else {
        find_product(&state, &parent_id).await?
    };

    let attrs_json = json!(body
        .attrs
        .iter()
        .map(|a| json!({ "name": a.name.trim(), "value": a.value.trim() }))
        .filter(|v| {
            v.get("name")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
                && v.get("value")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
        })
        .collect::<Vec<_>>());

    let label = variant_label_from(&attrs_json)
        .ok_or_else(|| LocalError::BadRequest("indiquez au moins une caractéristique".into()))?;

    let name = normalise(body.name.clone()).unwrap_or_else(|| format!("{} — {label}", parent.name));
    let sku = normalise(body.sku.clone()).or_else(|| {
        parent.sku.as_ref().map(|s| {
            let slug: String = label
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_uppercase())
                .take(12)
                .collect();
            format!("{s}-{slug}")
        })
    });

    let input = ProductInput {
        name,
        category_id: parent.category_id.clone(),
        sku,
        brand: parent.brand.clone(),
        description: parent.description.clone(),
        unit: parent.unit.clone(),
        is_service: Some(parent.is_service != 0),
        purchase_price_ht_cents: parent.purchase_price_ht_cents,
        selling_price_ttc_cents: body
            .selling_price_ttc_cents
            .or(parent.selling_price_ttc_cents),
        vat_rate_bp: Some(parent.vat_rate_bp),
        stock_quantity: Some(body.stock_quantity.unwrap_or(0.0)),
        low_stock_threshold: parent.low_stock_threshold,
        is_active: Some(parent.is_active != 0),
        vehicle_type: parent.vehicle_type.clone(),
        year: parent.year,
        cylinder_capacity: parent.cylinder_capacity,
        vin_or_serial: parent.vin_or_serial.clone(),
        plate_number: parent.plate_number.clone(),
        odometer_value: parent.odometer_value,
        odometer_unit: parent.odometer_unit.clone(),
        notes: parent.notes.clone(),
        parent_id: Some(parent_id),
        variant_label: Some(label),
        variant_attrs: Some(attrs_json),
    };

    products_create(State(state), Json(input)).await
}

/// The rules a product record must meet.
/// Units (decimals allowed) to the thousandths the database keeps.
pub(crate) fn to_milli(units: f64) -> i64 {
    (units * 1000.0).round() as i64
}

fn check_stock(units: Option<f64>) -> LocalResult<()> {
    match units {
        Some(q) if !q.is_finite() => Err(LocalError::BadRequest("Stock : nombre invalide".into())),
        Some(q) if q < 0.0 => Err(LocalError::BadRequest("Stock : ne peut pas être négatif".into())),
        Some(q) if q > 1_000_000.0 => Err(LocalError::BadRequest("Stock : valeur trop grande".into())),
        _ => Ok(()),
    }
}

fn check_product(b: &ProductInput) -> LocalResult<()> {
    use crate::local_server::validate as v;
    v::required("Nom", &b.name, 200)?;
    v::required("Unité", &b.unit, 30)?;
    v::optional("Marque", &b.brand, 100)?;
    v::optional("Référence", &b.sku, 64)?;
    v::optional("Description", &b.description, 2000)?;
    v::optional("Notes", &b.notes, 2000)?;
    v::non_negative("Prix d'achat HT", b.purchase_price_ht_cents, v::MAX_CENTS)?;
    v::non_negative("Prix de vente TTC", b.selling_price_ttc_cents, v::MAX_CENTS)?;
    v::rate_bp("TVA", b.vat_rate_bp)?;
    check_stock(b.stock_quantity)?;
    v::non_negative("Seuil d'alerte", b.low_stock_threshold, 1_000_000_000)?;
    v::non_negative("Kilométrage", b.odometer_value, 9_999_999)?;
    v::non_negative("Cylindrée", b.cylinder_capacity, 10_000)?;
    v::year(b.year, i64::from(Utc::now().year()))?;
    v::one_of("Type de véhicule", &b.vehicle_type, v::VEHICLE_TYPES)?;
    v::one_of("Unité de kilométrage", &b.odometer_unit, v::ODOMETER_UNITS)?;
    v::vin(&b.vin_or_serial)?;
    v::plate(&b.plate_number)?;
    Ok(())
}

async fn products_create(
    State(state): State<SharedState>,
    Json(body): Json<ProductInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    check_product(&body)?;
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"INSERT INTO products (
            id, category_id, name, sku, brand, description, unit, is_service,
            purchase_price_ht_cents, selling_price_ttc_cents, vat_rate_bp,
            stock_milli, low_stock_threshold, is_active,
            vehicle_type, year, cylinder_capacity, vin_or_serial, plate_number,
            odometer_value, odometer_unit, notes, parent_id, variant_label, variant_attrs,
            created_at, updated_at, row_version, field_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27, 1, '{}')"#,
    )
    .bind(id.to_string())
    .bind(body.category_id.as_deref())
    .bind(body.name.trim())
    .bind(normalise(body.sku.clone()))
    .bind(normalise(body.brand.clone()))
    .bind(normalise(body.description.clone()))
    .bind(&body.unit)
    .bind(bool_to_int(body.is_service, false))
    .bind(body.purchase_price_ht_cents)
    .bind(body.selling_price_ttc_cents)
    .bind(body.vat_rate_bp.unwrap_or(2000))
    .bind(to_milli(body.stock_quantity.unwrap_or(0.0)))
    .bind(body.low_stock_threshold)
    .bind(bool_to_int(body.is_active, true))
    .bind(body.vehicle_type.as_deref())
    .bind(body.year)
    .bind(body.cylinder_capacity)
    .bind(normalise(body.vin_or_serial.clone()))
    .bind(normalise(body.plate_number.clone()))
    .bind(body.odometer_value)
    .bind(body.odometer_unit.as_deref())
    .bind(normalise(body.notes.clone()))
    .bind(body.parent_id.as_deref())
    .bind(normalise(body.variant_label.clone()))
    .bind(encode_variant_attrs(&body.variant_attrs))
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    let row = find_product(&state, &id.to_string()).await?;
    let payload = product_to_json(&row);

    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Product,
        id,
        OpKind::Insert,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok((StatusCode::CREATED, Json(payload)))
}

async fn products_update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ProductInput>,
) -> LocalResult<Json<Value>> {
    check_product(&body)?;
    let existing = find_product(&state, &id.to_string()).await?;
    let now = Utc::now().to_rfc3339();

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    // The stock is whatever the shelf holds *now*. A form opened earlier carries the
    // number it saw then, and writing that back silently erased every sale and
    // reception made since. A stock figure is only a change when it was sent, and
    // then it is journalled as a count correction against the current stock.
    let (current_stock,): (i64,) = sqlx::query_as("SELECT stock_milli FROM products WHERE id = $1")
        .bind(id.to_string())
        .fetch_one(&mut *tx)
        .await?;
    let new_stock = body.stock_quantity.map(to_milli).unwrap_or(current_stock);
    let mut correction: Option<Value> = None;
    if new_stock != current_stock && existing.is_service == 0 {
        let movement_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO stock_movements (
                id, product_id, kind, quantity_milli, reason, created_at, updated_at
            ) VALUES ($1,$2,'inventory',$3,$4,$5,$5)"#,
        )
        .bind(movement_id.to_string())
        .bind(id.to_string())
        .bind(new_stock - current_stock)
        .bind("Correction du stock depuis la fiche produit")
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        correction = Some(json!({
            "id": movement_id, "product_id": id, "kind": "inventory",
            "quantity_milli": new_stock - current_stock,
            "reason": "Correction du stock depuis la fiche produit",
            "created_at": now, "updated_at": now
        }));
    }

    sqlx::query(
        r#"UPDATE products SET
            category_id             = $2,
            name                    = $3,
            sku                     = $4,
            brand                   = $5,
            description             = $6,
            unit                    = $7,
            is_service              = $8,
            purchase_price_ht_cents = $9,
            selling_price_ttc_cents = $10,
            vat_rate_bp             = $11,
            stock_milli             = $12,
            low_stock_threshold     = $13,
            is_active               = $14,
            vehicle_type            = $15,
            year                    = $16,
            cylinder_capacity       = $17,
            vin_or_serial           = $18,
            plate_number            = $19,
            odometer_value          = $20,
            odometer_unit           = $21,
            notes                   = $22,
            parent_id               = COALESCE($24, parent_id),
            variant_label           = COALESCE($25, variant_label),
            variant_attrs           = COALESCE($26, variant_attrs),
            updated_at              = $23,
            row_version             = row_version + 1
        WHERE id = $1"#,
    )
    .bind(id.to_string())
    .bind(body.category_id.as_deref())
    .bind(body.name.trim())
    .bind(normalise(body.sku.clone()))
    .bind(normalise(body.brand.clone()))
    .bind(normalise(body.description.clone()))
    .bind(&body.unit)
    .bind(bool_to_int(body.is_service, existing.is_service != 0))
    .bind(body.purchase_price_ht_cents)
    .bind(body.selling_price_ttc_cents)
    .bind(body.vat_rate_bp.unwrap_or(existing.vat_rate_bp))
    .bind(if existing.is_service == 0 { new_stock } else { current_stock })
    .bind(body.low_stock_threshold)
    .bind(bool_to_int(body.is_active, existing.is_active != 0))
    .bind(body.vehicle_type.as_deref())
    .bind(body.year)
    .bind(body.cylinder_capacity)
    .bind(normalise(body.vin_or_serial.clone()))
    .bind(normalise(body.plate_number.clone()))
    .bind(body.odometer_value)
    .bind(body.odometer_unit.as_deref())
    .bind(normalise(body.notes.clone()))
    .bind(&now)
    .bind(body.parent_id.as_deref())
    .bind(normalise(body.variant_label.clone()))
    .bind(encode_variant_attrs(&body.variant_attrs))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let row = find_product(&state, &id.to_string()).await?;
    let payload = product_to_json(&row);

    let device_id = state.device_id().await?;
    if let Some(movement) = correction {
        if let Some(mid) = movement["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()) {
            let _ = sync_log::append(
                &state.db,
                device_id,
                EntityType::StockMovement,
                mid,
                OpKind::Insert,
                movement,
                None,
            )
            .await?;
        }
    }
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Product,
        id,
        OpKind::Update,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(Json(payload))
}

async fn products_delete(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<StatusCode> {
    let now = Utc::now().to_rfc3339();
    // SKU and VIN are UNIQUE across deleted rows too: free them so the same
    // reference can be used again, keeping the old row identifiable.
    let r = sqlx::query(
        "UPDATE products SET deleted_at = $2,
                sku = CASE WHEN sku IS NULL THEN NULL ELSE sku || '~' || substr(id, 1, 8) END,
                vin_or_serial = CASE WHEN vin_or_serial IS NULL THEN NULL ELSE vin_or_serial || '~' || substr(id, 1, 8) END
          WHERE id = $1 AND deleted_at IS NULL",
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
        EntityType::Product,
        id,
        OpKind::Delete,
        json!({ "id": id }),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

fn normalise(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub(crate) fn bool_to_int(b: Option<bool>, default: bool) -> i64 {
    if b.unwrap_or(default) {
        1
    } else {
        0
    }
}

// ===========================================================================
// App settings
// ===========================================================================

async fn settings_list(State(state): State<SharedState>) -> LocalResult<Json<Vec<Value>>> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT key, value, updated_at FROM app_settings WHERE deleted_at IS NULL ORDER BY key ASC",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(setting_row_to_json).collect()))
}

/// Settings an employee (non-admin) is allowed to change. Everything else is
/// shop-wide configuration reserved for administrators. Keep this in sync with
/// the `preferences` category in the front-end settings registry.
/// The current value of each fiscal setting about to be written, to tell a real
/// change from a save that rewrites the same value.
async fn fiscal_settings_before<'a>(
    state: &SharedState,
    keys: impl Iterator<Item = &'a str>,
) -> std::collections::HashMap<String, Value> {
    let mut out = std::collections::HashMap::new();
    for key in keys.filter(|k| crate::local_server::fiscal::is_fiscal_setting(k)) {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM app_settings WHERE key = $1 AND deleted_at IS NULL")
                .bind(key)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten();
        let old = row
            .map(|(raw,)| serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw)))
            .unwrap_or(Value::Null);
        out.insert(key.to_string(), old);
    }
    out
}

/// A change to a setting that affects invoicing or VAT is a fiscal event. Best
/// effort: the setting is saved either way.
async fn journal_fiscal_setting(state: &SharedState, key: &str, before: Option<&Value>, new: &Value) {
    if !crate::local_server::fiscal::is_fiscal_setting(key) {
        return;
    }
    let old = before.cloned().unwrap_or(Value::Null);
    if &old == new {
        return;
    }
    let _ = crate::local_server::fiscal::journal_event(
        state,
        motodesk_fiscal::FiscalEventType::FiscalSettingChanged,
        json!({ "key": key, "old": old, "new": new }),
    )
    .await;
}

const EMPLOYEE_WRITABLE_SETTINGS: &[&str] =
    &["system.theme", "system.compact_mode", "system.accent_color", "system.radius", "system.blur"];

fn employee_can_write(key: &str) -> bool {
    EMPLOYEE_WRITABLE_SETTINGS.contains(&key)
}

/// `PUT /api/settings` — bulk upsert. The body is a flat `{ "key": <json>, ... }`
/// object (same shape as the cloud) and we return the materialised list.
async fn settings_put(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> LocalResult<Json<Vec<Value>>> {
    let now = Utc::now().to_rfc3339();
    let mut out: Vec<Value> = Vec::new();
    if let Some(obj) = body.as_object() {
        // Non-admins may only touch their personal preferences.
        if !crate::local_server::auth_guard::is_admin(&state).await
            && obj.keys().any(|k| !employee_can_write(k))
        {
            return Err(LocalError::Forbidden);
        }
        let before = fiscal_settings_before(&state, obj.keys().map(String::as_str)).await;
        let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
        for (k, v) in obj {
            upsert_setting_tx(&mut tx, k, v, &now).await?;
            out.push(json!({
                "key": k,
                "value": v.clone(),
                "updated_at": now,
            }));
        }
        tx.commit().await?;
        for (k, v) in obj {
            journal_fiscal_setting(&state, k, before.get(k.as_str()), v).await;
        }
        let device_id = state.device_id().await?;
        for item in &out {
            if let Some(key) = item.get("key").and_then(|v| v.as_str()) {
                let eid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, key.as_bytes());
                let _ = crate::local_server::sync_log::append(
                    &state.db,
                    device_id,
                    motodesk_core::entity::EntityType::AppSetting,
                    eid,
                    motodesk_core::sync::OpKind::Update,
                    item.clone(),
                    None,
                )
                .await;
            }
        }
    }
    state.notify_sync();
    Ok(Json(out))
}

async fn settings_get(
    State(state): State<SharedState>,
    Path(key): Path<String>,
) -> LocalResult<Json<Value>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT key, value, updated_at FROM app_settings WHERE key = $1 AND deleted_at IS NULL",
    )
    .bind(&key)
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some(r) => Ok(Json(setting_row_to_json(r))),
        None => Err(LocalError::NotFound),
    }
}

/// `PUT /api/settings/:key` — body is `{ "value": <json> }` to match the cloud.
async fn settings_put_key(
    State(state): State<SharedState>,
    Path(key): Path<String>,
    Json(body): Json<Value>,
) -> LocalResult<Json<Value>> {
    if !employee_can_write(&key) && !crate::local_server::auth_guard::is_admin(&state).await {
        return Err(LocalError::Forbidden);
    }
    let value = body.get("value").cloned().unwrap_or(Value::Null);
    let now = Utc::now().to_rfc3339();
    let before = fiscal_settings_before(&state, std::iter::once(key.as_str())).await;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    upsert_setting_tx(&mut tx, &key, &value, &now).await?;
    tx.commit().await?;
    journal_fiscal_setting(&state, &key, before.get(key.as_str()), &value).await;
    let device_id = state.device_id().await?;
    let eid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, key.as_bytes());
    let _ = crate::local_server::sync_log::append(
        &state.db,
        device_id,
        motodesk_core::entity::EntityType::AppSetting,
        eid,
        motodesk_core::sync::OpKind::Update,
        json!({ "key": key, "value": value }),
        None,
    )
    .await;
    state.notify_sync();
    Ok(Json(json!({
        "key": key,
        "value": value,
        "updated_at": now,
    })))
}

async fn settings_delete(
    State(state): State<SharedState>,
    Path(key): Path<String>,
) -> LocalResult<StatusCode> {
    if !employee_can_write(&key) && !crate::local_server::auth_guard::is_admin(&state).await {
        return Err(LocalError::Forbidden);
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE app_settings SET deleted_at = $2 WHERE key = $1")
        .bind(&key)
        .bind(&now)
        .execute(&state.db)
        .await?;
    if let Ok(device_id) = state.device_id().await {
        let eid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, key.as_bytes());
        let _ = crate::local_server::sync_log::append(
            &state.db,
            device_id,
            motodesk_core::entity::EntityType::AppSetting,
            eid,
            motodesk_core::sync::OpKind::Delete,
            json!({ "key": key }),
            None,
        )
        .await;
    }
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

fn setting_row_to_json((key, value, updated_at): (String, String, String)) -> Value {
    let parsed: Value = serde_json::from_str(&value).unwrap_or(Value::Null);
    json!({
        "key": key,
        "value": parsed,
        "updated_at": updated_at,
    })
}

async fn upsert_setting_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    value: &Value,
    now: &str,
) -> LocalResult<()> {
    sqlx::query(
        r#"
        INSERT INTO app_settings (key, value, updated_at) VALUES ($1, $2, $3)
        ON CONFLICT(key) DO UPDATE SET value = $2, updated_at = $3, deleted_at = NULL
        "#,
    )
    .bind(key)
    .bind(value.to_string())
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// ===========================================================================

mod templates;
pub use templates::templates_router;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::{insert_product, shop};

    fn input(name: &str, sku: Option<&str>, stock: Option<f64>) -> Json<ProductInput> {
        Json(ProductInput {
            name: name.into(),
            sku: sku.map(Into::into),
            unit: "piece".into(),
            stock_quantity: stock,
            ..Default::default()
        })
    }

    async fn stock_of(state: &SharedState, id: Uuid) -> i64 {
        let (n,): (i64,) = sqlx::query_as("SELECT stock_milli FROM products WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        n
    }

    #[tokio::test]
    async fn saving_a_product_does_not_overwrite_the_stock_with_a_stale_figure() {
        let state = shop().await;
        let id = insert_product(&state, "Filtre", Some("F-1"), 10, false, 1_200, 2_000).await;

        // A sale takes 3 while a form opened at 10 is still on screen.
        sqlx::query("UPDATE products SET stock_milli = 7000 WHERE id = $1")
            .bind(id.to_string())
            .execute(&state.db)
            .await
            .unwrap();

        // The form saves a new name and sends no stock (it was not touched).
        let _ = products_update(State(state.clone()), Path(id), input("Filtre à huile", Some("F-1"), None))
            .await
            .unwrap();
        assert_eq!(stock_of(&state, id).await, 7_000, "the sale is still counted");
    }

    #[tokio::test]
    async fn a_stock_correction_from_the_form_is_journalled() {
        let state = shop().await;
        let id = insert_product(&state, "Filtre", Some("F-2"), 10, false, 1_200, 2_000).await;

        let _ = products_update(State(state.clone()), Path(id), input("Filtre", Some("F-2"), Some(4.0)))
            .await
            .unwrap();
        assert_eq!(stock_of(&state, id).await, 4_000);
        let (qty, kind): (i64, String) = sqlx::query_as(
            "SELECT quantity_milli, kind FROM stock_movements WHERE product_id = $1",
        )
        .bind(id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!((qty, kind.as_str()), (-6_000, "inventory"));
    }

    /// Two PCs edit different fields of one product at about the same time. This PC
    /// renames it; another changed the price a moment earlier. Renaming must not claim
    /// the price (the row it sends still carries the old one), and the other PC's
    /// price must still apply here.
    #[tokio::test]
    async fn a_rename_claims_only_the_name_and_leaves_room_for_a_price_changed_elsewhere() {
        use motodesk_core::sync::{ChangeLogEntry, FieldStamp, OpKind as Op};
        let state = shop().await;
        let created = products_create(
            State(state.clone()),
            Json(ProductInput {
                name: "Filtre".into(),
                sku: Some("F-9".into()),
                unit: "piece".into(),
                selling_price_ttc_cents: Some(1_000),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let id = Uuid::parse_str(created.1 .0["id"].as_str().unwrap()).unwrap();
        let other_pc = Uuid::new_v4();
        let before_rename = chrono::Utc::now();

        let _ = products_update(
            State(state.clone()),
            Path(id),
            Json(ProductInput {
                name: "Filtre à huile".into(),
                sku: Some("F-9".into()),
                unit: "piece".into(),
                selling_price_ttc_cents: Some(1_000),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

        // What went out: the whole row, but a claim on the name only.
        let (meta,): (String,) = sqlx::query_as(
            "SELECT field_meta FROM change_log WHERE entity_id = $1 AND op = 'update'",
        )
        .bind(id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        let claimed: serde_json::Value = serde_json::from_str(&meta).unwrap();
        let mut keys: Vec<&String> = claimed.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["name"], "{claimed}");

        // The other PC changed the price just before the rename, without seeing it.
        let mut stamps = std::collections::HashMap::new();
        stamps.insert("selling_price_ttc_cents".to_string(), FieldStamp::new(before_rename, other_pc));
        let op = ChangeLogEntry {
            id: Uuid::new_v4(),
            device_id: other_pc,
            seq: 1,
            entity_type: EntityType::Product,
            entity_id: id,
            op: Op::Update,
            payload: json!({ "id": id.to_string(), "name": "Filtre", "unit": "piece", "selling_price_ttc_cents": 1_250 }),
            field_meta: stamps,
            created_at: before_rename,
            synced_to_cloud: false,
            relayed: false,
        };
        crate::local_server::mesh::apply_batch(&state, vec![op]).await.unwrap();

        let (name, price): (String, i64) =
            sqlx::query_as("SELECT name, selling_price_ttc_cents FROM products WHERE id = $1")
                .bind(id.to_string())
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!((name.as_str(), price), ("Filtre à huile", 1_250), "both edits survive");
    }

    /// Stock can be a fraction: 2,5 m of cable, 0,75 L of oil. It is kept exactly, a
    /// correction from the form is journalled for exactly the difference, and the
    /// low-stock alert compares the real figure.
    #[tokio::test]
    async fn a_product_can_hold_a_fraction_of_a_unit() {
        let state = shop().await;
        let created = products_create(State(state.clone()), input("Câble", Some("C-1"), Some(2.5)))
            .await
            .unwrap();
        let id = Uuid::parse_str(created.1 .0["id"].as_str().unwrap()).unwrap();
        assert_eq!(stock_of(&state, id).await, 2_500);
        assert_eq!(created.1 .0["stock_milli"], 2_500);
        assert_eq!(created.1 .0["stock_quantity"], 2.5);

        let _ = products_update(State(state.clone()), Path(id), input("Câble", Some("C-1"), Some(0.75)))
            .await
            .unwrap();
        assert_eq!(stock_of(&state, id).await, 750);
        let (qty,): (i64,) = sqlx::query_as("SELECT quantity_milli FROM stock_movements WHERE product_id = $1")
            .bind(id.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(qty, -1_750, "the correction is the exact difference");

        // 0,75 left against an alert threshold of 1: it is low, though it rounds to 0.
        sqlx::query("UPDATE products SET low_stock_threshold = 1 WHERE id = $1")
            .bind(id.to_string())
            .execute(&state.db)
            .await
            .unwrap();
        let low: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM products WHERE id = $1 AND low_stock_threshold IS NOT NULL AND stock_milli <= low_stock_threshold * 1000",
        )
        .bind(id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(low, 1);

        // Not a number a shelf can hold.
        let refused = products_update(State(state.clone()), Path(id), input("Câble", Some("C-1"), Some(-1.0)))
            .await
            .unwrap_err();
        assert!(matches!(refused, LocalError::BadRequest(_)));
    }

    #[tokio::test]
    async fn a_deleted_products_reference_can_be_used_again() {
        let state = shop().await;
        let id = insert_product(&state, "Ancien", Some("REF-9"), 0, false, 100, 2_000).await;
        products_delete(State(state.clone()), Path(id)).await.unwrap();

        // Reused: this used to come back as a raw 500.
        let _ = products_create(State(state.clone()), input("Nouveau", Some("REF-9"), Some(0.0)))
            .await
            .unwrap();

        // A live duplicate is still refused, in plain words.
        let again = products_create(State(state), input("Autre", Some("REF-9"), Some(0.0)))
            .await
            .unwrap_err();
        assert!(matches!(&again, LocalError::BadRequest(m) if m.contains("référence")), "{again:?}");
    }
}
