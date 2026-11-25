//! Two PCs, a real HTTP server between them: joining the network with an account's credentials,
//! and catching up. No mDNS (the test hands the join the address), everything else is the real
//! code path.

use std::sync::{Arc, Mutex};

use chrono::Duration;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, OpKind};
use serde_json::json;
use uuid::Uuid;

use crate::local_server::error::LocalError;
use crate::local_server::local_accounts::{self, status};
use crate::local_server::mesh::transport::{self, Kind};
use crate::local_server::mesh::{self, catchup, join, PeerInfo};
use crate::local_server::state::SharedState;
use crate::local_server::sync_log::field_meta_for_patch;
use crate::local_server::test_support::shop;

/// Serve a PC's LAN routes on a free port.
pub(super) async fn serve(state: &SharedState) -> PeerInfo {
    let app = crate::local_server::routes::mesh::lan_router().with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    PeerInfo { device_id: state.device_id().await.unwrap(), host: addr.ip().to_string(), port: addr.port() }
}

fn client_op(device: Uuid, seq: i64, last_name: &str) -> ChangeLogEntry {
    let id = Uuid::new_v4();
    let payload = json!({ "id": id.to_string(), "last_name": last_name });
    ChangeLogEntry {
        id: Uuid::new_v4(),
        device_id: device,
        seq,
        entity_type: EntityType::Client,
        entity_id: id,
        op: OpKind::Insert,
        field_meta: field_meta_for_patch(&Default::default(), &payload, device),
        payload,
        created_at: chrono::Utc::now() + Duration::minutes(1),
        synced_to_cloud: false,
        relayed: false,
    }
}

async fn clients(state: &SharedState) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT last_name FROM clients ORDER BY last_name")
        .fetch_all(&state.db)
        .await
        .unwrap();
    rows.into_iter().map(|(n,)| n).collect()
}

/// A network of one PC (A) with an administrator, an employee account, and some data.
async fn network_of_one() -> (SharedState, PeerInfo, Uuid) {
    let a = shop().await;
    local_accounts::setup(&a, "camille", "mot de passe admin", "mot de passe admin").await.unwrap();
    let lea = local_accounts::new_account("lea", "mot de passe lea", "employee", Some("Léa".into()))
        .await
        .unwrap();
    local_accounts::merge_accounts(&a.db, vec![lea]).await.unwrap();
    let device = Uuid::new_v4();
    mesh::apply_batch(&a, vec![client_op(device, 1, "Bernard"), client_op(device, 2, "Moreau")])
        .await
        .unwrap();
    let peer = serve(&a).await;
    (a, peer, device)
}

#[tokio::test]
async fn a_new_pc_joins_with_an_administrators_credentials_and_catches_up() {
    let (a, peer_a, _) = network_of_one().await;
    let b = shop().await;
    assert!(status(&b.db).await.unwrap().can_setup, "B has never been part of a network");

    // An employee's password is right, and not enough to add a PC.
    let refused = join::join_network(&b, "lea", "mot de passe lea", std::slice::from_ref(&peer_a)).await;
    assert!(matches!(refused, Err(LocalError::AdminRequiredToJoin)), "{refused:?}");
    assert!(status(&b.db).await.unwrap().can_setup, "nothing was handed over");
    assert!(b.mesh_secret.read().await.is_none());

    // B knows an administrator's username and password, nothing else.
    let bundle = join::join_network(&b, "camille", "mot de passe admin", std::slice::from_ref(&peer_a))
        .await
        .expect("the PC joins");
    assert_eq!(bundle["user"]["username"], "camille");
    assert_eq!(bundle["user"]["role"], "admin");

    // It is a member now: same company, same secret, both accounts, signed in, and A is a peer.
    let s = status(&b.db).await.unwrap();
    assert!(s.standalone && s.has_account && !s.can_setup);
    assert_eq!(*b.company_id.read().await, *a.company_id.read().await);
    assert_eq!(*b.mesh_secret.read().await, *a.mesh_secret.read().await);
    assert_eq!(local_accounts::all_accounts(&b.db).await.unwrap().len(), 2);
    let key = crate::local_server::recovery_key::stored(&a.db).await.unwrap();
    assert!(key.is_some());
    assert_eq!(crate::local_server::recovery_key::stored(&b.db).await.unwrap(), key, "the key works on B too");
    assert!(b.peers.read().await.iter().any(|p| p.port == peer_a.port));
    // The data comes with a catch-up.
    assert!(clients(&b).await.is_empty());
    let received = catchup::pull_from(&b, &peer_a).await.unwrap();
    assert_eq!(received, 2);
    assert_eq!(clients(&b).await, vec!["Bernard", "Moreau"]);

    // Having joined, B signs its employees in on its own, with no PC on.
    sqlx::query("DELETE FROM cached_session").execute(&b.db).await.unwrap();
    assert!(local_accounts::login(&b, "lea", "mot de passe lea").await.is_ok());
    assert!(local_accounts::login(&b, "lea", "pas le bon").await.is_err());
}

