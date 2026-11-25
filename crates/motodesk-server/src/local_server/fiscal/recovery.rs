//! Rebuilding a register's cursors from its own journal.
//!
//! The journal is the record; the rest of a register's state is a working copy of
//! it: where the chain ends, the two running totals, the closures, and the next
//! number of each series. A restored backup brings back an old copy of all of them,
//! and the changes recovered from the cloud bring the journal back up to date but
//! not the copies. Left alone, the register would carry on from the old tip (a fork
//! of its own chain), close days that are already closed, and issue invoice numbers
//! it has already issued.
//!
//! Everything here is derived from rows that cannot be edited, and running it on a
//! register that is already right changes nothing.

use motodesk_fiscal::chain::GENESIS_HASH;
use motodesk_fiscal::numbering::{normalise_register_code, parse_number};
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

use crate::local_server::error::{LocalError, LocalResult};

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct RebuildReport {
    /// Registers whose chain tip and totals were brought up to the journal.
    pub registers_moved: u32,
    /// Closures the journal sealed that the closures table did not have.
    pub closures_restored: u32,
    /// Numbering series moved past numbers the journal or the sales already hold.
    pub numbering_moved: u32,
}

fn io(e: impl std::fmt::Display) -> LocalError {
    LocalError::Internal(anyhow::anyhow!("{e}"))
}

