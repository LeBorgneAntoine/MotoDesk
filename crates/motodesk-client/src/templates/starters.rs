//! The layouts that come with the app: an invoice, a quote, a delivery note, a customer
//! sheet and a product sheet.
//!
//! They are templates like any other, so a shop prints a complete document from the first day
//! and then duplicates or adjusts one rather than starting from a blank page.

use super::schema::{Align, Element, Format, Kind, Layout, Page, RectEl, TableColumn, TableEl, TextEl, Weight};
use super::{create, list, set_default, ListQuery, Orientation, TemplateInput, TemplateKind, TemplateSummary};
use crate::error::ApiResult;
use crate::session::Client;

const INK: &str = "#1a1a1a";
const MUTED: &str = "#5c6166";
const NAVY: &str = "#1e3a5f";
const NAVY_SOFT: &str = "#eef2f6";
const RULE: &str = "#d0d5db";
const WHITE: &str = "#ffffff";

pub struct Starter {
    pub name: &'static str,
    pub kind: TemplateKind,
    /// Becomes the default of its kind when that kind has none yet.
    pub make_default: bool,
    pub build: fn() -> Layout,
}

pub const CATALOG: [Starter; 5] = [
    Starter { name: "Facture atelier", kind: TemplateKind::Facture, make_default: true, build: || commercial(Variant::Invoice) },
    Starter { name: "Devis atelier", kind: TemplateKind::Devis, make_default: true, build: || commercial(Variant::Quote) },
    Starter { name: "Bon de livraison", kind: TemplateKind::BonLivraison, make_default: true, build: || commercial(Variant::Delivery) },
    Starter { name: "Fiche client", kind: TemplateKind::Autre, make_default: true, build: client_sheet },
    Starter { name: "Fiche produit", kind: TemplateKind::Autre, make_default: false, build: product_sheet },
];

/// The layout a new template of this kind starts from.
pub fn starter_layout(kind: TemplateKind) -> Layout {
    let starter = CATALOG.iter().find(|starter| starter.kind == kind && starter.make_default).unwrap_or(&CATALOG[0]);
    (starter.build)()
}

/// The starters the library does not have yet (same name, same kind).
pub fn missing(existing: &[TemplateSummary]) -> Vec<&'static Starter> {
    CATALOG.iter().filter(|starter| !existing.iter().any(|row| row.name == starter.name && row.kind == starter.kind)).collect()
}

/// Create every starter the library does not have, archived templates included in « has ».
/// Answers how many were created. Templates that are already there are left alone.
pub async fn install_missing(client: &Client) -> ApiResult<usize> {
    let existing = list(client, &ListQuery { kind: None, include_archived: true }).await?;
    let mut with_default: Vec<TemplateKind> = existing.iter().filter(|row| row.is_default && !row.is_archived).map(|row| row.kind).collect();
    let mut created = 0;
    for starter in missing(&existing) {
        let input = TemplateInput {
            name: starter.name.to_string(),
            kind: starter.kind,
            page_size: Some("A4".to_string()),
            orientation: Some(Orientation::Portrait),
            schema: (starter.build)(),
            is_default: Some(false),
            is_archived: Some(false),
        };
        let row = create(client, &input).await?;
        created += 1;
        if starter.make_default && !with_default.contains(&starter.kind) {
            // Not being the default is a detail the user can settle later; the layout is saved.
            if set_default(client, &row.id).await.is_ok() {
                with_default.push(starter.kind);
            }
        }
    }
    Ok(created)
}

// ---- building blocks ---------------------------------------------------------------------------

struct Style {
    size: f64,
    align: Align,
    bold: bool,
    color: &'static str,
}

const BODY: Style = Style { size: 9.0, align: Align::Left, bold: false, color: INK };

impl Style {
    const fn size(self, size: f64) -> Self {
        Style { size, ..self }
    }

    const fn bold(self) -> Self {
        Style { bold: true, ..self }
    }

