//! The typed calls, against the real handlers: a contract that drifts on either side fails here.

use motodesk_client::clients::{
    self, ClientInput, ListQuery, OdometerUnit, Reprise, VehicleInput, VehicleSellInput, VehicleType,
};
use motodesk_client::{local_account, network, settings, sync, Client, Cloud, Local, TokenStore};
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
async fn the_network_status_is_read_and_a_pc_outside_a_local_network_cannot_reconnect() {
    let api = client().await;

    // The fixture is not a local network: nothing to reconnect to, nobody around.
    let status = network::mesh_status(&api).await.unwrap();
    assert_eq!(status, network::MeshStatus::default());

    let refused = network::reconnect(&api, "camille", "mot de passe").await.unwrap_err();
    assert_eq!((refused.status, refused.code.as_str()), (409, "conflict"));
    assert!(refused.message.contains("réseau local"), "the server says why: {}", refused.message);
}
