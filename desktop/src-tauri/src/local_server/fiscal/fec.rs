//! Fichier des Écritures Comptables.
//!
//! Article L47 A I of the LPF requires a taxpayer keeping computerised accounts
//! to hand the inspector a FEC; its shape is fixed by arrêté of 29 July 2013
//! (article A47 A-1 of the LPF): eighteen named columns, one header row, tab or
//! pipe separated, named `{SIREN}FEC{YYYYMMDD}`.
//!
//! Motodesk is not the book of record -- the expert-comptable's Quadratus is --
//! so what this produces is the sales and cash side of the year, in the shape
//! the accountant's software already knows how to import. The entries are
//! derived from the fiscal journal, so they cannot disagree with the invoices.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::local_server::error::LocalResult;
use crate::local_server::fiscal::Register;

/// French chart of accounts entries this export uses.
///
/// Deliberately the small standard set for a retail business. A dealership's
/// real plan comptable has more, but mapping products to detailed revenue
/// accounts is the accountant's decision, not the software's.
const ACCOUNT_CUSTOMER: (&str, &str) = ("411000", "Clients");
const ACCOUNT_SALES: (&str, &str) = ("707000", "Ventes de marchandises");
const ACCOUNT_BANK: (&str, &str) = ("512000", "Banque");
const ACCOUNT_CASH: (&str, &str) = ("531000", "Caisse");

const JOURNAL_SALES: (&str, &str) = ("VE", "Journal des ventes");
const JOURNAL_CASH: (&str, &str) = ("CA", "Journal de caisse");

/// VAT collected, by rate. 20% is the standard rate; the reduced rates get
/// their own sub-account so the CA3 boxes can be filled without unpicking them.
fn vat_account(rate_bp: i64) -> (String, String) {
    match rate_bp {
        0 => ("445719".into(), "TVA collectée - taux zéro".into()),
        210 => ("445713".into(), "TVA collectée - 2,10 %".into()),
        550 => ("445712".into(), "TVA collectée - 5,50 %".into()),
        1000 => ("445711".into(), "TVA collectée - 10,00 %".into()),
        2000 => ("445710".into(), "TVA collectée - 20,00 %".into()),
        other => (
            "445718".into(),
            format!("TVA collectée - {},{:02} %", other / 100, other % 100),
        ),
    }
}

#[derive(Debug, Serialize)]
pub struct FecExport {
    pub file_name: String,
    pub line_count: usize,
    pub content: String,
}

struct Line {
    journal_code: &'static str,
    journal_lib: &'static str,
    ecriture_num: String,
    ecriture_date: String,
    compte_num: String,
    compte_lib: String,
    comp_aux_num: String,
    comp_aux_lib: String,
    piece_ref: String,
    piece_date: String,
    ecriture_lib: String,
    debit: i64,
    credit: i64,
    valid_date: String,
}

/// Build the FEC for one calendar year.
pub async fn build(
    pool: &SqlitePool,
    register: &Register,
    year: i32,
    siren: &str,
) -> LocalResult<FecExport> {
    let start = format!("{year}-01-01");
    let end = format!("{year}-12-31");

    let mut lines = Vec::new();
    collect_sales(pool, register, &start, &end, &mut lines).await?;
    collect_payments(pool, register, &start, &end, &mut lines).await?;

    // FEC is read in chronological order; within a voucher the order of the
    // legs does not matter but keeping it stable makes diffs readable.
    lines.sort_by(|a, b| {
        a.ecriture_date
            .cmp(&b.ecriture_date)
            .then(a.ecriture_num.cmp(&b.ecriture_num))
    });

    let mut content = String::from(
        "JournalCode|JournalLib|EcritureNum|EcritureDate|CompteNum|CompteLib|CompAuxNum|CompAuxLib|PieceRef|PieceDate|EcritureLib|Debit|Credit|EcritureLet|DateLet|ValidDate|Montantdevise|Idevise\r\n",
    );
    for line in &lines {
        content.push_str(&format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|||{}||\r\n",
            line.journal_code,
            line.journal_lib,
            line.ecriture_num,
            line.ecriture_date,
            line.compte_num,
            escape(&line.compte_lib),
            line.comp_aux_num,
            escape(&line.comp_aux_lib),
            line.piece_ref,
            line.piece_date,
            escape(&line.ecriture_lib),
            amount(line.debit),
            amount(line.credit),
            line.valid_date,
        ));
    }

    let clean_siren: String = siren.chars().filter(|c| c.is_ascii_digit()).collect();
    let siren_part = if clean_siren.is_empty() {
        "000000000".to_string()
    } else {
        clean_siren
    };

    Ok(FecExport {
        file_name: format!("{siren_part}FEC{year}1231.txt"),
        line_count: lines.len(),
        content,
    })
}

