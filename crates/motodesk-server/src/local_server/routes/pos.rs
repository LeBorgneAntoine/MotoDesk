//! Till façade: atomic checkout, cashier sessions, drawer, ticket print, TPE.
//!
//! The journal stays the source of truth. These routes only compose the sales
//! and fiscal primitives so a customer at the counter never waits on a
//! half-written ticket.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::OpKind;
use motodesk_fiscal::{numbering::DocumentKind, FiscalEventType};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::local_server::credit;
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::fiscal::{self, mentions, closures, EntryLinks};
use crate::local_server::pos::{escpos, terminal};
use crate::local_server::routes::sales::{
    after_sale_sealed, load_sale, seal_in_tx, write_lines, LineInputBody,
};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/pos/checkout", post(checkout))
        .route("/api/pos/session", get(current_session))
        .route("/api/pos/session/open", post(open_session))
        .route("/api/pos/session/close", post(close_session))
        .route("/api/pos/drawer/open", post(open_drawer))
        .route("/api/pos/drawer/kick", post(kick_drawer))
        .route("/api/pos/print/:id", post(print_ticket))
        .route("/api/pos/terminal/charge", post(charge_card))
        .route("/api/pos/held", get(held_tickets))
        .route("/api/pos/frequents", get(frequents))
}

#[derive(Debug, Deserialize)]
pub struct CheckoutPayment {
    pub amount_cents: i64,
    pub method: Option<String>,
    pub bank: Option<String>,
    pub cheque_drawer: Option<String>,
    pub notes: Option<String>,
    pub auth_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CheckoutBody {
    pub client_id: Option<String>,
    /// `receipt` (default) or `invoice`.
    pub kind: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<LineInputBody>,
    pub payments: Vec<CheckoutPayment>,
}

pub(crate) async fn checkout(
    State(state): State<SharedState>,
    Json(body): Json<CheckoutBody>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    if body.lines.is_empty() {
        return Err(LocalError::BadRequest(
            "un ticket sans ligne ne peut pas être encaissé".into(),
        ));
    }
    let kind = match body.kind.as_deref() {
        None | Some("receipt") => DocumentKind::Receipt,
        Some("invoice") => DocumentKind::Invoice,
        Some(other) => {
            return Err(LocalError::BadRequest(format!(
                "la caisse n'émet que des tickets ou des factures, pas « {other} »"
            )))
        }
    };
    if kind == DocumentKind::Invoice && body.client_id.is_none() {
        return Err(LocalError::BadRequest(
            "une facture de caisse doit nommer un client".into(),
        ));
    }

    let totals = crate::local_server::routes::sales::compute(&body.lines)?;
    let mut remaining = totals.total_ttc_cents;
    let mut recorded: Vec<(CheckoutPayment, i64)> = Vec::new();
    for pay in body.payments {
        if pay.amount_cents <= 0 {
            return Err(LocalError::BadRequest(
                "le montant d'un règlement ne peut pas être nul".into(),
            ));
        }
        let method = pay.method.clone().unwrap_or_else(|| "cash".into());
        let applied = if method == "cash" {
            pay.amount_cents.min(remaining)
        } else if pay.amount_cents > remaining {
            return Err(LocalError::BadRequest(
                "un règlement carte ou chèque ne peut pas dépasser le reste à payer".into(),
            ));
        } else {
            pay.amount_cents
        };
        if applied <= 0 {
            continue;
        }
        remaining -= applied;
        recorded.push((pay, applied));
    }
    if remaining > 0 {
        return Err(LocalError::BadRequest(
            "la caisse refuse un ticket non soldé ; encaissez le montant TTC".into(),
        ));
    }
    if recorded.is_empty() {
        return Err(LocalError::BadRequest(
            "un ticket de caisse doit porter au moins un règlement".into(),
        ));
    }
    if let Some(cid) = body.client_id.as_deref() {
        credit::assert_allows(&state.db, cid, 0).await?;
    }

    let register = fiscal::current(&state).await?;
    let profile = mentions::load(&state.db).await?;
    let check = mentions::check(&profile);
    if !check.complete {
        let labels: Vec<&str> = check.missing.iter().map(|m| m.label.as_str()).collect();
        return Err(LocalError::BadRequest(format!(
            "profil d'entreprise incomplet : {}",
            labels.join(", ")
        )));
    }

    let id = Uuid::new_v4();
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let actor = fiscal::current_actor(&state).await;
    let device_id = state.device_id().await?;
    let company_id = state.company_id.read().await.map(|c| c.to_string());

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        r#"
        INSERT INTO sales (
            id, client_id, status, document_kind, sale_date,
            total_ht_cents, total_vat_cents, total_ttc_cents, notes,
            created_at, updated_at, row_version, field_meta, company_id
        ) VALUES ($1,$2,'draft',$3,$4,$5,$6,$7,$8,$9,$9,1,'{}',$10)
        "#,
    )
    .bind(id.to_string())
    .bind(&body.client_id)
    .bind(kind.as_str())
    .bind(&now_s[..10])
    .bind(totals.total_ht_cents)
    .bind(totals.total_vat_cents)
    .bind(totals.total_ttc_cents)
    .bind(&body.notes)
    .bind(&now_s)
    .bind(&company_id)
    .execute(&mut *tx)
    .await?;
    write_lines(&mut tx, id, &body.lines, &totals, &now_s).await?;

    let lot = seal_in_tx(
        &mut tx,
        &register,
        &profile,
        id,
        kind,
        &body.lines,
        None,
        now,
        actor.clone(),
    )
    .await?;

