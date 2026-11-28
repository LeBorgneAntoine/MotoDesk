use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, Utc};
use motodesk_core::entity::EntityType;
use motodesk_core::sync::OpKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::local_server::credit;
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::routes::sales::{self, LineInputBody, SaleInput};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log::{self, append, field_meta_for_patch};
use crate::local_server::validate;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", get(show).patch(update).delete(remove))
        .route("/:id/vehicles", get(list_vehicles).post(create_vehicle))
}

pub fn vehicles_router() -> Router<SharedState> {
    Router::new()
        .route("/", get(list_stock_vehicles).post(create_stock_vehicle))
        .route(
            "/:id",
            get(get_vehicle)
                .patch(update_vehicle)
                .delete(delete_vehicle),
        )
        .route("/:id/sell", post(sell_stock_vehicle))
}

#[derive(Debug, Deserialize, Default)]
struct VehicleListQuery {
    is_stock: Option<String>,
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VehicleBody {
    /// On update: hand the vehicle to this customer (ownership transfer).
    client_id: Option<String>,
    vehicle_type: Option<String>,
    make: Option<String>,
    model: Option<String>,
    year: Option<i32>,
    cylinder_capacity: Option<i32>,
    vin_or_serial: Option<String>,
    plate_number: Option<String>,
    odometer_value: Option<i32>,
    odometer_unit: Option<String>,
    is_stock: Option<bool>,
    purchase_price_ht_cents: Option<i32>,
    selling_price_ttc_cents: Option<i32>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SellBody {
    client_id: String,
    vat_rate_bp: Option<i64>,
    reprise: Option<RepriseBody>,
}

#[derive(Debug, Deserialize)]
struct RepriseBody {
    make: String,
    model: String,
    vehicle_type: Option<String>,
    vin_or_serial: Option<String>,
    plate_number: Option<String>,
    purchase_price_ht_cents: i64,
    selling_price_ttc_cents: Option<i32>,
    vat_rate_bp: Option<i64>,
    year: Option<i32>,
}

fn ht_from_ttc(ttc_cents: i64, vat_rate_bp: i64) -> i64 {
    if vat_rate_bp <= 0 {
        return ttc_cents;
    }
    (ttc_cents * 10_000 + (10_000 + vat_rate_bp) / 2) / (10_000 + vat_rate_bp)
}

fn vehicle_json(v: VehicleRow) -> Value {
    json!({
        "id": v.id, "client_id": v.client_id, "vehicle_type": v.vehicle_type,
        "make": v.make, "model": v.model, "year": v.year,
        "cylinder_capacity": v.cylinder_capacity, "vin_or_serial": v.vin_or_serial,
        "plate_number": v.plate_number, "odometer_value": v.odometer_value,
        "odometer_unit": v.odometer_unit, "is_stock": v.is_stock != 0,
        "purchase_price_ht_cents": v.purchase_price_ht_cents,
        "selling_price_ttc_cents": v.selling_price_ttc_cents,
        "notes": v.notes, "created_at": v.created_at, "updated_at": v.updated_at,
    })
}

async fn list_stock_vehicles(
    State(state): State<SharedState>,
    Query(q): Query<VehicleListQuery>,
) -> LocalResult<Json<Vec<Value>>> {
    let stock_only = q
        .is_stock
        .as_deref()
        .map(|s| matches!(s, "1" | "true" | "yes"))
        .unwrap_or(true);
    if !stock_only {
        return Ok(Json(vec![]));
    }
    let needle = q.q.as_ref().map(|s| format!("%{s}%"));
    let rows: Vec<VehicleRow> = sqlx::query_as(
        r#"
        SELECT id, client_id, vehicle_type, make, model, year, cylinder_capacity,
               vin_or_serial, plate_number, odometer_value, odometer_unit,
               is_stock, purchase_price_ht_cents, selling_price_ttc_cents,
               notes, created_at, updated_at
        FROM vehicles
        WHERE deleted_at IS NULL AND is_stock = 1
          AND ($1 IS NULL
               OR vin_or_serial LIKE $1 OR plate_number LIKE $1
               OR make LIKE $1 OR model LIKE $1)
        ORDER BY updated_at DESC
        LIMIT 500
        "#,
    )
    .bind(needle)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(vehicle_json).collect()))
}

