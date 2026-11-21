//! Register, closures, archives and the integrity checker.
//!
//! Everything here is either a control an inspector would ask to see, or the
//! button that produces the evidence for it.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, NaiveDate, Utc};
use motodesk_fiscal::ClosureKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::local_server::auth_guard::AdminUser;
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::fiscal::{self, archive, closures, mentions};
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/fiscal/register", get(register_info))
        .route("/api/fiscal/journal", get(journal))
        .route("/api/fiscal/verify", post(verify))
        .route("/api/fiscal/closures", get(list_closures).post(close))
        .route("/api/fiscal/closures/pending", get(pending))
        .route(
            "/api/fiscal/archives",
            get(list_archives).post(create_archive),
        )
        .route("/api/fiscal/archives/:id/verify", post(verify_archive))
        .route("/api/fiscal/profile", get(profile))
}

async fn register_info(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let register = fiscal::current(&state).await?;
    let info = fiscal::info(&state, &register).await?;
    Ok(Json(serde_json::to_value(info).unwrap_or(Value::Null)))
}

/// The mandatory-mentions status, so the UI can warn before the user has typed
/// a whole invoice they then cannot validate.
async fn profile(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let loaded = mentions::load(&state.db).await?;
    let check = mentions::check(&loaded);
    let mut out = serde_json::to_value(&check).unwrap_or(json!({}));
    if let Some(obj) = out.as_object_mut() {
        obj.insert(
            "payment_terms_text".into(),
            json!(mentions::payment_terms_text(&loaded)),
        );
    }
    Ok(Json(out))
}

#[derive(Debug, Deserialize, Default)]
pub struct JournalQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub event_type: Option<String>,
    pub sale_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct JournalItem {
    seq: i64,
    register_code: String,
    event_type: String,
    occurred_at: String,
    business_day: String,
    document_ref: Option<String>,
    sale_id: Option<String>,
    total_ttc_cents: i64,
    chain_hash: String,
    payload: String,
}

