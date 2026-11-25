//! Universal CSV import endpoint (`/api/import/preview` + `/api/import/commit`).
//!
//! The frontend parses + transforms the CSV client-side and posts typed row
//! payloads here. This module is the source of truth: it re-validates required
//! fields, resolves foreign keys (product category by name, vehicle owner by
//! email/phone), classifies each row against the chosen mode + match key, and —
//! on commit — writes to SQLite and appends to the `change_log` so the existing
//! sync engine fans the rows out to LAN peers and the cloud.

use axum::{routing::post, Json, Router};
use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::OpKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log::append;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/preview", post(preview))
        .route("/commit", post(commit))
}

// ===========================================================================
// Wire types
// ===========================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub entity: String,
    pub mode: String,
    pub match_key: String,
    /// When true, always insert without checking the DB for existing rows (blind import).
    #[serde(default)]
    pub ignore_duplicates: bool,
    #[serde(default)]
    pub create_missing_categories: bool,
    /// Per-reference-field lookup column chosen in the wizard, e.g.
    /// `{ "client": "email", "product": "sku", "sale": "reference" }`.
    /// Maps an entity field that points at another table to the column used to
    /// resolve the external row to its id. Absent entries fall back to the
    /// field's default lookup.
    #[serde(default)]
    pub references: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub rows: Vec<Map<String, Value>>,
}

#[derive(Debug, Serialize)]
struct RowOutcome {
    row: usize,
    disposition: &'static str,
    message: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct ImportResult {
    inserted: usize,
    updated: usize,
    skipped: usize,
    errored: usize,
    rows: Vec<RowOutcome>,
}

impl ImportResult {
    fn record(
        &mut self,
        row: usize,
        disposition: &'static str,
        message: Option<String>,
        id: Option<String>,
    ) {
        match disposition {
            "insert" => self.inserted += 1,
            "update" => self.updated += 1,
            "skip" => self.skipped += 1,
            _ => self.errored += 1,
        }
        self.rows.push(RowOutcome {
            row,
            disposition,
            message,
            id,
        });
    }
}

// ===========================================================================
// Handlers
// ===========================================================================

async fn preview(
    axum::extract::State(state): axum::extract::State<SharedState>,
    Json(req): Json<ImportRequest>,
) -> LocalResult<Json<ImportResult>> {
    let res = run(&state, &req, false).await?;
    Ok(Json(res))
}

async fn commit(
    axum::extract::State(state): axum::extract::State<SharedState>,
    Json(req): Json<ImportRequest>,
) -> LocalResult<Json<ImportResult>> {
    let res = run(&state, &req, true).await?;
    if res.inserted + res.updated > 0 {
        state.notify_sync();
    }
    Ok(Json(res))
}

// ===========================================================================
// Dispatch
// ===========================================================================

enum Plan {
    Insert,
    Update(String),
    Skip(&'static str),
}

fn decide(mode: &str, existing: Option<String>) -> Plan {
    match (mode, existing) {
        ("insert", Some(_)) => Plan::Skip("Déjà présent"),
        ("insert", None) => Plan::Insert,
        ("update", Some(id)) => Plan::Update(id),
        ("update", None) => Plan::Skip("Aucune correspondance"),
        ("upsert", Some(id)) => Plan::Update(id),
        ("upsert", None) => Plan::Insert,
        _ => Plan::Skip("Mode inconnu"),
    }
}

/// Match keys we allow per entity (guards the column interpolated into SQL).
fn match_column(entity: &str, key: &str) -> Option<&'static str> {
    match (entity, key) {
        ("clients", "email") => Some("email"),
        ("clients", "phone") => Some("phone"),
        ("clients", "last_name") => Some("last_name"),
        ("categories", "name") => Some("name"),
        ("products", "sku") => Some("sku"),
        ("products", "name") => Some("name"),
        ("vehicles", "vin_or_serial") => Some("vin_or_serial"),
        ("sales", "reference") => Some("reference"),
        _ => None,
    }
}

// ===========================================================================
// Generic relational references
// ===========================================================================

/// Whitelist of (target table, lookup column) pairs the importer may resolve a
/// reference against. Guards the column interpolated into SQL and the special
/// combined `email_or_phone` lookup. `id` resolves directly by primary key.
fn ref_column_allowed(table: &str, column: &str) -> bool {
    matches!(
        (table, column),
        ("clients", "id")
            | ("clients", "email")
            | ("clients", "phone")
            | ("clients", "last_name")
            | ("clients", "email_or_phone")
            | ("products", "id")
            | ("products", "sku")
            | ("products", "name")
            | ("products", "legacy_ref")
            | ("categories", "id")
            | ("categories", "name")
            | ("vehicles", "id")
            | ("vehicles", "vin_or_serial")
            | ("vehicles", "plate_number")
            | ("vehicles", "legacy_ref")
            | ("sales", "id")
            | ("sales", "reference")
            | ("sales", "legacy_ref")
            | ("clients", "legacy_ref")
    )
}

/// Resolve a reference value to the target row's id using the chosen lookup
/// column. Returns `None` when no matching row exists.
async fn resolve_reference(
    state: &SharedState,
    table: &str,
    column: &str,
    value: &str,
) -> LocalResult<Option<String>> {
    let sql = if column == "email_or_phone" {
        format!(
            "SELECT id FROM {table} WHERE (email = $1 OR phone = $1) AND deleted_at IS NULL ORDER BY created_at ASC LIMIT 1"
        )
    } else {
        format!(
            "SELECT id FROM {table} WHERE {column} = $1 AND deleted_at IS NULL ORDER BY created_at ASC LIMIT 1"
        )
    };
    let row: Option<(String,)> = sqlx::query_as(&sql)
        .bind(value)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.map(|r| r.0))
}

/// Sibling CSV columns that carry the same foreign key under another name.
/// Natural keys (`client`, `owner`) are preferred over leftover export UUIDs.
fn field_value_aliases(field: &str) -> &'static [&'static str] {
    match field {
        "client" => &["client_id", "owner"],
        "owner" => &["client", "client_id"],
        "vehicle" => &["vehicle_id"],
        "product" => &["product_id", "sku"],
        "sale" => &["sale_id", "reference"],
        "category" => &["category_id"],
        _ => &[],
    }
}

fn lookup_fallbacks(table: &str) -> &'static [&'static str] {
    match table {
        "clients" => &["id", "email_or_phone", "email", "phone", "last_name", "legacy_ref"],
        "vehicles" => &["id", "vin_or_serial", "plate_number", "legacy_ref"],
        "products" => &["id", "sku", "name", "legacy_ref"],
        "sales" => &["id", "reference", "legacy_ref"],
        "categories" => &["id", "name"],
        _ => &["id"],
    }
}

