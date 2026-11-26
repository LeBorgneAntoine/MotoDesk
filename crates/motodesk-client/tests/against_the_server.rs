//! The typed calls, against the real handlers: a contract that drifts on either side fails here.

use motodesk_client::clients::{
    self, ClientInput, ListQuery, OdometerUnit, Reprise, VehicleInput, VehicleSellInput, VehicleType,
};
use motodesk_client::employees::Role;
use motodesk_client::fiscal::{ClosureKind, JournalQuery};
use motodesk_client::import::schema::{self as import_schema, Mode};
use motodesk_client::import::{self, transforms, ImportRequest, ImportResult};
use motodesk_client::local_account::SetupPayload;
use motodesk_client::network_accounts::{self, NetworkAccountPatch, NewNetworkAccount};
use motodesk_client::products::{
    self, CategoryInput, ListQuery as ListProducts, ProductInput, Unit, VariantAttr, VariantInput,
};
use motodesk_client::sales::{self, DraftStatus, ListQuery as ListSales, PaymentInput, SaleInput, SaleLineInput};
use motodesk_client::stock::{self, MovementInput, SupplierInput};
use motodesk_client::workshop::{NewOrder, OrderEdit, Step, WorkshopLineInput};
use motodesk_client::{
    accounting, auth, backup, company, dashboard, fiscal, local_account, network, recovery, settings, sync, workshop, Client, Cloud, Local,
    TokenStore,
};
use motodesk_server::local_server::{loopback_api, test_support};

/// A client over a throwaway shop. The cloud address points nowhere: these calls are local.
async fn client() -> Client {
    let state = test_support::shop().await;
    let token = state.local_token.clone();
    let store = TokenStore::in_dir(&state.data_dir);
    Client::new(Local::new(loopback_api(state), &token), Cloud::new("http://127.0.0.1:9"), store)
}

#[tokio::test]
async fn a_request_without_the_local_token_is_refused() {
    let state = test_support::shop().await;
    let store = TokenStore::in_dir(&state.data_dir);
    let stranger = Client::new(Local::new(loopback_api(state), "not-the-token"), Cloud::new("http://127.0.0.1:9"), store);
    let err = clients::list(&stranger, &ListQuery::default()).await.unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (401, "unauthorized"));
}

