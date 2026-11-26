//! Shared fixtures for the local-server tests.
//!
//! Fiscal behaviour is only meaningful against a shop that could legally issue
//! an invoice, so the fixture fills in the mandatory mentions rather than
//! leaving each test to discover that validation refuses an empty profile.

use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use serde_json::json;
use uuid::Uuid;

use crate::local_server::db;
use crate::local_server::fiscal::Register;
use crate::local_server::state::{LocalState, SharedState};

/// A database with a complete company profile and an empty journal.
pub async fn shop() -> SharedState {
    Arc::new(shop_state().await)
}

/// The same shop, whose assistant talks to `openrouter` (a stand-in started by the test on this
/// PC) instead of OpenRouter.
pub async fn shop_with_openrouter(openrouter: &str) -> SharedState {
    let mut state = shop_state().await;
    state.openrouter_url = openrouter.to_string();
    Arc::new(state)
}

async fn shop_state() -> LocalState {
    // Tests would otherwise leave one key per throwaway register behind in the
    // developer's real credential store.
    std::env::set_var("MOTODESK_FISCAL_NO_KEYCHAIN", "1");

    let dir = std::env::temp_dir().join(format!("motodesk_test_{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let pool = db::init_pool(&dir.join("local.db")).await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    db::ensure_device_id(&pool).await.unwrap();

    sqlx::query(
        "INSERT INTO company_cache (id, company_id, name, legal_name, cached_at)
         VALUES (1, NULL, 'Garage Dupont', 'Garage Dupont SARL', $1)",
    )
    .bind(Utc::now().to_rfc3339())
    .execute(&pool)
    .await
    .unwrap();

    for (key, value) in [
        ("contact.address_line1", "\"12 rue des Motos\""),
        ("contact.postal_code", "\"35000\""),
        ("contact.city", "\"Rennes\""),
        ("legal.siret", "\"12345678900012\""),
        ("legal.legal_form", "\"EI\""),
        ("legal.ape_code", "\"4540Z\""),
        ("legal.vat_number", "\"FR12123456789\""),
        ("vat.is_subject", "true"),
        ("invoicing.payment_terms_days", "30"),
        ("invoicing.late_fee_rate_bp", "1000"),
        ("invoicing.recovery_indemnity_eur", "40"),
    ] {
        sqlx::query("INSERT INTO app_settings (key, value, updated_at) VALUES ($1,$2,$3)")
            .bind(key)
            .bind(value)
            .bind(Utc::now().to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
    }

    LocalState::new(
        pool,
        dir,
        "http://127.0.0.1:8080".into(),
        None,
        None,
        "test-local-token".into(),
    )
}

/// A catalogue row the till can scan. Stock is a whole unit; services skip it.
pub async fn insert_product(
    state: &SharedState,
    name: &str,
    sku: Option<&str>,
    stock: i64,
    is_service: bool,
    selling_ttc_cents: i64,
    vat_rate_bp: i64,
) -> Uuid {
    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO products (
            id, name, sku, unit, is_service, selling_price_ttc_cents, vat_rate_bp,
            stock_milli, is_active, created_at, updated_at, row_version, field_meta
        ) VALUES ($1,$2,$3,'piece',$4,$5,$6,$7,1,$8,$8,1,'{}')
        "#,
    )
    .bind(id.to_string())
    .bind(name)
    .bind(sku)
    .bind(is_service as i64)
    .bind(selling_ttc_cents)
    .bind(vat_rate_bp)
    .bind(stock * 1000)
    .bind(&now)
    .execute(&state.db)
    .await
    .unwrap();
    id
}

/// Trading that happened before today, which the routes cannot produce because
/// a chain refuses to go backwards. Inserted straight into the journal so
/// closures and archives can be exercised over a finished period.
///
/// The chain hashes are placeholders: nothing that uses this fixture verifies
/// them, and the verifier has its own tests over real entries.
pub async fn past_invoice(
    state: &SharedState,
    register: &Register,
    day: NaiveDate,
    ht: i64,
    vat: i64,
) {
    let seq: (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM fiscal_journal WHERE register_id = $1",
    )
    .bind(register.id.to_string())
    .fetch_one(&state.db)
    .await
    .unwrap();

    let payload = json!({
        "total_ht_cents": ht,
        "total_vat_cents": vat,
        "total_ttc_cents": ht + vat,
        "vat_breakdown": [{
            "rate_bp": 2_000,
            "base_ht_cents": ht,
            "vat_cents": vat,
            "total_ttc_cents": ht + vat,
        }],
    });

    let hash = format!("{:064x}", seq.0);

    sqlx::query(
        r#"
        INSERT INTO fiscal_journal (
            id, register_id, seq, event_type, occurred_at, business_day,
            payload, payload_hash, prev_hash, chain_hash, signature,
            document_ref, sale_id, total_ttc_cents, actor_id, company_id
        ) VALUES ($1,$2,$3,'invoice',$4,$5,$6,$7,$8,$9,'',$10,NULL,$11,NULL,NULL)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(register.id.to_string())
    .bind(seq.0)
    .bind(format!("{day}T10:00:00Z"))
    .bind(day.to_string())
    .bind(payload.to_string())
    .bind(&hash)
    .bind(format!("{:064x}", seq.0 - 1))
    .bind(&hash)
    .bind(format!("FA-TEST-{}", seq.0))
    .bind(ht + vat)
    .execute(&state.db)
    .await
    .unwrap();

    // Move the tip along, or the next real entry would reuse this sequence.
    sqlx::query(
        "UPDATE fiscal_counters SET last_seq = $2, last_chain_hash = $3,
                period_grand_total_cents = period_grand_total_cents + $4,
                perpetual_total_cents = perpetual_total_cents + $4
         WHERE register_id = $1",
    )
    .bind(register.id.to_string())
    .bind(seq.0)
    .bind(&hash)
    .bind(ht + vat)
    .execute(&state.db)
    .await
    .unwrap();
}
