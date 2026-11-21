//! The read-only Comptabilité module.
//!
//! Motodesk is not the book of record; the expert-comptable's software is. What
//! this offers is the four things a shop is repeatedly asked for between two
//! visits to the accountant: the sales journal, the cash journal, the VAT recap
//! that fills the CA3 boxes, and the ageing balance of who still owes what.
//! Plus a FEC export, because that is the one shape every French accounting
//! package can import and the one the inspector can demand under LPF art. L47 A.
//!
//! Nothing here writes. Every figure comes from documents already sealed into
//! the journal, so the module cannot disagree with the invoices.

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::local_server::error::LocalResult;
use crate::local_server::fiscal::{self, fec, mentions};
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/accounting/sales-journal", get(sales_journal))
        .route("/api/accounting/cash-journal", get(cash_journal))
        .route("/api/accounting/vat-recap", get(vat_recap))
        .route("/api/accounting/ageing", get(ageing))
        .route("/api/accounting/fec", get(fec_export))
}

#[derive(Debug, Deserialize, Default)]
pub struct PeriodQuery {
    pub from: Option<String>,
    pub to: Option<String>,
}

impl PeriodQuery {
    /// Defaults to the current calendar year, which is what someone opening the
    /// screen almost always wants.
    fn bounds(&self) -> (String, String) {
        let year = Utc::now().year();
        (
            self.from.clone().unwrap_or_else(|| format!("{year}-01-01")),
            self.to.clone().unwrap_or_else(|| format!("{year}-12-31")),
        )
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct SalesJournalRow {
    reference: Option<String>,
    document_kind: String,
    issued_on: String,
    client_name: Option<String>,
    total_ht_cents: i64,
    total_vat_cents: i64,
    total_ttc_cents: i64,
    paid_cents: i64,
    fiscal_seq: Option<i64>,
}

async fn sales_journal(
    State(state): State<SharedState>,
    Query(q): Query<PeriodQuery>,
) -> LocalResult<Json<Value>> {
    let (from, to) = q.bounds();

    let rows: Vec<SalesJournalRow> = sqlx::query_as(
        r#"
        SELECT s.reference,
               COALESCE(s.document_kind, 'invoice') AS document_kind,
               substr(COALESCE(s.locked_at, s.sale_date, ''), 1, 10) AS issued_on,
               TRIM(COALESCE(c.last_name, '') || ' ' || COALESCE(c.first_name, '')) AS client_name,
               COALESCE(s.total_ht_cents, 0)  AS total_ht_cents,
               COALESCE(s.total_vat_cents, 0) AS total_vat_cents,
               COALESCE(s.total_ttc_cents, 0) AS total_ttc_cents,
               COALESCE(p.paid, 0)            AS paid_cents,
               s.fiscal_seq
        FROM sales s
        LEFT JOIN clients c ON c.id = s.client_id
        LEFT JOIN (
            SELECT sale_id, SUM(amount_cents) AS paid FROM payments
            WHERE deleted_at IS NULL GROUP BY sale_id
        ) p ON p.sale_id = s.id
        WHERE s.locked_at IS NOT NULL AND s.deleted_at IS NULL
          AND substr(s.locked_at, 1, 10) BETWEEN $1 AND $2
        ORDER BY s.locked_at ASC
        "#,
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&state.db)
    .await?;

    let total_ht: i64 = rows.iter().map(|r| r.total_ht_cents).sum();
    let total_vat: i64 = rows.iter().map(|r| r.total_vat_cents).sum();
    let total_ttc: i64 = rows.iter().map(|r| r.total_ttc_cents).sum();

    Ok(Json(json!({
        "from": from,
        "to": to,
        "rows": rows,
        "total_ht_cents": total_ht,
        "total_vat_cents": total_vat,
        "total_ttc_cents": total_ttc,
    })))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct CashJournalRow {
    id: String,
    paid_on: String,
    reference: Option<String>,
    client_name: Option<String>,
    method: Option<String>,
    amount_cents: i64,
    reverses_payment_id: Option<String>,
}

async fn cash_journal(
    State(state): State<SharedState>,
    Query(q): Query<PeriodQuery>,
) -> LocalResult<Json<Value>> {
    let (from, to) = q.bounds();

    let rows: Vec<CashJournalRow> = sqlx::query_as(
        r#"
        SELECT p.id,
               substr(COALESCE(p.paid_at, ''), 1, 10) AS paid_on,
               s.reference,
               TRIM(COALESCE(c.last_name, '') || ' ' || COALESCE(c.first_name, '')) AS client_name,
               p.method,
               p.amount_cents,
               p.reverses_payment_id
        FROM payments p
        LEFT JOIN sales s   ON s.id = p.sale_id
        LEFT JOIN clients c ON c.id = p.client_id
        WHERE p.deleted_at IS NULL
          AND substr(COALESCE(p.paid_at, ''), 1, 10) BETWEEN $1 AND $2
        ORDER BY p.paid_at ASC
        "#,
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&state.db)
    .await?;

    // Grouped by tender so the drawer can be counted against the software.
    let mut by_method: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    for row in &rows {
        *by_method
            .entry(row.method.clone().unwrap_or_else(|| "autre".into()))
            .or_insert(0) += row.amount_cents;
    }

    Ok(Json(json!({
        "from": from,
        "to": to,
        "rows": rows,
        "total_cents": rows.iter().map(|r| r.amount_cents).sum::<i64>(),
        "by_method": by_method,
    })))
}

/// VAT collected per rate, the figures that go into the CA3 return.
async fn vat_recap(
    State(state): State<SharedState>,
    Query(q): Query<PeriodQuery>,
) -> LocalResult<Json<Value>> {
    let (from, to) = q.bounds();

    // Read from the journal payloads rather than from sale_lines: the payload is
    // the frozen version, and a line table can in principle drift from it.
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT payload FROM fiscal_journal
        WHERE event_type IN ('invoice', 'receipt', 'credit_note')
          AND business_day BETWEEN $1 AND $2
        ORDER BY seq ASC
        "#,
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&state.db)
    .await?;

    let mut by_rate: std::collections::BTreeMap<i64, (i64, i64)> =
        std::collections::BTreeMap::new();
    let mut document_count = 0i64;

    for (payload,) in rows {
        let payload: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
        document_count += 1;
        if let Some(buckets) = payload.get("vat_breakdown").and_then(|v| v.as_array()) {
            for bucket in buckets {
                let rate = bucket.get("rate_bp").and_then(|v| v.as_i64()).unwrap_or(0);
                let slot = by_rate.entry(rate).or_insert((0, 0));
                slot.0 += bucket
                    .get("base_ht_cents")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                slot.1 += bucket
                    .get("vat_cents")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
            }
        }
    }

    let lines: Vec<Value> = by_rate
        .iter()
        .map(|(rate, (base, vat))| {
            json!({
                "rate_bp": rate,
                "base_ht_cents": base,
                "vat_cents": vat,
                "total_ttc_cents": base + vat,
            })
        })
        .collect();

    Ok(Json(json!({
        "from": from,
        "to": to,
        "document_count": document_count,
        "rates": lines,
        "total_base_ht_cents": by_rate.values().map(|(b, _)| b).sum::<i64>(),
        "total_vat_cents": by_rate.values().map(|(_, v)| v).sum::<i64>(),
    })))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct AgeingRow {
    sale_id: String,
    reference: Option<String>,
    client_name: Option<String>,
    issued_on: String,
    total_ttc_cents: i64,
    paid_cents: i64,
    outstanding_cents: i64,
    days_overdue: i64,
}

/// Who owes what, bucketed the way a credit-control conversation goes.
async fn ageing(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let profile = mentions::load(&state.db).await?;
    let terms = profile.payment_terms_days.max(0);
    let today = fiscal::business_day(Utc::now());

    let rows: Vec<AgeingRow> = sqlx::query_as(
        r#"
        SELECT s.id AS sale_id,
               s.reference,
               TRIM(COALESCE(c.last_name, '') || ' ' || COALESCE(c.first_name, '')) AS client_name,
               substr(COALESCE(s.locked_at, s.sale_date, ''), 1, 10) AS issued_on,
               COALESCE(s.total_ttc_cents, 0) AS total_ttc_cents,
               COALESCE(p.paid, 0)            AS paid_cents,
               COALESCE(s.total_ttc_cents, 0) + COALESCE((SELECT SUM(c.total_ttc_cents) FROM sales c
                    WHERE c.corrects_sale_id = s.id AND c.locked_at IS NOT NULL
                      AND c.deleted_at IS NULL), 0) - COALESCE(p.paid, 0) AS outstanding_cents,
               CAST(julianday($1) - julianday(substr(COALESCE(s.locked_at, s.sale_date, $1), 1, 10)) AS INTEGER) - $2
                   AS days_overdue
        FROM sales s
        LEFT JOIN clients c ON c.id = s.client_id
        LEFT JOIN (
            SELECT sale_id, SUM(amount_cents) AS paid FROM payments
            WHERE deleted_at IS NULL GROUP BY sale_id
        ) p ON p.sale_id = s.id
        WHERE s.locked_at IS NOT NULL AND s.deleted_at IS NULL
          AND COALESCE(s.document_kind, 'invoice') = 'invoice'
          AND COALESCE(s.total_ttc_cents, 0) + COALESCE((SELECT SUM(c.total_ttc_cents) FROM sales c
                    WHERE c.corrects_sale_id = s.id AND c.locked_at IS NOT NULL
                      AND c.deleted_at IS NULL), 0) - COALESCE(p.paid, 0) > 0
        ORDER BY days_overdue DESC
        "#,
    )
    .bind(&today)
    .bind(terms)
    .fetch_all(&state.db)
    .await?;

    let bucket = |low: i64, high: Option<i64>| -> i64 {
        rows.iter()
            .filter(|r| r.days_overdue >= low && high.is_none_or(|h| r.days_overdue <= h))
            .map(|r| r.outstanding_cents)
            .sum()
    };

    Ok(Json(json!({
        "as_of": today,
        "payment_terms_days": terms,
        "rows": rows,
        "total_outstanding_cents": rows.iter().map(|r| r.outstanding_cents).sum::<i64>(),
        "buckets": {
            "not_due_cents": rows.iter().filter(|r| r.days_overdue < 0).map(|r| r.outstanding_cents).sum::<i64>(),
            "overdue_0_30_cents": bucket(0, Some(30)),
            "overdue_31_60_cents": bucket(31, Some(60)),
            "overdue_61_90_cents": bucket(61, Some(90)),
            "overdue_90_plus_cents": bucket(91, None),
        },
    })))
}

#[derive(Debug, Deserialize)]
pub struct FecQuery {
    pub year: Option<i32>,
}

async fn fec_export(
    State(state): State<SharedState>,
    Query(q): Query<FecQuery>,
) -> LocalResult<Json<Value>> {
    let year = q.year.unwrap_or_else(|| Utc::now().year() - 1);
    let register = fiscal::current(&state).await?;
    let profile = mentions::load(&state.db).await?;

    let export = fec::build(&state.db, &register, year, &profile.siren).await?;
    Ok(Json(json!({
        "file_name": export.file_name,
        "line_count": export.line_count,
        "content": export.content,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::routes::sales::tests::{new_draft, pay, seal};
    use crate::local_server::test_support::shop;

    fn today() -> PeriodQuery {
        PeriodQuery::default()
    }

    /// Two invoices of 252.00 TTC, one of them settled in cash.
    async fn a_morning() -> SharedState {
        let state = shop().await;
        for index in 0..2 {
            let id = new_draft(&state).await;
            seal(&state, id).await;
            if index == 0 {
                pay(&state, id, 25_200, "cash").await;
            }
        }
        state
    }

    #[tokio::test]
    async fn the_sales_journal_reports_what_was_invoiced_and_settled() {
        let state = a_morning().await;
        let Json(report) = sales_journal(State(state), Query(today())).await.unwrap();

        assert_eq!(report["rows"].as_array().unwrap().len(), 2);
        assert_eq!(report["total_ht_cents"], 42_000);
        assert_eq!(report["total_vat_cents"], 8_400);
        assert_eq!(report["total_ttc_cents"], 50_400);
        assert_eq!(report["rows"][0]["paid_cents"], 25_200);
        assert_eq!(report["rows"][1]["paid_cents"], 0);
    }

    #[tokio::test]
    async fn the_cash_journal_splits_the_takings_by_tender() {
        let state = a_morning().await;
        let Json(report) = cash_journal(State(state), Query(today())).await.unwrap();

        assert_eq!(report["total_cents"], 25_200);
        assert_eq!(report["by_method"]["cash"], 25_200);
    }

    /// The recap is what fills the CA3 boxes, so its bases and VAT must be the
    /// ones frozen in the journal, not a recomputation that could drift.
    #[tokio::test]
    async fn the_vat_recap_groups_the_frozen_bases_per_rate() {
        let state = a_morning().await;
        let Json(report) = vat_recap(State(state), Query(today())).await.unwrap();

        assert_eq!(report["document_count"], 2);
        assert_eq!(report["total_base_ht_cents"], 42_000);
        assert_eq!(report["total_vat_cents"], 8_400);

        let rates = report["rates"].as_array().unwrap();
        assert_eq!(rates.len(), 1);
        assert_eq!(rates[0]["rate_bp"], 2_000);
        assert_eq!(rates[0]["total_ttc_cents"], 50_400);
    }

    #[tokio::test]
    async fn the_ageing_balance_lists_only_what_is_still_owed() {
        let state = a_morning().await;
        let Json(report) = ageing(State(state)).await.unwrap();

        let rows = report["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "the settled invoice is off the balance");
        assert_eq!(report["total_outstanding_cents"], 25_200);

        // Issued today, so it sits in the not-yet-due bucket whatever the terms.
        assert_eq!(report["buckets"]["not_due_cents"], 25_200);
        assert_eq!(report["buckets"]["overdue_90_plus_cents"], 0);
    }

    #[tokio::test]
    async fn an_empty_period_reports_zeroes_rather_than_failing() {
        let state = shop().await;
        let empty = PeriodQuery {
            from: Some("2000-01-01".into()),
            to: Some("2000-12-31".into()),
        };

        let Json(sales) = sales_journal(State(state.clone()), Query(empty))
            .await
            .unwrap();
        assert_eq!(sales["total_ttc_cents"], 0);
        assert!(sales["rows"].as_array().unwrap().is_empty());

        let Json(ledger) = ageing(State(state)).await.unwrap();
        assert_eq!(ledger["total_outstanding_cents"], 0);
    }
}