    let mut payment_entries = Vec::new();
    let mut payment_ids = Vec::new();
    let mut outstanding = totals.total_ttc_cents;
    for (pay, applied) in &recorded {
        outstanding -= applied;
        let payment_id = Uuid::new_v4();
        let method = pay.method.clone().unwrap_or_else(|| "cash".into());
        let links = EntryLinks {
            document_ref: Some(lot.reference.clone()),
            sale_id: Some(id),
            total_ttc_cents: *applied,
            actor_id: actor.clone(),
        };
        let entry = fiscal::append(
            &mut tx,
            &register,
            FiscalEventType::Payment,
            now,
            json!({
                "payment_id": payment_id.to_string(),
                "sale_id": id.to_string(),
                "document_ref": lot.reference,
                "amount_cents": applied,
                "method": method,
                "auth_ref": pay.auth_ref,
                "outstanding_after_cents": outstanding,
            }),
            links.clone(),
        )
        .await?;
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
        .bind(&body.client_id)
        .bind(&now_s)
        .bind(applied)
        .bind(outstanding)
        .bind(&method)
        .bind(&pay.bank)
        .bind(&pay.cheque_drawer)
        .bind(&pay.notes)
        .bind(register.id.to_string())
        .bind(entry.seq)
        .bind(&now_s)
        .bind(&company_id)
        .execute(&mut *tx)
        .await?;
        payment_entries.push((entry, links));
        payment_ids.push(payment_id);
    }

    sqlx::query("UPDATE sales SET status = 'paid', updated_at = $2 WHERE id = $1")
        .bind(id.to_string())
        .bind(&now_s)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    fiscal::replicate(&state.db, device_id, &register, &lot.entry, &lot.links).await?;
    for (entry, links) in &payment_entries {
        fiscal::replicate(&state.db, device_id, &register, entry, links).await?;
    }
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
    // Only now: the stock movements and order updates this makes refer to the sale, and
    // the other PCs apply changes in the order they were logged.
    let _ = after_sale_sealed(&state, id).await;
    // The takings travel with the ticket. Only the journal entry used to, so the
    // other PCs saw a paid ticket with no payment behind it.
    for payment_id in payment_ids {
        if let Ok(payment) = crate::local_server::routes::sales::load_payment(&state, payment_id).await {
            let _ = sync_log::append(
                &state.db,
                device_id,
                EntityType::Payment,
                payment_id,
                OpKind::Insert,
                payment,
                None,
            )
            .await?;
        }
    }
    state.notify_sync();

    let cash_tendered: i64 = recorded
        .iter()
        .filter(|(p, _)| p.method.as_deref().unwrap_or("cash") == "cash")
        .map(|(p, _)| p.amount_cents)
        .sum();
    let cash_recorded: i64 = recorded
        .iter()
        .filter(|(p, _)| p.method.as_deref().unwrap_or("cash") == "cash")
        .map(|(_, a)| *a)
        .sum();

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "sale": sale,
            "change_cents": (cash_tendered - cash_recorded).max(0),
            "kick_drawer": recorded.iter().any(|(p, _)| p.method.as_deref().unwrap_or("cash") == "cash"),
        })),
    ))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct SessionRow {
    id: String,
    register_id: String,
    opened_at: String,
    opened_by: Option<String>,
    opening_float_cents: i64,
    closed_at: Option<String>,
    closed_by: Option<String>,
    closing_counted_cents: Option<i64>,
    expected_cash_cents: Option<i64>,
    variance_cents: Option<i64>,
}

async fn current_session(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let register = fiscal::current(&state).await?;
    let row = open_session_row(&state, register.id).await?;
    Ok(Json(match row {
        Some(s) => serde_json::to_value(s).unwrap_or(Value::Null),
        None => Value::Null,
    }))
}

async fn open_session_row(state: &SharedState, register_id: Uuid) -> LocalResult<Option<SessionRow>> {
    let row = sqlx::query_as::<_, SessionRow>(
        "SELECT id, register_id, opened_at, opened_by, opening_float_cents,
                closed_at, closed_by, closing_counted_cents, expected_cash_cents, variance_cents
         FROM pos_sessions WHERE register_id = $1 AND closed_at IS NULL
         ORDER BY opened_at DESC LIMIT 1",
    )
    .bind(register_id.to_string())
    .fetch_optional(&state.db)
    .await?;
    Ok(row)
}

#[derive(Debug, Deserialize)]
pub struct OpenSessionBody {
    pub opening_float_cents: i64,
}

async fn open_session(
    State(state): State<SharedState>,
    Json(body): Json<OpenSessionBody>,
) -> LocalResult<(StatusCode, Json<Value>)> {
    if body.opening_float_cents < 0 {
        return Err(LocalError::BadRequest(
            "le fond de caisse ne peut pas être négatif".into(),
        ));
    }
    let register = fiscal::current(&state).await?;
    if open_session_row(&state, register.id).await?.is_some() {
        return Err(LocalError::BadRequest(
            "une session est déjà ouverte sur cette caisse".into(),
        ));
    }
    let id = Uuid::new_v4();
    let now = Utc::now();
    let actor = fiscal::current_actor(&state).await;
    let device_id = state.device_id().await?;
    let links = EntryLinks {
        document_ref: None,
        sale_id: None,
        total_ttc_cents: 0,
        actor_id: actor.clone(),
    };

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    // After the Z the till stays shut until tomorrow. Opening it would put the
    // cashier in front of a till that looks ready and can record nothing: the
    // checkout is refused, but only once the customer's card has been charged.
    fiscal::ensure_day_open(&mut *tx, register.id, &fiscal::business_day(now)).await?;
    let entry = fiscal::append(
        &mut tx,
        &register,
        FiscalEventType::SessionOpened,
        now,
        json!({
            "session_id": id.to_string(),
            "opening_float_cents": body.opening_float_cents,
        }),
        links.clone(),
    )
    .await?;
    sqlx::query(
        r#"
        INSERT INTO pos_sessions (
            id, register_id, opened_at, opened_by, opening_float_cents, fiscal_open_seq
        ) VALUES ($1,$2,$3,$4,$5,$6)
        "#,
    )
    .bind(id.to_string())
    .bind(register.id.to_string())
    .bind(now.to_rfc3339())
    .bind(&actor)
    .bind(body.opening_float_cents)
    .bind(entry.seq)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    fiscal::replicate(&state.db, device_id, &register, &entry, &links).await?;
    state.notify_sync();

    let row = open_session_row(&state, register.id).await?.ok_or(LocalError::NotFound)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(row).unwrap_or(Value::Null)),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CloseSessionBody {
    pub counted_cents: i64,
    #[serde(default)]
    pub close_day: bool,
}

