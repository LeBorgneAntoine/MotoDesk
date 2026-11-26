//! Document templates against the service's own handlers: the library, its defaults, the
//! starter templates a new shop is given, and a sale printed with one.

use motodesk_client::clients::{self, ClientInput};
use motodesk_client::fiscal::{self, JournalQuery};
use motodesk_client::local_account::{self, SetupPayload};
use motodesk_client::sales::{self, DraftStatus, SaleInput, SaleLineInput};
use motodesk_client::templates::print::{self, PrintError};
use motodesk_client::templates::schema::{Element, Kind, Layout};
use motodesk_client::templates::{self, starters, ListQuery, Orientation, TemplateInput, TemplateKind};
use motodesk_client::{company, Client, Cloud, Local, TokenStore};
use motodesk_server::local_server::{loopback_api, test_support};

/// A shop on a local network, its administrator signed in.
async fn shop() -> Client {
    let state = test_support::shop().await;
    let token = state.local_token.clone();
    let store = TokenStore::in_dir(&state.data_dir);
    let api = Client::new(Local::new(loopback_api(state), &token), Cloud::new("http://127.0.0.1:9"), store);
    let password = "mot de passe admin".to_string();
    let session = local_account::setup(&api, &SetupPayload { username: "camille".into(), password: password.clone(), confirm: password, force: true })
        .await
        .expect("the first administrator creates the network");
    api.set_tokens(Some(session.bundle));
    api
}

fn input(name: &str, kind: TemplateKind, schema: Layout) -> TemplateInput {
    TemplateInput { name: name.into(), kind, page_size: Some("A4".into()), orientation: Some(Orientation::Portrait), schema, is_default: None, is_archived: None }
}

fn names(rows: &[templates::TemplateSummary]) -> Vec<&str> {
    rows.iter().map(|row| row.name.as_str()).collect()
}

