//! Complete sample documents, to look at a template with realistic content before it prints a
//! real one, and the block that says who the shop is.
//!
//! The shop's own identity is used where it is filled in; a shop that has not said who it is
//! yet still gets a full page, from a made-up garage.

use chrono::{Duration, NaiveDate};
use serde_json::{json, Map, Value};

use super::TemplateKind;
use crate::company::Company;
use crate::settings::AppSetting;

/// Where a template finds the shop's logo. The picture itself is handed to whatever draws the
/// page, under this name.
pub const LOGO_SOURCE: &str = "motodesk:logo";

/// Who issues the documents: the shop's names, address, registration numbers and bank, as the
/// settings hold them.
pub fn shop_block(company: Option<&Company>, settings: &[AppSetting]) -> Value {
    let setting = |key: &str| settings.iter().find(|setting| setting.key == key).map(|setting| setting.value.clone()).unwrap_or(Value::Null);
    // « Afficher le logo sur les documents » turned off means documents carry none.
    let show_logo = setting("invoicing.show_logo") != Value::Bool(false);
    let has_logo = company.is_some_and(|company| company.has_logo);
    json!({
        "shop": {
            "name": company.map(Company::name).unwrap_or_default(),
            "legal_name": company.map(Company::legal_name).unwrap_or_default(),
            "tagline": setting("shop.tagline"),
            "logo_url": if show_logo && has_logo { LOGO_SOURCE } else { "" },
        },
        "contact": {
            "address_line1": setting("contact.address_line1"),
            "address_line2": setting("contact.address_line2"),
            "postal_code": setting("contact.postal_code"),
            "city": setting("contact.city"),
            "country": setting("contact.country"),
            "phone": setting("contact.phone"),
            "email": setting("contact.email"),
            "website": setting("contact.website"),
        },
        "legal": {
            "siret": setting("legal.siret"),
            "siren": setting("legal.siren"),
            "vat_number": setting("legal.vat_number"),
            "ape_code": setting("legal.ape_code"),
            "legal_form": setting("legal.legal_form"),
            "rcs_city": setting("legal.rcs_city"),
            "capital_eur": setting("legal.capital_eur"),
        },
        "invoicing": {
            "bank_name": setting("invoicing.bank_name"),
            "bank_iban": setting("invoicing.bank_iban"),
            "bank_bic": setting("invoicing.bank_bic"),
            "footer_text": setting("invoicing.footer_text"),
        },
    })
}

/// The number of days a customer has to pay, from the settings (30 when it is not set).
pub fn payment_terms_days(settings: &[AppSetting]) -> i64 {
    settings.iter().find(|setting| setting.key == "invoicing.payment_terms_days").and_then(|setting| setting.value.as_i64()).unwrap_or(30)
}

/// A made-up garage, complete.
pub fn demo_shop() -> Value {
    json!({
        "shop": { "name": "Garage Martin Moto", "legal_name": "Garage Martin SARL", "tagline": "Atelier moto · cycles · électrique", "logo_url": "" },
        "contact": {
            "address_line1": "18 rue de la Gare", "address_line2": "Zone artisanale des Ateliers", "postal_code": "35000", "city": "Rennes",
            "country": "France", "phone": "02 99 41 28 17", "email": "contact@garage-martin.fr", "website": "https://garage-martin.fr",
        },
        "legal": {
            "siret": "848 392 104 00027", "siren": "848 392 104", "vat_number": "FR84848392104", "ape_code": "4540Z",
            "legal_form": "SARL", "rcs_city": "Rennes", "capital_eur": 8000,
        },
        "invoicing": {
            "bank_name": "Crédit Agricole Ille-et-Vilaine", "bank_iban": "FR76 1548 9000 1234 5678 9012 345", "bank_bic": "AGRIFRPP882",
            "footer_text": "SARL au capital de 8 000 € — RCS Rennes 848 392 104. TVA acquittée sur les débits.",
        },
    })
}

fn is_blank(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null)) || value.and_then(Value::as_str) == Some("")
}

/// `live`, with what it leaves empty taken from `demo`, block by block.
pub fn fill_empty(live: &Value, demo: &Value) -> Value {
    let mut out = live.as_object().cloned().unwrap_or_default();
    for (key, sample) in demo.as_object().into_iter().flatten() {
        if sample.is_object() {
            let current = out.get(key).filter(|current| current.is_object()).cloned().unwrap_or_else(|| json!({}));
            out.insert(key.clone(), fill_empty(&current, sample));
        } else if is_blank(out.get(key)) {
            out.insert(key.clone(), sample.clone());
        }
    }
    Value::Object(out)
}

fn item(label: &str, qty: f64, unit: &str, price_ht: i64, total_ht: i64, total_ttc: i64) -> Value {
    json!({ "label": label, "qty": qty, "unit": unit, "price_ht": price_ht, "vat_rate": 2000, "total_ht": total_ht, "total_ttc": total_ttc })
}

