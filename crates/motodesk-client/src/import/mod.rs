//! Importing a CSV file (port of `desktop/src/lib/api/import.ts` and `desktop/src/lib/import`).
//!
//! The file is read, mapped and typed on this side ([`csv`], [`schema`], [`transforms`]); the
//! service has the last word: it checks the rows again, finds the rows they point at, decides
//! for each whether it is added, updated or skipped, and writes them.

pub mod csv;
pub mod schema;
pub mod transforms;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::ApiResult;
use crate::session::Client;
use schema::Mode;

/// How many rows go in one request. A file of tens of thousands of rows sent at once would
/// be larger than the service accepts.
pub const BATCH_SIZE: usize = 1000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub entity: &'static str,
    pub mode: Mode,
    /// The field that tells an existing row from a new one.
    pub match_key: String,
    /// Add every row without looking for an existing one: a conflict comes back as an error
    /// on its row.
    pub ignore_duplicates: bool,
    /// For products: create the categories named in the file that do not exist yet.
    pub create_missing_categories: bool,
    /// For each reference, the column of the other table its value is looked up by.
    pub references: BTreeMap<String, String>,
    pub rows: Vec<Map<String, Value>>,
}

impl ImportRequest {
    /// The same request for one batch of rows.
    pub fn with_rows(&self, rows: &[Map<String, Value>]) -> Self {
        Self { rows: rows.to_vec(), ..self.clone() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposition {
    Insert,
    Update,
    Skip,
    /// Refused, or anything this version does not know.
    #[serde(other)]
    Error,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RowOutcome {
    /// The position of the row among the rows sent.
    pub row: usize,
    pub disposition: Disposition,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct ImportResult {
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errored: usize,
    pub rows: Vec<RowOutcome>,
}

impl ImportResult {
    /// Add the answer for a batch that started at row `offset` of what is being sent: its
    /// rows keep their place in the whole.
    pub fn absorb(&mut self, batch: ImportResult, offset: usize) {
        self.inserted += batch.inserted;
        self.updated += batch.updated;
        self.skipped += batch.skipped;
        self.errored += batch.errored;
        self.rows.extend(batch.rows.into_iter().map(|outcome| RowOutcome { row: outcome.row + offset, ..outcome }));
    }

    /// The rows the service refused.
    pub fn errors(&self) -> impl Iterator<Item = &RowOutcome> {
        self.rows.iter().filter(|outcome| outcome.disposition == Disposition::Error)
    }
}

/// Check the rows against the database, find what they point at and say what would happen to
/// each, without writing anything.
pub async fn preview(client: &Client, request: &ImportRequest) -> ApiResult<ImportResult> {
    client.post("/api/import/preview", request).await
}

/// The same, written: the rows go in and are sent to the other PCs and the cloud.
pub async fn commit(client: &Client, request: &ImportRequest) -> ApiResult<ImportResult> {
    client.post("/api/import/commit", request).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_request_is_sent_in_the_shape_the_service_reads() {
        let mut row = Map::new();
        row.insert("last_name".into(), json!("Martin"));
        let request = ImportRequest {
            entity: "clients",
            mode: Mode::Upsert,
            match_key: "email".into(),
            ignore_duplicates: false,
            create_missing_categories: true,
            references: BTreeMap::from([("owner".to_string(), "email".to_string())]),
            rows: vec![row],
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "entity": "clients",
                "mode": "upsert",
                "matchKey": "email",
                "ignoreDuplicates": false,
                "createMissingCategories": true,
                "references": { "owner": "email" },
                "rows": [{ "last_name": "Martin" }],
            })
        );
        assert!(request.with_rows(&[]).rows.is_empty());
    }

    #[test]
    fn the_answers_of_the_batches_add_up_and_rows_keep_their_place() {
        let batch = |row: usize, disposition: &str| -> ImportResult {
            serde_json::from_value(json!({
                "inserted": 1, "updated": 0, "skipped": 0, "errored": 1,
                "rows": [
                    { "row": row, "disposition": disposition, "message": null, "id": "a" },
                    { "row": row + 1, "disposition": "error", "message": "Référence déjà utilisée", "id": null },
                ],
            }))
            .unwrap()
        };
        let mut whole = ImportResult::default();
        whole.absorb(batch(0, "insert"), 0);
        whole.absorb(batch(0, "something new"), BATCH_SIZE);
        assert_eq!((whole.inserted, whole.errored, whole.rows.len()), (2, 2, 4));
        // An answer this version does not know counts as refused, and is where it was sent from.
        assert_eq!(whole.errors().map(|outcome| outcome.row).collect::<Vec<_>>(), [1, BATCH_SIZE, BATCH_SIZE + 1]);
    }
}
