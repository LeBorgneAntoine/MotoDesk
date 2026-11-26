//! Looking for an update, downloading it and verifying it, against a stand-in for the release
//! server: a newer version signed by a key made for the test.

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use base64::Engine as _;
use minisign::KeyPair;
use motodesk_client::updates::{self, Source, UpdateError};

fn base64(text: String) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

/// A release server on this PC: `/latest.json` announces `version`, `/setup.exe` is its installer.
/// Returns where it listens.
async fn release_server(keys: &KeyPair, version: &str, installer: Vec<u8>, served: Vec<u8>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let comment = format!("timestamp:1790000000\tfile:setup.exe\tversion:{version}");
    let signature = base64(minisign::sign(None, &keys.sk, Cursor::new(&installer), Some(&comment), None).unwrap().to_string());
    let manifest = serde_json::json!({
        "version": version,
        "notes": "### Corrigé\n- Un défaut",
        "pub_date": "2026-11-22T10:00:00Z",
        "platforms": { "windows-x86_64": { "signature": signature, "url": format!("{address}/setup.exe") } },
    })
    .to_string();
    let app = Router::new().route("/latest.json", get(move || async move { manifest })).route("/setup.exe", get(move || async move { served }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

fn source(address: &str, keys: &KeyPair) -> Source {
    Source { endpoint: format!("{address}/latest.json"), public_key: base64(keys.pk.to_box().unwrap().to_string()), insecure: true }
}

#[tokio::test]
async fn a_newer_version_is_found_downloaded_and_verified() {
    let keys = KeyPair::generate_unencrypted_keypair().unwrap();
    let installer: Vec<u8> = b"MZ".iter().copied().chain((0..200_000u32).map(|n| n as u8)).collect();
    let address = release_server(&keys, "0.2.0", installer.clone(), installer.clone()).await;
    let source = source(&address, &keys);

    // The running version is current, or older.
    assert_eq!(updates::check(&source, "0.2.0").await.unwrap(), None);
    let update = updates::check(&source, "0.1.0").await.unwrap().expect("0.2.0 is offered to 0.1.0");
    assert_eq!((update.version.as_str(), update.notes.as_str(), update.date.as_deref()), ("0.2.0", "### Corrigé\n- Un défaut", Some("2026-11-22")));

    let seen = Arc::new(Mutex::new(Vec::new()));
    let progress = seen.clone();
    let downloaded = updates::download(&source, &update, move |done, total| progress.lock().unwrap().push((done, total))).await.unwrap();
    assert_eq!(downloaded, installer);
    let seen = seen.lock().unwrap().clone();
    let size = installer.len() as u64;
    assert_eq!((seen.first(), seen.last()), (Some(&(0, Some(size))), Some(&(size, Some(size)))), "progress runs from nothing to everything");
    assert!(seen.windows(2).all(|pair| pair[0].0 <= pair[1].0));

    // The same release, read by a program that trusts another key: refused.
    let stranger = KeyPair::generate_unencrypted_keypair().unwrap();
    let refused =
        updates::download(&Source { public_key: base64(stranger.pk.to_box().unwrap().to_string()), ..source.clone() }, &update, |_, _| {}).await;
    assert!(matches!(refused, Err(UpdateError::Signature(_))), "{refused:?}");
    // And the official source never talks to a plain HTTP address.
    let official = updates::download(&Source { insecure: false, ..source.clone() }, &update, |_, _| {}).await;
    assert!(matches!(official, Err(UpdateError::Unreachable(_))), "{official:?}");
}

#[tokio::test]
async fn an_installer_that_is_not_the_signed_one_is_never_handed_over() {
    let keys = KeyPair::generate_unencrypted_keypair().unwrap();
    let signed = b"MZ the real installer".to_vec();
    let served = b"MZ something else....".to_vec();
    let address = release_server(&keys, "0.2.0", signed, served).await;
    let source = source(&address, &keys);
    let update = updates::check(&source, "0.1.0").await.unwrap().unwrap();
    let refused = updates::download(&source, &update, |_, _| {}).await;
    assert!(matches!(refused, Err(UpdateError::Signature(_))), "{refused:?}");
    assert_eq!(refused.unwrap_err().message(), "La signature de la mise à jour est invalide : elle n'a pas été installée.");
}

#[tokio::test]
async fn a_server_with_no_release_or_out_of_reach_says_so() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let app =
        Router::new().route("/empty.json", get(|| async { StatusCode::NO_CONTENT })).route("/down.json", get(|| async { StatusCode::BAD_GATEWAY }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let at = |path: &str| Source { endpoint: format!("{address}/{path}"), public_key: updates::PUBLIC_KEY.into(), insecure: true };

    assert_eq!(updates::check(&at("latest.json"), "0.1.0").await, Err(UpdateError::NoRelease), "nothing published yet");
    assert_eq!(updates::check(&at("empty.json"), "0.1.0").await, Ok(None), "the server says there is nothing newer");
    assert!(matches!(updates::check(&at("down.json"), "0.1.0").await, Err(UpdateError::Unreachable(_))));
    // Nobody listens there.
    let nowhere = Source { endpoint: "http://127.0.0.1:9/latest.json".into(), public_key: updates::PUBLIC_KEY.into(), insecure: true };
    assert!(matches!(updates::check(&nowhere, "0.1.0").await, Err(UpdateError::Unreachable(_))));
    // The official source is never asked over plain HTTP.
    let plain = Source { insecure: false, ..at("latest.json") };
    assert!(matches!(updates::check(&plain, "0.1.0").await, Err(UpdateError::Manifest(_))));
}
