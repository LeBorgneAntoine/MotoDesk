//! Sales, invoices, credit notes and payments.
//!
//! The lifecycle has exactly one irreversible step. Up to `validate` a document
//! is a draft or a quote and behaves like any other record: editable, syncable,
//! deletable. `validate` recomputes the totals, freezes a full snapshot, takes a
//! number from the register's series and seals the lot into the fiscal journal.
//! From that moment BOI-TVA-DECLA-30-10-30 § 90 applies: the only way to change
//! anything is a credit note, which is a new document of its own.
//!
//! Every refusal below is a legal one rather than a technical one, so the error
//! messages say which rule is being applied. The user is going to meet them at
//! the counter with a customer waiting.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, Utc};
use motodesk_core::entity::EntityType;
use motodesk_core::sync::OpKind;
use motodesk_fiscal::{
    document_totals,
    numbering::DocumentKind,
    vat::{self, LineInput},
    DocumentTotals, FiscalEventType,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::local_server::credit;
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::fiscal::{self, mentions, EntryLinks, Register};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", get(show).patch(update).delete(remove))
        .route("/:id/lines", post(replace_lines))
        .route("/:id/validate", post(validate))
        .route("/:id/issue-quote", post(issue_quote))
        .route("/:id/convert-quote", post(convert_quote))
        .route("/:id/credit-note", post(credit_note))
        .route("/:id/payments", post(add_payment))
        .route("/:id/reprint", post(reprint))
}

pub fn payments_router() -> Router<SharedState> {
    Router::new()
        .route("/", get(list_payments))
        .route("/:id/reverse", post(reverse_payment))
}

// ===========================================================================
// Drafts
// ===========================================================================

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub q: Option<String>,
    pub status: Option<String>,
    pub client_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SaleInput {
    pub client_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub sale_date: Option<String>,
    pub notes: Option<String>,
    /// `quote` or `draft`. A document is never created already validated.
    pub status: Option<String>,
    /// `invoice` (default) or `receipt` for a parked till ticket.
    #[serde(default)]
    pub document_kind: Option<String>,
    #[serde(default)]
    pub lines: Vec<LineInputBody>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LineInputBody {
    pub product_id: Option<String>,
    pub label: Option<String>,
    #[serde(default = "one_unit")]
    pub quantity_milli: i64,
    #[serde(default)]
    pub unit_price_ht_cents: i64,
    /// A shelf price. When present it is the price of record and the HT price is
    /// only a rounded display value (see `motodesk_fiscal::vat`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_price_ttc_cents: Option<i64>,
    #[serde(default)]
    pub vat_rate_bp: i64,
    #[serde(default)]
    pub discount_bp: i64,
}

fn one_unit() -> i64 {
    1_000
}

impl LineInputBody {
    fn to_vat_line(&self) -> LineInput {
        LineInput {
            quantity_milli: self.quantity_milli,
            unit_price_ht_cents: self.unit_price_ht_cents,
            unit_price_ttc_cents: self.unit_price_ttc_cents,
            discount_bp: self.discount_bp,
            vat_rate_bp: self.vat_rate_bp,
        }
    }

    /// The HT unit price a TTC line carries for display and for matching an avoir
    /// to its invoice. Derived, so two lines with the same TTC price always agree.
    pub fn normalized(&self) -> LocalResult<LineInputBody> {
        let mut line = self.clone();
        if let Some(ttc) = self.unit_price_ttc_cents {
            line.unit_price_ht_cents = vat::ht_from_ttc(ttc, self.vat_rate_bp)
                .map_err(|e| LocalError::BadRequest(e.to_string()))?;
        }
        Ok(line)
    }
}


#[derive(Debug, Serialize, sqlx::FromRow)]
struct SaleListItem {
    id: String,
    reference: Option<String>,
    document_kind: String,
    status: String,
    sale_date: Option<String>,
    client_name: Option<String>,
    total_ht_cents: Option<i64>,
    total_ttc_cents: Option<i64>,
    paid_cents: i64,
    locked_at: Option<String>,
    updated_at: String,
}

async fn list(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> LocalResult<Json<Vec<SaleListItem>>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let needle = q.q.as_ref().map(|s| format!("%{s}%"));

    let rows: Vec<SaleListItem> = sqlx::query_as(
        r#"
        SELECT s.id,
               s.reference,
               COALESCE(s.document_kind, 'invoice') AS document_kind,
               s.status,
               s.sale_date,
               TRIM(COALESCE(c.last_name, '') || ' ' || COALESCE(c.first_name, '')) AS client_name,
               s.total_ht_cents,
               s.total_ttc_cents,
               COALESCE(p.paid, 0) AS paid_cents,
               s.locked_at,
               s.updated_at
        FROM sales s
        LEFT JOIN clients c ON c.id = s.client_id
        LEFT JOIN (
            SELECT sale_id, SUM(amount_cents) AS paid FROM payments
            WHERE deleted_at IS NULL GROUP BY sale_id
        ) p ON p.sale_id = s.id
        WHERE s.deleted_at IS NULL
          AND ($1 IS NULL OR s.reference LIKE $1 OR s.legacy_ref LIKE $1 OR c.last_name LIKE $1 OR c.first_name LIKE $1)
          AND ($2 IS NULL OR s.status = $2)
          AND ($3 IS NULL OR s.client_id = $3)
          AND ($4 IS NULL OR COALESCE(NULLIF(s.sale_date, ''), substr(s.locked_at, 1, 10), '') >= $4)
          AND ($5 IS NULL OR COALESCE(NULLIF(s.sale_date, ''), substr(s.locked_at, 1, 10), '') <= $5)
        ORDER BY COALESCE(s.locked_at, s.updated_at) DESC
        LIMIT $6 OFFSET $7
        "#,
    )
    .bind(needle)
    .bind(&q.status)
    .bind(&q.client_id)
    .bind(&q.from)
    .bind(&q.to)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

pub(crate) async fn show(State(state): State<SharedState>, Path(id): Path<Uuid>) -> LocalResult<Json<Value>> {
    Ok(Json(load_sale(&state, id).await?))
}

pub(crate) async fn create(
    State(state): State<SharedState>,
    Json(body): Json<SaleInput>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let status = match body.status.as_deref() {
        None | Some("draft") => "draft",
        Some("quote") => "quote",
        Some(other) => {
            return Err(LocalError::BadRequest(format!(
                "un document ne peut pas être créé au statut « {other} » ; il doit être validé"
            )))
        }
    };

    let id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    let device_id = state.device_id().await?;
    let totals = compute(&body.lines)?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        r#"
        INSERT INTO sales (
            id, client_id, vehicle_id, status, document_kind, sale_date,
            total_ht_cents, total_vat_cents, total_ttc_cents, notes,
            created_at, updated_at, row_version, field_meta, company_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$11,1,'{}',$12)
        "#,
    )
    .bind(id.to_string())
    .bind(&body.client_id)
    .bind(&body.vehicle_id)
    .bind(status)
    .bind(match body.document_kind.as_deref() {
        Some("receipt") => "receipt",
        Some("quote") => "quote",
        _ if status == "quote" => "quote",
        _ => "invoice",
    })
    .bind(
        body.sale_date
            .clone()
            .unwrap_or_else(|| now[..10].to_string()),
    )
    .bind(totals.total_ht_cents)
    .bind(totals.total_vat_cents)
    .bind(totals.total_ttc_cents)
    .bind(&body.notes)
    .bind(&now)
    .bind(state.company_id.read().await.map(|c| c.to_string()))
    .execute(&mut *tx)
    .await?;

    write_lines(&mut tx, id, &body.lines, &totals, &now).await?;
    tx.commit().await?;

    let sale = load_sale(&state, id).await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Sale,
        id,
        OpKind::Insert,
        sale.clone(),
        None,
    )
    .await?;
    state.notify_sync();

    Ok((StatusCode::CREATED, Json(sale)))
}

async fn update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SaleInput>,
) -> LocalResult<Json<Value>> {
    ensure_editable(&state, id).await?;

    let now = Utc::now().to_rfc3339();
    let device_id = state.device_id().await?;
    let totals = compute(&body.lines)?;

    // A body without a date (a repair order refreshing its draft, a form whose
    // date field was emptied) keeps the date the draft already has, and gets
    // today's when it has none. Writing the absent date as NULL is how a draft
    // lost its date and was then sealed without one.
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        r#"
        UPDATE sales SET client_id = $2, vehicle_id = $3,
            sale_date = COALESCE(NULLIF(TRIM($4), ''), NULLIF(TRIM(sale_date), ''), $10),
            notes = $5,
            total_ht_cents = $6, total_vat_cents = $7, total_ttc_cents = $8,
            updated_at = $9, row_version = row_version + 1
        WHERE id = $1 AND locked_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .bind(&body.client_id)
    .bind(&body.vehicle_id)
    .bind(&body.sale_date)
    .bind(&body.notes)
    .bind(totals.total_ht_cents)
    .bind(totals.total_vat_cents)
    .bind(totals.total_ttc_cents)
    .bind(&now)
    .bind(&now[..10])
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM sale_lines WHERE sale_id = $1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    write_lines(&mut tx, id, &body.lines, &totals, &now).await?;
    tx.commit().await?;

    let sale = load_sale(&state, id).await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Sale,
        id,
        OpKind::Update,
        sale.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(Json(sale))
}

/// Rewrite an open draft, for callers outside this module (a repair order whose
/// parts changed after its draft invoice was made).
pub(crate) async fn update_draft(
    state: &SharedState,
    id: Uuid,
    input: SaleInput,
) -> LocalResult<Json<Value>> {
    update(State(state.clone()), Path(id), Json(input)).await
}

#[derive(Debug, Deserialize)]
pub struct LinesBody {
    pub lines: Vec<LineInputBody>,
}

async fn replace_lines(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<LinesBody>,
) -> LocalResult<Json<Value>> {
    ensure_editable(&state, id).await?;

    let now = Utc::now().to_rfc3339();
    let totals = compute(&body.lines)?;
    let device_id = state.device_id().await?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("DELETE FROM sale_lines WHERE sale_id = $1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    write_lines(&mut tx, id, &body.lines, &totals, &now).await?;
    sqlx::query(
        "UPDATE sales SET total_ht_cents = $2, total_vat_cents = $3, total_ttc_cents = $4,
         updated_at = $5, row_version = row_version + 1 WHERE id = $1",
    )
    .bind(id.to_string())
    .bind(totals.total_ht_cents)
    .bind(totals.total_vat_cents)
    .bind(totals.total_ttc_cents)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let sale = load_sale(&state, id).await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Sale,
        id,
        OpKind::Update,
        sale.clone(),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(Json(sale))
}

async fn remove(State(state): State<SharedState>, Path(id): Path<Uuid>) -> LocalResult<StatusCode> {
    ensure_editable(&state, id).await?;

    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE sales SET deleted_at = $2 WHERE id = $1 AND locked_at IS NULL")
        .bind(id.to_string())
        .bind(&now)
        .execute(&state.db)
        .await?;

    let device_id = state.device_id().await?;
    let _ = sync_log::append(
        &state.db,
        device_id,
        EntityType::Sale,
        id,
        OpKind::Delete,
        json!({ "id": id.to_string() }),
        None,
    )
    .await?;
    state.notify_sync();
    Ok(StatusCode::NO_CONTENT)
}

// ===========================================================================
// Validation: the one irreversible step
// ===========================================================================

pub(crate) async fn validate(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    let register = fiscal::current(&state).await?;
    let profile = mentions::load(&state.db).await?;
    let check = mentions::check(&profile);
    if !check.complete {
        // Sealing an invoice that is missing a legal mention would produce a
        // document that cannot be corrected except by credit note.
        let labels: Vec<&str> = check.missing.iter().map(|m| m.label.as_str()).collect();
        return Err(LocalError::BadRequest(format!(
            "profil d'entreprise incomplet : {}. Complétez Réglages › Légal & fiscal avant de valider.",
            labels.join(", ")
        )));
    }

    let existing = load_sale(&state, id).await?;
    if existing.get("locked_at").and_then(|v| v.as_str()).is_some() {
        return Err(LocalError::BadRequest(
            "ce document est déjà validé ; utilisez un avoir pour le corriger".into(),
        ));
    }
    if existing.get("status").and_then(|v| v.as_str()) == Some("quote") {
        // Sealing a quote used to turn that very row into the invoice, which made
        // the quote (and the number it was sent under) vanish.
        return Err(LocalError::BadRequest(
            "un devis ne se valide pas : transformez-le d'abord en facture".into(),
        ));
    }
    let lines = read_lines(&state, id).await?;
    if lines.is_empty() {
        return Err(LocalError::BadRequest(
            "un document sans ligne ne peut pas être validé".into(),
        ));
    }

    if let Some(cid) = existing.get("client_id").and_then(|v| v.as_str()) {
        let totals = compute(&lines)?;
        credit::assert_allows(&state.db, cid, totals.total_ttc_cents).await?;
    }

    if let Some(vid) = existing.get("vehicle_id").and_then(|v| v.as_str()) {
        assert_vehicle_matches_sale(
            &state,
            vid,
            existing.get("client_id").and_then(|v| v.as_str()),
        )
        .await?;
    }

    let sealed = seal_document(
        &state,
        &register,
        &profile,
        id,
        DocumentKind::Invoice,
        &lines,
        None,
    )
    .await?;
    after_commit("the follow-ups of the sale", after_sale_sealed(&state, id)).await;
    Ok(Json(sealed))
}

/// Give a quote its number. A quote is not a fiscal document, but it is sent to a
/// customer under a number, and the invoice made from it quotes that number. The
/// number is allocated when the quote is first issued (printed or converted), not
/// when the draft is created, so an abandoned draft burns none.
pub(crate) async fn issue_quote(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    let quote = load_sale(&state, id).await?;
    if quote.get("status").and_then(|v| v.as_str()) != Some("quote") {
        return Err(LocalError::BadRequest("seul un devis peut être émis".into()));
    }
    if quote.get("reference").and_then(|v| v.as_str()).is_some() {
        return Ok(Json(quote));
    }
    let register = fiscal::current(&state).await?;
    let now = Utc::now();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let (_, reference) = fiscal::next_number(&mut tx, &register, DocumentKind::Quote, now.year()).await?;
    let updated = sqlx::query(
        "UPDATE sales SET reference = $2, updated_at = $3, row_version = row_version + 1
          WHERE id = $1 AND reference IS NULL AND locked_at IS NULL AND status = 'quote'",
    )
    .bind(id.to_string())
    .bind(&reference)
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        // Someone issued it between the check and here: roll the number back too.
        return Err(LocalError::BadRequest("ce devis vient d'être émis par une autre opération".into()));
    }
    tx.commit().await?;

    let sale = load_sale(&state, id).await?;
    let device_id = state.device_id().await?;
    let _ = sync_log::append(&state.db, device_id, EntityType::Sale, id, OpKind::Update, sale.clone(), None).await?;
    state.notify_sync();
    Ok(Json(sale))
}

