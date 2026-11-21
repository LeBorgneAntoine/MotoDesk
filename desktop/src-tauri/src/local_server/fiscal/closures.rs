//! Daily, monthly and annual closures.
//!
//! § 170 of BOI-TVA-DECLA-30-10-30 makes all three levels "cumulatives et
//! impératives": each one totals the period, carries the running total for the
//! accounting period, and carries the perpetual total since the software was
//! first used. § 220 forbids ever purging them, which is why the closure rows
//! survive an archive-and-purge of the detail behind them.
//!
//! A closure is computed from the journal, never from the working tables. That
//! is what makes it reproducible: run it again on a restored database and it
//! must produce the same figures.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate, Utc};
use motodesk_fiscal::{ClosureKind, ClosureTotals, FiscalEventType, VatBucket};
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::fiscal::{self, EntryLinks, Register};
use crate::local_server::state::SharedState;

#[derive(Debug, Serialize)]
pub struct ClosureRow {
    pub id: String,
    pub kind: String,
    pub period_start: String,
    pub period_end: String,
    pub document_count: i64,
    pub total_ht_cents: i64,
    pub total_vat_cents: i64,
    pub total_ttc_cents: i64,
    pub vat_breakdown: Value,
    pub period_grand_total_cents: i64,
    pub perpetual_total_cents: i64,
    pub journal_seq: i64,
    pub closed_at: String,
}

/// The period a closure of `kind` covers when run for `on`.
pub fn period_bounds(kind: ClosureKind, on: NaiveDate) -> (NaiveDate, NaiveDate) {
    match kind {
        ClosureKind::Day => (on, on),
        ClosureKind::Month => {
            let start = NaiveDate::from_ymd_opt(on.year(), on.month(), 1).unwrap_or(on);
            let end = if on.month() == 12 {
                NaiveDate::from_ymd_opt(on.year(), 12, 31).unwrap_or(on)
            } else {
                NaiveDate::from_ymd_opt(on.year(), on.month() + 1, 1)
                    .and_then(|d| d.pred_opt())
                    .unwrap_or(on)
            };
            (start, end)
        }
        ClosureKind::Year => (
            NaiveDate::from_ymd_opt(on.year(), 1, 1).unwrap_or(on),
            NaiveDate::from_ymd_opt(on.year(), 12, 31).unwrap_or(on),
        ),
    }
}