#[tokio::test]
async fn catching_up_again_brings_only_what_is_new() {
    let (a, peer_a, device) = network_of_one().await;
    let b = shop().await;
    join::join_network(&b, "camille", "mot de passe admin", std::slice::from_ref(&peer_a)).await.unwrap();
    assert_eq!(catchup::pull_from(&b, &peer_a).await.unwrap(), 2);
    assert_eq!(catchup::pull_from(&b, &peer_a).await.unwrap(), 0, "nothing new");

    mesh::apply_batch(&a, vec![client_op(device, 3, "Girard")]).await.unwrap();
    assert_eq!(catchup::pull_from(&b, &peer_a).await.unwrap(), 1, "only the new change");
    assert_eq!(clients(&b).await, vec!["Bernard", "Girard", "Moreau"]);
}

#[tokio::test]
async fn accounts_travel_in_both_directions() {
    let (a, peer_a, _) = network_of_one().await;
    let b = shop().await;
    join::join_network(&b, "camille", "mot de passe admin", std::slice::from_ref(&peer_a)).await.unwrap();

    // An account created later on A reaches B; one created on B reaches A.
    let on_a = local_accounts::new_account("ines", "mot de passe ines", "employee", None).await.unwrap();
    local_accounts::merge_accounts(&a.db, vec![on_a]).await.unwrap();
    let on_b = local_accounts::new_account("hugo", "mot de passe hugo", "employee", None).await.unwrap();
    local_accounts::merge_accounts(&b.db, vec![on_b]).await.unwrap();

    crate::local_server::recovery_key::issue(&a).await.unwrap();
    catchup::exchange_accounts(&b, &peer_a).await.unwrap();
    assert_eq!(
        crate::local_server::recovery_key::stored(&b.db).await.unwrap(),
        crate::local_server::recovery_key::stored(&a.db).await.unwrap(),
        "a new key replaces the old one everywhere"
    );
    assert!(local_accounts::find_account(&b.db, "ines").await.unwrap().is_some());
    assert!(local_accounts::find_account(&a.db, "hugo").await.unwrap().is_some());
}

#[tokio::test]
async fn a_wrong_password_an_unknown_name_and_a_silent_network_do_not_get_in() {
    let (_a, peer_a, _) = network_of_one().await;

    let wrong = shop().await;
    let r = join::join_network(&wrong, "lea", "pas le bon", std::slice::from_ref(&peer_a)).await;
    assert!(matches!(r, Err(LocalError::InvalidCredentials)), "{r:?}");

    let unknown = shop().await;
    let r = join::join_network(&unknown, "personne", "mot de passe lea", std::slice::from_ref(&peer_a)).await;
    assert!(matches!(r, Err(LocalError::InvalidCredentials)), "{r:?}");
    for s in [&wrong, &unknown] {
        assert!(status(&s.db).await.unwrap().can_setup, "a refused PC joined nothing");
    }

    // No PC of the network on: the caveat. A PC that has never joined cannot sign in.
    let alone = shop().await;
    let nobody = PeerInfo { device_id: Uuid::new_v4(), host: "127.0.0.1".into(), port: 9 };
    let r = join::join_network(&alone, "lea", "mot de passe lea", &[nobody]).await;
    assert!(matches!(r, Err(LocalError::NoNetwork)), "{r:?}");
    let r = join::join_network(&alone, "lea", "mot de passe lea", &[]).await;
    assert!(matches!(r, Err(LocalError::NoNetwork)), "{r:?}");
}