/// Make a draft invoice from a quote. The quote stays as it was, numbered, and the
/// draft remembers it. Doing it again returns the draft that already exists; once
/// that invoice is validated the quote is done.
pub(crate) async fn convert_quote(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let quote = load_sale(&state, id).await?;
    if quote.get("status").and_then(|v| v.as_str()) != Some("quote") {
        return Err(LocalError::BadRequest("seul un devis peut être transformé en facture".into()));
    }
    if let Some(existing) = quote.get("converted_to_sale_id").and_then(|v| v.as_str()) {
        let existing_id = Uuid::parse_str(existing).map_err(|e| LocalError::Internal(e.into()))?;
        let invoice = load_sale(&state, existing_id).await?;
        let reference = invoice.get("reference").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if invoice.get("locked_at").and_then(|v| v.as_str()).is_some() {
            return Err(LocalError::BadRequest(format!(
                "ce devis a déjà été facturé ({reference})"
            )));
        }
        return Ok((StatusCode::OK, Json(invoice)));
    }
    let lines = read_lines(&state, id).await?;
    if lines.is_empty() {
        return Err(LocalError::BadRequest("un devis sans ligne ne peut pas être facturé".into()));
    }
    // The invoice cites the quote's number, so the quote has to have one.
    let Json(quote) = issue_quote(State(state.clone()), Path(id)).await?;
    let quote_ref = quote.get("reference").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let notes = match quote.get("notes").and_then(|v| v.as_str()).filter(|n| !n.is_empty()) {
        Some(n) => format!("Suite au devis {quote_ref}.\n{n}"),
        None => format!("Suite au devis {quote_ref}."),
    };

    let (status, Json(draft)) = create(
        State(state.clone()),
        Json(SaleInput {
            client_id: quote.get("client_id").and_then(|v| v.as_str()).map(String::from),
            vehicle_id: quote.get("vehicle_id").and_then(|v| v.as_str()).map(String::from),
            status: Some("draft".into()),
            document_kind: Some("invoice".into()),
            notes: Some(notes),
            lines,
            ..Default::default()
        }),
    )
    .await?;
    let draft_id = draft.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    sqlx::query("UPDATE sales SET source_quote_id = $2 WHERE id = $1")
        .bind(&draft_id)
        .bind(id.to_string())
        .execute(&state.db)
        .await?;
    let draft_uuid = Uuid::parse_str(&draft_id).map_err(|e| LocalError::Internal(e.into()))?;
    let draft = load_sale(&state, draft_uuid).await?;
    // The link to the quote was written after the draft was announced to the other
    // devices; announce it again so they know where this invoice comes from.
    let device_id = state.device_id().await?;
    let _ = sync_log::append(&state.db, device_id, EntityType::Sale, draft_uuid, OpKind::Update, draft.clone(), None).await?;
    state.notify_sync();
    Ok((status, Json(draft)))
}

#[derive(Debug, Deserialize, Default)]
pub struct CreditNoteBody {
    /// Lines to credit. Empty means the whole document.
    #[serde(default)]
    pub lines: Vec<LineInputBody>,
    pub reason: Option<String>,
}

/// Issue the "minus" operation § 90 requires: a new, separately numbered
/// document that reverses part or all of a validated one.
pub(crate) async fn credit_note(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreditNoteBody>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let register = fiscal::current(&state).await?;
    let profile = mentions::load(&state.db).await?;
    // An avoir is a document like the invoice it corrects and carries the same
    // seller identity: it was the one path that skipped the completeness check.
    let check = mentions::check(&profile);
    if !check.complete {
        let labels: Vec<&str> = check.missing.iter().map(|m| m.label.as_str()).collect();
        return Err(LocalError::BadRequest(format!(
            "profil d'entreprise incomplet : {}. Complétez Réglages › Légal & fiscal avant d'émettre un avoir.",
            labels.join(", ")
        )));
    }

    let original = load_sale(&state, id).await?;
    if original.get("locked_at").and_then(|v| v.as_str()).is_none() {
        return Err(LocalError::BadRequest(
            "seul un document validé peut faire l'objet d'un avoir ; un brouillon se modifie directement".into(),
        ));
    }

    if original.get("document_kind").and_then(|v| v.as_str()) == Some("credit_note") {
        return Err(LocalError::BadRequest(
            "un avoir ne peut pas être crédité ; émettez une facture si l'opération doit être rétablie".into(),
        ));
    }

    // Two avoirs for the same document must not both see the same remaining
    // quantity, so the check, the insert and the seal happen one at a time.
    static CREDIT_NOTE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one_at_a_time = CREDIT_NOTE.lock().await;

    let source = credit_source(&state, id, &body.lines).await?;

    // Negating the original lines is what makes the avoir cancel the invoice to
    // the cent: the VAT engine rounds symmetrically for exactly this reason.
    // Plain negation (not abs) so a returned line on the original is credited
    // back in the right direction.
    let credited: Vec<LineInputBody> = source
        .iter()
        .map(|l| LineInputBody {
            quantity_milli: -l.quantity_milli,
            ..l.clone()
        })
        .collect();
    let totals = compute(&credited)?;

    let note_id = Uuid::new_v4();
    let now_at = Utc::now();
    let now = now_at.to_rfc3339();
    let device_id = state.device_id().await?;
    let actor = fiscal::current_actor(&state).await;
    let corrected_ref = original
        .get("reference")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // One transaction for the row, its lines, the number, the journal entry and the
    // lock. The row used to be committed first and sealed afterwards, so a failure
    // in between left an editable "credit_note" draft behind that had to be cleaned
    // up by hand (and that `validate` could have sealed as a negative invoice).
    // Now a failure leaves nothing at all.
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        r#"
        INSERT INTO sales (
            id, client_id, vehicle_id, status, document_kind, sale_date,
            total_ht_cents, total_vat_cents, total_ttc_cents, notes,
            corrects_sale_id, created_at, updated_at, row_version, field_meta, company_id
        ) VALUES ($1,$2,$3,'credit_note','credit_note',$4,$5,$6,$7,$8,$9,$10,$10,1,'{}',$11)
        "#,
    )
    .bind(note_id.to_string())
    .bind(original.get("client_id").and_then(|v| v.as_str()))
    .bind(original.get("vehicle_id").and_then(|v| v.as_str()))
    .bind(&now[..10])
    .bind(totals.total_ht_cents)
    .bind(totals.total_vat_cents)
    .bind(totals.total_ttc_cents)
    .bind(body.reason.clone())
    .bind(id.to_string())
    .bind(&now)
    .bind(state.company_id.read().await.map(|c| c.to_string()))
    .execute(&mut *tx)
    .await?;
    write_lines(&mut tx, note_id, &credited, &totals, &now).await?;
    let lot = seal_in_tx(
        &mut tx,
        &register,
        &profile,
        note_id,
        DocumentKind::CreditNote,
        &credited,
        corrected_ref,
        now_at,
        actor,
    )
    .await?;
    let reopened = reopen_orders_if_cancelled(&mut tx, id, &original, &now).await?;
    tx.commit().await?;

    // From here the credit note is sealed: see `after_commit`.
    after_commit("announce the journal entry", fiscal::replicate(&state.db, device_id, &register, &lot.entry, &lot.links)).await;
    let sealed = after_commit("read the credit note back", load_sale(&state, note_id)).await;
    if let Some(sealed) = &sealed {
        after_commit("announce the credit note", sync_log::append(&state.db, device_id, EntityType::Sale, note_id, OpKind::Insert, sealed.clone(), None)).await;
    }
    // After the credit note itself, which the movements refer to.
    after_commit("announce the stock movements", emit_stock_movements(&state, note_id)).await;
    for order_id in reopened {
        if let Ok(payload) = crate::local_server::routes::workshop::show_payload(&state, order_id).await {
            let _ = sync_log::append(
                &state.db,
                device_id,
                EntityType::WorkshopOrder,
                order_id,
                OpKind::Update,
                payload,
                None,
            )
            .await;
        }
    }
    state.notify_sync();

    let sealed = sealed.unwrap_or_else(|| json!({ "id": note_id.to_string(), "reference": lot.reference, "status": "credit_note", "document_kind": "credit_note" }));
    Ok((StatusCode::CREATED, Json(sealed)))
}

/// When avoirs have taken back the whole of an invoice, the repair order that was
/// billed on it is no longer billed: it goes back to "Prêt" with no invoice, so it
/// can be corrected and invoiced again. (It used to stay "Facturé" forever, pointing
/// at an invoice that no longer stood.) Returns the orders that moved.
async fn reopen_orders_if_cancelled(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    original_id: Uuid,
    original: &Value,
    now: &str,
) -> LocalResult<Vec<Uuid>> {
    let original_total = original.get("total_ttc_cents").and_then(|v| v.as_i64()).unwrap_or(0);
    let (credited,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(total_ttc_cents), 0) FROM sales
         WHERE corrects_sale_id = $1 AND locked_at IS NOT NULL AND deleted_at IS NULL",
    )
    .bind(original_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if original_total <= 0 || credited + original_total != 0 {
        return Ok(Vec::new());
    }
    let orders: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM workshop_orders WHERE sale_id = $1 AND deleted_at IS NULL AND status = 'invoiced'",
    )
    .bind(original_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE workshop_orders SET status = 'ready', sale_id = NULL, updated_at = $2,
                row_version = row_version + 1
          WHERE sale_id = $1 AND deleted_at IS NULL AND status = 'invoiced'",
    )
    .bind(original_id.to_string())
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(orders.iter().filter_map(|(id,)| Uuid::parse_str(id).ok()).collect())
}

type LineIdentity = (Option<String>, Option<String>, i64, i64, i64, Option<i64>);

fn line_identity(l: &LineInputBody) -> LineIdentity {
    // The HT price of a TTC line is a rounded display value; two lines are the
    // same line when their TTC price is.
    (
        l.product_id.clone(),
        l.label.clone(),
        if l.unit_price_ttc_cents.is_some() { 0 } else { l.unit_price_ht_cents },
        l.vat_rate_bp,
        l.discount_bp,
        l.unit_price_ttc_cents,
    )
}

/// The lines an avoir may credit: what the original carries, minus what earlier
/// avoirs already took back. An empty request means "everything that is left".
/// A line the original does not have, or more of one than is left, is refused,
/// so an avoir can never exceed the document it corrects.
async fn credit_source(
    state: &SharedState,
    original_id: Uuid,
    requested: &[LineInputBody],
) -> LocalResult<Vec<LineInputBody>> {
    // identity -> (template line, quantity still creditable, in the original's sign)
    let mut left: Vec<(LineIdentity, LineInputBody, i64)> = Vec::new();
    for l in read_lines(state, original_id).await? {
        let key = line_identity(&l);
        match left.iter_mut().find(|e| e.0 == key) {
            Some(e) => e.2 += l.quantity_milli,
            None => left.push((key, l.clone(), l.quantity_milli)),
        }
    }

    // Earlier avoirs store their lines negated, so adding them takes quantity off.
    let earlier: Vec<(Option<String>, Option<String>, Option<i64>, i64, i64, i64, Option<i64>, String)> = sqlx::query_as(
        r#"
        SELECT l.product_id, l.label, l.unit_price_ht_cents, l.vat_rate_bp, l.discount_bp, l.quantity_milli,
               l.unit_price_ttc_cents, l.price_basis
        FROM sale_lines l
        JOIN sales s ON s.id = l.sale_id
        WHERE s.corrects_sale_id = $1 AND s.locked_at IS NOT NULL
          AND s.deleted_at IS NULL AND l.deleted_at IS NULL
        "#,
    )
    .bind(original_id.to_string())
    .fetch_all(&state.db)
    .await?;
    for (product_id, label, unit_ht, vat, discount, qty, unit_ttc, basis) in earlier {
        let key = if basis == "ttc" {
            (product_id, label, 0, vat, discount, unit_ttc)
        } else {
            (product_id, label, unit_ht.unwrap_or(0), vat, discount, None)
        };
        if let Some(e) = left.iter_mut().find(|e| e.0 == key) {
            e.2 += qty;
        }
    }

    let source: Vec<LineInputBody> = if requested.is_empty() {
        left.iter()
            .filter(|e| e.2 != 0)
            .map(|e| LineInputBody {
                quantity_milli: e.2,
                ..e.1.clone()
            })
            .collect()
    } else {
        let mut out = Vec::with_capacity(requested.len());
        for line in requested {
            let normalized = line.normalized()?;
            let line = &normalized;
            let name = line.label.clone().unwrap_or_else(|| "ligne".into());
            let Some(e) = left.iter_mut().find(|e| e.0 == line_identity(line)) else {
                return Err(LocalError::BadRequest(format!(
                    "la ligne « {name} » ne figure pas sur le document d'origine"
                )));
            };
            if line.quantity_milli == 0
                || line.quantity_milli.signum() != e.2.signum()
                || line.quantity_milli.abs() > e.2.abs()
            {
                return Err(LocalError::BadRequest(format!(
                    "la quantité à créditer pour « {name} » dépasse ce qui reste à créditer"
                )));
            }
            e.2 -= line.quantity_milli;
            out.push(line.clone());
        }
        out
    };

    if source.is_empty() {
        return Err(LocalError::BadRequest(
            "ce document a déjà été entièrement crédité".into(),
        ));
    }
    Ok(source)
}

