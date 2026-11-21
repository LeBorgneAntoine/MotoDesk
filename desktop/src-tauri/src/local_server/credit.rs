//! Encours checks shared by the till and invoice seal.

use sqlx::SqlitePool;

use crate::local_server::error::{LocalError, LocalResult};

/// Why this client must not take more unpaid TTC. `None` = allowed.
pub fn refuse_reason(
    blocked: bool,
    credit_limit_cents: Option<i64>,
    outstanding_cents: i64,
    additional_unpaid_cents: i64,
) -> Option<&'static str> {
    if blocked {
        return Some("compte client bloqué ; encours interdit");
    }
    let Some(limit) = credit_limit_cents else {
        return None;
    };
    if outstanding_cents + additional_unpaid_cents > limit {
        return Some("encours dépassé ; réglez l'encours ou relevez le plafond");
    }
    None
}

/// Sum of sealed invoice TTC still unpaid for this client.
pub async fn outstanding_cents(db: &SqlitePool, client_id: &str) -> LocalResult<i64> {
    let cents: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(COALESCE(s.total_ttc_cents, 0) + COALESCE((SELECT SUM(c.total_ttc_cents) FROM sales c
                    WHERE c.corrects_sale_id = s.id AND c.locked_at IS NOT NULL
                      AND c.deleted_at IS NULL), 0) - COALESCE(p.paid, 0)), 0)
        FROM sales s
        LEFT JOIN (
            SELECT sale_id, SUM(amount_cents) AS paid FROM payments
            WHERE deleted_at IS NULL GROUP BY sale_id
        ) p ON p.sale_id = s.id
        WHERE s.client_id = $1
          AND s.deleted_at IS NULL
          AND s.locked_at IS NOT NULL
          AND COALESCE(s.document_kind, 'invoice') = 'invoice'
          AND COALESCE(s.total_ttc_cents, 0) + COALESCE((SELECT SUM(c.total_ttc_cents) FROM sales c
                    WHERE c.corrects_sale_id = s.id AND c.locked_at IS NOT NULL
                      AND c.deleted_at IS NULL), 0) - COALESCE(p.paid, 0) > 0
        "#,
    )
    .bind(client_id)
    .fetch_one(db)
    .await?;
    Ok(cents)
}

pub async fn assert_allows(
    db: &SqlitePool,
    client_id: &str,
    additional_unpaid_cents: i64,
) -> LocalResult<()> {
    let row: Option<(Option<i64>, i64)> = sqlx::query_as(
        "SELECT credit_limit_cents, credit_blocked FROM clients WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(client_id)
    .fetch_optional(db)
    .await?;
    let Some((limit, blocked)) = row else {
        return Err(LocalError::BadRequest("client introuvable".into()));
    };
    let outstanding = outstanding_cents(db, client_id).await?;
    if let Some(msg) = refuse_reason(blocked != 0, limit, outstanding, additional_unpaid_cents) {
        return Err(LocalError::BadRequest(msg.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_wins() {
        assert!(refuse_reason(true, Some(1_000_000), 0, 1).is_some());
    }

    #[test]
    fn unlimited_passes() {
        assert!(refuse_reason(false, None, 99_000, 1).is_none());
    }

    #[test]
    fn over_limit_fails() {
        assert!(refuse_reason(false, Some(100), 80, 30).is_some());
    }

    #[test]
    fn paid_till_at_cap_passes() {
        assert!(refuse_reason(false, Some(100), 100, 0).is_none());
    }
}
