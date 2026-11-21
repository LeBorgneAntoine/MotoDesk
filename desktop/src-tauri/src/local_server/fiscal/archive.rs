//! Fiscal archives.
//!
//! § 200 to § 240 of BOI-TVA-DECLA-30-10-30: the data must be archived at least
//! once per accounting period, in a format that can be read without the
//! software that produced it, with a technical procedure guaranteeing the
//! archive's integrity, and accompanied by a notice. § 240 adds that the
//! production of the archive is itself traced in the journal.
//!
//! So an archive here is a ZIP of plain CSV -- readable in any spreadsheet a
//! decade from now -- plus a manifest carrying the SHA-256 of every file and the
//! register's public key, plus a French notice explaining how to check it.

use std::io::Write;

use chrono::Utc;
use motodesk_fiscal::FiscalEventType;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::fiscal::{self, EntryLinks, Register};
use crate::local_server::state::SharedState;

#[derive(Debug, Serialize)]
pub struct ArchiveRow {
    pub id: String,
    pub period_start: String,
    pub period_end: String,
    pub file_path: String,
    pub file_sha256: String,
    pub file_bytes: i64,
    pub entry_count: i64,
    pub first_seq: i64,
    pub last_seq: i64,
    pub created_at: String,
}

/// Build the archive for one calendar year and seal a trace of it.
pub async fn create(
    state: &SharedState,
    register: &Register,
    year: i32,
) -> LocalResult<ArchiveRow> {
    let period_start = format!("{year}-01-01");
    let period_end = format!("{year}-12-31");

    // Archiving a year still in progress would produce a file that is already
    // out of date.
    if year >= fiscal::current_year() {
        return Err(LocalError::BadRequest(format!(
            "l'exercice {year} n'est pas terminé ; l'archivage se fait après la clôture annuelle"
        )));
    }

    // § 200: the archive covers a closed accounting period. Producing one over
    // an unclosed year would freeze figures the closure could still change.
    let closed: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM fiscal_closures
         WHERE register_id = $1 AND kind = 'year' AND period_start = $2",
    )
    .bind(register.id.to_string())
    .bind(&period_start)
    .fetch_one(&state.db)
    .await?;
    if closed.0 == 0 {
        return Err(LocalError::BadRequest(format!(
            "l'exercice {year} n'a pas été clôturé ; faites la clôture annuelle avant d'archiver"
        )));
    }

    // § 220 asks for at most one archive per period. An existing one is handed
    // back rather than replaced — unless its file has gone missing, in which
    // case the obligation to hold an archive outweighs the one not to make two.
    if let Some(existing) = find(state, register, &period_start).await? {
        if std::path::Path::new(&existing.file_path).exists() {
            return Ok(existing);
        }
    }

    let dir = state.data_dir.join("archives");
    std::fs::create_dir_all(&dir).map_err(|e| LocalError::Internal(e.into()))?;
    let file_name = format!("motodesk-{}-{}.zip", register.code, year);
    let path = dir.join(&file_name);

    let files = collect(state, register, &period_start, &period_end).await?;
    let entry_count = files.entry_count;
    let (first_seq, last_seq) = (files.first_seq, files.last_seq);

    let bytes = write_zip(register, year, &files)?;
    std::fs::write(&path, &bytes).map_err(|e| LocalError::Internal(e.into()))?;

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let file_sha256 = hex::encode(hasher.finalize());
    let file_bytes = bytes.len() as i64;
    let path_string = path.to_string_lossy().to_string();

    let actor = fiscal::current_actor(state).await;
    let now = Utc::now();
    let links = EntryLinks {
        document_ref: Some(file_name.clone()),
        sale_id: None,
        total_ttc_cents: 0,
        actor_id: actor.clone(),
    };

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let entry = fiscal::append(
        &mut tx,
        register,
        FiscalEventType::ArchiveCreated,
        now,
        json!({
            "file_name": file_name,
            "period_start": period_start,
            "period_end": period_end,
            "file_sha256": file_sha256,
            "file_bytes": file_bytes,
            "entry_count": entry_count,
            "first_seq": first_seq,
            "last_seq": last_seq,
            "notice_included": true,
        }),
        links.clone(),
    )
    .await?;

    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO fiscal_archives (
            id, register_id, period_start, period_end, file_path, file_sha256,
            file_bytes, entry_count, first_seq, last_seq, notice_included,
            purged, journal_seq, created_at, actor_id, company_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,1,0,$11,$12,$13,$14)
        "#,
    )
    .bind(id.to_string())
    .bind(register.id.to_string())
    .bind(&period_start)
    .bind(&period_end)
    .bind(&path_string)
    .bind(&file_sha256)
    .bind(file_bytes)
    .bind(entry_count)
    .bind(first_seq)
    .bind(last_seq)
    .bind(entry.seq)
    .bind(now.to_rfc3339())
    .bind(&actor)
    .bind(&register.company_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let device_id = state.device_id().await.map_err(LocalError::Internal)?;
    fiscal::replicate(&state.db, device_id, register, &entry, &links).await?;
    state.notify_sync();

    Ok(ArchiveRow {
        id: id.to_string(),
        period_start,
        period_end,
        file_path: path_string,
        file_sha256,
        file_bytes,
        entry_count,
        first_seq,
        last_seq,
        created_at: now.to_rfc3339(),
    })
}