/// A step that follows a committed record: telling the other PCs, reading the row back, the
/// stock and workshop follow-ups. The record exists whatever happens here, so a failure is
/// logged and never answered to the caller, who would record the same thing a second time.
pub(crate) async fn after_commit<T, E: std::fmt::Display>(
    step: &str,
    work: impl std::future::Future<Output = Result<T, E>>,
) -> Option<T> {
    match work.await {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::error!(%error, step, "a step after a recorded operation failed");
            None
        }
    }
}

/// Freeze, number, sign and lock. Everything here happens in one transaction.
pub(crate) async fn seal_document(
    state: &SharedState,
    register: &Register,
    profile: &mentions::CompanyProfile,
    id: Uuid,
    kind: DocumentKind,
    lines: &[LineInputBody],
    corrects: Option<String>,
) -> LocalResult<Value> {
    let now = Utc::now();
    let actor = fiscal::current_actor(state).await;
    let device_id = state.device_id().await.map_err(LocalError::Internal)?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let lot = seal_in_tx(
        &mut tx,
        register,
        profile,
        id,
        kind,
        lines,
        corrects,
        now,
        actor,
    )
    .await?;
    tx.commit().await?;

    // From here the document is sealed: see `after_commit`.
    after_commit("announce the journal entry", fiscal::replicate(&state.db, device_id, register, &lot.entry, &lot.links)).await;
    let sale = after_commit("read the document back", load_sale(state, id)).await;
    if let Some(sale) = &sale {
        after_commit("announce the document", sync_log::append(&state.db, device_id, EntityType::Sale, id, OpKind::Update, sale.clone(), None)).await;
    }
    state.notify_sync();

    Ok(sale.unwrap_or_else(|| json!({ "id": id.to_string(), "reference": lot.reference, "status": kind.as_str(), "document_kind": kind.as_str() })))
}

pub(crate) struct SealedLot {
    pub entry: motodesk_fiscal::FiscalEntry,
    pub links: EntryLinks,
    pub reference: String,
}

/// Same as [`seal_document`] but inside the caller's transaction, so a till
/// checkout can seal and take payment without a hole between the two.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn seal_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    register: &Register,
    profile: &mentions::CompanyProfile,
    id: Uuid,
    kind: DocumentKind,
    lines: &[LineInputBody],
    corrects: Option<String>,
    now: chrono::DateTime<Utc>,
    actor: Option<String>,
) -> LocalResult<SealedLot> {
    let totals = compute(lines)?;
    let (_, reference) = fiscal::next_number(tx, register, kind, now.year()).await?;

    let client: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT COALESCE(first_name, ''), last_name, COALESCE(address, ''), COALESCE(email, '')
         FROM clients WHERE id = (SELECT client_id FROM sales WHERE id = $1)",
    )
    .bind(id.to_string())
    .fetch_optional(&mut **tx)
    .await?;

    let payload = json!({
        "reference": reference,
        "document_kind": kind.as_str(),
        "corrects": corrects,
        "sale_id": id.to_string(),
        "issued_at": fiscal::format_stamp(now),
        "seller": {
            "name": profile.name,
            "address": profile.address,
            "postal_code": profile.postal_code,
            "city": profile.city,
            "siret": profile.siret,
            "vat_number": profile.vat_number,
            "legal_form": profile.legal_form,
            "rcs_city": profile.rcs_city,
            "ape_code": profile.ape_code,
            "capital_eur": profile.capital_eur,
        },
        "buyer": client.as_ref().map(|(first, last, address, email)| json!({
            "name": format!("{last} {first}").trim().to_string(),
            "address": address,
            "email": email,
        })),
        "lines": lines.iter().zip(totals.lines.iter()).map(|(input, computed)| json!({
            "label": input.label,
            "product_id": input.product_id,
            "quantity_milli": input.quantity_milli,
            "unit_price_ht_cents": input.normalized().map(|n| n.unit_price_ht_cents).unwrap_or(input.unit_price_ht_cents),
            "unit_price_ttc_cents": input.unit_price_ttc_cents,
            "discount_bp": input.discount_bp,
            "vat_rate_bp": input.vat_rate_bp,
            "net_ht_cents": computed.net_ht_cents,
            "net_ttc_cents": computed.net_ttc_cents,
        })).collect::<Vec<_>>(),
        "vat_breakdown": totals.buckets.iter().map(|b| json!({
            "rate_bp": b.rate_bp,
            "base_ht_cents": b.base_ht_cents,
            "vat_cents": b.vat_cents,
            "total_ttc_cents": b.total_ttc_cents,
        })).collect::<Vec<_>>(),
        "total_ht_cents": totals.total_ht_cents,
        "total_vat_cents": totals.total_vat_cents,
        "total_ttc_cents": totals.total_ttc_cents,
        "payment_terms": mentions::payment_terms_text(profile),
        "software_version": fiscal::SOFTWARE_VERSION,
    });

    let links = EntryLinks {
        document_ref: Some(reference.clone()),
        sale_id: Some(id),
        total_ttc_cents: totals.total_ttc_cents,
        actor_id: actor.clone(),
    };

    let event = match kind {
        DocumentKind::CreditNote => FiscalEventType::CreditNote,
        DocumentKind::Receipt => FiscalEventType::Receipt,
        _ => FiscalEventType::Invoice,
    };
    let entry = fiscal::append(tx, register, event, now, payload, links.clone()).await?;

    let status = match kind {
        DocumentKind::CreditNote => "credit_note",
        DocumentKind::Receipt => "receipt",
        _ => "invoice",
    };
    // A sealed document always carries a date. The one the draft was given is
    // kept; a draft that reaches this point without one (written by an older
    // build, an import or another PC) takes the day it is sealed, because
    // nothing can add it afterwards: the row is frozen from here on.
    let sealed_row = sqlx::query(
        r#"
        UPDATE sales SET reference = $2, status = $3, document_kind = $4,
            total_ht_cents = $5, total_vat_cents = $6, total_ttc_cents = $7,
            fiscal_register_id = $8, fiscal_seq = $9, locked_at = $10,
            sale_date = COALESCE(NULLIF(TRIM(sale_date), ''), substr($10, 1, 10)),
            updated_at = $10, row_version = row_version + 1
        WHERE id = $1 AND locked_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .bind(&reference)
    .bind(status)
    .bind(kind.as_str())
    .bind(totals.total_ht_cents)
    .bind(totals.total_vat_cents)
    .bind(totals.total_ttc_cents)
    .bind(register.id.to_string())
    .bind(entry.seq)
    .bind(now.to_rfc3339())
    .execute(&mut **tx)
    .await?;
    // Two validations of the same document can race past the check made before
    // the transaction. The loser has already taken a number and written a
    // journal entry in this transaction; failing here rolls all of it back.
    if sealed_row.rows_affected() != 1 {
        return Err(LocalError::BadRequest(
            "ce document vient d'être validé par une autre opération".into(),
        ));
    }

    apply_stock_movements(tx, lines, &now.to_rfc3339(), &id.to_string(), kind.as_str()).await?;

    Ok(SealedLot {
        entry,
        links,
        reference,
    })
}

pub(crate) async fn assert_stock_vehicle_sellable(state: &SharedState, vehicle_id: &str) -> LocalResult<()> {
    let row: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT is_stock, client_id FROM vehicles WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(vehicle_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((is_stock, owner)) = row else {
        return Err(LocalError::BadRequest("véhicule introuvable".into()));
    };
    if is_stock == 0 && owner.is_some() {
        return Err(LocalError::BadRequest(
            "ce véhicule n'est plus en stock ; il a déjà un propriétaire".into(),
        ));
    }
    Ok(())
}

/// A draft that names a vehicle is either a sale of a stock bike or an invoice
/// for work on (or the sale of parts for) a customer's own bike. The first needs
/// the bike to still be in stock; the second needs the bike to belong to the
/// customer being invoiced. What must never validate is a draft for a bike that
/// has since been sold to someone else.
pub(crate) async fn assert_vehicle_matches_sale(
    state: &SharedState,
    vehicle_id: &str,
    client_id: Option<&str>,
) -> LocalResult<()> {
    let row: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT is_stock, client_id FROM vehicles WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(vehicle_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((is_stock, owner)) = row else {
        return Err(LocalError::BadRequest("véhicule introuvable".into()));
    };
    if is_stock == 0 {
        if let (Some(owner), Some(client)) = (owner.as_deref(), client_id) {
            if owner != client {
                return Err(LocalError::BadRequest(
                    "ce véhicule n'est plus en stock : il appartient désormais à un autre client".into(),
                ));
            }
        }
    }
    Ok(())
}

/// After a seal: mark the OR invoiced, transfer a stock bike to the buyer.
pub(crate) async fn after_sale_sealed(state: &SharedState, sale_id: Uuid) -> LocalResult<()> {
    let now = Utc::now().to_rfc3339();
    let device_id = state.device_id().await?;

    let orders: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM workshop_orders WHERE sale_id = $1 AND deleted_at IS NULL AND status != 'invoiced'",
    )
    .bind(sale_id.to_string())
    .fetch_all(&state.db)
    .await?;
    sqlx::query(
        r#"
        UPDATE workshop_orders
        SET status = 'invoiced', updated_at = $2, row_version = row_version + 1
        WHERE sale_id = $1 AND deleted_at IS NULL AND status != 'invoiced'
        "#,
    )
    .bind(sale_id.to_string())
    .bind(&now)
    .execute(&state.db)
    .await?;
    // Status changes made as a side effect of sealing are changes like any other:
    // left out of the change log they never reached the other PCs, which went on
    // showing the order as open (and billable again).
    for (order_id,) in orders {
        if let Ok(order_uuid) = Uuid::parse_str(&order_id) {
            if let Ok(payload) =
                crate::local_server::routes::workshop::show_payload(state, order_uuid).await
            {
                let _ = sync_log::append(
                    &state.db,
                    device_id,
                    EntityType::WorkshopOrder,
                    order_uuid,
                    OpKind::Update,
                    payload,
                    None,
                )
                .await;
            }
        }
    }

    let pair: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT vehicle_id, client_id FROM sales WHERE id = $1",
    )
    .bind(sale_id.to_string())
    .fetch_optional(&state.db)
    .await?;
    if let Some((Some(vid), Some(cid))) = pair {
        sqlx::query(
            r#"
            UPDATE vehicles
            SET is_stock = 0, client_id = $2, updated_at = $3, row_version = row_version + 1
            WHERE id = $1 AND is_stock = 1 AND deleted_at IS NULL
            "#,
        )
        .bind(&vid)
        .bind(&cid)
        .bind(&now)
        .execute(&state.db)
        .await?;
        if let Ok(vehicle_uuid) = Uuid::parse_str(&vid) {
            if let Ok(payload) =
                crate::local_server::routes::clients::load_vehicle(state, vehicle_uuid).await
            {
                let _ = sync_log::append(
                    &state.db,
                    device_id,
                    EntityType::Vehicle,
                    vehicle_uuid,
                    OpKind::Update,
                    payload,
                    None,
                )
                .await;
            }
        }
    }
    emit_stock_movements(state, sale_id).await?;
    Ok(())
}

/// Tell the other devices about the stock a document moved. The stock of a product
/// is the sum of its movements: a sale made on one PC used to change that PC's
/// shelf only, and every other PC went on showing the old figure. A receiver adds
/// the movement once (it knows it by its id) and so lands on the same number.
pub(crate) async fn emit_stock_movements(state: &SharedState, sale_id: Uuid) -> LocalResult<()> {
    type Row = (String, String, Option<String>, String, i64, Option<i64>, Option<String>, String, String);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, product_id, supplier_id, kind, quantity_milli, unit_cost_ht_cents, reason, created_at, updated_at
           FROM stock_movements WHERE sale_id = $1 AND deleted_at IS NULL",
    )
    .bind(sale_id.to_string())
    .fetch_all(&state.db)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let device_id = state.device_id().await?;
    for (id, product_id, supplier_id, kind, qty, cost, reason, created_at, updated_at) in rows {
        let Ok(movement_id) = Uuid::parse_str(&id) else { continue };
        let _ = sync_log::append(
            &state.db,
            device_id,
            EntityType::StockMovement,
            movement_id,
            OpKind::Insert,
            json!({
                "id": id, "product_id": product_id, "supplier_id": supplier_id, "kind": kind,
                "quantity_milli": qty, "unit_cost_ht_cents": cost, "reason": reason,
                "sale_id": sale_id.to_string(), "created_at": created_at, "updated_at": updated_at,
            }),
            None,
        )
        .await?;
    }
    Ok(())
}

// ===========================================================================
// Payments
// ===========================================================================

#[derive(Debug, Deserialize)]
pub struct PaymentBody {
    pub amount_cents: i64,
    pub method: Option<String>,
    pub paid_at: Option<String>,
    pub bank: Option<String>,
    pub cheque_drawer: Option<String>,
    pub notes: Option<String>,
}