fn workshop_items() -> Vec<Value> {
    vec![
        item("Révision 24 000 km — forfait atelier", 1.0, "forf.", 8900, 8900, 10680),
        item("Huile moteur Yamalube 10W-40", 3.2, "L", 1450, 4640, 5568),
        item("Filtre à huile HF204", 1.0, "pce", 1280, 1280, 1536),
        item("Filtre à air d'origine", 1.0, "pce", 2190, 2190, 2628),
        item("Plaquettes de frein avant sintered", 1.0, "jeu", 4850, 4850, 5820),
        item("Liquide de frein DOT 4", 1.0, "pce", 980, 980, 1176),
        item("Main d'œuvre mécanique", 1.5, "h", 5800, 8700, 10440),
    ]
}

fn quote_items() -> Vec<Value> {
    vec![
        item("Kit cylindre-piston Ø 68 mm haute compression", 1.0, "kit", 18900, 18900, 22680),
        item("Joint de culasse + kit joints moteur", 1.0, "kit", 6400, 6400, 7680),
        item("Huile moteur 10W-50 racing (5 L)", 1.0, "bidon", 4200, 4200, 5040),
        item("Main d'œuvre dépose / repose moteur", 6.0, "h", 5800, 34800, 41760),
        item("Rodage + essai route", 1.0, "forf.", 4500, 4500, 5400),
    ]
}

fn delivery_items() -> Vec<Value> {
    vec![
        item("Pneu avant Michelin Pilot Road 6 120/70 ZR17", 1.0, "pce", 12900, 12900, 15480),
        item("Pneu arrière Michelin Pilot Road 6 180/55 ZR17", 1.0, "pce", 16400, 16400, 19680),
        item("Valve aluminium coudée", 2.0, "pce", 450, 900, 1080),
        item("Équilibrage + montage", 1.0, "forf.", 2800, 2800, 3360),
    ]
}

fn vehicle(label: &str, plate: &str, vin: &str, odometer: &str) -> Value {
    json!({ "label": label, "plate_number": plate, "vin_or_serial": vin, "odometer": odometer })
}

/// A whole document of this kind: the shop (`shop`, as [`shop_block`] or [`demo_shop`] gives
/// it), a customer, vehicles, a product, lines, totals and the legal wording.
pub fn sample_data(kind: TemplateKind, shop: &Value, payment_terms_days: i64) -> Value {
    let items = match kind {
        TemplateKind::Devis => quote_items(),
        TemplateKind::BonLivraison => delivery_items(),
        _ => workshop_items(),
    };
    let sum = |field: &str| items.iter().filter_map(|item| item[field].as_i64()).sum::<i64>();
    let (total_ht, total_ttc) = (sum("total_ht"), sum("total_ttc"));
    let today = NaiveDate::from_ymd_opt(2026, 5, 29).unwrap_or_default();
    let due = today + Duration::days(payment_terms_days);
    let (title, prefix) = match kind {
        TemplateKind::Facture => ("FACTURE", "FA"),
        TemplateKind::Devis => ("DEVIS", "DE"),
        TemplateKind::BonLivraison => ("BON DE LIVRAISON", "BL"),
        TemplateKind::Autre => ("DOCUMENT", "DOC"),
    };
    let notes = match kind {
        TemplateKind::Devis => "Devis valable 30 jours. Acompte de 30 % à la commande. Délai d'immobilisation estimé : 4 jours ouvrés.",
        TemplateKind::BonLivraison => "Livraison magasin — palettes à réceptionner avant 12 h. Contrôle visuel à la livraison.",
        TemplateKind::Autre => "Cliente fidèle depuis 2019. Préfère les rendez-vous le mardi matin. Facturation à 30 jours.",
        TemplateKind::Facture => "Révision des 24 000 km. RAS transmission. Prochain contrôle : plaquettes arrière vers 28 000 km.",
    };
    let unpaid = kind == TemplateKind::Facture;
    let date = format!("{}T09:30:00", today.format("%Y-%m-%d"));

    let mut data: Map<String, Value> = shop.as_object().cloned().unwrap_or_default();
    data.insert(
        "client".into(),
        json!({
            "first_name": "Léa", "last_name": "Martin", "full_name": "Léa Martin",
            "address": "12 rue des Lilas\n35000 Rennes", "phone": "06 12 34 56 78", "email": "lea.martin@example.fr",
        }),
    );
    data.insert("vehicle".into(), vehicle("Yamaha MT-07 (2023)", "AB-123-CD", "JYARM34E8PA000001", "24 680 km"));
    data.insert(
        "vehicles".into(),
        json!([
            vehicle("Yamaha MT-07 (2023)", "AB-123-CD", "JYARM34E8PA000001", "24 680 km"),
            vehicle("Honda Super Cub 125 (2019)", "CD-456-EF", "MLHJA3310K5000123", "8 140 km"),
            vehicle("Specialized Turbo Vado 4.0", "—", "WSBC6142123456789", "1 920 km"),
        ]),
    );
    data.insert(
        "product".into(),
        json!({
            "name": "Pneu avant Michelin Pilot Road 6 120/70 ZR17", "sku": "MICH-PR6-12070", "brand": "Michelin", "category": "Consommables",
            "unit": "pièce",
            "description": "Pneu sport-touring homologué. Sculpture 2CT+ pour un grip à froid et une usure régulière. Pression recommandée 2,5 bar à froid. Montage et équilibrage en atelier.",
            "notes": "Référence fournisseur : MIC-024189. Délai réappro 3 jours. Compatible MT-07 / Tracer 7 / Street Triple.",
            "price_ht": 12900, "price_ttc": 15480, "vat_rate": 2000, "stock": 6, "low_stock": 2,
        }),
    );
    data.insert("items".into(), Value::Array(items));
    data.insert(
        "doc".into(),
        json!({
            "title": title,
            "number": format!("{prefix}-2026-C1-00042"),
            "kind": kind.id(),
            "date": date,
            "due_date": format!("{}T09:30:00", due.format("%Y-%m-%d")),
            "notes": notes,
            "total_ht": total_ht,
            "total_tva": total_ttc - total_ht,
            "total_ttc": total_ttc,
            "paid": if unpaid { 0 } else { total_ttc },
            "outstanding": if unpaid { total_ttc } else { 0 },
            "payment_terms": "Paiement à 30 jours. En cas de retard, pénalités égales à 3 fois le taux d'intérêt légal et indemnité forfaitaire de 40 € (C. com. art. L441-10 et D441-5).",
            "vat_exemption": "",
        }),
    );
    data.insert("payments".into(), json!([{ "date": date, "method": "Carte bancaire", "amount": if unpaid { 0 } else { total_ttc } }]));
    Value::Object(data)
}