/// The archive already held for a period, if there is one.
pub async fn find(
    state: &SharedState,
    register: &Register,
    period_start: &str,
) -> LocalResult<Option<ArchiveRow>> {
    let row: Option<ArchiveDbRow> = sqlx::query_as(
        r#"
        SELECT id, period_start, period_end, file_path, file_sha256, file_bytes,
               entry_count, first_seq, last_seq, created_at
        FROM fiscal_archives
        WHERE register_id = $1 AND period_start = $2
        ORDER BY created_at DESC LIMIT 1
        "#,
    )
    .bind(register.id.to_string())
    .bind(period_start)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(ArchiveDbRow::into_row))
}

pub async fn list(state: &SharedState) -> LocalResult<Vec<ArchiveRow>> {
    let rows: Vec<ArchiveDbRow> = sqlx::query_as(
        r#"
        SELECT id, period_start, period_end, file_path, file_sha256, file_bytes,
               entry_count, first_seq, last_seq, created_at
        FROM fiscal_archives ORDER BY period_start DESC
        "#,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(ArchiveDbRow::into_row).collect())
}

/// Re-hash an archive on disk and compare it with what was recorded.
pub async fn verify(state: &SharedState, id: &str) -> LocalResult<Value> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT file_path, file_sha256 FROM fiscal_archives WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (path, expected) = row.ok_or(LocalError::NotFound)?;

    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(json!({
            "ok": false,
            "problem": "fichier d'archive introuvable",
            "file_path": path,
        }));
    };

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = hex::encode(hasher.finalize());

    Ok(json!({
        "ok": actual == expected,
        "file_path": path,
        "expected_sha256": expected,
        "actual_sha256": actual,
        "problem": if actual == expected { Value::Null } else {
            json!("l'empreinte du fichier ne correspond plus à celle enregistrée")
        },
    }))
}

struct ArchiveContents {
    journal: String,
    closures: String,
    documents: String,
    lines: String,
    payments: String,
    entry_count: i64,
    first_seq: i64,
    last_seq: i64,
    public_key: String,
}

/// seq, event, occurred_at, business_day, ref, ttc, payload, and the four hashes.
type JournalRow = (
    i64,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
    String,
);
/// kind, bounds, counts, the four totals, the breakdown and the closing stamp.
type ClosureRow = (
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    i64,
    i64,
    String,
);
/// id, reference, kind, dates, client, totals, journal sequence.
type DocumentRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
);
/// sale id, reference, label, quantity, price, rate, discount, line total.
type LineRow = (String, String, String, i64, i64, i64, i64, i64);

