//! Two PCs of one shop, a real HTTP server between them: what one adds, renames, hides or deletes
//! reaches the other, byte for byte, and nothing forged or half-copied is ever shown.

use super::*;
use crate::local_server::shared_folder::crypto::CHUNK_BYTES;
use crate::local_server::shared_folder::{self as shared, Actor, FilePatch, FolderPatch};
use crate::local_server::test_support::shop;

const SECRET: &str = "the-network-secret-of-this-shop";

fn admin() -> Actor {
    Actor { user_id: "admin-1".into(), name: "Alice".into(), is_admin: true }
}
fn employee() -> Actor {
    Actor { user_id: "emp-1".into(), name: "Bob".into(), is_admin: false }
}

/// A PC of the network: its own database, the shop's secret, in network mode.
async fn pc() -> SharedState {
    let state = shop().await;
    *state.mesh_secret.write().await = Some(SECRET.into());
    sqlx::query("UPDATE sync_state SET standalone = 1 WHERE id = 1").execute(&state.db).await.unwrap();
    state
}

/// Serve a PC's LAN routes on a free port; the way other PCs reach it.
async fn serve(state: &SharedState) -> PeerInfo {
    let app = crate::local_server::routes::mesh::lan_router().with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    PeerInfo { device_id: state.device_id().await.unwrap(), host: addr.ip().to_string(), port: addr.port() }
}

fn body(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + i / 7) % 251) as u8).collect()
}

async fn folder(state: &SharedState, parent: Option<&str>, name: &str, visibility: &str) -> shared::FolderView {
    shared::create_folder(state, &admin(), parent, name, visibility).await.unwrap()
}

async fn upload(state: &SharedState, folder: &str, name: &str, data: &[u8]) -> shared::FileView {
    let start = shared::begin_upload(state, &admin(), folder, name, data.len() as u64, None).await.unwrap();
    for (i, chunk) in data.chunks(CHUNK_BYTES).enumerate() {
        shared::put_chunk(state, &admin(), &start.file.id, i as u32, chunk).await.unwrap();
    }
    shared::finish_upload(state, &admin(), &start.file.id).await.unwrap()
}

async fn read_all(state: &SharedState, who: &Actor, file_id: &str) -> LocalResult<Vec<u8>> {
    let dl = shared::open_download(state, who, file_id).await?;
    let mut out = Vec::new();
    for i in 0..dl.chunk_count() {
        out.extend(dl.read_chunk(state, i).await?);
    }
    Ok(out)
}

async fn names(state: &SharedState, who: &Actor, folder: Option<&str>) -> (Vec<String>, Vec<String>) {
    let l = shared::browse(state, who, folder).await.unwrap();
    (l.folders.into_iter().map(|f| f.name).collect(), l.files.into_iter().map(|f| f.name).collect())
}

async fn count(state: &SharedState, sql: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql).fetch_one(&state.db).await.unwrap();
    n
}

#[tokio::test]
async fn a_file_added_on_one_pc_reaches_the_other_byte_for_byte() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let f = folder(&a, None, "Comptabilité", "admins").await;
    let data = body(CHUNK_BYTES * 2 + 777);
    let file = upload(&a, &f.id, "Bilan 2025.pdf", &data).await;

    let stats = sync_with(&b, &peer_a).await.unwrap();
    assert_eq!((stats.folders, stats.files, stats.chunks), (1, 1, 3));
    assert_eq!(names(&b, &admin(), None).await.0, vec!["Comptabilité"]);
    assert_eq!(read_all(&b, &admin(), &file.id).await.unwrap(), data);
}

#[tokio::test]
async fn a_pc_that_already_matches_asks_for_nothing_more() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let f = folder(&a, None, "Docs", "admins").await;
    upload(&a, &f.id, "a.txt", b"hello").await;
    assert!(sync_with(&b, &peer_a).await.unwrap().changed());
    assert!(!sync_with(&b, &peer_a).await.unwrap().changed(), "the second round is empty");
    assert_eq!(digest(&local_summary(&a).await.unwrap()), digest(&local_summary(&b).await.unwrap()));
}

