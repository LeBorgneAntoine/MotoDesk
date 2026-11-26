//! Admin-only local data manager: table browser, safe clear, SQL console.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlparser::ast::{Query as SqlQuery, SetExpr, Statement};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;
use sqlx::{Column, Row, TypeInfo, ValueRef};
use uuid::Uuid;

use crate::local_server::auth_guard::AdminUser;
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;

const MAX_PREVIEW_ROWS: i64 = 100;
const MAX_SQL_ROWS: i64 = 500;

const BUSINESS_TABLES: &[&str] = &[
    "clients",
    "vehicles",
    "categories",
    "products",
    "sales",
    "sale_lines",
    "app_settings",
    "doc_templates",
    "users",
    "company_cache",
    "payments",
    "suppliers",
    "workshop_orders",
    "stock_movements",
];

const SYSTEM_TABLES: &[&str] = &["sync_state", "cached_session", "change_log"];

/// Tables the SQL console may not even read: they hold key material and cloud
/// credentials. Matched as whole words anywhere in the statement, so line
/// breaks, comments, quoting or a CTE cannot hide the name.
const SECRET_TABLES: &[&str] =
    &["fiscal_key_fallback", "cached_session", "local_accounts", "network_recovery"];

/// Tables the SQL console may read but never write: the fiscal record, its
/// counters, the audit trail and the sync plumbing.
const NO_WRITE_TABLES: &[&str] = &[
    "fiscal_registers",
    "fiscal_journal",
    "fiscal_closures",
    "fiscal_counters",
    "fiscal_archives",
    "fiscal_number_series",
    "document_counters",
    "pos_sessions",
    "audit_log",
    "change_log",
    "sync_state",
];

/// Columns whose values are never shown, in the table view or in query results.
const MASKED_COLUMNS: &[&str] = &["tokens_json", "secret_key", "mesh_secret", "mesh_psk"];

/// Rows that belong to the fiscal record: sealed documents, their lines and their
/// payments. Clearing a table skips them; the database also refuses to change
/// them (migration 016), so this is about reporting honestly rather than being
/// the only line of defence.
fn sealed_clause(table: &str) -> &'static str {
    match table {
        "sales" => " AND locked_at IS NULL",
        "sale_lines" => {
            " AND (sale_id IS NULL OR sale_id NOT IN (SELECT id FROM sales WHERE locked_at IS NOT NULL))"
        }
        "payments" => " AND fiscal_seq IS NULL",
        _ => "",
    }
}

const ALL_MANAGED_TABLES: &[&str] = &[
    "clients",
    "vehicles",
    "categories",
    "products",
    "sales",
    "sale_lines",
    "app_settings",
    "doc_templates",
    "users",
    "company_cache",
    "payments",
    "suppliers",
    "workshop_orders",
    "stock_movements",
    "sync_state",
    "cached_session",
    "change_log",
];

struct ConfirmEntry {
    action_key: String,
    expires: Instant,
}

static CONFIRM_TOKENS: LazyLock<Mutex<HashMap<String, ConfirmEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/tables", get(list_tables))
        .route("/tables/:name/preview", get(preview_table))
        .route("/tables/:name/impact/clear", get(impact_clear))
        .route("/tables/:name/clear", post(clear_table))
        .route("/sql/preview", post(sql_preview))
        .route("/sql/execute", post(sql_execute))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TableInfo {
    name: String,
    row_count: i64,
    category: &'static str,
    has_soft_delete: bool,
    can_clear: bool,
}

