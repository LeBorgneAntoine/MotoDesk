//! From the cells of a file to the rows the service takes (port of
//! `desktop/src/lib/import/transforms.ts`): which column fills which field, how its text is
//! cleaned, read as a number, a rate or a yes/no, and what is wrong with it when it is not.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization as _;

use super::ImportResult;
use super::csv::{ParsedCsv, to_csv};
use super::schema::{Entity, Field, FieldType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseMode {
    #[default]
    Unchanged,
    Upper,
    Lower,
    Title,
}

impl CaseMode {
    pub const ALL: [CaseMode; 4] = [CaseMode::Unchanged, CaseMode::Upper, CaseMode::Lower, CaseMode::Title];

    pub fn id(self) -> &'static str {
        match self {
            CaseMode::Unchanged => "none",
            CaseMode::Upper => "upper",
            CaseMode::Lower => "lower",
            CaseMode::Title => "title",
        }
    }

    pub fn from_id(id: &str) -> CaseMode {
        Self::ALL.into_iter().find(|mode| mode.id() == id).unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            CaseMode::Unchanged => "Inchangé",
            CaseMode::Upper => "MAJUSCULES",
            CaseMode::Lower => "minuscules",
            CaseMode::Title => "Capitales Initiales",
        }
    }
}

/// Where the value of a field comes from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Source {
    /// The field is not imported.
    #[default]
    Ignore,
    /// A column of the file, by its header.
    Column(String),
    /// The same value for every row ([`Mapping::constant`]).
    Constant,
}

/// How one field of the entity is filled.
#[derive(Debug, Clone, PartialEq)]
pub struct Mapping {
    pub field: &'static Field,
    pub source: Source,
    pub constant: String,
    pub trim: bool,
    pub case: CaseMode,
    /// An empty value is left out rather than sent as an empty text.
    pub empty_to_null: bool,
    /// What an empty value is replaced with.
    pub default_value: String,
    /// For a reference: the column of the other table the value is looked up by.
    pub ref_lookup: &'static str,
}

impl Mapping {
    pub fn new(field: &'static Field) -> Self {
        Self {
            field,
            source: Source::Ignore,
            constant: String::new(),
            trim: true,
            case: CaseMode::Unchanged,
            empty_to_null: true,
            default_value: String::new(),
            ref_lookup: field.default_lookup(),
        }
    }

    pub fn is_ignored(&self) -> bool {
        self.source == Source::Ignore
    }
}

/// One cell of the preview.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cell {
    /// The value as the preview shows it.
    pub display: String,
    pub error: Option<String>,
}

/// One row of the file, ready to send or not.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RowResult {
    /// What the service receives: only the fields that have a value.
    pub payload: Map<String, Value>,
    /// One cell per field of the entity, in their order.
    pub cells: Vec<Cell>,
    pub errors: Vec<String>,
}

impl RowResult {
    pub fn has_error(&self) -> bool {
        !self.errors.is_empty()
    }
}

// ---- matching columns to fields by their headers ----------------------------------------------