fn candidate_values(row: &Map<String, Value>, field: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut natural = Vec::new();
    let mut ids = Vec::new();
    let mut push = |v: String| {
        if seen.insert(v.clone()) {
            if Uuid::parse_str(&v).is_ok() {
                ids.push(v);
            } else {
                natural.push(v);
            }
        }
    };
    if let Some(v) = get_str(row, field) {
        push(v);
    }
    for k in field_value_aliases(field) {
        if let Some(v) = get_str(row, k) {
            push(v);
        }
    }
    natural.extend(ids);
    natural
}

async fn resolve_value(
    state: &SharedState,
    table: &str,
    column: &str,
    value: &str,
) -> Result<Option<String>, String> {
    if let Some(id) = resolve_reference(state, table, column, value)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(Some(id));
    }
    for fb in lookup_fallbacks(table) {
        if *fb == column || !ref_column_allowed(table, fb) {
            continue;
        }
        if let Some(id) = resolve_reference(state, table, fb, value)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Resolve one reference field of a row to a foreign-key id.
///
/// - Empty/absent source → `Ok(None)` (caller decides if that's an error).
/// - Value present and resolved → `Ok(Some(id))`.
/// - Value present but unresolved → `Err(msg)` so the row is reported in error,
///   except categories with create-missing enabled (created on commit).
///
/// A wizard import of parents mints new ids, so a child CSV that still carries
/// the export UUID will miss. Natural-key columns (`client`, `owner`, `vehicle`,
/// `product`, `sale`) are tried first; leftover UUID columns are a last resort.
async fn resolve_field_ref(
    state: &SharedState,
    device_id: Uuid,
    req: &ImportRequest,
    write: bool,
    row: &Map<String, Value>,
    field: &str,
    target_table: &str,
    default_lookup: &str,
) -> Result<Option<String>, String> {
    let values = candidate_values(row, field);
    if values.is_empty() {
        return Ok(None);
    }
    let column = req
        .references
        .get(field)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(default_lookup);

    if !ref_column_allowed(target_table, column) {
        return Err(format!(
            "Clé de correspondance « {column} » invalide pour {target_table}"
        ));
    }

    for value in &values {
        if let Some(id) = resolve_value(state, target_table, column, value).await? {
            return Ok(Some(id));
        }
    }

    // Categories may be created on the fly (legacy behaviour) when matching by name.
    if target_table == "categories" && column == "name" && req.create_missing_categories {
        let create_from = values
            .iter()
            .find(|v| Uuid::parse_str(v).is_err())
            .unwrap_or(&values[0]);
        if write {
            let id = insert_category(state, device_id, create_from, 100)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(Some(id));
        }
        // Preview: it would be created on commit, so don't flag it as an error.
        return Ok(None);
    }

    Err(format!(
        "Référence introuvable : {field} = « {} »",
        values[0]
    ))
}

async fn run(state: &SharedState, req: &ImportRequest, write: bool) -> LocalResult<ImportResult> {
    let device_id = state.device_id().await?;
    let mut out = ImportResult::default();

    for (idx, row) in req.rows.iter().enumerate() {
        let res = match req.entity.as_str() {
            "clients" => import_client(state, device_id, req, write, row).await,
            "categories" => import_category(state, device_id, req, write, row).await,
            "products" => import_product(state, device_id, req, write, row).await,
            "vehicles" => import_vehicle(state, device_id, req, write, row).await,
            "sales" => import_sale(state, device_id, req, write, row).await,
            "sale_lines" => import_sale_line(state, device_id, req, write, row).await,
            other => {
                return Err(LocalError::Internal(anyhow::anyhow!(
                    "Type d'import inconnu : {other}"
                )))
            }
        };
        match res {
            Ok((disp, id)) => out.record(idx, disp, None, id),
            Err(msg) => out.record(idx, "error", Some(msg), None),
        }
    }

    Ok(out)
}

type RowResult = Result<(&'static str, Option<String>), String>;

// ===========================================================================
// Field accessors
// ===========================================================================

fn get_str(row: &Map<String, Value>, k: &str) -> Option<String> {
    row.get(k).and_then(|v| match v {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

fn get_i64(row: &Map<String, Value>, k: &str) -> Option<i64> {
    row.get(k).and_then(|v| match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    })
}

fn get_bool(row: &Map<String, Value>, k: &str) -> Option<bool> {
    row.get(k).and_then(|v| match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|x| x != 0),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" | "1" | "oui" | "yes" => Some(true),
            "false" | "0" | "non" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    })
}

fn get_uuid(row: &Map<String, Value>, k: &str) -> Option<String> {
    get_str(row, k).and_then(|s| Uuid::parse_str(&s).ok().map(|u| u.to_string()))
}

async fn find_existing(
    state: &SharedState,
    table: &str,
    column: &str,
    value: &str,
) -> LocalResult<Option<String>> {
    let sql = format!(
        "SELECT id FROM {table} WHERE {column} = $1 AND deleted_at IS NULL ORDER BY created_at ASC LIMIT 1"
    );
    let row: Option<(String,)> = sqlx::query_as(&sql)
        .bind(value)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.map(|r| r.0))
}

/// Resolve the match key value for this row + classify it against the mode.
async fn classify(
    state: &SharedState,
    req: &ImportRequest,
    table: &str,
    row: &Map<String, Value>,
) -> LocalResult<Plan> {
    if req.ignore_duplicates {
        return Ok(Plan::Insert);
    }
    let Some(column) = match_column(&req.entity, &req.match_key) else {
        // No usable match key → behave as insert-only.
        return Ok(if req.mode == "update" {
            Plan::Skip("Clé de correspondance invalide")
        } else {
            Plan::Insert
        });
    };
    let Some(value) = get_str(row, &req.match_key) else {
        return Ok(if req.mode == "update" {
            Plan::Skip("Valeur de correspondance absente")
        } else {
            Plan::Insert
        });
    };
    let existing = find_existing(state, table, column, &value).await?;
    Ok(decide(&req.mode, existing))
}

// ===========================================================================
// Clients
// ===========================================================================

async fn fetch_client_json(state: &SharedState, id: &str) -> LocalResult<Value> {
    let r: (String, Option<String>, String, Option<String>, Option<String>, Option<String>, Option<String>, String, String) =
        sqlx::query_as(
            "SELECT id, first_name, last_name, phone, email, address, notes, created_at, updated_at FROM clients WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(json!({
        "id": r.0, "first_name": r.1, "last_name": r.2, "phone": r.3, "email": r.4,
        "address": r.5, "notes": r.6, "created_at": r.7, "updated_at": r.8,
    }))
}