#[derive(Debug, Deserialize)]
struct PreviewQuery {
    q: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TablePreview {
    columns: Vec<String>,
    rows: Vec<Value>,
    total: i64,
    limit: i64,
    offset: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImpactReport {
    table: String,
    operation: String,
    rows_affected: i64,
    warnings: Vec<String>,
    confirm_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClearBody {
    confirm_token: String,
}

#[derive(Debug, Deserialize)]
struct SqlBody {
    sql: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SqlExecuteBody {
    sql: String,
    confirm_token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SqlPreviewResult {
    statement_type: String,
    read_only: bool,
    rows_affected: Option<i64>,
    columns: Option<Vec<String>>,
    rows: Option<Vec<Value>>,
    sample_rows: Option<Vec<Value>>,
    message: Option<String>,
    elapsed_ms: u64,
    confirm_token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SqlExecuteResult {
    rows_affected: i64,
    columns: Option<Vec<String>>,
    rows: Option<Vec<Value>>,
    elapsed_ms: u64,
}

// ---------------------------------------------------------------------------
// Confirm tokens
// ---------------------------------------------------------------------------

fn purge_expired_tokens() {
    let now = Instant::now();
    let mut guard = CONFIRM_TOKENS.lock().expect("confirm tokens");
    guard.retain(|_, e| e.expires > now);
}

fn issue_token(action_key: String) -> String {
    purge_expired_tokens();
    let token = Uuid::new_v4().to_string();
    guard_insert(token.clone(), action_key);
    token
}

fn guard_insert(token: String, action_key: String) {
    let mut guard = CONFIRM_TOKENS.lock().expect("confirm tokens");
    guard.insert(
        token,
        ConfirmEntry {
            action_key,
            expires: Instant::now() + Duration::from_secs(120),
        },
    );
}

fn consume_token(token: &str, expected_action: &str) -> LocalResult<()> {
    purge_expired_tokens();
    let mut guard = CONFIRM_TOKENS.lock().expect("confirm tokens");
    let entry = guard.remove(token).ok_or_else(|| {
        LocalError::BadRequest("Jeton de confirmation invalide ou expiré.".into())
    })?;
    if entry.expires < Instant::now() {
        return Err(LocalError::BadRequest(
            "Jeton de confirmation expiré (120 s). Relancez l'aperçu.".into(),
        ));
    }
    if entry.action_key != expected_action {
        return Err(LocalError::BadRequest(
            "Le jeton ne correspond pas à l'action demandée.".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Table helpers
// ---------------------------------------------------------------------------

fn table_category(name: &str) -> &'static str {
    if SYSTEM_TABLES.contains(&name) {
        "system"
    } else {
        "business"
    }
}

fn can_clear_table(name: &str) -> bool {
    BUSINESS_TABLES.contains(&name)
}

fn validate_table_name(name: &str) -> LocalResult<()> {
    if !ALL_MANAGED_TABLES.contains(&name) {
        return Err(LocalError::NotFound);
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(LocalError::BadRequest("Nom de table invalide.".into()));
    }
    Ok(())
}

async fn table_has_column(state: &SharedState, table: &str, col: &str) -> LocalResult<bool> {
    let sql = format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = $1");
    let (n,): (i64,) = sqlx::query_as(&sql).bind(col).fetch_one(&state.db).await?;
    Ok(n > 0)
}

async fn count_rows(state: &SharedState, table: &str, active_only: bool) -> LocalResult<i64> {
    let sql = if active_only && table_has_column(state, table, "deleted_at").await? {
        format!("SELECT COUNT(*) FROM \"{table}\" WHERE deleted_at IS NULL")
    } else {
        format!("SELECT COUNT(*) FROM \"{table}\"")
    };
    let (n,): (i64,) = sqlx::query_as(&sql).fetch_one(&state.db).await?;
    Ok(n)
}

/// Rows a "clear" would really touch (active, and not sealed), and how many
/// sealed rows it will leave alone.
async fn count_clearable(state: &SharedState, table: &str, has_soft: bool) -> LocalResult<(i64, i64)> {
    let guard = sealed_clause(table);
    let base = if has_soft { " WHERE deleted_at IS NULL" } else { " WHERE 1=1" };
    let (clearable,): (i64,) =
        sqlx::query_as(&format!("SELECT COUNT(*) FROM \"{table}\"{base}{guard}"))
            .fetch_one(&state.db)
            .await?;
    let (all,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM \"{table}\"{base}"))
        .fetch_one(&state.db)
        .await?;
    Ok((clearable, all - clearable))
}

async fn pending_sync_count(state: &SharedState) -> LocalResult<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM change_log WHERE synced_to_cloud = 0")
        .fetch_one(&state.db)
        .await?;
    Ok(n)
}

fn scan_blocked_keywords(sql: &str) -> Option<&'static str> {
    let upper = sql.to_uppercase();
    for kw in [
        "ATTACH",
        "DETACH",
        "PRAGMA",
        "VACUUM",
        "LOAD_EXTENSION",
        "READ ",
        "WRITE ",
        "CREATE TRIGGER",
        "CREATE VIRTUAL",
    ] {
        if upper.contains(kw) {
            return Some(kw);
        }
    }
    None
}

fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_one_statement(sql: &str) -> LocalResult<Statement> {
    let mut trimmed = sql.trim().to_string();
    if trimmed.is_empty() {
        return Err(LocalError::BadRequest("Requête SQL vide.".into()));
    }
    if let Some(kw) = scan_blocked_keywords(&trimmed) {
        return Err(LocalError::BadRequest(format!(
            "Instruction interdite ({kw})."
        )));
    }
    while trimmed.ends_with(';') {
        trimmed.pop();
        trimmed = trimmed.trim().to_string();
    }
    if trimmed.contains(';') {
        return Err(LocalError::BadRequest(
            "Une seule instruction SQL est autorisée.".into(),
        ));
    }

    let dialect = SQLiteDialect {};
    let stmts =
        Parser::parse_sql(&dialect, &trimmed).map_err(|e| LocalError::BadRequest(e.to_string()))?;
    if stmts.len() != 1 {
        return Err(LocalError::BadRequest(
            "Une seule instruction SQL est autorisée.".into(),
        ));
    }
    let stmt = stmts.into_iter().next().unwrap();
    enforce_table_rules(&trimmed, &stmt)?;
    Ok(stmt)
}

fn sql_words(sql: &str) -> Vec<String> {
    sql.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect()
}

/// A query body that writes (`WITH x AS (...) UPDATE ...` parses as a query).
fn query_writes(q: &SqlQuery) -> bool {
    fn set_expr_writes(e: &SetExpr) -> bool {
        match e {
            SetExpr::Select(_) | SetExpr::Values(_) | SetExpr::Table(_) => false,
            SetExpr::Query(q) => query_writes(q),
            SetExpr::SetOperation { left, right, .. } => {
                set_expr_writes(left) || set_expr_writes(right)
            }
            // Insert / Update / anything added later: treat as a write.
            _ => true,
        }
    }
    set_expr_writes(&q.body)
}

/// Whole-statement table rules, independent of how the SQL is laid out.
fn enforce_table_rules(sql: &str, stmt: &Statement) -> LocalResult<()> {
    if let Statement::Query(q) = stmt {
        if query_writes(q) {
            return Err(LocalError::BadRequest(
                "Une écriture à l'intérieur d'une requête WITH n'est pas prise en charge.".into(),
            ));
        }
    }
    let words = sql_words(sql);
    if let Some(t) = SECRET_TABLES.iter().find(|t| words.iter().any(|w| w == *t)) {
        return Err(LocalError::BadRequest(format!(
            "La table « {t} » contient des secrets et n'est pas accessible depuis la console."
        )));
    }
    if !is_read_only(stmt) {
        if let Some(t) = NO_WRITE_TABLES.iter().find(|t| words.iter().any(|w| w == *t)) {
            return Err(LocalError::BadRequest(format!(
                "La table « {t} » est en lecture seule : journal fiscal, compteurs et synchronisation ne se modifient pas à la main."
            )));
        }
    }
    Ok(())
}

fn statement_type_label(stmt: &Statement) -> &'static str {
    match stmt {
        Statement::Query(_) => "SELECT",
        Statement::Insert { .. } => "INSERT",
        Statement::Update { .. } => "UPDATE",
        Statement::Delete { .. } => "DELETE",
        _ => "DDL",
    }
}

fn is_read_only(stmt: &Statement) -> bool {
    matches!(stmt, Statement::Query(q) if !query_writes(q))
}

fn is_ddl(stmt: &Statement) -> bool {
    !matches!(
        stmt,
        Statement::Query(_)
            | Statement::Insert { .. }
            | Statement::Update { .. }
            | Statement::Delete { .. }
    )
}

fn table_from_sql(sql: &str, stmt: &Statement) -> Option<String> {
    let s = sql.trim();
    let upper = s.to_uppercase();
    match stmt {
        Statement::Insert { .. } if upper.contains("INTO") => extract_after_keyword(s, &["INTO"]),
        Statement::Update { .. } if upper.starts_with("UPDATE ") => {
            let tail = s[7..].trim_start();
            let end = tail.to_uppercase().find(" SET ").unwrap_or(tail.len());
            Some(
                tail[..end]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('`')
                    .to_string(),
            )
        }
        Statement::Delete { .. } if upper.starts_with("DELETE FROM ") => {
            let tail = s[12..].trim_start();
            let end = tail.to_uppercase().find(" WHERE ").unwrap_or(tail.len());
            Some(
                tail[..end]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('`')
                    .to_string(),
            )
        }
        Statement::Query(_) if upper.starts_with("SELECT ") => extract_from_clause_table(s),
        _ => None,
    }
}

fn extract_after_keyword(sql: &str, keywords: &[&str]) -> Option<String> {
    let upper = sql.to_uppercase();
    for kw in keywords {
        if let Some(pos) = upper.find(kw) {
            let start = pos + kw.len();
            let tail = sql[start..].trim_start();
            let token: String = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }
    None
}

fn extract_from_clause_table(sql: &str) -> Option<String> {
    let upper = sql.to_uppercase();
    let from_pos = upper.find(" FROM ")?;
    let tail = sql[from_pos + 6..].trim_start();
    let token: String = tail
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn ensure_table_allowed_for_write(table: &str, write: bool) -> LocalResult<()> {
    validate_table_name(table)?;
    if write && SYSTEM_TABLES.contains(&table) {
        return Err(LocalError::BadRequest(format!(
            "La table système « {table} » est en lecture seule."
        )));
    }
    Ok(())
}

/// One cell as JSON, decoded by what SQLite actually stored in it.
///
/// NULL is looked at first: sqlx decodes a NULL as any type without complaint
/// (`try_get::<i64>` gives 0), so "try each type until one works" turned every
/// empty cell into the number 0. For a value that is there, its storage class
/// decides (not the column's declared type, and not which decode happens to
/// succeed), so text made of digits stays text with its leading zeros.
fn cell_to_value(row: &sqlx::sqlite::SqliteRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else {
        return Value::Null;
    };
    if raw.is_null() {
        return Value::Null;
    }
    // For a non-NULL value this is the storage class: INTEGER, REAL, TEXT or BLOB.
    let stored = raw.type_info();
    match stored.name() {
        "INTEGER" => row.try_get::<i64, _>(i).map(|v| json!(v)).unwrap_or(Value::Null),
        "REAL" => row.try_get::<f64, _>(i).map(|v| json!(v)).unwrap_or(Value::Null),
        "TEXT" => row.try_get::<String, _>(i).map(Value::String).unwrap_or(Value::Null),
        // A BLOB (a logo, a key) has no useful JSON form; it was null before too.
        _ => Value::Null,
    }
}

async fn rows_to_json(
    rows: Vec<sqlx::sqlite::SqliteRow>,
) -> LocalResult<(Vec<String>, Vec<Value>)> {
    if rows.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let cols: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut obj = serde_json::Map::new();
        for (i, name) in cols.iter().enumerate() {
            let mut value = cell_to_value(&row, i);
            if MASKED_COLUMNS.contains(&name.to_ascii_lowercase().as_str()) && !value.is_null() {
                value = Value::String("••••••••".into());
            }
            obj.insert(name.clone(), value);
        }
        out.push(Value::Object(obj));
    }
    Ok((cols, out))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn list_tables(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> LocalResult<Json<Vec<TableInfo>>> {
    let mut out = Vec::new();
    for name in ALL_MANAGED_TABLES {
        let has_soft = table_has_column(&state, name, "deleted_at").await?;
        let row_count = count_rows(&state, name, has_soft).await?;
        out.push(TableInfo {
            name: name.to_string(),
            row_count,
            category: table_category(name),
            has_soft_delete: has_soft,
            can_clear: can_clear_table(name),
        });
    }
    Ok(Json(out))
}

async fn preview_table(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(name): Path<String>,
    Query(q): Query<PreviewQuery>,
) -> LocalResult<Json<TablePreview>> {
    validate_table_name(&name)?;
    let limit = q
        .limit
        .unwrap_or(MAX_PREVIEW_ROWS)
        .clamp(1, MAX_PREVIEW_ROWS);
    let offset = q.offset.unwrap_or(0).max(0);
    let has_soft = table_has_column(&state, &name, "deleted_at").await?;

    // One WHERE clause for the page and for its count, so `total` is the number
    // of rows the search found and the pager cannot offer a page that is empty.
    let mut filter = String::new();
    if has_soft {
        filter.push_str(" WHERE deleted_at IS NULL");
    }
    let mut like: Option<String> = None;
    if let Some(needle) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let cols = text_columns(&state, &name).await?;
        if !cols.is_empty() {
            let preds: Vec<String> = cols.iter().map(|c| format!("\"{c}\" LIKE $1")).collect();
            let joiner = if has_soft { " AND (" } else { " WHERE (" };
            filter.push_str(joiner);
            filter.push_str(&preds.join(" OR "));
            filter.push(')');
            like = Some(format!("%{}%", needle.replace('%', "\\%")));
        }
    }

    let count_sql = format!("SELECT COUNT(*) FROM \"{name}\"{filter}");
    let page_sql = format!("SELECT * FROM \"{name}\"{filter} LIMIT {limit} OFFSET {offset}");
    let (total, rows) = match &like {
        Some(like) => {
            let (total,): (i64,) =
                sqlx::query_as(&count_sql).bind(like).fetch_one(&state.db).await?;
            let rows = sqlx::query(&page_sql).bind(like).fetch_all(&state.db).await?;
            (total, rows)
        }
        None => {
            let (total,): (i64,) = sqlx::query_as(&count_sql).fetch_one(&state.db).await?;
            let rows = sqlx::query(&page_sql).fetch_all(&state.db).await?;
            (total, rows)
        }
    };

    let (columns, json_rows) = rows_to_json(rows).await?;
    Ok(Json(TablePreview {
        columns,
        rows: json_rows,
        total,
        limit,
        offset,
    }))
}

async fn text_columns(state: &SharedState, table: &str) -> LocalResult<Vec<String>> {
    let sql = format!(
        "SELECT name FROM pragma_table_info('{table}') WHERE type IN ('TEXT','') OR type IS NULL"
    );
    let rows: Vec<(String,)> = sqlx::query_as(&sql).fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

async fn impact_clear(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(name): Path<String>,
) -> LocalResult<Json<ImpactReport>> {
    validate_table_name(&name)?;
    if !can_clear_table(&name) {
        return Err(LocalError::BadRequest(
            "Cette table système ne peut pas être vidée.".into(),
        ));
    }

    let has_soft = table_has_column(&state, &name, "deleted_at").await?;
    let (rows_affected, sealed_kept) = count_clearable(&state, &name, has_soft).await?;
    let operation = if has_soft {
        "Marquer toutes les lignes comme supprimées (deleted_at)"
    } else {
        "Supprimer toutes les lignes (DELETE)"
    };

    let mut warnings = vec![
        "Les modifications seront enregistrées localement et pourront être synchronisées vers le cloud.".into(),
    ];
    if sealed_kept > 0 {
        warnings.insert(
            0,
            format!(
                "{sealed_kept} ligne(s) scellée(s) dans le journal fiscal sont protégées : elles ne seront ni supprimées ni modifiées."
            ),
        );
    }
    let pending = pending_sync_count(&state).await?;
    if pending > 0 {
        warnings.push(format!(
            "{pending} opération(s) en attente de synchronisation cloud."
        ));
    }

    match name.as_str() {
        "categories" => {
            let (n,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM products WHERE deleted_at IS NULL AND category_id IS NOT NULL",
            )
            .fetch_one(&state.db)
            .await?;
            if n > 0 {
                warnings.push(format!(
                    "{n} produit(s) actifs référencent encore une catégorie."
                ));
            }
        }
        "clients" => {
            let (n,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM vehicles WHERE deleted_at IS NULL AND client_id IS NOT NULL",
            )
            .fetch_one(&state.db)
            .await?;
            if n > 0 {
                warnings.push(format!("{n} véhicule(s) sont encore liés à un client."));
            }
        }
        _ => {}
    }

    let action_key = format!("clear:{name}");
    let confirm_token = issue_token(action_key);

    Ok(Json(ImpactReport {
        table: name,
        operation: operation.to_string(),
        rows_affected,
        warnings,
        confirm_token,
    }))
}

async fn clear_table(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(name): Path<String>,
    Json(body): Json<ClearBody>,
) -> LocalResult<Json<Value>> {
    validate_table_name(&name)?;
    if !can_clear_table(&name) {
        return Err(LocalError::BadRequest(
            "Cette table système ne peut pas être vidée.".into(),
        ));
    }

    let action_key = format!("clear:{name}");
    consume_token(&body.confirm_token, &action_key)?;

    let now = Utc::now().to_rfc3339();
    let has_soft = table_has_column(&state, &name, "deleted_at").await?;
    let guard = sealed_clause(&name);
    let ids: Vec<(String,)> = sqlx::query_as(&format!(
        "SELECT id FROM \"{name}\" WHERE {}{guard}",
        if has_soft { "deleted_at IS NULL" } else { "1=1" }
    ))
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let affected = if has_soft {
        let sql = format!("UPDATE \"{name}\" SET deleted_at = $1 WHERE deleted_at IS NULL{guard}");
        let r = sqlx::query(&sql).bind(&now).execute(&state.db).await?;
        r.rows_affected() as i64
    } else {
        let sql = format!("DELETE FROM \"{name}\"");
        let r = sqlx::query(&sql).execute(&state.db).await?;
        r.rows_affected() as i64
    };

    if let Ok(device_id) = state.device_id().await {
        let entity = match name.as_str() {
            "clients" => Some(motodesk_core::entity::EntityType::Client),
            "vehicles" => Some(motodesk_core::entity::EntityType::Vehicle),
            "products" => Some(motodesk_core::entity::EntityType::Product),
            "categories" => Some(motodesk_core::entity::EntityType::Category),
            "sales" => Some(motodesk_core::entity::EntityType::Sale),
            "sale_lines" => Some(motodesk_core::entity::EntityType::SaleLine),
            "doc_templates" => Some(motodesk_core::entity::EntityType::DocTemplate),
            _ => None,
        };
        if let Some(et) = entity {
            for (id,) in ids.iter().take(5000) {
                if let Ok(uuid) = uuid::Uuid::parse_str(id) {
                    let _ = crate::local_server::sync_log::append(
                        &state.db,
                        device_id,
                        et,
                        uuid,
                        motodesk_core::sync::OpKind::Delete,
                        serde_json::json!({ "id": id }),
                        None,
                    )
                    .await;
                }
            }
        }
    }

    let _ = sqlx::query(
        "INSERT INTO audit_log (id, action, entity_type, entity_id, payload, created_at) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind("clear_table")
    .bind(&name)
    .bind(&name)
    .bind(serde_json::json!({ "rows": affected }).to_string())
    .bind(&now)
    .execute(&state.db)
    .await;

    if affected > 0 {
        // A purge is a fiscal event (§ 250): said in the journal, not only in the
        // audit log. Best effort, like the audit row above.
        let _ = crate::local_server::fiscal::journal_event(
            &state,
            motodesk_fiscal::FiscalEventType::DataPurged,
            json!({ "table": name, "rows": affected, "source": "data_manager" }),
        )
        .await;
    }
    state.notify_sync();
    Ok(Json(json!({
        "table": name,
        "rowsAffected": affected,
    })))
}

async fn sql_preview(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<SqlBody>,
) -> LocalResult<Json<SqlPreviewResult>> {
    let normalized = normalize_sql(&body.sql);
    let stmt = parse_one_statement(&body.sql)?;
    if is_ddl(&stmt) {
        return Err(LocalError::BadRequest(
            "DDL (CREATE/DROP/ALTER) désactivé en v1.".into(),
        ));
    }

    let label = statement_type_label(&stmt);
    let read_only = is_read_only(&stmt);
    let started = Instant::now();

    if read_only {
        if let Some(ref t) = table_from_sql(&body.sql, &stmt) {
            validate_table_name(t)?;
        }
        let limited_sql = ensure_select_limit(&body.sql);
        let rows = sqlx::query(&limited_sql).fetch_all(&state.db).await?;
        let (columns, json_rows) = rows_to_json(rows).await?;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        return Ok(Json(SqlPreviewResult {
            statement_type: label.to_string(),
            read_only: true,
            rows_affected: None,
            columns: Some(columns),
            rows: Some(json_rows),
            sample_rows: None,
            message: None,
            elapsed_ms,
            confirm_token: None,
        }));
    }

    let table = table_from_sql(&body.sql, &stmt)
        .ok_or_else(|| LocalError::BadRequest("Impossible d'identifier la table cible.".into()))?;
    ensure_table_allowed_for_write(&table, true)?;

    let (rows_affected, sample_rows) = match &stmt {
        Statement::Delete { .. } => {
            let count_sql = delete_to_count_sql(&body.sql)?;
            let (n,): (i64,) = sqlx::query_as(&count_sql).fetch_one(&state.db).await?;
            let sample_sql = format!("{} LIMIT 5", delete_to_select_sql(&body.sql)?);
            let sample = fetch_sample(&state, &sample_sql).await?;
            (Some(n), Some(sample))
        }
        Statement::Update { .. } => {
            let count_sql = update_to_count_sql(&body.sql)?;
            let (n,): (i64,) = sqlx::query_as(&count_sql).fetch_one(&state.db).await?;
            let sample_sql = format!("{} LIMIT 5", update_to_select_sql(&body.sql)?);
            let sample = fetch_sample(&state, &sample_sql).await?;
            (Some(n), Some(sample))
        }
        Statement::Insert { .. } => (None, None),
        _ => {
            return Err(LocalError::BadRequest(
                "Type d'instruction non pris en charge.".into(),
            ))
        }
    };

    let action_key = format!("sql:{normalized}");
    let confirm_token = issue_token(action_key);
    let message = if matches!(stmt, Statement::Insert { .. }) {
        Some("INSERT : le nombre de lignes ne peut pas être estimé. Vérifiez la requête avant exécution.".into())
    } else {
        None
    };

    Ok(Json(SqlPreviewResult {
        statement_type: label.to_string(),
        read_only: false,
        rows_affected,
        columns: None,
        rows: None,
        sample_rows,
        message,
        elapsed_ms: started.elapsed().as_millis() as u64,
        confirm_token: Some(confirm_token),
    }))
}

async fn sql_execute(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<SqlExecuteBody>,
) -> LocalResult<Json<SqlExecuteResult>> {
    let normalized = normalize_sql(&body.sql);
    let action_key = format!("sql:{normalized}");
    consume_token(&body.confirm_token, &action_key)?;

    let stmt = parse_one_statement(&body.sql)?;
    if is_ddl(&stmt) {
        return Err(LocalError::BadRequest(
            "DDL (CREATE/DROP/ALTER) désactivé en v1.".into(),
        ));
    }
    if is_read_only(&stmt) {
        return Err(LocalError::BadRequest(
            "Les requêtes SELECT ne nécessitent pas d'exécution confirmée.".into(),
        ));
    }

    if let Some(t) = table_from_sql(&body.sql, &stmt) {
        ensure_table_allowed_for_write(&t, true)?;
    }

    let started = Instant::now();
    let result = sqlx::query(&body.sql).execute(&state.db).await?;
    let rows_affected = result.rows_affected() as i64;
    let now = Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "INSERT INTO audit_log (id, action, entity_type, entity_id, payload, created_at) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind("sql_execute")
    .bind("sql")
    .bind("console")
    .bind(serde_json::json!({ "sql": body.sql, "rows": rows_affected }).to_string())
    .bind(&now)
    .execute(&state.db)
    .await;
    state.notify_sync();

    Ok(Json(SqlExecuteResult {
        rows_affected,
        columns: None,
        rows: None,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }))
}

fn ensure_select_limit(sql: &str) -> String {
    let upper = sql.to_uppercase();
    if upper.contains(" LIMIT ") {
        return sql.to_string();
    }
    format!("{sql} LIMIT {MAX_SQL_ROWS}")
}

async fn fetch_sample(state: &SharedState, sql: &str) -> LocalResult<Vec<Value>> {
    let rows = sqlx::query(sql).fetch_all(&state.db).await?;
    let (_, json_rows) = rows_to_json(rows).await?;
    Ok(json_rows)
}

fn delete_to_count_sql(sql: &str) -> LocalResult<String> {
    let s = sql.trim();
    let upper = s.to_uppercase();
    if !upper.starts_with("DELETE FROM ") {
        return Err(LocalError::BadRequest("DELETE mal formé.".into()));
    }
    let tail = s[12..].trim_start();
    Ok(format!("SELECT COUNT(*) FROM {tail}"))
}

fn delete_to_select_sql(sql: &str) -> LocalResult<String> {
    let s = sql.trim();
    let upper = s.to_uppercase();
    if !upper.starts_with("DELETE FROM ") {
        return Err(LocalError::BadRequest("DELETE mal formé.".into()));
    }
    let tail = s[12..].trim_start();
    Ok(format!("SELECT * FROM {tail}"))
}

fn update_to_count_sql(sql: &str) -> LocalResult<String> {
    let select = update_to_select_sql(sql)?;
    Ok(select.replacen("SELECT *", "SELECT COUNT(*)", 1))
}

fn update_to_select_sql(sql: &str) -> LocalResult<String> {
    let s = sql.trim();
    let upper = s.to_uppercase();
    if !upper.starts_with("UPDATE ") {
        return Err(LocalError::BadRequest("UPDATE mal formé.".into()));
    }
    let tail = s[7..].trim_start();
    let set_idx = upper[7..]
        .find(" SET ")
        .ok_or_else(|| LocalError::BadRequest("UPDATE sans clause SET.".into()))?;
    let table_part = tail[..set_idx].trim();
    let after_set = &tail[set_idx + 5..];
    let where_idx = after_set.to_uppercase().find(" WHERE ");
    let where_clause = match where_idx {
        Some(i) => format!(" WHERE {}", after_set[i + 7..].trim_start()),
        None => String::new(),
    };
    Ok(format!("SELECT * FROM {table_part}{where_clause}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::auth_guard::require_admin;

    #[test]
    fn console_refuses_writes_hidden_inside_a_with_clause() {
        let err = parse_one_statement(
            "WITH x AS (SELECT 1 LIMIT 1) UPDATE clients SET last_name = 'x'",
        );
        assert!(err.is_err(), "a WITH ... UPDATE must not pass as a read");
    }

    #[test]
    fn console_cannot_read_secret_tables_however_the_sql_is_laid_out() {
        for sql in [
            "SELECT * FROM fiscal_key_fallback",
            "SELECT *\nFROM fiscal_key_fallback",
            "select secret_key from \"fiscal_key_fallback\"",
            "SELECT /* x */ tokens_json FROM cached_session",
            "SELECT * FROM clients WHERE id IN (SELECT register_code FROM fiscal_key_fallback)",
        ] {
            assert!(parse_one_statement(sql).is_err(), "should be refused: {sql}");
        }
    }

    #[test]
    fn console_cannot_write_the_fiscal_record() {
        for sql in [
            "UPDATE fiscal_counters SET perpetual_total_cents = 0",
            "DELETE FROM fiscal_number_series",
            "INSERT INTO fiscal_journal (id) VALUES ('x')",
            "UPDATE clients SET last_name = 'x' WHERE id IN (SELECT id FROM fiscal_registers)",
            "INSERT /* INTO clients */ INTO document_counters (kind) VALUES ('x')",
        ] {
            assert!(parse_one_statement(sql).is_err(), "should be refused: {sql}");
        }
    }

    #[test]
    fn console_still_allows_ordinary_work() {
        assert!(parse_one_statement("SELECT * FROM clients").is_ok());
        assert!(parse_one_statement("SELECT * FROM fiscal_journal").is_ok());
        assert!(parse_one_statement("UPDATE clients SET notes = 'x' WHERE id = 'a'").is_ok());
        assert!(parse_one_statement("WITH c AS (SELECT id FROM clients) SELECT * FROM c").is_ok());
    }
    use crate::local_server::db;
    use crate::local_server::state::LocalState;
    use std::sync::Arc;

    async fn admin_state() -> SharedState {
        let path = std::env::temp_dir().join(format!("motodesk_data_{}.db", Uuid::new_v4()));
        let pool = db::init_pool(&path).await.unwrap();
        db::run_migrations(&pool).await.unwrap();
        db::ensure_device_id(&pool).await.unwrap();
        sqlx::query(
            "UPDATE sync_state SET company_id = 'c0000001-0000-0000-0000-000000000001' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .unwrap();
        let user = json!({
            "id": "u1",
            "email": "admin@test.fr",
            "full_name": "Admin",
            "role": "admin"
        });
        sqlx::query(
            "INSERT OR REPLACE INTO cached_session (id, user_json, tokens_json, cached_at) VALUES (1, $1, '{}', $2)",
        )
        .bind(user.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        Arc::new(LocalState::new(
            pool,
            std::env::temp_dir(),
            "http://127.0.0.1:8080".into(),
            None,
            None,
            "test-local-token".into(),
        ))
    }

    async fn employee_state() -> SharedState {
        let state = admin_state().await;
        let user = json!({
            "id": "u2",
            "email": "emp@test.fr",
            "full_name": "Emp",
            "role": "employee"
        });
        sqlx::query("UPDATE cached_session SET user_json = $1 WHERE id = 1")
            .bind(user.to_string())
            .execute(&state.db)
            .await
            .unwrap();
        state
    }

    #[tokio::test]
    async fn require_admin_rejects_employee() {
        let state = employee_state().await;
        assert!(matches!(
            require_admin(&state).await,
            Err(LocalError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn require_admin_accepts_admin_with_company() {
        let state = admin_state().await;
        assert!(require_admin(&state).await.is_ok());
    }

    #[tokio::test]
    async fn parse_rejects_multi_statement() {
        let err = parse_one_statement("SELECT 1; SELECT 2").unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
    }

    #[tokio::test]
    async fn parse_rejects_pragma() {
        let err = parse_one_statement("PRAGMA table_info(clients)").unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
    }

    #[tokio::test]
    async fn system_table_cannot_clear() {
        assert!(!can_clear_table("change_log"));
        assert!(can_clear_table("clients"));
    }

    #[tokio::test]
    async fn confirm_token_single_use() {
        let t = issue_token("sql:test".into());
        consume_token(&t, "sql:test").unwrap();
        assert!(consume_token(&t, "sql:test").is_err());
    }

    #[test]
    fn write_on_system_table_rejected() {
        let err = ensure_table_allowed_for_write("change_log", true).unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
    }

    #[tokio::test]
    async fn select_preview_runs_on_empty_db() {
        let state = admin_state().await;
        let stmt = parse_one_statement("SELECT id FROM clients LIMIT 1").unwrap();
        assert!(is_read_only(&stmt));
        let rows = sqlx::query("SELECT id FROM clients LIMIT 1")
            .fetch_all(&state.db)
            .await
            .unwrap();
        let (_, json_rows) = rows_to_json(rows).await.unwrap();
        assert!(json_rows.is_empty() || json_rows.len() <= 1);
    }

    #[tokio::test]
    async fn delete_preview_does_not_mutate() {
        let state = admin_state().await;
        let before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clients")
            .fetch_one(&state.db)
            .await
            .unwrap();
        let count_sql = delete_to_count_sql("DELETE FROM clients WHERE 1=0").unwrap();
        let (n,): (i64,) = sqlx::query_as(&count_sql)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(n, 0);
        let after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clients")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(before.0, after.0);
    }

    /// A client written the way the app writes one, with only the columns a
    /// test cares about; everything else is left to the schema (NULL or default).
    async fn insert_client(state: &SharedState, id: &str, last_name: &str, extra: &[(&str, &str)]) {
        let mut cols = String::from("id, last_name, created_at, updated_at");
        let mut marks = String::from("$1, $2, $3, $3");
        for (n, (col, _)) in extra.iter().enumerate() {
            cols.push_str(&format!(", {col}"));
            marks.push_str(&format!(", ${}", n + 4));
        }
        let sql = format!("INSERT INTO clients ({cols}) VALUES ({marks})");
        let mut query = sqlx::query(&sql)
            .bind(id)
            .bind(last_name)
            .bind("2026-01-15T10:00:00+00:00");
        for (_, value) in extra {
            query = query.bind(*value);
        }
        query.execute(&state.db).await.unwrap();
    }

    async fn preview(state: &SharedState, table: &str, q: Option<&str>, limit: Option<i64>) -> TablePreview {
        let admin = require_admin(state).await.unwrap();
        let Json(page) = preview_table(
            State(state.clone()),
            admin,
            Path(table.to_string()),
            Query(PreviewQuery { q: q.map(str::to_string), limit, offset: None }),
        )
        .await
        .unwrap();
        page
    }

    #[tokio::test]
    async fn an_empty_cell_comes_back_as_null_not_as_zero() {
        let state = admin_state().await;
        // phone (TEXT), credit_limit_cents (INTEGER) and deleted_at (a date) are
        // left NULL; credit_blocked is a real 0 and must stay one.
        insert_client(&state, "c-null", "Sansrien", &[]).await;

        let page = preview(&state, "clients", None, None).await;
        let row = &page.rows[0];
        for col in ["phone", "credit_limit_cents", "deleted_at", "first_name", "notes"] {
            assert_eq!(row[col], Value::Null, "{col} is NULL in the database");
        }
        assert_eq!(row["credit_blocked"], json!(0), "a stored 0 is still a 0");
        assert_eq!(row["row_version"], json!(1));
        assert_eq!(row["last_name"], json!("Sansrien"));

        // The same decoding serves the SQL console and the samples of a write preview.
        let rows = sqlx::query("SELECT deleted_at, credit_limit_cents, NULL AS computed FROM clients")
            .fetch_all(&state.db)
            .await
            .unwrap();
        let (_, json_rows) = rows_to_json(rows).await.unwrap();
        assert_eq!(
            json_rows[0],
            json!({ "deleted_at": null, "credit_limit_cents": null, "computed": null })
        );
    }

    #[tokio::test]
    async fn text_made_of_digits_stays_text() {
        let state = admin_state().await;
        insert_client(
            &state,
            "c-digits",
            "Chiffres",
            &[("address", "35000"), ("phone", "0612345678"), ("legacy_ref", "01234")],
        )
        .await;

        let page = preview(&state, "clients", None, None).await;
        let row = &page.rows[0];
        assert_eq!(row["address"], json!("35000"));
        assert_eq!(row["phone"], json!("0612345678"));
        assert_eq!(row["legacy_ref"], json!("01234"));

        // What a query computes follows the value, not the column it came from.
        let rows = sqlx::query(
            "SELECT '007' AS txt, 7 AS whole, 1.5 AS frac, CAST(phone AS INTEGER) AS as_int FROM clients",
        )
        .fetch_all(&state.db)
        .await
        .unwrap();
        let (_, json_rows) = rows_to_json(rows).await.unwrap();
        assert_eq!(
            json_rows[0],
            json!({ "txt": "007", "whole": 7, "frac": 1.5, "as_int": 612345678 })
        );
    }

    #[tokio::test]
    async fn total_is_the_number_of_rows_the_search_found() {
        let state = admin_state().await;
        insert_client(&state, "c-1", "Dupont", &[]).await;
        insert_client(&state, "c-2", "Martin", &[("notes", "ami de Dupont")]).await;
        insert_client(&state, "c-3", "Bernard", &[]).await;
        insert_client(&state, "c-4", "Dupont", &[("deleted_at", "2026-02-01T00:00:00+00:00")]).await;

        // No search: the table's count (active rows only, as before).
        let all = preview(&state, "clients", None, None).await;
        assert_eq!(all.total, 3);
        assert_eq!(all.rows.len(), 3);
        let blank = preview(&state, "clients", Some("   "), None).await;
        assert_eq!(blank.total, 3, "a blank search is no search");

        // A search: the rows that match, whatever the page size.
        let found = preview(&state, "clients", Some("dupont"), None).await;
        assert_eq!(found.rows.len(), 2);
        assert_eq!(found.total, 2);
        let first_page = preview(&state, "clients", Some("dupont"), Some(1)).await;
        assert_eq!(first_page.rows.len(), 1);
        assert_eq!(first_page.total, 2, "total counts the matches, not the page");

        let none = preview(&state, "clients", Some("zzz-introuvable"), None).await;
        assert_eq!(none.rows.len(), 0);
        assert_eq!(none.total, 0);
    }
}
