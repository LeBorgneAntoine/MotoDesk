use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{EntitySnapshot, FieldMetaMap};
use serde_json::Value;
use sqlx::{Acquire, SqlitePool};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::local_server::error::LocalResult;
use crate::local_server::mesh::persist::persist_snapshot_with;
use crate::local_server::state::SharedState;

/// Process-wide guard so the startup spawn and the post-login refresh
/// never run two concurrent bootstraps against the same SQLite file.
fn bootstrap_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Default, serde::Serialize)]
pub struct BootstrapResult {
    pub imported: bool,
    pub clients: u32,
    pub vehicles: u32,
    pub products: u32,
    pub categories: u32,
    pub templates: u32,
    pub sales: u32,
    pub sale_lines: u32,
    pub payments: u32,
    pub suppliers: u32,
    pub workshop_orders: u32,
    pub stock_movements: u32,
    pub journal_entries: u32,
    /// Rows the cloud sent that this device could not store. They are logged; the
    /// rest of the import goes ahead rather than being thrown away with them.
    pub skipped: u32,
}

pub async fn needs_bootstrap(pool: &SqlitePool) -> LocalResult<bool> {
    let bootstrapped: Option<(Option<String>,)> =
        sqlx::query_as("SELECT cloud_bootstrapped_at FROM sync_state WHERE id = 1")
            .fetch_optional(pool)
            .await?;

    if bootstrapped
        .and_then(|(t,)| t)
        .filter(|s| !s.is_empty())
        .is_some()
    {
        return Ok(false);
    }

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clients WHERE deleted_at IS NULL")
        .fetch_one(pool)
        .await?;

    Ok(count == 0)
}

pub async fn bootstrap_from_cloud(state: &SharedState) -> LocalResult<BootstrapResult> {
    bootstrap_from_cloud_inner(state, false).await
}

/// Re-import all shop entities from cloud (overwrites local mirror).
pub async fn force_bootstrap_from_cloud(state: &SharedState) -> LocalResult<BootstrapResult> {
    sqlx::query("UPDATE sync_state SET cloud_bootstrapped_at = NULL WHERE id = 1")
        .execute(&state.db)
        .await?;
    bootstrap_from_cloud_inner(state, true).await
}

async fn bootstrap_from_cloud_inner(
    state: &SharedState,
    force: bool,
) -> LocalResult<BootstrapResult> {
    let _guard = bootstrap_lock().lock().await;

    if !force && !needs_bootstrap(&state.db).await? {
        let (c,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clients WHERE deleted_at IS NULL")
            .fetch_one(&state.db)
            .await?;
        return Ok(BootstrapResult {
            clients: c as u32,
            ..Default::default()
        });
    }

    let token = crate::local_server::secrets::session_access_token(&state.db).await?;
    let Some(token) = token else {
        tracing::warn!("bootstrap skipped: no session token");
        return Ok(BootstrapResult::default());
    };

    let base = crate::local_server::cloud::cloud_base_url(state).await;
    let url = format!("{base}/api/sync/export");
    tracing::info!(%url, "fetching cloud export for local bootstrap");

    let res = state
        .http
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| crate::local_server::error::LocalError::Internal(anyhow::anyhow!(e)))?;

    if !res.status().is_success() {
        tracing::warn!(status = %res.status(), "cloud export failed");
        return Ok(BootstrapResult::default());
    }

    let data: Value = res
        .json()
        .await
        .map_err(|e| crate::local_server::error::LocalError::Internal(anyhow::anyhow!(e)))?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let result = import_export(&mut tx, &data).await?;

    sqlx::query("UPDATE sync_state SET cloud_bootstrapped_at = $1 WHERE id = 1")
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    tracing::info!(
        clients = result.clients,
        vehicles = result.vehicles,
        categories = result.categories,
        products = result.products,
        templates = result.templates,
        sales = result.sales,
        sale_lines = result.sale_lines,
        payments = result.payments,
        suppliers = result.suppliers,
        workshop_orders = result.workshop_orders,
        stock_movements = result.stock_movements,
        journal_entries = result.journal_entries,
        skipped = result.skipped,
        "cloud bootstrap complete"
    );

    Ok(result)
}