async fn insert_vehicle(
    state: &SharedState,
    client_id: Option<&str>,
    body: &VehicleBody,
    force_stock: bool,
) -> LocalResult<Value> {
    let body = &validated_vehicle(body, true)?;
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let is_stock = if force_stock {
        1
    } else {
        i32::from(body.is_stock.unwrap_or(false))
    };
    sqlx::query(
        r#"
        INSERT INTO vehicles (
            id, client_id, vehicle_type, make, model, year, cylinder_capacity,
            vin_or_serial, plate_number, odometer_value, odometer_unit,
            is_stock, purchase_price_ht_cents, selling_price_ttc_cents,
            notes, created_at, updated_at, row_version, field_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$16,1,'{}')
        "#,
    )
    .bind(id.to_string())
    .bind(client_id)
    .bind(body.vehicle_type.as_deref().unwrap_or("moto"))
    .bind(body.make.as_deref().unwrap_or(""))
    .bind(body.model.as_deref().unwrap_or(""))
    .bind(body.year)
    .bind(body.cylinder_capacity)
    .bind(&body.vin_or_serial)
    .bind(&body.plate_number)
    .bind(body.odometer_value.unwrap_or(0))
    .bind(body.odometer_unit.as_deref().unwrap_or("km"))
    .bind(is_stock)
    .bind(body.purchase_price_ht_cents)
    .bind(body.selling_price_ttc_cents)
    .bind(&body.notes)
    .bind(&now)
    .execute(&state.db)
    .await?;

    let device_id = state.device_id().await?;
    let payload = load_vehicle(state, id).await?;
    let _ = append(
        &state.db,
        device_id,
        EntityType::Vehicle,
        id,
        OpKind::Insert,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(payload)
}

async fn create_stock_vehicle(
    State(state): State<SharedState>,
    Json(body): Json<VehicleBody>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    if body.make.as_deref().unwrap_or("").trim().is_empty()
        || body.model.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err(LocalError::BadRequest(
            "indiquez la marque et le modèle".into(),
        ));
    }
    let payload = insert_vehicle(&state, None, &body, true).await?;
    Ok((StatusCode::CREATED, Json(payload)))
}

async fn sell_stock_vehicle(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SellBody>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    sales::assert_stock_vehicle_sellable(&state, &id.to_string()).await?;
    let vehicle = load_vehicle(&state, id).await?;
    let ttc = vehicle
        .get("selling_price_ttc_cents")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| {
            LocalError::BadRequest("ce véhicule n'a pas de prix de vente TTC".into())
        })?;
    // A business that charges no VAT sells at the price on the ticket, with none in it.
    let usual = if crate::local_server::fiscal::mentions::load(&state.db).await?.vat_subject { 2_000 } else { 0 };
    let vat = body.vat_rate_bp.unwrap_or(usual);
    let ht = ht_from_ttc(ttc, vat);
    let make = vehicle.get("make").and_then(|v| v.as_str()).unwrap_or("");
    let model = vehicle.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let vin = vehicle
        .get("vin_or_serial")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut label = format!("{make} {model}").trim().to_string();
    if !vin.is_empty() {
        label = format!("{label} · {vin}");
    }

    let mut lines = vec![LineInputBody {
        product_id: None,
        label: Some(label),
        quantity_milli: 1_000,
        unit_price_ht_cents: ht,
        // The price on the bike's ticket is the price: a round TTC figure has no
        // exact HT price in cents.
        unit_price_ttc_cents: Some(ttc),
        vat_rate_bp: vat,
        discount_bp: 0,
    }];

    if let Some(reprise) = &body.reprise {
        let rvat = reprise.vat_rate_bp.unwrap_or(vat);
        if reprise.purchase_price_ht_cents != 0 {
            let rlabel = format!("Reprise {} {}", reprise.make, reprise.model);
            lines.push(LineInputBody {
                product_id: None,
                label: Some(rlabel),
                quantity_milli: 1_000,
                unit_price_ht_cents: -reprise.purchase_price_ht_cents.abs(),
                vat_rate_bp: rvat,
                discount_bp: 0,
                unit_price_ttc_cents: None,
            });
        }
        insert_vehicle(
            &state,
            None,
            &VehicleBody {
                client_id: None,
                vehicle_type: reprise.vehicle_type.clone(),
                make: Some(reprise.make.clone()),
                model: Some(reprise.model.clone()),
                year: reprise.year,
                cylinder_capacity: None,
                vin_or_serial: reprise.vin_or_serial.clone(),
                plate_number: reprise.plate_number.clone(),
                odometer_value: None,
                odometer_unit: None,
                is_stock: Some(true),
                purchase_price_ht_cents: Some(reprise.purchase_price_ht_cents.abs() as i32),
                selling_price_ttc_cents: reprise.selling_price_ttc_cents,
                notes: Some("Reprise VN/VO".into()),
            },
            true,
        )
        .await?;
    }

    sales::create(
        State(state.clone()),
        Json(SaleInput {
            client_id: Some(body.client_id),
            vehicle_id: Some(id.to_string()),
            sale_date: None,
            notes: None,
            status: Some("draft".into()),
            document_kind: Some("invoice".into()),
            lines,
        }),
    )
    .await
}

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ClientInput {
    pub first_name: Option<String>,
    pub last_name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub credit_limit_cents: Option<i64>,
    #[serde(default)]
    pub credit_blocked: Option<bool>,
}