async fn close_session(
    State(state): State<SharedState>,
    Json(body): Json<CloseSessionBody>,
) -> LocalResult<Json<Value>> {
    if body.counted_cents < 0 {
        return Err(LocalError::BadRequest(
            "le comptage du tiroir ne peut pas être négatif".into(),
        ));
    }
    let register = fiscal::current(&state).await?;
    let session = open_session_row(&state, register.id)
        .await?
        .ok_or_else(|| LocalError::BadRequest("aucune session ouverte".into()))?;

    let expected = expected_cash(&state, &session).await?;
    let variance = body.counted_cents - expected;
    let now = Utc::now();
    let actor = fiscal::current_actor(&state).await;
    let device_id = state.device_id().await?;
    let links = EntryLinks {
        document_ref: None,
        sale_id: None,
        total_ttc_cents: 0,
        actor_id: actor.clone(),
    };

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let entry = fiscal::append(
        &mut tx,
        &register,
        FiscalEventType::SessionClosed,
        now,
        json!({
            "session_id": session.id,
            "opening_float_cents": session.opening_float_cents,
            "expected_cash_cents": expected,
            "counted_cents": body.counted_cents,
            "variance_cents": variance,
        }),
        links.clone(),
    )
    .await?;
    sqlx::query(
        r#"
        UPDATE pos_sessions SET
            closed_at = $2, closed_by = $3,
            closing_counted_cents = $4, expected_cash_cents = $5,
            variance_cents = $6, fiscal_close_seq = $7
        WHERE id = $1 AND closed_at IS NULL
        "#,
    )
    .bind(&session.id)
    .bind(now.to_rfc3339())
    .bind(&actor)
    .bind(body.counted_cents)
    .bind(expected)
    .bind(variance)
    .bind(entry.seq)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    fiscal::replicate(&state.db, device_id, &register, &entry, &links).await?;

    // The session is closed and journalled by now. If the day cannot be closed as
    // well, say so without turning the whole answer into an error: the cashier has
    // closed the till, and reporting a failure made them close it again.
    let mut day_closure: Option<Value> = None;
    let mut day_closure_error: Option<String> = None;
    if body.close_day {
        let on = chrono::Local::now().date_naive();
        match closures::close(&state, &register, motodesk_fiscal::ClosureKind::Day, on).await {
            Ok(closed) => day_closure = Some(serde_json::to_value(closed).unwrap_or(Value::Null)),
            Err(LocalError::BadRequest(msg)) => day_closure_error = Some(msg),
            Err(other) => return Err(other),
        }
    }

    state.notify_sync();
    Ok(Json(json!({
        "session_id": session.id,
        "expected_cash_cents": expected,
        "counted_cents": body.counted_cents,
        "variance_cents": variance,
        "day_closure": day_closure,
        "day_closure_error": day_closure_error,
    })))
}

async fn expected_cash(state: &SharedState, session: &SessionRow) -> LocalResult<i64> {
    let cash: (Option<i64>,) = sqlx::query_as(
        r#"
        SELECT SUM(amount_cents) FROM payments
         WHERE deleted_at IS NULL
           AND method = 'cash'
           AND paid_at >= $1
           AND fiscal_register_id = $2
        "#,
    )
    .bind(&session.opened_at)
    .bind(&session.register_id)
    .fetch_one(&state.db)
    .await?;
    Ok(session.opening_float_cents + cash.0.unwrap_or(0))
}

async fn open_drawer(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let register = fiscal::current(&state).await?;
    let now = Utc::now();
    let actor = fiscal::current_actor(&state).await;
    let device_id = state.device_id().await?;
    let links = EntryLinks {
        document_ref: None,
        sale_id: None,
        total_ttc_cents: 0,
        actor_id: actor.clone(),
    };
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let entry = fiscal::append(
        &mut tx,
        &register,
        FiscalEventType::DrawerOpened,
        now,
        json!({ "reason": "no_sale" }),
        links.clone(),
    )
    .await?;
    tx.commit().await?;
    fiscal::replicate(&state.db, device_id, &register, &entry, &links).await?;
    Ok(Json(json!({
        "journal_seq": entry.seq,
        "kick_drawer": true,
        "pulse": escpos::drawer_pulse(),
    })))
}

/// Hardware pulse only. The sale (or no-sale) is already in the journal.
async fn kick_drawer(Json(body): Json<PrintBody>) -> LocalResult<Json<Value>> {
    let Some(target) = body
        .target
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Err(LocalError::BadRequest(
            "aucune imprimante ticket n'est configurée sur cette caisse".into(),
        ));
    };
    escpos::write_bytes(target, &escpos::drawer_pulse())?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, Default)]
pub struct PrintBody {
    pub target: Option<String>,
    pub kick_drawer: Option<bool>,
}

async fn print_ticket(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PrintBody>,
) -> LocalResult<Json<Value>> {
    let Json(reprint) = crate::local_server::routes::sales::reprint(
        State(state.clone()),
        Path(id),
    )
    .await?;
    let sale = load_sale(&state, id).await?;
    let profile = mentions::load(&state.db).await?;
    let text = ticket_text(&profile, &sale, reprint["duplicata"].as_bool().unwrap_or(false));
    let kick = body.kick_drawer.unwrap_or(false);
    let bytes = escpos::render_ticket(&text, kick);
    if let Some(target) = body.target.as_deref().filter(|s| !s.is_empty()) {
        escpos::write_bytes(target, &bytes)?;
    }
    Ok(Json(json!({
        "reprint": reprint,
        "bytes_len": bytes.len(),
        "text": text,
    })))
}

