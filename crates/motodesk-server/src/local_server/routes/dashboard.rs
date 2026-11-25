use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_server::error::LocalResult;
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/dashboard/stats", get(stats))
        .route("/api/dashboard/recent", get(recent))
}

#[derive(Debug, Deserialize)]
struct RecentQuery {
    limit: Option<i64>,
}

async fn stats(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let (active_clients,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM clients WHERE deleted_at IS NULL")
            .fetch_one(&state.db)
            .await?;
    let (vehicles_in_workshop,): (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM workshop_orders
           WHERE deleted_at IS NULL AND status IN ('pending','in_progress','ready')"#,
    )
    .fetch_one(&state.db)
    .await?;
    let (stock_available,): (i64,) = sqlx::query_as(
        r#"SELECT CAST(COALESCE(SUM(stock_milli), 0) / 1000 AS INTEGER) FROM products
           WHERE deleted_at IS NULL AND is_service = 0 AND is_active = 1"#,
    )
    .fetch_one(&state.db)
    .await?;

    // Month boundaries are the shop's calendar, not UTC's.
    let month_start = chrono::Local::now().format("%Y-%m-01").to_string();
    let revenue = revenue_since(&state, &month_start).await?;

    Ok(Json(json!({
        "active_clients": active_clients,
        "vehicles_in_workshop": vehicles_in_workshop,
        "stock_available": stock_available,
        "revenue_this_month_cents": revenue,
    })))
}

/// Sealed documents since `since`, credit notes included. A credit note is stored
/// with a negative total, so it is added like any other document; subtracting it
/// (as this once did) turned every refund into extra revenue.
async fn revenue_since(state: &SharedState, since: &str) -> LocalResult<i64> {
    let (revenue,): (i64,) = sqlx::query_as(
        r#"SELECT CAST(COALESCE(SUM(total_ttc_cents), 0) AS INTEGER) FROM sales
           WHERE deleted_at IS NULL AND locked_at IS NOT NULL
             AND sale_date >= $1 AND document_kind != 'quote'"#,
    )
    .bind(since)
    .fetch_one(&state.db)
    .await?;
    Ok(revenue)
}

async fn recent(
    State(state): State<SharedState>,
    Query(q): Query<RecentQuery>,
) -> LocalResult<Json<Value>> {
    let limit = q.limit.unwrap_or(10).clamp(1, 50);
    let rows: Vec<(
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    )> = sqlx::query_as(
        r#"
        SELECT s.id, s.reference, s.document_kind, s.status,
               TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')),
               CASE WHEN v.id IS NULL THEN NULL ELSE TRIM(v.make || ' ' || v.model) END,
               COALESCE(s.sale_date, s.created_at)
        FROM sales s
        LEFT JOIN clients c ON c.id = s.client_id
        LEFT JOIN vehicles v ON v.id = s.vehicle_id
        WHERE s.deleted_at IS NULL
        ORDER BY s.updated_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, reference, kind, status, client_name, vehicle, created_at)| {
                let doc_type = match kind.as_str() {
                    "quote" => "DEVIS",
                    "credit_note" => "AVOIR",
                    "receipt" => "TICKET",
                    _ => "FACTURE",
                };
                let fallback_ref: String = id.chars().take(8).collect();
                json!({
                    "id": id,
                    "doc_number": reference.unwrap_or(fallback_ref),
                    "doc_type": doc_type,
                    "client_name": client_name.filter(|s| !s.is_empty()),
                    "vehicle_summary": vehicle.filter(|s| !s.is_empty()),
                    "status": status.unwrap_or(kind),
                    "created_at": created_at,
                })
            },
        )
        .collect();
    Ok(Json(json!(items)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::shop;

    async fn sealed(state: &SharedState, kind: &str, total: i64) {
        sqlx::query(
            "INSERT INTO sales (id, status, document_kind, sale_date, total_ttc_cents, locked_at,
                                created_at, updated_at, row_version, field_meta)
             VALUES ($1, 'paid', $2, '2026-11-05', $3, '2026-11-05T10:00:00Z',
                     '2026-11-05T10:00:00Z', '2026-11-05T10:00:00Z', 1, '{}')",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(kind)
        .bind(total)
        .execute(&state.db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_credit_note_takes_revenue_off_instead_of_adding_to_it() {
        let state = shop().await;
        sealed(&state, "invoice", 10_000).await;
        sealed(&state, "credit_note", -3_000).await;
        sealed(&state, "quote", 99_999).await;
        assert_eq!(revenue_since(&state, "2026-11-01").await.unwrap(), 7_000);
    }
}
