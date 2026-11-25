//! Fill a new, throwaway data folder with a local network, a few customers, a small catalogue
//! and a made-up company profile (so that an invoice can be validated), to look at the screens
//! with data in them.
//!
//! ```text
//! cargo run -p motodesk-gpui --example seed -- <folder>
//! MOTODESK_PROFILE=demo MOTODESK_DATA_DIR=<folder> cargo run -p motodesk-gpui
//! ```
//!
//! It refuses a folder that already holds a database, so it can never touch a shop's data. The
//! account it creates is signed in on that folder and its password is random and not kept:
//! the folder is for looking, not for working.

use std::path::PathBuf;

use motodesk_client::clients::{self, ClientInput, OdometerUnit, VehicleInput, VehicleType};
use motodesk_client::local_account::{self, SetupPayload};
use motodesk_client::products::{self, ProductInput, Unit};
use motodesk_client::{Client, Cloud, Local, TokenStore, recovery, settings};
use motodesk_server::local_server::{self, local_auth};

const FIRST: [&str; 12] =
    ["Léa", "Camille", "Noël", "Maëlle", "Jean", "Chloé", "Rémi", "Inès", "Hugo", "Zoé", "Loïc", "Anaïs"];
const LAST: [&str; 11] = [
    "Martin", "Le Gall", "Dubois", "Garçon", "Bernard", "Kervella", "Moreau", "Guégan", "Petit", "Le Bihan", "Robert",
];
const BIKES: [(&str, &str, VehicleType); 6] = [
    ("Honda", "CB 500 F", VehicleType::Moto),
    ("Yamaha", "MT-07", VehicleType::Moto),
    ("Peugeot", "Kisbee 50", VehicleType::Moto),
    ("Trek", "Marlin 7", VehicleType::Bicycle),
    ("Super Soco", "TC Max", VehicleType::Electric),
    ("Kawasaki", "Z650", VehicleType::Moto),
];

fn main() {
    let Some(folder) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: seed <new folder>");
        std::process::exit(2);
    };
    if folder.join("local.db").exists() {
        eprintln!("{} already holds a database: choose a new folder.", folder.display());
        std::process::exit(2);
    }
    // Never the real keychain entries, never the real cloud.
    // SAFETY: nothing else runs yet; the process has a single thread at this point.
    unsafe {
        if std::env::var_os("MOTODESK_PROFILE").is_none() {
            std::env::set_var("MOTODESK_PROFILE", "demo");
        }
        std::env::set_var("MOTODESK_FISCAL_NO_KEYCHAIN", "1");
    }

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    let made = runtime.block_on(seed(folder.clone()));
    runtime.shutdown_background();
    match made {
        Ok(count) => println!("{count} customers in {}", folder.display()),
        Err(error) => {
            eprintln!("seeding failed: {error:#}");
            std::process::exit(1);
        }
    }
}