async fn add_payment(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PaymentBody>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    if body.amount_cents == 0 {
        return Err(LocalError::BadRequest(
            "le montant d'un règlement ne peut pas être nul".into(),
        ));
    }
    if body.amount_cents < 0 {
        return Err(LocalError::BadRequest(
            "un règlement ne peut pas être négatif ; pour corriger une erreur, annulez le règlement".into(),
        ));
    }
    if body.method.as_deref() == Some("credit") {
        return Err(LocalError::BadRequest(
            "« à crédit » n'est pas un règlement : laissez le solde ouvert, il apparaîtra dans la balance âgée".into(),
        ));
    }

    let register = fiscal::current(&state).await?;
    let sale = load_sale(&state, id).await?;
    if sale.get("locked_at").and_then(|v| v.as_str()).is_none() {
        return Err(LocalError::BadRequest(
            "un règlement ne peut être encaissé que sur un document validé".into(),
        ));
    }
    if sale.get("document_kind").and_then(|v| v.as_str()) == Some("credit_note") {
        return Err(LocalError::BadRequest(
            "on n'encaisse pas un avoir".into(),
        ));
    }

    let payment_id = Uuid::new_v4();
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let paid_at = body.paid_at.clone().unwrap_or_else(|| now_s.clone());
    let actor = fiscal::current_actor(&state).await;
    let device_id = state.device_id().await?;
    let client_id = sale
        .get("client_id")
        .and_then(|v| v.as_str())
        .map(String::from);

    let total = sale
        .get("total_ttc_cents")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let already = paid_total(&state, id).await?;
    let credits = credited_total(&state, id).await?;
    let balance = total + credits - already;
    if balance <= 0 {
        return Err(LocalError::BadRequest(
            "ce document est déjà soldé".into(),
        ));
    }
    if body.amount_cents > balance {
        return Err(LocalError::BadRequest(format!(
            "le règlement ({}) dépasse le solde restant ({}) ; saisissez le montant réellement encaissé, le rendu de monnaie ne se saisit pas ici",
            format_cents(body.amount_cents),
            format_cents(balance)
        )));
    }
    let outstanding = balance - body.amount_cents;

    let links = EntryLinks {
        document_ref: sale
            .get("reference")
            .and_then(|v| v.as_str())
            .map(String::from),
        sale_id: Some(id),
        total_ttc_cents: body.amount_cents,
        actor_id: actor.clone(),
    };

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let entry = fiscal::append(
        &mut tx,
        &register,
        FiscalEventType::Payment,
        now,
        json!({
            "payment_id": payment_id.to_string(),
            "sale_id": id.to_string(),
            "document_ref": links.document_ref,
            "amount_cents": body.amount_cents,
            "method": body.method.clone().unwrap_or_else(|| "cash".into()),
            "paid_at": paid_at,
            "bank": body.bank,
            "cheque_drawer": body.cheque_drawer,
            "outstanding_after_cents": outstanding,
        }),
        links.clone(),
    )
    .await?;

    // The balance was read before the transaction; another payment may have been
    // recorded in between. Appending to the journal takes the write lock, so it
    // can be checked again here without a race.
    let (paid_now,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(amount_cents), 0) FROM payments WHERE sale_id = $1 AND deleted_at IS NULL",
    )
    .bind(id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if paid_now != already {
        return Err(LocalError::BadRequest(
            "le solde du document vient de changer ; vérifiez les règlements et recommencez".into(),
        ));
    }

    sqlx::query(
        r#"
        INSERT INTO payments (
            id, sale_id, client_id, paid_at, amount_cents, outstanding_cents,
            method, bank, cheque_drawer, notes, fiscal_register_id, fiscal_seq,
            created_at, updated_at, row_version, field_meta, company_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13,1,'{}',$14)
        "#,
    )
    .bind(payment_id.to_string())
    .bind(id.to_string())
    .bind(&client_id)
    .bind(&paid_at)
    .bind(body.amount_cents)
    .bind(outstanding)
    .bind(body.method.clone().unwrap_or_else(|| "cash".into()))
    .bind(&body.bank)
    .bind(&body.cheque_drawer)
    .bind(&body.notes)
    .bind(register.id.to_string())
    .bind(entry.seq)
    .bind(&now_s)
    .bind(state.company_id.read().await.map(|c| c.to_string()))
    .execute(&mut *tx)
    .await?;

    // A document settled in full is marked paid; the amounts themselves stay
    // where they are, so this is presentation rather than accounting.
    if outstanding <= 0 {
        sqlx::query("UPDATE sales SET status = 'paid', updated_at = $2 WHERE id = $1 AND status IN ('invoice', 'receipt')")
            .bind(id.to_string())
            .bind(&now_s)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    // From here the money is recorded: see `after_commit`.
    after_commit("announce the journal entry", fiscal::replicate(&state.db, device_id, &register, &entry, &links)).await;
    let payment = after_commit("read the payment back", load_payment(&state, payment_id)).await;
    if let Some(payment) = &payment {
        after_commit("announce the payment", sync_log::append(&state.db, device_id, EntityType::Payment, payment_id, OpKind::Insert, payment.clone(), None)).await;
    }
    state.notify_sync();
    let payment = payment.unwrap_or_else(|| json!({ "id": payment_id.to_string(), "sale_id": id.to_string(), "amount_cents": body.amount_cents }));

    Ok((StatusCode::CREATED, Json(payment)))
}

async fn credited_total(state: &SharedState, sale_id: Uuid) -> LocalResult<i64> {
    let (n,): (i64,) = sqlx::query_as(
        r#"SELECT COALESCE(SUM(total_ttc_cents), 0) FROM sales
           WHERE corrects_sale_id = $1 AND locked_at IS NOT NULL AND deleted_at IS NULL"#,
    )
    .bind(sale_id.to_string())
    .fetch_one(&state.db)
    .await?;
    Ok(n)
}

fn format_cents(cents: i64) -> String {
    format!("{},{:02} €", cents / 100, (cents % 100).abs())
}

/// Cancel a receipt entered in error by recording its opposite, never by
/// deleting it: § 90 again.
async fn reverse_payment(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    let register = fiscal::current(&state).await?;
    let original = load_payment(&state, id).await?;

    if original
        .get("reverses_payment_id")
        .and_then(|v| v.as_str())
        .is_some()
    {
        return Err(LocalError::BadRequest(
            "ce mouvement est déjà une annulation".into(),
        ));
    }
    let already: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM payments WHERE reverses_payment_id = $1 AND deleted_at IS NULL",
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?;
    if already.is_some() {
        return Err(LocalError::BadRequest(
            "ce règlement a déjà été annulé".into(),
        ));
    }

    let amount = -original
        .get("amount_cents")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let sale_id = original
        .get("sale_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let reversal_id = Uuid::new_v4();
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let actor = fiscal::current_actor(&state).await;
    let device_id = state.device_id().await?;

    let links = EntryLinks {
        document_ref: None,
        sale_id: sale_id.as_deref().and_then(|s| Uuid::parse_str(s).ok()),
        total_ttc_cents: amount,
        actor_id: actor.clone(),
    };

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let entry = fiscal::append(
        &mut tx,
        &register,
        FiscalEventType::PaymentReversal,
        now,
        json!({
            "payment_id": reversal_id.to_string(),
            "reverses_payment_id": id.to_string(),
            "sale_id": sale_id,
            "amount_cents": amount,
        }),
        links.clone(),
    )
    .await?;

    sqlx::query(
        r#"
        INSERT INTO payments (
            id, sale_id, client_id, paid_at, amount_cents, outstanding_cents,
            method, notes, reverses_payment_id, fiscal_register_id, fiscal_seq,
            created_at, updated_at, row_version, field_meta, company_id
        ) VALUES ($1,$2,$3,$4,$5,0,$6,$7,$8,$9,$10,$11,$11,1,'{}',$12)
        "#,
    )
    .bind(reversal_id.to_string())
    .bind(&sale_id)
    .bind(original.get("client_id").and_then(|v| v.as_str()))
    .bind(&now_s)
    .bind(amount)
    .bind(original.get("method").and_then(|v| v.as_str()))
    .bind("Annulation de règlement")
    .bind(id.to_string())
    .bind(register.id.to_string())
    .bind(entry.seq)
    .bind(&now_s)
    .bind(state.company_id.read().await.map(|c| c.to_string()))
    .execute(&mut *tx)
    .await?;

    // The invoice was marked paid because it was settled. With the payment taken
    // back it is open again, and must look it.
    sqlx::query(
        r#"
        UPDATE sales
        SET status = CASE document_kind WHEN 'receipt' THEN 'receipt' ELSE 'invoice' END,
            updated_at = $2, row_version = row_version + 1
        WHERE id = $1 AND status = 'paid'
          AND COALESCE(total_ttc_cents, 0)
              + COALESCE((SELECT SUM(c.total_ttc_cents) FROM sales c
                          WHERE c.corrects_sale_id = sales.id AND c.locked_at IS NOT NULL
                            AND c.deleted_at IS NULL), 0)
              - COALESCE((SELECT SUM(p.amount_cents) FROM payments p
                          WHERE p.sale_id = sales.id AND p.deleted_at IS NULL), 0) > 0
        "#,
    )
    .bind(&sale_id)
    .bind(&now_s)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // From here the reversal is recorded: see `after_commit`.
    after_commit("announce the journal entry", fiscal::replicate(&state.db, device_id, &register, &entry, &links)).await;
    let payment = after_commit("read the reversal back", load_payment(&state, reversal_id)).await;
    if let Some(payment) = &payment {
        after_commit("announce the reversal", sync_log::append(&state.db, device_id, EntityType::Payment, reversal_id, OpKind::Insert, payment.clone(), None)).await;
    }
    state.notify_sync();
    let payment = payment.unwrap_or_else(|| json!({ "id": reversal_id.to_string(), "amount_cents": amount }));

    Ok((StatusCode::CREATED, Json(payment)))
}

#[derive(Debug, Deserialize, Default)]
pub struct PaymentQuery {
    pub sale_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<i64>,
}

async fn list_payments(
    State(state): State<SharedState>,
    Query(q): Query<PaymentQuery>,
) -> LocalResult<Json<Vec<Value>>> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let rows: Vec<PaymentRow> = sqlx::query_as(
        r#"
        SELECT p.id, p.sale_id, p.client_id, p.paid_at, p.amount_cents,
               p.outstanding_cents, p.method, p.bank, p.cheque_drawer, p.notes,
               p.reverses_payment_id, p.fiscal_seq, s.reference
        FROM payments p
        LEFT JOIN sales s ON s.id = p.sale_id
        WHERE p.deleted_at IS NULL
          AND ($1 IS NULL OR p.sale_id = $1)
          AND ($2 IS NULL OR substr(COALESCE(p.paid_at, ''), 1, 10) >= $2)
          AND ($3 IS NULL OR substr(COALESCE(p.paid_at, ''), 1, 10) <= $3)
        ORDER BY p.paid_at DESC LIMIT $4
        "#,
    )
    .bind(&q.sale_id)
    .bind(&q.from)
    .bind(&q.to)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(PaymentRow::into_json).collect()))
}

/// Log a print and say whether it is the original or a copy.
///
/// § 50 counts a duplicate as an operation to be traced, and the copy has to be
/// distinguishable from the original — hence the DUPLICATA mention the client
/// stamps on everything after the first. The original is traced too: without it
/// there is no way to tell a second copy from a first one.
pub(crate) async fn reprint(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> LocalResult<Json<Value>> {
    let register = fiscal::current(&state).await?;
    let sale = load_sale(&state, id).await?;
    let reference = sale
        .get("reference")
        .and_then(|v| v.as_str())
        .map(String::from);
    if reference.is_none() {
        return Err(LocalError::BadRequest(
            "seul un document validé peut être imprimé".into(),
        ));
    }

    let previous: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM fiscal_journal
         WHERE sale_id = $1 AND event_type IN ('document_printed', 'document_reprinted')",
    )
    .bind(id.to_string())
    .fetch_one(&state.db)
    .await?;
    let copy_number = previous.0 + 1;
    let duplicata = copy_number > 1;
    let mention = duplicata.then(|| "DUPLICATA".to_string());

    let now = Utc::now();
    let actor = fiscal::current_actor(&state).await;
    let links = EntryLinks {
        document_ref: reference.clone(),
        sale_id: Some(id),
        total_ttc_cents: 0,
        actor_id: actor.clone(),
    };

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let entry = fiscal::append(
        &mut tx,
        &register,
        if duplicata {
            FiscalEventType::DocumentReprinted
        } else {
            FiscalEventType::DocumentPrinted
        },
        now,
        json!({
            "sale_id": id.to_string(),
            "reference": reference,
            "copy_number": copy_number,
            "mention": mention,
        }),
        links.clone(),
    )
    .await?;
    tx.commit().await?;

    // From here the copy is journalled: see `after_commit`.
    if let Some(device_id) = after_commit("find this PC", state.device_id()).await {
        after_commit("announce the journal entry", fiscal::replicate(&state.db, device_id, &register, &entry, &links)).await;
    }
    state.notify_sync();

    Ok(json!({
        "duplicata": duplicata,
        "copy_number": copy_number,
        "mention": mention,
        "journal_seq": entry.seq,
    })
    .into())
}

// ===========================================================================
// Shared helpers
// ===========================================================================

pub(crate) fn compute(lines: &[LineInputBody]) -> LocalResult<DocumentTotals> {
    let inputs: Vec<LineInput> = lines.iter().map(LineInputBody::to_vat_line).collect();
    document_totals(&inputs).map_err(|e| LocalError::BadRequest(e.to_string()))
}