/// What people call a field in their exports, without accents, spaces or capitals.
fn aliases(key: &str) -> &'static [&'static str] {
    match key {
        "first_name" => &["firstname", "prenom", "prenoms"],
        "last_name" => &["lastname", "nom", "nomdefamille", "surname", "famille"],
        "phone" => &["telephone", "tel", "mobile", "portable", "numero", "gsm"],
        "email" => &["mail", "courriel", "adressemail", "emailaddress"],
        "address" => &["adresse", "addr", "rue", "adresseligne", "addressline"],
        "notes" => &["note", "remarque", "commentaire", "details", "detail"],
        "name" => &["nom", "designation", "libelle", "intitule", "title", "titre"],
        "position" => &["ordre", "rang", "index", "tri"],
        "category" => &["categorie", "famille", "rayon", "categoryname", "libellefamille"],
        "id" => &["uuid", "identifiant", "identifiantid"],
        "sku" => &["reference", "ref", "codearticle", "code", "refarticle", "articlecode"],
        "brand" => &["marque", "fabricant", "manufacturer"],
        "description" => &["descriptif", "desc"],
        "unit" => &["unite", "conditionnement"],
        "is_service" => &["service", "estservice"],
        "purchase_price_ht_cents" => &["prixachat", "achat", "purchaseprice", "prixachatht", "pa", "prixhtachat"],
        "selling_price_ttc_cents" => &[
            "prixvente",
            "prix",
            "prixttc",
            "saleprice",
            "sellingprice",
            "prixventettc",
            "pv",
            "saleprixeur",
            "salepriceeur",
            "prixeur",
        ],
        "vat_rate_bp" => &["tva", "vat", "tauxtva", "vatrate"],
        "stock_milli" => &["stock", "quantite", "qte", "qty", "quantity", "stockquantity"],
        "low_stock_threshold" => &["seuil", "seuilstock", "stockmini", "lowstock", "alerte"],
        "is_active" => &["actif", "active", "enabled", "disponible"],
        "owner" => &["proprietaire", "client", "clientnom", "owner", "customer", "owneremail", "ownerphone"],
        "vehicle_type" => &["type", "typevehicule", "vehicletype", "categorie"],
        "make" => &["marque", "make", "constructeur"],
        "model" => &["modele", "model"],
        "year" => &["annee", "year", "millesime"],
        "cylinder_capacity" => &["cylindree", "cc", "cylinder", "cylindreecc"],
        "vin_or_serial" => &["vin", "numeroserie", "serie", "serial", "noserie", "chassis"],
        "plate_number" => &["immatriculation", "plaque", "immat", "plate", "platenumber"],
        "reference" => &["reference", "ref", "numero", "ticket", "facture", "invoice", "ndoc", "numfacture"],
        "client" => &["client", "customer", "clientid", "acheteur"],
        "vehicle" => &["vehicule", "vehicle", "vehiculeid", "moto"],
        "status" => &["statut", "status", "etat"],
        "sale_date" => &["date", "datevente", "saledate", "datefacture"],
        "total_ht_cents" => &["totalht", "ht", "montantht"],
        "total_ttc_cents" => &["totalttc", "ttc", "montantttc", "total"],
        "payment_method" => &["paiement", "modepaiement", "payment", "reglement"],
        "sale" => &["vente", "sale", "ticket", "facture", "reference", "saleid", "venteid"],
        "product" => &["produit", "product", "article", "sku", "ref", "productid", "produitid"],
        "label" => &["designation", "libelle", "label", "intitule", "description"],
        "quantity_milli" => &["quantite", "qte", "quantity", "qty", "nombre"],
        "unit_price_ht_cents" => &["prixunitaireht", "puht", "unitpriceht", "prixht"],
        "unit_price_ttc_cents" => &["prixunitairettc", "puttc", "unitpricettc", "prixttc", "prixunitaire", "pu"],
        "discount_bp" => &["remise", "discount", "rabais"],
        "line_total_ttc_cents" => &["totalligne", "montantligne", "linetotal", "totallignettc"],
        _ => &[],
    }
}

/// A header reduced to what identifies it: no accent, no capital, letters and digits only.
pub fn normalise_header(header: &str) -> String {
    header.nfd().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// The names a field goes by, normalised: its key, its label, then its aliases.
fn candidates(field: &Field) -> Vec<String> {
    let mut names = vec![normalise_header(field.key), normalise_header(field.label)];
    for alias in aliases(field.key) {
        if !names.iter().any(|name| name == alias) {
            names.push(alias.to_string());
        }
    }
    names
}

/// The header that is one of the field's names, among those still free.
fn exact_match<'h>(field: &Field, headers: &[(&'h str, String)], used: &[&str]) -> Option<&'h str> {
    candidates(field)
        .iter()
        .find_map(|name| headers.iter().find(|(raw, norm)| norm == name && !used.contains(raw)).map(|(raw, _)| *raw))
}

/// Failing that, a header that holds one of the names or is held by one, the longest names
/// first. Names and headers of fewer than three letters are too short to mean anything.
fn loose_match<'h>(field: &Field, headers: &[(&'h str, String)], used: &[&str]) -> Option<&'h str> {
    let mut names = candidates(field);
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    names.iter().filter(|name| name.len() >= 3).find_map(|name| {
        headers
            .iter()
            .find(|(raw, norm)| !used.contains(raw) && norm.len() >= 3 && (norm.contains(name.as_str()) || name.contains(norm.as_str())))
            .map(|(raw, _)| *raw)
    })
}

