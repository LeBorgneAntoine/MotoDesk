use chrono::Utc;
use serde_json::json;
use sqlx::{Column, Row};
use uuid::Uuid;

use crate::local_server::error::LocalResult;

pub(crate) async fn load_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    entity_type: motodesk_core::entity::EntityType,
    entity_id: Uuid,
) -> LocalResult<Option<motodesk_core::sync::EntitySnapshot>> {
    use motodesk_core::entity::EntityType;
    use motodesk_core::sync::{EntitySnapshot, FieldMetaMap};

    let table = match entity_type {
        EntityType::Client => "clients",
        EntityType::Vehicle => "vehicles",
        EntityType::Category => "categories",
        EntityType::Product => "products",
        EntityType::AppSetting => "app_settings",
        EntityType::DocTemplate => "doc_templates",
        EntityType::User => "users",
        EntityType::Sale => "sales",
        EntityType::SaleLine => "sale_lines",
        EntityType::Payment => "payments",
        EntityType::FiscalEntry => "fiscal_journal",
        EntityType::Supplier => "suppliers",
        EntityType::WorkshopOrder => "workshop_orders",
        EntityType::StockMovement => "stock_movements",
    };

    let pk = if entity_type == EntityType::AppSetting {
        "key"
    } else {
        "id"
    };
    // Include tombstones so a later update cannot resurrect a deleted row.
    let sql = format!("SELECT * FROM {table} WHERE {pk} = $1");
    let row = sqlx::query(&sql)
        .bind(entity_id.to_string())
        .fetch_optional(&mut **tx)
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let mut payload = row_to_json(&row)?;
    if entity_type == EntityType::WorkshopOrder {
        attach_workshop_lines(tx, &entity_id.to_string(), &mut payload).await?;
    }
    let field_meta: FieldMetaMap = row
        .try_get::<String, _>("field_meta")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let updated_at = row
        .try_get::<String, _>("updated_at")
        .ok()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    let row_version = row.try_get::<i64, _>("row_version").unwrap_or(1);
    let deleted_at = row
        .try_get::<Option<String>, _>("deleted_at")
        .ok()
        .flatten()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));

    Ok(Some(EntitySnapshot {
        entity_type,
        entity_id,
        row_version,
        deleted_at,
        updated_at,
        payload,
        field_meta,
    }))
}

fn row_to_json(row: &sqlx::sqlite::SqliteRow) -> LocalResult<serde_json::Value> {
    let cols = row.columns();
    let mut map = serde_json::Map::new();
    for col in cols {
        let name = col.name();
        if name == "field_meta" || name == "deleted_at" {
            continue;
        }
        // Read each value as what SQLite says it is. A text-only read dropped every
        // INTEGER and REAL column from the snapshot, and the merge then wrote
        // defaults over them (stock 0, prices NULL, VAT 20 %) whenever a remote
        // change touched a different field of the same row.
        if let Ok(Some(n)) = row.try_get::<Option<i64>, _>(name) {
            map.insert(name.to_string(), serde_json::Value::from(n));
        } else if let Ok(Some(f)) = row.try_get::<Option<f64>, _>(name) {
            if let Some(num) = serde_json::Number::from_f64(f) {
                map.insert(name.to_string(), serde_json::Value::Number(num));
            }
        } else if let Ok(Some(s)) = row.try_get::<Option<String>, _>(name) {
            map.insert(name.to_string(), serde_json::Value::String(s));
        }
    }
    Ok(serde_json::Value::Object(map))
}

pub(crate) async fn persist_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    snap: &motodesk_core::sync::EntitySnapshot,
) -> LocalResult<()> {
    persist_snapshot_with(tx, snap, true).await
}