async fn collect_sales(
    pool: &SqlitePool,
    register: &Register,
    start: &str,
    end: &str,
    out: &mut Vec<Line>,
) -> LocalResult<()> {
    let rows: Vec<(String, String, String, i64, i64, String, String, i64)> = sqlx::query_as(
        r#"
        SELECT COALESCE(s.reference, s.id),
               substr(COALESCE(s.locked_at, s.sale_date, ''), 1, 10),
               COALESCE(s.document_kind, 'invoice'),
               COALESCE(s.total_ht_cents, 0),
               COALESCE(s.total_ttc_cents, 0),
               COALESCE(c.id, ''),
               TRIM(COALESCE(c.last_name, '') || ' ' || COALESCE(c.first_name, '')),
               COALESCE(s.fiscal_seq, 0)
        FROM sales s
        LEFT JOIN clients c ON c.id = s.client_id
        WHERE s.fiscal_register_id = $1
          AND substr(COALESCE(s.locked_at, ''), 1, 10) BETWEEN $2 AND $3
        ORDER BY s.fiscal_seq ASC
        "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    for (reference, date, kind, total_ht, total_ttc, client_id, client_name, fiscal_seq) in rows {
        let date_fec = compact(&date);
        let label = if kind == "credit_note" {
            format!("Avoir {reference}")
        } else {
            format!("Facture {reference}")
        };

        // Customer account is debited by the gross, revenue and VAT credited.
        // A credit note carries negative amounts, which the FEC expresses by
        // swapping the sides rather than by writing a negative figure.
        push(
            out,
            Line {
                journal_code: JOURNAL_SALES.0,
                journal_lib: JOURNAL_SALES.1,
                ecriture_num: reference.clone(),
                ecriture_date: date_fec.clone(),
                compte_num: ACCOUNT_CUSTOMER.0.into(),
                compte_lib: ACCOUNT_CUSTOMER.1.into(),
                comp_aux_num: client_id.clone(),
                comp_aux_lib: client_name.clone(),
                piece_ref: reference.clone(),
                piece_date: date_fec.clone(),
                ecriture_lib: label.clone(),
                debit: total_ttc,
                credit: 0,
                valid_date: date_fec.clone(),
            },
        );

        push(
            out,
            Line {
                journal_code: JOURNAL_SALES.0,
                journal_lib: JOURNAL_SALES.1,
                ecriture_num: reference.clone(),
                ecriture_date: date_fec.clone(),
                compte_num: ACCOUNT_SALES.0.into(),
                compte_lib: ACCOUNT_SALES.1.into(),
                comp_aux_num: String::new(),
                comp_aux_lib: String::new(),
                piece_ref: reference.clone(),
                piece_date: date_fec.clone(),
                ecriture_lib: label.clone(),
                debit: 0,
                credit: total_ht,
                valid_date: date_fec.clone(),
            },
        );

        // One VAT leg per rate, so the sum of each account matches the CA3. The
        // amounts are the ones sealed in the journal: rebuilding them from the
        // lines rounded differently from the document and left a stray cent on
        // the 20 % account.
        let legs: Vec<(i64, i64)> = match sealed_vat_legs(pool, register, fiscal_seq).await? {
            Some(sealed) => sealed,
            None => {
                let buckets: Vec<(i64, i64)> = sqlx::query_as(
                    r#"
                    SELECT l.vat_rate_bp, SUM(COALESCE(l.line_total_ttc_cents, 0))
                    FROM sale_lines l
                    JOIN sales s ON s.id = l.sale_id
                    WHERE COALESCE(s.reference, s.id) = $1 AND l.deleted_at IS NULL
                    GROUP BY l.vat_rate_bp
                    "#,
                )
                .bind(&reference)
                .fetch_all(pool)
                .await?;
                buckets
                    .into_iter()
                    .map(|(rate_bp, bucket_ttc)| {
                        let bucket_ht = bucket_ttc * 10_000 / (10_000 + rate_bp);
                        (rate_bp, bucket_ttc - bucket_ht)
                    })
                    .collect()
            }
        };

        let mut vat_written = 0i64;
        for (rate_bp, vat) in &legs {
            let (rate_bp, vat) = (*rate_bp, *vat);
            if rate_bp == 0 {
                continue;
            }
            if vat == 0 {
                continue;
            }
            vat_written += vat;
            let (num, lib) = vat_account(rate_bp);
            push(
                out,
                Line {
                    journal_code: JOURNAL_SALES.0,
                    journal_lib: JOURNAL_SALES.1,
                    ecriture_num: reference.clone(),
                    ecriture_date: date_fec.clone(),
                    compte_num: num,
                    compte_lib: lib,
                    comp_aux_num: String::new(),
                    comp_aux_lib: String::new(),
                    piece_ref: reference.clone(),
                    piece_date: date_fec.clone(),
                    ecriture_lib: label.clone(),
                    debit: 0,
                    credit: vat,
                    valid_date: date_fec.clone(),
                },
            );
        }

        // Any cent left over from per-line reconstruction lands on the standard
        // rate rather than leaving the voucher unbalanced, which a FEC importer
        // would reject outright.
        let residual = total_ttc - total_ht - vat_written;
        if residual != 0 {
            let (num, lib) = vat_account(2000);
            push(
                out,
                Line {
                    journal_code: JOURNAL_SALES.0,
                    journal_lib: JOURNAL_SALES.1,
                    ecriture_num: reference.clone(),
                    ecriture_date: date_fec.clone(),
                    compte_num: num,
                    compte_lib: lib,
                    comp_aux_num: String::new(),
                    comp_aux_lib: String::new(),
                    piece_ref: reference.clone(),
                    piece_date: date_fec.clone(),
                    ecriture_lib: label,
                    debit: 0,
                    credit: residual,
                    valid_date: date_fec,
                },
            );
        }
    }
    Ok(())
}