/// Compute and seal a closure. Idempotent: a period already closed is returned
/// as it was, because § 170 does not allow a second, different answer.
pub async fn close(
    state: &SharedState,
    register: &Register,
    kind: ClosureKind,
    on: NaiveDate,
) -> LocalResult<ClosureRow> {
    let (start, end) = period_bounds(kind, on);
    let (start_s, end_s) = (start.to_string(), end.to_string());

    if let Some(existing) = find(state, register, kind, &start_s).await? {
        return Ok(existing);
    }

    // Closing a period that is still running would give a total that changes the
    // moment the next sale is rung up. The one exception is the shop's own day: at
    // the end of the day the cashier closes the till and the day with it (the Z).
    // That is allowed because the journal then refuses any further sale or payment
    // on that day (`fiscal::append`), so the total cannot move afterwards.
    let today = fiscal::business_day(Utc::now());
    if start_s > today {
        return Err(LocalError::BadRequest(format!(
            "la période commence le {start_s} : elle n'a pas encore commencé"
        )));
    }
    if end_s >= today {
        if kind != ClosureKind::Day {
            return Err(LocalError::BadRequest(format!(
                "la période se termine le {end_s} et n'est pas encore écoulée ; la clôture {} n'est possible qu'à partir du lendemain",
                match kind {
                    ClosureKind::Month => "mensuelle",
                    _ => "annuelle",
                }
            )));
        }
        let (open_sessions,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pos_sessions WHERE register_id = $1 AND closed_at IS NULL",
        )
        .bind(register.id.to_string())
        .fetch_one(&state.db)
        .await?;
        if open_sessions > 0 {
            return Err(LocalError::BadRequest(
                "une session de caisse est encore ouverte : fermez-la (comptage du tiroir) avant de clôturer la journée".into(),
            ));
        }
    }

    require_lower_levels_closed(state, register, kind, &start_s, &end_s).await?;

    let totals = aggregate(&state.db, register, kind, &start_s, &end_s).await?;

    let actor = fiscal::current_actor(state).await;
    let now = Utc::now();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;

    // Both running totals are recomputed as they stood at the end of the period
    // rather than read off the live counters. A closure run late — three days of
    // trading closed on Friday morning — must still report Monday's figures, and
    // running it again on a restored database must give the same answer.
    let period_opened_on: String = sqlx::query_as::<_, (String,)>(
        "SELECT period_opened_on FROM fiscal_counters WHERE register_id = $1",
    )
    .bind(register.id.to_string())
    .fetch_optional(&mut *tx)
    .await?
    .map(|r| r.0)
    .unwrap_or_default();

    let (period_total, perpetual_total) =
        running_totals(&mut tx, register, &period_opened_on, &end_s).await?;

    let sealed = ClosureTotals {
        kind,
        register_id: register.code.clone(),
        period_start: start_s.clone(),
        period_end: end_s.clone(),
        document_count: totals.document_count,
        first_seq: totals.first_seq,
        last_seq: totals.last_seq,
        total_ht_cents: totals.total_ht_cents,
        total_vat_cents: totals.total_vat_cents,
        total_ttc_cents: totals.total_ttc_cents,
        buckets: totals.buckets.clone(),
        period_grand_total_cents: period_total,
        perpetual_total_cents: perpetual_total,
    };

    let entry = fiscal::append(
        &mut tx,
        register,
        kind.event(),
        now,
        sealed.to_payload(),
        EntryLinks {
            document_ref: Some(format!("{}-{}", kind.as_str().to_uppercase(), start_s)),
            sale_id: None,
            total_ttc_cents: totals.total_ttc_cents,
            actor_id: actor.clone(),
        },
    )
    .await?;

    let id = Uuid::new_v4();
    let breakdown = serde_json::to_string(&sealed.buckets).unwrap_or_else(|_| "[]".into());

    sqlx::query(
        r#"
        INSERT INTO fiscal_closures (
            id, register_id, kind, period_start, period_end, document_count,
            first_seq, last_seq, total_ht_cents, total_vat_cents, total_ttc_cents,
            vat_breakdown, period_grand_total_cents, perpetual_total_cents,
            journal_seq, chain_hash, closed_at, actor_id, company_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)
        "#,
    )
    .bind(id.to_string())
    .bind(register.id.to_string())
    .bind(kind.as_str())
    .bind(&start_s)
    .bind(&end_s)
    .bind(totals.document_count)
    .bind(totals.first_seq)
    .bind(totals.last_seq)
    .bind(totals.total_ht_cents)
    .bind(totals.total_vat_cents)
    .bind(totals.total_ttc_cents)
    .bind(&breakdown)
    .bind(period_total)
    .bind(perpetual_total)
    .bind(entry.seq)
    .bind(&entry.chain_hash)
    .bind(now.to_rfc3339())
    .bind(&actor)
    .bind(&register.company_id)
    .execute(&mut *tx)
    .await?;

    // An annual closure ends the accounting period. Only the period counter
    // resets; the perpetual total carries on (§ 273).
    if kind == ClosureKind::Year {
        let new_start = NaiveDate::from_ymd_opt(end.year() + 1, 1, 1)
            .map(|d| d.to_string())
            .unwrap_or_default();
        // The new period does not necessarily start empty: closed late, in February,
        // January's trading already belongs to it. Resetting to zero dropped it from
        // the period total of every later closure.
        let (carried, _) = running_totals(&mut tx, register, &new_start, "9999-12-31").await?;
        sqlx::query(
            "UPDATE fiscal_counters SET period_grand_total_cents = $4, period_opened_on = $2, updated_at = $3
             WHERE register_id = $1",
        )
        .bind(register.id.to_string())
        .bind(&new_start)
        .bind(now.to_rfc3339())
        .bind(carried)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let device_id = state.device_id().await.map_err(LocalError::Internal)?;
    fiscal::replicate(
        &state.db,
        device_id,
        register,
        &entry,
        &EntryLinks {
            document_ref: Some(format!("{}-{}", kind.as_str().to_uppercase(), start_s)),
            sale_id: None,
            total_ttc_cents: totals.total_ttc_cents,
            actor_id: actor.clone(),
        },
    )
    .await?;
    state.notify_sync();

    Ok(ClosureRow {
        id: id.to_string(),
        kind: kind.as_str().to_string(),
        period_start: start_s,
        period_end: end_s,
        document_count: totals.document_count,
        total_ht_cents: totals.total_ht_cents,
        total_vat_cents: totals.total_vat_cents,
        total_ttc_cents: totals.total_ttc_cents,
        vat_breakdown: serde_json::to_value(&sealed.buckets).unwrap_or(Value::Null),
        period_grand_total_cents: period_total,
        perpetual_total_cents: perpetual_total,
        journal_seq: entry.seq,
        closed_at: now.to_rfc3339(),
    })
}

/// The two cumulative totals § 170 asks every closure to carry, as they stood
/// at the end of `end_day`.
///
/// The period total starts at the opening of the accounting period, which the
/// annual closure moves forward; the perpetual total starts at the first use of
/// the software and is never reset (§ 273).
async fn running_totals(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    register: &Register,
    period_opened_on: &str,
    end_day: &str,
) -> LocalResult<(i64, i64)> {
    let row: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN business_day >= $2 THEN total_ttc_cents ELSE 0 END), 0),
            COALESCE(SUM(total_ttc_cents), 0)
        FROM fiscal_journal
        WHERE register_id = $1
          AND business_day <= $3
          AND event_type IN ('invoice', 'receipt', 'credit_note')
        "#,
    )
    .bind(register.id.to_string())
    .bind(period_opened_on)
    .bind(end_day)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row)
}