/// A column named like an identifier (`client_id`, `uuid`) is looked up by identifier.
fn lookup_for_header(field: &Field, header: &str) -> &'static str {
    if field.reference().is_none() {
        return field.default_lookup();
    }
    let normalised = normalise_header(header);
    // A natural-key column (`client`, `owner`, `sale`...) is not an identifier just because a
    // sibling `*_id` column exists.
    if normalised == normalise_header(field.key) {
        return field.default_lookup();
    }
    if normalised.ends_with("id") || normalised.contains("uuid") { "id" } else { field.default_lookup() }
}

/// A first mapping for an entity: each field takes the column whose header looks like it.
/// A column fills one field only.
///
/// Headers that are exactly a field's name are given out first, to every field, and only
/// then the loose matches: otherwise "Prénom" would take a column headed "Nom" (its own name
/// holds it) before "Nom" got to it.
pub fn suggest_mappings(entity: &'static Entity, headers: &[String]) -> Vec<Mapping> {
    let normalised: Vec<(&str, String)> = headers.iter().map(|h| (h.as_str(), normalise_header(h))).collect();
    let mut used: Vec<&str> = Vec::new();
    let mut found: Vec<Option<&str>> = entity
        .fields
        .iter()
        .map(|field| {
            let header = exact_match(field, &normalised, &used);
            used.extend(header);
            header
        })
        .collect();
    for (field, slot) in entity.fields.iter().zip(found.iter_mut()) {
        if slot.is_none() {
            *slot = loose_match(field, &normalised, &used);
            used.extend(*slot);
        }
    }
    entity
        .fields
        .iter()
        .zip(found)
        .map(|(field, header)| {
            let mut mapping = Mapping::new(field);
            if let Some(header) = header {
                mapping.source = Source::Column(header.to_string());
                mapping.ref_lookup = lookup_for_header(field, header);
            }
            mapping
        })
        .collect()
}

/// The required fields no column fills: what stops the wizard at the mapping step.
pub fn unmapped_required(mappings: &[Mapping]) -> Vec<&'static Field> {
    mappings.iter().filter(|mapping| mapping.field.required && mapping.is_ignored()).map(|mapping| mapping.field).collect()
}

// ---- reading values ---------------------------------------------------------------------------

/// "jean-pierre DUPONT" becomes "Jean-Pierre Dupont".
fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut start_of_word = true;
    for c in text.chars() {
        if c.is_alphabetic() {
            if start_of_word {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start_of_word = false;
        } else {
            out.push(c);
            // An apostrophe is part of the word ("d'Arc"); a hyphen starts another ("Jean-Pierre").
            start_of_word = c != '\'';
        }
    }
    out
}

fn apply_text(raw: &str, mapping: &Mapping) -> String {
    let text = if mapping.trim { raw.trim() } else { raw };
    let text = match mapping.case {
        CaseMode::Unchanged => text.to_string(),
        CaseMode::Upper => text.to_uppercase(),
        CaseMode::Lower => text.to_lowercase(),
        CaseMode::Title => title_case(text),
    };
    if text.is_empty() { mapping.default_value.trim().to_string() } else { text }
}

/// A number written the French or the English way: `1.592,81`, `1,234.56`, `1 234,5`, `12,5`.
pub fn parse_number(text: &str) -> Option<f64> {
    let kept: String = text.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')).collect();
    if kept.is_empty() {
        return None;
    }
    let cleaned = match (kept.rfind(','), kept.rfind('.')) {
        // Whichever comes last is the decimal mark; the other one groups thousands.
        (Some(comma), Some(dot)) if comma > dot => kept.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => kept.replace(',', ""),
        (Some(_), None) => kept.replacen(',', ".", 1),
        _ => kept,
    };
    cleaned.parse::<f64>().ok().filter(|number| number.is_finite())
}

fn parse_flag(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "1" | "true" | "vrai" | "oui" | "o" | "yes" | "y" | "x" | "actif" => Some(true),
        "0" | "false" | "faux" | "non" | "n" | "no" | "" | "inactif" => Some(false),
        _ => None,
    }
}