/// Width of an 80 mm ticket in the standard font.
const TICKET_WIDTH: usize = 42;

fn euros(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{},{:02} €", abs / 100, abs % 100)
}

fn ticket_center(s: &str) -> String {
    let len = s.chars().count();
    let pad = TICKET_WIDTH.saturating_sub(len) / 2;
    format!("{}{s}", " ".repeat(pad))
}

/// `left` on the left, `right` flush right; `left` is cut to leave room.
fn ticket_row(left: &str, right: &str) -> String {
    let right_len = right.chars().count();
    let room = TICKET_WIDTH.saturating_sub(right_len + 1);
    let left: String = left.chars().take(room).collect();
    let gap = TICKET_WIDTH.saturating_sub(left.chars().count() + right_len).max(1);
    format!("{left}{}{right}", " ".repeat(gap))
}

fn payment_method_label(method: &str) -> &str {
    match method {
        "cash" => "Espèces",
        "card" => "Carte bancaire",
        "cheque" => "Chèque",
        "transfer" => "Virement",
        other => other,
    }
}

fn percent_label(rate_bp: i64) -> String {
    if rate_bp % 100 == 0 {
        format!("{}", rate_bp / 100)
    } else {
        format!("{},{:02}", rate_bp / 100, (rate_bp % 100).abs()).trim_end_matches('0').to_string()
    }
}

fn quantity_label(quantity_milli: i64) -> String {
    if quantity_milli % 1000 == 0 {
        format!("{}", quantity_milli / 1000)
    } else {
        format!("{:.3}", quantity_milli as f64 / 1000.0)
            .trim_end_matches('0')
            .replace('.', ",")
    }
}

fn ticket_text(profile: &mentions::CompanyProfile, sale: &Value, duplicata: bool) -> String {
    let rule = "-".repeat(TICKET_WIDTH);
    let mut out: Vec<String> = Vec::new();

    out.push(ticket_center(&profile.name));
    if !profile.address.is_empty() {
        out.push(ticket_center(&profile.address));
    }
    let city = format!("{} {}", profile.postal_code, profile.city);
    if !city.trim().is_empty() {
        out.push(ticket_center(city.trim()));
    }
    out.push(ticket_center(&format!("SIRET {}", profile.siret)));
    if !profile.vat_number.is_empty() {
        out.push(ticket_center(&format!("TVA {}", profile.vat_number)));
    }
    out.push(rule.clone());

    let stamp = sale
        .get("locked_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_default();
    let reference = sale.get("reference").and_then(|v| v.as_str()).unwrap_or("");
    out.push(ticket_row(reference, &stamp));
    if duplicata {
        out.push(ticket_center("*** DUPLICATA ***"));
    }
    out.push(rule.clone());

    if let Some(lines) = sale.get("lines").and_then(|v| v.as_array()) {
        for line in lines {
            let label = line.get("label").and_then(|v| v.as_str()).unwrap_or("");
            let qty = line.get("quantity_milli").and_then(|v| v.as_i64()).unwrap_or(0);
            let ttc = line
                .get("line_total_ttc_cents")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let vat = line.get("vat_rate_bp").and_then(|v| v.as_i64()).unwrap_or(0);
            let discount = line.get("discount_bp").and_then(|v| v.as_i64()).unwrap_or(0);
            out.push(ticket_row(
                &format!("{} x {}", quantity_label(qty), label),
                &euros(ttc),
            ));
            let mut detail = format!("  TVA {} %", percent_label(vat));
            if discount > 0 {
                detail.push_str(&format!("  remise {} %", percent_label(discount)));
            }
            out.push(detail);
        }
    }

    out.push(rule.clone());
    let total_ht = sale.get("total_ht_cents").and_then(|v| v.as_i64());
    let total_vat = sale.get("total_vat_cents").and_then(|v| v.as_i64());
    let total_ttc = sale.get("total_ttc_cents").and_then(|v| v.as_i64());
    if let Some(ht) = total_ht {
        out.push(ticket_row("Total HT", &euros(ht)));
    }
    if let Some(vat) = total_vat {
        out.push(ticket_row("TVA", &euros(vat)));
    }
    if let Some(ttc) = total_ttc {
        out.push(ticket_row("Total TTC", &euros(ttc)));
    }
    out.push(rule.clone());

    if let Some(pays) = sale.get("payments").and_then(|v| v.as_array()) {
        for p in pays {
            let method = p.get("method").and_then(|v| v.as_str()).unwrap_or("");
            let amount = p.get("amount_cents").and_then(|v| v.as_i64()).unwrap_or(0);
            out.push(ticket_row(payment_method_label(method), &euros(amount)));
        }
        out.push(rule.clone());
    }

    out.push(ticket_center("Ticket remis sur demande (AGEC)"));
    out.push(ticket_center(fiscal::SOFTWARE_VERSION));
    let mut text = out.join("\n");
    text.push('\n');
    text
}

#[derive(Debug, Deserialize)]
pub struct ChargeBody {
    pub amount_cents: i64,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub force: Option<String>,
}

async fn charge_card(
    State(state): State<SharedState>,
    Json(body): Json<ChargeBody>,
) -> LocalResult<Json<Value>> {
    // Nothing goes to the terminal on a closed day: the checkout that follows would
    // be refused, and the customer would have paid for a sale that is not recorded.
    let register = fiscal::current(&state).await?;
    fiscal::ensure_day_open(&state.db, register.id, &fiscal::business_day(Utc::now())).await?;

    let mode = terminal::TerminalMode::parse(body.mode.as_deref().unwrap_or("simulator"));
    let outcome = terminal::charge(mode, body.amount_cents, body.force.as_deref());
    if matches!(outcome, terminal::ChargeOutcome::Accepted { .. }) {
        Ok(Json(serde_json::to_value(outcome).unwrap_or(Value::Null)))
    } else {
        Err(LocalError::BadRequest(match outcome {
            terminal::ChargeOutcome::Refused { reason } => reason,
            terminal::ChargeOutcome::Timeout => "le terminal n'a pas répondu".into(),
            terminal::ChargeOutcome::Accepted { .. } => unreachable!(),
        }))
    }
}

async fn held_tickets(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let rows: Vec<(String, Option<String>, i64, String)> = sqlx::query_as(
        r#"
        SELECT id, notes, COALESCE(total_ttc_cents, 0), updated_at
          FROM sales
         WHERE deleted_at IS NULL AND locked_at IS NULL
           AND COALESCE(document_kind, '') = 'receipt'
           AND status = 'draft'
         ORDER BY updated_at DESC
         LIMIT 50
        "#,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, notes, ttc, updated_at)| json!({
            "id": id,
            "notes": notes,
            "total_ttc_cents": ttc,
            "updated_at": updated_at,
        }))
        .collect::<Vec<_>>())))
}