/// § 170 makes the three levels cumulative, so a month cannot be closed over
/// days that were never closed, nor a year over open months. Refusing here is
/// what keeps the levels reconcilable with each other.
async fn require_lower_levels_closed(
    state: &SharedState,
    register: &Register,
    kind: ClosureKind,
    start: &str,
    end: &str,
) -> LocalResult<()> {
    let (level, open) = match kind {
        ClosureKind::Day => return Ok(()),
        ClosureKind::Month => (
            "journalières",
            unclosed_days(state, register, start, end).await?,
        ),
        ClosureKind::Year => (
            "mensuelles",
            unclosed_months(state, register, start, end).await?,
        ),
    };

    if open.is_empty() {
        return Ok(());
    }

    let shown = open.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    let more = if open.len() > 5 {
        format!(" (et {} autres)", open.len() - 5)
    } else {
        String::new()
    };
    Err(LocalError::BadRequest(format!(
        "clôtures {level} manquantes : {shown}{more}. Les clôtures sont cumulatives et doivent être faites dans l'ordre."
    )))
}

/// Business days inside the range that saw activity and were never closed.
async fn unclosed_days(
    state: &SharedState,
    register: &Register,
    start: &str,
    end: &str,
) -> LocalResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT j.business_day
        FROM fiscal_journal j
        WHERE j.register_id = $1
          AND j.business_day BETWEEN $2 AND $3
          AND NOT EXISTS (
              SELECT 1 FROM fiscal_closures c
              WHERE c.register_id = j.register_id AND c.kind = 'day'
                AND c.period_start = j.business_day
          )
        ORDER BY j.business_day ASC
        "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Months inside the range that saw activity and were never closed.
async fn unclosed_months(
    state: &SharedState,
    register: &Register,
    start: &str,
    end: &str,
) -> LocalResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT substr(j.business_day, 1, 7) AS month
        FROM fiscal_journal j
        WHERE j.register_id = $1
          AND j.business_day BETWEEN $2 AND $3
          AND NOT EXISTS (
              SELECT 1 FROM fiscal_closures c
              WHERE c.register_id = j.register_id AND c.kind = 'month'
                AND substr(c.period_start, 1, 7) = substr(j.business_day, 1, 7)
          )
        ORDER BY month ASC
        "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub struct PeriodTotals {
    pub document_count: i64,
    pub first_seq: i64,
    pub last_seq: i64,
    pub total_ht_cents: i64,
    pub total_vat_cents: i64,
    pub total_ttc_cents: i64,
    pub buckets: Vec<VatBucket>,
}