#[tokio::test]
async fn a_template_goes_through_its_whole_life() {
    let api = shop().await;
    let everything = ListQuery { kind: None, include_archived: true };
    assert!(templates::list(&api, &everything).await.unwrap().is_empty());

    // Created with a layout: it comes back as it was sent.
    let layout = Layout { elements: vec![Element::new_text(12.0, 12.0, 1), Element::new_table(12.0, 60.0, 1)], ..Layout::default() };
    let created = templates::create(&api, &input("Facture simple", TemplateKind::Facture, layout.clone())).await.unwrap();
    assert_eq!((created.name.as_str(), created.kind, created.is_default, created.is_archived), ("Facture simple", TemplateKind::Facture, false, false));
    assert_eq!(created.schema, layout);
    assert_eq!(templates::get(&api, &created.id).await.unwrap().schema, layout);

    // Changed: a new name, the page turned, an element moved.
    let mut changed = created.as_input();
    changed.name = "Facture paysage".into();
    changed.orientation = Some(Orientation::Landscape);
    changed.schema.set_landscape(true);
    changed.schema.elements[0].x = 40.5;
    let updated = templates::update(&api, &created.id, &changed).await.unwrap();
    assert_eq!((updated.name.as_str(), updated.orientation), ("Facture paysage", Orientation::Landscape));
    assert_eq!((updated.schema.page.width, updated.schema.elements[0].x), (297.0, 40.5));
    assert!(matches!(updated.schema.elements[1].kind, Kind::Table(_)));

    // A copy is a template of its own; the default of a kind is one template at a time.
    let copy = templates::duplicate(&api, &created.id).await.unwrap();
    assert_ne!(copy.id, created.id);
    assert_eq!(copy.schema, updated.schema);
    assert_eq!(templates::for_printing(&api, TemplateKind::Devis).await.unwrap(), None, "no quote template yet");
    templates::set_default(&api, &created.id).await.unwrap();
    templates::set_default(&api, &copy.id).await.unwrap();
    let invoices = templates::list(&api, &ListQuery { kind: Some(TemplateKind::Facture), include_archived: false }).await.unwrap();
    assert_eq!(invoices.iter().filter(|row| row.is_default).map(|row| row.id.as_str()).collect::<Vec<_>>(), [copy.id.as_str()]);
    assert_eq!(templates::for_printing(&api, TemplateKind::Facture).await.unwrap().map(|template| template.id), Some(copy.id.clone()));

    // Archived: out of the usual list, still there when asked for.
    let mut archived = updated.as_input();
    archived.is_archived = Some(true);
    templates::update(&api, &created.id, &archived).await.unwrap();
    assert_eq!(names(&templates::list(&api, &ListQuery::default()).await.unwrap()).len(), 1);
    assert_eq!(templates::list(&api, &everything).await.unwrap().len(), 2);

    templates::delete(&api, &created.id).await.unwrap();
    assert!(templates::get(&api, &created.id).await.unwrap_err().is_not_found());
    assert_eq!(templates::list(&api, &everything).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_new_shop_is_given_the_starter_templates_once() {
    let api = shop().await;
    assert_eq!(starters::install_missing(&api).await.unwrap(), 5);
    let library = templates::list(&api, &ListQuery::default()).await.unwrap();
    let mut listed = names(&library);
    listed.sort_unstable();
    assert_eq!(listed, ["Bon de livraison", "Devis atelier", "Facture atelier", "Fiche client", "Fiche produit"]);
    // One default per kind: the customer sheet is the one for « Autre », the product sheet is not.
    let defaults: Vec<&str> = library.iter().filter(|row| row.is_default).map(|row| row.name.as_str()).collect();
    assert_eq!(defaults.len(), 4);
    assert!(defaults.contains(&"Fiche client") && !defaults.contains(&"Fiche produit"));

    // Asked again, nothing is created; a starter that was deleted comes back, alone.
    assert_eq!(starters::install_missing(&api).await.unwrap(), 0);
    let quote = library.iter().find(|row| row.name == "Devis atelier").unwrap();
    templates::delete(&api, &quote.id).await.unwrap();
    assert_eq!(starters::install_missing(&api).await.unwrap(), 1);
    let quote = templates::for_printing(&api, TemplateKind::Devis).await.unwrap().expect("the quote template is back");
    assert_eq!((quote.name.as_str(), quote.is_default), ("Devis atelier", true));
    assert_eq!(quote.schema, starters::starter_layout(TemplateKind::Devis), "stored and read back unchanged");

    // An archived starter counts as there: it is not installed a second time.
    let invoice = templates::for_printing(&api, TemplateKind::Facture).await.unwrap().unwrap();
    let mut archived = invoice.as_input();
    archived.is_archived = Some(true);
    templates::update(&api, &invoice.id, &archived).await.unwrap();
    assert_eq!(starters::install_missing(&api).await.unwrap(), 0);
}

// ---- printing -----------------------------------------------------------------------------------

/// Whether the file writes `text` somewhere (ASCII only: the file holds it letter by letter).
fn pdf_says(file: &[u8], text: &str) -> bool {
    use std::io::Read as _;

    let wanted = format!("<{}> Tj", text.bytes().map(|byte| format!("{byte:02X}")).collect::<String>());
    let find = |from: usize, needle: &[u8]| file[from..].windows(needle.len()).position(|window| window == needle).map(|at| at + from);
    let mut at = 0;
    while let Some(start) = find(at, b"stream\n") {
        let end = find(start, b"\nendstream").expect("a stream ends");
        let mut plain = Vec::new();
        // A picture's stream is not text: whatever does not unpack is skipped.
        if flate2::read::ZlibDecoder::new(&file[start + 7..end]).read_to_end(&mut plain).is_ok() && String::from_utf8_lossy(&plain).contains(&wanted) {
            return true;
        }
        at = end + 10;
    }
    false
}

async fn prints_in_the_journal(api: &Client) -> Vec<String> {
    let entries = fiscal::journal(api, &JournalQuery::default()).await.unwrap();
    let mut prints: Vec<(i64, String)> =
        entries.into_iter().filter(|entry| entry.event_type.starts_with("document_")).map(|entry| (entry.seq, entry.event_type)).collect();
    prints.sort();
    prints.into_iter().map(|(_, event)| event).collect()
}

fn one_line(label: &str) -> Vec<SaleLineInput> {
    vec![SaleLineInput { label: Some(label.into()), quantity_milli: 1_000, unit_price_ht_cents: 10_000, vat_rate_bp: 2000, ..Default::default() }]
}

#[tokio::test]
async fn a_sealed_invoice_prints_as_the_original_then_as_copies() {
    let api = shop().await;
    let buyer = ClientInput { last_name: "Kervella".into(), address: Some("3 quai du Port".into()), ..Default::default() };
    let buyer = clients::create(&api, &buyer).await.unwrap();
    let input =
        SaleInput { client_id: Some(buyer.id.clone()), sale_date: Some("2026-11-05".into()), lines: one_line("Revision"), ..Default::default() };
    let draft = sales::create(&api, &input).await.unwrap();

    // A draft has no number: it is not printed.
    assert_eq!(print::prepare(&api, &draft.id).await.unwrap_err(), PrintError::Draft);
    let invoice = sales::validate(&api, &draft.id).await.unwrap();
    let number = invoice.reference.clone().expect("a sealed invoice has its number");

    // Without a template nothing is printed, and nothing is traced: the original is not used up.
    assert_eq!(print::prepare(&api, &invoice.id).await.unwrap_err(), PrintError::NoTemplate(TemplateKind::Facture));
    starters::install_missing(&api).await.unwrap();
    let job = print::prepare(&api, &invoice.id).await.unwrap();
    assert!(job.is_traced());
    assert!(prints_in_the_journal(&api).await.is_empty(), "getting ready leaves no trace");

    let original = job.finish(&api).await.unwrap();
    let trace = original.trace.clone().expect("a sealed document is traced");
    assert_eq!((trace.duplicata, trace.copy_number, trace.mention.as_deref()), (false, 1, None));
    assert_eq!(job.file_stem(original.trace.as_ref()), number);
    assert!(original.pdf.starts_with(b"%PDF-"));
    for text in [number.as_str(), "FACTURE", "Kervella", "3 quai du Port", "Revision"] {
        assert!(pdf_says(&original.pdf, text), "the original says {text}");
    }
    assert!(!pdf_says(&original.pdf, "DUPLICATA"));

    // Every print after the first is a copy, numbered, and says so across the page.
    let copy = job.finish(&api).await.unwrap();
    let trace = copy.trace.clone().unwrap();
    assert_eq!((trace.duplicata, trace.copy_number, trace.mention.as_deref()), (true, 2, Some("DUPLICATA")));
    assert_eq!(job.file_stem(copy.trace.as_ref()), format!("{number} - duplicata 2"));
    assert!(pdf_says(&copy.pdf, "DUPLICATA") && pdf_says(&copy.pdf, &number));
    let third = print::prepare(&api, &invoice.id).await.unwrap().finish(&api).await.unwrap();
    assert_eq!(third.trace.map(|trace| trace.copy_number), Some(3));
    assert_eq!(prints_in_the_journal(&api).await, ["document_printed", "document_reprinted", "document_reprinted"]);
}

#[tokio::test]
async fn a_quote_takes_its_number_when_it_is_printed_and_leaves_no_trace() {
    let api = shop().await;
    starters::install_missing(&api).await.unwrap();
    let quote = sales::create(&api, &SaleInput { status: DraftStatus::Quote, lines: one_line("Pneu arriere"), ..Default::default() }).await.unwrap();
    assert_eq!(quote.reference, None, "a quote being written has no number");

    let job = print::prepare(&api, &quote.id).await.unwrap();
    let number = job.sale.reference.clone().expect("it is numbered on its way to paper");
    assert!(!job.is_traced());
    let printed = job.finish(&api).await.unwrap();
    assert!(printed.trace.is_none());
    assert_eq!(job.file_stem(None), number);
    for text in [number.as_str(), "DEVIS", "Client de passage", "Pneu arriere"] {
        assert!(pdf_says(&printed.pdf, text), "the quote says {text}");
    }
    assert!(!pdf_says(&printed.pdf, "DUPLICATA"));

    // Printed again, it is the same quote under the same number.
    let again = print::prepare(&api, &quote.id).await.unwrap();
    assert_eq!(again.sale.reference.as_deref(), Some(number.as_str()));
    assert!(again.finish(&api).await.unwrap().trace.is_none());
    assert_eq!(sales::get(&api, &quote.id).await.unwrap().reference.as_deref(), Some(number.as_str()));
    assert!(prints_in_the_journal(&api).await.is_empty());
}

#[tokio::test]
async fn a_printed_document_carries_the_logo_and_the_pictures_of_its_template() {
    let api = shop().await;
    let pictures = |file: &[u8]| String::from_utf8_lossy(file).matches("/Subtype /Image").count();
    let quote = sales::create(&api, &SaleInput { status: DraftStatus::Quote, lines: one_line("Pneu arriere"), ..Default::default() }).await.unwrap();

    // A template that shows the shop's logo, and a picture written in its own text.
    let dot = "data:image/svg+xml;base64,\
               PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCA4IDgiPjxyZWN0IHdpZHRoPSI4IiBoZWlnaHQ9IjgiLz48L3N2Zz4=";
    let mut logo = Element::new_image(12.0, 12.0, 1);
    let mut stamp = Element::new_image(60.0, 12.0, 2);
    let mut elsewhere = Element::new_image(110.0, 12.0, 3);
    for (element, src) in [(&mut logo, "{{shop.logo_url}}"), (&mut stamp, dot), (&mut elsewhere, "https://example.invalid/tampon.png")] {
        let Kind::Image(image) = &mut element.kind else { unreachable!("an image element") };
        image.src = src.into();
    }
    let layout = Layout { elements: vec![logo, stamp, elsewhere, Element::new_text(12.0, 60.0, 4)], ..Layout::default() };
    templates::create(&api, &input("Devis illustré", TemplateKind::Devis, layout)).await.unwrap();

    // No logo yet: only the picture the template carries is drawn. One at a web address is not fetched.
    let without = print::prepare(&api, &quote.id).await.unwrap().finish(&api).await.unwrap();
    assert_eq!(pictures(&without.pdf), 1);

    let square = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 8 8\"><rect width=\"8\" height=\"8\" fill=\"#06c\"/></svg>".to_vec();
    company::upload_logo(&api, "image/svg+xml", square).await.unwrap();
    let with = print::prepare(&api, &quote.id).await.unwrap().finish(&api).await.unwrap();
    assert_eq!(pictures(&with.pdf), 2, "the logo is on the page");
}
