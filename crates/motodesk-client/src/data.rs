//! The administrator's tools on the local database (port of `desktop/src/lib/api/data.ts`):
//! looking into a table, emptying one, and running a single SQL statement. Administrators only.
//!
//! Nothing here writes without two steps: an impact report or a preview first, which hands
//! back a token, then the action itself with that token.

use axum::http::Method;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::error::ApiResult;
use crate::import::csv::to_csv;
use crate::session::Client;
use crate::settings::encode;

/// One row of a table or of a result: its values by column.
pub type Row = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    pub name: String,
    #[serde(default)]
    pub row_count: i64,
    /// `business` (the shop's records) or `system` (what the application keeps for itself).
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub has_soft_delete: bool,
    #[serde(default)]
    pub can_clear: bool,
}

impl TableInfo {
    pub fn is_system(&self) -> bool {
        self.category == "system"
    }
}

/// One page of a table.
///
/// `total` counts the rows of the table, whatever was searched for: with a search, only the
/// rows of the page say how many were found.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct TablePreview {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    /// How many rows the table holds.
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// What an action would do, before it is done.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactReport {
    pub table: String,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub rows_affected: i64,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// What the action must be sent with.
    pub confirm_token: String,
}

/// What a statement is and would do. A read comes back with its rows; a write with an
/// estimate, a sample of the rows it would touch, and the token to run it.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SqlPreview {
    pub statement_type: String,
    pub read_only: bool,
    pub rows_affected: Option<i64>,
    pub columns: Option<Vec<String>>,
    pub rows: Option<Vec<Row>>,
    pub sample_rows: Option<Vec<Row>>,
    pub message: Option<String>,
    pub elapsed_ms: i64,
    pub confirm_token: Option<String>,
}

impl SqlPreview {
    /// The token to run the statement with: only a write has one.
    pub fn token_to_execute(&self) -> Option<&str> {
        self.confirm_token.as_deref().filter(|_| !self.read_only)
    }

    /// One line for the output of the console.
    pub fn summary(&self) -> String {
        if self.read_only {
            format!("{} — {} ligne(s) ({} ms)", self.statement_type, self.rows.as_ref().map_or(0, Vec::len), self.elapsed_ms)
        } else {
            format!("{} — aperçu écriture ({} ms)", self.statement_type, self.elapsed_ms)
        }
    }

