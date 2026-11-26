//! A real document as a template reads it.
//!
//! Everything an invoice must carry by law is put together here: who issues it and under which
//! registration numbers (CGI ann. II art. 242 nonies A), the payment terms and the penalties
//! for paying late (C. com. art. L441-9), and the amounts line by line. A document that reaches
//! paper without one of these has to be issued again, so the wording comes from the same
//! profile the service checks before it agrees to seal anything.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, SecondsFormat};
use motodesk_fiscal::vat::{self, LineInput};
use serde_json::{json, Map, Value};

use super::interpolate::grouped;
use super::TemplateKind;
use crate::clients::{format_client_name, format_vehicle_label, ClientDetail};
use crate::fiscal::ProfileCheck;
use crate::sales::{payment_method_label, Sale, SaleLine};

/// What a document is printed with, besides itself.
#[derive(Debug, Clone, Copy)]
pub struct DocumentContext<'a> {
    /// Who issues it, as [`super::sample::shop_block`] gives it.
    pub shop: &'a Value,
    /// The customer, with their address and vehicles. `None` prints « Client de passage ».
    pub client: Option<&'a ClientDetail>,
    /// The legal wording, as the service composes it.
    pub profile: Option<&'a ProfileCheck>,
}

/// The kind of template a document is printed with.
pub fn template_kind_for(sale: &Sale) -> TemplateKind {
    if sale.is_quote() {
        TemplateKind::Devis
    } else {
        TemplateKind::Facture
    }
}

/// What is written across the top of a document.
pub fn document_title(sale: &Sale) -> &'static str {
    match (sale.is_quote(), sale.document_kind.as_str()) {
        (true, _) => "DEVIS",
        (false, "credit_note") => "AVOIR",
        (false, "receipt") => "TICKET",
        _ => "FACTURE",
    }
}

/// The instant `days` after `instant`, written the way `instant` is. Empty when `instant` is
/// not a date: nothing is then printed, rather than a wrong day.
fn days_after(instant: &str, days: i64) -> String {
    let (text, days) = (instant.trim(), Duration::days(days));
    if let Ok(at) = DateTime::parse_from_rfc3339(text) {
        (at + days).to_rfc3339_opts(SecondsFormat::Secs, true)
    } else if let Ok(at) = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
        (at + days).format("%Y-%m-%dT%H:%M:%S").to_string()
    } else if let Ok(day) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        (day + days).format("%Y-%m-%d").to_string()
    } else {
        String::new()
    }
}

fn engine_line(line: &SaleLine) -> LineInput {
    LineInput {
        quantity_milli: line.quantity_milli,
        unit_price_ht_cents: line.unit_price_ht_cents.unwrap_or(0),
        unit_price_ttc_cents: line.shelf_price_cents(),
        discount_bp: line.discount_bp,
        vat_rate_bp: line.vat_rate_bp,
    }
}

/// The lines, with their amounts from the same engine as the service: a line priced TTC (a
/// shelf price) prints what the customer paid, and the lines add up to the printed totals.
fn items(sale: &Sale) -> Vec<Value> {
    sale.lines
        .iter()
        .map(|line| {
            let computed = vat::line_totals(&engine_line(line)).ok();
            let ht = computed.map(|totals| totals.net_ht_cents).unwrap_or(0);
            // Half a cent goes up, as it does wherever else the amount is shown.
            let ttc =
                computed.and_then(|totals| totals.net_ttc_cents).unwrap_or_else(|| ht + (ht * line.vat_rate_bp * 2 + 10_000).div_euclid(20_000));
            json!({
                "label": line.label.clone().unwrap_or_default(),
                "qty": line.quantity_milli as f64 / 1000.0,
                "unit": "",
                "price_ht": line.unit_price_ht_cents.unwrap_or(0),
                "price_ttc": line.unit_price_ttc_cents.unwrap_or(0),
                "vat_rate": line.vat_rate_bp,
                "discount": line.discount_bp,
                "total_ht": ht,
                "total_ttc": ttc,
            })
        })
        .collect()
}

/// The vehicle the document is about, when it names one of the customer's.
fn vehicle(sale: &Sale, client: Option<&ClientDetail>) -> Value {
    let found = sale.vehicle_id.as_deref().and_then(|id| client?.vehicles.iter().find(|vehicle| vehicle.id == id));
    let Some(vehicle) = found else { return json!({ "label": "", "plate_number": "", "vin_or_serial": "", "odometer": "" }) };
    let odometer = match vehicle.odometer_value {
        0 => String::new(),
        value => format!("{}\u{a0}{}", grouped(&value.to_string()), vehicle.odometer_unit.label()),
    };
    json!({
        "label": format_vehicle_label(&vehicle.make, &vehicle.model, vehicle.year),
        "plate_number": vehicle.plate_number.clone().unwrap_or_default(),
        "vin_or_serial": vehicle.vin_or_serial.clone().unwrap_or_default(),
        "odometer": odometer,
    })
}