    const fn right(self) -> Self {
        Style { align: Align::Right, ..self }
    }

    const fn color(self, color: &'static str) -> Self {
        Style { color, ..self }
    }
}

fn text(id: &str, x: f64, y: f64, w: f64, h: f64, content: &str, style: Style) -> Element {
    let kind = Kind::Text(TextEl {
        content: content.to_string(),
        font_size: style.size,
        align: style.align,
        font_weight: if style.bold { Weight::Bold } else { Weight::Normal },
        color: style.color.to_string(),
    });
    Element { id: id.to_string(), x, y, w, h, kind }
}

fn rect(id: &str, x: f64, y: f64, w: f64, h: f64, (stroke, stroke_width, fill): (&str, f64, &str)) -> Element {
    Element { id: id.to_string(), x, y, w, h, kind: Kind::Rect(RectEl { stroke: stroke.to_string(), stroke_width, fill: fill.to_string() }) }
}

/// A frame with the standard thin rule.
fn frame(id: &str, x: f64, y: f64, w: f64, h: f64, fill: &str) -> Element {
    rect(id, x, y, w, h, (RULE, 0.25, fill))
}

fn column(header: &str, binding: &str, width_pct: f64, format: Format, align: Align) -> TableColumn {
    TableColumn { header: header.to_string(), binding: binding.to_string(), width_pct, format: Some(format), align: Some(align) }
}

fn wrap(elements: Vec<Element>) -> Layout {
    Layout { version: 1, page: Page { margin: 12.0, ..Page::A4_PORTRAIT }, elements }
}

/// The band at the top of every page, with who the shop is.
fn masthead(prefix: &str, title: &str) -> Vec<Element> {
    vec![
        rect(&format!("{prefix}_band"), 0.0, 0.0, 210.0, 22.0, (NAVY, 0.0, NAVY)),
        text(&format!("{prefix}_shop"), 12.0, 5.0, 118.0, 12.0, "{{shop.name}}", BODY.size(16.0).bold().color(WHITE)),
        text(&format!("{prefix}_title"), 128.0, 5.0, 70.0, 12.0, title, BODY.size(16.0).bold().color(WHITE).right()),
        text(
            &format!("{prefix}_addr"),
            12.0,
            26.0,
            110.0,
            28.0,
            "{{shop.legal_name}}\n{{contact.address_line1}}\n{{contact.postal_code}} {{contact.city}}\nTél. {{contact.phone}}  ·  {{contact.email}}",
            BODY.size(8.0).color(MUTED),
        ),
        text(
            &format!("{prefix}_legal"),
            12.0,
            54.0,
            110.0,
            10.0,
            "SIRET {{legal.siret}}  ·  TVA {{legal.vat_number}}  ·  APE {{legal.ape_code}}",
            BODY.size(7.0).color(MUTED),
        ),
    ]
}