#[tokio::test]
async fn a_file_is_not_shown_until_all_of_its_content_has_arrived() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let f = folder(&a, None, "Docs", "admins").await;
    let data = body(CHUNK_BYTES * 3);
    let file = upload(&a, &f.id, "gros.bin", &data).await;

    // A first round that only has time for one chunk.
    let first = sync_with_budget(&b, &peer_a, 1).await.unwrap();
    assert_eq!(first.chunks, 1);
    assert!(names(&b, &admin(), Some(&f.id)).await.1.is_empty(), "not listed while incomplete");
    assert!(matches!(shared::open_download(&b, &admin(), &file.id).await, Err(LocalError::NotFound)));
    assert_eq!(shared::usage(&b).await.unwrap().syncing_files, 1);

    // The next round resumes where it stopped.
    let second = sync_with_budget(&b, &peer_a, 10).await.unwrap();
    assert_eq!(second.chunks, 2);
    assert_eq!(read_all(&b, &admin(), &file.id).await.unwrap(), data);
    assert_eq!(shared::usage(&b).await.unwrap().syncing_files, 0);
}

#[tokio::test]
async fn renaming_and_changing_who_sees_a_folder_reach_the_other_pc() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let f = folder(&a, None, "Brouillon", "everyone").await;
    let file = upload(&a, &f.id, "note.txt", b"note").await;
    sync_with(&b, &peer_a).await.unwrap();
    assert_eq!(names(&b, &employee(), None).await.0, vec!["Brouillon"]);

    // Later, on A: renamed, and made private.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    shared::update_folder(
        &a,
        &admin(),
        &f.id,
        FolderPatch { name: Some("Comptabilité".into()), visibility: Some("admins".into()), ..Default::default() },
    )
    .await
    .unwrap();
    shared::update_file(&a, &admin(), &file.id, FilePatch { name: Some("bilan.txt".into()), folder_id: None }).await.unwrap();
    sync_with(&b, &peer_a).await.unwrap();

    assert_eq!(names(&b, &admin(), None).await.0, vec!["Comptabilité"]);
    assert_eq!(names(&b, &admin(), Some(&f.id)).await.1, vec!["bilan.txt"]);
    assert!(names(&b, &employee(), None).await.0.is_empty(), "private on the other PC too: an employee there sees nothing");
    assert!(matches!(shared::open_download(&b, &employee(), &file.id).await, Err(LocalError::NotFound)));
}

#[tokio::test]
async fn deleting_a_file_or_a_folder_erases_it_on_the_other_pc() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let keep = folder(&a, None, "Garder", "admins").await;
    let gone = folder(&a, None, "Jeter", "admins").await;
    let f1 = upload(&a, &keep.id, "x.bin", &body(CHUNK_BYTES + 5)).await;
    let f2 = upload(&a, &gone.id, "y.bin", b"y").await;
    sync_with(&b, &peer_a).await.unwrap();
    assert_eq!(read_all(&b, &admin(), &f1.id).await.unwrap().len(), CHUNK_BYTES + 5);

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    shared::delete_file(&a, &admin(), &f1.id).await.unwrap();
    shared::delete_folder(&a, &admin(), &gone.id).await.unwrap();
    sync_with(&b, &peer_a).await.unwrap();

    assert!(matches!(shared::open_download(&b, &admin(), &f1.id).await, Err(LocalError::NotFound)));
    assert!(matches!(shared::open_download(&b, &admin(), &f2.id).await, Err(LocalError::NotFound)));
    assert_eq!(names(&b, &admin(), None).await.0, vec!["Garder"]);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM shared_file_chunks").await, 0, "the encrypted content is erased on the other PC too");
}

#[tokio::test]
async fn nested_folders_arrive_whatever_order_their_rows_come_in() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let l1 = folder(&a, None, "A", "admins").await;
    let l2 = folder(&a, Some(&l1.id), "B", "admins").await;
    let l3 = folder(&a, Some(&l2.id), "C", "admins").await;
    upload(&a, &l3.id, "profond.txt", b"deep").await;
    sync_with(&b, &peer_a).await.unwrap();

    let listing = shared::browse(&b, &admin(), Some(&l3.id)).await.unwrap();
    assert_eq!(listing.breadcrumb.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["A", "B", "C"]);
    assert_eq!(listing.files.len(), 1);
}