/// Total a period straight from the journal payloads.
pub async fn aggregate(
    pool: &SqlitePool,
    register: &Register,
    _kind: ClosureKind,
    start: &str,
    end: &str,
) -> LocalResult<PeriodTotals> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        r#"
        SELECT seq, event_type, payload
        FROM fiscal_journal
        WHERE register_id = $1 AND business_day >= $2 AND business_day <= $3
        ORDER BY seq ASC
        "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let mut totals = PeriodTotals {
        document_count: 0,
        first_seq: 0,
        last_seq: 0,
        total_ht_cents: 0,
        total_vat_cents: 0,
        total_ttc_cents: 0,
        buckets: Vec::new(),
    };
    let mut by_rate: BTreeMap<i64, (i64, i64)> = BTreeMap::new();

    for (seq, event_type, payload) in rows {
        if totals.first_seq == 0 {
            totals.first_seq = seq;
        }
        totals.last_seq = seq;

        let Some(event) = FiscalEventType::parse(&event_type) else {
            continue;
        };
        if !event.affects_turnover() {
            continue;
        }

        let payload: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
        totals.document_count += 1;
        totals.total_ht_cents += int(&payload, "total_ht_cents");
        totals.total_vat_cents += int(&payload, "total_vat_cents");
        totals.total_ttc_cents += int(&payload, "total_ttc_cents");

        if let Some(buckets) = payload.get("vat_breakdown").and_then(|v| v.as_array()) {
            for bucket in buckets {
                let rate = int(bucket, "rate_bp");
                let slot = by_rate.entry(rate).or_insert((0, 0));
                slot.0 += int(bucket, "base_ht_cents");
                slot.1 += int(bucket, "vat_cents");
            }
        }
    }

    totals.buckets = by_rate
        .into_iter()
        .map(|(rate_bp, (base_ht_cents, vat_cents))| VatBucket {
            rate_bp,
            base_ht_cents,
            vat_cents,
            total_ttc_cents: base_ht_cents + vat_cents,
        })
        .collect();

    Ok(totals)
}

pub async fn find(
    state: &SharedState,
    register: &Register,
    kind: ClosureKind,
    period_start: &str,
) -> LocalResult<Option<ClosureRow>> {
    let row: Option<ClosureDbRow> = sqlx::query_as(
        r#"
        SELECT id, kind, period_start, period_end, document_count, total_ht_cents,
               total_vat_cents, total_ttc_cents, vat_breakdown,
               period_grand_total_cents, perpetual_total_cents, journal_seq, closed_at
        FROM fiscal_closures
        WHERE register_id = $1 AND kind = $2 AND period_start = $3
        "#,
    )
    .bind(register.id.to_string())
    .bind(kind.as_str())
    .bind(period_start)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(ClosureDbRow::into_row))
}

pub async fn list(
    state: &SharedState,
    kind: Option<&str>,
    limit: i64,
) -> LocalResult<Vec<ClosureRow>> {
    let rows: Vec<ClosureDbRow> = sqlx::query_as(
        r#"
        SELECT id, kind, period_start, period_end, document_count, total_ht_cents,
               total_vat_cents, total_ttc_cents, vat_breakdown,
               period_grand_total_cents, perpetual_total_cents, journal_seq, closed_at
        FROM fiscal_closures
        WHERE ($1 IS NULL OR kind = $1)
        ORDER BY period_end DESC, kind ASC
        LIMIT $2
        "#,
    )
    .bind(kind)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(ClosureDbRow::into_row).collect())
}

/// Days with activity that have not been closed yet.
pub async fn pending_days(state: &SharedState, register: &Register) -> LocalResult<Vec<String>> {
    let today = fiscal::business_day(Utc::now());
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT j.business_day
        FROM fiscal_journal j
        WHERE j.register_id = $1
          AND j.business_day < $2
          AND NOT EXISTS (
              SELECT 1 FROM fiscal_closures c
              WHERE c.register_id = j.register_id AND c.kind = 'day'
                AND c.period_start = j.business_day
          )
        ORDER BY j.business_day ASC
        "#,
    )
    .bind(register.id.to_string())
    .bind(&today)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

