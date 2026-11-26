//! A stand-in for the release server, to look at the update screens without a published
//! version: it announces version 9.9.9 with release notes, and serves a stand-in installer
//! signed with a key made on the spot. Nothing it serves is a real installer, and a development
//! build never runs what it downloads.
//!
//! ```powershell
//! cargo run -p motodesk-gpui --example release_server      # prints the two variables to set
//! ```

use std::io::Cursor;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::CONTENT_LENGTH;
use axum::response::Response;
use axum::routing::get;
use base64::Engine as _;
use minisign::KeyPair;

const VERSION: &str = "9.9.9";
const ADDRESS: &str = "127.0.0.1:8792";
const NOTES: &str = "### Ajouté\n\n- **Impression directe** des factures, sans passer par un PDF.\n- Un tableau de bord par technicien.\n\n\
                     ### Corrigé\n\n- La date d'échéance d'un avoir.\n- Une recherche de client plus rapide sur les grands fichiers.\n\n\
                     ### Modifié\n\n- Les modèles de documents s'ouvrent plus vite.";

fn base64(text: String) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

#[tokio::main]
async fn main() {
    let keys = KeyPair::generate_unencrypted_keypair().expect("a key pair");
    // Three megabytes that start like a Windows program and are not one.
    let installer: Vec<u8> = b"MZ".iter().copied().chain((0..3_000_000u32).map(|n| (n % 251) as u8)).collect();
    let comment = format!("timestamp:1790000000\tfile:setup.exe\tversion:{VERSION}");
    let signature = base64(minisign::sign(None, &keys.sk, Cursor::new(&installer), Some(&comment), None).expect("a signature").to_string());
    let manifest = serde_json::json!({
        "version": VERSION,
        "notes": NOTES,
        "pub_date": "2026-11-22T10:00:00Z",
        "platforms": { "windows-x86_64": { "signature": signature, "url": format!("http://{ADDRESS}/setup.exe") } },
    })
    .to_string();

    // Served a piece at a time, slowly enough for the progress to be seen.
    let download = move || {
        let installer = installer.clone();
        async move {
            let size = installer.len();
            let pieces = futures_util::stream::unfold((installer, 0usize), |(installer, at)| async move {
                if at >= installer.len() {
                    return None;
                }
                tokio::time::sleep(Duration::from_millis(120)).await;
                let end = (at + 100_000).min(installer.len());
                Some((Ok::<_, std::io::Error>(installer[at..end].to_vec()), (installer, end)))
            });
            Response::builder().header(CONTENT_LENGTH, size).body(Body::from_stream(pieces)).expect("an answer")
        }
    };
    let app = Router::new().route("/latest.json", get(move || async move { manifest })).route("/setup.exe", get(download));

    println!("Stand-in release server on http://{ADDRESS} (version {VERSION}). In another terminal:");
    println!("$env:MOTODESK_DEV_UPDATE_ENDPOINT = \"http://{ADDRESS}/latest.json\"");
    println!("$env:MOTODESK_DEV_UPDATE_KEY = \"{}\"", base64(keys.pk.to_box().expect("a public key").to_string()));
    let listener = tokio::net::TcpListener::bind(ADDRESS).await.expect("the address is free");
    axum::serve(listener, app).await.expect("the server runs");
}
