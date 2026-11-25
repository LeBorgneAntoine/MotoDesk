//! Three PCs of one shop, real HTTP between them: one is removed, the others carry on.

use super::*;
use crate::local_server::mesh::catchup;
use crate::local_server::shared_folder::{self as shared, Actor};
use crate::local_server::test_support::shop;
use chrono::Duration;
use motodesk_core::entity::EntityType;
use motodesk_core::sync::{ChangeLogEntry, OpKind};
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "mot de passe admin";
const NEW_PASSWORD: &str = "un tout autre mot de passe";

async fn serve(state: &SharedState) -> PeerInfo {
    let app = crate::local_server::routes::mesh::lan_router().with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    PeerInfo { device_id: state.device_id().await.unwrap(), host: addr.ip().to_string(), port: addr.port() }
}

struct Shop {
    a: SharedState,
    peer_a: PeerInfo,
    b: SharedState,
    c: SharedState,
    admin: String,
}

/// A created the network; B and C joined it; A knows of all three.
async fn three_pcs() -> Shop {
    let a = shop().await;
    let bundle = local_accounts::setup(&a, "camille", PASSWORD, PASSWORD).await.unwrap();
    let admin = bundle["user"]["id"].as_str().unwrap().to_string();
    touch_self(&a).await.unwrap();
    let peer_a = serve(&a).await;
    let (b, c) = (shop().await, shop().await);
    for pc in [&b, &c] {
        join::join_network(pc, "camille", PASSWORD, std::slice::from_ref(&peer_a)).await.unwrap();
        catchup::exchange_accounts(pc, &peer_a).await.unwrap();
    }
    Shop { a, peer_a, b, c, admin }
}

async fn removal(s: &Shop, which: &SharedState) -> LocalResult<()> {
    let id = which.device_id().await.unwrap().to_string();
    let input = Removal { password: PASSWORD.into(), new_password: NEW_PASSWORD.into(), confirm: NEW_PASSWORD.into() };
    remove(&s.a, &s.admin, &id, input).await
}

fn client(device: Uuid, seq: i64, last_name: &str) -> ChangeLogEntry {
    let id = Uuid::new_v4();
    let payload = json!({ "id": id.to_string(), "last_name": last_name });
    ChangeLogEntry {
        id: Uuid::new_v4(),
        device_id: device,
        seq,
        entity_type: EntityType::Client,
        entity_id: id,
        op: OpKind::Insert,
        field_meta: crate::local_server::sync_log::field_meta_for_patch(&Default::default(), &payload, device),
        payload,
        created_at: chrono::Utc::now() + Duration::minutes(1),
        synced_to_cloud: false,
        relayed: false,
    }
}

async fn names(state: &SharedState) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT last_name FROM clients ORDER BY last_name")
        .fetch_all(&state.db)
        .await
        .unwrap();
    rows.into_iter().map(|(n,)| n).collect()
}

#[tokio::test]
async fn every_pc_ends_up_on_the_list() {
    let s = three_pcs().await;
    let list = all(&s.a.db).await.unwrap();
    assert_eq!(list.len(), 3, "{list:?}");
    assert!(list.iter().all(|d| d.removed_at.is_none() && !d.name.is_empty()));
    assert_eq!(key_epoch(&s.b.db).await.unwrap(), 1);
}

#[tokio::test]
async fn a_removed_pc_is_shut_out_and_the_others_come_back_with_the_new_password() {
    let s = three_pcs().await;
    // B records something while it has not heard of the removal.
    mesh::apply_batch(&s.b, vec![client(Uuid::new_v4(), 1, "Moreau")]).await.unwrap();
    removal(&s, &s.c).await.unwrap();
    mesh::apply_batch(&s.a, vec![client(Uuid::new_v4(), 1, "Girard")]).await.unwrap();

    assert_eq!(key_epoch(&s.a.db).await.unwrap(), 2);
    for pc in [&s.b, &s.c] {
        assert!(catchup::pull_from(pc, &s.peer_a).await.is_err(), "the old key opens nothing");
        assert!(catchup::exchange_accounts(pc, &s.peer_a).await.is_err());
    }
    // The old password, which the removed PC knows, gets nothing any more.
    let refused = reconnect(&s.c, "camille", PASSWORD, std::slice::from_ref(&s.peer_a)).await;
    assert!(matches!(refused, Err(LocalError::InvalidCredentials)), "{refused:?}");

    reconnect(&s.b, "camille", NEW_PASSWORD, std::slice::from_ref(&s.peer_a)).await.unwrap();
    assert_eq!(key_epoch(&s.b.db).await.unwrap(), 2);
    assert_eq!(*s.b.mesh_secret.read().await, *s.a.mesh_secret.read().await);
    assert_ne!(*s.c.mesh_secret.read().await, *s.a.mesh_secret.read().await);

    catchup::pull_from(&s.b, &s.peer_a).await.unwrap();
    let peer_b = serve(&s.b).await;
    catchup::pull_from(&s.a, &peer_b).await.unwrap();
    assert_eq!(names(&s.a).await, vec!["Girard", "Moreau"], "nothing recorded on B was lost");
    assert_eq!(names(&s.b).await, vec!["Girard", "Moreau"]);
    assert!(names(&s.c).await.is_empty());

    catchup::exchange_accounts(&s.b, &s.peer_a).await.unwrap();
    let c_id = s.c.device_id().await.unwrap().to_string();
    let on_b = all(&s.b.db).await.unwrap();
    assert!(on_b.iter().find(|d| d.device_id == c_id).unwrap().removed_at.is_some());
}