#[tokio::test]
async fn guessing_a_password_locks_the_username_for_a_while() {
    let (_a, peer_a, _) = network_of_one().await;
    let guesser = shop().await;
    for _ in 0..5 {
        let r = join::join_network(&guesser, "lea", "essai", std::slice::from_ref(&peer_a)).await;
        assert!(matches!(r, Err(LocalError::InvalidCredentials)), "{r:?}");
    }
    // Even the right password is refused now.
    let r = join::join_network(&guesser, "lea", "mot de passe lea", std::slice::from_ref(&peer_a)).await;
    assert!(matches!(r, Err(LocalError::TooManyAttempts)), "{r:?}");
}

/// Handshakes opened side by side, then proved one after another: only the first five proofs
/// are looked at, and after that not even the right password gets through.
#[tokio::test]
async fn opening_many_handshakes_at_once_does_not_get_past_the_lock() {
    use sha2::Sha256;
    use srp::groups::G_2048;

    let (a, _peer, _) = network_of_one().await;
    let client = srp::client::SrpClient::<Sha256>::new(&G_2048);
    let ephemeral = [7u8; 64];
    let start = || join::JoinStart {
        username: "lea".into(),
        a_pub: hex::encode(client.compute_public_ephemeral(&ephemeral)),
    };

    let mut open = Vec::new();
    for _ in 0..8 {
        open.push(join::start(&a, start()).await.unwrap());
    }
    let right = open.pop().unwrap();
    for (i, reply) in open.into_iter().enumerate() {
        let guess = join::JoinFinish { session: reply.session, m1: "00".repeat(32) };
        let refused = join::finish(&a, guess).await.unwrap_err();
        if i < 5 {
            assert!(matches!(refused, LocalError::InvalidCredentials), "{i}: {refused:?}");
        } else {
            assert!(matches!(refused, LocalError::TooManyAttempts), "{i}: {refused:?}");
        }
    }

    let salt = hex::decode(&right.salt).unwrap();
    let secret = local_accounts::derive_secret("mot de passe lea", &salt).unwrap();
    let proof = client
        .process_reply(&ephemeral, b"lea", &secret, &salt, &hex::decode(&right.b_pub).unwrap())
        .unwrap();
    let answer = join::finish(&a, join::JoinFinish { session: right.session, m1: hex::encode(proof.proof()) }).await;
    assert!(matches!(answer, Err(LocalError::TooManyAttempts)));
}

#[tokio::test]
async fn an_accented_name_joins_however_it_is_typed() {
    let (a, peer_a, _) = network_of_one().await;
    let elodie = local_accounts::new_account("Élodie", "mot de passe elodie", "admin", None)
        .await
        .unwrap();
    local_accounts::merge_accounts(&a.db, vec![elodie]).await.unwrap();

    let b = shop().await;
    let bundle = join::join_network(&b, "élodie", "mot de passe elodie", std::slice::from_ref(&peer_a))
        .await
        .expect("the PC joins");
    assert_eq!(bundle["user"]["username"], "Élodie");
}

#[tokio::test]
async fn a_pc_that_is_not_part_of_a_network_cannot_be_joined() {
    let fresh = shop().await;
    let peer = serve(&fresh).await;
    let joiner = shop().await;
    let r = join::join_network(&joiner, "lea", "mot de passe lea", &[peer]).await;
    assert!(matches!(r, Err(LocalError::NoNetwork)), "{r:?}");
}

