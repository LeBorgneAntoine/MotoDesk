//! Document number allocation.
//!
//! This used to hand out `FA-{year}-{n}` from a company-wide counter that was
//! deliberately not synced. On a single machine that worked. With mesh peers it
//! does not: two devices working offline both allocate `FA-2026-00042`, and a
//! duplicated invoice number breaks the unbroken-sequence requirement of CGI
//! ann. II art. 242 nonies A in a way that cannot be repaired afterwards.
//!
//! Numbers now come from the calling device's own register series
//! (`FA-2026-C1-00042`), which stays unbroken per series and never needs the
//! network. The endpoint is kept for quotes and for callers that want a number
//! before they have a document; invoices get theirs at validation, from
//! `routes::sales`, so that an abandoned draft cannot leave a hole.

use axum::{extract::State, routing::post, Json, Router};
use motodesk_fiscal::numbering::DocumentKind;
use serde::{Deserialize, Serialize};

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::fiscal;
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/api/documents/next-number", post(next_number))
}

#[derive(Debug, Deserialize)]
struct NextNumberBody {
    kind: String,
}

#[derive(Debug, Serialize)]
struct AllocatedNumber {
    kind: String,
    year: i32,
    register_code: String,
    number: i64,
    reference: String,
}

async fn next_number(
    State(state): State<SharedState>,
    Json(body): Json<NextNumberBody>,
) -> LocalResult<Json<AllocatedNumber>> {
    let kind = DocumentKind::parse(&body.kind).ok_or_else(|| {
        LocalError::BadRequest("kind must be invoice, credit_note, quote or receipt".into())
    })?;

    // Invoice numbers are allocated by validation, inside the same transaction
    // that seals the document. Handing one out here would let a caller burn a
    // number and never use it.
    if kind == DocumentKind::Invoice || kind == DocumentKind::CreditNote {
        return Err(LocalError::BadRequest(
            "les numéros de facture et d'avoir sont attribués à la validation du document".into(),
        ));
    }

    let register = fiscal::current(&state).await?;
    let year = fiscal::current_year();

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let (number, reference) = fiscal::next_number(&mut tx, &register, kind, year).await?;
    tx.commit().await?;

    let _ = sqlx::query(
        "INSERT INTO audit_log (id, company_id, action, entity_type, entity_id, payload, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&register.company_id)
    .bind("allocate_number")
    .bind(kind.as_str())
    .bind(&reference)
    .bind(serde_json::json!({ "number": number, "year": year, "register": register.code }).to_string())
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;

    Ok(Json(AllocatedNumber {
        kind: kind.as_str().to_string(),
        year,
        register_code: register.code.clone(),
        number,
        reference,
    }))
}