/// Write what the cloud exported into the local database.
///
/// Every row goes through the same code a change arriving from another device
/// goes through (`persist_snapshot_with`), so a rebuilt device holds the same
/// columns a synced one does. This used to be a second, shorter copy of each
/// insert, which quietly dropped fiscal seals, credit limits, product variants,
/// pricing basis and everything below.
///
/// Order matters for the foreign keys: people and catalogue first, then
/// documents, then what hangs off them.
pub(crate) async fn import_export(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    data: &Value,
) -> LocalResult<BootstrapResult> {
    let mut r = BootstrapResult {
        imported: true,
        ..Default::default()
    };

    r.clients = import_rows(tx, data, "clients", EntityType::Client, &mut r.skipped).await?;
    r.vehicles = import_rows(tx, data, "vehicles", EntityType::Vehicle, &mut r.skipped).await?;
    r.categories = import_rows(tx, data, "categories", EntityType::Category, &mut r.skipped).await?;
    r.products = import_rows(tx, data, "products", EntityType::Product, &mut r.skipped).await?;

    if let Some(settings) = data.get("settings").and_then(|v| v.as_object()) {
        for (key, value) in settings {
            sqlx::query(
                r#"
                INSERT INTO app_settings (key, value, updated_at) VALUES ($1, $2, $3)
                ON CONFLICT(key) DO UPDATE SET value = $2, updated_at = $3, deleted_at = NULL
                "#,
            )
            .bind(key)
            .bind(value.to_string())
            .bind(Utc::now().to_rfc3339())
            .execute(&mut **tx)
            .await?;
        }
    }

    r.templates = import_rows(tx, data, "templates", EntityType::DocTemplate, &mut r.skipped).await?;
    r.suppliers = import_rows(tx, data, "suppliers", EntityType::Supplier, &mut r.skipped).await?;
    r.sales = import_rows(tx, data, "sales", EntityType::Sale, &mut r.skipped).await?;
    r.sale_lines = import_rows(tx, data, "sale_lines", EntityType::SaleLine, &mut r.skipped).await?;
    r.payments = import_rows(tx, data, "payments", EntityType::Payment, &mut r.skipped).await?;
    r.workshop_orders =
        import_rows(tx, data, "workshop_orders", EntityType::WorkshopOrder, &mut r.skipped).await?;
    r.stock_movements =
        import_rows(tx, data, "stock_movements", EntityType::StockMovement, &mut r.skipped).await?;
    r.journal_entries =
        import_rows(tx, data, "fiscal_journal", EntityType::FiscalEntry, &mut r.skipped).await?;

    Ok(r)
}

async fn import_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    data: &Value,
    key: &str,
    entity_type: EntityType,
    skipped: &mut u32,
) -> LocalResult<u32> {
    let mut rows: Vec<&Value> = data
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    if entity_type == EntityType::Product {
        // A variant points at its parent product.
        rows.sort_by_key(|r| r.get("parent_id").is_some_and(|v| !v.is_null()));
    }

    let mut imported = 0u32;
    for row in rows {
        let Some(snap) = snapshot_of(entity_type, row) else {
            *skipped += 1;
            continue;
        };
        match import_one(tx, &snap).await {
            Ok(true) => imported += 1,
            // Already here and sealed: the local copy is the legal one.
            Ok(false) => {}
            Err(e) => {
                #[cfg(test)]
                eprintln!("skipped {} {}: {e}", entity_type.as_str(), snap.entity_id);
                tracing::warn!(
                    entity = %entity_type.as_str(),
                    id = %snap.entity_id,
                    error = %e,
                    "bootstrap: a row could not be stored and was skipped"
                );
                *skipped += 1;
            }
        }
    }
    Ok(imported)
}

/// A row as the cloud exported it, in the shape the merge layer works with.
fn snapshot_of(entity_type: EntityType, row: &Value) -> Option<EntitySnapshot> {
    let id = row
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())?;
    let mut payload = row.clone();
    let field_meta: FieldMetaMap = payload
        .as_object_mut()
        .and_then(|o| o.remove("field_meta"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let updated_at = row
        .get("updated_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    Some(EntitySnapshot {
        entity_type,
        entity_id: id,
        row_version: row.get("row_version").and_then(|v| v.as_i64()).unwrap_or(1),
        deleted_at: None,
        updated_at,
        payload,
        field_meta,
    })
}