/// `apply_stock = false` stores a stock movement without moving the shelf figure.
/// A device being rebuilt from the cloud receives the products with their stock
/// already worked out *and* the movements that produced it; applying both would
/// count every movement twice.
pub(crate) async fn persist_snapshot_with(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    snap: &motodesk_core::sync::EntitySnapshot,
    apply_stock: bool,
) -> LocalResult<()> {
    use motodesk_core::entity::EntityType;

    let now = Utc::now().to_rfc3339();
    let field_meta = serde_json::to_string(&snap.field_meta).unwrap_or_else(|_| "{}".into());
    let updated_at = snap.updated_at.to_rfc3339();
    let p = &snap.payload;
    let pid = snap.entity_id.to_string();

    match snap.entity_type {
        EntityType::Client => {
            sqlx::query(
                r#"
                INSERT INTO clients (id, first_name, last_name, phone, email, address, notes, credit_limit_cents, credit_blocked, created_at, updated_at, row_version, field_meta)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $12, $13, $8, $9, $10, $11)
                ON CONFLICT(id) DO UPDATE SET
                    first_name = excluded.first_name,
                    last_name = excluded.last_name,
                    phone = excluded.phone,
                    email = excluded.email,
                    address = excluded.address,
                    notes = excluded.notes,
                    credit_limit_cents = excluded.credit_limit_cents,
                    credit_blocked = excluded.credit_blocked,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("first_name").and_then(|v| v.as_str()))
            .bind(p.get("last_name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("phone").and_then(|v| v.as_str()))
            .bind(p.get("email").and_then(|v| v.as_str()))
            .bind(p.get("address").and_then(|v| v.as_str()))
            .bind(p.get("notes").and_then(|v| v.as_str()))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .bind(p.get("credit_limit_cents").and_then(|v| v.as_i64()))
            .bind(parse_bool(p.get("credit_blocked"), false) as i32)
            .execute(&mut **tx)
            .await?;
        }
        EntityType::Vehicle => {
            sqlx::query(
                r#"
                INSERT INTO vehicles (
                    id, client_id, vehicle_type, make, model, year, cylinder_capacity,
                    vin_or_serial, plate_number, odometer_value, odometer_unit,
                    is_stock, purchase_price_ht_cents, selling_price_ttc_cents,
                    notes, created_at, updated_at, row_version, field_meta
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)
                ON CONFLICT(id) DO UPDATE SET
                    client_id = excluded.client_id,
                    vehicle_type = excluded.vehicle_type,
                    make = excluded.make,
                    model = excluded.model,
                    year = excluded.year,
                    cylinder_capacity = excluded.cylinder_capacity,
                    vin_or_serial = excluded.vin_or_serial,
                    plate_number = excluded.plate_number,
                    odometer_value = excluded.odometer_value,
                    odometer_unit = excluded.odometer_unit,
                    is_stock = excluded.is_stock,
                    purchase_price_ht_cents = excluded.purchase_price_ht_cents,
                    selling_price_ttc_cents = excluded.selling_price_ttc_cents,
                    notes = excluded.notes,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("client_id").and_then(|v| v.as_str()))
            .bind(
                p.get("vehicle_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("moto"),
            )
            .bind(p.get("make").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("model").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(parse_i32(p.get("year")))
            .bind(parse_i32(p.get("cylinder_capacity")))
            .bind(p.get("vin_or_serial").and_then(|v| v.as_str()))
            .bind(p.get("plate_number").and_then(|v| v.as_str()))
            .bind(parse_i32(p.get("odometer_value")).unwrap_or(0))
            .bind(
                p.get("odometer_unit")
                    .and_then(|v| v.as_str())
                    .unwrap_or("km"),
            )
            .bind(parse_bool(p.get("is_stock"), false) as i32)
            .bind(parse_i32(p.get("purchase_price_ht_cents")))
            .bind(parse_i32(p.get("selling_price_ttc_cents")))
            .bind(p.get("notes").and_then(|v| v.as_str()))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .execute(&mut **tx)
            .await?;
        }
        EntityType::Category => {
            sqlx::query(
                r#"
                INSERT INTO categories (id, name, slug, position, created_at, updated_at, row_version, field_meta)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    slug = excluded.slug,
                    position = excluded.position,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("slug").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(parse_i32(p.get("position")).unwrap_or(0))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .execute(&mut **tx)
            .await?;
        }
        EntityType::Product => {
            sqlx::query(
                r#"
                INSERT INTO products (
                    id, category_id, name, sku, brand, description, unit, is_service,
                    purchase_price_ht_cents, selling_price_ttc_cents, vat_rate_bp,
                    stock_milli, low_stock_threshold, is_active,
                    vehicle_type, year, cylinder_capacity, vin_or_serial, plate_number,
                    odometer_value, odometer_unit, notes, parent_id, variant_label, variant_attrs,
                    created_at, updated_at, row_version, field_meta
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$15,$16,$17,$18)
                ON CONFLICT(id) DO UPDATE SET
                    category_id = excluded.category_id,
                    name = excluded.name,
                    sku = excluded.sku,
                    brand = excluded.brand,
                    description = excluded.description,
                    unit = excluded.unit,
                    is_service = excluded.is_service,
                    purchase_price_ht_cents = excluded.purchase_price_ht_cents,
                    selling_price_ttc_cents = excluded.selling_price_ttc_cents,
                    vat_rate_bp = excluded.vat_rate_bp,
                    -- The stock is the sum of its movements, which travel as their
                    -- own changes: a product edit must not overwrite the figure with
                    -- the one its author happened to see.
                    stock_milli = products.stock_milli,
                    low_stock_threshold = excluded.low_stock_threshold,
                    is_active = excluded.is_active,
                    vehicle_type = excluded.vehicle_type,
                    year = excluded.year,
                    cylinder_capacity = excluded.cylinder_capacity,
                    vin_or_serial = excluded.vin_or_serial,
                    plate_number = excluded.plate_number,
                    odometer_value = excluded.odometer_value,
                    odometer_unit = excluded.odometer_unit,
                    notes = excluded.notes,
                    parent_id = excluded.parent_id,
                    variant_label = excluded.variant_label,
                    variant_attrs = excluded.variant_attrs,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("category_id").and_then(|v| v.as_str()))
            .bind(p.get("name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("sku").and_then(|v| v.as_str()))
            .bind(p.get("brand").and_then(|v| v.as_str()))
            .bind(p.get("description").and_then(|v| v.as_str()))
            .bind(p.get("unit").and_then(|v| v.as_str()).unwrap_or("piece"))
            .bind(parse_bool(p.get("is_service"), false) as i32)
            .bind(parse_i32(p.get("purchase_price_ht_cents")))
            .bind(parse_i32(p.get("selling_price_ttc_cents")))
            .bind(parse_i32(p.get("vat_rate_bp")).unwrap_or(2000))
            .bind(stock_milli_of(p))
            .bind(parse_i32(p.get("low_stock_threshold")))
            .bind(parse_bool(p.get("is_active"), true) as i32)
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .bind(p.get("vehicle_type").and_then(|v| v.as_str()))
            .bind(parse_i32(p.get("year")))
            .bind(parse_i32(p.get("cylinder_capacity")))
            .bind(p.get("vin_or_serial").and_then(|v| v.as_str()))
            .bind(p.get("plate_number").and_then(|v| v.as_str()))
            .bind(parse_i32(p.get("odometer_value")))
            .bind(p.get("odometer_unit").and_then(|v| v.as_str()))
            .bind(p.get("notes").and_then(|v| v.as_str()))
            .bind(p.get("parent_id").and_then(|v| v.as_str()))
            .bind(p.get("variant_label").and_then(|v| v.as_str()))
            .bind(p.get("variant_attrs").map(|v| {
                if v.is_string() {
                    v.as_str().unwrap_or("null").to_string()
                } else {
                    v.to_string()
                }
            }))
            .execute(&mut **tx)
            .await?;
        }
        EntityType::AppSetting => {
            let key = p
                .get("key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| pid.clone());
            let value = p
                .get("value")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into());
            sqlx::query(
                r#"
                INSERT INTO app_settings (key, value, updated_at, row_version, field_meta)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    deleted_at = NULL
                "#,
            )
            .bind(key)
            .bind(value)
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .execute(&mut **tx)
            .await?;
        }
        EntityType::DocTemplate => {
            let schema = p
                .get("schema")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "{}".into());
            sqlx::query(
                r#"
                INSERT INTO doc_templates (
                    id, name, kind, page_size, orientation, schema, thumbnail_data_url,
                    is_default, is_archived, created_at, updated_at, row_version, field_meta
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    kind = excluded.kind,
                    page_size = excluded.page_size,
                    orientation = excluded.orientation,
                    schema = excluded.schema,
                    thumbnail_data_url = excluded.thumbnail_data_url,
                    is_default = excluded.is_default,
                    is_archived = excluded.is_archived,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("kind").and_then(|v| v.as_str()).unwrap_or("facture"))
            .bind(p.get("page_size").and_then(|v| v.as_str()).unwrap_or("A4"))
            .bind(
                p.get("orientation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("portrait"),
            )
            .bind(schema)
            .bind(p.get("thumbnail_data_url").and_then(|v| v.as_str()))
            .bind(parse_bool(p.get("is_default"), false) as i32)
            .bind(parse_bool(p.get("is_archived"), false) as i32)
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .execute(&mut **tx)
            .await?;
        }
        EntityType::User => {
            sqlx::query(
                r#"
                INSERT INTO users (
                    id, email, full_name, role, is_active, created_at, updated_at, row_version, field_meta
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
                ON CONFLICT(id) DO UPDATE SET
                    email = excluded.email,
                    full_name = excluded.full_name,
                    role = excluded.role,
                    is_active = excluded.is_active,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("email").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("full_name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("role").and_then(|v| v.as_str()).unwrap_or("employee"))
            .bind(parse_bool(p.get("is_active"), true) as i32)
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .execute(&mut **tx)
            .await?;
        }
        EntityType::Supplier => {
            sqlx::query(
                r#"
                INSERT INTO suppliers (
                    id, name, contact, phone, email, address, notes, legacy_ref,
                    created_at, updated_at, row_version, field_meta
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name, contact = excluded.contact, phone = excluded.phone,
                    email = excluded.email, address = excluded.address, notes = excluded.notes,
                    updated_at = excluded.updated_at, row_version = excluded.row_version,
                    field_meta = excluded.field_meta, deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("contact").and_then(|v| v.as_str()))
            .bind(p.get("phone").and_then(|v| v.as_str()))
            .bind(p.get("email").and_then(|v| v.as_str()))
            .bind(p.get("address").and_then(|v| v.as_str()))
            .bind(p.get("notes").and_then(|v| v.as_str()))
            .bind(p.get("legacy_ref").and_then(|v| v.as_str()))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .execute(&mut **tx)
            .await?;
        }
        EntityType::WorkshopOrder => {
            sqlx::query(
                r#"
                INSERT INTO workshop_orders (
                    id, sale_id, client_id, vehicle_id, received_at, promised_at, label,
                    mileage, fuel_level, planned_hours, notes, status, assigned_to, legacy_ref,
                    created_at, updated_at, row_version, field_meta, source_quote_id
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)
                ON CONFLICT(id) DO UPDATE SET
                    source_quote_id = COALESCE(workshop_orders.source_quote_id, excluded.source_quote_id),
                    sale_id = excluded.sale_id, client_id = excluded.client_id,
                    vehicle_id = excluded.vehicle_id, received_at = excluded.received_at,
                    promised_at = excluded.promised_at, label = excluded.label,
                    mileage = excluded.mileage, fuel_level = excluded.fuel_level,
                    planned_hours = excluded.planned_hours, notes = excluded.notes,
                    status = excluded.status, assigned_to = excluded.assigned_to,
                    updated_at = excluded.updated_at, row_version = excluded.row_version,
                    field_meta = excluded.field_meta, deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("sale_id").and_then(|v| v.as_str()))
            .bind(p.get("client_id").and_then(|v| v.as_str()))
            .bind(p.get("vehicle_id").and_then(|v| v.as_str()))
            .bind(p.get("received_at").and_then(|v| v.as_str()))
            .bind(p.get("promised_at").and_then(|v| v.as_str()))
            .bind(p.get("label").and_then(|v| v.as_str()))
            .bind(parse_i32(p.get("mileage")))
            .bind(p.get("fuel_level").and_then(|v| v.as_str()))
            .bind(p.get("planned_hours").and_then(|v| v.as_f64()))
            .bind(p.get("notes").and_then(|v| v.as_str()))
            .bind(p.get("status").and_then(|v| v.as_str()).unwrap_or("pending"))
            .bind(p.get("assigned_to").and_then(|v| v.as_str()))
            .bind(p.get("legacy_ref").and_then(|v| v.as_str()))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .bind(p.get("source_quote_id").and_then(|v| v.as_str()))
            .execute(&mut **tx)
            .await?;
            replace_workshop_lines(tx, &pid, p, &now).await?;
        }
        EntityType::StockMovement => {
            let inserted = sqlx::query(
                r#"
                INSERT INTO stock_movements (
                    id, product_id, supplier_id, kind, quantity_milli, unit_cost_ht_cents,
                    reason, sale_id, created_at, updated_at, row_version, field_meta
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
                ON CONFLICT(id) DO NOTHING
                "#,
            )
            .bind(&pid)
            .bind(p.get("product_id").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("supplier_id").and_then(|v| v.as_str()))
            .bind(p.get("kind").and_then(|v| v.as_str()).unwrap_or("adjustment"))
            .bind(parse_i64(p.get("quantity_milli")).unwrap_or(0))
            .bind(parse_i64(p.get("unit_cost_ht_cents")))
            .bind(p.get("reason").and_then(|v| v.as_str()))
            .bind(p.get("sale_id").and_then(|v| v.as_str()))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .execute(&mut **tx)
            .await?;
            // A movement this device did not already have is stock that moved
            // elsewhere: apply it here, once. (One it already has is its own, echoed
            // back, and was applied when it was made.) The shelf figure is the sum of
            // the movements, so two PCs selling at the same time both count.
            if apply_stock && inserted.rows_affected() == 1 {
                sqlx::query(
                    "UPDATE products SET stock_milli = stock_milli + $2,
                            updated_at = $3, row_version = row_version + 1
                      WHERE id = $1 AND is_service = 0 AND deleted_at IS NULL",
                )
                .bind(p.get("product_id").and_then(|v| v.as_str()).unwrap_or(""))
                .bind(parse_i64(p.get("quantity_milli")).unwrap_or(0))
                .bind(&now)
                .execute(&mut **tx)
                .await?;
            }
        }
        EntityType::Sale => {
            let was_locked: Option<(Option<String>,)> =
                sqlx::query_as("SELECT locked_at FROM sales WHERE id = $1")
                    .bind(&pid)
                    .fetch_optional(&mut **tx)
                    .await?;
            let was_locked = matches!(was_locked, Some((Some(_),)));
            // A draft's old lines go before the sale row is written: once the row
            // carries its seal, the triggers (rightly) refuse to delete its lines.
            if !was_locked && p.get("lines").is_some_and(|v| v.is_array()) {
                sqlx::query("DELETE FROM sale_lines WHERE sale_id = $1")
                    .bind(&pid)
                    .execute(&mut **tx)
                    .await?;
            }
            // A sale that arrives already sealed must land sealed, otherwise the
            // receiving device would treat a validated invoice as an editable
            // draft. The merge layer refuses later writes to it from then on.
            sqlx::query(
                r#"
                INSERT INTO sales (
                    id, client_id, vehicle_id, reference, status, sale_date,
                    total_ht_cents, total_ttc_cents, payment_method, notes,
                    created_at, updated_at, row_version, field_meta,
                    total_vat_cents, document_kind, fiscal_register_id, fiscal_seq,
                    locked_at, corrects_sale_id, source_quote_id
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)
                ON CONFLICT(id) DO UPDATE SET
                    client_id = excluded.client_id,
                    vehicle_id = excluded.vehicle_id,
                    reference = excluded.reference,
                    status = excluded.status,
                    sale_date = excluded.sale_date,
                    total_ht_cents = excluded.total_ht_cents,
                    total_ttc_cents = excluded.total_ttc_cents,
                    total_vat_cents = excluded.total_vat_cents,
                    document_kind = excluded.document_kind,
                    payment_method = excluded.payment_method,
                    notes = excluded.notes,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    -- Seals only ever get applied, never cleared.
                    fiscal_register_id = COALESCE(sales.fiscal_register_id, excluded.fiscal_register_id),
                    fiscal_seq = COALESCE(sales.fiscal_seq, excluded.fiscal_seq),
                    locked_at = COALESCE(sales.locked_at, excluded.locked_at),
                    corrects_sale_id = COALESCE(sales.corrects_sale_id, excluded.corrects_sale_id),
                    source_quote_id = COALESCE(sales.source_quote_id, excluded.source_quote_id),
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("client_id").and_then(|v| v.as_str()))
            .bind(p.get("vehicle_id").and_then(|v| v.as_str()))
            .bind(p.get("reference").and_then(|v| v.as_str()))
            .bind(p.get("status").and_then(|v| v.as_str()).unwrap_or("invoice"))
            .bind(p.get("sale_date").and_then(|v| v.as_str()))
            .bind(parse_i64(p.get("total_ht_cents")))
            .bind(parse_i64(p.get("total_ttc_cents")))
            .bind(p.get("payment_method").and_then(|v| v.as_str()))
            .bind(p.get("notes").and_then(|v| v.as_str()))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .bind(parse_i64(p.get("total_vat_cents")))
            .bind(p.get("document_kind").and_then(|v| v.as_str()).unwrap_or("invoice"))
            .bind(p.get("fiscal_register_id").and_then(|v| v.as_str()))
            .bind(parse_i64(p.get("fiscal_seq")))
            .bind(p.get("locked_at").and_then(|v| v.as_str()))
            .bind(p.get("corrects_sale_id").and_then(|v| v.as_str()))
            .bind(p.get("source_quote_id").and_then(|v| v.as_str()))
            .execute(&mut **tx)
            .await?;
            apply_embedded_sale_lines(tx, &pid, p, &now).await?;
        }
        EntityType::Payment => {
            sqlx::query(
                r#"
                INSERT INTO payments (
                    id, sale_id, client_id, paid_at, due_at, amount_cents,
                    outstanding_cents, method, bank, cash_register, cheque_drawer,
                    notes, created_at, updated_at, row_version, field_meta,
                    fiscal_register_id, fiscal_seq, reverses_payment_id
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)
                ON CONFLICT(id) DO UPDATE SET
                    sale_id = excluded.sale_id,
                    client_id = excluded.client_id,
                    paid_at = excluded.paid_at,
                    due_at = excluded.due_at,
                    amount_cents = excluded.amount_cents,
                    outstanding_cents = excluded.outstanding_cents,
                    method = excluded.method,
                    bank = excluded.bank,
                    cash_register = excluded.cash_register,
                    cheque_drawer = excluded.cheque_drawer,
                    notes = excluded.notes,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    fiscal_register_id = COALESCE(payments.fiscal_register_id, excluded.fiscal_register_id),
                    fiscal_seq = COALESCE(payments.fiscal_seq, excluded.fiscal_seq),
                    reverses_payment_id = COALESCE(payments.reverses_payment_id, excluded.reverses_payment_id),
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("sale_id").and_then(|v| v.as_str()))
            .bind(p.get("client_id").and_then(|v| v.as_str()))
            .bind(p.get("paid_at").and_then(|v| v.as_str()))
            .bind(p.get("due_at").and_then(|v| v.as_str()))
            .bind(parse_i64(p.get("amount_cents")).unwrap_or(0))
            .bind(parse_i64(p.get("outstanding_cents")).unwrap_or(0))
            .bind(p.get("method").and_then(|v| v.as_str()))
            .bind(p.get("bank").and_then(|v| v.as_str()))
            .bind(p.get("cash_register").and_then(|v| v.as_str()))
            .bind(p.get("cheque_drawer").and_then(|v| v.as_str()))
            .bind(p.get("notes").and_then(|v| v.as_str()))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .bind(p.get("fiscal_register_id").and_then(|v| v.as_str()))
            .bind(parse_i64(p.get("fiscal_seq")))
            .bind(p.get("reverses_payment_id").and_then(|v| v.as_str()))
            .execute(&mut **tx)
            .await?;
        }
        EntityType::FiscalEntry => {
            // Insert or nothing. A replicated journal row is accepted once and
            // is thereafter as immutable here as it is on the register that
            // wrote it; the merge layer has already refused any update.
            ensure_replica_register(tx, p).await?;
            sqlx::query(
                r#"
                INSERT INTO fiscal_journal (
                    id, register_id, seq, event_type, occurred_at, business_day,
                    payload, payload_hash, prev_hash, chain_hash, signature,
                    document_ref, sale_id, total_ttc_cents, actor_id, company_id
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
                ON CONFLICT(id) DO NOTHING
                "#,
            )
            .bind(&pid)
            .bind(p.get("register_id").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(parse_i64(p.get("seq")).unwrap_or(0))
            .bind(p.get("event_type").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(
                p.get("occurred_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&now),
            )
            .bind(p.get("business_day").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(json_text(p.get("payload")))
            .bind(p.get("payload_hash").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("prev_hash").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("chain_hash").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("signature").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(p.get("document_ref").and_then(|v| v.as_str()))
            .bind(p.get("sale_id").and_then(|v| v.as_str()))
            .bind(parse_i64(p.get("total_ttc_cents")).unwrap_or(0))
            .bind(p.get("actor_id").and_then(|v| v.as_str()))
            .bind(p.get("company_id").and_then(|v| v.as_str()))
            .execute(&mut **tx)
            .await?;
            adopt_declared_key(tx, p).await?;
        }
        EntityType::SaleLine => {
            sqlx::query(
                r#"
                INSERT INTO sale_lines (
                    id, sale_id, product_id, label, quantity_milli,
                    unit_price_ht_cents, unit_price_ttc_cents, vat_rate_bp,
                    discount_bp, line_total_ttc_cents, position,
                    created_at, updated_at, row_version, field_meta, price_basis
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
                ON CONFLICT(id) DO UPDATE SET
                    price_basis = excluded.price_basis,
                    sale_id = excluded.sale_id,
                    product_id = excluded.product_id,
                    label = excluded.label,
                    quantity_milli = excluded.quantity_milli,
                    unit_price_ht_cents = excluded.unit_price_ht_cents,
                    unit_price_ttc_cents = excluded.unit_price_ttc_cents,
                    vat_rate_bp = excluded.vat_rate_bp,
                    discount_bp = excluded.discount_bp,
                    line_total_ttc_cents = excluded.line_total_ttc_cents,
                    position = excluded.position,
                    updated_at = excluded.updated_at,
                    row_version = excluded.row_version,
                    field_meta = excluded.field_meta,
                    deleted_at = NULL
                "#,
            )
            .bind(&pid)
            .bind(p.get("sale_id").and_then(|v| v.as_str()))
            .bind(p.get("product_id").and_then(|v| v.as_str()))
            .bind(p.get("label").and_then(|v| v.as_str()))
            .bind(parse_i64(p.get("quantity_milli")).unwrap_or(1000))
            .bind(parse_i64(p.get("unit_price_ht_cents")))
            .bind(parse_i64(p.get("unit_price_ttc_cents")))
            .bind(parse_i32(p.get("vat_rate_bp")).unwrap_or(2000))
            .bind(parse_i32(p.get("discount_bp")).unwrap_or(0))
            .bind(parse_i64(p.get("line_total_ttc_cents")))
            .bind(parse_i32(p.get("position")).unwrap_or(0))
            .bind(p.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now))
            .bind(&updated_at)
            .bind(snap.row_version)
            .bind(&field_meta)
            .bind(p.get("price_basis").and_then(|v| v.as_str()).unwrap_or("ht"))
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

/// A journal row belongs to a register, and the register has to exist on this
/// device before the row can be stored. Another PC's register was never sent as a
/// record of its own, so its journal could not be stored here at all (the row
/// pointed at a register this device had never heard of).
///
/// Every chain begins with a `register_opened` entry that says who the register is,
/// including the public key its signatures are checked against, so the replica is
/// built from that. It is never invented from anything else: a register with a
/// made-up key would make every signature look forged.
async fn ensure_replica_register(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    p: &serde_json::Value,
) -> LocalResult<()> {
    let Some(register_id) = p
        .get("register_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        return Ok(());
    };
    let known: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM fiscal_registers WHERE id = $1")
        .bind(register_id)
        .fetch_optional(&mut **tx)
        .await?;
    if known.is_some() || p.get("event_type").and_then(|v| v.as_str()) != Some("register_opened") {
        return Ok(());
    }

    let inner: serde_json::Value = match p.get("payload") {
        Some(serde_json::Value::String(s)) => serde_json::from_str(s).unwrap_or_default(),
        Some(other) => other.clone(),
        None => return Ok(()),
    };
    let text = |key: &str| inner.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let (Some(code), Some(public_key)) = (text("register_code"), text("public_key")) else {
        return Ok(());
    };
    sqlx::query(
        r#"
        INSERT INTO fiscal_registers (
            id, company_id, code, label, device_id, public_key, key_store,
            software_version, opened_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(register_id)
    .bind(p.get("company_id").and_then(|v| v.as_str()))
    .bind(code)
    .bind(format!("Caisse {code}"))
    .bind(text("device_id"))
    .bind(public_key)
    .bind(text("key_store").unwrap_or("keychain"))
    .bind(text("software_version").unwrap_or("?"))
    .bind(
        p.get("occurred_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// A register whose key was replaced says so in its journal. The copy of that register
/// held here has to follow, or the entries signed with the new key would fail against
/// the old one.
async fn adopt_declared_key(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    p: &serde_json::Value,
) -> LocalResult<()> {
    if p.get("event_type").and_then(|v| v.as_str()) != Some("register_key_changed") {
        return Ok(());
    }
    let inner: serde_json::Value = match p.get("payload") {
        Some(serde_json::Value::String(s)) => serde_json::from_str(s).unwrap_or_default(),
        Some(other) => other.clone(),
        None => return Ok(()),
    };
    let (Some(register_id), Some(key)) = (
        p.get("register_id").and_then(|v| v.as_str()),
        inner.get("public_key").and_then(|v| v.as_str()).filter(|k| !k.is_empty()),
    ) else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE fiscal_registers SET public_key = $2, key_store = COALESCE($3, key_store) WHERE id = $1",
    )
    .bind(register_id)
    .bind(key)
    .bind(inner.get("key_store").and_then(|v| v.as_str()))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// A sale travels with its lines (they are part of the document), but a receiver
/// used to write the sale row and drop them: the other PC showed an invoice with a
/// total and nothing on it. Lines are applied here.
///
/// - A draft or quote is replaced by exactly the lines it was sent with (its old
///   lines were removed before the sale row was written).
/// - A sealed document's lines are immutable; they are only ever added, never
///   changed or removed, which is what the database triggers enforce as well.
async fn apply_embedded_sale_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    sale_id: &str,
    payload: &serde_json::Value,
    now: &str,
) -> LocalResult<()> {
    let Some(lines) = payload.get("lines").and_then(|v| v.as_array()) else {
        return Ok(());
    };
    for (index, line) in lines.iter().enumerate() {
        let Some(line_id) = line.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        sqlx::query(
            r#"
            INSERT INTO sale_lines (
                id, sale_id, product_id, label, quantity_milli,
                unit_price_ht_cents, unit_price_ttc_cents, vat_rate_bp,
                discount_bp, line_total_ttc_cents, position, price_basis,
                created_at, updated_at, row_version, field_meta
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13,1,'{}')
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(line_id)
        .bind(sale_id)
        .bind(line.get("product_id").and_then(|v| v.as_str()))
        .bind(line.get("label").and_then(|v| v.as_str()))
        .bind(parse_i64(line.get("quantity_milli")).unwrap_or(1000))
        .bind(parse_i64(line.get("unit_price_ht_cents")))
        .bind(parse_i64(line.get("unit_price_ttc_cents")))
        .bind(parse_i32(line.get("vat_rate_bp")).unwrap_or(2000))
        .bind(parse_i32(line.get("discount_bp")).unwrap_or(0))
        .bind(parse_i64(line.get("line_total_ttc_cents")))
        .bind(parse_i32(line.get("position")).unwrap_or(index as i32))
        .bind(line.get("price_basis").and_then(|v| v.as_str()).unwrap_or("ht"))
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(crate) async fn soft_delete_entity(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    entity_type: motodesk_core::entity::EntityType,
    entity_id: Uuid,
    at: chrono::DateTime<chrono::Utc>,
) -> LocalResult<()> {
    use motodesk_core::entity::EntityType;
    let (table, pk) = match entity_type {
        EntityType::Client => ("clients", "id"),
        EntityType::Vehicle => ("vehicles", "id"),
        EntityType::Category => ("categories", "id"),
        EntityType::Product => ("products", "id"),
        EntityType::AppSetting => ("app_settings", "key"),
        EntityType::DocTemplate => ("doc_templates", "id"),
        EntityType::User => ("users", "id"),
        EntityType::Sale => ("sales", "id"),
        EntityType::SaleLine => ("sale_lines", "id"),
        EntityType::Payment => ("payments", "id"),
        EntityType::Supplier => ("suppliers", "id"),
        EntityType::WorkshopOrder => ("workshop_orders", "id"),
        EntityType::StockMovement => ("stock_movements", "id"),
        // Journal rows have no deleted_at and a trigger that refuses deletion.
        // Reaching here would mean the merge guard was bypassed.
        EntityType::FiscalEntry => return Ok(()),
    };
    // A sealed document stays visible: the way to withdraw one is a credit note.
    let sql = if matches!(entity_type, EntityType::Sale) {
        format!("UPDATE {table} SET deleted_at = $1 WHERE {pk} = $2 AND locked_at IS NULL")
    } else {
        format!("UPDATE {table} SET deleted_at = $1 WHERE {pk} = $2")
    };
    sqlx::query(&sql)
        .bind(at.to_rfc3339())
        .bind(entity_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Serialise a nested payload value back to the text SQLite stores.
fn json_text(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "{}".to_string(),
    }
}

async fn attach_workshop_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    order_id: &str,
    payload: &mut serde_json::Value,
) -> LocalResult<()> {
    #[derive(sqlx::FromRow)]
    struct LineSnap {
        id: String,
        product_id: Option<String>,
        label: Option<String>,
        quantity_milli: i64,
        unit_price_ht_cents: i64,
        vat_rate_bp: i64,
        discount_bp: i64,
        position: i64,
        unit_price_ttc_cents: Option<i64>,
    }
    let rows: Vec<LineSnap> = sqlx::query_as(
        r#"
        SELECT id, product_id, label, quantity_milli, unit_price_ht_cents,
               vat_rate_bp, discount_bp, position, unit_price_ttc_cents
        FROM workshop_order_lines
        WHERE workshop_order_id = $1
        ORDER BY position ASC
        "#,
    )
    .bind(order_id)
    .fetch_all(&mut **tx)
    .await?;
    let lines: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "product_id": r.product_id,
                "label": r.label,
                "quantity_milli": r.quantity_milli,
                "unit_price_ht_cents": r.unit_price_ht_cents,
                "unit_price_ttc_cents": r.unit_price_ttc_cents,
                "vat_rate_bp": r.vat_rate_bp,
                "discount_bp": r.discount_bp,
                "position": r.position,
            })
        })
        .collect();
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("lines".into(), json!(lines));
    }
    Ok(())
}

async fn replace_workshop_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    order_id: &str,
    payload: &serde_json::Value,
    now: &str,
) -> LocalResult<()> {
    let Some(arr) = payload.get("lines").and_then(|v| v.as_array()) else {
        return Ok(());
    };
    sqlx::query("DELETE FROM workshop_order_lines WHERE workshop_order_id = $1")
        .bind(order_id)
        .execute(&mut **tx)
        .await?;
    for (i, line) in arr.iter().enumerate() {
        let id = line
            .get("id")
            .and_then(|v| v.as_str())
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
        .bind(line.get("product_id").and_then(|v| v.as_str()))
        .bind(line.get("label").and_then(|v| v.as_str()).unwrap_or(""))
        .bind(parse_i64(line.get("quantity_milli")).unwrap_or(1_000))
        .bind(parse_i64(line.get("unit_price_ht_cents")).unwrap_or(0))
        .bind(parse_i64(line.get("vat_rate_bp")).unwrap_or(2_000))
        .bind(parse_i64(line.get("discount_bp")).unwrap_or(0))
        .bind(i as i64)
        .bind(now)
        .bind(parse_i64(line.get("unit_price_ttc_cents")))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// A product's stock in thousandths of a unit, from a payload that carries either that
/// (`stock_milli`) or the older whole-unit figure (`stock_quantity`, which a device
/// that has not been updated still sends).
pub(crate) fn stock_milli_of(p: &serde_json::Value) -> i64 {
    parse_i64(p.get("stock_milli")).unwrap_or_else(|| {
        p.get("stock_quantity")
            .and_then(|v| v.as_f64())
            .map(|units| (units * 1000.0).round() as i64)
            .unwrap_or(0)
    })
}

fn parse_i32(v: Option<&serde_json::Value>) -> Option<i32> {
    let v = v?;
    if let Some(n) = v.as_i64() {
        return Some(n as i32);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<i32>().ok();
    }
    None
}

fn parse_i64(v: Option<&serde_json::Value>) -> Option<i64> {
    let v = v?;
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<i64>().ok();
    }
    None
}

fn parse_bool(v: Option<&serde_json::Value>, default: bool) -> bool {
    let Some(v) = v else { return default };
    if let Some(b) = v.as_bool() {
        return b;
    }
    if let Some(n) = v.as_i64() {
        return n != 0;
    }
    if let Some(s) = v.as_str() {
        return matches!(s, "true" | "1" | "yes" | "on");
    }
    default
}