#[tokio::test]
async fn removal_keeps_the_administrator_signed_in_and_changes_their_password() {
    let s = three_pcs().await;
    removal(&s, &s.c).await.unwrap();
    assert!(local_accounts::session_user(&s.a.db).await.unwrap().is_some());
    assert!(local_accounts::login(&s.a, "camille", PASSWORD).await.is_err());
    assert!(local_accounts::login(&s.a, "camille", NEW_PASSWORD).await.is_ok());
}

#[tokio::test]
async fn removal_asks_for_the_password_a_new_one_and_another_pc() {
    let s = three_pcs().await;
    let me = s.a.device_id().await.unwrap().to_string();
    let c = s.c.device_id().await.unwrap().to_string();
    let input = |password: &str, new: &str| Removal { password: password.into(), new_password: new.into(), confirm: new.into() };

    assert!(remove(&s.a, &s.admin, &me, input(PASSWORD, NEW_PASSWORD)).await.is_err(), "not oneself");
    assert!(remove(&s.a, &s.admin, &c, input("faux", NEW_PASSWORD)).await.is_err(), "wrong password");
    assert!(remove(&s.a, &s.admin, &c, input(PASSWORD, PASSWORD)).await.is_err(), "same password");
    assert!(remove(&s.a, &s.admin, "inconnu", input(PASSWORD, NEW_PASSWORD)).await.is_err());
    assert_eq!(key_epoch(&s.a.db).await.unwrap(), 1, "nothing changed");

    removal(&s, &s.c).await.unwrap();
    assert!(remove(&s.a, &s.admin, &c, input(NEW_PASSWORD, "encore un autre")).await.is_err(), "already removed");
}

#[tokio::test]
async fn shared_files_stay_readable_through_a_change_of_key() {
    let s = three_pcs().await;
    let who = Actor { user_id: s.admin.clone(), name: "Camille".into(), is_admin: true };
    let folder = shared::create_folder(&s.a, &who, None, "Compta", "admins").await.unwrap();
    let data = b"bilan de l'annee".to_vec();
    let start = shared::begin_upload(&s.a, &who, &folder.id, "bilan.txt", data.len() as u64, None).await.unwrap();
    shared::put_chunk(&s.a, &who, &start.file.id, 0, &data).await.unwrap();
    shared::finish_upload(&s.a, &who, &start.file.id).await.unwrap();
    shared::replication::sync_with(&s.b, &s.peer_a).await.unwrap();

    let read = |pc: SharedState, who: Actor, file: String| async move {
        let download = shared::open_download(&pc, &who, &file).await.unwrap();
        download.read_chunk(&pc, 0).await.unwrap()
    };
    let again = |w: &Actor| Actor { user_id: w.user_id.clone(), name: w.name.clone(), is_admin: true };

    removal(&s, &s.c).await.unwrap();
    assert_eq!(read(s.a.clone(), again(&who), start.file.id.clone()).await, data);
    reconnect(&s.b, "camille", NEW_PASSWORD, std::slice::from_ref(&s.peer_a)).await.unwrap();
    assert_eq!(read(s.b.clone(), again(&who), start.file.id.clone()).await, data);
}

#[tokio::test]
async fn a_pc_that_lost_its_key_gets_it_back_the_same_way() {
    let s = three_pcs().await;
    *s.b.mesh_secret.write().await = None;
    assert!(mesh::needs_reconnect(&s.b).await);
    reconnect(&s.b, "camille", PASSWORD, std::slice::from_ref(&s.peer_a)).await.unwrap();
    assert_eq!(*s.b.mesh_secret.read().await, *s.a.mesh_secret.read().await);
}

#[test]
fn a_key_is_named_without_being_given_away() {
    assert_eq!(key_id("secret"), key_id("secret"));
    assert_ne!(key_id("secret"), key_id("secreu"));
    assert_eq!(key_id("secret").len(), 16);
    assert!(!key_id("secret").contains("secret"));
}