#[tokio::test]
async fn a_customer_and_a_vehicle_go_through_their_whole_life() {
    let api = client().await;

    let created = clients::create(
        &api,
        &ClientInput {
            first_name: Some("Léa".into()),
            last_name: "Martin".into(),
            phone: Some("06 12 34 56 78".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(created.last_name, "Martin");

    let page = clients::list(&api, &ListQuery { q: "léa mar".into(), limit: Some(50), offset: Some(0) }).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, created.id);
    assert_eq!(page.items[0].vehicle_count, 0);

    let updated = clients::update(
        &api,
        &created.id,
        &ClientInput { first_name: Some("Léa".into()), last_name: "Martin-Dubois".into(), ..Default::default() },
    )
    .await
    .unwrap();
    assert_eq!(updated.last_name, "Martin-Dubois");
    assert_eq!(updated.phone, None, "a field sent empty is cleared");

    let vehicle = clients::create_vehicle(
        &api,
        &created.id,
        &VehicleInput {
            vehicle_type: VehicleType::Moto,
            make: "Honda".into(),
            model: "CB 500".into(),
            year: Some(2019),
            odometer_value: Some(12_500),
            odometer_unit: Some(OdometerUnit::Km),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!((vehicle.make.as_str(), vehicle.year, vehicle.odometer_value), ("Honda", Some(2019), 12_500));

    let detail = clients::get(&api, &created.id).await.unwrap();
    assert_eq!(detail.client.last_name, "Martin-Dubois");
    assert_eq!(detail.vehicles.len(), 1);
    assert_eq!(detail.client.outstanding_cents, Some(0));

    let renamed = clients::update_vehicle(
        &api,
        &vehicle.id,
        &VehicleInput { vehicle_type: VehicleType::Moto, make: "Honda".into(), model: "CB 500 F".into(), ..Default::default() },
    )
    .await
    .unwrap();
    assert_eq!(renamed.model, "CB 500 F");
    assert_eq!(clients::get_vehicle(&api, &vehicle.id).await.unwrap().model, "CB 500 F");

    clients::delete_vehicle(&api, &vehicle.id).await.unwrap();
    assert!(clients::list_vehicles(&api, &created.id).await.unwrap().is_empty());

    clients::delete(&api, &created.id).await.unwrap();
    let gone = clients::get(&api, &created.id).await.unwrap_err();
    assert_eq!((gone.status, gone.code.as_str()), (404, "not_found"));
}

#[tokio::test]
async fn a_fresh_pc_offers_to_create_its_first_local_account() {
    let api = client().await;
    let status = local_account::status(&api).await.unwrap();
    assert!(!status.has_account);
    assert_eq!(local_account::local_state(Some(status)), local_account::LocalState::Setup);
}

#[tokio::test]
async fn settings_and_sync_status_answer_in_the_shapes_the_interface_reads() {
    let api = client().await;

    let stored = settings::set(&api, "system.theme", &serde_json::json!("dark")).await.unwrap();
    assert_eq!(stored.value, serde_json::json!("dark"));
    let all = settings::list(&api).await.unwrap();
    assert!(all.iter().any(|s| s.key == "system.theme" && s.value == "dark"));

    let status = sync::status(&api).await.unwrap();
    assert!(!status.device_id.is_empty());
}

#[tokio::test]
async fn a_stock_vehicle_is_listed_then_sold_with_a_trade_in() {
    let api = client().await;

    let bike = clients::create_stock_vehicle(
        &api,
        &VehicleInput {
            vehicle_type: VehicleType::Moto,
            make: "Yamaha".into(),
            model: "MT-07".into(),
            vin_or_serial: Some("JYARM321000012345".into()),
            purchase_price_ht_cents: Some(500_000),
            selling_price_ttc_cents: Some(720_000),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(bike.is_stock && bike.client_id.is_none());

    let stock = clients::list_stock_vehicles(&api, "").await.unwrap();
    assert_eq!(stock.len(), 1);
    assert_eq!(clients::list_stock_vehicles(&api, "jyarm321").await.unwrap().len(), 1);
    assert!(clients::list_stock_vehicles(&api, "ducati").await.unwrap().is_empty());
    assert_eq!(
        clients::vehicle_margin_cents(stock[0].purchase_price_ht_cents, stock[0].selling_price_ttc_cents),
        Some(220_000)
    );

    let buyer =
        clients::create(&api, &ClientInput { last_name: "Kervella".into(), ..Default::default() }).await.unwrap();
    let sale = clients::sell_stock_vehicle(
        &api,
        &bike.id,
        &VehicleSellInput {
            client_id: buyer.id.clone(),
            reprise: Some(Reprise {
                make: "Honda".into(),
                model: "CB 125".into(),
                purchase_price_ht_cents: 90_000,
                ..Default::default()
            }),
        },
    )
    .await
    .unwrap();
    assert!(!sale.id.is_empty());

    // The trade-in entered the stock.
    let stock = clients::list_stock_vehicles(&api, "CB 125").await.unwrap();
    assert_eq!(stock.len(), 1);
    assert_eq!(stock[0].purchase_price_ht_cents, Some(90_000));
}

#[tokio::test]
async fn the_home_screen_counts_the_shop_and_lists_its_last_documents() {
    let api = client().await;

    let empty = dashboard::stats(&api).await.unwrap();
    assert_eq!((empty.active_clients, empty.vehicles_in_workshop, empty.revenue_this_month_cents), (0, 0, 0));
    assert!(dashboard::recent(&api, 10).await.unwrap().is_empty());

    // Selling a stock vehicle writes a draft invoice: the one document there is.
    let bike = clients::create_stock_vehicle(
        &api,
        &VehicleInput {
            vehicle_type: VehicleType::Moto,
            make: "Yamaha".into(),
            model: "MT-07".into(),
            selling_price_ttc_cents: Some(720_000),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let buyer = clients::create(
        &api,
        &ClientInput { first_name: Some("Maëlle".into()), last_name: "Kervella".into(), ..Default::default() },
    )
    .await
    .unwrap();
    let sale =
        clients::sell_stock_vehicle(&api, &bike.id, &VehicleSellInput { client_id: buyer.id, reprise: None }).await.unwrap();

    let stats = dashboard::stats(&api).await.unwrap();
    assert_eq!(stats.active_clients, 1);
    assert_eq!(stats.revenue_this_month_cents, 0, "a draft is not revenue yet");

    let recent = dashboard::recent(&api, 10).await.unwrap();
    assert_eq!(recent.len(), 1);
    let line = &recent[0];
    assert_eq!((line.id.as_str(), line.doc_type.as_str()), (sale.id.as_str(), "FACTURE"));
    assert_eq!(line.client_name.as_deref(), Some("Maëlle Kervella"));
    assert!(!line.doc_number.is_empty() && !line.status.is_empty() && !line.created_at.is_empty());
    assert_eq!(dashboard::status_label(&line.status), "Brouillon");
}

#[tokio::test]
async fn the_catalogue_goes_through_its_whole_life() {
    let api = client().await;

    // A category, then a product filed under it.
    let brakes = products::create_category(&api, &CategoryInput { name: "Freinage".into(), position: Some(10) }).await.unwrap();
    let pads = products::create(
        &api,
        &ProductInput {
            name: "Plaquette de frein avant".into(),
            category_id: Some(brakes.id.clone()),
            sku: Some("BRMB-PLQ-001".into()),
            brand: Some("Brembo".into()),
            selling_price_ttc_cents: Some(4_990),
            vat_rate_bp: 2000,
            stock_quantity: Some(3.),
            low_stock_threshold: Some(5),
            is_active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!((pads.stock_quantity, pads.stock_milli, pads.unit), (3., 3_000, Unit::Piece));

    let categories = products::list_categories(&api).await.unwrap();
    // A new shop comes with a few categories of its own; ours counts its one product.
    let ours = categories.iter().find(|c| c.id == brakes.id).expect("the new category is listed");
    assert_eq!((ours.name.as_str(), ours.product_count, ours.position), ("Freinage", 1, 10));

    // The list: by a piece of the name, brand or SKU; by category; by low stock.
    let found = products::list(&api, &ListProducts { q: "brmb-plq".into(), ..Default::default() }).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].category_name.as_deref(), Some("Freinage"));
    assert!(products::is_low_stock(found[0].is_service, found[0].stock_quantity, found[0].low_stock_threshold));
    let in_category =
        products::list(&api, &ListProducts { category_id: Some(brakes.id.clone()), ..Default::default() }).await.unwrap();
    assert_eq!(in_category.len(), 1);
    assert_eq!(products::list(&api, &ListProducts { low_stock: true, ..Default::default() }).await.unwrap().len(), 1);

    // A reception from a new supplier brings the stock over its threshold.
    let supplier = stock::create_supplier(&api, &SupplierInput { name: "Bihr".into(), ..Default::default() }).await.unwrap();
    assert_eq!(stock::list_suppliers(&api).await.unwrap().len(), 1);
    stock::create_movement(
        &api,
        &MovementInput {
            product_id: pads.id.clone(),
            supplier_id: Some(supplier.id),
            quantity_milli: 4_500,
            unit_cost_ht_cents: Some(2_100),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(products::get(&api, &pads.id).await.unwrap().stock_quantity, 7.5);
    assert!(products::list(&api, &ListProducts { low_stock: true, ..Default::default() }).await.unwrap().is_empty());
    let moves = stock::list_movements(&api, Some(&pads.id)).await.unwrap();
    assert!(moves.iter().any(|m| m.kind == "reception" && m.quantity_milli == 4_500));

    // An edit that leaves the stock alone does not undo the reception.
    let renamed = products::update(
        &api,
        &pads.id,
        &ProductInput {
            name: "Plaquettes avant".into(),
            category_id: Some(brakes.id.clone()),
            sku: Some("BRMB-PLQ-001".into()),
            vat_rate_bp: 2000,
            is_active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!((renamed.name.as_str(), renamed.stock_quantity), ("Plaquettes avant", 7.5));
    assert_eq!(renamed.brand, None, "a field sent empty is cleared");

    // A variant is a product of its own, named after its model.
    let blue = products::create_variant(
        &api,
        &pads.id,
        &VariantInput {
            attrs: vec![VariantAttr { name: "couleur".into(), value: "bleu".into() }],
            stock_quantity: Some(2.),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(blue.parent_id.as_deref(), Some(pads.id.as_str()));
    assert_eq!(blue.variant_label.as_deref(), Some("couleur bleu"));
    assert_eq!(blue.name, "Plaquettes avant — couleur bleu");
    let variants = products::list_variants(&api, &pads.id).await.unwrap();
    assert!(variants.iter().any(|v| v.id == blue.id));

    // Deleting: the variant, the product, then the category.
    products::delete(&api, &blue.id).await.unwrap();
    products::delete(&api, &pads.id).await.unwrap();
    assert!(products::list(&api, &ListProducts::default()).await.unwrap().is_empty());
    products::delete_category(&api, &brakes.id).await.unwrap();
    assert!(products::list_categories(&api).await.unwrap().iter().all(|c| c.id != brakes.id));
}

#[tokio::test]
async fn a_document_goes_from_draft_to_sealed_invoice_paid_then_credited() {
    let api = client().await;
    assert_eq!(fiscal::check_profile(&api).await.unwrap().blocker(), None, "the fixture shop can invoice");

    let buyer = clients::create(&api, &ClientInput { last_name: "Kervella".into(), ..Default::default() }).await.unwrap();
    let lines = vec![
        // A shelf price: 9,99 TTC is what the customer pays, whatever the rounding of its HT.
        SaleLineInput {
            label: Some("Bidon d'huile".into()),
            quantity_milli: 2_000,
            unit_price_ht_cents: 833,
            unit_price_ttc_cents: Some(999),
            vat_rate_bp: 2000,
            ..Default::default()
        },
        SaleLineInput {
            label: Some("Main d'œuvre".into()),
            quantity_milli: 1_500,
            unit_price_ht_cents: 6_000,
            vat_rate_bp: 2000,
            discount_bp: 1000,
            ..Default::default()
        },
    ];
    let draft = sales::create(
        &api,
        &SaleInput {
            client_id: Some(buyer.id.clone()),
            sale_date: Some("2026-11-05".into()),
            lines: lines.clone(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!((draft.status.as_str(), draft.is_sealed(), draft.lines.len()), ("draft", false, 2));
    // 2 x 9,99 TTC = 19,98 ; 1,5 x 60,00 HT - 10 % = 81,00 HT = 97,20 TTC.
    assert_eq!(draft.total_ttc_cents, Some(1_998 + 9_720));
    assert_eq!(draft.lines[0].shelf_price_cents(), Some(999));
    assert_eq!(draft.lines[1].shelf_price_cents(), None);

    // The list finds it by its customer, and by its status.
    let found = sales::list(&api, &ListSales { q: "kervella".into(), ..Default::default() }).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].client_name.as_deref(), found[0].due_cents()), (Some("Kervella"), None));
    assert_eq!(sales::list(&api, &ListSales { status: "draft".into(), ..Default::default() }).await.unwrap().len(), 1);
    assert!(sales::list(&api, &ListSales { status: "paid".into(), ..Default::default() }).await.unwrap().is_empty());

    // An edit replaces the lines.
    let edited = sales::update(
        &api,
        &draft.id,
        &SaleInput {
            client_id: Some(buyer.id.clone()),
            sale_date: Some("2026-11-05".into()),
            notes: Some("À régler à réception.".into()),
            lines: vec![lines[1].clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!((edited.lines.len(), edited.total_ttc_cents), (1, Some(9_720)));

    // Sealing gives it its number and closes it to edits and deletion.
    let sealed = sales::validate(&api, &draft.id).await.unwrap();
    assert!(sealed.is_sealed() && sealed.reference.is_some());
    assert_eq!(sealed.outstanding_cents, 9_720);
    assert!(sales::update(&api, &draft.id, &SaleInput::default()).await.is_err());
    assert!(sales::delete(&api, &draft.id).await.is_err());

    // A receipt, taken back by the opposite movement, then the full payment.
    let receipt = sales::add_payment(
        &api,
        &draft.id,
        &PaymentInput { amount_cents: 5_000, method: "cheque".into(), bank: Some("Crédit Agricole".into()), ..Default::default() },
    )
    .await
    .unwrap();
    assert!(receipt.is_reversible());
    assert_eq!(sales::get(&api, &draft.id).await.unwrap().outstanding_cents, 4_720);
    let reversal = sales::reverse_payment(&api, &receipt.id).await.unwrap();
    assert_eq!((reversal.amount_cents, reversal.is_reversible()), (-5_000, false));
    let too_much = sales::add_payment(&api, &draft.id, &PaymentInput { amount_cents: 9_721, method: "cash".into(), ..Default::default() }).await;
    assert!(too_much.is_err(), "a receipt cannot exceed what is owed");
    sales::add_payment(&api, &draft.id, &PaymentInput { amount_cents: 9_720, method: "card".into(), ..Default::default() }).await.unwrap();
    let paid = sales::get(&api, &draft.id).await.unwrap();
    assert_eq!((paid.status.as_str(), paid.outstanding_cents, paid.payments.len()), ("paid", 0, 3));

    // A mistake on a sealed document is corrected by a credit note, itself sealed.
    let note = sales::create_credit_note(&api, &draft.id).await.unwrap();
    assert!(note.is_credit_note() && note.is_sealed());
    assert_eq!(note.corrects_sale_id.as_deref(), Some(draft.id.as_str()));
    assert_eq!(note.total_ttc_cents, Some(-9_720));
}

#[tokio::test]
async fn a_quote_becomes_an_invoice_or_a_repair_order_and_is_kept() {
    let api = client().await;
    let quote = sales::create(
        &api,
        &SaleInput {
            status: DraftStatus::Quote,
            lines: vec![SaleLineInput {
                label: Some("Révision".into()),
                quantity_milli: 1_000,
                unit_price_ht_cents: 10_000,
                vat_rate_bp: 2000,
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(quote.is_quote() && !quote.is_closed_quote());

    let invoice = sales::convert_quote(&api, &quote.id).await.unwrap();
    assert_eq!((invoice.status.as_str(), invoice.source_quote_id.as_deref()), ("draft", Some(quote.id.as_str())));
    assert_eq!(invoice.total_ttc_cents, Some(12_000));
    // Asking again gives the same draft, and the quote now points at it.
    assert_eq!(sales::convert_quote(&api, &quote.id).await.unwrap().id, invoice.id);
    let kept = sales::get(&api, &quote.id).await.unwrap();
    assert!(kept.is_closed_quote());
    assert_eq!(kept.converted_to_sale_id.as_deref(), Some(invoice.id.as_str()));

    // A draft is deleted freely.
    sales::delete(&api, &invoice.id).await.unwrap();
    assert!(!sales::get(&api, &quote.id).await.unwrap().is_closed_quote(), "the quote is open again");

    let order = workshop::create_order_from_quote(&api, &quote.id).await.unwrap();
    assert!(!order.id.is_empty());
    assert!(sales::get(&api, &quote.id).await.unwrap().reference.is_some(), "the quote took its number on the way");
}

#[tokio::test]
async fn a_repair_order_goes_through_the_workshop_and_becomes_an_invoice() {
    let api = client().await;
    let owner = clients::create(&api, &ClientInput { last_name: "Le Gall".into(), ..Default::default() }).await.unwrap();

    let order = workshop::create(
        &api,
        &NewOrder { label: "Révision 10 000 km".into(), client_id: Some(owner.id.clone()), ..Default::default() },
    )
    .await
    .unwrap();
    assert_eq!((order.status, order.client_name.as_deref()), (Step::Pending, Some("Le Gall")));

    // The form: a part at its shelf price, an hour and a half of labour, the mileage.
    let edit = OrderEdit {
        label: "Révision 10 000 km".into(),
        client_id: Some(owner.id.clone()),
        mileage: Some(10_250),
        fuel_level: Some("1/2".into()),
        promised_at: Some("2026-12-01".into()),
        notes: Some("Bruit à l'avant.".into()),
        lines: vec![
            WorkshopLineInput {
                label: Some("Bidon d'huile".into()),
                quantity_milli: 2_000,
                unit_price_ht_cents: 833,
                unit_price_ttc_cents: Some(999),
                vat_rate_bp: 2000,
                ..Default::default()
            },
            WorkshopLineInput {
                label: Some("Main d'œuvre".into()),
                quantity_milli: 1_500,
                unit_price_ht_cents: 6_000,
                vat_rate_bp: 2000,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let saved = workshop::update(&api, &order.id, &edit).await.unwrap();
    assert_eq!((saved.mileage, saved.fuel_level.as_deref(), saved.lines.len()), (Some(10_250), Some("1/2"), 2));
    assert_eq!(saved.lines[0].unit_price_ttc_cents, Some(999));

    // Sending a field empty clears it; moving the card touches nothing else.
    let cleared = workshop::update(&api, &order.id, &OrderEdit { fuel_level: None, ..edit.clone() }).await.unwrap();
    assert_eq!(cleared.fuel_level, None);
    let moved = workshop::set_step(&api, &order.id, Step::InProgress).await.unwrap();
    assert_eq!((moved.status, moved.mileage, moved.lines.len()), (Step::InProgress, Some(10_250), 2));
    workshop::set_step(&api, &order.id, Step::Ready).await.unwrap();
    assert!(workshop::set_step(&api, &order.id, Step::Invoiced).await.is_err(), "invoiced is never set by hand");

    let board = workshop::list_board(&api).await.unwrap();
    assert_eq!(board.iter().map(|o| (o.id.as_str(), o.status)).collect::<Vec<_>>(), [(order.id.as_str(), Step::Ready)]);

    // Invoicing drafts the invoice; asking again brings the same draft up to date.
    let draft = workshop::invoice(&api, &order.id).await.unwrap();
    assert!(!draft.refreshed_from_order);
    let again = workshop::invoice(&api, &order.id).await.unwrap();
    assert_eq!((again.id.as_str(), again.refreshed_from_order), (draft.id.as_str(), true));
    let invoice = sales::get(&api, &draft.id).await.unwrap();
    // 2 x 9,99 TTC + 1,5 x 60,00 HT (108,00 TTC).
    assert_eq!((invoice.lines.len(), invoice.total_ttc_cents), (2, Some(1_998 + 10_800)));

    // Only the validated invoice makes the order "Facturé", and then it no longer changes.
    sales::validate(&api, &draft.id).await.unwrap();
    let invoiced = workshop::get(&api, &order.id).await.unwrap();
    assert_eq!((invoiced.status, invoiced.sale_id.as_deref()), (Step::Invoiced, Some(draft.id.as_str())));
    assert!(workshop::update(&api, &order.id, &edit).await.is_err());

    // An order that was not invoiced can be removed.
    let mistake = workshop::create(&api, &NewOrder { label: "Erreur de saisie".into(), ..Default::default() }).await.unwrap();
    workshop::delete(&api, &mistake.id).await.unwrap();
    assert!(workshop::list_board(&api).await.unwrap().iter().all(|o| o.id != mistake.id));
}

#[tokio::test]
async fn the_books_and_the_fiscal_controls_follow_a_sealed_invoice() {
    // Closing a period and archiving are for administrators: a signed-in one is needed.
    let (api, _) = local_network().await;
    let today = chrono::Local::now().date_naive();
    let (year, day) = (chrono::Datelike::year(&today), today.format("%Y-%m-%d").to_string());
    let (from, to) = (format!("{year}-01-01"), format!("{year}-12-31"));

    // One invoice of 120,00 TTC at 20 %, of which 50,00 are paid by card.
    let buyer = clients::create(&api, &ClientInput { last_name: "Kervella".into(), ..Default::default() }).await.unwrap();
    let draft = sales::create(
        &api,
        &SaleInput {
            client_id: Some(buyer.id),
            sale_date: Some(day.clone()),
            lines: vec![SaleLineInput {
                label: Some("Révision".into()),
                quantity_milli: 1_000,
                unit_price_ht_cents: 10_000,
                vat_rate_bp: 2000,
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let invoice = sales::validate(&api, &draft.id).await.unwrap();
    sales::add_payment(&api, &draft.id, &PaymentInput { amount_cents: 5_000, method: "card".into(), ..Default::default() }).await.unwrap();

    // The reports.
    let sold = accounting::sales_journal(&api, &from, &to).await.unwrap();
    assert_eq!((sold.rows.len(), sold.total_ht_cents, sold.total_vat_cents, sold.total_ttc_cents), (1, 10_000, 2_000, 12_000));
    assert_eq!((sold.rows[0].reference.as_deref(), sold.rows[0].kind_label()), (invoice.reference.as_deref(), "Facture"));
    assert_eq!((sold.rows[0].client_name.as_deref(), sold.rows[0].paid_cents), (Some("Kervella"), 5_000));
    assert!(accounting::sales_journal(&api, "2001-01-01", "2001-12-31").await.unwrap().rows.is_empty());

    let cash = accounting::cash_journal(&api, &from, &to).await.unwrap();
    assert_eq!((cash.rows.len(), cash.total_cents, cash.methods_by_amount()), (1, 5_000, vec![("card", 5_000)]));

    let vat = accounting::vat_recap(&api, &from, &to).await.unwrap();
    assert_eq!((vat.document_count, vat.total_base_ht_cents, vat.total_vat_cents), (1, 10_000, 2_000));
    assert_eq!(vat.rates.iter().map(|r| (r.rate_bp, r.vat_cents)).collect::<Vec<_>>(), [(2000, 2_000)]);

    let owed = accounting::ageing(&api).await.unwrap();
    assert_eq!((owed.rows.len(), owed.total_outstanding_cents, owed.buckets.not_due_cents), (1, 7_000, 7_000));
    assert!(owed.rows[0].days_overdue < 0, "an invoice of today is within its payment terms");

    let fec = accounting::fec_export(&api, year).await.unwrap();
    assert!(fec.line_count > 0 && fec.file_name.contains("FEC"));
    assert!(fec.content.lines().count() > fec.preview(1).len());

    // The register and its journal.
    let register = fiscal::register(&api).await.unwrap();
    assert!(!register.code.is_empty() && register.last_seq > 0);
    assert_eq!(register.perpetual_total_cents, 12_000);
    assert!(invoice.reference.as_deref().unwrap_or_default().contains(&register.code));

    let journal =
        fiscal::journal(&api, &JournalQuery { from: from.clone(), to: to.clone(), limit: Some(2000), ..Default::default() }).await.unwrap();
    let events: Vec<&str> = journal.iter().map(|e| e.event_type.as_str()).collect();
    assert!(events.contains(&"invoice") && events.contains(&"payment"));
    let only_invoices =
        fiscal::journal(&api, &JournalQuery { from, to, event_type: "invoice".into(), limit: Some(2000) }).await.unwrap();
    assert_eq!(only_invoices.len(), 1);
    assert!(only_invoices[0].matches(invoice.reference.as_deref().unwrap_or("?")));

    let report = fiscal::verify_chains(&api).await.unwrap();
    assert!(report.ok && report.registers.iter().all(|r| r.ok && r.signatures_verified > 0));

    // Closing today, then last year, then archiving it.
    assert!(fiscal::pending_closures(&api).await.unwrap().is_empty(), "the day in progress is not a closure overdue");
    let closed = fiscal::close_period(&api, ClosureKind::Day, Some(&day)).await.unwrap();
    assert_eq!((closed.kind, closed.document_count, closed.total_ttc_cents), (ClosureKind::Day, 1, 12_000));
    // A period is closed once: asking again hands back the closure that exists.
    assert_eq!(fiscal::close_period(&api, ClosureKind::Day, Some(&day)).await.unwrap().id, closed.id);

    assert!(fiscal::list_archives(&api).await.unwrap().is_empty());
    assert!(fiscal::create_archive(&api, year).await.is_err(), "a year in progress is not archived");
    assert!(fiscal::create_archive(&api, year - 1).await.is_err(), "nor is a year that was not closed");
    fiscal::close_period(&api, ClosureKind::Year, None).await.unwrap();
    let archive = fiscal::create_archive(&api, year - 1).await.unwrap();
    assert!(archive.entry_count >= 0 && archive.file_bytes > 0);
    assert_eq!(fiscal::list_archives(&api).await.unwrap().len(), 1);
    assert!(fiscal::verify_archive(&api, &archive.id).await.unwrap().ok);

    let closures = fiscal::list_closures(&api, 200).await.unwrap();
    assert!(closures.iter().any(|c| c.kind == ClosureKind::Day) && closures.iter().any(|c| c.kind == ClosureKind::Year));
}

#[tokio::test]
async fn the_network_status_is_read_and_a_pc_outside_a_local_network_cannot_reconnect() {
    let api = client().await;

    // The fixture is not a local network: nothing to reconnect to, nobody around.
    let status = network::mesh_status(&api).await.unwrap();
    assert_eq!(status, network::MeshStatus::default());

    let refused = network::reconnect(&api, "camille", "mot de passe").await.unwrap_err();
    assert_eq!((refused.status, refused.code.as_str()), (409, "conflict"));
    assert!(refused.message.contains("réseau local"), "the server says why: {}", refused.message);
}

/// A fresh PC on which the first administrator has just created the local network.
async fn local_network() -> (Client, String) {
    let state = test_support::shop().await;
    let token = state.local_token.clone();
    let store = TokenStore::in_dir(&state.data_dir);
    let api = Client::new(Local::new(loopback_api(state), &token), Cloud::new("http://127.0.0.1:9"), store);
    let password = "mot de passe admin".to_string();
    let session = local_account::setup(
        &api,
        &SetupPayload { username: "camille".into(), password: password.clone(), confirm: password.clone(), force: true },
    )
    .await
    .expect("the first administrator creates the network");
    api.set_tokens(Some(session.bundle));
    (api, password)
}

#[tokio::test]
async fn the_accounts_of_a_local_network_are_created_changed_and_removed() {
    let (api, _) = local_network().await;

    let accounts = network_accounts::list(&api).await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert!(accounts[0].is_admin() && accounts[0].is_active);
    assert_eq!(accounts[0].username, "camille");

    let lea = network_accounts::create(
        &api,
        &NewNetworkAccount {
            username: "lea".into(),
            password: "mot de passe lea".into(),
            confirm: "mot de passe lea".into(),
            role: Role::Employee,
            full_name: Some("Léa Martin".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!((lea.display_name(), lea.is_admin(), lea.is_active), ("Léa Martin", false, true));

    let off = network_accounts::update(&api, &lea.id, &NetworkAccountPatch { is_active: Some(false), ..Default::default() })
        .await
        .unwrap();
    assert!(!off.is_active);

    network_accounts::update(
        &api,
        &lea.id,
        &NetworkAccountPatch {
            password: Some("un autre mot de passe".into()),
            confirm: Some("un autre mot de passe".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let twice = network_accounts::create(
        &api,
        &NewNetworkAccount {
            username: "lea".into(),
            password: "mot de passe lea".into(),
            confirm: "mot de passe lea".into(),
            role: Role::Employee,
            full_name: None,
        },
    )
    .await
    .unwrap_err();
    // The name is taken, even by an account that is switched off: the server says which case it is.
    assert_eq!(twice.status, 400);
    assert!(twice.message.contains("désactivé"), "{}", twice.message);

    network_accounts::delete(&api, &lea.id).await.unwrap();
    assert_eq!(network_accounts::list(&api).await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_recovery_key_and_the_pcs_of_a_local_network_are_read() {
    let (api, password) = local_network().await;

    // Creating the network made a key; it is not confirmed until someone types it back.
    let status = recovery::status(&api).await.unwrap();
    assert!(status.exists && !status.confirmed);

    let issued = recovery::issue(&api, &password).await.unwrap();
    recovery::confirm(&api, &issued.recovery_key).await.unwrap();
    assert!(recovery::status(&api).await.unwrap().confirmed);

    let refused = recovery::issue(&api, "pas le bon").await.unwrap_err();
    assert!((400..500).contains(&refused.status), "a wrong password does not issue a key: {}", refused.status);

    // The list of PCs answers in the shape the screen reads; this PC is never offered for removal.
    let devices = network::list_devices(&api).await.unwrap();
    assert!(devices.iter().filter(|d| d.this_pc).count() <= 1);
    assert!(devices.iter().all(|d| !d.device_id.is_empty()));
}

#[tokio::test]
async fn a_local_network_writes_its_own_identity_and_logo() {
    let (api, _) = local_network().await;

    let named = company::update(&api, "  Garage Dupont ", "Garage Dupont SARL").await.unwrap();
    assert_eq!((named.name(), named.legal_name()), ("Garage Dupont", "Garage Dupont SARL"));
    assert!(!named.has_logo);
    assert_eq!(company::logo(&api).await.unwrap(), None);

    // An emptied legal name is removed, not kept.
    let cleared = company::update(&api, "Garage Dupont", "").await.unwrap();
    assert_eq!(cleared.legal_name, None);
    assert_eq!(company::get(&api).await.unwrap().name(), "Garage Dupont");

    let unnamed = company::update(&api, " ", "").await.unwrap_err();
    assert_eq!(unnamed.status, 400);

    let picture = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 8 8\"><rect width=\"8\" height=\"8\"/></svg>".to_vec();
    let with_logo = company::upload_logo(&api, "image/svg+xml", picture.clone()).await.unwrap();
    assert!(with_logo.has_logo && with_logo.logo_updated_at.is_some());
    assert_eq!(company::logo(&api).await.unwrap(), Some(picture));

    let not_an_image = company::upload_logo(&api, "application/pdf", vec![1, 2, 3]).await.unwrap_err();
    assert_eq!(not_an_image.status, 400);
    let too_big = company::upload_logo(&api, "image/png", vec![0; company::MAX_LOGO_BYTES + 1]).await.unwrap_err();
    assert_eq!(too_big.status, 400);
}

#[tokio::test]
async fn a_backup_is_written_and_listed() {
    let (api, _) = local_network().await;

    let before = backup::status(&api).await.unwrap();
    assert!(before.files.is_empty() && !before.restore_pending);
    assert!(!before.backups_dir.is_empty());

    let written = backup::export(&api, "trop court").await.unwrap();
    // Ten characters exactly is the shortest accepted.
    assert_eq!(written.kind_label(), "Manuelle");
    let refused = backup::export(&api, "court").await.unwrap_err();
    assert_eq!(refused.status, 400);

    let after = backup::status(&api).await.unwrap();
    assert_eq!(after.files.len(), 1);
    assert_eq!(after.files[0].filename, written.filename);
    assert!(after.files[0].size_bytes > 0 && after.last_backup_at.is_some());

    // A wrong password queues nothing.
    let wrong = backup::restore(&api, &written.filename, "pas le bon mot").await.unwrap_err();
    assert!((400..500).contains(&wrong.status), "{}", wrong.status);
    assert!(!backup::status(&api).await.unwrap().restore_pending);
}

#[tokio::test]
async fn the_sync_diagnostics_and_the_addresses_typed_by_hand_are_read_and_written() {
    let (api, _) = local_network().await;

    let diagnostics = sync::diagnostics(&api).await.unwrap();
    assert!(!diagnostics.device_id.is_empty());
    assert!(diagnostics.standalone, "a local network has no cloud");
    assert!(diagnostics.lan_peers.is_empty());

    assert!(sync::static_peers(&api).await.unwrap().is_empty());
    let typed = sync::parse_static_peers("192.168.1.42:17300\r\n\n 192.168.1.43:17300 , poste-3:17300");
    assert_eq!(typed, ["192.168.1.42:17300", "192.168.1.43:17300", "poste-3:17300"]);
    assert_eq!(sync::set_static_peers(&api, &typed).await.unwrap(), typed);
    assert_eq!(sync::static_peers(&api).await.unwrap(), typed);
}

#[tokio::test]
async fn syncing_from_a_local_network_keeps_its_session() {
    let (api, _) = local_network().await;
    let session = api.tokens().expect("the administrator is signed in");
    assert!(session.user.is_local());

    // What the sync screen does before syncing. Handing a local session back to the service
    // would replace its record and end it: the client does not.
    auth::sync_session_to_local(&api, &session).await;
    sync::sync_now(&api).await.expect("a local network syncs without a cloud");

    let me = auth::local_me(&api).await.expect("the session still stands");
    assert_eq!(me.id, session.user.id);
}

/// Send a CSV text the way the wizard does: mapped by its headers, clean rows only, in batches.
async fn import_csv(api: &Client, entity: &str, mode: Mode, match_key: &str, text: &str, write: bool) -> (ImportResult, Vec<usize>) {
    let parsed = import::csv::parse(text).unwrap();
    let entity = import_schema::entity(entity).unwrap();
    let mappings = transforms::suggest_mappings(entity, &parsed.headers);
    assert!(transforms::unmapped_required(&mappings).is_empty(), "a required field has no column");
    let results = transforms::transform(&parsed, &mappings);
    let (rows, index_map) = transforms::sendable(&results);
    let request = ImportRequest {
        entity: entity.id,
        mode,
        match_key: match_key.to_string(),
        ignore_duplicates: false,
        create_missing_categories: true,
        references: transforms::references(&mappings),
        rows: Vec::new(),
    };
    let mut whole = ImportResult::default();
    // Two rows at a time, to go through the batching.
    for (batch, chunk) in rows.chunks(2).enumerate() {
        let body = request.with_rows(chunk);
        let answer = if write { import::commit(api, &body).await } else { import::preview(api, &body).await };
        whole.absorb(answer.unwrap(), batch * 2);
    }
    (whole, index_map)
}

#[tokio::test]
async fn a_csv_file_becomes_customers_and_products() {
    let api = client().await;
    let customers = "Nom;Prénom;Email;Téléphone\n\
                     Martin;Léa;lea@example.fr;06 12 34 56 78\n\
                     ;Sans Nom;personne@example.fr;\n\
                     Dubois;Rémi;remi@example.fr;\n\
                     Moreau;Inès;ines@example.fr;07 00 00 00 01\n";

    // Looking first writes nothing.
    let (looked, _) = import_csv(&api, "clients", Mode::Insert, "email", customers, false).await;
    assert_eq!((looked.inserted, looked.skipped, looked.errored), (3, 0, 0));
    assert_eq!(clients::list(&api, &ListQuery::default()).await.unwrap().total, 0);

    let (written, index_map) = import_csv(&api, "clients", Mode::Insert, "email", customers, true).await;
    assert_eq!((written.inserted, written.rows.len()), (3, 3));
    // The row without a name never left: the rows sent are lines 2, 4 and 5 of the file.
    assert_eq!(index_map.iter().map(|&i| transforms::line_of(i)).collect::<Vec<_>>(), [2, 4, 5]);
    let page = clients::list(&api, &ListQuery { q: "martin".into(), limit: Some(10), offset: Some(0) }).await.unwrap();
    assert_eq!(page.items[0].first_name.as_deref(), Some("Léa"));

    // The same file again: everyone is already there.
    let (again, _) = import_csv(&api, "clients", Mode::Insert, "email", customers, true).await;
    assert_eq!((again.inserted, again.skipped), (0, 3));

    // Add-or-update, found by e-mail: one changes, one is new.
    let changes = "Nom;Email;Téléphone\nMartin;lea@example.fr;06 99 99 99 99\nKervella;chloe@example.fr;\n";
    let (merged, _) = import_csv(&api, "clients", Mode::Upsert, "email", changes, true).await;
    assert_eq!((merged.inserted, merged.updated), (1, 1));
    assert_eq!(clients::list(&api, &ListQuery::default()).await.unwrap().total, 4);

    // Products: prices in euros, a rate in percent, a quantity with decimals, a category by name.
    let catalogue = "Référence,Désignation,Catégorie,Prix TTC,TVA,Stock\n\
                     HUI-5W40,Huile 5W40 1L,Lubrifiants,\"12,90\",20,\"7,5\"\n\
                     PLQ-AV,Plaquettes avant,Freinage,34.50,20,4\n";
    let (stocked, _) = import_csv(&api, "products", Mode::Insert, "sku", catalogue, true).await;
    assert_eq!((stocked.inserted, stocked.errored), (2, 0), "{:?}", stocked.rows);
    let found = products::list(&api, &ListProducts { q: "HUI".into(), ..Default::default() }).await.unwrap();
    let oil = &found[0];
    assert_eq!((oil.name.as_str(), oil.selling_price_ttc_cents, oil.vat_rate_bp), ("Huile 5W40 1L", Some(1290), 2000));
    assert_eq!(oil.stock_milli, 7_500);
    assert!(products::list_categories(&api).await.unwrap().iter().any(|c| c.name == "Lubrifiants"));
}