async fn collect(
    state: &SharedState,
    register: &Register,
    start: &str,
    end: &str,
) -> LocalResult<ArchiveContents> {
    let entries: Vec<JournalRow> = sqlx::query_as(
        r#"
            SELECT seq, event_type, occurred_at, business_day,
                   COALESCE(document_ref, ''), total_ttc_cents,
                   payload, payload_hash, prev_hash, chain_hash, signature
            FROM fiscal_journal
            WHERE register_id = $1 AND business_day >= $2 AND business_day <= $3
            ORDER BY seq ASC
            "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(&state.db)
    .await?;

    let mut journal = String::from(
        "seq;type_evenement;horodatage;journee;reference;total_ttc_cents;empreinte_donnees;empreinte_precedente;empreinte_chainage;signature;donnees\n",
    );
    let (mut first_seq, mut last_seq) = (0i64, 0i64);
    for (seq, event, at, day, doc_ref, total, payload, ph, prev, chain, sig) in &entries {
        if first_seq == 0 {
            first_seq = *seq;
        }
        last_seq = *seq;
        journal.push_str(&csv_row(&[
            &seq.to_string(),
            event,
            at,
            day,
            doc_ref,
            &total.to_string(),
            ph,
            prev,
            chain,
            sig,
            payload,
        ]));
    }

    let closure_rows: Vec<ClosureRow> = sqlx::query_as(
        r#"
            SELECT kind, period_start, period_end, document_count, total_ht_cents,
                   total_vat_cents, total_ttc_cents, vat_breakdown,
                   period_grand_total_cents, perpetual_total_cents, closed_at
            FROM fiscal_closures
            WHERE register_id = $1 AND period_start >= $2 AND period_end <= $3
            ORDER BY period_start ASC, kind ASC
            "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(&state.db)
    .await?;

    let mut closures = String::from(
        "type;debut;fin;nb_documents;total_ht_cents;total_tva_cents;total_ttc_cents;ventilation_tva;grand_total_periode_cents;total_perpetuel_cents;cloture_le\n",
    );
    for (kind, s, e, count, ht, vat, ttc, breakdown, period, perpetual, at) in &closure_rows {
        closures.push_str(&csv_row(&[
            kind,
            s,
            e,
            &count.to_string(),
            &ht.to_string(),
            &vat.to_string(),
            &ttc.to_string(),
            breakdown,
            &period.to_string(),
            &perpetual.to_string(),
            at,
        ]));
    }

    let doc_rows: Vec<DocumentRow> = sqlx::query_as(
        r#"
            SELECT s.id, COALESCE(s.reference, ''), COALESCE(s.document_kind, 'invoice'),
                   COALESCE(s.sale_date, ''), COALESCE(s.locked_at, ''),
                   COALESCE(c.last_name || ' ' || COALESCE(c.first_name, ''), ''),
                   COALESCE(s.total_ht_cents, 0), COALESCE(s.total_vat_cents, 0),
                   COALESCE(s.total_ttc_cents, 0), COALESCE(s.fiscal_seq, 0)
            FROM sales s
            LEFT JOIN clients c ON c.id = s.client_id
            WHERE s.fiscal_register_id = $1 AND substr(COALESCE(s.locked_at, ''), 1, 10) >= $2
              AND substr(COALESCE(s.locked_at, ''), 1, 10) <= $3
            ORDER BY s.fiscal_seq ASC
            "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(&state.db)
    .await?;

    let mut documents = String::from(
        "id;reference;type;date;valide_le;client;total_ht_cents;total_tva_cents;total_ttc_cents;sequence_journal\n",
    );
    for (id, reference, kind, date, locked, client, ht, vat, ttc, seq) in &doc_rows {
        documents.push_str(&csv_row(&[
            id,
            reference,
            kind,
            date,
            locked,
            client.trim(),
            &ht.to_string(),
            &vat.to_string(),
            &ttc.to_string(),
            &seq.to_string(),
        ]));
    }

    let line_rows: Vec<LineRow> = sqlx::query_as(
        r#"
        SELECT l.sale_id, COALESCE(s.reference, ''), COALESCE(l.label, ''),
               l.quantity_milli, COALESCE(l.unit_price_ht_cents, 0),
               l.vat_rate_bp, l.discount_bp, COALESCE(l.line_total_ttc_cents, 0)
        FROM sale_lines l
        JOIN sales s ON s.id = l.sale_id
        WHERE s.fiscal_register_id = $1 AND substr(COALESCE(s.locked_at, ''), 1, 10) >= $2
          AND substr(COALESCE(s.locked_at, ''), 1, 10) <= $3
        ORDER BY s.fiscal_seq ASC, l.position ASC
        "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(&state.db)
    .await?;

    let mut lines = String::from(
        "document_id;reference;libelle;quantite_milli;pu_ht_cents;taux_tva_bp;remise_bp;total_ligne_ttc_cents\n",
    );
    for (sale_id, reference, label, qty, pu, rate, discount, total) in &line_rows {
        lines.push_str(&csv_row(&[
            sale_id,
            reference,
            label,
            &qty.to_string(),
            &pu.to_string(),
            &rate.to_string(),
            &discount.to_string(),
            &total.to_string(),
        ]));
    }

    let payment_rows: Vec<(String, String, String, i64, String, i64)> = sqlx::query_as(
        r#"
        SELECT p.id, COALESCE(s.reference, ''), COALESCE(p.paid_at, ''),
               p.amount_cents, COALESCE(p.method, ''), COALESCE(p.fiscal_seq, 0)
        FROM payments p
        LEFT JOIN sales s ON s.id = p.sale_id
        WHERE p.fiscal_register_id = $1 AND substr(COALESCE(p.paid_at, ''), 1, 10) >= $2
          AND substr(COALESCE(p.paid_at, ''), 1, 10) <= $3
        ORDER BY p.fiscal_seq ASC
        "#,
    )
    .bind(register.id.to_string())
    .bind(start)
    .bind(end)
    .fetch_all(&state.db)
    .await?;

    let mut payments =
        String::from("id;reference_document;date;montant_cents;mode;sequence_journal\n");
    for (id, reference, at, amount, method, seq) in &payment_rows {
        payments.push_str(&csv_row(&[
            id,
            reference,
            at,
            &amount.to_string(),
            method,
            &seq.to_string(),
        ]));
    }

    Ok(ArchiveContents {
        journal,
        closures,
        documents,
        lines,
        payments,
        entry_count: entries.len() as i64,
        first_seq,
        last_seq,
        public_key: register.public_key_hex(),
    })
}

fn write_zip(register: &Register, year: i32, contents: &ArchiveContents) -> LocalResult<Vec<u8>> {
    let mut buffer = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buffer);
        let mut zip = zip::ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        let members: [(&str, &str); 5] = [
            ("journal.csv", &contents.journal),
            ("clotures.csv", &contents.closures),
            ("documents.csv", &contents.documents),
            ("lignes.csv", &contents.lines),
            ("reglements.csv", &contents.payments),
        ];

        let mut digests = serde_json::Map::new();
        for (name, body) in members {
            zip.start_file(name, options)
                .map_err(|e| LocalError::Internal(anyhow::anyhow!(e)))?;
            zip.write_all(body.as_bytes())
                .map_err(|e| LocalError::Internal(e.into()))?;

            let mut hasher = Sha256::new();
            hasher.update(body.as_bytes());
            digests.insert(name.to_string(), json!(hex::encode(hasher.finalize())));
        }

        let manifest = json!({
            "logiciel": fiscal::SOFTWARE_VERSION,
            "caisse": register.code,
            "cle_publique_ed25519": contents.public_key,
            "exercice": year,
            "genere_le": Utc::now().to_rfc3339(),
            "nombre_entrees": contents.entry_count,
            "premiere_sequence": contents.first_seq,
            "derniere_sequence": contents.last_seq,
            "algorithme_empreinte": "SHA-256",
            "algorithme_signature": "Ed25519",
            "empreintes": Value::Object(digests),
        });
        let manifest_text =
            serde_json::to_string_pretty(&manifest).map_err(|e| LocalError::Internal(e.into()))?;

        zip.start_file("manifeste.json", options)
            .map_err(|e| LocalError::Internal(anyhow::anyhow!(e)))?;
        zip.write_all(manifest_text.as_bytes())
            .map_err(|e| LocalError::Internal(e.into()))?;

        zip.start_file("NOTICE.txt", options)
            .map_err(|e| LocalError::Internal(anyhow::anyhow!(e)))?;
        zip.write_all(notice(register, year).as_bytes())
            .map_err(|e| LocalError::Internal(e.into()))?;

        zip.finish()
            .map_err(|e| LocalError::Internal(anyhow::anyhow!(e)))?;
    }
    Ok(buffer)
}

/// § 230 requires a notice, in French, explaining the archive and how its
/// integrity can be checked.
fn notice(register: &Register, year: i32) -> String {
    format!(
        "ARCHIVE FISCALE — NOTICE\n\
         ========================\n\
         \n\
         Logiciel        : {software}\n\
         Caisse          : {code}\n\
         Exercice        : {year}\n\
         \n\
         OBJET\n\
         -----\n\
         Cette archive est produite en application de l'article 286, I-3° bis du code\n\
         général des impôts et du BOI-TVA-DECLA-30-10-30. Elle contient l'intégralité\n\
         des données de règlement enregistrées par la caisse ci-dessus pour l'exercice\n\
         indiqué, dans un format ouvert lisible sans le logiciel qui l'a produite.\n\
         \n\
         CONTENU\n\
         -------\n\
         journal.csv      Journal fiscal permanent : une ligne par événement, dans\n\
         \x20                l'ordre où il a été enregistré. Colonnes de chaînage\n\
         \x20                incluses (empreintes et signature).\n\
         clotures.csv     Clôtures journalières, mensuelles et annuelles, avec le\n\
         \x20                grand total de la période et le total perpétuel.\n\
         documents.csv    Factures et avoirs validés : en-têtes.\n\
         lignes.csv       Détail ligne à ligne des documents ci-dessus.\n\
         reglements.csv   Règlements encaissés.\n\
         manifeste.json   Empreinte SHA-256 de chaque fichier, clé publique de la\n\
         \x20                caisse, bornes de séquence.\n\
         \n\
         Les fichiers CSV utilisent le point-virgule comme séparateur, le codage UTF-8\n\
         et les guillemets doubles pour les valeurs qui en contiennent. Les montants\n\
         sont exprimés en centimes d'euro (entiers), les quantités en millièmes\n\
         d'unité, les taux en points de base (2000 = 20,00 %).\n\
         \n\
         VÉRIFIER L'INTÉGRITÉ\n\
         --------------------\n\
         1. Empreinte des fichiers : calculez le SHA-256 de chaque fichier extrait et\n\
         \x20  comparez-le à la valeur correspondante dans manifeste.json.\n\
         \n\
         2. Chaînage du journal : pour chaque ligne de journal.csv, l'empreinte de\n\
         \x20  chaînage se recalcule ainsi :\n\
         \n\
         \x20      SHA-256( caisse | RS | sequence | RS | horodatage | RS |\n\
         \x20               type_evenement | RS | empreinte_donnees | RS |\n\
         \x20               empreinte_precedente | RS )\n\
         \n\
         \x20  où RS est l'octet 0x1E, et où empreinte_donnees est le SHA-256 de la\n\
         \x20  colonne « donnees » sous sa forme canonique (JSON aux clés triées, sans\n\
         \x20  espace superflu). L'empreinte_precedente de la première entrée d'une\n\
         \x20  caisse est composée de soixante-quatre zéros.\n\
         \n\
         3. Signature : la colonne « signature » est la signature Ed25519 de\n\
         \x20  l'empreinte de chaînage (octets bruts), vérifiable avec la clé publique\n\
         \x20  figurant dans manifeste.json.\n\
         \n\
         Toute modification d'une ligne du journal invalide son empreinte, ainsi que\n\
         le chaînage de toutes les lignes qui la suivent. La position de la première\n\
         rupture désigne la ligne altérée.\n\
         \n\
         CONSERVATION\n\
         ------------\n\
         Cette archive doit être conservée et rester lisible pendant six ans au moins\n\
         (article L. 102 B du livre des procédures fiscales), et pendant la durée\n\
         couverte par le droit de reprise de l'administration.\n",
        software = fiscal::SOFTWARE_VERSION,
        code = register.code,
        year = year,
    )
}

/// One CSV record, semicolon-separated, quoted only where necessary.
fn csv_row(fields: &[&str]) -> String {
    let mut out = String::new();
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push(';');
        }
        out.push_str(&csv_field(field));
    }
    out.push('\n');
    out
}