async fn import_client(
    state: &SharedState,
    device_id: Uuid,
    req: &ImportRequest,
    write: bool,
    row: &Map<String, Value>,
) -> RowResult {
    let last_name = match get_str(row, "last_name") {
        Some(v) => v,
        None => return Err("Nom requis".to_string()),
    };

    let plan = classify(state, req, "clients", row)
        .await
        .map_err(|e| e.to_string())?;
    let (id, op) = match plan {
        Plan::Skip(why) => return Ok(("skip", Some(why.to_string()))),
        Plan::Insert => (
            get_uuid(row, "id").unwrap_or_else(|| Uuid::new_v4().to_string()),
            OpKind::Insert,
        ),
        Plan::Update(id) => (id, OpKind::Update),
    };

    if !write {
        return Ok((
            if matches!(op, OpKind::Insert) {
                "insert"
            } else {
                "update"
            },
            Some(id),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let first_name = get_str(row, "first_name");
    let phone = get_str(row, "phone");
    let email = get_str(row, "email");
    let address = get_str(row, "address");
    let notes = get_str(row, "notes");

    if matches!(op, OpKind::Insert) {
        sqlx::query(
            r#"INSERT INTO clients (id, first_name, last_name, phone, email, address, notes, created_at, updated_at, row_version, field_meta)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,1,'{}')"#,
        )
        .bind(&id)
        .bind(&first_name)
        .bind(&last_name)
        .bind(&phone)
        .bind(&email)
        .bind(&address)
        .bind(&notes)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query(
            r#"UPDATE clients SET
                first_name = COALESCE($2, first_name),
                last_name  = COALESCE($3, last_name),
                phone      = COALESCE($4, phone),
                email      = COALESCE($5, email),
                address    = COALESCE($6, address),
                notes      = COALESCE($7, notes),
                updated_at = $8,
                row_version = row_version + 1
               WHERE id = $1"#,
        )
        .bind(&id)
        .bind(&first_name)
        .bind(Some(&last_name))
        .bind(&phone)
        .bind(&email)
        .bind(&address)
        .bind(&notes)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }

    let payload = fetch_client_json(state, &id)
        .await
        .map_err(|e| e.to_string())?;
    let entity_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    append(
        &state.db,
        device_id,
        EntityType::Client,
        entity_id,
        op,
        payload,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok((
        if matches!(op, OpKind::Insert) {
            "insert"
        } else {
            "update"
        },
        Some(id),
    ))
}

// ===========================================================================
// Categories
// ===========================================================================

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

async fn fetch_category_json(state: &SharedState, id: &str) -> LocalResult<Value> {
    let r: (String, String, String, i64, String, String) = sqlx::query_as(
        "SELECT id, name, slug, position, created_at, updated_at FROM categories WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(json!({
        "id": r.0, "name": r.1, "slug": r.2, "position": r.3,
        "created_at": r.4, "updated_at": r.5,
    }))
}

/// Insert a category by name (used directly + by product FK resolution). Returns its id.
async fn insert_category(
    state: &SharedState,
    device_id: Uuid,
    name: &str,
    position: i64,
) -> LocalResult<String> {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let slug = slugify(name);
    sqlx::query(
        r#"INSERT INTO categories (id, name, slug, position, created_at, updated_at, row_version, field_meta)
           VALUES ($1,$2,$3,$4,$5,$6,1,'{}')"#,
    )
    .bind(id.to_string())
    .bind(name)
    .bind(&slug)
    .bind(position)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;
    let payload = fetch_category_json(state, &id.to_string()).await?;
    append(
        &state.db,
        device_id,
        EntityType::Category,
        id,
        OpKind::Insert,
        payload,
        None,
    )
    .await?;
    Ok(id.to_string())
}

async fn import_category(
    state: &SharedState,
    device_id: Uuid,
    req: &ImportRequest,
    write: bool,
    row: &Map<String, Value>,
) -> RowResult {
    let name = match get_str(row, "name") {
        Some(v) => v,
        None => return Err("Nom requis".to_string()),
    };

    let plan = classify(state, req, "categories", row)
        .await
        .map_err(|e| e.to_string())?;
    let (id, op) = match plan {
        Plan::Skip(why) => return Ok(("skip", Some(why.to_string()))),
        // Honour a supplied UUID so a later product import can join on category_id.
        Plan::Insert => (
            get_uuid(row, "id").unwrap_or_else(|| Uuid::new_v4().to_string()),
            OpKind::Insert,
        ),
        Plan::Update(id) => (id, OpKind::Update),
    };

    if !write {
        return Ok((
            if matches!(op, OpKind::Insert) {
                "insert"
            } else {
                "update"
            },
            Some(id),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let position = get_i64(row, "position");

    if matches!(op, OpKind::Insert) {
        let slug = slugify(&name);
        sqlx::query(
            r#"INSERT INTO categories (id, name, slug, position, created_at, updated_at, row_version, field_meta)
               VALUES ($1,$2,$3,$4,$5,$6,1,'{}')"#,
        )
        .bind(&id)
        .bind(&name)
        .bind(&slug)
        .bind(position.unwrap_or(100))
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query(
            r#"UPDATE categories SET
                name = $2,
                slug = $3,
                position = COALESCE($4, position),
                updated_at = $5,
                row_version = row_version + 1
               WHERE id = $1"#,
        )
        .bind(&id)
        .bind(&name)
        .bind(slugify(&name))
        .bind(position)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }

    let payload = fetch_category_json(state, &id)
        .await
        .map_err(|e| e.to_string())?;
    let entity_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    append(
        &state.db,
        device_id,
        EntityType::Category,
        entity_id,
        op,
        payload,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok((
        if matches!(op, OpKind::Insert) {
            "insert"
        } else {
            "update"
        },
        Some(id),
    ))
}

// ===========================================================================
// Products
// ===========================================================================

async fn fetch_product_json(state: &SharedState, id: &str) -> LocalResult<Value> {
    let r: (
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        i64,
        Option<i64>,
        Option<i64>,
        i64,
        i64,
        Option<i64>,
        i64,
        String,
        String,
    ) = sqlx::query_as(
        r#"SELECT id, category_id, name, sku, brand, description, unit, is_service,
                  purchase_price_ht_cents, selling_price_ttc_cents, vat_rate_bp,
                  stock_milli, low_stock_threshold, is_active, created_at, updated_at
           FROM products WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(json!({
        "id": r.0, "category_id": r.1, "name": r.2, "sku": r.3, "brand": r.4,
        "description": r.5, "unit": r.6, "is_service": r.7 != 0,
        "purchase_price_ht_cents": r.8, "selling_price_ttc_cents": r.9,
        "vat_rate_bp": r.10, "stock_quantity": r.11 as f64 / 1000.0, "stock_milli": r.11,
        "low_stock_threshold": r.12,
        "is_active": r.13 != 0, "created_at": r.14, "updated_at": r.15,
    }))
}

async fn import_product(
    state: &SharedState,
    device_id: Uuid,
    req: &ImportRequest,
    write: bool,
    row: &Map<String, Value>,
) -> RowResult {
    let name = match get_str(row, "name") {
        Some(v) => v,
        None => return Err("Nom requis".to_string()),
    };

    let plan = classify(state, req, "products", row)
        .await
        .map_err(|e| e.to_string())?;
    let (id, op) = match plan {
        Plan::Skip(why) => return Ok(("skip", Some(why.to_string()))),
        Plan::Insert => (
            get_uuid(row, "id").unwrap_or_else(|| Uuid::new_v4().to_string()),
            OpKind::Insert,
        ),
        Plan::Update(id) => (id, OpKind::Update),
    };

    // Resolve the category reference (default: by name, create-missing) up front
    // so an unresolved value is reported in the preview, not only on commit.
    let category_id = resolve_field_ref(
        state,
        device_id,
        req,
        write,
        row,
        "category",
        "categories",
        "name",
    )
    .await?;

    if !write {
        return Ok((
            if matches!(op, OpKind::Insert) {
                "insert"
            } else {
                "update"
            },
            Some(id),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let sku = get_str(row, "sku");
    let brand = get_str(row, "brand");
    let description = get_str(row, "description");
    let unit = get_str(row, "unit");
    let is_service = get_bool(row, "is_service");
    let purchase = get_i64(row, "purchase_price_ht_cents");
    let selling = get_i64(row, "selling_price_ttc_cents");
    let vat = get_i64(row, "vat_rate_bp");
    let stock = get_i64(row, "stock_milli");
    let low_stock = get_i64(row, "low_stock_threshold");
    let is_active = get_bool(row, "is_active");

    if matches!(op, OpKind::Insert) {
        sqlx::query(
            r#"INSERT INTO products (
                id, category_id, name, sku, brand, description, unit, is_service,
                purchase_price_ht_cents, selling_price_ttc_cents, vat_rate_bp,
                stock_milli, low_stock_threshold, is_active,
                created_at, updated_at, row_version, field_meta
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,1,'{}')"#,
        )
        .bind(&id)
        .bind(&category_id)
        .bind(&name)
        .bind(&sku)
        .bind(&brand)
        .bind(&description)
        .bind(unit.as_deref().unwrap_or("piece"))
        .bind(is_service.unwrap_or(false) as i64)
        .bind(&purchase)
        .bind(&selling)
        .bind(vat.unwrap_or(2000))
        .bind(stock.unwrap_or(0))
        .bind(&low_stock)
        .bind(is_active.unwrap_or(true) as i64)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query(
            r#"UPDATE products SET
                category_id             = COALESCE($2, category_id),
                name                    = COALESCE($3, name),
                sku                     = COALESCE($4, sku),
                brand                   = COALESCE($5, brand),
                description             = COALESCE($6, description),
                unit                    = COALESCE($7, unit),
                is_service              = COALESCE($8, is_service),
                purchase_price_ht_cents = COALESCE($9, purchase_price_ht_cents),
                selling_price_ttc_cents = COALESCE($10, selling_price_ttc_cents),
                vat_rate_bp             = COALESCE($11, vat_rate_bp),
                stock_milli             = COALESCE($12, stock_milli),
                low_stock_threshold     = COALESCE($13, low_stock_threshold),
                is_active               = COALESCE($14, is_active),
                updated_at              = $15,
                row_version             = row_version + 1
               WHERE id = $1"#,
        )
        .bind(&id)
        .bind(&category_id)
        .bind(Some(&name))
        .bind(&sku)
        .bind(&brand)
        .bind(&description)
        .bind(&unit)
        .bind(is_service.map(|b| b as i64))
        .bind(&purchase)
        .bind(&selling)
        .bind(&vat)
        .bind(&stock)
        .bind(&low_stock)
        .bind(is_active.map(|b| b as i64))
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }

    let payload = fetch_product_json(state, &id)
        .await
        .map_err(|e| e.to_string())?;
    let entity_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    append(
        &state.db,
        device_id,
        EntityType::Product,
        entity_id,
        op,
        payload,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok((
        if matches!(op, OpKind::Insert) {
            "insert"
        } else {
            "update"
        },
        Some(id),
    ))
}

// ===========================================================================
// Vehicles
// ===========================================================================

async fn fetch_vehicle_json(state: &SharedState, id: &str) -> LocalResult<Value> {
    let r: (
        String,
        Option<String>,
        String,
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        String,
        String,
    ) = sqlx::query_as(
        r#"SELECT id, client_id, vehicle_type, make, model, year, cylinder_capacity,
                  vin_or_serial, plate_number, purchase_price_ht_cents,
                  selling_price_ttc_cents, notes, created_at, updated_at
           FROM vehicles WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(json!({
        "id": r.0, "client_id": r.1, "vehicle_type": r.2, "make": r.3, "model": r.4,
        "year": r.5, "cylinder_capacity": r.6, "vin_or_serial": r.7, "plate_number": r.8,
        "purchase_price_ht_cents": r.9, "selling_price_ttc_cents": r.10, "notes": r.11,
        "created_at": r.12, "updated_at": r.13,
    }))
}

async fn import_vehicle(
    state: &SharedState,
    device_id: Uuid,
    req: &ImportRequest,
    write: bool,
    row: &Map<String, Value>,
) -> RowResult {
    let make = match get_str(row, "make") {
        Some(v) => v,
        None => return Err("Marque requise".to_string()),
    };
    let model = match get_str(row, "model") {
        Some(v) => v,
        None => return Err("Modèle requis".to_string()),
    };

    let plan = classify(state, req, "vehicles", row)
        .await
        .map_err(|e| e.to_string())?;
    let (id, op) = match plan {
        Plan::Skip(why) => return Ok(("skip", Some(why.to_string()))),
        Plan::Insert => (
            get_uuid(row, "id").unwrap_or_else(|| Uuid::new_v4().to_string()),
            OpKind::Insert,
        ),
        Plan::Update(id) => (id, OpKind::Update),
    };

    // Resolve the owner reference (default: client by e-mail or phone) up front.
    let client_id = resolve_field_ref(
        state,
        device_id,
        req,
        write,
        row,
        "owner",
        "clients",
        "email_or_phone",
    )
    .await?;

    if !write {
        return Ok((
            if matches!(op, OpKind::Insert) {
                "insert"
            } else {
                "update"
            },
            Some(id),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let vehicle_type = get_str(row, "vehicle_type");
    let year = get_i64(row, "year");
    let cc = get_i64(row, "cylinder_capacity");
    let vin = get_str(row, "vin_or_serial");
    let plate = get_str(row, "plate_number");
    let purchase = get_i64(row, "purchase_price_ht_cents");
    let selling = get_i64(row, "selling_price_ttc_cents");
    let notes = get_str(row, "notes");

    if matches!(op, OpKind::Insert) {
        sqlx::query(
            r#"INSERT INTO vehicles (
                id, client_id, vehicle_type, make, model, year, cylinder_capacity,
                vin_or_serial, plate_number, purchase_price_ht_cents,
                selling_price_ttc_cents, notes, created_at, updated_at, row_version, field_meta
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,1,'{}')"#,
        )
        .bind(&id)
        .bind(&client_id)
        .bind(vehicle_type.as_deref().unwrap_or("moto"))
        .bind(&make)
        .bind(&model)
        .bind(&year)
        .bind(&cc)
        .bind(&vin)
        .bind(&plate)
        .bind(&purchase)
        .bind(&selling)
        .bind(&notes)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query(
            r#"UPDATE vehicles SET
                client_id               = COALESCE($2, client_id),
                vehicle_type            = COALESCE($3, vehicle_type),
                make                    = COALESCE($4, make),
                model                   = COALESCE($5, model),
                year                    = COALESCE($6, year),
                cylinder_capacity       = COALESCE($7, cylinder_capacity),
                vin_or_serial           = COALESCE($8, vin_or_serial),
                plate_number            = COALESCE($9, plate_number),
                purchase_price_ht_cents = COALESCE($10, purchase_price_ht_cents),
                selling_price_ttc_cents = COALESCE($11, selling_price_ttc_cents),
                notes                   = COALESCE($12, notes),
                updated_at              = $13,
                row_version             = row_version + 1
               WHERE id = $1"#,
        )
        .bind(&id)
        .bind(&client_id)
        .bind(&vehicle_type)
        .bind(Some(&make))
        .bind(Some(&model))
        .bind(&year)
        .bind(&cc)
        .bind(&vin)
        .bind(&plate)
        .bind(&purchase)
        .bind(&selling)
        .bind(&notes)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }

    let payload = fetch_vehicle_json(state, &id)
        .await
        .map_err(|e| e.to_string())?;
    let entity_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    append(
        &state.db,
        device_id,
        EntityType::Vehicle,
        entity_id,
        op,
        payload,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok((
        if matches!(op, OpKind::Insert) {
            "insert"
        } else {
            "update"
        },
        Some(id),
    ))
}

// ===========================================================================
// Sales
// ===========================================================================

#[allow(clippy::type_complexity)]
async fn fetch_sale_json(state: &SharedState, id: &str) -> LocalResult<Value> {
    let r: (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
        String,
    ) = sqlx::query_as(
        r#"SELECT id, client_id, vehicle_id, reference, status, sale_date,
                  total_ht_cents, total_ttc_cents, payment_method, notes,
                  created_at, updated_at
           FROM sales WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(json!({
        "id": r.0, "client_id": r.1, "vehicle_id": r.2, "reference": r.3, "status": r.4,
        "sale_date": r.5, "total_ht_cents": r.6, "total_ttc_cents": r.7,
        "payment_method": r.8, "notes": r.9, "created_at": r.10, "updated_at": r.11,
    }))
}

async fn import_sale(
    state: &SharedState,
    device_id: Uuid,
    req: &ImportRequest,
    write: bool,
    row: &Map<String, Value>,
) -> RowResult {
    let plan = classify(state, req, "sales", row)
        .await
        .map_err(|e| e.to_string())?;
    let (id, op) = match plan {
        Plan::Skip(why) => return Ok(("skip", Some(why.to_string()))),
        Plan::Insert => (
            get_uuid(row, "id").unwrap_or_else(|| Uuid::new_v4().to_string()),
            OpKind::Insert,
        ),
        Plan::Update(id) => (id, OpKind::Update),
    };

    // Resolve the related client + vehicle up front so unresolved links are
    // reported in the preview.
    let client_id = resolve_field_ref(
        state,
        device_id,
        req,
        write,
        row,
        "client",
        "clients",
        "email_or_phone",
    )
    .await?;
    let vehicle_id = resolve_field_ref(
        state,
        device_id,
        req,
        write,
        row,
        "vehicle",
        "vehicles",
        "vin_or_serial",
    )
    .await?;

    if !write {
        return Ok((
            if matches!(op, OpKind::Insert) {
                "insert"
            } else {
                "update"
            },
            Some(id),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let reference = get_str(row, "reference");
    let status = get_str(row, "status").unwrap_or_else(|| "invoice".to_string());
    let sale_date = get_str(row, "sale_date");
    let total_ht = get_i64(row, "total_ht_cents");
    let total_ttc = get_i64(row, "total_ttc_cents");
    let payment_method = get_str(row, "payment_method");
    let notes = get_str(row, "notes");

    if matches!(op, OpKind::Insert) {
        sqlx::query(
            r#"INSERT INTO sales (
                id, client_id, vehicle_id, reference, status, sale_date,
                total_ht_cents, total_ttc_cents, payment_method, notes,
                created_at, updated_at, row_version, field_meta
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,1,'{}')"#,
        )
        .bind(&id)
        .bind(&client_id)
        .bind(&vehicle_id)
        .bind(&reference)
        .bind(&status)
        .bind(&sale_date)
        .bind(&total_ht)
        .bind(&total_ttc)
        .bind(&payment_method)
        .bind(&notes)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        let updated = sqlx::query(
            r#"UPDATE sales SET
                client_id      = COALESCE($2, client_id),
                vehicle_id     = COALESCE($3, vehicle_id),
                reference      = COALESCE($4, reference),
                status         = COALESCE($5, status),
                sale_date      = COALESCE($6, sale_date),
                total_ht_cents = COALESCE($7, total_ht_cents),
                total_ttc_cents = COALESCE($8, total_ttc_cents),
                payment_method = COALESCE($9, payment_method),
                notes          = COALESCE($10, notes),
                updated_at     = $11,
                row_version    = row_version + 1
               WHERE id = $1 AND locked_at IS NULL"#,
        )
        .bind(&id)
        .bind(&client_id)
        .bind(&vehicle_id)
        .bind(&reference)
        .bind(Some(&status))
        .bind(&sale_date)
        .bind(&total_ht)
        .bind(&total_ttc)
        .bind(&payment_method)
        .bind(&notes)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
        if updated.rows_affected() == 0 {
            return Err(
                "Document scellé dans le journal fiscal : il ne peut plus être modifié (émettez un avoir)."
                    .to_string(),
            );
        }
    }

    let payload = fetch_sale_json(state, &id)
        .await
        .map_err(|e| e.to_string())?;
    let entity_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    append(
        &state.db,
        device_id,
        EntityType::Sale,
        entity_id,
        op,
        payload,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok((
        if matches!(op, OpKind::Insert) {
            "insert"
        } else {
            "update"
        },
        Some(id),
    ))
}

// ===========================================================================
// Sale lines
// ===========================================================================

#[allow(clippy::type_complexity)]
async fn fetch_sale_line_json(state: &SharedState, id: &str) -> LocalResult<Value> {
    let r: (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
        Option<i64>,
        i64,
        i64,
        Option<i64>,
        i64,
        String,
        String,
    ) = sqlx::query_as(
        r#"SELECT id, sale_id, product_id, label, quantity_milli,
                  unit_price_ht_cents, unit_price_ttc_cents, vat_rate_bp,
                  discount_bp, line_total_ttc_cents, position, created_at, updated_at
           FROM sale_lines WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(json!({
        "id": r.0, "sale_id": r.1, "product_id": r.2, "label": r.3,
        "quantity_milli": r.4, "unit_price_ht_cents": r.5, "unit_price_ttc_cents": r.6,
        "vat_rate_bp": r.7, "discount_bp": r.8, "line_total_ttc_cents": r.9,
        "position": r.10, "created_at": r.11, "updated_at": r.12,
    }))
}

async fn import_sale_line(
    state: &SharedState,
    device_id: Uuid,
    req: &ImportRequest,
    write: bool,
    row: &Map<String, Value>,
) -> RowResult {
    // A line must be attached to a sale. Resolve it first (default: by the
    // sale's business reference) and fail the row if the link can't be found.
    let sale_id = resolve_field_ref(
        state,
        device_id,
        req,
        write,
        row,
        "sale",
        "sales",
        "reference",
    )
    .await?;
    let Some(sale_id) = sale_id else {
        return Err("Vente liée requise (référence introuvable)".to_string());
    };
    let sealed: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM sales WHERE id = $1 AND locked_at IS NOT NULL",
    )
    .bind(&sale_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    if sealed.is_some() {
        return Err(
            "Vente scellée dans le journal fiscal : on ne peut plus lui ajouter ni modifier de ligne."
                .to_string(),
        );
    }
    let product_id = resolve_field_ref(
        state, device_id, req, write, row, "product", "products", "sku",
    )
    .await?;

    let plan = classify(state, req, "sale_lines", row)
        .await
        .map_err(|e| e.to_string())?;
    let (id, op) = match plan {
        Plan::Skip(why) => return Ok(("skip", Some(why.to_string()))),
        Plan::Insert => (Uuid::new_v4().to_string(), OpKind::Insert),
        Plan::Update(id) => (id, OpKind::Update),
    };

    if !write {
        return Ok((
            if matches!(op, OpKind::Insert) {
                "insert"
            } else {
                "update"
            },
            Some(id),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let label = get_str(row, "label");
    let quantity = get_i64(row, "quantity_milli").unwrap_or(1000);
    let unit_ht = get_i64(row, "unit_price_ht_cents");
    let unit_ttc = get_i64(row, "unit_price_ttc_cents");
    let vat = get_i64(row, "vat_rate_bp").unwrap_or(2000);
    let discount = get_i64(row, "discount_bp").unwrap_or(0);
    let line_total = get_i64(row, "line_total_ttc_cents");
    let position = get_i64(row, "position").unwrap_or(0);

    if matches!(op, OpKind::Insert) {
        sqlx::query(
            r#"INSERT INTO sale_lines (
                id, sale_id, product_id, label, quantity_milli,
                unit_price_ht_cents, unit_price_ttc_cents, vat_rate_bp,
                discount_bp, line_total_ttc_cents, position,
                created_at, updated_at, row_version, field_meta
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,1,'{}')"#,
        )
        .bind(&id)
        .bind(&sale_id)
        .bind(&product_id)
        .bind(&label)
        .bind(quantity)
        .bind(&unit_ht)
        .bind(&unit_ttc)
        .bind(vat)
        .bind(discount)
        .bind(&line_total)
        .bind(position)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query(
            r#"UPDATE sale_lines SET
                sale_id              = COALESCE($2, sale_id),
                product_id           = COALESCE($3, product_id),
                label                = COALESCE($4, label),
                quantity_milli       = $5,
                unit_price_ht_cents  = COALESCE($6, unit_price_ht_cents),
                unit_price_ttc_cents = COALESCE($7, unit_price_ttc_cents),
                vat_rate_bp          = $8,
                discount_bp          = $9,
                line_total_ttc_cents = COALESCE($10, line_total_ttc_cents),
                position             = $11,
                updated_at           = $12,
                row_version          = row_version + 1
               WHERE id = $1"#,
        )
        .bind(&id)
        .bind(Some(&sale_id))
        .bind(&product_id)
        .bind(&label)
        .bind(quantity)
        .bind(&unit_ht)
        .bind(&unit_ttc)
        .bind(vat)
        .bind(discount)
        .bind(&line_total)
        .bind(position)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }

    let payload = fetch_sale_line_json(state, &id)
        .await
        .map_err(|e| e.to_string())?;
    let entity_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    append(
        &state.db,
        device_id,
        EntityType::SaleLine,
        entity_id,
        op,
        payload,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok((
        if matches!(op, OpKind::Insert) {
            "insert"
        } else {
            "update"
        },
        Some(id),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::db;
    use crate::local_server::state::LocalState;
    use serde_json::json;
    use std::sync::Arc;

    async fn test_state() -> SharedState {
        let path = std::env::temp_dir().join(format!("motodesk_import_{}.db", Uuid::new_v4()));
        let pool = db::init_pool(&path).await.expect("pool");
        db::run_migrations(&pool).await.expect("migrations");
        db::ensure_device_id(&pool).await.expect("device id");
        Arc::new(LocalState::new(
            pool,
            std::env::temp_dir(),
            "http://127.0.0.1:8080".into(),
            None,
            None,
            "test-local-token".into(),
        ))
    }

    fn row(v: Value) -> Map<String, Value> {
        v.as_object().expect("object row").clone()
    }

    async fn scalar_i64(state: &SharedState, sql: &str) -> i64 {
        let (n,): (i64,) = sqlx::query_as(sql).fetch_one(&state.db).await.unwrap();
        n
    }

    fn req(
        entity: &str,
        mode: &str,
        match_key: &str,
        ignore_duplicates: bool,
        rows: Vec<Map<String, Value>>,
    ) -> ImportRequest {
        ImportRequest {
            entity: entity.into(),
            mode: mode.into(),
            match_key: match_key.into(),
            ignore_duplicates,
            create_missing_categories: true,
            references: std::collections::HashMap::new(),
            rows,
        }
    }

    #[tokio::test]
    async fn clients_insert_then_skip_then_upsert() {
        let state = test_state().await;

        let rows = vec![
            row(json!({ "last_name": "DUPONT", "first_name": "Jean", "phone": "0102030405" })),
            row(json!({ "last_name": "MARTIN", "first_name": "Marie" })),
        ];

        // 1) Insert mode → both created.
        let r = run(
            &state,
            &req("clients", "insert", "last_name", false, rows.clone()),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 2, "two clients inserted");
        assert_eq!(r.skipped, 0);
        assert_eq!(scalar_i64(&state, "SELECT COUNT(*) FROM clients").await, 2);

        // 2) Re-run insert with same rows → both skipped (matched by last_name).
        let r = run(
            &state,
            &req("clients", "insert", "last_name", false, rows.clone()),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 0);
        assert_eq!(r.skipped, 2, "duplicates skipped in insert mode");

        // 3) Upsert with a new phone → existing row updated, not duplicated.
        let upsert = vec![row(json!({ "last_name": "DUPONT", "phone": "0699999999" }))];
        let r = run(
            &state,
            &req("clients", "upsert", "last_name", false, upsert),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.updated, 1, "existing client updated");
        assert_eq!(r.inserted, 0);
        assert_eq!(scalar_i64(&state, "SELECT COUNT(*) FROM clients").await, 2);

        let (phone,): (Option<String>,) =
            sqlx::query_as("SELECT phone FROM clients WHERE last_name = 'DUPONT'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(phone.as_deref(), Some("0699999999"));

        // change_log must have a row per write (3 inserts/updates total).
        assert_eq!(
            scalar_i64(
                &state,
                "SELECT COUNT(*) FROM change_log WHERE entity_type = 'client'"
            )
            .await,
            3,
        );
    }

    #[tokio::test]
    async fn products_insert_creates_missing_category() {
        let state = test_state().await;

        let rows = vec![row(json!({
            "name": "Pneu Michelin Road 6",
            "category": "Pneus",
            "sku": "PN-001",
            "selling_price_ttc_cents": 9990,
        }))];

        let r = run(&state, &req("products", "insert", "sku", false, rows), true)
            .await
            .unwrap();
        assert_eq!(r.inserted, 1);

        // Category "Pneus" was created and linked.
        assert_eq!(
            scalar_i64(
                &state,
                "SELECT COUNT(*) FROM categories WHERE name = 'Pneus'"
            )
            .await,
            1,
        );
        let (linked,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM products p JOIN categories c ON c.id = p.category_id WHERE c.name = 'Pneus'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(linked, 1, "product linked to created category");

        let (price,): (Option<i64>,) =
            sqlx::query_as("SELECT selling_price_ttc_cents FROM products WHERE sku = 'PN-001'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(price, Some(9990));
    }

    #[tokio::test]
    async fn products_link_to_imported_categories_by_name() {
        let state = test_state().await;
        let cat_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

        let r = run(
            &state,
            &req(
                "categories",
                "insert",
                "name",
                false,
                vec![row(json!({
                    "id": cat_id,
                    "name": "Huiles",
                    "position": 170,
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1);

        let (stored,): (String,) =
            sqlx::query_as("SELECT id FROM categories WHERE name = 'Huiles'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(stored, cat_id, "imported category keeps its id");

        let r = run(
            &state,
            &req(
                "products",
                "insert",
                "sku",
                false,
                vec![row(json!({
                    "name": "Huile 10W40",
                    "category": "Huiles",
                    "sku": "HUI-001",
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1);

        let (linked,): (String,) =
            sqlx::query_as("SELECT category_id FROM products WHERE sku = 'HUI-001'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(linked, cat_id);
    }

    #[tokio::test]
    async fn products_link_to_imported_categories_by_id() {
        let state = test_state().await;
        let cat_id = "aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff";

        run(
            &state,
            &req(
                "categories",
                "insert",
                "name",
                false,
                vec![row(json!({
                    "id": cat_id,
                    "name": "Pneus",
                }))],
            ),
            true,
        )
        .await
        .unwrap();

        let mut body = req(
            "products",
            "insert",
            "sku",
            false,
            vec![row(json!({
                "name": "Pneu avant",
                "category": cat_id,
                "sku": "PN-AV",
            }))],
        );
        body.create_missing_categories = false;
        body.references.insert("category".into(), "id".into());

        let r = run(&state, &body, true).await.unwrap();
        assert_eq!(r.inserted, 1);
        assert_eq!(r.errored, 0);

        let (linked,): (String,) =
            sqlx::query_as("SELECT category_id FROM products WHERE sku = 'PN-AV'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(linked, cat_id);
    }

    #[tokio::test]
    async fn vehicles_link_to_clients_by_phone_when_ids_differ() {
        let state = test_state().await;

        // Simulate a wizard client import that minted a new id (no `id` in the row).
        run(
            &state,
            &req(
                "clients",
                "insert",
                "last_name",
                false,
                vec![row(json!({
                    "last_name": "Baudet Alain",
                    "phone": "02 97 55 41 75",
                }))],
            ),
            true,
        )
        .await
        .unwrap();

        let r = run(
            &state,
            &req(
                "vehicles",
                "insert",
                "vin_or_serial",
                true,
                vec![row(json!({
                    "make": "Yamaha",
                    "model": "Aerox",
                    "owner": "02 97 55 41 75",
                    "vin_or_serial": "VG55BR00002327075",
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1, "{:?}", r.rows);
        assert_eq!(r.errored, 0);

        let (linked,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM vehicles v JOIN clients c ON c.id = v.client_id
             WHERE c.phone = '02 97 55 41 75'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(linked, 1);
    }

    #[tokio::test]
    async fn sales_link_by_phone_and_vin_when_export_ids_differ() {
        let state = test_state().await;

        run(
            &state,
            &req(
                "clients",
                "insert",
                "last_name",
                false,
                vec![row(json!({
                    "last_name": "Baudet Alain",
                    "phone": "02 97 55 41 75",
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        run(
            &state,
            &req(
                "vehicles",
                "insert",
                "vin_or_serial",
                true,
                vec![row(json!({
                    "make": "Yamaha",
                    "model": "Aerox",
                    "owner": "02 97 55 41 75",
                    "vin_or_serial": "VG55BR00002327075",
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        run(
            &state,
            &req(
                "products",
                "insert",
                "sku",
                false,
                vec![row(json!({ "name": "Huile", "sku": "HUI-10W40" }))],
            ),
            true,
        )
        .await
        .unwrap();

        // Export UUIDs in *_id, natural keys in client/vehicle — the wizard
        // case after parents were imported without preserving ids.
        let r = run(
            &state,
            &req_refs(
                "sales",
                "insert",
                "reference",
                &[("client", "email_or_phone"), ("vehicle", "vin_or_serial")],
                vec![row(json!({
                    "reference": "684@2004-12-24",
                    "client_id": "00000000-0000-0000-0000-000000000001",
                    "client": "02 97 55 41 75",
                    "vehicle_id": "00000000-0000-0000-0000-000000000002",
                    "vehicle": "VG55BR00002327075",
                    "total_ttc_cents": 197900,
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1, "{:?}", r.rows);
        assert_eq!(r.errored, 0);

        let (linked,): (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM sales s
               JOIN clients c ON c.id = s.client_id
               JOIN vehicles v ON v.id = s.vehicle_id
               WHERE c.phone = '02 97 55 41 75'
                 AND v.vin_or_serial = 'VG55BR00002327075'"#,
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(linked, 1, "sale linked by phone and VIN, not export UUIDs");

        let r = run(
            &state,
            &req_refs(
                "sale_lines",
                "insert",
                "",
                &[("sale", "reference"), ("product", "sku")],
                vec![row(json!({
                    "sale_id": "00000000-0000-0000-0000-000000000003",
                    "sale": "684@2004-12-24",
                    "product_id": "00000000-0000-0000-0000-000000000004",
                    "product": "HUI-10W40",
                    "label": "Huile 10W40",
                    "quantity_milli": 1000,
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1, "{:?}", r.rows);
        assert_eq!(r.errored, 0);
    }

    #[tokio::test]
    async fn vehicles_link_to_clients_by_name_when_there_is_no_contact() {
        let state = test_state().await;
        run(
            &state,
            &req(
                "clients",
                "insert",
                "last_name",
                false,
                vec![row(json!({ "last_name": "Martin Marie" }))],
            ),
            true,
        )
        .await
        .unwrap();

        let r = run(
            &state,
            &req(
                "vehicles",
                "insert",
                "vin_or_serial",
                true,
                vec![row(json!({
                    "make": "MBK",
                    "model": "Booster",
                    "owner": "Martin Marie",
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1, "{:?}", r.rows);

        let (linked,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM vehicles v JOIN clients c ON c.id = v.client_id
             WHERE c.last_name = 'Martin Marie'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(linked, 1);
    }

    #[tokio::test]
    async fn clients_ignore_duplicates_blind_insert() {
        let state = test_state().await;
        let row1 = row(json!({ "last_name": "DUPONT", "first_name": "Jean" }));

        run(
            &state,
            &req("clients", "insert", "last_name", false, vec![row1.clone()]),
            true,
        )
        .await
        .unwrap();

        // With dedup: second insert skipped.
        let r = run(
            &state,
            &req("clients", "insert", "last_name", false, vec![row1.clone()]),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.skipped, 1);
        assert_eq!(scalar_i64(&state, "SELECT COUNT(*) FROM clients").await, 1);

        // Blind import: same row inserted again (last_name is not unique).
        let r = run(
            &state,
            &req("clients", "insert", "", true, vec![row1]),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1);
        assert_eq!(scalar_i64(&state, "SELECT COUNT(*) FROM clients").await, 2);
    }

    fn req_refs(
        entity: &str,
        mode: &str,
        match_key: &str,
        refs: &[(&str, &str)],
        rows: Vec<Map<String, Value>>,
    ) -> ImportRequest {
        ImportRequest {
            entity: entity.into(),
            mode: mode.into(),
            match_key: match_key.into(),
            ignore_duplicates: match_key.is_empty(),
            create_missing_categories: true,
            references: refs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            rows,
        }
    }

    #[tokio::test]
    async fn sales_and_lines_resolve_references() {
        let state = test_state().await;

        // Seed a client and a product the sale + line will link to.
        run(
            &state,
            &req(
                "clients",
                "insert",
                "email",
                false,
                vec![row(json!({ "last_name": "DURAND", "email": "d@ex.fr" }))],
            ),
            true,
        )
        .await
        .unwrap();
        run(
            &state,
            &req(
                "products",
                "insert",
                "sku",
                false,
                vec![row(json!({ "name": "Plaquettes", "sku": "PL-9" }))],
            ),
            true,
        )
        .await
        .unwrap();

        // Sale links the client by e-mail.
        let r = run(
            &state,
            &req_refs(
                "sales",
                "insert",
                "reference",
                &[("client", "email")],
                vec![row(
                    json!({ "reference": "T-100", "client": "d@ex.fr", "total_ttc_cents": 4500 }),
                )],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1);
        let (linked,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sales s JOIN clients c ON c.id = s.client_id WHERE c.email = 'd@ex.fr'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(linked, 1, "sale linked to client by e-mail");

        // Line links the sale by reference and the product by SKU.
        let r = run(
            &state,
            &req_refs(
                "sale_lines",
                "insert",
                "",
                &[("sale", "reference"), ("product", "sku")],
                vec![row(json!({
                    "sale": "T-100", "product": "PL-9", "label": "Plaquettes av.",
                    "quantity_milli": 2000, "unit_price_ttc_cents": 2250,
                }))],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.inserted, 1);
        let (joined,): (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM sale_lines l
               JOIN sales s ON s.id = l.sale_id
               JOIN products p ON p.id = l.product_id
               WHERE s.reference = 'T-100' AND p.sku = 'PL-9'"#,
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(joined, 1, "line linked to both sale and product");

        // Unresolved sale reference → the row is reported as an error.
        let r = run(
            &state,
            &req_refs(
                "sale_lines",
                "insert",
                "",
                &[("sale", "reference")],
                vec![row(json!({ "sale": "DOES-NOT-EXIST", "label": "x" }))],
            ),
            true,
        )
        .await
        .unwrap();
        assert_eq!(r.errored, 1, "unresolved reference flagged as error");
        assert_eq!(r.inserted, 0);
    }
}