/// `(rate, VAT)` per rate as sealed in the journal entry of a document.
async fn sealed_vat_legs(
    pool: &SqlitePool,
    register: &Register,
    fiscal_seq: i64,
) -> LocalResult<Option<Vec<(i64, i64)>>> {
    if fiscal_seq <= 0 {
        return Ok(None);
    }
    let payload: Option<(String,)> =
        sqlx::query_as("SELECT payload FROM fiscal_journal WHERE register_id = $1 AND seq = $2")
            .bind(register.id.to_string())
            .bind(fiscal_seq)
            .fetch_optional(pool)
            .await?;
    let Some((payload,)) = payload else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();
    let Some(buckets) = value.get("vat_breakdown").and_then(|v| v.as_array()) else {
        return Ok(None);
    };
    Ok(Some(
        buckets
            .iter()
            .map(|b| {
                (
                    b.get("rate_bp").and_then(|v| v.as_i64()).unwrap_or(0),
                    b.get("vat_cents").and_then(|v| v.as_i64()).unwrap_or(0),
                )
            })
            .collect(),
    ))
}

async fn collect_payments(
    pool: &SqlitePool,
    register: &Register,
    start: &str,
    end: &str,
    out: &mut Vec<Line>,
) -> LocalResult<()> {
    let rows: Vec<(String, String, i64, String, String, String, String)> = sqlx::query_as(
        r#"
        SELECT p.id, substr(COALESCE(p.paid_at, ''), 1, 10), p.amount_cents,
               COALESCE(p.method, ''), COALESCE(s.reference, ''),
               COALESCE(c.id, ''),
               TRIM(COALESCE(c.last_name, '') || ' ' || COALESCE(c.first_name, ''))
        FROM payments p
        LEFT JOIN sales s ON s.id = p.sale_id
        LEFT JOIN clients c ON c.id = p.client_id
        WHERE p.fiscal_register_id = $1 AND p.deleted_at IS NULL
          AND substr(COALESCE(p.paid_at, ''), 1, 10) BETWEEN $2 AND $3
        ORDER BY p.fiscal_seq ASC
        "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    for (id, date, amount_cents, method, reference, client_id, client_name) in rows {
        let date_fec = compact(&date);
        let voucher = if reference.is_empty() {
            format!("RG-{}", &id[..8.min(id.len())])
        } else {
            format!("RG-{reference}")
        };
        let (account, account_lib) = if method == "cash" || method == "especes" {
            ACCOUNT_CASH
        } else {
            ACCOUNT_BANK
        };
        let label = format!("Règlement {reference}").trim_end().to_string();

        push(
            out,
            Line {
                journal_code: JOURNAL_CASH.0,
                journal_lib: JOURNAL_CASH.1,
                ecriture_num: voucher.clone(),
                ecriture_date: date_fec.clone(),
                compte_num: account.into(),
                compte_lib: account_lib.into(),
                comp_aux_num: String::new(),
                comp_aux_lib: String::new(),
                piece_ref: reference.clone(),
                piece_date: date_fec.clone(),
                ecriture_lib: label.clone(),
                debit: amount_cents,
                credit: 0,
                valid_date: date_fec.clone(),
            },
        );
        push(
            out,
            Line {
                journal_code: JOURNAL_CASH.0,
                journal_lib: JOURNAL_CASH.1,
                ecriture_num: voucher,
                ecriture_date: date_fec.clone(),
                compte_num: ACCOUNT_CUSTOMER.0.into(),
                compte_lib: ACCOUNT_CUSTOMER.1.into(),
                comp_aux_num: client_id,
                comp_aux_lib: client_name,
                piece_ref: reference,
                piece_date: date_fec.clone(),
                ecriture_lib: label,
                debit: 0,
                credit: amount_cents,
                valid_date: date_fec,
            },
        );
    }
    Ok(())
}