#[tokio::test]
async fn two_pcs_that_changed_the_same_file_end_up_the_same() {
    let (a, b) = (pc().await, pc().await);
    let (peer_a, peer_b) = (serve(&a).await, serve(&b).await);
    let f = folder(&a, None, "Docs", "admins").await;
    let file = upload(&a, &f.id, "orig.txt", b"x").await;
    sync_with(&b, &peer_a).await.unwrap();

    // Both rename it, B a moment later.
    shared::update_file(&a, &admin(), &file.id, FilePatch { name: Some("de-a.txt".into()), folder_id: None }).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    shared::update_file(&b, &admin(), &file.id, FilePatch { name: Some("de-b.txt".into()), folder_id: None }).await.unwrap();

    sync_with(&a, &peer_b).await.unwrap();
    sync_with(&b, &peer_a).await.unwrap();
    let (na, nb) = (names(&a, &admin(), Some(&f.id)).await.1, names(&b, &admin(), Some(&f.id)).await.1);
    assert_eq!(na, nb, "both PCs agree");
    assert_eq!(na, vec!["de-b.txt"], "the later change wins");
}

#[test]
fn a_change_at_the_same_instant_resolves_the_same_way_everywhere() {
    let base = Stamp { id: "x".into(), updated_at: "2026-01-01T10:00:00+00:00".into(), deleted: false, h: "aaaa".into() };
    let other = Stamp { h: "bbbb".into(), ..base.clone() };
    let gone = Stamp { deleted: true, ..base.clone() };
    // Same time, different content: the larger hash wins on both sides.
    assert!(remote_wins(&base, &other));
    assert!(!remote_wins(&other, &base));
    // Same time, one deleted: the deletion wins on both sides.
    assert!(remote_wins(&base, &gone));
    assert!(!remote_wins(&gone, &base));
    // Otherwise the later change wins, however the timestamps are written.
    let same_instant_elsewhere = Stamp { updated_at: "2026-01-01T11:00:00+01:00".into(), ..base.clone() };
    assert!(
        !remote_wins(&base, &same_instant_elsewhere) && !remote_wins(&same_instant_elsewhere, &base),
        "the same instant written in two time zones is a tie, and an identical row changes nothing"
    );
    let much_later = Stamp { updated_at: "2026-01-02T00:00:00+00:00".into(), ..base.clone() };
    assert!(remote_wins(&base, &much_later) && !remote_wins(&much_later, &base));
}

#[test]
fn only_what_is_missing_or_older_is_asked_for() {
    let stamp = |id: &str, t: &str, h: &str| Stamp { id: id.into(), updated_at: t.into(), deleted: false, h: h.into() };
    let mine = vec![stamp("same", "2026-01-01T10:00:00+00:00", "h"), stamp("old", "2026-01-01T10:00:00+00:00", "h")];
    let theirs = vec![
        stamp("same", "2026-01-01T10:00:00+00:00", "h"),
        stamp("old", "2026-01-02T10:00:00+00:00", "h"),
        stamp("new", "2026-01-01T10:00:00+00:00", "h"),
    ];
    assert_eq!(wanted(&mine, &theirs), vec!["old", "new"]);
}

#[tokio::test]
async fn files_still_being_uploaded_are_not_offered_to_other_pcs() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let f = folder(&a, None, "Docs", "admins").await;
    let start = shared::begin_upload(&a, &admin(), &f.id, "pas-fini.bin", 10, None).await.unwrap();
    sync_with(&b, &peer_a).await.unwrap();
    assert_eq!(count(&b, "SELECT COUNT(*) FROM shared_files").await, 0);

    // Even by asking for it by name.
    let secret = catchup::secret_of(&b).await.unwrap();
    let reply = catchup::post(
        &b,
        &peer_a,
        "/mesh/shared/rows",
        catchup::signed(&secret, fields(vec![("folders", json!([])), ("files", json!([start.file.id]))])),
    )
    .await
    .unwrap();
    let rows = catchup::verified(&secret, reply).unwrap();
    assert_eq!(rows["files"], json!([]));
}