#[derive(sqlx::FromRow)]
struct ClosureDbRow {
    id: String,
    kind: String,
    period_start: String,
    period_end: String,
    document_count: i64,
    total_ht_cents: i64,
    total_vat_cents: i64,
    total_ttc_cents: i64,
    vat_breakdown: String,
    period_grand_total_cents: i64,
    perpetual_total_cents: i64,
    journal_seq: i64,
    closed_at: String,
}

impl ClosureDbRow {
    fn into_row(self) -> ClosureRow {
        ClosureRow {
            id: self.id,
            kind: self.kind,
            period_start: self.period_start,
            period_end: self.period_end,
            document_count: self.document_count,
            total_ht_cents: self.total_ht_cents,
            total_vat_cents: self.total_vat_cents,
            total_ttc_cents: self.total_ttc_cents,
            vat_breakdown: serde_json::from_str(&self.vat_breakdown).unwrap_or(Value::Null),
            period_grand_total_cents: self.period_grand_total_cents,
            perpetual_total_cents: self.perpetual_total_cents,
            journal_seq: self.journal_seq,
            closed_at: self.closed_at,
        }
    }
}

fn int(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(|v| v.as_i64()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::{past_invoice, shop};
    use chrono::Duration;
    use motodesk_fiscal::VatBucket as Bucket;

    fn days_ago(n: i64) -> NaiveDate {
        (Utc::now() - Duration::days(n)).date_naive()
    }

    #[tokio::test]
    async fn a_running_month_or_year_cannot_be_closed() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        let today = fiscal::business_day(Utc::now()).parse::<NaiveDate>().unwrap();

        for kind in [ClosureKind::Month, ClosureKind::Year] {
            assert!(matches!(
                close(&state, &register, kind, today).await,
                Err(LocalError::BadRequest(_))
            ));
        }
    }

    #[tokio::test]
    async fn a_day_totals_its_documents_and_seals_itself() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        let day = days_ago(1);
        past_invoice(&state, &register, day, 10_000, 2_000).await;
        past_invoice(&state, &register, day, 5_000, 1_000).await;

        let closure = close(&state, &register, ClosureKind::Day, day)
            .await
            .unwrap();

        assert_eq!(closure.document_count, 2);
        assert_eq!(closure.total_ht_cents, 15_000);
        assert_eq!(closure.total_vat_cents, 3_000);
        assert_eq!(closure.total_ttc_cents, 18_000);
        assert_eq!(closure.perpetual_total_cents, 18_000);

        // Both rates collapse into one bucket, and the bucket has to add up.
        let buckets: Vec<Bucket> = serde_json::from_value(closure.vat_breakdown.clone()).unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].base_ht_cents + buckets[0].vat_cents, 18_000);

        // The closure is itself an entry in the chain (§ 170).
        let (event,): (String,) =
            sqlx::query_as("SELECT event_type FROM fiscal_journal WHERE seq = $1")
                .bind(closure.journal_seq)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(event, "closure_day");
    }

    #[tokio::test]
    async fn closing_the_same_day_twice_returns_the_first_answer() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        let day = days_ago(1);
        past_invoice(&state, &register, day, 10_000, 2_000).await;

        let first = close(&state, &register, ClosureKind::Day, day)
            .await
            .unwrap();
        let second = close(&state, &register, ClosureKind::Day, day)
            .await
            .unwrap();

        assert_eq!(first.id, second.id);
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'closure_day'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn a_month_cannot_be_closed_over_open_days() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();

        // A day inside a month that is already over, so only the missing daily
        // closure can be what stops the monthly one.
        let day = days_ago(40);
        past_invoice(&state, &register, day, 10_000, 2_000).await;

        let refused = close(&state, &register, ClosureKind::Month, day).await;
        assert!(matches!(refused, Err(LocalError::BadRequest(_))));

        close(&state, &register, ClosureKind::Day, day)
            .await
            .unwrap();
        let month = close(&state, &register, ClosureKind::Month, day)
            .await
            .unwrap();
        assert_eq!(month.total_ttc_cents, 12_000);
    }

    #[tokio::test]
    async fn the_annual_closure_resets_the_period_but_not_the_perpetual_total() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();

        // Last year, so the year is over and can legitimately be closed.
        let last_year = Utc::now().date_naive().year() - 1;
        let day = NaiveDate::from_ymd_opt(last_year, 6, 15).unwrap();
        past_invoice(&state, &register, day, 100_000, 20_000).await;

        close(&state, &register, ClosureKind::Day, day)
            .await
            .unwrap();
        close(&state, &register, ClosureKind::Month, day)
            .await
            .unwrap();
        let year = close(&state, &register, ClosureKind::Year, day)
            .await
            .unwrap();

        assert_eq!(year.total_ttc_cents, 120_000);
        assert_eq!(year.perpetual_total_cents, 120_000);

        let (period, perpetual, opened): (i64, i64, String) = sqlx::query_as(
            "SELECT period_grand_total_cents, perpetual_total_cents, period_opened_on
             FROM fiscal_counters WHERE register_id = $1",
        )
        .bind(register.id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();

        assert_eq!(period, 0, "the accounting period starts again from zero");
        assert_eq!(
            perpetual, 120_000,
            "the perpetual total never resets (§ 273)"
        );
        assert_eq!(opened, format!("{}-01-01", last_year + 1));
    }

    #[tokio::test]
    async fn an_annual_closure_run_late_keeps_the_new_years_trading_in_the_period() {
        let today = Utc::now().date_naive();
        if today.month() == 1 && today.day() < 3 {
            return; // no room for "trading already done this year"
        }
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();

        let last_year = today.year() - 1;
        let old_day = NaiveDate::from_ymd_opt(last_year, 6, 15).unwrap();
        past_invoice(&state, &register, old_day, 100_000, 20_000).await;
        // January of this year, before the year-end closure is finally run.
        let new_day = NaiveDate::from_ymd_opt(today.year(), 1, 2).unwrap();
        past_invoice(&state, &register, new_day, 10_000, 2_000).await;

        close(&state, &register, ClosureKind::Day, old_day).await.unwrap();
        close(&state, &register, ClosureKind::Month, old_day).await.unwrap();
        let year = close(&state, &register, ClosureKind::Year, old_day).await.unwrap();
        assert_eq!(year.total_ttc_cents, 120_000, "the closed year holds only its own trading");

        let (period,): (i64,) =
            sqlx::query_as("SELECT period_grand_total_cents FROM fiscal_counters WHERE register_id = $1")
                .bind(register.id.to_string())
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(period, 12_000, "January's trading is the new period's, not wiped out");
    }

    #[test]
    fn day_period_is_one_day() {
        let d = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
        assert_eq!(period_bounds(ClosureKind::Day, d), (d, d));
    }

    #[test]
    fn month_period_covers_the_calendar_month() {
        let d = NaiveDate::from_ymd_opt(2026, 2, 14).unwrap();
        let (start, end) = period_bounds(ClosureKind::Month, d);
        assert_eq!(start.to_string(), "2026-02-01");
        assert_eq!(end.to_string(), "2026-02-28");
    }

    #[test]
    fn december_month_period_does_not_overflow_the_year() {
        let d = NaiveDate::from_ymd_opt(2026, 12, 3).unwrap();
        let (start, end) = period_bounds(ClosureKind::Month, d);
        assert_eq!(start.to_string(), "2026-12-01");
        assert_eq!(end.to_string(), "2026-12-31");
    }

    #[test]
    fn year_period_covers_the_calendar_year() {
        let d = NaiveDate::from_ymd_opt(2026, 6, 30).unwrap();
        let (start, end) = period_bounds(ClosureKind::Year, d);
        assert_eq!(start.to_string(), "2026-01-01");
        assert_eq!(end.to_string(), "2026-12-31");
    }

    #[test]
    fn leap_february_is_twenty_nine_days() {
        let d = NaiveDate::from_ymd_opt(2028, 2, 3).unwrap();
        assert_eq!(
            period_bounds(ClosureKind::Month, d).1.to_string(),
            "2028-02-29"
        );
    }
}