/// Best-sellers for the till strip: sealed lines, most sold then most recent.
async fn frequents(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        i64,
        Option<i64>,
        i64,
        i64,
        Option<i64>,
        i64,
        i64,
        String,
    )> = sqlx::query_as(
        r#"
        SELECT p.id, p.name, p.sku, p.brand, p.unit, p.is_service,
               p.selling_price_ttc_cents, p.vat_rate_bp, p.stock_milli,
               p.low_stock_threshold, p.is_active,
               COUNT(*) AS hits,
               MAX(COALESCE(s.locked_at, s.sale_date, s.updated_at)) AS last_sold_at
          FROM sale_lines sl
          JOIN sales s ON s.id = sl.sale_id
          JOIN products p ON p.id = sl.product_id
         WHERE sl.deleted_at IS NULL
           AND s.deleted_at IS NULL
           AND sl.product_id IS NOT NULL
           AND p.deleted_at IS NULL
           AND p.is_active = 1
           AND COALESCE(s.document_kind, s.status, 'invoice')
               NOT IN ('quote', 'credit_note')
         GROUP BY p.id
         ORDER BY hits DESC, last_sold_at DESC
         LIMIT 16
        "#,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(
            |(
                id,
                name,
                sku,
                brand,
                unit,
                is_service,
                selling_price_ttc_cents,
                vat_rate_bp,
                stock_milli,
                low_stock_threshold,
                is_active,
                hits,
                last_sold_at,
            )| {
                json!({
                    "id": id,
                    "name": name,
                    "sku": sku,
                    "brand": brand,
                    "unit": unit,
                    "is_service": is_service != 0,
                    "selling_price_ttc_cents": selling_price_ttc_cents,
                    "vat_rate_bp": vat_rate_bp,
                    "stock_quantity": stock_milli as f64 / 1000.0,
                    "stock_milli": stock_milli,
                    "low_stock_threshold": low_stock_threshold,
                    "is_active": is_active != 0,
                    "hits": hits,
                    "last_sold_at": last_sold_at,
                })
            }
        )
        .collect::<Vec<_>>())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::routes::sales::tests::helmet_and_service;
    use crate::local_server::test_support::{insert_product, shop};
    use axum::extract::{Path, State};

    fn cash(amount: i64) -> CheckoutPayment {
        CheckoutPayment {
            amount_cents: amount,
            method: Some("cash".into()),
            bank: None,
            cheque_drawer: None,
            notes: None,
            auth_ref: None,
        }
    }

    fn card(amount: i64, auth: &str) -> CheckoutPayment {
        CheckoutPayment {
            amount_cents: amount,
            method: Some("card".into()),
            bank: None,
            cheque_drawer: None,
            notes: None,
            auth_ref: Some(auth.into()),
        }
    }

    #[tokio::test]
    async fn checkout_numbers_a_receipt_and_takes_payment() {
        let state = shop().await;
        let (_, Json(out)) = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: Some("receipt".into()),
                notes: None,
                lines: helmet_and_service(),
                payments: vec![cash(25_200)],
            }),
        )
        .await
        .unwrap();
        let sale = &out["sale"];
        let reference = sale["reference"].as_str().unwrap();
        assert!(reference.starts_with("RC-"), "got {reference}");
        assert_eq!(sale["status"], "paid");
        assert_eq!(sale["document_kind"], "receipt");
        assert_eq!(sale["paid_cents"], 25_200);
        assert_eq!(out["change_cents"], 0);
        assert_eq!(out["kick_drawer"], true);

        let (events,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM fiscal_journal WHERE event_type IN ('receipt','payment')",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(events, 2);
    }

    #[tokio::test]
    async fn unpaid_checkout_is_refused_and_leaves_no_orphan() {
        let state = shop().await;
        let err = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: Some("receipt".into()),
                notes: None,
                lines: helmet_and_service(),
                payments: vec![cash(1_000)],
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sales")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(n, 0);
        let (j,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fiscal_journal")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(j, 0);
    }

    #[tokio::test]
    async fn split_cash_and_card_cover_ttc() {
        let state = shop().await;
        let (_, Json(out)) = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines: helmet_and_service(),
                payments: vec![cash(10_000), card(15_200, "SIM-15200")],
            }),
        )
        .await
        .unwrap();
        assert_eq!(out["sale"]["paid_cents"], 25_200);
        assert_eq!(out["sale"]["payments"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn cash_overpay_records_ttc_and_returns_change() {
        let state = shop().await;
        let (_, Json(out)) = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines: helmet_and_service(),
                payments: vec![cash(50_000)],
            }),
        )
        .await
        .unwrap();
        assert_eq!(out["sale"]["paid_cents"], 25_200);
        assert_eq!(out["change_cents"], 24_800);
    }

    #[tokio::test]
    async fn a_checkout_reaches_the_other_devices_with_its_takings_and_stock() {
        let state = shop().await;
        let pid = insert_product(&state, "Huile", Some("HUI-9"), 10, false, 1_200, 2_000).await;
        let line = LineInputBody {
            product_id: Some(pid.to_string()),
            label: Some("Huile".into()),
            quantity_milli: 2_000,
            unit_price_ht_cents: 1_000,
            vat_rate_bp: 2_000,
            discount_bp: 0,
            unit_price_ttc_cents: None,
        };
        let totals = crate::local_server::routes::sales::compute(std::slice::from_ref(&line)).unwrap();
        let _ = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines: vec![line],
                payments: vec![cash(totals.total_ttc_cents)],
            }),
        )
        .await
        .unwrap();

        for (entity, expected) in [("sale", 1), ("payment", 1), ("stock_movement", 1)] {
            let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM change_log WHERE entity_type = $1")
                .bind(entity)
                .fetch_one(&state.db)
                .await
                .unwrap();
            assert!(n >= expected, "{entity}: {n} change(s) queued");
        }
    }

    /// Selling 2,5 and then 0,5 of a product takes exactly 3 off the shelf, and the shelf
    /// stays equal to the sum of the ledger. The shelf used to move by whole units only
    /// (2, then nothing) while the ledger recorded the fractions.
    #[tokio::test]
    async fn a_fraction_sold_at_the_till_moves_the_shelf_by_that_fraction() {
        let state = shop().await;
        let pid = insert_product(&state, "Huile vrac", Some("VRAC-1"), 10, false, 1_200, 2_000).await;
        let mut sold = 0;
        for qty in [2_500_i64, 500] {
            let line = LineInputBody {
                product_id: Some(pid.to_string()),
                label: Some("Huile vrac".into()),
                quantity_milli: qty,
                unit_price_ht_cents: 1_000,
                vat_rate_bp: 2_000,
                discount_bp: 0,
                unit_price_ttc_cents: None,
            };
            let totals = crate::local_server::routes::sales::compute(std::slice::from_ref(&line)).unwrap();
            let _ = checkout(
                State(state.clone()),
                Json(CheckoutBody {
                    client_id: None,
                    kind: None,
                    notes: None,
                    lines: vec![line],
                    payments: vec![cash(totals.total_ttc_cents)],
                }),
            )
            .await
            .unwrap();
            sold += qty;
        }
        let (shelf,): (i64,) = sqlx::query_as("SELECT stock_milli FROM products WHERE id = $1")
            .bind(pid.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(shelf, 10_000 - sold);
        let (ledger,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(quantity_milli), 0) FROM stock_movements WHERE product_id = $1",
        )
        .bind(pid.to_string())
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(10_000 + ledger, shelf, "the shelf is the opening stock plus the movements");
    }

    #[tokio::test]
    async fn stock_moves_on_checkout_and_credit_note() {
        let state = shop().await;
        let pid = insert_product(&state, "Huile", Some("HUI-1"), 10, false, 1_200, 2_000).await;
        let line = LineInputBody {
            product_id: Some(pid.to_string()),
            label: Some("Huile".into()),
            quantity_milli: 2_000,
            unit_price_ht_cents: 1_000,
            vat_rate_bp: 2_000,
            discount_bp: 0,
            unit_price_ttc_cents: None,
        };
        let totals = crate::local_server::routes::sales::compute(std::slice::from_ref(&line)).unwrap();
        let (_, Json(out)) = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines: vec![line],
                payments: vec![cash(totals.total_ttc_cents)],
            }),
        )
        .await
        .unwrap();
        let (stock,): (i64,) = sqlx::query_as("SELECT stock_milli FROM products WHERE id = $1")
            .bind(pid.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(stock, 8_000);

        let sale_id = Uuid::parse_str(out["sale"]["id"].as_str().unwrap()).unwrap();
        let _ = crate::local_server::routes::sales::credit_note(
            State(state.clone()),
            axum::extract::Path(sale_id),
            Json(crate::local_server::routes::sales::CreditNoteBody {
                lines: vec![],
                reason: Some("erreur".into()),
            }),
        )
        .await
        .unwrap();
        let (stock,): (i64,) = sqlx::query_as("SELECT stock_milli FROM products WHERE id = $1")
            .bind(pid.to_string())
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(stock, 10_000);
    }

    /// Labour has no shelf: selling it must leave the stock history alone, while a product
    /// sold on the same ticket is recorded.
    #[tokio::test]
    async fn a_service_on_a_ticket_leaves_no_stock_movement() {
        let state = shop().await;
        let oil = insert_product(&state, "Huile", Some("HUI-2"), 10, false, 1_200, 2_000).await;
        let labour = insert_product(&state, "Main d'oeuvre", None, 0, true, 6_000, 2_000).await;
        let line = |pid: Uuid, qty: i64| LineInputBody {
            product_id: Some(pid.to_string()),
            label: None,
            quantity_milli: qty,
            unit_price_ht_cents: 1_000,
            vat_rate_bp: 2_000,
            discount_bp: 0,
            unit_price_ttc_cents: None,
        };
        let lines = vec![line(oil, 2_000), line(labour, 1_500)];
        let totals = crate::local_server::routes::sales::compute(&lines).unwrap();
        let _ = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines,
                payments: vec![cash(totals.total_ttc_cents)],
            }),
        )
        .await
        .unwrap();

        let moved = |pid: Uuid| {
            let db = state.db.clone();
            async move {
                let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM stock_movements WHERE product_id = $1")
                    .bind(pid.to_string())
                    .fetch_one(&db)
                    .await
                    .unwrap();
                n
            }
        };
        assert_eq!(moved(oil).await, 1, "the product sold is in the stock history");
        assert_eq!(moved(labour).await, 0, "labour is not");
    }

    #[tokio::test]
    async fn a_mistaken_ticket_is_credited_not_deleted() {
        let state = shop().await;
        let (_, Json(out)) = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines: helmet_and_service(),
                payments: vec![cash(25_200)],
            }),
        )
        .await
        .unwrap();
        let id = Uuid::parse_str(out["sale"]["id"].as_str().unwrap()).unwrap();
        let _ = crate::local_server::routes::sales::credit_note(
            State(state.clone()),
            axum::extract::Path(id),
            Json(crate::local_server::routes::sales::CreditNoteBody {
                lines: vec![],
                reason: Some("annulation comptoir".into()),
            }),
        )
        .await
        .unwrap();
        let Json(original) = crate::local_server::routes::sales::show(
            State(state.clone()),
            axum::extract::Path(id),
        )
        .await
        .unwrap();
        assert!(original["locked_at"].is_string());
        let (notes,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sales WHERE document_kind = 'credit_note'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(notes, 1);
    }

    #[tokio::test]
    async fn session_close_records_variance_without_rewriting_the_journal() {
        let state = shop().await;
        let (_, Json(open)) = open_session(
            State(state.clone()),
            Json(OpenSessionBody {
                opening_float_cents: 10_000,
            }),
        )
        .await
        .unwrap();
        assert_eq!(open["opening_float_cents"], 10_000);

        let _ = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines: helmet_and_service(),
                payments: vec![cash(25_200)],
            }),
        )
        .await
        .unwrap();

        let Json(closed) = close_session(
            State(state.clone()),
            Json(CloseSessionBody {
                counted_cents: 34_000,
                close_day: false,
            }),
        )
        .await
        .unwrap();
        assert_eq!(closed["expected_cash_cents"], 35_200);
        assert_eq!(closed["variance_cents"], -1_200);

        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM fiscal_journal WHERE event_type IN ('session_opened','session_closed')",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(n, 2);

        // Closing again is refused; the first close stays the answer.
        assert!(matches!(
            close_session(
                State(state.clone()),
                Json(CloseSessionBody {
                    counted_cents: 35_200,
                    close_day: false,
                }),
            )
            .await,
            Err(LocalError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn no_sale_drawer_is_journalled() {
        let state = shop().await;
        let Json(out) = open_drawer(State(state.clone())).await.unwrap();
        assert_eq!(out["kick_drawer"], true);
        let (n,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'drawer_opened'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn refused_card_does_not_seal() {
        let state = shop().await;
        let refused = charge_card(
            State(state.clone()),
            Json(ChargeBody {
                amount_cents: 2_520,
                mode: Some("simulator".into()),
                force: Some("refuse".into()),
            }),
        )
        .await;
        assert!(matches!(refused, Err(LocalError::BadRequest(_))));
    }

    fn charge(amount: i64) -> Json<ChargeBody> {
        Json(ChargeBody {
            amount_cents: amount,
            mode: Some("simulator".into()),
            force: Some("accept".into()),
        })
    }

    async fn sessions_opened(state: &SharedState) -> i64 {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM fiscal_journal WHERE event_type = 'session_opened'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        n
    }

    /// Once the day is closed with the till (the Z), the till cannot be opened again
    /// that day and nothing is sent to the card terminal. Both used to go through, and
    /// only the checkout was refused: by then the customer's card had been charged for
    /// a sale that could not be recorded.
    #[tokio::test]
    async fn a_closed_day_refuses_a_new_session_and_a_card_charge() {
        let state = shop().await;
        let _ = open_session(
            State(state.clone()),
            Json(OpenSessionBody {
                opening_float_cents: 10_000,
            }),
        )
        .await
        .expect("the till opens on a day that is not closed");
        let Json(accepted) = charge_card(State(state.clone()), charge(25_200))
            .await
            .expect("and the terminal takes a card");
        assert_eq!(accepted["status"], "accepted");
        let _ = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines: helmet_and_service(),
                payments: vec![card(25_200, accepted["auth_ref"].as_str().unwrap())],
            }),
        )
        .await
        .unwrap();

        let Json(closed) = close_session(
            State(state.clone()),
            Json(CloseSessionBody {
                counted_cents: 10_000,
                close_day: true,
            }),
        )
        .await
        .unwrap();
        assert!(closed["day_closure"].is_object(), "{closed}");
        assert!(closed["day_closure_error"].is_null(), "{closed}");

        let refusal = |err: LocalError| match err {
            LocalError::BadRequest(msg) => msg,
            other => panic!("expected a refusal the cashier can read, got {other:?}"),
        };

        let msg = refusal(
            open_session(
                State(state.clone()),
                Json(OpenSessionBody {
                    opening_float_cents: 10_000,
                }),
            )
            .await
            .unwrap_err(),
        );
        assert!(msg.contains("est clôturée"), "{msg}");
        assert!(msg.contains("La caisse reprend demain"), "{msg}");
        let Json(current) = current_session(State(state.clone())).await.unwrap();
        assert!(current.is_null(), "no session is left open: {current}");
        assert_eq!(sessions_opened(&state).await, 1, "the refused opening is not journalled");

        let msg = refusal(charge_card(State(state.clone()), charge(25_200)).await.unwrap_err());
        assert!(msg.contains("est clôturée"), "{msg}");

        // The checkout was already refused; it still is, with the same words.
        let msg = refusal(
            checkout(
                State(state.clone()),
                Json(CheckoutBody {
                    client_id: None,
                    kind: None,
                    notes: None,
                    lines: helmet_and_service(),
                    payments: vec![cash(25_200)],
                }),
            )
            .await
            .unwrap_err(),
        );
        assert!(msg.contains("est clôturée"), "{msg}");
    }

    /// Only today's closure shuts the till: yesterday closed is the normal state of
    /// every morning.
    #[tokio::test]
    async fn yesterdays_closure_does_not_keep_the_till_shut() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        let yesterday = chrono::Local::now().date_naive().pred_opt().unwrap();
        closures::close(&state, &register, motodesk_fiscal::ClosureKind::Day, yesterday)
            .await
            .unwrap();

        let (_, Json(open)) = open_session(
            State(state.clone()),
            Json(OpenSessionBody {
                opening_float_cents: 5_000,
            }),
        )
        .await
        .unwrap();
        assert_eq!(open["opening_float_cents"], 5_000);
        assert_eq!(sessions_opened(&state).await, 1);
        let Json(accepted) = charge_card(State(state.clone()), charge(2_520)).await.unwrap();
        assert_eq!(accepted["status"], "accepted");
    }

    #[tokio::test]
    async fn sku_lookup_finds_the_exact_code() {
        let state = shop().await;
        let id = insert_product(&state, "Gants", Some("GNT-42"), 3, false, 2_400, 2_000).await;
        let Json(found) = crate::local_server::routes::catalog::products_by_sku(
            State(state.clone()),
            Path("GNT-42".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(found["id"], id.to_string());
        assert!(matches!(
            crate::local_server::routes::catalog::products_by_sku(
                State(state.clone()),
                Path("nope".to_string()),
            )
            .await,
            Err(LocalError::NotFound)
        ));
    }

    #[tokio::test]
    async fn frequents_rank_sealed_products_by_how_often_they_sold() {
        let state = shop().await;
        let oil = insert_product(&state, "Huile", Some("HUI-1"), 20, false, 1_200, 2_000).await;
        let gloves = insert_product(&state, "Gants", Some("GNT-1"), 20, false, 2_400, 2_000).await;
        let oil_line = LineInputBody {
            product_id: Some(oil.to_string()),
            label: Some("Huile".into()),
            quantity_milli: 1_000,
            unit_price_ht_cents: 1_000,
            vat_rate_bp: 2_000,
            discount_bp: 0,
            unit_price_ttc_cents: None,
        };
        let glove_line = LineInputBody {
            product_id: Some(gloves.to_string()),
            label: Some("Gants".into()),
            quantity_milli: 1_000,
            unit_price_ht_cents: 2_000,
            vat_rate_bp: 2_000,
            discount_bp: 0,
            unit_price_ttc_cents: None,
        };
        let oil_ttc = crate::local_server::routes::sales::compute(std::slice::from_ref(&oil_line))
            .unwrap()
            .total_ttc_cents;
        let glove_ttc =
            crate::local_server::routes::sales::compute(std::slice::from_ref(&glove_line))
                .unwrap()
                .total_ttc_cents;
        for _ in 0..2 {
            let _ = checkout(
                State(state.clone()),
                Json(CheckoutBody {
                    client_id: None,
                    kind: None,
                    notes: None,
                    lines: vec![oil_line.clone()],
                    payments: vec![cash(oil_ttc)],
                }),
            )
            .await
            .unwrap();
        }
        let _ = checkout(
            State(state.clone()),
            Json(CheckoutBody {
                client_id: None,
                kind: None,
                notes: None,
                lines: vec![glove_line.clone()],
                payments: vec![cash(glove_ttc)],
            }),
        )
        .await
        .unwrap();

        let Json(out) = frequents(State(state.clone())).await.unwrap();
        let list = out.as_array().unwrap();
        assert_eq!(list[0]["id"], oil.to_string());
        assert_eq!(list[0]["hits"], 2);
        assert_eq!(list[1]["id"], gloves.to_string());
        assert_eq!(list[1]["hits"], 1);
    }

    #[tokio::test]
    async fn frequents_count_unsealed_invoices_from_the_sales_table() {
        let state = shop().await;
        let oil = insert_product(&state, "Huile", Some("HUI-1"), 20, false, 1_200, 2_000).await;
        let now = Utc::now().to_rfc3339();
        let sale_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO sales (
                id, status, document_kind, sale_date,
                total_ht_cents, total_ttc_cents,
                created_at, updated_at, row_version, field_meta
            ) VALUES ($1,'invoice','invoice',$2,1000,1200,$3,$3,1,'{}')
            "#,
        )
        .bind(sale_id.to_string())
        .bind(&now[..10])
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO sale_lines (
                id, sale_id, product_id, label, quantity_milli,
                unit_price_ht_cents, vat_rate_bp, discount_bp,
                position, created_at, updated_at, row_version, field_meta
            ) VALUES ($1,$2,$3,'Huile',1000,1000,2000,0,0,$4,$4,1,'{}')
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(sale_id.to_string())
        .bind(oil.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .unwrap();

        let Json(out) = frequents(State(state.clone())).await.unwrap();
        let list = out.as_array().unwrap();
        assert_eq!(list[0]["id"], oil.to_string());
        assert_eq!(list[0]["hits"], 1);
    }

    #[test]
    fn ticket_amounts_are_in_euros_with_the_vat_and_a_date() {
        let profile = mentions::CompanyProfile {
            name: "Garage Dupont".into(),
            address: "12 rue des Motos".into(),
            postal_code: "35000".into(),
            city: "Rennes".into(),
            siret: "12345678900012".into(),
            vat_number: "FR12123456789".into(),
            ..Default::default()
        };
        let sale = json!({
            "reference": "RC-2026-C1-00007",
            "locked_at": "2026-11-11T10:30:00+00:00",
            "total_ht_cents": 21000, "total_vat_cents": 4200, "total_ttc_cents": 25200,
            "lines": [{"label": "Casque", "quantity_milli": 1500, "line_total_ttc_cents": 14400,
                        "vat_rate_bp": 2000, "discount_bp": 0}],
            "payments": [{"method": "cash", "amount_cents": 25200}]
        });
        let text = ticket_text(&profile, &sale, false);
        assert!(text.contains("252,00 €"), "{text}");
        assert!(text.contains("144,00 €"), "{text}");
        assert!(text.contains("Espèces"), "{text}");
        assert!(text.contains("TVA FR12123456789"), "{text}");
        assert!(text.contains("TVA 20 %"), "{text}");
        assert!(text.contains("1,5 x Casque"), "{text}");
        assert!(text.contains("11/11/2026"), "{text}");
        assert!(!text.contains(" c\n"), "no raw cents: {text}");
        assert!(text.lines().all(|l| l.chars().count() <= 42), "{text}");
    }

    #[test]
    fn euros_formats_negative_and_small_amounts() {
        assert_eq!(euros(5), "0,05 €");
        assert_eq!(euros(-4002), "-40,02 €");
        assert_eq!(euros(100), "1,00 €");
    }
}