pub async fn rebuild_from_journal(pool: &SqlitePool) -> LocalResult<RebuildReport> {
    let mut report = RebuildReport::default();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    // Registers this device keeps counters for (its own); replicas of other PCs'
    // registers have none and are not touched.
    let registers: Vec<(String, String, i64, String)> = sqlx::query_as(
        "SELECT r.id, r.code, c.last_seq, COALESCE(c.period_opened_on, '')
           FROM fiscal_registers r JOIN fiscal_counters c ON c.register_id = r.id",
    )
    .fetch_all(&mut *tx)
    .await?;

    for (register_id, code, counted_seq, period_opened_on) in registers {
        // --- closures the journal sealed -------------------------------------
        let missing: Vec<(i64, String, String, String, Option<String>, Option<String>, String)> =
            sqlx::query_as(
                "SELECT j.seq, j.payload, j.chain_hash, j.occurred_at, j.actor_id, j.company_id, j.event_type
                   FROM fiscal_journal j
                  WHERE j.register_id = $1
                    AND j.event_type IN ('closure_day', 'closure_month', 'closure_year')
                    AND NOT EXISTS (SELECT 1 FROM fiscal_closures c
                                     WHERE c.register_id = j.register_id AND c.journal_seq = j.seq)
                  ORDER BY j.seq",
            )
            .bind(&register_id)
            .fetch_all(&mut *tx)
            .await?;

        for (seq, payload, chain_hash, occurred_at, actor, company, _event) in missing {
            let p: Value = serde_json::from_str(&payload).map_err(io)?;
            let int = |k: &str| p.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
            let text = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
            sqlx::query(
                r#"
                INSERT INTO fiscal_closures (
                    id, register_id, kind, period_start, period_end, document_count,
                    first_seq, last_seq, total_ht_cents, total_vat_cents, total_ttc_cents,
                    vat_breakdown, period_grand_total_cents, perpetual_total_cents,
                    journal_seq, chain_hash, closed_at, actor_id, company_id
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&register_id)
            .bind(text("kind"))
            .bind(text("period_start"))
            .bind(text("period_end"))
            .bind(int("document_count"))
            .bind(int("first_seq"))
            .bind(int("last_seq"))
            .bind(int("total_ht_cents"))
            .bind(int("total_vat_cents"))
            .bind(int("total_ttc_cents"))
            .bind(p.get("vat_breakdown").cloned().unwrap_or(Value::Null).to_string())
            .bind(int("period_grand_total_cents"))
            .bind(int("perpetual_total_cents"))
            .bind(seq)
            .bind(chain_hash)
            .bind(occurred_at)
            .bind(actor)
            .bind(company)
            .execute(&mut *tx)
            .await?;
            report.closures_restored += 1;
        }

        // --- where the chain ends, and the totals ----------------------------
        let tip: Option<(i64, String, String)> = sqlx::query_as(
            "SELECT seq, chain_hash, occurred_at FROM fiscal_journal
              WHERE register_id = $1 ORDER BY seq DESC LIMIT 1",
        )
        .bind(&register_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((tip_seq, tip_hash, tip_at)) = tip else {
            continue;
        };

        // An annual closure ends the accounting period: the next one opens on 1 January.
        let last_year_end: Option<(String,)> = sqlx::query_as(
            "SELECT period_end FROM fiscal_closures
              WHERE register_id = $1 AND kind = 'year' ORDER BY period_end DESC LIMIT 1",
        )
        .bind(&register_id)
        .fetch_optional(&mut *tx)
        .await?;
        let derived_start = last_year_end
            .and_then(|(end,)| end.get(..4).and_then(|y| y.parse::<i32>().ok()))
            .map(|y| format!("{}-01-01", y + 1))
            .unwrap_or_default();
        let period_start = if derived_start > period_opened_on {
            derived_start
        } else {
            period_opened_on.clone()
        };

        let behind = tip_seq > counted_seq;
        if behind || period_start != period_opened_on {
            let (period_total, perpetual_total): (i64, i64) = sqlx::query_as(
                r#"
                SELECT
                    COALESCE(SUM(CASE WHEN business_day >= $2 THEN total_ttc_cents ELSE 0 END), 0),
                    COALESCE(SUM(total_ttc_cents), 0)
                FROM fiscal_journal
                WHERE register_id = $1 AND event_type IN ('invoice', 'receipt', 'credit_note')
                "#,
            )
            .bind(&register_id)
            .bind(&period_start)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE fiscal_counters SET last_seq = $2, last_chain_hash = $3, last_occurred_at = $4,
                        period_grand_total_cents = $5, perpetual_total_cents = $6,
                        period_opened_on = $7, updated_at = $8
                  WHERE register_id = $1",
            )
            .bind(&register_id)
            .bind(tip_seq)
            .bind(if tip_hash.is_empty() { GENESIS_HASH.to_string() } else { tip_hash })
            .bind(tip_at)
            .bind(period_total)
            .bind(perpetual_total)
            .bind(&period_start)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&mut *tx)
            .await?;
            report.registers_moved += 1;
        }

        // --- numbering ---------------------------------------------------------
        // The highest number each series has handed out, from the journal and from
        // the documents themselves (quotes are numbered but never journalled).
        let refs: Vec<(String,)> = sqlx::query_as(
            "SELECT document_ref FROM fiscal_journal WHERE register_id = $1 AND document_ref IS NOT NULL
             UNION
             SELECT reference FROM sales WHERE reference IS NOT NULL",
        )
        .bind(&register_id)
        .fetch_all(&mut *tx)
        .await?;
        let mine = normalise_register_code(&code);
        let mut highest: HashMap<(String, i32), i64> = HashMap::new();
        for (reference,) in refs {
            if let Some((kind, year, register_code, value)) = parse_number(&reference) {
                if register_code == mine {
                    let slot = highest.entry((kind.as_str().to_string(), year)).or_insert(0);
                    *slot = (*slot).max(value);
                }
            }
        }
        for ((kind, year), top) in highest {
            let held: Option<(i64,)> = sqlx::query_as(
                "SELECT next_value FROM fiscal_number_series WHERE register_id = $1 AND kind = $2 AND year = $3",
            )
            .bind(&register_id)
            .bind(&kind)
            .bind(year)
            .fetch_optional(&mut *tx)
            .await?;
            if held.map(|(n,)| n).unwrap_or(0) <= top {
                sqlx::query(
                    "INSERT INTO fiscal_number_series (register_id, kind, year, next_value) VALUES ($1, $2, $3, $4)
                     ON CONFLICT(register_id, kind, year) DO UPDATE SET next_value = excluded.next_value",
                )
                .bind(&register_id)
                .bind(&kind)
                .bind(year)
                .bind(top + 1)
                .execute(&mut *tx)
                .await?;
                report.numbering_moved += 1;
            }
        }
    }

    tx.commit().await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::fiscal;
    use crate::local_server::test_support::{past_invoice, shop};
    use chrono::{Duration, Utc};
    use motodesk_fiscal::{format_number, ClosureKind, DocumentKind};

    type Counters = (i64, String, String, i64, i64, String);

    async fn counters(pool: &SqlitePool, register: &str) -> Counters {
        sqlx::query_as(
            "SELECT last_seq, last_chain_hash, last_occurred_at, period_grand_total_cents,
                    perpetual_total_cents, COALESCE(period_opened_on, '')
               FROM fiscal_counters WHERE register_id = $1",
        )
        .bind(register)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    type Closure = (String, String, String, i64, i64, i64, i64, i64, i64, i64, i64, String);

    async fn closures(pool: &SqlitePool, register: &str) -> Vec<Closure> {
        sqlx::query_as(
            "SELECT kind, period_start, period_end, document_count, total_ht_cents, total_vat_cents,
                    total_ttc_cents, period_grand_total_cents, perpetual_total_cents, journal_seq,
                    first_seq, chain_hash
               FROM fiscal_closures WHERE register_id = $1 ORDER BY journal_seq",
        )
        .bind(register)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    /// Issue documents and close a day through the real code, then put the register's
    /// working copies back as an older backup would hold them, and rebuild.
    #[tokio::test]
    async fn a_register_is_rebuilt_from_its_journal_exactly() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        let rid = register.id.to_string();
        let day = (Utc::now() - Duration::days(3)).date_naive();

        // The register was opened before the days this test trades on.
        sqlx::query("UPDATE fiscal_counters SET period_opened_on = $2 WHERE register_id = $1")
            .bind(&rid)
            .bind((day - Duration::days(1)).to_string())
            .execute(&state.db)
            .await
            .unwrap();
        // What the backup holds: the register as it was just after opening.
        let backup = counters(&state.db, &rid).await;

        past_invoice(&state, &register, day, 10_000, 2_000).await;
        past_invoice(&state, &register, day, 5_000, 1_000).await;
        // Give the second one a real number, 7th of this series. (The journal refuses
        // edits, as it should; this fixture only wants a number that parses.)
        sqlx::query("DROP TRIGGER fiscal_journal_no_update").execute(&state.db).await.unwrap();
        let number = format_number(DocumentKind::Invoice, 2026, &register.code, 7).unwrap();
        sqlx::query("UPDATE fiscal_journal SET document_ref = $2 WHERE register_id = $1 AND seq = 3")
            .bind(&rid)
            .bind(&number)
            .execute(&state.db)
            .await
            .unwrap();
        crate::local_server::fiscal::closures::close(&state, &register, ClosureKind::Day, day)
            .await
            .unwrap();

        let truth = counters(&state.db, &rid).await;
        let truth_closures = closures(&state.db, &rid).await;
        assert_eq!(truth_closures.len(), 1);
        assert_eq!(truth.4, 18_000, "18 000 cents of turnover to rebuild");

        // The restore: cursors, closures and numbering go back, the journal stays
        // (the recovered changes brought it up to date).
        sqlx::query("DROP TRIGGER fiscal_closures_no_delete").execute(&state.db).await.unwrap();
        sqlx::query("DELETE FROM fiscal_closures").execute(&state.db).await.unwrap();
        sqlx::query("DELETE FROM fiscal_number_series").execute(&state.db).await.unwrap();
        sqlx::query(
            "UPDATE fiscal_counters SET last_seq = $2, last_chain_hash = $3, last_occurred_at = $4,
                    period_grand_total_cents = $5, perpetual_total_cents = $6 WHERE register_id = $1",
        )
        .bind(&rid)
        .bind(backup.0)
        .bind(&backup.1)
        .bind(&backup.2)
        .bind(backup.3)
        .bind(backup.4)
        .execute(&state.db)
        .await
        .unwrap();

        let report = rebuild_from_journal(&state.db).await.unwrap();
        assert_eq!(report.closures_restored, 1);
        assert_eq!(report.registers_moved, 1);
        assert_eq!(report.numbering_moved, 1);

        assert_eq!(counters(&state.db, &rid).await, truth, "the cursor and both totals are back");
        let rebuilt = closures(&state.db, &rid).await;
        assert_eq!(rebuilt.len(), 1);
        assert_eq!(rebuilt, truth_closures, "the closure is the one the journal sealed");

        // The next invoice is number 8, not a repeat of 7 (or of 1).
        let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let (value, reference) = fiscal::next_number(&mut tx, &register, DocumentKind::Invoice, 2026)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        assert_eq!(value, 8, "{reference}");

        // Running it again on a register that is right changes nothing.
        assert_eq!(rebuild_from_journal(&state.db).await.unwrap(), RebuildReport::default());
    }

    /// Writing to the chain of a base known to be behind the cloud would fork it.
    #[tokio::test]
    async fn the_chain_does_not_move_while_the_base_is_behind_the_cloud() {
        use motodesk_fiscal::FiscalEventType;
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();

        async fn try_append(
            state: &crate::local_server::state::SharedState,
            register: &fiscal::Register,
        ) -> LocalResult<()> {
            let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
            fiscal::append(
                &mut tx,
                register,
                FiscalEventType::IntegrityChecked,
                Utc::now(),
                serde_json::json!({}),
                fiscal::EntryLinks::default(),
            )
            .await?;
            tx.commit().await?;
            Ok(())
        }

        try_append(&state, &register).await.unwrap();
        sqlx::query("UPDATE sync_state SET seq_gap = 12 WHERE id = 1").execute(&state.db).await.unwrap();
        let blocked = try_append(&state, &register).await.unwrap_err();
        assert!(matches!(&blocked, LocalError::BadRequest(m) if m.contains("cloud")), "{blocked:?}");
        sqlx::query("UPDATE sync_state SET seq_gap = 0 WHERE id = 1").execute(&state.db).await.unwrap();
        try_append(&state, &register).await.unwrap();
    }
}