async fn ensure_editable(state: &SharedState, id: Uuid) -> LocalResult<()> {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT locked_at, reference FROM sales WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?;
    let (locked_at, reference) = row.ok_or(LocalError::NotFound)?;
    let converted: Option<(String,)> = sqlx::query_as(
        "SELECT c.id FROM sales c WHERE c.source_quote_id = $1 AND c.deleted_at IS NULL LIMIT 1",
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?;
    if converted.is_some() {
        return Err(LocalError::BadRequest(
            "ce devis a été transformé en facture : il n'est plus modifiable (modifiez la facture, ou supprimez-la pour rouvrir le devis)".into(),
        ));
    }
    if locked_at.is_some() {
        return Err(LocalError::BadRequest(format!(
            "le document {} est validé et ne peut plus être modifié ; établissez un avoir pour le corriger (CGI art. 286 I-3° bis)",
            reference.unwrap_or_else(|| "".into())
        )));
    }
    Ok(())
}

pub(crate) async fn write_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    sale_id: Uuid,
    lines: &[LineInputBody],
    totals: &DocumentTotals,
    now: &str,
) -> LocalResult<()> {
    for (position, (raw, computed)) in lines.iter().zip(totals.lines.iter()).enumerate() {
        let normalized = raw.normalized()?;
        let input = &normalized;
        // A TTC line keeps the price the customer saw; an HT line derives it.
        let (line_ttc, unit_ttc, basis) = match (input.unit_price_ttc_cents, computed.net_ttc_cents) {
            (Some(unit), Some(line)) => (line, unit, "ttc"),
            _ => (
                vat::ttc_from_ht(computed.net_ht_cents, input.vat_rate_bp)
                    .map_err(|e| LocalError::BadRequest(e.to_string()))?,
                vat::ttc_from_ht(input.unit_price_ht_cents, input.vat_rate_bp)
                    .map_err(|e| LocalError::BadRequest(e.to_string()))?,
                "ht",
            ),
        };

        sqlx::query(
            r#"
            INSERT INTO sale_lines (
                id, sale_id, product_id, label, quantity_milli,
                unit_price_ht_cents, unit_price_ttc_cents, vat_rate_bp, discount_bp,
                line_total_ttc_cents, position, created_at, updated_at, row_version, field_meta,
                price_basis
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$12,1,'{}',$13)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(sale_id.to_string())
        .bind(&input.product_id)
        .bind(&input.label)
        .bind(input.quantity_milli)
        .bind(input.unit_price_ht_cents)
        .bind(unit_ttc)
        .bind(input.vat_rate_bp)
        .bind(input.discount_bp)
        .bind(line_ttc)
        .bind(position as i64)
        .bind(now)
        .bind(basis)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Move stock by the sealed quantities. Services are skipped. A credit note
/// carries negative quantities, so the same statement restocks the shelf.
pub(crate) async fn apply_stock_movements(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    lines: &[LineInputBody],
    now: &str,
    sale_id: &str,
    kind: &str,
) -> LocalResult<()> {
    let ledger_kind = if kind == "credit_note" { "credit" } else { "sale" };
    crate::local_server::routes::stock::record_sale_lines(tx, lines, sale_id, ledger_kind, now)
        .await?;
    for line in lines {
        let Some(pid) = line.product_id.as_deref() else {
            continue;
        };
        // In thousandths, like the ledger entry just written for the same line:
        // the shelf figure is the sum of those entries.
        if line.quantity_milli == 0 {
            continue;
        }
        sqlx::query(
            r#"
            UPDATE products
               SET stock_milli = stock_milli - $2,
                   updated_at = $3,
                   row_version = row_version + 1
             WHERE id = $1 AND is_service = 0 AND deleted_at IS NULL
            "#,
        )
        .bind(pid)
        .bind(line.quantity_milli)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// product id, label, quantity, unit price, VAT rate, discount.
type StoredLine = (Option<String>, Option<String>, i64, Option<i64>, i64, i64, Option<i64>, String);

async fn read_lines(state: &SharedState, sale_id: Uuid) -> LocalResult<Vec<LineInputBody>> {
    let rows: Vec<StoredLine> = sqlx::query_as(
        r#"
        SELECT product_id, label, quantity_milli, unit_price_ht_cents, vat_rate_bp, discount_bp,
               unit_price_ttc_cents, price_basis
        FROM sale_lines WHERE sale_id = $1 AND deleted_at IS NULL ORDER BY position ASC
        "#,
    )
    .bind(sale_id.to_string())
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                product_id,
                label,
                quantity_milli,
                unit_price_ht_cents,
                vat_rate_bp,
                discount_bp,
                unit_price_ttc_cents,
                price_basis,
            )| {
                LineInputBody {
                    product_id,
                    label,
                    quantity_milli,
                    unit_price_ht_cents: unit_price_ht_cents.unwrap_or(0),
                    // Only a line that was priced TTC stays TTC; for the others the
                    // stored TTC price is a derived figure, not the price of record.
                    unit_price_ttc_cents: if price_basis == "ttc" { unit_price_ttc_cents } else { None },
                    vat_rate_bp,
                    discount_bp,
                }
            },
        )
        .collect())
}

async fn paid_total(state: &SharedState, sale_id: Uuid) -> LocalResult<i64> {
    let row: (Option<i64>,) = sqlx::query_as(
        "SELECT SUM(amount_cents) FROM payments WHERE sale_id = $1 AND deleted_at IS NULL",
    )
    .bind(sale_id.to_string())
    .fetch_one(&state.db)
    .await?;
    Ok(row.0.unwrap_or(0))
}

pub(crate) async fn load_sale(state: &SharedState, id: Uuid) -> LocalResult<Value> {
    let row: Option<SaleRow> = sqlx::query_as(
        r#"
        SELECT id, client_id, vehicle_id, reference, status,
               COALESCE(document_kind, 'invoice') AS document_kind, sale_date,
               total_ht_cents, total_vat_cents, total_ttc_cents, notes,
               fiscal_register_id, fiscal_seq, locked_at, corrects_sale_id,
               source_quote_id,
               (SELECT c.id FROM sales c WHERE c.source_quote_id = sales.id AND c.deleted_at IS NULL
                 ORDER BY c.created_at DESC LIMIT 1) AS converted_to_sale_id,
               created_at, updated_at
        FROM sales WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?;
    let row = row.ok_or(LocalError::NotFound)?;

    let lines: Vec<SaleLineRow> = sqlx::query_as(
        r#"
        SELECT id, product_id, label, quantity_milli, unit_price_ht_cents,
               unit_price_ttc_cents, vat_rate_bp, discount_bp, line_total_ttc_cents, position,
               price_basis
        FROM sale_lines WHERE sale_id = $1 AND deleted_at IS NULL ORDER BY position ASC
        "#,
    )
    .bind(id.to_string())
    .fetch_all(&state.db)
    .await?;

    let payments: Vec<PaymentRow> = sqlx::query_as(
        r#"
        SELECT p.id, p.sale_id, p.client_id, p.paid_at, p.amount_cents,
               p.outstanding_cents, p.method, p.bank, p.cheque_drawer, p.notes,
               p.reverses_payment_id, p.fiscal_seq, s.reference
        FROM payments p LEFT JOIN sales s ON s.id = p.sale_id
        WHERE p.sale_id = $1 AND p.deleted_at IS NULL ORDER BY p.paid_at ASC
        "#,
    )
    .bind(id.to_string())
    .fetch_all(&state.db)
    .await?;

    let paid: i64 = payments.iter().map(|p| p.amount_cents).sum();
    let total = row.total_ttc_cents.unwrap_or(0);

    let mut out = serde_json::to_value(&row).unwrap_or(json!({}));
    if let Some(obj) = out.as_object_mut() {
        obj.insert(
            "lines".into(),
            serde_json::to_value(&lines).unwrap_or(json!([])),
        );
        obj.insert(
            "payments".into(),
            Value::Array(payments.into_iter().map(PaymentRow::into_json).collect()),
        );
        obj.insert("paid_cents".into(), json!(paid));
        obj.insert("outstanding_cents".into(), json!(total - paid));
    }
    Ok(out)
}

pub(crate) async fn load_payment(state: &SharedState, id: Uuid) -> LocalResult<Value> {
    let row: Option<PaymentRow> = sqlx::query_as(
        r#"
        SELECT p.id, p.sale_id, p.client_id, p.paid_at, p.amount_cents,
               p.outstanding_cents, p.method, p.bank, p.cheque_drawer, p.notes,
               p.reverses_payment_id, p.fiscal_seq, s.reference
        FROM payments p LEFT JOIN sales s ON s.id = p.sale_id
        WHERE p.id = $1 AND p.deleted_at IS NULL
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?;
    Ok(row.ok_or(LocalError::NotFound)?.into_json())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct SaleRow {
    id: String,
    client_id: Option<String>,
    vehicle_id: Option<String>,
    reference: Option<String>,
    status: String,
    document_kind: String,
    sale_date: Option<String>,
    total_ht_cents: Option<i64>,
    total_vat_cents: Option<i64>,
    total_ttc_cents: Option<i64>,
    notes: Option<String>,
    fiscal_register_id: Option<String>,
    fiscal_seq: Option<i64>,
    locked_at: Option<String>,
    corrects_sale_id: Option<String>,
    /// On an invoice made from a quote: the quote.
    source_quote_id: Option<String>,
    /// On a quote: the invoice made from it, while it exists.
    converted_to_sale_id: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct SaleLineRow {
    id: String,
    product_id: Option<String>,
    label: Option<String>,
    quantity_milli: i64,
    unit_price_ht_cents: Option<i64>,
    unit_price_ttc_cents: Option<i64>,
    vat_rate_bp: i64,
    discount_bp: i64,
    line_total_ttc_cents: Option<i64>,
    position: i64,
    price_basis: String,
}

#[derive(Debug, sqlx::FromRow)]
struct PaymentRow {
    id: String,
    sale_id: Option<String>,
    client_id: Option<String>,
    paid_at: Option<String>,
    amount_cents: i64,
    outstanding_cents: i64,
    method: Option<String>,
    bank: Option<String>,
    cheque_drawer: Option<String>,
    notes: Option<String>,
    reverses_payment_id: Option<String>,
    fiscal_seq: Option<i64>,
    reference: Option<String>,
}

impl PaymentRow {
    fn into_json(self) -> Value {
        json!({
            "id": self.id,
            "sale_id": self.sale_id,
            "client_id": self.client_id,
            "paid_at": self.paid_at,
            "amount_cents": self.amount_cents,
            "outstanding_cents": self.outstanding_cents,
            "method": self.method,
            "bank": self.bank,
            "cheque_drawer": self.cheque_drawer,
            "notes": self.notes,
            "reverses_payment_id": self.reverses_payment_id,
            "fiscal_seq": self.fiscal_seq,
            "reference": self.reference,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::local_server::test_support::shop;

    pub(crate) fn helmet_and_service() -> Vec<LineInputBody> {
        vec![
            LineInputBody {
                product_id: None,
                label: Some("Casque intégral".into()),
                quantity_milli: 1_000,
                unit_price_ht_cents: 12_000,
                vat_rate_bp: 2_000,
                discount_bp: 0,
                unit_price_ttc_cents: None,
            },
            LineInputBody {
                product_id: None,
                label: Some("Main d'œuvre".into()),
                quantity_milli: 1_500,
                unit_price_ht_cents: 6_000,
                vat_rate_bp: 2_000,
                discount_bp: 0,
                unit_price_ttc_cents: None,
            },
        ]
    }

    pub(crate) async fn new_draft(state: &SharedState) -> Uuid {
        let (_, Json(sale)) = create(
            State(state.clone()),
            Json(SaleInput {
                client_id: None,
                vehicle_id: None,
                sale_date: None,
                notes: None,
                status: None,
                document_kind: None,
                lines: helmet_and_service(),
            }),
        )
        .await
        .unwrap();
        Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap()
    }

    pub(crate) async fn seal(state: &SharedState, id: Uuid) -> Value {
        let Json(sale) = validate(State(state.clone()), Path(id)).await.unwrap();
        sale
    }

    pub(crate) async fn pay(
        state: &SharedState,
        id: Uuid,
        amount_cents: i64,
        method: &str,
    ) -> Value {
        let (_, Json(payment)) = add_payment(
            State(state.clone()),
            Path(id),
            Json(PaymentBody {
                amount_cents,
                method: Some(method.into()),
                paid_at: None,
                bank: None,
                cheque_drawer: None,
                notes: None,
            }),
        )
        .await
        .unwrap();
        payment
    }

    #[tokio::test]
    async fn a_draft_totals_its_lines() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let Json(sale) = show(State(state.clone()), Path(id)).await.unwrap();

        // 120.00 + 1.5 x 60.00 = 210.00 HT, 42.00 VAT.
        assert_eq!(sale["total_ht_cents"], 21_000);
        assert_eq!(sale["total_vat_cents"], 4_200);
        assert_eq!(sale["total_ttc_cents"], 25_200);
        assert_eq!(sale["status"], "draft");
        assert!(sale["locked_at"].is_null());
    }

    #[tokio::test]
    async fn validation_numbers_locks_and_seals() {
        let state = shop().await;
        let id = new_draft(&state).await;

        let sale = seal(&state, id).await;
        let reference = sale["reference"].as_str().unwrap().to_string();

        assert!(reference.starts_with("FA-"), "got {reference}");
        assert_eq!(
            reference.split('-').count(),
            4,
            "number carries a register segment"
        );
        assert!(sale["locked_at"].is_string());
        assert_eq!(sale["fiscal_seq"].as_i64().unwrap(), 2); // after register_opened

        let (event, total): (String, i64) = sqlx::query_as(
            "SELECT event_type, total_ttc_cents FROM fiscal_journal WHERE document_ref = $1",
        )
        .bind(&reference)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(event, "invoice");
        assert_eq!(total, 25_200);
    }

    #[tokio::test]
    async fn a_counter_ticket_is_numbered_rc_and_chained() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let register = fiscal::current(&state).await.unwrap();
        let profile = mentions::load(&state.db).await.unwrap();
        let sale = seal_document(
            &state,
            &register,
            &profile,
            id,
            DocumentKind::Receipt,
            &helmet_and_service(),
            None,
        )
        .await
        .unwrap();
        let reference = sale["reference"].as_str().unwrap().to_string();
        assert!(reference.starts_with("RC-"), "got {reference}");
        assert_eq!(sale["document_kind"], "receipt");
        assert_eq!(sale["status"], "receipt");

        let (event,): (String,) = sqlx::query_as(
            "SELECT event_type FROM fiscal_journal WHERE document_ref = $1",
        )
        .bind(&reference)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(event, "receipt");
    }

    #[tokio::test]
    async fn a_validated_invoice_refuses_edits_and_deletion() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;

        let edit = update(
            State(state.clone()),
            Path(id),
            Json(SaleInput {
                client_id: None,
                vehicle_id: None,
                sale_date: None,
                notes: Some("changed".into()),
                status: None,
                document_kind: None,
                lines: helmet_and_service(),
            }),
        )
        .await;
        assert!(matches!(edit, Err(LocalError::BadRequest(_))));

        let removal = remove(State(state.clone()), Path(id)).await;
        assert!(matches!(removal, Err(LocalError::BadRequest(_))));

        // Still there, still the same.
        let Json(sale) = show(State(state.clone()), Path(id)).await.unwrap();
        assert!(sale["notes"].is_null());
    }

    #[tokio::test]
    async fn validating_twice_is_refused() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;
        assert!(matches!(
            validate(State(state.clone()), Path(id)).await,
            Err(LocalError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn an_empty_document_cannot_be_validated() {
        let state = shop().await;
        let (_, Json(sale)) = create(
            State(state.clone()),
            Json(SaleInput {
                client_id: None,
                vehicle_id: None,
                sale_date: None,
                notes: None,
                status: None,
                document_kind: None,
                lines: vec![],
            }),
        )
        .await
        .unwrap();
        let id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();
        assert!(matches!(
            validate(State(state.clone()), Path(id)).await,
            Err(LocalError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn an_incomplete_company_profile_blocks_validation() {
        let state = shop().await;
        sqlx::query("DELETE FROM app_settings WHERE key = 'legal.siret'")
            .execute(&state.db)
            .await
            .unwrap();

        let id = new_draft(&state).await;
        let err = validate(State(state.clone()), Path(id)).await.unwrap_err();
        match err {
            LocalError::BadRequest(message) => assert!(
                message.contains("SIRET"),
                "the message must name the missing mention, got: {message}"
            ),
            other => panic!("expected a bad request, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_credit_note_cancels_the_invoice_to_the_cent() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let invoice = seal(&state, id).await;

        let (_, Json(note)) = credit_note(
            State(state.clone()),
            Path(id),
            Json(CreditNoteBody {
                lines: vec![],
                reason: Some("Erreur de saisie".into()),
            }),
        )
        .await
        .unwrap();

        assert_eq!(
            note["total_ttc_cents"].as_i64().unwrap(),
            -invoice["total_ttc_cents"].as_i64().unwrap()
        );
        assert!(note["reference"].as_str().unwrap().starts_with("AV-"));
        assert_eq!(note["corrects_sale_id"], invoice["id"]);
        assert!(note["locked_at"].is_string());

        // The original is untouched: correction is by opposite entry, not edit.
        let Json(original) = show(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(original["total_ttc_cents"], invoice["total_ttc_cents"]);
    }

    #[tokio::test]
    async fn a_draft_cannot_be_credited() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let result = credit_note(
            State(state.clone()),
            Path(id),
            Json(CreditNoteBody::default()),
        )
        .await;
        assert!(matches!(result, Err(LocalError::BadRequest(_))));
    }

    #[tokio::test]
    async fn payments_settle_a_document_and_reach_the_journal() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;

        pay(&state, id, 10_000, "card").await;

        let Json(partial) = show(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(partial["paid_cents"], 10_000);
        assert_eq!(partial["outstanding_cents"], 15_200);
        assert_eq!(partial["status"], "invoice");

        pay(&state, id, 15_200, "cash").await;

        let Json(settled) = show(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(settled["outstanding_cents"], 0);
        assert_eq!(settled["status"], "paid");

        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'payment'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn a_payment_needs_a_validated_document() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let result = add_payment(
            State(state.clone()),
            Path(id),
            Json(PaymentBody {
                amount_cents: 1_000,
                method: None,
                paid_at: None,
                bank: None,
                cheque_drawer: None,
                notes: None,
            }),
        )
        .await;
        assert!(matches!(result, Err(LocalError::BadRequest(_))));
    }

    #[tokio::test]
    async fn a_mistaken_receipt_is_reversed_not_deleted() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;

        let payment = pay(&state, id, 25_200, "cash").await;
        let payment_id = Uuid::parse_str(payment["id"].as_str().unwrap()).unwrap();

        let (_, Json(reversal)) = reverse_payment(State(state.clone()), Path(payment_id))
            .await
            .unwrap();
        assert_eq!(reversal["amount_cents"], -25_200);

        let Json(sale) = show(State(state.clone()), Path(id)).await.unwrap();
        assert_eq!(sale["paid_cents"], 0);
        // Both movements remain visible.
        assert_eq!(sale["payments"].as_array().unwrap().len(), 2);

        assert!(matches!(
            reverse_payment(State(state.clone()), Path(payment_id)).await,
            Err(LocalError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn only_copies_carry_the_duplicata_mention() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;

        let Json(first) = reprint(State(state.clone()), Path(id)).await.unwrap();
        let Json(second) = reprint(State(state.clone()), Path(id)).await.unwrap();
        let Json(third) = reprint(State(state.clone()), Path(id)).await.unwrap();

        assert_eq!(first["copy_number"], 1);
        assert_eq!(first["duplicata"], false);
        assert!(first["mention"].is_null());

        assert_eq!(second["copy_number"], 2);
        assert_eq!(second["mention"], "DUPLICATA");
        assert_eq!(third["copy_number"], 3);

        // The original is traced too: without it a second copy could not be
        // told apart from a first one.
        let (originals,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'document_printed'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        let (copies,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'document_reprinted'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!((originals, copies), (1, 2));
    }

    #[tokio::test]
    async fn a_day_of_trading_leaves_a_verifiable_chain() {
        let state = shop().await;

        for _ in 0..3 {
            let id = new_draft(&state).await;
            seal(&state, id).await;
            pay(&state, id, 25_200, "card").await;
        }

        let report = fiscal::verify_all(&state).await.unwrap();
        assert!(report.ok, "chain should verify: {report:?}");
        assert_eq!(report.registers.len(), 1);
        // 1 opening + 3 invoices + 3 payments.
        assert_eq!(report.registers[0].entries, 7);
        assert_eq!(report.registers[0].signatures_verified, 7);
    }

    /// A FEC an accountant's software will refuse is worth nothing, and the one
    /// thing every importer checks is that each voucher balances to the cent.
    #[tokio::test]
    async fn the_fec_balances_voucher_by_voucher() {
        use crate::local_server::fiscal::{fec, mentions};
        use std::collections::BTreeMap;

        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();

        let paid = new_draft(&state).await;
        seal(&state, paid).await;
        pay(&state, paid, 25_200, "cash").await;

        // An unpaid invoice and a credit note, so the export covers the three
        // shapes an entry can take rather than only the happy one.
        let credited = new_draft(&state).await;
        seal(&state, credited).await;
        let (_, Json(_avoir)) = credit_note(
            State(state.clone()),
            Path(credited),
            Json(CreditNoteBody {
                lines: Vec::new(),
                reason: None,
            }),
        )
        .await
        .unwrap();

        let profile = mentions::load(&state.db).await.unwrap();
        let export = fec::build(
            &state.db,
            &register,
            Utc::now().date_naive().year(),
            &profile.siren,
        )
        .await
        .unwrap();

        assert_eq!(
            export.file_name,
            format!(
                "{}FEC{}1231.txt",
                profile.siren,
                Utc::now().date_naive().year()
            )
        );
        assert!(export.line_count >= 8);

        let mut per_voucher: BTreeMap<String, i64> = BTreeMap::new();
        let mut rows = 0;
        for line in export
            .content
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
        {
            let cols: Vec<&str> = line.split('|').collect();
            assert_eq!(cols.len(), 18, "the arrêté fixes eighteen columns: {line}");
            let debit = cents(cols[11]);
            let credit = cents(cols[12]);
            assert!(
                debit >= 0 && credit >= 0,
                "a FEC carries no negative amount"
            );
            *per_voucher.entry(cols[2].to_string()).or_insert(0) += debit - credit;
            rows += 1;
        }

        assert_eq!(rows, export.line_count);
        for (voucher, balance) in per_voucher {
            assert_eq!(balance, 0, "voucher {voucher} does not balance");
        }
    }

    /// "1234,56" back to cents, for reading the export under test.
    fn cents(text: &str) -> i64 {
        let normalised = text.replace(',', ".");
        (normalised.parse::<f64>().unwrap_or(0.0) * 100.0).round() as i64
    }

    #[tokio::test]
    async fn tampering_with_a_journal_row_is_detected() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;

        // The triggers make this impossible through SQL, which is the point;
        // simulate an attacker with raw file access by dropping them first.
        sqlx::query("DROP TRIGGER fiscal_journal_no_update")
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("UPDATE fiscal_journal SET total_ttc_cents = 1, payload = json_set(payload, '$.total_ttc_cents', 1) WHERE seq = 2")
            .execute(&state.db)
            .await
            .unwrap();

        let report = fiscal::verify_all(&state).await.unwrap();
        assert!(!report.ok);
        assert_eq!(report.registers[0].problem_seq, Some(2));
        assert!(report.registers[0].problem.is_some());
    }

    #[tokio::test]
    async fn a_journal_that_lost_its_tail_is_detected() {
        let state = shop().await;
        let first = new_draft(&state).await;
        seal(&state, first).await;
        let second = new_draft(&state).await;
        seal(&state, second).await;
        assert!(fiscal::verify_all(&state).await.unwrap().ok, "intact to begin with");

        // Someone with the file removes the last entries. What is left is a valid
        // chain, so only a comparison with the counters shows it.
        sqlx::query("DROP TRIGGER fiscal_journal_no_delete")
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM fiscal_journal WHERE seq = (SELECT MAX(seq) FROM fiscal_journal)")
            .execute(&state.db)
            .await
            .unwrap();

        let report = fiscal::verify_all(&state).await.unwrap();
        assert!(!report.ok);
        let problem = report.registers[0].problem.clone().unwrap();
        assert!(problem.contains("s'arrête"), "{problem}");
        // The document whose entry went missing is reported too.
        assert!(report.registers[0].findings.iter().any(|f| f.contains("validé")), "{:?}", report.registers[0].findings);
    }

    #[tokio::test]
    async fn unsigned_entries_are_reported() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;
        sqlx::query("DROP TRIGGER fiscal_journal_no_update")
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("UPDATE fiscal_journal SET signature = '' WHERE seq = 2")
            .execute(&state.db)
            .await
            .unwrap();

        let report = fiscal::verify_all(&state).await.unwrap();
        assert!(!report.ok);
        assert_eq!(report.registers[0].unsigned, 1);
        assert!(report.registers[0].problem.clone().unwrap().contains("signature"));
    }

    #[tokio::test]
    async fn the_declared_events_reach_the_journal() {
        use motodesk_fiscal::FiscalEventType;
        let state = shop().await;
        fiscal::journal_event(&state, FiscalEventType::IntegrityChecked, json!({ "ok": true }))
            .await
            .unwrap();
        fiscal::journal_event(&state, FiscalEventType::DataPurged, json!({ "table": "clients", "rows": 3 }))
            .await
            .unwrap();
        fiscal::journal_event(
            &state,
            FiscalEventType::FiscalSettingChanged,
            json!({ "key": "legal.siret", "old": null, "new": "12345678900012" }),
        )
        .await
        .unwrap();
        let kinds: Vec<(String,)> = sqlx::query_as(
            "SELECT event_type FROM fiscal_journal WHERE event_type IN ('integrity_checked','data_purged','fiscal_setting_changed') ORDER BY seq",
        )
        .fetch_all(&state.db)
        .await
        .unwrap();
        assert_eq!(kinds.len(), 3);
        // And they are signed and chained like everything else.
        assert!(fiscal::verify_all(&state).await.unwrap().ok);
        assert!(fiscal::is_fiscal_setting("legal.siret"));
        assert!(!fiscal::is_fiscal_setting("system.theme"));
    }

    #[tokio::test]
    async fn the_journal_refuses_updates_and_deletes() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;

        let update_err = sqlx::query("UPDATE fiscal_journal SET total_ttc_cents = 0")
            .execute(&state.db)
            .await;
        assert!(update_err.is_err());

        let delete_err = sqlx::query("DELETE FROM fiscal_journal")
            .execute(&state.db)
            .await;
        assert!(delete_err.is_err());
    }

    #[tokio::test]
    async fn numbers_restart_per_series_but_never_repeat_within_one() {
        let state = shop().await;
        let mut seen = Vec::new();
        for _ in 0..3 {
            let id = new_draft(&state).await;
            let sale = seal(&state, id).await;
            seen.push(sale["reference"].as_str().unwrap().to_string());
        }

        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "numbers must not repeat: {seen:?}");
        assert!(seen[0].ends_with("00001"));
        assert!(seen[2].ends_with("00003"));
    }

    #[tokio::test]
    async fn the_database_refuses_to_rewrite_a_sealed_document() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let sale = seal(&state, id).await;
        let total = sale["total_ttc_cents"].as_i64().unwrap();
        let payment = pay(&state, id, total, "cash").await;
        let payment_id = payment["id"].as_str().unwrap().to_string();
        let sale_id = id.to_string();

        for sql in [
            "UPDATE sales SET total_ttc_cents = 1 WHERE id = $1",
            "UPDATE sales SET reference = 'FA-FAKE' WHERE id = $1",
            "UPDATE sales SET deleted_at = '2026-01-01T00:00:00Z' WHERE id = $1",
            "UPDATE sales SET status = 'draft' WHERE id = $1",
            "UPDATE sales SET locked_at = NULL WHERE id = $1",
            "DELETE FROM sales WHERE id = $1",
            "UPDATE sale_lines SET quantity_milli = 9000 WHERE sale_id = $1",
            "UPDATE sale_lines SET deleted_at = '2026-01-01T00:00:00Z' WHERE sale_id = $1",
            "DELETE FROM sale_lines WHERE sale_id = $1",
            "UPDATE payments SET amount_cents = 1 WHERE sale_id = $1",
            "DELETE FROM payments WHERE sale_id = $1",
        ] {
            let res = sqlx::query(sql).bind(&sale_id).execute(&state.db).await;
            assert!(res.is_err(), "should have been refused: {sql}");
        }

        // What a sealed document legitimately goes through still works.
        sqlx::query("UPDATE sales SET status = 'paid', updated_at = $2, row_version = row_version + 1 WHERE id = $1")
            .bind(&sale_id)
            .bind(Utc::now().to_rfc3339())
            .execute(&state.db)
            .await
            .expect("a status change to paid must stay possible");
        sqlx::query("UPDATE payments SET updated_at = $2 WHERE id = $1")
            .bind(&payment_id)
            .bind(Utc::now().to_rfc3339())
            .execute(&state.db)
            .await
            .expect("a no-op touch of a sealed payment must stay possible");

        let (kept,): (i64,) = sqlx::query_as("SELECT total_ttc_cents FROM sales WHERE id = $1")
            .bind(&sale_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(kept, total);
    }

    /// Every business write also appends to the change log. Those transactions
    /// used to start deferred, read the next sequence number and then write; when
    /// two overlapped, SQLite refused the upgrade at once with "database is
    /// locked" and the busy timeout never applied.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_writers_wait_their_turn_instead_of_failing() {
        let state = shop().await;
        let device_id = state.device_id().await.unwrap();

        let mut tasks = Vec::new();
        for i in 0..32 {
            let st = state.clone();
            tasks.push(tokio::spawn(async move {
                sync_log::append(
                    &st.db,
                    device_id,
                    EntityType::Client,
                    Uuid::new_v4(),
                    OpKind::Insert,
                    json!({ "i": i }),
                    None,
                )
                .await
                .map(|e| e.seq)
            }));
        }
        let mut seqs = Vec::new();
        for t in tasks {
            seqs.push(t.await.unwrap().expect("a write must wait for the lock, not fail"));
        }
        seqs.sort();
        seqs.dedup();
        assert_eq!(seqs.len(), 32, "sequence numbers must be unique");
    }

    #[tokio::test]
    async fn drafts_stay_editable_at_the_database_level() {
        let state = shop().await;
        let id = new_draft(&state).await;
        sqlx::query("UPDATE sales SET total_ttc_cents = 5 WHERE id = $1")
            .bind(id.to_string())
            .execute(&state.db)
            .await
            .expect("an unsealed draft is not protected");
    }

    fn dated(sale_date: Option<&str>) -> Json<SaleInput> {
        Json(SaleInput {
            sale_date: sale_date.map(String::from),
            lines: helmet_and_service(),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn a_draft_updated_without_a_date_keeps_the_one_it_had() {
        let state = shop().await;
        let (_, Json(sale)) = create(State(state.clone()), dated(Some("2026-10-03"))).await.unwrap();
        let id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();

        // Absent, and emptied in the form: neither erases the date.
        for missing in [None, Some(""), Some("  ")] {
            let Json(sale) = update(State(state.clone()), Path(id), dated(missing)).await.unwrap();
            assert_eq!(sale["sale_date"], "2026-10-03", "after an update sending {missing:?}");
        }

        // A date that is sent still replaces it.
        let Json(sale) = update(State(state.clone()), Path(id), dated(Some("2026-10-05"))).await.unwrap();
        assert_eq!(sale["sale_date"], "2026-10-05");
    }

    #[tokio::test]
    async fn a_draft_that_had_no_date_gets_todays_when_it_is_updated() {
        let state = shop().await;
        let id = new_draft(&state).await;
        // What an older build left behind after an update without a date.
        sqlx::query("UPDATE sales SET sale_date = NULL WHERE id = $1")
            .bind(id.to_string())
            .execute(&state.db)
            .await
            .unwrap();

        let Json(sale) = update(State(state.clone()), Path(id), dated(None)).await.unwrap();
        assert_eq!(sale["sale_date"], sale["updated_at"].as_str().unwrap()[..10]);
    }

    #[tokio::test]
    async fn a_document_validated_after_an_update_without_a_date_is_sealed_with_one() {
        let state = shop().await;
        let (_, Json(sale)) = create(State(state.clone()), dated(Some("2026-10-03"))).await.unwrap();
        let id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();
        let Json(updated) = update(State(state.clone()), Path(id), dated(None)).await.unwrap();
        assert_eq!(updated["sale_date"], "2026-10-03");

        let sealed = seal(&state, id).await;
        assert!(sealed["locked_at"].is_string());
        assert_eq!(sealed["sale_date"], "2026-10-03", "the date the draft was given is kept");
    }

    /// The seal is the last moment a date can be written: afterwards the database
    /// refuses to touch it. A draft can still arrive here without one (written by
    /// an older build, an import, another PC), so the seal fills it in itself.
    #[tokio::test]
    async fn a_draft_without_a_date_is_sealed_on_the_day_it_is_validated() {
        let state = shop().await;
        for missing in [None, Some("")] {
            let id = new_draft(&state).await;
            sqlx::query("UPDATE sales SET sale_date = $2 WHERE id = $1")
                .bind(id.to_string())
                .bind(missing)
                .execute(&state.db)
                .await
                .unwrap();

            let sealed = seal(&state, id).await;
            let locked_at = sealed["locked_at"].as_str().unwrap();
            assert_eq!(sealed["sale_date"], locked_at[..10], "sealed from a draft dated {missing:?}");

            let res = sqlx::query("UPDATE sales SET sale_date = NULL WHERE id = $1")
                .bind(id.to_string())
                .execute(&state.db)
                .await;
            assert!(res.is_err(), "the date of a sealed document cannot be removed");
        }
    }

    #[tokio::test]
    async fn the_date_filter_finds_a_document_that_was_sealed_without_a_date() {
        let state = shop().await;
        // Such rows predate the rule above and cannot be rewritten.
        sqlx::query(
            "INSERT INTO sales (id, status, document_kind, sale_date, total_ttc_cents, locked_at,
                                created_at, updated_at, row_version, field_meta)
             VALUES ($1, 'invoice', 'invoice', NULL, 1200, '2026-11-12T09:00:00Z',
                     '2026-11-12T09:00:00Z', '2026-11-12T09:00:00Z', 1, '{}')",
        )
        .bind(Uuid::new_v4().to_string())
        .execute(&state.db)
        .await
        .unwrap();

        let found = |from: &str, to: &str| {
            let state = state.clone();
            let query = ListQuery {
                from: Some(from.into()),
                to: Some(to.into()),
                ..Default::default()
            };
            async move { list(State(state), Query(query)).await.unwrap().0.len() }
        };
        assert_eq!(found("2026-11-01", "2026-11-30").await, 1, "listed in the month it was sealed");
        assert_eq!(found("2026-10-01", "2026-10-31").await, 0);
        assert_eq!(found("2026-12-01", "2026-12-31").await, 0);
    }

    #[tokio::test]
    async fn list_matches_legacy_ref() {
        let state = shop().await;
        let id = new_draft(&state).await;
        sqlx::query("UPDATE sales SET legacy_ref = 'FAC-2019-0042' WHERE id = $1")
            .bind(id.to_string())
            .execute(&state.db)
            .await
            .unwrap();

        let Json(rows) = list(
            State(state),
            Query(ListQuery {
                q: Some("2019-0042".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert!(
            rows.iter().any(|r| r.id == id.to_string()),
            "imported Motogest pieces must be findable by legacy_ref"
        );
    }

    async fn draft_for_vehicle(
        state: &SharedState,
        client_id: Uuid,
        vehicle_id: Uuid,
    ) -> Uuid {
        let (_, Json(sale)) = create(
            State(state.clone()),
            Json(SaleInput {
                client_id: Some(client_id.to_string()),
                vehicle_id: Some(vehicle_id.to_string()),
                sale_date: None,
                notes: None,
                status: None,
                document_kind: None,
                lines: helmet_and_service(),
            }),
        )
        .await
        .unwrap();
        Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap()
    }

    pub(crate) async fn insert_client(state: &SharedState, name: &str) -> Uuid {
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO clients (id, last_name, created_at, updated_at, row_version, field_meta)
             VALUES ($1,$2,$3,$3,1,'{}')",
        )
        .bind(id.to_string())
        .bind(name)
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();
        id
    }

    async fn insert_owned_bike(state: &SharedState, owner: Uuid) -> Uuid {
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO vehicles (
                id, client_id, vehicle_type, make, model, is_stock, created_at, updated_at
             ) VALUES ($1,$2,'moto','Yamaha','MT-07',0,$3,$3)",
        )
        .bind(id.to_string())
        .bind(owner.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();
        id
    }

    async fn credit(state: &SharedState, id: Uuid, lines: Vec<LineInputBody>) -> LocalResult<Value> {
        credit_note(
            State(state.clone()),
            Path(id),
            Json(CreditNoteBody { lines, reason: None }),
        )
        .await
        .map(|(_, Json(v))| v)
    }

    async fn try_pay(state: &SharedState, id: Uuid, amount_cents: i64, method: &str) -> LocalResult<Value> {
        add_payment(
            State(state.clone()),
            Path(id),
            Json(PaymentBody {
                amount_cents,
                method: Some(method.into()),
                paid_at: None,
                bank: None,
                cheque_drawer: None,
                notes: None,
            }),
        )
        .await
        .map(|(_, Json(v))| v)
    }

    async fn status_of(state: &SharedState, id: Uuid) -> String {
        let (s,): (String,) = sqlx::query_as("SELECT status FROM sales WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        s
    }

    #[tokio::test]
    async fn a_payment_cannot_exceed_the_balance_or_be_negative() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let total = seal(&state, id).await["total_ttc_cents"].as_i64().unwrap();

        assert!(try_pay(&state, id, total + 1, "cash").await.is_err());
        assert!(try_pay(&state, id, -500, "cash").await.is_err());
        assert!(try_pay(&state, id, total / 2, "credit").await.is_err());

        try_pay(&state, id, total - 100, "card").await.unwrap();
        assert_eq!(status_of(&state, id).await, "invoice");
        assert!(try_pay(&state, id, 101, "cash").await.is_err());
        try_pay(&state, id, 100, "cash").await.unwrap();
        assert_eq!(status_of(&state, id).await, "paid");
        assert!(try_pay(&state, id, 1, "cash").await.is_err(), "already settled");
    }

    #[tokio::test]
    async fn reversing_a_payment_reopens_the_invoice() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let total = seal(&state, id).await["total_ttc_cents"].as_i64().unwrap();
        let payment = try_pay(&state, id, total, "cash").await.unwrap();
        assert_eq!(status_of(&state, id).await, "paid");

        let payment_id = Uuid::parse_str(payment["id"].as_str().unwrap()).unwrap();
        let _ = reverse_payment(State(state.clone()), Path(payment_id)).await.unwrap();
        assert_eq!(status_of(&state, id).await, "invoice");
        try_pay(&state, id, total, "card").await.expect("the balance is open again");
    }

    #[tokio::test]
    async fn a_credited_invoice_has_no_balance_left_to_collect() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;
        credit(&state, id, vec![]).await.unwrap();
        assert!(try_pay(&state, id, 100, "cash").await.is_err());
    }

    pub(crate) fn totals_of(lines: &[LineInputBody]) -> vat::DocumentTotals {
        compute(lines).unwrap()
    }

    fn shelf_line(label: &str, ttc: i64, qty_milli: i64) -> LineInputBody {
        LineInputBody {
            product_id: None,
            label: Some(label.into()),
            quantity_milli: qty_milli,
            unit_price_ht_cents: 0,
            unit_price_ttc_cents: Some(ttc),
            vat_rate_bp: 2_000,
            discount_bp: 0,
        }
    }

    async fn draft_with(state: &SharedState, lines: Vec<LineInputBody>) -> Uuid {
        let (_, Json(sale)) = create(
            State(state.clone()),
            Json(SaleInput {
                client_id: None,
                vehicle_id: None,
                sale_date: None,
                notes: None,
                status: None,
                document_kind: None,
                lines,
            }),
        )
        .await
        .unwrap();
        Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap()
    }

    /// The reason for pricing a line TTC: 9,99 EUR at 20 % has no net amount in
    /// cents, so an HT line rang up 10,00 and three of them 29,99.
    #[tokio::test]
    async fn a_shelf_price_is_charged_exactly_and_survives_a_credit_note() {
        let state = shop().await;
        let id = draft_with(&state, vec![shelf_line("Chaîne 520", 999, 3_000)]).await;
        let sale = seal(&state, id).await;
        assert_eq!(sale["total_ttc_cents"], 2_997, "3 x 9,99, not 29,99");
        assert_eq!(
            sale["total_ht_cents"].as_i64().unwrap() + sale["total_vat_cents"].as_i64().unwrap(),
            2_997
        );

        // The stored line keeps what the customer saw.
        let (unit_ttc, basis, line_total): (i64, String, i64) = sqlx::query_as(
            "SELECT unit_price_ttc_cents, price_basis, line_total_ttc_cents FROM sale_lines WHERE sale_id = $1",
        )
        .bind(id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!((unit_ttc, basis.as_str(), line_total), (999, "ttc", 2_997));

        // A credit note for the whole of it cancels it to the cent, and so does
        // taking it back in two halves.
        let first = credit(&state, id, vec![shelf_line("Chaîne 520", 999, 1_000)]).await.unwrap();
        assert_eq!(first["total_ttc_cents"], -999);
        let rest = credit(&state, id, vec![]).await.unwrap();
        assert_eq!(rest["total_ttc_cents"], -1_998);
        assert!(credit(&state, id, vec![]).await.is_err(), "nothing left to credit");
    }

    /// The accounting export must agree with the document it exports.
    fn today() -> chrono::NaiveDate {
        chrono::Local::now().date_naive()
    }

    /// The Z at the till: close the day that is still running, then nothing more
    /// can be rung up on it.
    #[tokio::test]
    async fn closing_today_is_allowed_and_makes_the_day_final() {
        use crate::local_server::fiscal::closures;
        use motodesk_fiscal::ClosureKind;

        let state = shop().await;
        let earlier = new_draft(&state).await;
        let invoice = seal(&state, earlier).await;
        let register = fiscal::current(&state).await.unwrap();

        let closed = closures::close(&state, &register, ClosureKind::Day, today()).await.unwrap();
        assert_eq!(closed.total_ttc_cents, invoice["total_ttc_cents"].as_i64().unwrap());
        // Asking again returns the same closure, not a second answer.
        let again = closures::close(&state, &register, ClosureKind::Day, today()).await.unwrap();
        assert_eq!(again.id, closed.id);

        // A new sale, a payment on the earlier one, a credit note: all refused.
        let later = new_draft(&state).await;
        let refused = validate(State(state.clone()), Path(later)).await.unwrap_err();
        assert!(matches!(&refused, LocalError::BadRequest(m) if m.contains("clôturée")), "{refused:?}");
        let paid = try_pay(&state, earlier, 100, "cash").await.unwrap_err();
        assert!(matches!(&paid, LocalError::BadRequest(m) if m.contains("clôturée")), "{paid:?}");
        let credited = credit(&state, earlier, vec![]).await.unwrap_err();
        assert!(matches!(&credited, LocalError::BadRequest(m) if m.contains("clôturée")), "{credited:?}");

        // The refused attempts left nothing behind.
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fiscal_journal WHERE event_type IN ('invoice','payment','credit_note')")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn the_running_month_and_year_still_cannot_be_closed() {
        use crate::local_server::fiscal::closures;
        use motodesk_fiscal::ClosureKind;

        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        for kind in [ClosureKind::Month, ClosureKind::Year] {
            let err = closures::close(&state, &register, kind, today()).await.unwrap_err();
            assert!(matches!(&err, LocalError::BadRequest(m) if m.contains("pas encore écoulée")), "{err:?}");
        }
        let future = today() + chrono::Duration::days(3);
        assert!(closures::close(&state, &register, ClosureKind::Day, future).await.is_err());
    }

    #[tokio::test]
    async fn the_day_cannot_be_closed_while_the_till_session_is_open() {
        use crate::local_server::fiscal::closures;
        use motodesk_fiscal::ClosureKind;

        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        sqlx::query(
            "INSERT INTO pos_sessions (id, register_id, opened_at, opening_float_cents) VALUES ('s1', $1, $2, 5000)",
        )
        .bind(register.id.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&state.db)
        .await
        .unwrap();
        let err = closures::close(&state, &register, ClosureKind::Day, today()).await.unwrap_err();
        assert!(matches!(&err, LocalError::BadRequest(m) if m.contains("session de caisse")), "{err:?}");

        sqlx::query("UPDATE pos_sessions SET closed_at = $1 WHERE id = 's1'")
            .bind(Utc::now().to_rfc3339())
            .execute(&state.db)
            .await
            .unwrap();
        closures::close(&state, &register, ClosureKind::Day, today()).await.unwrap();
    }

    #[tokio::test]
    async fn the_fec_balances_and_carries_the_vat_that_was_sealed() {
        let state = shop().await;
        let mixed = LineInputBody {
            product_id: None,
            label: Some("Pneu".into()),
            quantity_milli: 1_000,
            unit_price_ht_cents: 0,
            unit_price_ttc_cents: Some(12_999),
            vat_rate_bp: 550,
            discount_bp: 0,
        };
        let id = draft_with(
            &state,
            vec![shelf_line("Chaîne 520", 999, 3_000), shelf_line("Valve", 10, 3_000), mixed],
        )
        .await;
        let sale = seal(&state, id).await;
        let sealed_vat = sale["total_vat_cents"].as_i64().unwrap();

        let register = fiscal::current(&state).await.unwrap();
        let fec = crate::local_server::fiscal::fec::build(&state.db, &register, Utc::now().year(), "123456789")
            .await
            .unwrap();

        let cents = |s: &str| -> i64 { s.replace(',', "").parse().unwrap() };
        let (mut debit, mut credit, mut vat_credit) = (0i64, 0i64, 0i64);
        for row in fec.content.split("
").skip(1).filter(|l| !l.is_empty()) {
            let f: Vec<&str> = row.split('|').collect();
            debit += cents(f[11]);
            credit += cents(f[12]);
            if f[4].starts_with("44571") {
                vat_credit += cents(f[12]);
            }
        }
        assert_eq!(debit, credit, "the voucher balances");
        assert_eq!(vat_credit, sealed_vat, "VAT legs are the sealed VAT, no stray cent");
    }

    #[tokio::test]
    async fn shelf_prices_and_workshop_prices_mix_on_one_document() {
        let state = shop().await;
        let workshop = LineInputBody {
            product_id: None,
            label: Some("Main d'oeuvre".into()),
            quantity_milli: 1_500,
            unit_price_ht_cents: 5_500,
            unit_price_ttc_cents: None,
            vat_rate_bp: 2_000,
            discount_bp: 0,
        };
        let id = draft_with(&state, vec![workshop, shelf_line("Valve", 450, 2_000)]).await;
        let sale = seal(&state, id).await;
        // 1,5 h at 55,00 HT = 82,50 HT -> 99,00 TTC; 2 valves at 4,50 = 9,00 TTC.
        assert_eq!(sale["total_ttc_cents"], 9_900 + 900);
        let note = credit(&state, id, vec![]).await.unwrap();
        assert_eq!(note["total_ttc_cents"], -(9_900 + 900));
    }

    #[tokio::test]
    async fn a_till_ticket_of_shelf_prices_balances_to_the_cent() {
        let state = shop().await;
        let lines = vec![shelf_line("Casque", 12_999, 1_000), shelf_line("Valve", 999, 3_000)];
        let totals = compute(&lines).unwrap();
        assert_eq!(totals.total_ttc_cents, 12_999 + 2_997);
        let id = draft_with(&state, lines).await;
        let sale = seal(&state, id).await;
        assert_eq!(sale["total_ttc_cents"], 12_999 + 2_997);
        // Paying exactly what the labels add up to settles it.
        let paid = try_pay(&state, id, 12_999 + 2_997, "card").await.unwrap();
        assert_eq!(paid["amount_cents"], 12_999 + 2_997);
        assert_eq!(status_of(&state, id).await, "paid");
    }

    #[tokio::test]
    async fn a_document_can_only_be_credited_once_in_full() {
        let state = shop().await;
        let id = new_draft(&state).await;
        let invoice = seal(&state, id).await;

        let note = credit(&state, id, vec![]).await.unwrap();
        assert_eq!(
            note["total_ttc_cents"].as_i64().unwrap(),
            -invoice["total_ttc_cents"].as_i64().unwrap(),
            "a full avoir cancels the invoice exactly"
        );

        let again = credit(&state, id, vec![]).await.unwrap_err();
        assert!(matches!(again, LocalError::BadRequest(m) if m.contains("entièrement crédité")));
    }

    #[tokio::test]
    async fn a_partial_avoir_cannot_exceed_what_is_left() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;
        let first_line = helmet_and_service().remove(0);

        let mut half = first_line.clone();
        half.quantity_milli = first_line.quantity_milli / 2;
        credit(&state, id, vec![half.clone()]).await.unwrap();

        // The other half is still creditable, a third of the original is not.
        let mut too_much = first_line.clone();
        too_much.quantity_milli = first_line.quantity_milli;
        let err = credit(&state, id, vec![too_much]).await.unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
        credit(&state, id, vec![half]).await.unwrap();
    }

    #[tokio::test]
    async fn an_avoir_cannot_credit_a_line_the_invoice_never_had() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;
        let foreign = LineInputBody {
            product_id: None,
            label: Some("Moto neuve".into()),
            quantity_milli: 1_000,
            unit_price_ht_cents: 999_900,
            vat_rate_bp: 2_000,
            discount_bp: 0,
            unit_price_ttc_cents: None,
        };
        let err = credit(&state, id, vec![foreign]).await.unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(m) if m.contains("ne figure pas")));
    }

    #[tokio::test]
    async fn an_avoir_cannot_itself_be_credited() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;
        let note = credit(&state, id, vec![]).await.unwrap();
        let note_id = Uuid::parse_str(note["id"].as_str().unwrap()).unwrap();
        let err = credit(&state, note_id, vec![]).await.unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
    }

    #[tokio::test]
    async fn validating_twice_in_a_row_cannot_burn_a_second_number() {
        let state = shop().await;
        let id = new_draft(&state).await;
        seal(&state, id).await;
        let err = validate(State(state.clone()), Path(id)).await.unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'invoice'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(n, 1, "exactly one invoice entry in the journal");
    }

    #[tokio::test]
    async fn an_invoice_for_work_on_the_customers_own_bike_validates() {
        let state = shop().await;
        let owner = insert_client(&state, "Martin").await;
        let bike = insert_owned_bike(&state, owner).await;
        let id = draft_for_vehicle(&state, owner, bike).await;

        let sale = seal(&state, id).await;
        assert!(sale["reference"].as_str().unwrap().starts_with("FA-"));

        // Sealing an invoice for the bike must not move it anywhere.
        let (still_owner,): (Option<String>,) =
            sqlx::query_as("SELECT client_id FROM vehicles WHERE id = $1")
                .bind(bike.to_string())
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(still_owner, Some(owner.to_string()));
    }

    #[tokio::test]
    async fn a_draft_for_a_bike_sold_to_someone_else_is_refused() {
        let state = shop().await;
        let owner = insert_client(&state, "Martin").await;
        let stranger = insert_client(&state, "Durand").await;
        let bike = insert_owned_bike(&state, owner).await;
        let id = draft_for_vehicle(&state, stranger, bike).await;

        let err = validate(State(state.clone()), Path(id)).await.unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
    }

    #[tokio::test]
    async fn stock_bike_moves_to_buyer_on_seal() {
        let state = shop().await;
        let now = Utc::now().to_rfc3339();
        let client_id = Uuid::new_v4();
        let vehicle_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO clients (id, last_name, created_at, updated_at, row_version, field_meta)
             VALUES ($1,'Martin',$2,$2,1,'{}')",
        )
        .bind(client_id.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO vehicles (
                id, vehicle_type, make, model, is_stock, selling_price_ttc_cents,
                purchase_price_ht_cents, created_at, updated_at
             ) VALUES ($1,'moto','Honda','CB500',1,120000,80000,$2,$2)",
        )
        .bind(vehicle_id.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();

        let (_, Json(sale)) = create(
            State(state.clone()),
            Json(SaleInput {
                client_id: Some(client_id.to_string()),
                vehicle_id: Some(vehicle_id.to_string()),
                sale_date: None,
                notes: None,
                status: None,
                document_kind: None,
                lines: vec![LineInputBody {
                    product_id: None,
                    label: Some("Honda CB500".into()),
                    quantity_milli: 1_000,
                    unit_price_ht_cents: 100_000,
                    vat_rate_bp: 2_000,
                    discount_bp: 0,
                    unit_price_ttc_cents: None,
                }],
            }),
        )
        .await
        .unwrap();
        let sale_id = Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap();
        seal(&state, sale_id).await;

        let row: (i64, Option<String>) = sqlx::query_as(
            "SELECT is_stock, client_id FROM vehicles WHERE id = $1",
        )
        .bind(vehicle_id.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(row.0, 0);
        assert_eq!(row.1.as_deref(), Some(client_id.to_string().as_str()));
    }

    #[tokio::test]
    async fn reprise_line_keeps_ht_plus_vat_equal_ttc() {
        let state = shop().await;
        let (_, Json(sale)) = create(
            State(state.clone()),
            Json(SaleInput {
                client_id: None,
                vehicle_id: None,
                sale_date: None,
                notes: None,
                status: None,
                document_kind: None,
                lines: vec![
                    LineInputBody {
                        product_id: None,
                        label: Some("Yamaha MT-07".into()),
                        quantity_milli: 1_000,
                        unit_price_ht_cents: 100_000,
                        vat_rate_bp: 2_000,
                        discount_bp: 0,
                        unit_price_ttc_cents: None,
                    },
                    LineInputBody {
                        product_id: None,
                        label: Some("Reprise Honda CBF".into()),
                        quantity_milli: 1_000,
                        unit_price_ht_cents: -40_000,
                        vat_rate_bp: 2_000,
                        discount_bp: 0,
                        unit_price_ttc_cents: None,
                    },
                ],
            }),
        )
        .await
        .unwrap();
        let ht = sale["total_ht_cents"].as_i64().unwrap();
        let vat = sale["total_vat_cents"].as_i64().unwrap();
        let ttc = sale["total_ttc_cents"].as_i64().unwrap();
        assert_eq!(ht + vat, ttc);
        assert_eq!(ht, 60_000);
        assert_eq!(vat, 12_000);
        assert_eq!(ttc, 72_000);
    }

    #[tokio::test]
    async fn refuse_selling_a_bike_that_already_has_an_owner() {
        let state = shop().await;
        let now = Utc::now().to_rfc3339();
        let client_id = Uuid::new_v4();
        let vehicle_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO clients (id, last_name, created_at, updated_at, row_version, field_meta)
             VALUES ($1,'Martin',$2,$2,1,'{}')",
        )
        .bind(client_id.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO vehicles (
                id, client_id, vehicle_type, make, model, is_stock, created_at, updated_at
             ) VALUES ($1,$2,'moto','Honda','CB',0,$3,$3)",
        )
        .bind(vehicle_id.to_string())
        .bind(client_id.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();

        let err = assert_stock_vehicle_sellable(&state, &vehicle_id.to_string())
            .await
            .unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
    }

    /// Once a document or a payment is committed, whatever fails afterwards must not be answered
    /// as a failure: the person at the keyboard would record it a second time. The change log is
    /// made to refuse every write here, as a database locked by another PC's changes would.
    #[tokio::test]
    async fn what_is_recorded_is_never_answered_as_failed() {
        let state = shop().await;
        let paid = draft_with(&state, vec![shelf_line("Casque", 12_000, 1_000)]).await;
        validate(State(state.clone()), Path(paid)).await.unwrap();
        let sealed_later = draft_with(&state, vec![shelf_line("Chaîne", 4_000, 1_000)]).await;

        sqlx::query("CREATE TRIGGER broken_change_log BEFORE INSERT ON change_log BEGIN SELECT RAISE(ABORT, 'broken on purpose'); END")
            .execute(&state.db)
            .await
            .unwrap();

        let Json(invoice) = validate(State(state.clone()), Path(sealed_later)).await.expect("the invoice is sealed");
        assert!(invoice["locked_at"].is_string());

        let payment = try_pay(&state, paid, 12_000, "cash").await.expect("the payment is recorded");
        assert_eq!(payment["amount_cents"], 12_000);
        let payment_id = Uuid::parse_str(payment["id"].as_str().unwrap()).unwrap();

        let (_, Json(reversal)) = reverse_payment(State(state.clone()), Path(payment_id)).await.expect("the reversal is recorded");
        assert_eq!(reversal["amount_cents"], -12_000);

        let note = credit(&state, paid, vec![]).await.expect("the credit note is sealed");
        assert!(note["locked_at"].is_string());

        let Json(copy) = reprint(State(state.clone()), Path(sealed_later)).await.expect("the reprint is journalled");
        assert_eq!(copy["copy_number"], 1);

        let (payments,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM payments").fetch_one(&state.db).await.unwrap();
        assert_eq!(payments, 2, "one payment and its reversal, each recorded once");
    }
}