/// Everything a template can show of this document. `now` dates a document that has no date
/// of its own yet (a quote still being worked on).
pub fn document_data(sale: &Sale, context: &DocumentContext, now: DateTime<Local>) -> Value {
    let profile = context.profile.map(|check| &check.profile);
    let terms = profile.map(|profile| profile.payment_terms_days).unwrap_or(30);
    let issued = sale.locked_at.clone().or_else(|| sale.sale_date.clone()).unwrap_or_else(|| now.to_rfc3339_opts(SecondsFormat::Secs, false));
    let client = context.client.map(|detail| &detail.client);
    let text = |value: Option<&String>| value.cloned().unwrap_or_default();

    let mut data: Map<String, Value> = context.shop.as_object().cloned().unwrap_or_default();
    data.insert(
        "client".into(),
        json!({
            "first_name": text(client.and_then(|client| client.first_name.as_ref())),
            "last_name": text(client.map(|client| &client.last_name)),
            "full_name": client.map(|client| format_client_name(client.first_name.as_deref(), &client.last_name)).unwrap_or_else(|| "Client de passage".into()),
            "address": text(client.and_then(|client| client.address.as_ref())),
            "phone": text(client.and_then(|client| client.phone.as_ref())),
            "email": text(client.and_then(|client| client.email.as_ref())),
        }),
    );
    data.insert("vehicle".into(), vehicle(sale, context.client));
    data.insert("items".into(), Value::Array(items(sale)));
    data.insert(
        "payments".into(),
        sale.payments.iter().map(|payment| json!({ "date": payment.paid_at, "method": payment_method_label(payment.method.as_deref()), "amount": payment.amount_cents })).collect(),
    );
    data.insert(
        "doc".into(),
        json!({
            "number": text(sale.reference.as_ref()),
            "title": document_title(sale),
            "kind": sale.document_kind,
            "date": issued,
            "due_date": days_after(&issued, terms),
            "notes": text(sale.notes.as_ref()),
            "total_ht": sale.total_ht_cents.unwrap_or(0),
            "total_tva": sale.total_vat_cents.unwrap_or(0),
            "total_ttc": sale.total_ttc_cents.unwrap_or(0),
            "paid": sale.paid_cents,
            "outstanding": sale.outstanding_cents,
            // Mandatory wording. Without the profile it still prints: a document short of one
            // mention is better than no document.
            "payment_terms": context.profile.map(|check| check.payment_terms_text.clone()).unwrap_or_else(|| format!("Paiement à {terms} jours.")),
            "vat_exemption": profile.filter(|profile| !profile.vat_subject).map(|profile| profile.vat_exemption_text.clone()).unwrap_or_default(),
        }),
    );
    Value::Object(data)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;
    use crate::fiscal::CompanyProfile;

    fn sale(extra: Value) -> Sale {
        let mut sale = json!({
            "id": "s1", "status": "invoice", "document_kind": "invoice", "reference": "FA-2026-C1-00007",
            "client_id": "c1", "vehicle_id": "v1", "sale_date": "2026-11-05", "locked_at": "2026-11-05T10:15:00+00:00",
            "total_ht_cents": 9_765, "total_vat_cents": 1_953, "total_ttc_cents": 11_718, "paid_cents": 5_000, "outstanding_cents": 6_718,
            "notes": "Révision annuelle",
            "lines": [
                { "id": "l1", "label": "Bidon d'huile", "quantity_milli": 2_000, "unit_price_ht_cents": 833, "unit_price_ttc_cents": 999,
                  "price_basis": "ttc", "vat_rate_bp": 2000 },
                { "id": "l2", "label": "Main d'œuvre", "quantity_milli": 1_500, "unit_price_ht_cents": 6_000, "unit_price_ttc_cents": 7_200,
                  "price_basis": "ht", "vat_rate_bp": 2000, "discount_bp": 1000 },
            ],
            "payments": [{ "id": "p1", "paid_at": "2026-11-05T10:20:00+00:00", "amount_cents": 5_000, "method": "card" }],
        });
        for (key, value) in extra.as_object().cloned().unwrap_or_default() {
            sale[key] = value;
        }
        serde_json::from_value(sale).unwrap()
    }

    fn customer() -> ClientDetail {
        serde_json::from_value(json!({
            "id": "c1", "first_name": "Léa", "last_name": "Martin", "address": "12 rue des Lilas\n35000 Rennes", "phone": "06 12 34 56 78",
            "vehicles": [
                { "id": "v0", "make": "Honda", "model": "Super Cub", "odometer_value": 0 },
                { "id": "v1", "make": "Yamaha", "model": "MT-07", "year": 2023, "plate_number": "AB-123-CD", "odometer_value": 24_680, "odometer_unit": "km" },
            ],
        }))
        .unwrap()
    }

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 26, 9, 0, 0).unwrap()
    }

    #[test]
    fn an_invoice_carries_its_customer_its_lines_and_the_legal_wording() {
        let shop = json!({ "shop": { "name": "Garage Demo" } });
        let profile = ProfileCheck {
            payment_terms_text: "Paiement à 45 jours. Pénalités de retard…".into(),
            profile: CompanyProfile { payment_terms_days: 45, ..Default::default() },
            ..Default::default()
        };
        let customer = customer();
        let data = document_data(&sale(json!({})), &DocumentContext { shop: &shop, client: Some(&customer), profile: Some(&profile) }, now());

        assert_eq!(data["shop"]["name"], "Garage Demo");
        assert_eq!(
            data["client"],
            json!({ "first_name": "Léa", "last_name": "Martin", "full_name": "Léa Martin", "address": "12 rue des Lilas\n35000 Rennes", "phone": "06 12 34 56 78", "email": "" })
        );
        assert_eq!(
            data["vehicle"],
            json!({ "label": "Yamaha MT-07 (2023)", "plate_number": "AB-123-CD", "vin_or_serial": "", "odometer": "24\u{a0}680\u{a0}km" })
        );

        // A shelf price prints what the customer pays; a line priced HT has its discount taken off.
        assert_eq!(
            (&data["items"][0]["total_ttc"], &data["items"][0]["price_ttc"], &data["items"][0]["qty"]),
            (&json!(1_998), &json!(999), &json!(2.0))
        );
        assert_eq!(
            (&data["items"][1]["total_ht"], &data["items"][1]["total_ttc"], &data["items"][1]["discount"]),
            (&json!(8_100), &json!(9_720), &json!(1000))
        );

        let doc = &data["doc"];
        assert_eq!((&doc["number"], &doc["title"], &doc["kind"]), (&json!("FA-2026-C1-00007"), &json!("FACTURE"), &json!("invoice")));
        // Dated from when it was sealed, due as many days later as the shop gives.
        assert_eq!((&doc["date"], &doc["due_date"]), (&json!("2026-11-05T10:15:00+00:00"), &json!("2026-12-20T10:15:00Z")));
        assert_eq!(
            (&doc["total_ht"], &doc["total_tva"], &doc["total_ttc"], &doc["paid"], &doc["outstanding"]),
            (&json!(9_765), &json!(1_953), &json!(11_718), &json!(5_000), &json!(6_718))
        );
        assert_eq!(
            (&doc["payment_terms"], &doc["vat_exemption"], &doc["notes"]),
            (&json!("Paiement à 45 jours. Pénalités de retard…"), &json!(""), &json!("Révision annuelle"))
        );
        assert_eq!(data["payments"], json!([{ "date": "2026-11-05T10:20:00+00:00", "method": "Carte bancaire", "amount": 5_000 }]));
    }

    #[test]
    fn a_document_without_customer_or_profile_still_prints() {
        let shop = json!({});
        let quote = sale(
            json!({ "status": "quote", "document_kind": "quote", "reference": null, "locked_at": null, "sale_date": null, "client_id": null, "vehicle_id": null }),
        );
        let data = document_data(&quote, &DocumentContext { shop: &shop, client: None, profile: None }, now());
        assert_eq!((&data["client"]["full_name"], &data["client"]["last_name"]), (&json!("Client de passage"), &json!("")));
        assert_eq!(data["vehicle"]["label"], "");
        assert_eq!(
            (&data["doc"]["title"], &data["doc"]["number"], &data["doc"]["payment_terms"]),
            (&json!("DEVIS"), &json!(""), &json!("Paiement à 30 jours."))
        );
        // Not dated yet: it is dated from now.
        assert!(data["doc"]["date"].as_str().unwrap().starts_with("2026-11-26T09:00:00"));
        assert!(data["doc"]["due_date"].as_str().unwrap().starts_with("2026-12-26T"));
    }

    #[test]
    fn each_kind_of_document_has_its_title_template_and_exemption() {
        assert_eq!((document_title(&sale(json!({}))), template_kind_for(&sale(json!({})))), ("FACTURE", TemplateKind::Facture));
        let credit = sale(json!({ "status": "credit_note", "document_kind": "credit_note" }));
        assert_eq!((document_title(&credit), template_kind_for(&credit)), ("AVOIR", TemplateKind::Facture));
        let receipt = sale(json!({ "status": "receipt", "document_kind": "receipt" }));
        assert_eq!(document_title(&receipt), "TICKET");
        let quote = sale(json!({ "status": "quote", "document_kind": "quote" }));
        assert_eq!((document_title(&quote), template_kind_for(&quote)), ("DEVIS", TemplateKind::Devis));

        // A shop not subject to VAT says under which article.
        let exempt = ProfileCheck {
            profile: CompanyProfile { vat_subject: false, vat_exemption_text: "TVA non applicable, art. 293 B du CGI".into(), ..Default::default() },
            ..Default::default()
        };
        let data = document_data(&sale(json!({})), &DocumentContext { shop: &json!({}), client: None, profile: Some(&exempt) }, now());
        assert_eq!(data["doc"]["vat_exemption"], "TVA non applicable, art. 293 B du CGI");
    }

    #[test]
    fn a_due_date_is_written_the_way_the_date_is() {
        assert_eq!(days_after("2026-11-05", 30), "2026-12-05");
        assert_eq!(days_after("2026-11-05T09:30:00", 30), "2026-12-05T09:30:00");
        assert_eq!(days_after("2026-12-20T10:15:00Z", 15), "2027-01-04T10:15:00Z");
        assert_eq!(days_after("bientôt", 30), "");
    }
}