fn csv_field(value: &str) -> String {
    let needs_quotes =
        value.contains(';') || value.contains('"') || value.contains('\n') || value.contains('\r');
    if needs_quotes {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[derive(sqlx::FromRow)]
struct ArchiveDbRow {
    id: String,
    period_start: String,
    period_end: String,
    file_path: String,
    file_sha256: String,
    file_bytes: i64,
    entry_count: i64,
    first_seq: i64,
    last_seq: i64,
    created_at: String,
}

impl ArchiveDbRow {
    fn into_row(self) -> ArchiveRow {
        ArchiveRow {
            id: self.id,
            period_start: self.period_start,
            period_end: self.period_end,
            file_path: self.file_path,
            file_sha256: self.file_sha256,
            file_bytes: self.file_bytes,
            entry_count: self.entry_count,
            first_seq: self.first_seq,
            last_seq: self.last_seq,
            created_at: self.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::fiscal::closures;
    use crate::local_server::test_support::{past_invoice, shop};
    use chrono::{Datelike, NaiveDate};
    use motodesk_fiscal::ClosureKind;
    use std::io::Read;

    /// A shop that traded last year and closed the year, which is the state an
    /// archive is allowed to be produced from.
    async fn traded_and_closed() -> (SharedState, Register, i32) {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        let year = Utc::now().date_naive().year() - 1;
        let day = NaiveDate::from_ymd_opt(year, 3, 12).unwrap();

        past_invoice(&state, &register, day, 40_000, 8_000).await;
        closures::close(&state, &register, ClosureKind::Day, day)
            .await
            .unwrap();
        closures::close(&state, &register, ClosureKind::Month, day)
            .await
            .unwrap();
        closures::close(&state, &register, ClosureKind::Year, day)
            .await
            .unwrap();

        (state, register, year)
    }

    fn members(bytes: &[u8]) -> Vec<String> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
        (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect()
    }

    fn member_text(bytes: &[u8], name: &str) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
        let mut file = zip.by_name(name).unwrap();
        let mut out = String::new();
        file.read_to_string(&mut out).unwrap();
        out
    }

    #[tokio::test]
    async fn an_unclosed_year_cannot_be_archived() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();
        let year = Utc::now().date_naive().year() - 1;

        assert!(matches!(
            create(&state, &register, year).await,
            Err(LocalError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn a_running_year_cannot_be_archived() {
        let state = shop().await;
        let register = fiscal::current(&state).await.unwrap();

        assert!(matches!(
            create(&state, &register, fiscal::current_year()).await,
            Err(LocalError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn the_archive_carries_open_data_a_manifest_and_a_notice() {
        let (state, register, year) = traded_and_closed().await;
        let archive = create(&state, &register, year).await.unwrap();

        let bytes = std::fs::read(&archive.file_path).unwrap();
        let names = members(&bytes);
        for expected in [
            "journal.csv",
            "clotures.csv",
            "documents.csv",
            "lignes.csv",
            "reglements.csv",
            "manifeste.json",
            "NOTICE.txt",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }

        // § 230: the notice is in French and says how to check the archive.
        let notice = member_text(&bytes, "NOTICE.txt");
        assert!(notice.contains("VÉRIFIER L'INTÉGRITÉ"));
        assert!(notice.contains("286"));

        // The manifest's digest of a member has to match that member.
        let manifest: Value = serde_json::from_str(&member_text(&bytes, "manifeste.json")).unwrap();
        let recorded = manifest["empreintes"]["journal.csv"].as_str().unwrap();
        let mut hasher = Sha256::new();
        hasher.update(member_text(&bytes, "journal.csv").as_bytes());
        assert_eq!(recorded, hex::encode(hasher.finalize()));

        assert_eq!(manifest["cle_publique_ed25519"], register.public_key_hex());
        assert!(archive.entry_count > 0);
    }

    #[tokio::test]
    async fn a_period_is_archived_only_once() {
        let (state, register, year) = traded_and_closed().await;

        let first = create(&state, &register, year).await.unwrap();
        let second = create(&state, &register, year).await.unwrap();

        assert_eq!(first.id, second.id);
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fiscal_archives")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn a_tampered_archive_fails_its_check() {
        let (state, register, year) = traded_and_closed().await;
        let archive = create(&state, &register, year).await.unwrap();

        let intact = verify(&state, &archive.id).await.unwrap();
        assert_eq!(intact["ok"], true);

        std::fs::write(&archive.file_path, b"not the archive it was").unwrap();
        let broken = verify(&state, &archive.id).await.unwrap();
        assert_eq!(broken["ok"], false);
        assert!(broken["problem"].is_string());
    }

    #[tokio::test]
    async fn a_missing_archive_is_reported_rather_than_erroring() {
        let (state, register, year) = traded_and_closed().await;
        let archive = create(&state, &register, year).await.unwrap();

        std::fs::remove_file(&archive.file_path).unwrap();
        let report = verify(&state, &archive.id).await.unwrap();
        assert_eq!(report["ok"], false);
        assert_eq!(report["problem"], "fichier d'archive introuvable");
    }

    #[test]
    fn plain_values_are_not_quoted() {
        assert_eq!(csv_row(&["a", "b"]), "a;b\n");
    }

    #[test]
    fn separators_and_quotes_are_escaped() {
        assert_eq!(
            csv_row(&["Durand; Fils", "24\" wheel"]),
            "\"Durand; Fils\";\"24\"\" wheel\"\n"
        );
    }

    #[test]
    fn newlines_inside_a_field_survive_round_tripping() {
        assert_eq!(csv_field("line1\nline2"), "\"line1\nline2\"");
    }
}