/// The foot of a commercial document: payment terms, bank, legal wording.
fn footer(prefix: &str) -> Vec<Element> {
    let small = || BODY.size(7.0).color(MUTED);
    vec![
        rect(&format!("{prefix}_foot_rule"), 12.0, 262.0, 186.0, 0.4, (RULE, 0.0, RULE)),
        text(&format!("{prefix}_terms"), 12.0, 265.0, 186.0, 10.0, "{{doc.payment_terms}}", small()),
        text(
            &format!("{prefix}_bank"),
            12.0,
            275.0,
            186.0,
            8.0,
            "{{invoicing.bank_name}}  ·  IBAN {{invoicing.bank_iban}}  ·  BIC {{invoicing.bank_bic}}",
            small(),
        ),
        text(&format!("{prefix}_mentions"), 12.0, 283.0, 186.0, 8.0, "{{invoicing.footer_text}}", small()),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Invoice,
    Quote,
    Delivery,
}

fn commercial(variant: Variant) -> Layout {
    let (p, due_label, client_label) = match variant {
        Variant::Invoice => ("invoice", "Échéance", "CLIENT"),
        Variant::Quote => ("quote", "Valable jusqu'au", "CLIENT"),
        Variant::Delivery => ("delivery", "Livraison", "LIVRÉ À"),
    };
    let id = |name: &str| format!("{p}_{name}");
    let label = || BODY.size(7.0).color(MUTED);
    let mut elements = masthead(p, "{{doc.title}}");
    elements.extend([
        frame(&id("meta_bg"), 128.0, 26.0, 70.0, 38.0, NAVY_SOFT),
        text(&id("meta_num_l"), 132.0, 29.0, 22.0, 6.0, "N°", label()),
        text(&id("meta_num"), 154.0, 29.0, 40.0, 6.0, "{{doc.number}}", BODY.bold().right()),
        text(&id("meta_date_l"), 132.0, 37.0, 22.0, 6.0, "Date", label()),
        text(&id("meta_date"), 154.0, 37.0, 40.0, 6.0, "{{doc.date | date}}", BODY.right()),
        text(&id("meta_due_l"), 132.0, 45.0, 28.0, 6.0, due_label, label()),
        text(&id("meta_due"), 154.0, 45.0, 40.0, 6.0, "{{doc.due_date | date}}", BODY.right()),
        text(&id("meta_ref_l"), 132.0, 53.0, 22.0, 6.0, "Réf.", label()),
        text(&id("meta_ref"), 154.0, 53.0, 40.0, 6.0, "{{vehicle.plate_number}}", BODY.size(8.0).right()),
        frame(&id("client_box"), 12.0, 68.0, 186.0, 28.0, WHITE),
        text(&id("client_l"), 15.0, 70.0, 40.0, 5.0, client_label, label().bold()),
        text(&id("client_name"), 15.0, 76.0, 100.0, 7.0, "{{client.full_name}}", BODY.size(11.0).bold()),
        text(&id("client_addr"), 15.0, 83.0, 100.0, 11.0, "{{client.address}}\n{{client.phone}}  ·  {{client.email}}", BODY.size(8.0).color(MUTED)),
        text(
            &id("vehicle"),
            120.0,
            76.0,
            74.0,
            16.0,
            "{{vehicle.label}}\n{{vehicle.plate_number}}  ·  {{vehicle.odometer}}\nVIN {{vehicle.vin_or_serial}}",
            BODY.size(8.0).right(),
        ),
        Element {
            id: id("items"),
            x: 12.0,
            y: 100.0,
            w: 186.0,
            h: 92.0,
            kind: Kind::Table(TableEl {
                list_binding: "items".to_string(),
                columns: vec![
                    column("Désignation", "label", 42.0, Format::Text, Align::Left),
                    column("Qté", "qty", 10.0, Format::Qty, Align::Right),
                    column("Unité", "unit", 10.0, Format::Text, Align::Center),
                    column("PU HT", "price_ht", 14.0, Format::Price, Align::Right),
                    column("TVA", "vat_rate", 10.0, Format::Percent, Align::Right),
                    column("Total TTC", "total_ttc", 14.0, Format::Price, Align::Right),
                ],
                row_height: 7.0,
                header_height: 8.0,
                font_size: 8.0,
            }),
        },
        frame(&id("tot_bg"), 128.0, 196.0, 70.0, 36.0, NAVY_SOFT),
        text(&id("tot_ht_l"), 132.0, 199.0, 30.0, 7.0, "Total HT", BODY.size(8.0).color(MUTED)),
        text(&id("tot_ht"), 158.0, 199.0, 36.0, 7.0, "{{doc.total_ht | price}}", BODY.right()),
        text(&id("tot_tva_l"), 132.0, 207.0, 30.0, 7.0, "TVA", BODY.size(8.0).color(MUTED)),
        text(&id("tot_tva"), 158.0, 207.0, 36.0, 7.0, "{{doc.total_tva | price}}", BODY.right()),
        text(&id("tot_ttc_l"), 132.0, 216.0, 30.0, 10.0, "Total TTC", BODY.size(10.0).bold()),
        text(&id("tot_ttc"), 158.0, 216.0, 36.0, 10.0, "{{doc.total_ttc | price}}", BODY.size(11.0).bold().right()),
        text(&id("notes_l"), 12.0, 196.0, 50.0, 5.0, "NOTES", label().bold()),
        text(&id("notes"), 12.0, 202.0, 110.0, 22.0, "{{doc.notes}}", BODY.size(8.0)),
    ]);
    elements.extend(footer(p));
    match variant {
        Variant::Invoice => elements.extend([
            text(&id("due_amt_l"), 132.0, 226.0, 30.0, 6.0, "Reste dû", label()),
            text(&id("due_amt"), 158.0, 226.0, 36.0, 6.0, "{{doc.outstanding | price}}", BODY.size(8.0).bold().right()),
        ]),
        Variant::Delivery => elements.push(text(
            &id("sign"),
            12.0,
            248.0,
            186.0,
            12.0,
            "Marchandise reçue en bon état. Date : ____________    Signature du destinataire : ________________",
            BODY.size(8.0).color(MUTED),
        )),
        Variant::Quote => {}
    }
    wrap(elements)
}

fn client_sheet() -> Layout {
    let section = || BODY.size(8.0).bold().color(MUTED);
    let mut elements = masthead("cli", "FICHE CLIENT");
    elements.extend([
        frame("cli_name_bg", 12.0, 68.0, 186.0, 22.0, NAVY_SOFT),
        text("cli_name", 16.0, 72.0, 178.0, 14.0, "{{client.full_name}}", BODY.size(18.0).bold()),
        text("cli_sec_contact", 12.0, 96.0, 90.0, 6.0, "COORDONNÉES", section()),
        text("cli_contact", 12.0, 104.0, 90.0, 28.0, "{{client.address}}\n\nTél. {{client.phone}}\n{{client.email}}", BODY),
        text("cli_sec_shop", 118.0, 96.0, 80.0, 6.0, "SUIVI ATELIER", section()),
        text("cli_shop_notes", 118.0, 104.0, 80.0, 28.0, "{{doc.notes}}", BODY.size(8.0).color(MUTED)),
        text("cli_sec_veh", 12.0, 138.0, 90.0, 6.0, "VÉHICULES", section()),
        Element {
            id: "cli_vehicles".to_string(),
            x: 12.0,
            y: 146.0,
            w: 186.0,
            h: 90.0,
            kind: Kind::Table(TableEl {
                list_binding: "vehicles".to_string(),
                columns: vec![
                    column("Véhicule", "label", 38.0, Format::Text, Align::Left),
                    column("Immatriculation", "plate_number", 18.0, Format::Text, Align::Left),
                    column("VIN / n° série", "vin_or_serial", 28.0, Format::Text, Align::Left),
                    column("Compteur", "odometer", 16.0, Format::Text, Align::Right),
                ],
                row_height: 8.0,
                header_height: 8.0,
                font_size: 8.0,
            }),
        },
    ]);
    elements.extend(footer("cli"));
    wrap(elements)
}

fn product_sheet() -> Layout {
    let section = || BODY.size(8.0).bold().color(MUTED);
    let mut elements = masthead("prd", "FICHE PRODUIT");
    elements.extend([
        frame("prd_name_bg", 12.0, 68.0, 186.0, 22.0, NAVY_SOFT),
        text("prd_name", 16.0, 72.0, 140.0, 14.0, "{{product.name}}", BODY.size(16.0).bold()),
        text("prd_sku", 150.0, 74.0, 44.0, 10.0, "{{product.sku}}", BODY.size(10.0).right().color(MUTED)),
        frame("prd_price_bg", 128.0, 96.0, 70.0, 42.0, WHITE),
        text("prd_price_l", 132.0, 100.0, 62.0, 6.0, "PRIX DE VENTE TTC", BODY.size(7.0).bold().color(MUTED)),
        text("prd_price", 132.0, 108.0, 62.0, 14.0, "{{product.price_ttc | price}}", BODY.size(18.0).bold()),
        text("prd_vat", 132.0, 124.0, 62.0, 8.0, "TVA {{product.vat_rate | percent}}  ·  HT {{product.price_ht | price}}", BODY.size(8.0).color(MUTED)),
        text("prd_sec_id", 12.0, 96.0, 110.0, 6.0, "IDENTIFICATION", section()),
        text(
            "prd_id",
            12.0,
            104.0,
            110.0,
            28.0,
            "Marque  {{product.brand}}\nCatégorie  {{product.category}}\nUnité  {{product.unit}}\nStock  {{product.stock}}  (seuil {{product.low_stock}})",
            BODY,
        ),
        text("prd_sec_desc", 12.0, 144.0, 90.0, 6.0, "DESCRIPTION", section()),
        text("prd_desc", 12.0, 152.0, 186.0, 36.0, "{{product.description}}", BODY),
        text("prd_sec_notes", 12.0, 192.0, 90.0, 6.0, "NOTES ATELIER", section()),
        text("prd_notes", 12.0, 200.0, 186.0, 28.0, "{{product.notes}}", BODY.size(8.0).color(MUTED)),
    ]);
    elements.extend(footer("prd"));
    wrap(elements)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_starter_is_a_full_page_with_its_own_identifiers() {
        for starter in &CATALOG {
            let layout = (starter.build)();
            assert_eq!((layout.page.width, layout.page.height, layout.page.margin), (210.0, 297.0, 12.0), "{}", starter.name);
            assert!(layout.elements.len() >= 15, "{} has {} elements", starter.name, layout.elements.len());
            let ids: HashSet<&str> = layout.elements.iter().map(|element| element.id.as_str()).collect();
            assert_eq!(ids.len(), layout.elements.len(), "{}: an identifier is used twice", starter.name);
            for element in &layout.elements {
                assert!(element.x >= 0.0 && element.y >= 0.0 && element.x + element.w <= 210.0 && element.y + element.h <= 297.0, "{} leaves the page", element.id);
            }
        }
    }

    #[test]
    fn a_commercial_document_has_one_table_bound_to_its_lines() {
        for kind in [TemplateKind::Facture, TemplateKind::Devis, TemplateKind::BonLivraison] {
            let layout = starter_layout(kind);
            let tables: Vec<&TableEl> = layout.elements.iter().filter_map(|e| if let Kind::Table(table) = &e.kind { Some(table) } else { None }).collect();
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].list_binding, "items");
            assert_eq!(tables[0].columns.iter().map(|c| c.width_pct).sum::<f64>(), 100.0);
        }
        // Only the invoice says what is left to pay; only the delivery note is signed.
        let has = |kind, id: &str| starter_layout(kind).element(id).is_some();
        assert!(has(TemplateKind::Facture, "invoice_due_amt") && !has(TemplateKind::Devis, "quote_due_amt"));
        assert!(has(TemplateKind::BonLivraison, "delivery_sign") && !has(TemplateKind::Facture, "invoice_sign"));
        // « Autre » starts from the customer sheet, which lists the vehicles.
        assert!(starter_layout(TemplateKind::Autre).element("cli_vehicles").is_some());
    }

    #[test]
    fn a_starter_already_there_is_not_installed_again() {
        let row = |name: &str, kind| TemplateSummary { id: name.into(), name: name.into(), kind, page_size: "A4".into(), orientation: Orientation::Portrait, is_default: false, is_archived: true, updated_at: String::new() };
        assert_eq!(missing(&[]).len(), 5);
        let have = [row("Facture atelier", TemplateKind::Facture), row("Fiche client", TemplateKind::Devis)];
        let names: Vec<&str> = missing(&have).iter().map(|starter| starter.name).collect();
        // Archived counts as there; the same name under another kind does not.
        assert_eq!(names, ["Devis atelier", "Bon de livraison", "Fiche client", "Fiche produit"]);
    }
}