#[tokio::test]
async fn a_chunk_damaged_at_the_other_pc_is_refused_and_the_file_never_shows() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let f = folder(&a, None, "Docs", "admins").await;
    let file = upload(&a, &f.id, "a.bin", &body(CHUNK_BYTES + 10)).await;
    sqlx::query("UPDATE shared_file_chunks SET data = substr(data, 1, 40) || x'00' || substr(data, 42) WHERE idx = 1")
        .execute(&a.db)
        .await
        .unwrap();

    sync_with(&b, &peer_a).await.unwrap();
    assert!(matches!(shared::open_download(&b, &admin(), &file.id).await, Err(LocalError::NotFound)), "never listed");
    assert_eq!(count(&b, "SELECT COUNT(*) FROM shared_file_chunks WHERE idx = 1").await, 0, "the damaged chunk was not kept");
}

#[tokio::test]
async fn a_pc_with_another_shops_secret_gets_nothing() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    let f = folder(&a, None, "Docs", "admins").await;
    upload(&a, &f.id, "a.txt", b"secret").await;
    *b.mesh_secret.write().await = Some("another-shops-secret".into());
    assert!(sync_with(&b, &peer_a).await.is_err());
    assert_eq!(count(&b, "SELECT COUNT(*) FROM shared_files").await, 0);
}

#[tokio::test]
async fn a_pc_outside_network_mode_neither_serves_nor_asks() {
    let (a, b) = (pc().await, pc().await);
    let peer_a = serve(&a).await;
    sqlx::query("UPDATE sync_state SET standalone = 0 WHERE id = 1").execute(&a.db).await.unwrap();
    let f = folder(&b, None, "Docs", "admins").await;
    upload(&b, &f.id, "a.txt", b"x").await;
    // B asks A, which is not in network mode: refused, nothing breaks.
    assert!(sync_with(&b, &peer_a).await.is_err());
    // And a PC not in network mode does not ask at all.
    assert_eq!(sync_with(&a, &peer_a).await.unwrap(), Stats::default());
}

#[tokio::test]
async fn rows_a_peer_makes_up_are_refused() {
    let state = pc().await;
    let now = Utc::now().to_rfc3339();
    let good = FolderRecord {
        id: Uuid::new_v4().to_string(),
        parent_id: None,
        name: "Normal".into(),
        visibility: "admins".into(),
        created_by: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        deleted_at: None,
    };
    let bad_name = FolderRecord { id: Uuid::new_v4().to_string(), name: "../../etc".into(), ..good.clone() };
    let bad_visibility = FolderRecord { id: Uuid::new_v4().to_string(), visibility: "public".into(), ..good.clone() };
    let bad_id = FolderRecord { id: "not-a-uuid".into(), ..good.clone() };
    let stats = apply_rows(&state, vec![good.clone(), bad_name, bad_visibility, bad_id], vec![]).await.unwrap();
    assert_eq!(stats.folders, 1, "only the sound one got in");

    let file = |size: i64, chunks: i64, key: &str| FileRecord {
        id: Uuid::new_v4().to_string(),
        folder_id: good.id.clone(),
        name: "x.bin".into(),
        mime: None,
        size_bytes: size,
        chunk_count: chunks,
        content_id: Uuid::new_v4().to_string(),
        wrapped_key: key.into(),
        key_version: 1,
        created_by: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        deleted_at: None,
    };
    let good_key = "ab".repeat(40);
    let stats = apply_rows(
        &state,
        vec![],
        vec![
            file(10, 1, &good_key),
            file(10, 5, &good_key),                          // chunk count does not match the size
            file(MAX_FILE_BYTES as i64 + 1, 101, &good_key), // too big
            file(10, 1, "zz"),                               // not a key
        ],
    )
    .await
    .unwrap();
    assert_eq!(stats.files, 1);
}

#[tokio::test]
async fn a_third_pc_gets_everything_from_whichever_peer_has_it() {
    let (a, b, c) = (pc().await, pc().await, pc().await);
    let (peer_a, peer_b) = (serve(&a).await, serve(&b).await);
    let f = folder(&a, None, "Docs", "admins").await;
    let data = body(CHUNK_BYTES + 100);
    let file = upload(&a, &f.id, "a.bin", &data).await;
    sync_with(&b, &peer_a).await.unwrap();

    // C joins later and catches up from B alone; A need not be there.
    let _ = peer_a;
    sync_with(&c, &peer_b).await.unwrap();
    assert_eq!(read_all(&c, &admin(), &file.id).await.unwrap(), data);
}
