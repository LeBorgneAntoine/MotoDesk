//! Two PCs, a real HTTP server between them: joining the network with an account's credentials,
//! and catching up. No mDNS (the test hands the join the address), everything else is the real
//! code path.

use chrono::Duration;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, OpKind};
use serde_json::json;
use uuid::Uuid;

use crate::local_server::error::LocalError;
use crate::local_server::local_accounts::{self, status};
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
async fn a_new_pc_joins_with_an_employees_credentials_and_catches_up() {
    let (a, peer_a, _) = network_of_one().await;
    let b = shop().await;
    assert!(status(&b.db).await.unwrap().can_setup, "B has never been part of a network");

    // B knows a username and a password, nothing else.
    let bundle = join::join_network(&b, "lea", "mot de passe lea", std::slice::from_ref(&peer_a))
        .await
        .expect("the PC joins");
    assert_eq!(bundle["user"]["username"], "lea");
    assert_eq!(bundle["user"]["role"], "employee");

    // It is a member now: same company, same secret, both accounts, signed in, and A is a peer.
    let s = status(&b.db).await.unwrap();
    assert!(s.standalone && s.has_account && !s.can_setup);
    assert_eq!(*b.company_id.read().await, *a.company_id.read().await);
    assert_eq!(*b.mesh_secret.read().await, *a.mesh_secret.read().await);
    assert_eq!(local_accounts::all_accounts(&b.db).await.unwrap().len(), 2);
    assert!(b.peers.read().await.iter().any(|p| p.port == peer_a.port));
    assert!(!crate::local_server::auth_guard::is_admin(&b).await, "an employee is not an admin");

    // The data comes with a catch-up.
    assert!(clients(&b).await.is_empty());
    let received = catchup::pull_from(&b, &peer_a).await.unwrap();
    assert_eq!(received, 2);
    assert_eq!(clients(&b).await, vec!["Bernard", "Moreau"]);

    // Having joined, B signs in on its own, with no PC on.
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

    catchup::exchange_accounts(&b, &peer_a).await.unwrap();
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
async fn the_lan_routes_refuse_what_is_not_signed_with_the_networks_secret() {
    let (_a, peer_a, _) = network_of_one().await;
    let http = reqwest::Client::new();
    let url = |path: &str| format!("http://{}:{}{path}", peer_a.host, peer_a.port);

    // Unsigned.
    let r = http.post(url("/mesh/pull")).json(&json!({ "vv": {}, "limit": 10 })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = http.post(url("/mesh/accounts")).json(&json!({ "accounts": [] })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // Signed with another network's secret.
    let fields = json!({ "vv": {}, "limit": 10 });
    let (nonce, ts, sig) = crate::local_server::mesh_auth::sign_value("some other secret", &fields);
    let r = http
        .post(url("/mesh/pull"))
        .json(&json!({ "vv": {}, "limit": 10, "nonce": nonce, "ts": ts, "sig": sig }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 401);
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