/// A number the French way: a comma, thin spaces between thousands, at least `min` and at
/// most `max` decimals.
fn french(number: f64, min: usize, max: usize) -> String {
    let fixed = format!("{:.*}", max, number.abs());
    let (whole, decimals) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut decimals = decimals.to_string();
    while decimals.len() > min && decimals.ends_with('0') {
        decimals.pop();
    }
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push('\u{202f}');
        }
        grouped.push(digit);
    }
    let sign = if number < 0. && fixed.chars().any(|c| c.is_ascii_digit() && c != '0') { "-" } else { "" };
    if decimals.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped},{decimals}") }
}

/// Whether the header of the column says its values are already in the stored unit
/// (`..._cents`, `..._milli`, `..._bp`).
fn header_says(source: &Source, token: &str) -> bool {
    matches!(source, Source::Column(header) if normalise_header(header).contains(token))
}

/// The value to send and what the preview shows, or what is wrong with the text.
fn convert(field: &Field, text: &str, source: &Source) -> Result<(Value, String), (String, String)> {
    let bad = |why: &str| Err((text.to_string(), why.to_string()));
    match field.kind {
        FieldType::Text | FieldType::Reference(_) => Ok((Value::from(text), text.to_string())),
        FieldType::Quantity => {
            let Some(number) = parse_number(text) else { return bad("Quantité invalide") };
            let milli = if header_says(source, "milli") { number.round() } else { (number * 1000.).round() } as i64;
            Ok((Value::from(milli), french(milli as f64 / 1000., 0, 3)))
        }
        FieldType::Integer => {
            let Some(number) = parse_number(text) else { return bad("Entier invalide") };
            let whole = number.round() as i64;
            Ok((Value::from(whole), whole.to_string()))
        }
        FieldType::Money => {
            let Some(number) = parse_number(text) else { return bad("Montant invalide") };
            let cents = if header_says(source, "cent") { number.round() } else { (number * 100.).round() } as i64;
            Ok((Value::from(cents), format!("{} €", french(cents as f64 / 100., 2, 2))))
        }
        FieldType::Rate => {
            let Some(number) = parse_number(text).filter(|number| *number >= 0.) else { return bad("Taux invalide") };
            // A rate is written the way people write it: "20", "20 %", "5,5". A value already
            // in basis points is known by its header ("..._bp") or by being above 100.
            let bp = if header_says(source, "bp") || number > 100. { number.round() } else { (number * 100.).round() } as i64;
            Ok((Value::from(bp), format!("{} %", french(bp as f64 / 100., 0, 2))))
        }
        FieldType::Flag => match parse_flag(text) {
            Some(flag) => Ok((Value::from(flag), if flag { "Oui" } else { "Non" }.to_string())),
            None => bad("Booléen invalide (oui/non)"),
        },
        FieldType::Choice(options) => {
            let wanted = normalise_header(text);
            match options.iter().find(|(value, label)| normalise_header(value) == wanted || normalise_header(label) == wanted) {
                Some((value, label)) => Ok((Value::from(*value), label.to_string())),
                None => {
                    let allowed: Vec<&str> = options.iter().map(|(_, label)| *label).collect();
                    bad(&format!("Valeur invalide ({})", allowed.join(", ")))
                }
            }
        }
    }
}

// ---- rows -------------------------------------------------------------------------------------

/// One row: `cell_of(mapping)` gives the text of the file for a mapping read from a column.
fn transform_row<'a>(mappings: &[Mapping], cell_of: impl Fn(usize) -> &'a str) -> RowResult {
    let mut row = RowResult::default();
    for (index, mapping) in mappings.iter().enumerate() {
        let field = mapping.field;
        let raw = match &mapping.source {
            Source::Ignore => {
                row.cells.push(Cell::default());
                continue;
            }
            Source::Constant => mapping.constant.as_str(),
            Source::Column(_) => cell_of(index),
        };
        let text = apply_text(raw, mapping);
        if text.is_empty() {
            if field.required {
                row.cells.push(Cell { display: String::new(), error: Some("Champ requis".into()) });
                row.errors.push(format!("{} : requis", field.label));
            } else {
                // Left out, unless an empty text is what was asked for.
                if !mapping.empty_to_null && field.kind == FieldType::Text {
                    row.payload.insert(field.key.to_string(), Value::from(""));
                }
                row.cells.push(Cell::default());
            }
            continue;
        }
        match convert(field, &text, &mapping.source) {
            Ok((value, display)) => {
                row.payload.insert(field.key.to_string(), value);
                row.cells.push(Cell { display, error: None });
            }
            Err((display, error)) => {
                row.errors.push(format!("{} : {error}", field.label));
                row.cells.push(Cell { display, error: Some(error) });
            }
        }
    }
    row
}