/// The rules a client record must meet, and the cleaned-up record to store.
fn validated_client(b: ClientInput) -> LocalResult<ClientInput> {
    Ok(ClientInput {
        first_name: validate::optional("Prénom", &b.first_name, 100)?,
        last_name: validate::required("Nom", &b.last_name, 100)?,
        phone: validate::phone(&b.phone)?,
        email: validate::email(&b.email)?,
        address: validate::optional("Adresse", &b.address, 500)?,
        notes: validate::optional("Notes", &b.notes, 5000)?,
        credit_limit_cents: validate::non_negative(
            "Plafond d'encours",
            b.credit_limit_cents,
            validate::MAX_CENTS,
        )?,
        credit_blocked: b.credit_blocked,
    })
}

/// The rules a vehicle record must meet. `creating` makes make and model
/// mandatory; an update that sends them must not blank them.
fn validated_vehicle(b: &VehicleBody, creating: bool) -> LocalResult<VehicleBody> {
    let this_year = i64::from(Utc::now().year());
    validate::one_of("Type de véhicule", &b.vehicle_type, validate::VEHICLE_TYPES)?;
    validate::one_of("Unité de kilométrage", &b.odometer_unit, validate::ODOMETER_UNITS)?;
    let name = |field: &str, v: &Option<String>| -> LocalResult<Option<String>> {
        match v {
            Some(s) => validate::required(field, s, 60).map(Some),
            None if creating => validate::required(field, "", 60).map(Some),
            None => Ok(None),
        }
    };
    let int = |field: &str, v: Option<i32>, max: i64| -> LocalResult<Option<i32>> {
        Ok(validate::non_negative(field, v.map(i64::from), max)?.map(|n| n as i32))
    };
    Ok(VehicleBody {
        client_id: b.client_id.clone(),
        vehicle_type: b.vehicle_type.clone(),
        make: name("Marque", &b.make)?,
        model: name("Modèle", &b.model)?,
        year: validate::year(b.year.map(i64::from), this_year)?.map(|y| y as i32),
        cylinder_capacity: int("Cylindrée", b.cylinder_capacity, 10_000)?,
        vin_or_serial: validate::vin(&b.vin_or_serial)?,
        plate_number: validate::plate(&b.plate_number)?,
        odometer_value: int("Kilométrage", b.odometer_value, 9_999_999)?,
        odometer_unit: b.odometer_unit.clone(),
        is_stock: b.is_stock,
        purchase_price_ht_cents: int("Prix d'achat HT", b.purchase_price_ht_cents, i64::from(i32::MAX))?,
        selling_price_ttc_cents: int("Prix de vente TTC", b.selling_price_ttc_cents, i64::from(i32::MAX))?,
        notes: validate::optional("Notes", &b.notes, 2000)?,
    })
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct ClientRow {
    id: String,
    first_name: Option<String>,
    last_name: String,
    phone: Option<String>,
    email: Option<String>,
    address: Option<String>,
    notes: Option<String>,
    credit_limit_cents: Option<i64>,
    credit_blocked: i64,
    created_at: String,
    updated_at: String,
    row_version: i64,
    field_meta: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct ClientListItem {
    id: String,
    first_name: Option<String>,
    last_name: String,
    phone: Option<String>,
    email: Option<String>,
    vehicle_count: i64,
    created_at: String,
    updated_at: String,
}

fn row_to_client(r: ClientRow) -> Value {
    json!({
        "id": r.id,
        "first_name": r.first_name,
        "last_name": r.last_name,
        "phone": r.phone,
        "email": r.email,
        "address": r.address,
        "notes": r.notes,
        "credit_limit_cents": r.credit_limit_cents,
        "credit_blocked": r.credit_blocked != 0,
        "created_at": r.created_at,
        "updated_at": r.updated_at,
    })
}

#[derive(Debug, Serialize)]
struct ClientListPage {
    items: Vec<ClientListItem>,
    total: i64,
}

/// The words of a search, each turned into a LIKE pattern. "Jean Dupont" used to
/// be one pattern tried against one column at a time, so it matched nobody: now
/// every word has to match somewhere on the customer, which includes the plate,
/// VIN, make and model of their vehicles. `%`, `_` and `\` typed by the user are
/// literal, not wildcards.
fn search_patterns(q: Option<&str>) -> Vec<String> {
    q.unwrap_or("")
        .split_whitespace()
        .take(6)
        .map(|word| {
            let escaped = word.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            format!("%{escaped}%")
        })
        .collect()
}

/// `AND (…)` for each word, numbered from `$1`.
fn search_clause(words: usize) -> String {
    (1..=words)
        .map(|n| {
            format!(
                r#" AND (c.last_name LIKE ${n} ESCAPE '\' OR c.first_name LIKE ${n} ESCAPE '\'
                    OR c.phone LIKE ${n} ESCAPE '\' OR c.email LIKE ${n} ESCAPE '\'
                    OR EXISTS (SELECT 1 FROM vehicles v2
                               WHERE v2.client_id = c.id AND v2.deleted_at IS NULL
                                 AND (v2.plate_number LIKE ${n} ESCAPE '\'
                                      OR v2.vin_or_serial LIKE ${n} ESCAPE '\'
                                      OR v2.make LIKE ${n} ESCAPE '\'
                                      OR v2.model LIKE ${n} ESCAPE '\')))"#
            )
        })
        .collect()
}

async fn count_clients(state: &SharedState, words: &[String]) -> LocalResult<i64> {
    let sql = format!(
        "SELECT COUNT(*) FROM clients c WHERE c.deleted_at IS NULL{}",
        search_clause(words.len())
    );
    let mut query = sqlx::query_scalar(&sql);
    for w in words {
        query = query.bind(w);
    }
    Ok(query.fetch_one(&state.db).await?)
}

async fn list(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> LocalResult<Json<ClientListPage>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let words = search_patterns(q.q.as_deref());
    let n = words.len();

    let sql = format!(
        r#"
        SELECT c.id, c.first_name, c.last_name, c.phone, c.email,
               COALESCE(v.cnt, 0) as vehicle_count, c.created_at, c.updated_at
        FROM clients c
        LEFT JOIN (
            SELECT client_id, COUNT(*) AS cnt FROM vehicles
            WHERE client_id IS NOT NULL AND deleted_at IS NULL GROUP BY client_id
        ) v ON v.client_id = c.id
        WHERE c.deleted_at IS NULL{}
        ORDER BY c.last_name ASC, c.first_name ASC LIMIT ${} OFFSET ${}
        "#,
        search_clause(n),
        n + 1,
        n + 2
    );
    let mut query = sqlx::query_as::<_, ClientListItem>(&sql);
    for w in &words {
        query = query.bind(w);
    }
    let items = query.bind(limit).bind(offset).fetch_all(&state.db).await?;

    let total = count_clients(&state, &words).await?;
    Ok(Json(ClientListPage { items, total }))
}

async fn show(State(state): State<SharedState>, Path(id): Path<Uuid>) -> LocalResult<Json<Value>> {
    let mut client = find_client(&state, id).await?;
    let vehicles = list_vehicles_inner(&state, id).await?;
    let outstanding = credit::outstanding_cents(&state.db, &id.to_string()).await?;
    if let Some(obj) = client.as_object_mut() {
        obj.insert("vehicles".into(), json!(vehicles));
        obj.insert("outstanding_cents".into(), json!(outstanding));
    }
    Ok(Json(client))
}

async fn find_client(state: &SharedState, id: Uuid) -> LocalResult<Value> {
    let row: ClientRow = sqlx::query_as(
        "SELECT id, first_name, last_name, phone, email, address, notes, credit_limit_cents, credit_blocked, created_at, updated_at, row_version, field_meta FROM clients WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?
    .ok_or(LocalError::NotFound)?;
    Ok(row_to_client(row))
}

async fn create(
    State(state): State<SharedState>,
    Json(body): Json<ClientInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let body = validated_client(body)?;
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let device_id = state.device_id().await?;
    let meta = sync_log::field_meta_for_patch(
        &Default::default(),
        &serde_json::to_value(&body).unwrap_or_default(),
        device_id,
    );

    sqlx::query(
        r#"
        INSERT INTO clients (id, first_name, last_name, phone, email, address, notes, credit_limit_cents, credit_blocked, created_at, updated_at, row_version, field_meta)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 1, $12)
        "#,
    )
    .bind(id.to_string())
    .bind(&body.first_name)
    .bind(body.last_name.trim())
    .bind(&body.phone)
    .bind(&body.email)
    .bind(&body.address)
    .bind(&body.notes)
    .bind(body.credit_limit_cents)
    .bind(if body.credit_blocked.unwrap_or(false) { 1 } else { 0 })
    .bind(&now)
    .bind(&now)
    .bind(serde_json::to_string(&meta).unwrap_or_else(|_| "{}".into()))
    .execute(&state.db)
    .await?;

    let client = find_client(&state, id).await?;
    let _ = append(
        &state.db,
        device_id,
        EntityType::Client,
        id,
        OpKind::Insert,
        client.clone(),
        Some(meta),
    )
    .await?;
    state.notify_sync();

    Ok((StatusCode::CREATED, Json(client)))
}

async fn update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ClientInput>,
) -> LocalResult<Json<Value>> {
    let body = validated_client(body)?;
    let existing: ClientRow = sqlx::query_as(
        "SELECT id, first_name, last_name, phone, email, address, notes, credit_limit_cents, credit_blocked, created_at, updated_at, row_version, field_meta FROM clients WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?
    .ok_or(LocalError::NotFound)?;

    let device_id = state.device_id().await?;
    let old_meta: motodesk_core::sync::FieldMetaMap =
        serde_json::from_str(&existing.field_meta).unwrap_or_default();
    let patch = serde_json::to_value(&body).unwrap_or_default();
    let meta = field_meta_for_patch(&old_meta, &patch, device_id);
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        UPDATE clients SET first_name = $2, last_name = $3, phone = $4, email = $5,
            address = $6, notes = $7,
            credit_limit_cents = $10,
            credit_blocked = COALESCE($11, credit_blocked),
            updated_at = $8, row_version = row_version + 1, field_meta = $9
        WHERE id = $1
        "#,
    )
    .bind(id.to_string())
    .bind(&body.first_name)
    .bind(body.last_name.trim())
    .bind(&body.phone)
    .bind(&body.email)
    .bind(&body.address)
    .bind(&body.notes)
    .bind(&now)
    .bind(serde_json::to_string(&meta).unwrap_or_else(|_| "{}".into()))
    .bind(body.credit_limit_cents)
    .bind(body.credit_blocked.map(|b| if b { 1i64 } else { 0 }))
    .execute(&state.db)
    .await?;

    let client = find_client(&state, id).await?;
    let _ = append(
        &state.db,
        device_id,
        EntityType::Client,
        id,
        OpKind::Update,
        client.clone(),
        Some(meta),
    )
    .await?;
    state.notify_sync();
    Ok(Json(client))
}

async fn remove(State(state): State<SharedState>, Path(id): Path<Uuid>) -> LocalResult<StatusCode> {
    let now = Utc::now().to_rfc3339();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let r = sqlx::query("UPDATE clients SET deleted_at = $2 WHERE id = $1 AND deleted_at IS NULL")
        .bind(id.to_string())
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(LocalError::NotFound);
    }
    // The customer's vehicles stop pointing at a customer who no longer exists:
    // they stay in the base, unattached, and can be given to someone else. (They
    // used to keep the dead reference, so they were listed under nobody.)
    let detached: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM vehicles WHERE client_id = $1 AND deleted_at IS NULL",
    )
    .bind(id.to_string())
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE vehicles SET client_id = NULL, updated_at = $2, row_version = row_version + 1
          WHERE client_id = $1 AND deleted_at IS NULL",
    )
    .bind(id.to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    for (vehicle_id,) in detached {
        if let Ok(vid) = Uuid::parse_str(&vehicle_id) {
            if let Ok(v) = load_vehicle(&state, vid).await {
                let device_id = state.device_id().await?;
                let _ = append(&state.db, device_id, EntityType::Vehicle, vid, OpKind::Update, v, None).await?;
            }
        }
    }
    let device_id = state.device_id().await?;
    let _ = append(
        &state.db,
        device_id,
        EntityType::Client,
        id,
        OpKind::Delete,
        json!({ "id": id }),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, sqlx::FromRow, Serialize)]
struct VehicleRow {
    id: String,
    client_id: Option<String>,
    vehicle_type: String,
    make: String,
    model: String,
    year: Option<i32>,
    cylinder_capacity: Option<i32>,
    vin_or_serial: Option<String>,
    plate_number: Option<String>,
    odometer_value: i32,
    odometer_unit: String,
    is_stock: i32,
    purchase_price_ht_cents: Option<i32>,
    selling_price_ttc_cents: Option<i32>,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
}

async fn list_vehicles(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Vec<Value>>> {
    find_client(&state, id).await?;
    Ok(Json(list_vehicles_inner(&state, id).await?))
}

async fn list_vehicles_inner(state: &SharedState, client_id: Uuid) -> LocalResult<Vec<Value>> {
    let rows: Vec<VehicleRow> = sqlx::query_as(
        r#"
        SELECT id, client_id, vehicle_type, make, model, year, cylinder_capacity,
               vin_or_serial, plate_number, odometer_value, odometer_unit,
               is_stock, purchase_price_ht_cents, selling_price_ttc_cents,
               notes, created_at, updated_at
        FROM vehicles WHERE client_id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(client_id.to_string())
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(vehicle_json).collect())
}

async fn create_vehicle(
    State(state): State<SharedState>,
    Path(client_id): Path<Uuid>,
    Json(body): Json<VehicleBody>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    find_client(&state, client_id).await?;
    let payload = insert_vehicle(&state, Some(&client_id.to_string()), &body, false).await?;
    Ok((StatusCode::CREATED, Json(payload)))
}

pub(crate) async fn load_vehicle(state: &SharedState, id: Uuid) -> LocalResult<Value> {
    let row: VehicleRow = sqlx::query_as(
        r#"
        SELECT id, client_id, vehicle_type, make, model, year, cylinder_capacity,
               vin_or_serial, plate_number, odometer_value, odometer_unit,
               is_stock, purchase_price_ht_cents, selling_price_ttc_cents,
               notes, created_at, updated_at
        FROM vehicles WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?
    .ok_or(LocalError::NotFound)?;
    Ok(vehicle_json(row))
}

async fn get_vehicle(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    Ok(Json(load_vehicle(&state, id).await?))
}

async fn ensure_vehicle_exists(state: &SharedState, id: Uuid) -> LocalResult<()> {
    let exists: Option<(String,)> =
        sqlx::query_as("SELECT id FROM vehicles WHERE id = $1 AND deleted_at IS NULL")
            .bind(id.to_string())
            .fetch_optional(&state.db)
            .await?;
    if exists.is_none() {
        return Err(LocalError::NotFound);
    }
    Ok(())
}

async fn update_vehicle(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<VehicleBody>,
) -> LocalResult<Json<Value>> {
    ensure_vehicle_exists(&state, id).await?;
    let body = validated_vehicle(&body, false)?;
    // Handing a bike to another customer is a deliberate act: the target must exist.
    if let Some(new_owner) = body.client_id.as_deref() {
        let owner = Uuid::parse_str(new_owner)
            .map_err(|_| LocalError::BadRequest("client : identifiant invalide".into()))?;
        find_client(&state, owner).await?;
    }
    let now = Utc::now().to_rfc3339();
    // The form always sends the whole record, so the optional fields are replaced
    // (an emptied plate really empties it). The mandatory ones keep their value
    // when absent. `COALESCE` on every column made it impossible to clear a field
    // while still answering "Véhicule enregistré".
    sqlx::query(
        r#"
        UPDATE vehicles SET
            make = COALESCE($2, make),
            model = COALESCE($3, model),
            vehicle_type = COALESCE($4, vehicle_type),
            year = $5,
            cylinder_capacity = $6,
            vin_or_serial = $7,
            plate_number = $8,
            odometer_value = COALESCE($9, odometer_value),
            odometer_unit = COALESCE($10, odometer_unit),
            is_stock = COALESCE($11, is_stock),
            purchase_price_ht_cents = $12,
            selling_price_ttc_cents = $13,
            notes = $14,
            client_id = COALESCE($16, client_id),
            updated_at = $15,
            row_version = row_version + 1
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .bind(&body.make)
    .bind(&body.model)
    .bind(&body.vehicle_type)
    .bind(body.year)
    .bind(body.cylinder_capacity)
    .bind(&body.vin_or_serial)
    .bind(&body.plate_number)
    .bind(body.odometer_value)
    .bind(&body.odometer_unit)
    .bind(body.is_stock.map(|b| b as i32))
    .bind(body.purchase_price_ht_cents)
    .bind(body.selling_price_ttc_cents)
    .bind(&body.notes)
    .bind(&now)
    .bind(&body.client_id)
    .execute(&state.db)
    .await?;
    let device_id = state.device_id().await?;
    let payload = load_vehicle(&state, id).await?;
    let _ = append(
        &state.db,
        device_id,
        EntityType::Vehicle,
        id,
        OpKind::Update,
        payload.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(Json(payload))
}

async fn delete_vehicle(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<StatusCode> {
    let now = Utc::now().to_rfc3339();
    // The VIN is UNIQUE across deleted rows too: free it so the vehicle can be
    // entered again.
    let r = sqlx::query(
        "UPDATE vehicles SET deleted_at = $2,
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
    let _ = append(
        &state.db,
        device_id,
        EntityType::Vehicle,
        id,
        OpKind::Delete,
        json!({ "id": id }),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::routes::sales::tests as sales_tests;
    use crate::local_server::test_support::shop;

    #[test]
    fn ttc_backs_out_ht() {
        assert_eq!(ht_from_ttc(12_000, 2_000), 10_000);
    }

    fn client_input(last_name: &str) -> ClientInput {
        ClientInput {
            first_name: Some("Jean".into()),
            last_name: last_name.into(),
            phone: None,
            email: None,
            address: None,
            notes: None,
            credit_limit_cents: None,
            credit_blocked: None,
        }
    }

    fn vehicle_body() -> VehicleBody {
        VehicleBody {
            client_id: None,
            vehicle_type: Some("moto".into()),
            make: Some("Yamaha".into()),
            model: Some("MT-07".into()),
            year: Some(2021),
            cylinder_capacity: Some(689),
            vin_or_serial: Some("JYARM0300MA000001".into()),
            plate_number: Some("ab-123-cd".into()),
            odometer_value: Some(12_000),
            odometer_unit: Some("km".into()),
            is_stock: None,
            purchase_price_ht_cents: None,
            selling_price_ttc_cents: None,
            notes: Some("RAS".into()),
        }
    }

    #[tokio::test]
    async fn bad_client_data_is_refused_with_a_message_naming_the_field() {
        let state = shop().await;
        for (input, field) in [
            (ClientInput { phone: Some("abc".into()), ..client_input("Martin") }, "Téléphone"),
            (ClientInput { email: Some("not-an-email".into()), ..client_input("Martin") }, "Courriel"),
            (ClientInput { credit_limit_cents: Some(-5000), ..client_input("Martin") }, "Plafond"),
            (client_input("   "), "Nom"),
        ] {
            let err = create(State(state.clone()), Json(input)).await.unwrap_err();
            assert!(
                matches!(&err, LocalError::BadRequest(m) if m.contains(field)),
                "expected a message about {field}, got {err:?}"
            );
        }

        let (_, Json(ok)) = create(
            State(state.clone()),
            Json(ClientInput {
                phone: Some(" 06 12 34 56 78 ".into()),
                email: Some("jean@example.com".into()),
                credit_limit_cents: Some(150_000),
                ..client_input(" Martin ")
            }),
        )
        .await
        .unwrap();
        assert_eq!(ok["last_name"], "Martin");
        assert_eq!(ok["phone"], "06 12 34 56 78");
    }

    #[tokio::test]
    async fn bad_vehicle_data_is_refused() {
        let state = shop().await;
        let owner = sales_tests::insert_client(&state, "Martin").await;
        for (body, field) in [
            (VehicleBody { year: Some(3050), ..vehicle_body() }, "Année"),
            (VehicleBody { odometer_value: Some(-500), ..vehicle_body() }, "Kilométrage"),
            (VehicleBody { plate_number: Some("zz".into()), ..vehicle_body() }, "Plaque"),
            (VehicleBody { make: Some("  ".into()), ..vehicle_body() }, "Marque"),
            (VehicleBody { vehicle_type: Some("fusée".into()), ..vehicle_body() }, "Type"),
        ] {
            let err = create_vehicle(State(state.clone()), Path(owner), Json(body)).await.unwrap_err();
            assert!(
                matches!(&err, LocalError::BadRequest(m) if m.contains(field)),
                "expected a message about {field}, got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_emptied_field_is_really_emptied_on_update() {
        let state = shop().await;
        let owner = sales_tests::insert_client(&state, "Martin").await;
        let (_, Json(v)) = create_vehicle(State(state.clone()), Path(owner), Json(vehicle_body()))
            .await
            .unwrap();
        assert_eq!(v["plate_number"], "AB-123-CD");
        let id = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();

        let Json(updated) = update_vehicle(
            State(state.clone()),
            Path(id),
            Json(VehicleBody {
                plate_number: None,
                vin_or_serial: None,
                notes: None,
                year: None,
                ..vehicle_body()
            }),
        )
        .await
        .unwrap();
        assert!(updated["plate_number"].is_null());
        assert!(updated["vin_or_serial"].is_null());
        assert!(updated["notes"].is_null());
        assert!(updated["year"].is_null());
        assert_eq!(updated["make"], "Yamaha", "mandatory fields are kept");
    }

    #[tokio::test]
    async fn a_vehicle_can_be_handed_to_another_customer() {
        let state = shop().await;
        let seller = sales_tests::insert_client(&state, "Martin").await;
        let buyer = sales_tests::insert_client(&state, "Durand").await;
        let (_, Json(v)) = create_vehicle(State(state.clone()), Path(seller), Json(vehicle_body()))
            .await
            .unwrap();
        let id = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();

        let Json(moved) = update_vehicle(
            State(state.clone()),
            Path(id),
            Json(VehicleBody { client_id: Some(buyer.to_string()), ..vehicle_body() }),
        )
        .await
        .unwrap();
        assert_eq!(moved["client_id"], buyer.to_string());

        let err = update_vehicle(
            State(state.clone()),
            Path(id),
            Json(VehicleBody { client_id: Some(Uuid::new_v4().to_string()), ..vehicle_body() }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, LocalError::NotFound));
    }

    #[tokio::test]
    async fn sell_with_reprise_creates_a_stock_bike() {
        let state = shop().await;
        let now = Utc::now().to_rfc3339();
        let client_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO clients (id, last_name, created_at, updated_at, row_version, field_meta)
             VALUES ($1,'Dupont',$2,$2,1,'{}')",
        )
        .bind(client_id.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();

        let (_, Json(stock)) = create_stock_vehicle(
            State(state.clone()),
            Json(VehicleBody {
                client_id: None,
                vehicle_type: Some("moto".into()),
                make: Some("Yamaha".into()),
                model: Some("MT-07".into()),
                year: Some(2022),
                cylinder_capacity: None,
                vin_or_serial: Some("VINMT07".into()),
                plate_number: None,
                odometer_value: None,
                odometer_unit: None,
                is_stock: Some(true),
                purchase_price_ht_cents: Some(80_000),
                selling_price_ttc_cents: Some(120_000),
                notes: None,
            }),
        )
        .await
        .unwrap();
        let vid = Uuid::parse_str(stock["id"].as_str().unwrap()).unwrap();

        let (_, Json(sale)) = sell_stock_vehicle(
            State(state.clone()),
            Path(vid),
            Json(SellBody {
                client_id: client_id.to_string(),
                vat_rate_bp: Some(2_000),
                reprise: Some(RepriseBody {
                    make: "Honda".into(),
                    model: "CBF".into(),
                    vehicle_type: Some("moto".into()),
                    vin_or_serial: Some("VINREPRISE".into()),
                    plate_number: None,
                    purchase_price_ht_cents: 40_000,
                    selling_price_ttc_cents: Some(50_000),
                    vat_rate_bp: Some(2_000),
                    year: None,
                }),
            }),
        )
        .await
        .unwrap();
        let ht = sale["total_ht_cents"].as_i64().unwrap();
        let vat = sale["total_vat_cents"].as_i64().unwrap();
        let ttc = sale["total_ttc_cents"].as_i64().unwrap();
        assert_eq!(ht + vat, ttc);

        let sale_id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();
        sales_tests::seal(&state, sale_id).await;

        let Json(stock_after) = list_stock_vehicles(
            State(state),
            Query(VehicleListQuery {
                is_stock: Some("true".into()),
                q: Some("VINREPRISE".into()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(stock_after.len(), 1);
        assert_eq!(stock_after[0]["vin_or_serial"], "VINREPRISE");
    }

    async fn names_found_by(state: &SharedState, q: &str) -> Vec<String> {
        let Json(page) = list(
            State(state.clone()),
            Query(ListQuery { q: Some(q.into()), ..Default::default() }),
        )
        .await
        .unwrap();
        page.items.iter().map(|c| c.last_name.clone()).collect()
    }

    #[tokio::test]
    async fn search_matches_a_full_name_and_the_customers_vehicle() {
        let state = shop().await;
        let (_, Json(martin)) = create(State(state.clone()), Json(client_input("Martin"))).await.unwrap();
        let _ = create(
            State(state.clone()),
            Json(ClientInput { first_name: Some("Paul".into()), ..client_input("Durand") }),
        )
        .await
        .unwrap();
        let martin_id = martin["id"].as_str().unwrap().to_string();
        insert_vehicle(&state, Some(&martin_id), &vehicle_body(), false).await.unwrap();

        // First and last name together, in either order.
        assert_eq!(names_found_by(&state, "Jean Martin").await, vec!["Martin"]);
        assert_eq!(names_found_by(&state, "martin jean").await, vec!["Martin"]);
        // A plate, a VIN and a model reach the owner.
        assert_eq!(names_found_by(&state, "ab-123").await, vec!["Martin"]);
        assert_eq!(names_found_by(&state, "JYARM03").await, vec!["Martin"]);
        assert_eq!(names_found_by(&state, "mt-07 jean").await, vec!["Martin"]);
        // Every word has to match; wildcards typed by the user are not wildcards.
        assert!(names_found_by(&state, "Jean Durand").await.is_empty());
        assert!(names_found_by(&state, "%").await.is_empty());
        // No query lists everyone.
        assert_eq!(names_found_by(&state, "  ").await.len(), 2);
    }

    #[tokio::test]
    async fn deleting_a_customer_detaches_their_vehicles() {
        let state = shop().await;
        let (_, Json(owner)) = create(State(state.clone()), Json(client_input("Martin"))).await.unwrap();
        let owner_id = owner["id"].as_str().unwrap().to_string();
        let vehicle = insert_vehicle(&state, Some(&owner_id), &vehicle_body(), false).await.unwrap();
        let vehicle_id = vehicle["id"].as_str().unwrap().to_string();

        remove(State(state.clone()), Path(Uuid::parse_str(&owner_id).unwrap())).await.unwrap();

        let (client_id, deleted): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT client_id, deleted_at FROM vehicles WHERE id = $1")
                .bind(&vehicle_id)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert!(client_id.is_none(), "the vehicle no longer points at the deleted customer");
        assert!(deleted.is_none(), "and it is kept");
    }
}