/// A FEC never carries a negative amount: it swaps debit and credit instead.
fn push(out: &mut Vec<Line>, mut line: Line) {
    if line.debit < 0 || line.credit < 0 {
        let (debit, credit) = (line.debit, line.credit);
        line.debit = if credit < 0 { -credit } else { 0 };
        line.credit = if debit < 0 { -debit } else { 0 };
    }
    if line.debit == 0 && line.credit == 0 {
        return;
    }
    out.push(line);
}

/// Cents to the `1234,56` form the arrêté expects.
fn amount(cents: i64) -> String {
    if cents == 0 {
        return "0,00".into();
    }
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.abs();
    format!("{sign}{},{:02}", abs / 100, abs % 100)
}

fn compact(iso_date: &str) -> String {
    iso_date.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// The pipe is the field separator, so it cannot appear inside a field.
fn escape(value: &str) -> String {
    value.replace('|', "/").replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_use_a_comma_and_two_decimals() {
        assert_eq!(amount(0), "0,00");
        assert_eq!(amount(5), "0,05");
        assert_eq!(amount(123_456), "1234,56");
        assert_eq!(amount(-250), "-2,50");
    }

    #[test]
    fn dates_lose_their_separators() {
        assert_eq!(compact("2026-08-21"), "20260821");
        assert_eq!(compact(""), "");
    }

    #[test]
    fn separators_cannot_leak_into_a_field() {
        assert_eq!(escape("Durand | Fils"), "Durand / Fils");
        assert_eq!(escape("a\nb"), "a b");
    }

    #[test]
    fn a_negative_leg_swaps_sides_instead_of_going_negative() {
        let mut out = Vec::new();
        push(
            &mut out,
            Line {
                journal_code: "VE",
                journal_lib: "Ventes",
                ecriture_num: "AV-1".into(),
                ecriture_date: "20260821".into(),
                compte_num: "411000".into(),
                compte_lib: "Clients".into(),
                comp_aux_num: String::new(),
                comp_aux_lib: String::new(),
                piece_ref: "AV-1".into(),
                piece_date: "20260821".into(),
                ecriture_lib: "Avoir".into(),
                debit: -1_200,
                credit: 0,
                valid_date: "20260821".into(),
            },
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].debit, 0);
        assert_eq!(out[0].credit, 1_200);
    }

    #[test]
    fn empty_legs_are_dropped() {
        let mut out = Vec::new();
        push(
            &mut out,
            Line {
                journal_code: "VE",
                journal_lib: "Ventes",
                ecriture_num: "FA-1".into(),
                ecriture_date: "20260821".into(),
                compte_num: "445710".into(),
                compte_lib: "TVA".into(),
                comp_aux_num: String::new(),
                comp_aux_lib: String::new(),
                piece_ref: "FA-1".into(),
                piece_date: "20260821".into(),
                ecriture_lib: "Facture".into(),
                debit: 0,
                credit: 0,
                valid_date: "20260821".into(),
            },
        );
        assert!(out.is_empty());
    }

    #[test]
    fn vat_accounts_follow_the_rate() {
        assert_eq!(vat_account(2000).0, "445710");
        assert_eq!(vat_account(550).0, "445712");
        assert_eq!(vat_account(1234).0, "445718");
        assert!(vat_account(1234).1.contains("12,34"));
    }
}