/// The sample document of this kind for this shop: its own identity where it is filled in.
pub fn sample_for(kind: TemplateKind, company: Option<&Company>, settings: &[AppSetting]) -> Value {
    let shop = fill_empty(&shop_block(company, settings), &demo_shop());
    sample_data(kind, &shop, payment_terms_days(settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(key: &str, value: Value) -> AppSetting {
        serde_json::from_value(json!({ "key": key, "value": value, "updated_at": "2026-11-26T10:00:00Z" })).expect("a setting")
    }

    #[test]
    fn what_the_shop_left_empty_comes_from_the_sample_garage() {
        let settings = [setting("contact.city", json!("Brest")), setting("contact.phone", json!("")), setting("invoicing.payment_terms_days", json!(45))];
        let data = sample_for(TemplateKind::Facture, None, &settings);
        assert_eq!(data["contact"]["city"], "Brest");
        assert_eq!(data["contact"]["phone"], "02 99 41 28 17");
        assert_eq!(data["shop"]["name"], "Garage Martin Moto");
        assert_eq!(data["shop"]["logo_url"], "", "no logo is made up");
        assert_eq!(data["doc"]["due_date"], "2026-07-13T09:30:00", "45 days after the 29th of May");
        assert_eq!(payment_terms_days(&[]), 30);
    }

    #[test]
    fn the_totals_of_a_sample_are_the_sum_of_its_lines() {
        for kind in TemplateKind::ALL {
            let data = sample_data(kind, &demo_shop(), 30);
            let items = data["items"].as_array().unwrap();
            let sum = |field: &str| items.iter().map(|item| item[field].as_i64().unwrap()).sum::<i64>();
            assert_eq!(data["doc"]["total_ht"], sum("total_ht"), "{kind:?}");
            assert_eq!(data["doc"]["total_ttc"], sum("total_ttc"), "{kind:?}");
            assert_eq!(data["doc"]["total_tva"].as_i64().unwrap(), sum("total_ttc") - sum("total_ht"));
        }
        let invoice = sample_data(TemplateKind::Facture, &demo_shop(), 30);
        assert_eq!((invoice["doc"]["title"].as_str(), invoice["doc"]["number"].as_str()), (Some("FACTURE"), Some("FA-2026-C1-00042")));
        assert_eq!(invoice["doc"]["outstanding"], invoice["doc"]["total_ttc"]);
        assert_eq!(sample_data(TemplateKind::Devis, &demo_shop(), 30)["doc"]["outstanding"], 0);
        assert_eq!(invoice["vehicles"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn the_logo_is_named_only_when_there_is_one_to_show() {
        let company: Company = serde_json::from_value(json!({ "name": "Atelier", "legal_name": "Atelier SARL", "has_logo": true })).expect("a company");
        assert_eq!(shop_block(Some(&company), &[])["shop"]["logo_url"], LOGO_SOURCE);
        assert_eq!(shop_block(Some(&company), &[setting("invoicing.show_logo", json!(false))])["shop"]["logo_url"], "");
        assert_eq!(shop_block(None, &[])["shop"]["logo_url"], "");
        assert_eq!(shop_block(Some(&company), &[])["shop"]["legal_name"], "Atelier SARL");
    }
}