    /// What a write would touch, in words.
    pub fn estimate(&self) -> String {
        match (self.read_only, self.rows_affected) {
            (true, _) => "Lecture seule — résultat ci-dessous".into(),
            (false, Some(rows)) => format!("~{rows} ligne(s) estimée(s)"),
            (false, None) => "Écriture — estimation indisponible".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SqlExecuted {
    pub rows_affected: i64,
    pub columns: Option<Vec<String>>,
    pub rows: Option<Vec<Row>>,
    pub elapsed_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleared {
    pub table: String,
    #[serde(default)]
    pub rows_affected: i64,
}

pub async fn list_tables(client: &Client) -> ApiResult<Vec<TableInfo>> {
    client.get("/api/data/tables").await
}

/// One page of a table; `q` searches its text columns.
pub async fn preview_table(client: &Client, name: &str, q: &str, limit: i64, offset: i64) -> ApiResult<TablePreview> {
    let mut query = format!("limit={limit}&offset={offset}");
    let q = q.trim();
    if !q.is_empty() {
        query.push_str(&format!("&q={}", encode(q)));
    }
    client.get(format!("/api/data/tables/{}/preview?{query}", encode(name))).await
}

/// What emptying a table would remove, and the token to do it with.
pub async fn impact_clear(client: &Client, name: &str) -> ApiResult<ImpactReport> {
    client.get(format!("/api/data/tables/{}/impact/clear", encode(name))).await
}

pub async fn clear_table(client: &Client, name: &str, confirm_token: &str) -> ApiResult<Cleared> {
    client.post(format!("/api/data/tables/{}/clear", encode(name)), &serde_json::json!({ "confirmToken": confirm_token })).await
}

/// Say what one SQL statement is and would do. A read is run; a write is not.
pub async fn preview_sql(client: &Client, sql: &str) -> ApiResult<SqlPreview> {
    client.post("/api/data/sql/preview", &serde_json::json!({ "sql": sql })).await
}

/// Run a write that was previewed, with the token its preview gave.
pub async fn execute_sql(client: &Client, sql: &str, confirm_token: &str) -> ApiResult<SqlExecuted> {
    client.call(Method::POST, "/api/data/sql/execute").json(&serde_json::json!({ "sql": sql, "confirmToken": confirm_token })).send().await
}

/// A value as a cell shows it: nothing for an absent one, text as it is, a list or an object
/// as JSON.
pub fn cell_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        nested => Some(nested.to_string()),
    }
}

/// Whether a column holds identifiers, shown in fixed-width type.
pub fn is_id_column(name: &str) -> bool {
    name == "id" || name.ends_with("_id")
}

/// Rows as a CSV text: the columns in their order, one line per row.
pub fn rows_to_csv(columns: &[String], rows: &[Row]) -> String {
    let lines: Vec<Vec<String>> =
        rows.iter().map(|row| columns.iter().map(|column| cell_text(row.get(column)).unwrap_or_default()).collect()).collect();
    to_csv(columns, &lines, ',')
}

/// Where a page stands: "101–200 sur 437". With a search, the table's count is not the
/// number of rows found, so the page only says what it holds.
pub fn page_label(offset: i64, shown: usize, total: i64, searching: bool) -> String {
    if shown == 0 {
        return if searching && total > 0 { "Aucune ligne trouvée".into() } else { "Aucune ligne".into() };
    }
    let (first, last) = (offset + 1, offset + shown as i64);
    if searching { format!("Lignes trouvées {first}–{last} · {total} dans la table") } else { format!("{first}–{last} sur {total}") }
}

/// Whether there may be a page after this one. With a search, a full page is the only sign.
pub fn has_next_page(offset: i64, shown: usize, total: i64, searching: bool, page_size: i64) -> bool {
    if searching { shown as i64 >= page_size } else { offset + page_size < total }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(value: Value) -> Row {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn a_cell_shows_text_as_it_is_nothing_for_null_and_json_for_the_rest() {
        let record = row(json!({ "name": "Léa", "count": 3, "active": true, "gone": null, "tags": ["a", "b"] }));
        assert_eq!(cell_text(record.get("name")).as_deref(), Some("Léa"));
        assert_eq!(cell_text(record.get("count")).as_deref(), Some("3"));
        assert_eq!(cell_text(record.get("active")).as_deref(), Some("true"));
        assert_eq!(cell_text(record.get("gone")), None);
        assert_eq!(cell_text(record.get("missing")), None);
        assert_eq!(cell_text(record.get("tags")).as_deref(), Some("[\"a\",\"b\"]"));
    }

    #[test]
    fn a_page_is_written_as_csv_in_the_order_of_its_columns() {
        let columns = vec!["id".to_string(), "name".to_string(), "notes".to_string()];
        let rows = [row(json!({ "name": "Martin, Léa", "id": "c-1", "notes": null })), row(json!({ "id": "c-2", "name": "Dubois" }))];
        assert_eq!(rows_to_csv(&columns, &rows), "id,name,notes\r\nc-1,\"Martin, Léa\",\r\nc-2,Dubois,");
        assert!(is_id_column("id") && is_id_column("client_id") && !is_id_column("identity"));
    }

    #[test]
    fn a_page_says_where_it_stands() {
        assert_eq!(page_label(0, 100, 437, false), "1–100 sur 437");
        assert_eq!(page_label(400, 37, 437, false), "401–437 sur 437");
        assert_eq!(page_label(0, 0, 0, false), "Aucune ligne");
        assert!(has_next_page(0, 100, 437, false, 100) && !has_next_page(400, 37, 437, false, 100));
        // With a search, the table's count is not the number of rows found.
        assert_eq!(page_label(0, 3, 437, true), "Lignes trouvées 1–3 · 437 dans la table");
        assert_eq!(page_label(0, 0, 437, true), "Aucune ligne trouvée");
        assert!(!has_next_page(0, 3, 437, true, 100) && has_next_page(0, 100, 437, true, 100));
    }

    #[test]
    fn a_preview_says_what_the_statement_is_and_only_a_write_can_be_run() {
        let read: SqlPreview = serde_json::from_value(json!({
            "statementType": "SELECT", "readOnly": true, "rowsAffected": null,
            "columns": ["id"], "rows": [{ "id": "a" }, { "id": "b" }], "sampleRows": null,
            "message": null, "elapsedMs": 4, "confirmToken": null,
        }))
        .unwrap();
        assert_eq!(read.summary(), "SELECT — 2 ligne(s) (4 ms)");
        assert_eq!(read.estimate(), "Lecture seule — résultat ci-dessous");
        assert_eq!(read.token_to_execute(), None);

        let write: SqlPreview = serde_json::from_value(json!({
            "statementType": "UPDATE", "readOnly": false, "rowsAffected": 12, "elapsedMs": 7, "confirmToken": "tok",
        }))
        .unwrap();
        assert_eq!(write.summary(), "UPDATE — aperçu écriture (7 ms)");
        assert_eq!(write.estimate(), "~12 ligne(s) estimée(s)");
        assert_eq!(write.token_to_execute(), Some("tok"));
        assert_eq!(SqlPreview { read_only: false, ..Default::default() }.estimate(), "Écriture — estimation indisponible");
    }
}