/// Store one row. `Ok(false)` means it was left alone.
///
/// Each row is stored under its own savepoint, so one that cannot be (a document
/// pointing at a customer the cloud has since deleted, say) does not undo the
/// thousands that can. It is first retried without its links to other records,
/// because a sealed invoice is worth more kept than dropped.
async fn import_one(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    snap: &EntitySnapshot,
) -> LocalResult<bool> {
    if local_copy_is_sealed(tx, snap).await? {
        return Ok(false);
    }

    let mut last_error = None;
    for attempt in 0..2 {
        let candidate = if attempt == 0 {
            snap.clone()
        } else {
            detached(snap)
        };
        let mut sp = tx.begin().await?;
        let outcome: LocalResult<()> = async {
            evict_unique_clashes(&mut sp, &candidate).await?;
            persist_snapshot_with(&mut sp, &candidate, false).await?;
            after_import(&mut sp, &candidate).await
        }
        .await;
        match outcome {
            Ok(()) => {
                sp.commit().await?;
                return Ok(true);
            }
            Err(e) => {
                sp.rollback().await?;
                last_error = Some(e);
            }
        }
    }
    Err(last_error.expect("two attempts were made"))
}

/// What the device already holds and must not be rewritten: a sealed document,
/// the lines of one, a payment that is in the journal.
async fn local_copy_is_sealed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    snap: &EntitySnapshot,
) -> LocalResult<bool> {
    let id = snap.entity_id.to_string();
    let sql = match snap.entity_type {
        EntityType::Sale => "SELECT 1 FROM sales WHERE id = $1 AND locked_at IS NOT NULL",
        EntityType::SaleLine => {
            "SELECT 1 FROM sale_lines l JOIN sales s ON s.id = l.sale_id
              WHERE l.id = $1 AND s.locked_at IS NOT NULL"
        }
        EntityType::Payment => "SELECT 1 FROM payments WHERE id = $1 AND fiscal_seq IS NOT NULL",
        _ => return Ok(false),
    };
    let found: Option<(i64,)> = sqlx::query_as(sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(found.is_some())
}

/// The same row without its references to other records.
fn detached(snap: &EntitySnapshot) -> EntitySnapshot {
    const LINKS: [&str; 6] = [
        "client_id",
        "vehicle_id",
        "product_id",
        "supplier_id",
        "category_id",
        "parent_id",
    ];
    let mut copy = snap.clone();
    if let Some(obj) = copy.payload.as_object_mut() {
        for key in LINKS {
            if obj.contains_key(key) {
                obj.insert(key.to_string(), Value::Null);
            }
        }
        if let Some(lines) = obj.get_mut("lines").and_then(|v| v.as_array_mut()) {
            for line in lines {
                if let Some(l) = line.as_object_mut() {
                    l.insert("product_id".into(), Value::Null);
                }
            }
        }
    }
    copy
}

/// The cloud is authoritative for a mirror, so a local row that holds the same
/// unique value under another id gives way (this device seeded its own default
/// categories with ids the cloud never heard of).
async fn evict_unique_clashes(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    snap: &EntitySnapshot,
) -> LocalResult<()> {
    let id = snap.entity_id.to_string();
    let p = &snap.payload;
    let text = |key: &str| {
        p.get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    match snap.entity_type {
        EntityType::Vehicle => {
            if let Some(vin) = text("vin_or_serial") {
                evict_conflict(tx, "vehicles", "vin_or_serial", &vin, &id).await?;
            }
        }
        EntityType::Category => {
            if let Some(name) = text("name") {
                evict_conflict(tx, "categories", "name", &name, &id).await?;
            }
            if let Some(slug) = text("slug") {
                evict_conflict(tx, "categories", "slug", &slug, &id).await?;
            }
        }
        EntityType::Product => {
            if let Some(sku) = text("sku") {
                evict_conflict(tx, "products", "sku", &sku, &id).await?;
            }
            if let Some(vin) = text("vin_or_serial") {
                evict_conflict(tx, "products", "vin_or_serial", &vin, &id).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Things the shared row writer deliberately does not do for a bootstrap.
async fn after_import(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    snap: &EntitySnapshot,
) -> LocalResult<()> {
    if snap.entity_type == EntityType::Product {
        // A product edit never overwrites the shelf figure on a synced device (the
        // stock is the sum of the movements). For a mirror, the cloud's figure is
        // that sum, so it is taken as it is.
        let stock = crate::local_server::mesh::persist::stock_milli_of(&snap.payload);
        sqlx::query("UPDATE products SET stock_milli = $2 WHERE id = $1")
            .bind(snap.entity_id.to_string())
            .bind(stock)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Delete any row that holds the given UNIQUE secondary value but has a
/// different primary id. Lets us safely import authoritative cloud rows on
/// top of locally seeded rows whose ids were generated independently.
async fn evict_conflict(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    column: &str,
    value: &str,
    new_id: &str,
) -> LocalResult<()> {
    // `table`/`column` are hard-coded call-site literals, never user input,
    // so it's safe to interpolate them into the statement.
    let sql = format!(
        "DELETE FROM {table} WHERE {column} = $1 AND id <> $2",
        table = table,
        column = column,
    );
    sqlx::query(&sql)
        .bind(value)
        .bind(new_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::db;
    use serde_json::json;

    async fn blank_device() -> SqlitePool {
        let dir = std::env::temp_dir().join(format!("motodesk_boot_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = db::init_pool(&dir.join("local.db")).await.unwrap();
        db::run_migrations(&pool).await.unwrap();
        pool
    }

    /// What `/api/sync/export` returns for a small shop, as Postgres renders it.
    fn export() -> (Value, Ids) {
        let ids = Ids::new();
        let at = "2026-11-10T09:30:00.123456+00:00";
        let data = json!({
            "clients": [{
                "id": ids.client, "first_name": "Jean", "last_name": "Dupont", "phone": "0600000000",
                "credit_limit_cents": 50_000, "credit_blocked": true,
                "created_at": at, "updated_at": at, "row_version": 3, "field_meta": {}
            }],
            "vehicles": [], "categories": [],
            "products": [{
                "id": ids.product, "name": "Filtre à huile", "sku": "FH-1", "unit": "piece",
                "is_service": false, "selling_price_ttc_cents": 999, "vat_rate_bp": 2000,
                "stock_milli": 7_500, "is_active": true,
                "created_at": at, "updated_at": at, "row_version": 1, "field_meta": {}
            }],
            "settings": {}, "templates": [],
            "suppliers": [{
                "id": ids.supplier, "name": "Motul France", "created_at": at, "updated_at": at,
                "row_version": 1, "field_meta": {}
            }],
            "sales": [{
                "id": ids.invoice, "client_id": ids.client, "reference": "FA-2026-C1-00001",
                "status": "invoice", "document_kind": "invoice", "sale_date": "2026-11-10",
                "total_ht_cents": 1665, "total_vat_cents": 333, "total_ttc_cents": 1998,
                "fiscal_register_id": "C1", "fiscal_seq": 4, "locked_at": at,
                "source_quote_id": ids.quote,
                "created_at": at, "updated_at": at, "row_version": 2, "field_meta": {}
            }, {
                "id": ids.quote, "client_id": ids.client, "reference": "DE-2026-C1-00001",
                "status": "quote", "document_kind": "invoice", "total_ttc_cents": 1998,
                "created_at": at, "updated_at": at, "row_version": 1, "field_meta": {}
            }],
            "sale_lines": [{
                "id": ids.line, "sale_id": ids.invoice, "product_id": ids.product, "label": "Filtre à huile",
                "quantity_milli": 2_000, "unit_price_ht_cents": 833, "unit_price_ttc_cents": 999,
                "price_basis": "ttc", "vat_rate_bp": 2000, "discount_bp": 0,
                "line_total_ttc_cents": 1998, "position": 0,
                "created_at": at, "updated_at": at, "row_version": 1, "field_meta": {}
            }],
            "payments": [{
                "id": ids.payment, "sale_id": ids.invoice, "client_id": ids.client,
                "paid_at": "2026-11-10", "amount_cents": 1998, "outstanding_cents": 0,
                "method": "card", "fiscal_register_id": "C1", "fiscal_seq": 5,
                "created_at": at, "updated_at": at, "row_version": 1, "field_meta": {}
            }],
            "workshop_orders": [{
                "id": ids.order, "client_id": ids.client, "label": "Devis DE-2026-C1-00001",
                "status": "pending", "source_quote_id": ids.quote,
                "created_at": at, "updated_at": at, "row_version": 1, "field_meta": {},
                "lines": [{
                    "id": ids.order_line, "product_id": ids.product, "label": "Filtre à huile",
                    "quantity_milli": 2_000, "unit_price_ht_cents": 833, "unit_price_ttc_cents": 999,
                    "vat_rate_bp": 2000, "discount_bp": 0, "position": 0
                }]
            }],
            "stock_movements": [{
                "id": ids.movement, "product_id": ids.product, "kind": "sale",
                "quantity_milli": -2_000, "sale_id": ids.invoice,
                "created_at": at, "updated_at": at, "row_version": 1, "field_meta": {}
            }],
            "fiscal_journal": [{
                // Every chain opens with the entry that says who the register is.
                "id": ids.opened, "company_id": ids.company, "register_id": ids.register, "seq": 1,
                "event_type": "register_opened", "occurred_at": at, "business_day": "2026-11-10",
                "payload": {"register_code": "C1", "public_key": "ab12", "key_store": "keychain",
                            "device_id": Uuid::new_v4().to_string(), "software_version": "test"},
                "payload_hash": "a0", "prev_hash": "00", "chain_hash": "a1", "signature": "a2",
                "total_ttc_cents": 0, "received_at": at
            }, {
                "id": ids.journal, "company_id": ids.company, "register_id": ids.register, "seq": 4,
                "event_type": "document_sealed", "occurred_at": at, "business_day": "2026-11-10",
                "payload": {"reference": "FA-2026-C1-00001"},
                "payload_hash": "aa", "prev_hash": "bb", "chain_hash": "cc", "signature": "dd",
                "document_ref": "FA-2026-C1-00001", "sale_id": ids.invoice, "total_ttc_cents": 1998,
                "received_at": at
            }],
        });
        (data, ids)
    }

    struct Ids {
        client: String,
        product: String,
        supplier: String,
        invoice: String,
        quote: String,
        line: String,
        payment: String,
        order: String,
        order_line: String,
        movement: String,
        journal: String,
        opened: String,
        register: String,
        company: String,
    }

    impl Ids {
        fn new() -> Self {
            let id = || Uuid::new_v4().to_string();
            Ids {
                client: id(),
                product: id(),
                supplier: id(),
                invoice: id(),
                quote: id(),
                line: id(),
                payment: id(),
                order: id(),
                order_line: id(),
                movement: id(),
                journal: id(),
                opened: id(),
                register: id(),
                company: id(),
            }
        }
    }

    async fn load(pool: &SqlitePool, data: &Value) -> BootstrapResult {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let result = import_export(&mut tx, data).await.unwrap();
        tx.commit().await.unwrap();
        result
    }

    async fn scalar<T>(pool: &SqlitePool, sql: &str, id: &str) -> T
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
    {
        sqlx::query_scalar(sql).bind(id).fetch_one(pool).await.unwrap()
    }

    #[tokio::test]
    async fn a_new_device_gets_everything_the_cloud_holds() {
        let pool = blank_device().await;
        let (data, ids) = export();
        let r = load(&pool, &data).await;

        assert_eq!(r.skipped, 0);
        assert_eq!(
            (r.clients, r.products, r.suppliers, r.sales, r.sale_lines),
            (1, 1, 1, 2, 1)
        );
        assert_eq!(
            (r.payments, r.workshop_orders, r.stock_movements, r.journal_entries),
            (1, 1, 1, 2)
        );

        // Customer fields the old import dropped.
        let limit: Option<i64> = scalar(&pool, "SELECT credit_limit_cents FROM clients WHERE id = $1", &ids.client).await;
        assert_eq!(limit, Some(50_000));

        // The invoice arrives sealed, with its number, register and link to the quote.
        let (locked, seq, kind, source): (Option<String>, Option<i64>, String, Option<String>) = sqlx::query_as(
            "SELECT locked_at, fiscal_seq, document_kind, source_quote_id FROM sales WHERE id = $1",
        )
        .bind(&ids.invoice)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(locked.is_some(), "a sealed invoice must not turn into an editable draft");
        assert_eq!((seq, kind.as_str()), (Some(4), "invoice"));
        assert_eq!(source.as_deref(), Some(ids.quote.as_str()));

        // The line keeps how it was priced.
        let basis: String = scalar(&pool, "SELECT price_basis FROM sale_lines WHERE id = $1", &ids.line).await;
        assert_eq!(basis, "ttc");

        // The rest of the shop.
        let paid: i64 = scalar(&pool, "SELECT amount_cents FROM payments WHERE id = $1", &ids.payment).await;
        assert_eq!(paid, 1998);
        let (label, quote): (String, Option<String>) =
            sqlx::query_as("SELECT label, source_quote_id FROM workshop_orders WHERE id = $1")
                .bind(&ids.order)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(label.starts_with("Devis"));
        assert_eq!(quote.as_deref(), Some(ids.quote.as_str()));
        let ttc: Option<i64> = scalar(&pool, "SELECT unit_price_ttc_cents FROM workshop_order_lines WHERE id = $1", &ids.order_line).await;
        assert_eq!(ttc, Some(999), "a repair-order part keeps its shelf price");
        let entries: i64 = scalar(&pool, "SELECT COUNT(*) FROM fiscal_journal WHERE id = $1", &ids.journal).await;
        assert_eq!(entries, 1);
        // The register the journal belongs to is rebuilt from its opening entry, with
        // the key its signatures are checked against.
        let (code, key): (String, String) =
            sqlx::query_as("SELECT code, public_key FROM fiscal_registers WHERE id = $1")
                .bind(&ids.register)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((code.as_str(), key.as_str()), ("C1", "ab12"));
        let supplier: String = scalar(&pool, "SELECT name FROM suppliers WHERE id = $1", &ids.supplier).await;
        assert_eq!(supplier, "Motul France");
    }

    /// The cloud sends the stock already worked out and the movements that made it.
    /// Applying both would take 2 pieces off a second time.
    #[tokio::test]
    async fn movements_are_kept_without_moving_the_stock_again() {
        let pool = blank_device().await;
        let (data, ids) = export();
        load(&pool, &data).await;

        let stock: i64 = scalar(&pool, "SELECT stock_milli FROM products WHERE id = $1", &ids.product).await;
        assert_eq!(stock, 7_500, "the cloud's figure (7,5), not 5,5");
        let moved: i64 = scalar(&pool, "SELECT quantity_milli FROM stock_movements WHERE id = $1", &ids.movement).await;
        assert_eq!(moved, -2_000, "the history is still there");
    }

    /// Importing again (a forced refresh) neither fails on the sealed documents it
    /// already holds nor duplicates anything.
    #[tokio::test]
    async fn importing_twice_changes_nothing() {
        let pool = blank_device().await;
        let (data, ids) = export();
        load(&pool, &data).await;
        let second = load(&pool, &data).await;

        assert_eq!(second.skipped, 0);
        for (table, expected) in [
            ("sales", 2i64),
            ("sale_lines", 1),
            ("payments", 1),
            ("workshop_orders", 1),
            ("workshop_order_lines", 1),
            ("stock_movements", 1),
            ("fiscal_journal", 2),
            ("fiscal_registers", 1),
        ] {
            let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(n, expected, "{table}");
        }
        let stock: i64 = scalar(&pool, "SELECT stock_milli FROM products WHERE id = $1", &ids.product).await;
        assert_eq!(stock, 7_500);
    }

    /// A document that points at something the cloud no longer sends is kept
    /// (without that link), and one that cannot be stored at all is counted and
    /// logged instead of undoing the whole import.
    #[tokio::test]
    async fn one_bad_row_does_not_undo_the_import() {
        let pool = blank_device().await;
        let (mut data, ids) = export();
        let missing = Uuid::new_v4().to_string();
        data["sales"][0]["client_id"] = json!(missing);
        data["stock_movements"].as_array_mut().unwrap().push(json!({
            "id": Uuid::new_v4().to_string(), "product_id": Uuid::new_v4().to_string(),
            "kind": "adjustment", "quantity_milli": 1_000
        }));

        let r = load(&pool, &data).await;
        assert_eq!(r.skipped, 1, "the movement of a product that does not exist");
        assert_eq!(r.sales, 2, "the invoice is kept");
        let client: Option<String> = scalar(&pool, "SELECT client_id FROM sales WHERE id = $1", &ids.invoice).await;
        assert_eq!(client, None, "…without its dangling customer");
        let customers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clients").fetch_one(&pool).await.unwrap();
        assert_eq!(customers, 1);
    }
}