#[tokio::test]
async fn a_stranger_who_does_not_hold_the_account_is_not_believed() {
    use axum::routing::post;
    use axum::Json;

    // Advertises itself as a PC of the network, and answers the handshake with made-up values.
    let app = axum::Router::new()
        .route(
            "/mesh/join/start",
            post(|| async {
                Json(json!({ "session": "x", "salt": "00".repeat(16), "b_pub": "05".repeat(256) }))
            }),
        )
        .route(
            "/mesh/join/finish",
            post(|| async { Json(json!({ "m2": "00".repeat(32), "sealed": "00".repeat(80) })) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let fake = PeerInfo { device_id: Uuid::new_v4(), host: addr.ip().to_string(), port: addr.port() };

    let b = shop().await;
    let r = join::join_network(&b, "lea", "mot de passe lea", &[fake]).await;
    assert!(matches!(r, Err(LocalError::NoNetwork)), "{r:?}");
    assert!(status(&b.db).await.unwrap().can_setup, "nothing from the stranger was adopted");
    assert!(b.mesh_secret.read().await.is_none());
}

#[tokio::test]
async fn the_lan_routes_refuse_what_is_not_sealed_with_the_networks_secret() {
    let (_a, peer_a, _) = network_of_one().await;
    let http = reqwest::Client::new();
    let url = |path: &str| format!("http://{}:{}{path}", peer_a.host, peer_a.port);

    // Unsigned.
    let r = http.post(url("/mesh/pull")).json(&json!({ "vv": {}, "limit": 10 })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = http.post(url("/mesh/accounts")).json(&json!({ "accounts": [] })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // Sealed with another network's secret.
    let fields = json!({ "vv": {}, "limit": 10 }).as_object().unwrap().clone();
    let foreign = transport::seal("some other secret", Kind::Request, fields.clone());
    let r = http.post(url("/mesh/pull")).json(&foreign).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // Sealed properly, then sent twice: the second is refused.
    let secret = catchup::secret_of(&_a).await.unwrap();
    let genuine = transport::seal(&secret, Kind::Request, fields);
    let r = http.post(url("/mesh/pull")).json(&genuine).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let r = http.post(url("/mesh/pull")).json(&genuine).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
}

/// Everything two PCs say to each other, as someone on the Wi-Fi would capture it.
#[tokio::test]
async fn nothing_readable_crosses_the_network() {
    let (a, _plain, device) = network_of_one().await;
    let seen = Arc::new(Mutex::new(Vec::<u8>::new()));
    let tap = seen.clone();
    let app = crate::local_server::routes::mesh::lan_router().with_state(a.clone()).layer(
        axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
            let tap = tap.clone();
            async move {
                let path = req.uri().path().to_string();
                let (parts, body) = req.into_parts();
                let sent = axum::body::to_bytes(body, usize::MAX).await.unwrap();
                let res = next.run(axum::extract::Request::from_parts(parts, axum::body::Body::from(sent.clone()))).await;
                let (head, body) = res.into_parts();
                let answered = axum::body::to_bytes(body, usize::MAX).await.unwrap();
                // Joining has its own sealing and is checked by its own tests.
                if !path.starts_with("/mesh/join") {
                    let mut tap = tap.lock().unwrap();
                    tap.extend_from_slice(&sent);
                    tap.extend_from_slice(&answered);
                }
                axum::response::Response::from_parts(head, axum::body::Body::from(answered))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let peer = PeerInfo { device_id: a.device_id().await.unwrap(), host: addr.ip().to_string(), port: addr.port() };

    let b = shop().await;
    join::join_network(&b, "camille", "mot de passe admin", std::slice::from_ref(&peer)).await.unwrap();
    catchup::exchange_accounts(&b, &peer).await.unwrap();
    assert_eq!(catchup::pull_from(&b, &peer).await.unwrap(), 2);
    mesh::apply_batch(&b, vec![client_op(device, 3, "Girard")]).await.unwrap();
    let ops = crate::local_server::sync_log::ops_after(&b.db, device, 2, 10).await.unwrap();
    mesh::broadcast_ops_to_peers(&b, std::slice::from_ref(&peer), ops).await;
    assert!(clients(&a).await.contains(&"Girard".to_string()), "the push got through");

    let wire = String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
    assert!(wire.len() > 500, "the exchanges were captured");
    for word in ["Bernard", "Moreau", "Girard", "argon2", "camille", "password_hash"] {
        assert!(!wire.contains(word), "{word} crossed the network in clear");
    }
}

#[tokio::test]
async fn a_network_pc_pushes_from_a_cursor_not_everything_again() {
    let (a, _peer, device) = network_of_one().await;
    let (first, last) = crate::local_server::sync_log::unpushed_lan_ops(&a.db, 200).await.unwrap();
    assert_eq!(first.len(), 2);
    crate::local_server::sync_log::mark_lan_pushed(&a.db, last).await.unwrap();
    let (again, _) = crate::local_server::sync_log::unpushed_lan_ops(&a.db, 200).await.unwrap();
    assert!(again.is_empty(), "what was pushed is not pushed again");

    mesh::apply_batch(&a, vec![client_op(device, 3, "Girard")]).await.unwrap();
    let (next, _) = crate::local_server::sync_log::unpushed_lan_ops(&a.db, 200).await.unwrap();
    assert_eq!(next.len(), 1, "only the new change");
}