async fn seed(folder: PathBuf) -> anyhow::Result<usize> {
    use rand::RngCore as _;

    let token = local_auth::generate_token();
    let service = local_server::start(folder, token.clone()).await?;
    let client = Client::new(
        Local::new(service.api, &token),
        Cloud::new("http://127.0.0.1:9"),
        TokenStore::in_dir(&service.state.data_dir),
    );

    let mut secret = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    let password = hex::encode(secret);
    let session = local_account::setup(
        &client,
        &SetupPayload { username: "demo".into(), password: password.clone(), confirm: password, force: true },
    )
    .await?;
    client.set_tokens(Some(session.bundle));
    if let Some(key) = session.recovery_key {
        recovery::confirm(&client, &key).await?;
    }

    let mut made = 0;
    for i in 0..137usize {
        let (first, last) = (FIRST[i % FIRST.len()], LAST[(i * 7 + i / 5) % LAST.len()]);
        let created = clients::create(
            &client,
            &ClientInput {
                first_name: (i % 9 != 4).then(|| first.to_string()),
                last_name: last.to_string(),
                phone: (i % 6 != 1).then(|| format!("06 {:02} {:02} {:02} {:02}", i % 97, (i * 7) % 97, (i * 13) % 97, (i * 31) % 97)),
                email: (i % 4 != 2).then(|| format!("{}.{}@example.fr", first.to_lowercase().replace(['é', 'ë', 'ï', 'ö', 'è'], "e"), i)),
                address: (i % 3 == 0).then(|| format!("{} rue des Motos, 35000 Rennes", 1 + i % 80)),
                notes: (i % 11 == 0).then(|| "Préfère être appelé après 18 h.".to_string()),
                credit_limit_cents: (i % 13 == 0).then_some(150_000),
                credit_blocked: Some(i % 29 == 7),
            },
        )
        .await?;
        for v in 0..(i % 4) {
            let (make, model, kind) = BIKES[(i + v) % BIKES.len()];
            clients::create_vehicle(
                &client,
                &created.id,
                &VehicleInput {
                    vehicle_type: kind,
                    make: make.into(),
                    model: model.into(),
                    year: Some(2014 + ((i + v) % 11) as i32),
                    cylinder_capacity: (kind == VehicleType::Moto).then_some(500),
                    plate_number: (kind != VehicleType::Bicycle)
                        .then(|| format!("{}{}-{:03}-{}{}", (b'A' + (i % 26) as u8) as char, (b'A' + (v % 26) as u8) as char, 100 + (i * 3 + v) % 900, (b'A' + ((i / 3) % 26) as u8) as char, (b'A' + ((i + v) % 26) as u8) as char)),
                    odometer_value: Some(((i * 731 + v * 97) % 48_000) as i64),
                    odometer_unit: Some(OdometerUnit::Km),
                    ..Default::default()
                },
            )
            .await?;
        }
        made += 1;
    }

    // Parc VN/VO: vehicles for sale, with their prices.
    for i in 0..14usize {
        let (make, model, kind) = BIKES[i % BIKES.len()];
        let buy = 180_000 + (i as i64 * 73_100) % 620_000;
        clients::create_stock_vehicle(
            &client,
            &VehicleInput {
                vehicle_type: kind,
                make: make.into(),
                model: model.into(),
                year: Some(2018 + (i % 8) as i32),
                cylinder_capacity: (kind == VehicleType::Moto).then_some(650),
                vin_or_serial: (i % 5 != 3).then(|| format!("JH2PC40A{:02}M{:06}", i, 100_123 + i * 7919)),
                plate_number: (i % 3 == 0 && kind != VehicleType::Bicycle).then(|| format!("G{}-{:03}-VO", (b'A' + i as u8) as char, 210 + i * 31)),
                odometer_value: Some(((i * 4_211) % 36_000) as i64),
                odometer_unit: Some(OdometerUnit::Km),
                purchase_price_ht_cents: (i % 6 != 5).then_some(buy),
                selling_price_ttc_cents: (i % 7 != 6).then_some(buy * 3 / 2 + 49_000),
                ..Default::default()
            },
        )
        .await?;
    }

    // The mentions an invoice must carry: without them validation is refused. All made up.
    let profile: settings::SettingsBulk = [
        ("shop.legal_name", serde_json::json!("Garage de démonstration")),
        ("contact.address_line1", serde_json::json!("12 rue des Motos")),
        ("contact.postal_code", serde_json::json!("35000")),
        ("contact.city", serde_json::json!("Rennes")),
        ("legal.siret", serde_json::json!("12345678900012")),
        ("legal.legal_form", serde_json::json!("EI")),
        ("legal.ape_code", serde_json::json!("4540Z")),
        ("legal.vat_number", serde_json::json!("FR12123456789")),
        ("vat.is_subject", serde_json::json!(true)),
        ("invoicing.payment_terms_days", serde_json::json!(30)),
        ("invoicing.late_fee_rate_bp", serde_json::json!(1000)),
        ("invoicing.recovery_indemnity_eur", serde_json::json!(40)),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();
    settings::set_bulk(&client, &profile).await?;

    // A small catalogue: parts at a shelf price, and an hour of labour.
    for (name, brand, sku, unit, price_ttc, stock) in [
        ("Plaquettes de frein avant", Some("Brembo"), "BRMB-PLQ-001", Unit::Piece, 4_990, 8.),
        ("Huile moteur 10W40", Some("Motul"), "MOT-5100-1L", Unit::Liter, 1_499, 24.),
        ("Kit chaîne 520", Some("DID"), "DID-520-KIT", Unit::Piece, 12_900, 3.),
        ("Chambre à air 700x25", Some("Michelin"), "MIC-CAA-700", Unit::Piece, 999, 1.),
        ("Bougie CR8E", Some("NGK"), "NGK-CR8E", Unit::Piece, 890, 0.),
        ("Main d'œuvre atelier", None, "MO-ATELIER", Unit::Hour, 7_200, 0.),
    ] {
        products::create(
            &client,
            &ProductInput {
                name: name.into(),
                brand: brand.map(str::to_string),
                sku: Some(sku.into()),
                unit,
                is_service: unit == Unit::Hour,
                selling_price_ttc_cents: Some(price_ttc),
                vat_rate_bp: 2000,
                stock_quantity: Some(stock),
                low_stock_threshold: (unit != Unit::Hour).then_some(2),
                is_active: true,
                ..Default::default()
            },
        )
        .await?;
    }
    Ok(made)
}
