//! Document templates against the service's own handlers: the library, its defaults, and the
//! starter templates a new shop is given.

use motodesk_client::local_account::{self, SetupPayload};
use motodesk_client::templates::schema::{Element, Kind, Layout};
use motodesk_client::templates::{self, starters, ListQuery, Orientation, TemplateInput, TemplateKind};
use motodesk_client::{Client, Cloud, Local, TokenStore};
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