/// Every row of the file, through the mapping. `mappings` has one entry per field of the
/// entity, in their order (as [`suggest_mappings`] builds it).
pub fn transform(parsed: &ParsedCsv, mappings: &[Mapping]) -> Vec<RowResult> {
    // Where each mapping reads in a row, found once.
    let columns: Vec<Option<usize>> = mappings
        .iter()
        .map(|mapping| match &mapping.source {
            Source::Column(header) => parsed.column(header),
            _ => None,
        })
        .collect();
    parsed
        .rows
        .iter()
        .map(|raw| transform_row(mappings, |index| columns[index].and_then(|column| raw.get(column)).map_or("", String::as_str)))
        .collect()
}

/// The rows that share a value of `key` with another row of the file.
pub fn find_duplicate_rows(results: &[RowResult], key: &str) -> BTreeSet<usize> {
    let mut seen: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, result) in results.iter().enumerate() {
        let value = match result.payload.get(key) {
            Some(Value::String(text)) if !text.is_empty() => text.to_lowercase(),
            Some(Value::Null) | None | Some(Value::String(_)) => continue,
            Some(other) => other.to_string(),
        };
        seen.entry(value).or_default().push(index);
    }
    seen.into_values().filter(|rows| rows.len() > 1).flatten().collect()
}

/// The rows without an error, as the service takes them, and for each its position among
/// the rows of the file (what turns a refused row back into a line of the file).
pub fn sendable(results: &[RowResult]) -> (Vec<Map<String, Value>>, Vec<usize>) {
    results.iter().enumerate().filter(|(_, result)| !result.has_error()).map(|(index, result)| (result.payload.clone(), index)).unzip()
}

/// The line of the file a row sits on: the header is line 1.
pub fn line_of(row_index: usize) -> usize {
    row_index + 2
}

/// The lookup column chosen for each reference that is imported, by field.
pub fn references(mappings: &[Mapping]) -> std::collections::BTreeMap<String, String> {
    mappings
        .iter()
        .filter(|mapping| !mapping.is_ignored() && mapping.field.reference().is_some())
        .map(|mapping| (mapping.field.key.to_string(), mapping.ref_lookup.to_string()))
        .collect()
}

/// What the service said about a row it refused, in words for the person importing. A value
/// that must be unique and is already taken comes back as the database's own sentence
/// (`UNIQUE constraint failed: products.sku`): it is said with the name of the field.
pub fn explain_refusal(entity: &Entity, message: &str) -> String {
    let column = message
        .split("UNIQUE constraint failed: ")
        .nth(1)
        .and_then(|rest| rest.split([',', ' ']).next())
        .and_then(|qualified| qualified.rsplit('.').next())
        .filter(|column| !column.is_empty());
    match column {
        Some(column) => format!("{} : cette valeur est déjà utilisée par une autre fiche.", entity.label_of(column)),
        None => message.to_string(),
    }
}