async fn journal(
    State(state): State<SharedState>,
    Query(q): Query<JournalQuery>,
) -> LocalResult<Json<Vec<JournalItem>>> {
    let limit = q.limit.unwrap_or(200).clamp(1, 2000);
    let offset = q.offset.unwrap_or(0).max(0);

    let rows: Vec<JournalItem> = sqlx::query_as(
        r#"
        SELECT j.seq, r.code AS register_code, j.event_type, j.occurred_at, j.business_day,
               j.document_ref, j.sale_id, j.total_ttc_cents, j.chain_hash, j.payload
        FROM fiscal_journal j
        JOIN fiscal_registers r ON r.id = j.register_id
        WHERE ($1 IS NULL OR j.business_day >= $1)
          AND ($2 IS NULL OR j.business_day <= $2)
          AND ($3 IS NULL OR j.event_type = $3)
          AND ($4 IS NULL OR j.sale_id = $4)
        ORDER BY j.occurred_at DESC, j.seq DESC
        LIMIT $5 OFFSET $6
        "#,
    )
    .bind(&q.from)
    .bind(&q.to)
    .bind(&q.event_type)
    .bind(&q.sale_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

async fn verify(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let report = fiscal::verify_all(&state).await?;
    // The verdict is itself traced (§ 240). Best effort: the check has to be able to
    // run, and report, precisely when the register's key or chain is in trouble.
    let problems: Vec<String> = report
        .registers
        .iter()
        .flat_map(|r| r.problem.iter().chain(r.findings.iter().skip(1)))
        .cloned()
        .collect();
    let _ = fiscal::journal_event(
        &state,
        motodesk_fiscal::FiscalEventType::IntegrityChecked,
        serde_json::json!({ "ok": report.ok, "registers": report.registers.len(), "problems": problems }),
    )
    .await;
    Ok(Json(serde_json::to_value(report).unwrap_or(Value::Null)))
}

#[derive(Debug, Deserialize)]
pub struct CloseBody {
    /// `day`, `month` or `year`.
    pub kind: String,
    /// Any date inside the period. Defaults to the latest period that is over:
    /// yesterday for a day, last month for a month, last year for a year.
    pub on: Option<String>,
}

async fn close(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<CloseBody>,
) -> LocalResult<Json<Value>> {
    let kind = ClosureKind::parse(&body.kind)
        .ok_or_else(|| LocalError::BadRequest("clôture inconnue : jour, mois ou année".into()))?;

    let on = match body.on.as_deref() {
        Some(text) => NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .map_err(|_| LocalError::BadRequest("date attendue au format AAAA-MM-JJ".into()))?,
        None => default_closure_day(kind, Utc::now().with_timezone(&chrono::Local).date_naive()),
    };

    let register = fiscal::current(&state).await?;
    let row = closures::close(&state, &register, kind, on).await?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

/// A date inside the latest period of `kind` that is over: yesterday, last month,
/// last year. Sending no date used to mean "yesterday" for every kind, which lies
/// inside the running month and year on all but one day, so the monthly and annual
/// buttons failed on every other day.
pub(crate) fn default_closure_day(kind: ClosureKind, today: NaiveDate) -> NaiveDate {
    match kind {
        ClosureKind::Day => today.pred_opt().unwrap_or(today),
        ClosureKind::Month => today.with_day(1).and_then(|d| d.pred_opt()).unwrap_or(today),
        ClosureKind::Year => NaiveDate::from_ymd_opt(today.year() - 1, 12, 31).unwrap_or(today),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct ClosureQuery {
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

async fn list_closures(
    State(state): State<SharedState>,
    Query(q): Query<ClosureQuery>,
) -> LocalResult<Json<Value>> {
    let rows = closures::list(
        &state,
        q.kind.as_deref(),
        q.limit.unwrap_or(60).clamp(1, 500),
    )
    .await?;
    Ok(Json(serde_json::to_value(rows).unwrap_or(Value::Null)))
}

async fn pending(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let register = fiscal::current(&state).await?;
    let days = closures::pending_days(&state, &register).await?;
    Ok(Json(json!({ "days": days })))
}

async fn list_archives(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let rows = archive::list(&state).await?;
    Ok(Json(serde_json::to_value(rows).unwrap_or(Value::Null)))
}

#[derive(Debug, Deserialize)]
pub struct ArchiveBody {
    pub year: i32,
}

async fn create_archive(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<ArchiveBody>,
) -> LocalResult<Json<Value>> {
    let register = fiscal::current(&state).await?;
    let row = archive::create(&state, &register, body.year).await?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

async fn verify_archive(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> LocalResult<Json<Value>> {
    Ok(Json(archive::verify(&state, &id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::auth_guard::require_admin;
    use crate::local_server::error::LocalError;
    use crate::local_server::test_support::shop;
    use serde_json::json;

    #[test]
    fn the_buttons_without_a_date_close_the_latest_finished_period() {
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        for today in [day(2026, 11, 11), day(2026, 11, 2), day(2026, 11, 30)] {
            assert_eq!(default_closure_day(ClosureKind::Day, today), today.pred_opt().unwrap());
            assert_eq!(default_closure_day(ClosureKind::Month, today), day(2026, 10, 31));
            assert_eq!(default_closure_day(ClosureKind::Year, today), day(2025, 12, 31));
        }
        assert_eq!(default_closure_day(ClosureKind::Month, day(2026, 1, 15)), day(2025, 12, 31));
        assert_eq!(default_closure_day(ClosureKind::Day, day(2026, 3, 1)), day(2026, 2, 28));
    }

    #[tokio::test]
    async fn employee_cannot_close_or_archive() {
        let state = shop().await;
        let user = json!({
            "id": "u-emp",
            "email": "caisse@test.fr",
            "full_name": "Caissier",
            "role": "employee"
        });
        sqlx::query(
            "INSERT OR REPLACE INTO cached_session (id, user_json, tokens_json, cached_at)
             VALUES (1, $1, '{}', $2)",
        )
        .bind(user.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&state.db)
        .await
        .unwrap();

        assert!(
            matches!(require_admin(&state).await, Err(LocalError::Forbidden)),
            "the cashier must not reach close or archive"
        );
    }
}