/// The rows that were not imported, as a CSV to correct and import again: the line they came
/// from, what was wrong, then the row as it was. `None` when every row went in.
///
/// Written with the separator of the file it came from, and marked as UTF-8 so that Excel
/// reads its accents.
pub fn error_report(
    entity: &Entity,
    parsed: &ParsedCsv,
    results: &[RowResult],
    outcome: Option<&ImportResult>,
    index_map: &[usize],
) -> Option<String> {
    let mut headers = vec!["ligne_csv".to_string(), "erreur".to_string()];
    headers.extend(parsed.headers.iter().cloned());
    let line = |index: usize, error: String| {
        let mut row = vec![line_of(index).to_string(), error];
        row.extend(parsed.rows.get(index).cloned().unwrap_or_default());
        row
    };
    // What the file itself got wrong, then what the service refused.
    let mut rows: Vec<Vec<String>> =
        results.iter().enumerate().filter(|(_, result)| result.has_error()).map(|(index, result)| line(index, result.errors.join(" · "))).collect();
    if let Some(outcome) = outcome {
        for refused in outcome.errors() {
            let index = index_map.get(refused.row).copied().unwrap_or_default();
            rows.push(line(index, explain_refusal(entity, refused.message.as_deref().unwrap_or_default())));
        }
    }
    (!rows.is_empty()).then(|| format!("\u{feff}{}", to_csv(&headers, &rows, parsed.delimiter)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::csv::parse;
    use super::super::schema::entity;
    use super::super::{Disposition, RowOutcome};
    use super::*;

    fn headers(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// The value a single column gives a field of an entity.
    fn read(entity_id: &str, field: &str, header: &str, value: &str) -> RowResult {
        let entity = entity(entity_id).unwrap();
        let mappings: Vec<Mapping> = entity
            .fields
            .iter()
            .map(|f| {
                let mut mapping = Mapping::new(f);
                if f.key == field {
                    mapping.source = Source::Column(header.to_string());
                }
                mapping
            })
            .collect();
        let parsed = ParsedCsv { headers: headers(&[header]), rows: vec![vec![value.to_string()]], delimiter: ';' };
        transform(&parsed, &mappings).remove(0)
    }

    #[test]
    fn numbers_are_read_the_french_and_the_english_way() {
        assert_eq!(parse_number("1.592,81"), Some(1592.81));
        assert_eq!(parse_number("1,234.56"), Some(1234.56));
        assert_eq!(parse_number("1 234,5"), Some(1234.5));
        assert_eq!(parse_number("12,5"), Some(12.5));
        assert_eq!(parse_number("-3.5"), Some(-3.5));
        assert_eq!(parse_number("19,90 €"), Some(19.9));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number(""), None);
    }

    #[test]
    fn a_rate_is_taken_as_written_in_percent() {
        let rate = |value: &str, header: &str| read("products", "vat_rate_bp", header, value).payload.get("vat_rate_bp").cloned();
        assert_eq!(rate("20", "TVA"), Some(json!(2000)));
        assert_eq!(rate("5,5", "TVA"), Some(json!(550)));
        assert_eq!(rate("20 %", "TVA"), Some(json!(2000)));
        // Already in basis points: above 100, or said by the header.
        assert_eq!(rate("2000", "TVA"), Some(json!(2000)));
        assert_eq!(rate("2000", "vat_rate_bp"), Some(json!(2000)));
        assert_eq!(rate("550", "tva_bp"), Some(json!(550)));
        let refused = read("products", "vat_rate_bp", "TVA", "-5");
        assert_eq!(refused.errors, ["TVA (%) : Taux invalide"]);
        assert_eq!(read("products", "vat_rate_bp", "TVA", "5,5").cells[9].display, "5,5 %");
    }

    #[test]
    fn a_quantity_keeps_its_decimals_in_thousandths() {
        let stock = |value: &str, header: &str| read("products", "stock_milli", header, value).payload.get("stock_milli").cloned();
        assert_eq!(stock("12", "Stock"), Some(json!(12_000)));
        assert_eq!(stock("2,5", "Stock"), Some(json!(2_500)));
        assert_eq!(stock("0,75", "Stock"), Some(json!(750)));
        assert_eq!(stock("2500", "stock_milli"), Some(json!(2_500)));
    }

    #[test]
    fn an_amount_is_typed_in_euros_and_stored_in_cents() {
        let price = read("products", "selling_price_ttc_cents", "Prix", "1 592,81 €");
        assert_eq!(price.payload.get("selling_price_ttc_cents"), Some(&json!(159_281)));
        assert_eq!(price.cells[8].display, "1\u{202f}592,81 €");
        let already = read("products", "selling_price_ttc_cents", "selling_price_ttc_cents", "1990");
        assert_eq!(already.payload.get("selling_price_ttc_cents"), Some(&json!(1990)));
        assert_eq!(already.cells[8].display, "19,90 €");
        assert_eq!(read("products", "selling_price_ttc_cents", "Prix", "gratuit").errors, ["Prix de vente TTC : Montant invalide"]);
    }

    #[test]
    fn yes_no_and_lists_are_read_however_they_are_written() {
        assert_eq!(read("products", "is_active", "Actif", "Oui").payload.get("is_active"), Some(&json!(true)));
        assert_eq!(read("products", "is_active", "Actif", "non").payload.get("is_active"), Some(&json!(false)));
        assert_eq!(read("products", "is_active", "Actif", "peut-être").errors, ["Actif : Booléen invalide (oui/non)"]);

        // By label, without its accent, or by stored value.
        assert_eq!(read("vehicles", "vehicle_type", "Type", "velo").payload.get("vehicle_type"), Some(&json!("bicycle")));
        assert_eq!(read("vehicles", "vehicle_type", "Type", "MOTO").payload.get("vehicle_type"), Some(&json!("moto")));
        let unknown = read("vehicles", "vehicle_type", "Type", "quad");
        assert_eq!(unknown.errors, ["Type : Valeur invalide (Moto, Vélo, Électrique)"]);
    }

    #[test]
    fn a_required_field_left_empty_is_an_error_and_an_optional_one_is_left_out() {
        let nameless = read("clients", "last_name", "Nom", "  ");
        assert!(nameless.has_error());
        assert_eq!(nameless.errors, ["Nom : requis"]);
        assert_eq!(nameless.cells[2].error.as_deref(), Some("Champ requis"));

        let no_phone = read("clients", "phone", "Téléphone", "");
        assert!(!no_phone.has_error());
        assert!(!no_phone.payload.contains_key("phone"));
    }

    #[test]
    fn text_is_trimmed_recased_defaulted_and_sent_empty_only_when_asked() {
        let clients = entity("clients").unwrap();
        let mut mapping = Mapping::new(clients.field("last_name").unwrap());
        mapping.source = Source::Constant;
        mapping.constant = "  jean-pierre d'ARC  ".into();
        mapping.case = CaseMode::Title;
        assert_eq!(transform_row(&[mapping.clone()], |_| "").payload.get("last_name"), Some(&json!("Jean-Pierre D'arc")));
        mapping.case = CaseMode::Upper;
        assert_eq!(transform_row(&[mapping.clone()], |_| "").payload.get("last_name"), Some(&json!("JEAN-PIERRE D'ARC")));

        let mut notes = Mapping::new(clients.field("notes").unwrap());
        notes.source = Source::Column("Notes".into());
        notes.default_value = " à rappeler ".into();
        assert_eq!(transform_row(&[notes.clone()], |_| " ").payload.get("notes"), Some(&json!("à rappeler")));
        notes.default_value.clear();
        notes.empty_to_null = false;
        assert_eq!(transform_row(&[notes], |_| "").payload.get("notes"), Some(&json!("")));
    }

    #[test]
    fn columns_are_matched_to_fields_by_their_headers() {
        let clients = entity("clients").unwrap();
        let found = suggest_mappings(clients, &headers(&["Prénom", "NOM", "Téléphone portable", "Courriel", "Ville"]));
        let source = |key: &str| found.iter().find(|m| m.field.key == key).map(|m| m.source.clone()).unwrap();
        assert_eq!(source("first_name"), Source::Column("Prénom".into()));
        assert_eq!(source("last_name"), Source::Column("NOM".into()));
        assert_eq!(source("phone"), Source::Column("Téléphone portable".into()));
        assert_eq!(source("email"), Source::Column("Courriel".into()));
        assert_eq!(source("address"), Source::Ignore);
        assert!(unmapped_required(&found).is_empty());

        // "Prénom" does not take the column headed "Nom" from the name, which is required.
        let short = suggest_mappings(clients, &headers(&["Nom", "Email"]));
        assert_eq!(short.iter().find(|m| m.field.key == "last_name").unwrap().source, Source::Column("Nom".into()));
        assert_eq!(short.iter().find(|m| m.field.key == "first_name").unwrap().source, Source::Ignore);

        // Without a column for the name, the wizard stops at the mapping.
        let incomplete = suggest_mappings(clients, &headers(&["Courriel"]));
        assert_eq!(unmapped_required(&incomplete).iter().map(|f| f.key).collect::<Vec<_>>(), ["last_name"]);
    }

    #[test]
    fn a_column_fills_one_field_and_an_id_column_is_looked_up_by_id() {
        let vehicles = entity("vehicles").unwrap();
        let found = suggest_mappings(vehicles, &headers(&["client_id", "Marque", "Modèle"]));
        let owner = found.iter().find(|m| m.field.key == "owner").unwrap();
        assert_eq!(owner.source, Source::Column("client_id".into()));
        assert_eq!(owner.ref_lookup, "id");

        let by_mail = suggest_mappings(vehicles, &headers(&["owner", "Marque", "Modèle"]));
        assert_eq!(by_mail.iter().find(|m| m.field.key == "owner").unwrap().ref_lookup, "email_or_phone");
        assert_eq!(references(&by_mail).get("owner").map(String::as_str), Some("email_or_phone"));

        // "Marque" names both the brand and nothing else here: it is used once.
        let columns: Vec<&Source> = found.iter().map(|m| &m.source).filter(|s| **s == Source::Column("Marque".into())).collect();
        assert_eq!(columns.len(), 1);
    }

    #[test]
    fn rows_sharing_a_key_are_found_and_only_clean_rows_are_sent() {
        let parsed = parse("Nom;Email\nMartin;lea@example.fr\n;sans.nom@example.fr\nDubois;LEA@example.fr\nMoreau;\n").unwrap();
        let mappings = suggest_mappings(entity("clients").unwrap(), &parsed.headers);
        let results = transform(&parsed, &mappings);
        assert_eq!(results.iter().filter(|r| r.has_error()).count(), 1);
        // The same address, whatever its capitals; a row without one shares nothing.
        assert_eq!(find_duplicate_rows(&results, "email").into_iter().collect::<Vec<_>>(), [0, 2]);

        let (rows, index_map) = sendable(&results);
        assert_eq!(rows.len(), 3);
        assert_eq!(index_map, [0, 2, 3]);
        assert_eq!(rows[1].get("last_name"), Some(&json!("Dubois")));
        assert_eq!(line_of(index_map[2]), 5);
    }

    #[test]
    fn the_error_report_names_the_line_what_was_wrong_and_the_row_as_it_was() {
        let parsed = parse("Nom;Email\nMartin;lea@example.fr\n;sans.nom@example.fr\nDubois;deja@example.fr\n").unwrap();
        let mappings = suggest_mappings(entity("clients").unwrap(), &parsed.headers);
        let results = transform(&parsed, &mappings);
        let (_, index_map) = sendable(&results);
        // The service refused the second row sent: the third row of the file, on line 4.
        let outcome = ImportResult {
            inserted: 1,
            errored: 1,
            rows: vec![
                RowOutcome { row: 0, disposition: Disposition::Insert, message: None, id: None },
                RowOutcome { row: 1, disposition: Disposition::Error, message: Some("E-mail déjà utilisé".into()), id: None },
            ],
            ..Default::default()
        };
        let clients = entity("clients").unwrap();
        let report = error_report(clients, &parsed, &results, Some(&outcome), &index_map).unwrap();
        assert_eq!(
            report,
            "\u{feff}ligne_csv;erreur;Nom;Email\r\n3;Nom : requis;;sans.nom@example.fr\r\n4;E-mail déjà utilisé;Dubois;deja@example.fr"
        );
        // Before anything is sent, only the file's own errors are there.
        assert!(error_report(clients, &parsed, &results, None, &index_map).unwrap().ends_with("3;Nom : requis;;sans.nom@example.fr"));
        let clean = parse("Nom\nMartin\n").unwrap();
        let clean_results = transform(&clean, &suggest_mappings(entity("clients").unwrap(), &clean.headers));
        assert_eq!(error_report(clients, &clean, &clean_results, None, &[0]), None);
    }

    #[test]
    fn a_value_already_taken_is_said_with_the_name_of_its_field() {
        let products = entity("products").unwrap();
        let raw = "error returned from database: (code: 2067) UNIQUE constraint failed: products.sku";
        assert_eq!(explain_refusal(products, raw), "Référence (SKU) : cette valeur est déjà utilisée par une autre fiche.");
        // A column the wizard does not know keeps its name; anything else is passed on as it is.
        assert!(explain_refusal(products, "UNIQUE constraint failed: products.slug").starts_with("slug : "));
        assert_eq!(explain_refusal(products, "Catégorie introuvable"), "Catégorie introuvable");
    }
}
