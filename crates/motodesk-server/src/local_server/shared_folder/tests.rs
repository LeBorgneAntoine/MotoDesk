//! Rules of the shared folder: who sees what, how files are stored, and what tampering does.

use super::*;
use crate::local_server::test_support::shop;

fn admin() -> Actor {
    Actor { user_id: "admin-1".into(), name: "Alice".into(), is_admin: true }
}
fn employee() -> Actor {
    Actor { user_id: "emp-1".into(), name: "Bob".into(), is_admin: false }
}

async fn ready_shop() -> SharedState {
    let state = shop().await;
    *state.mesh_secret.write().await = Some("a-secret-shared-by-the-shop".into());
    state
}

/// A deterministic, not-trivially-compressible body of `len` bytes.
fn body(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + i / 7) % 251) as u8).collect()
}

async fn upload(state: &SharedState, who: &Actor, folder: &str, name: &str, data: &[u8]) -> FileView {
    let start = begin_upload(state, who, folder, name, data.len() as u64, Some("application/pdf")).await.unwrap();
    for (i, chunk) in data.chunks(CHUNK_BYTES).enumerate() {
        put_chunk(state, who, &start.file.id, i as u32, chunk).await.unwrap();
    }
    finish_upload(state, who, &start.file.id).await.unwrap()
}

async fn read_all(state: &SharedState, who: &Actor, file_id: &str) -> LocalResult<Vec<u8>> {
    let dl = open_download(state, who, file_id).await?;
    let mut out = Vec::new();
    for i in 0..dl.chunk_count() {
        out.extend(dl.read_chunk(state, i).await?);
    }
    Ok(out)
}

async fn folder(state: &SharedState, parent: Option<&str>, name: &str, visibility: &str) -> FolderView {
    create_folder(state, &admin(), parent, name, visibility).await.unwrap()
}

async fn count(state: &SharedState, sql: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql).fetch_one(&state.db).await.unwrap();
    n
}

#[tokio::test]
async fn nothing_works_before_the_shop_has_a_network_secret() {
    let state = shop().await;
    assert!(!available(&state).await);
    let err = browse(&state, &admin(), None).await.unwrap_err();
    assert!(matches!(err, LocalError::BadRequest(m) if m.contains("disponible")));
}

#[tokio::test]
async fn a_file_round_trips_across_chunks_and_is_unreadable_at_rest() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Comptabilité", "admins").await;
    let data = body(CHUNK_BYTES * 2 + 12_345);
    let file = upload(&state, &admin(), &f.id, "Bilan 2025.pdf", &data).await;
    assert_eq!(file.state, "ready");
    assert_eq!(file.chunk_count, 3);
    assert_eq!(read_all(&state, &admin(), &file.id).await.unwrap(), data);

    let stored: Vec<(Vec<u8>,)> = sqlx::query_as("SELECT data FROM shared_file_chunks").fetch_all(&state.db).await.unwrap();
    assert_eq!(stored.len(), 3);
    let probe = &data[100..140];
    assert!(!stored.iter().any(|(d,)| d.windows(probe.len()).any(|w| w == probe)));
}

#[tokio::test]
async fn a_file_only_shows_once_its_upload_is_complete() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let data = body(CHUNK_BYTES + 10);
    let start = begin_upload(&state, &admin(), &f.id, "a.pdf", data.len() as u64, None).await.unwrap();
    assert!(browse(&state, &admin(), Some(&f.id)).await.unwrap().files.is_empty());

    put_chunk(&state, &admin(), &start.file.id, 0, &data[..CHUNK_BYTES]).await.unwrap();
    let early = finish_upload(&state, &admin(), &start.file.id).await.unwrap_err();
    assert!(matches!(early, LocalError::BadRequest(m) if m.contains("incomplet")));

    put_chunk(&state, &admin(), &start.file.id, 1, &data[CHUNK_BYTES..]).await.unwrap();
    finish_upload(&state, &admin(), &start.file.id).await.unwrap();
    assert_eq!(browse(&state, &admin(), Some(&f.id)).await.unwrap().files.len(), 1);
}

#[tokio::test]
async fn an_upload_can_be_resumed_and_a_chunk_resent() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let data = body(CHUNK_BYTES * 2);
    let start = begin_upload(&state, &admin(), &f.id, "big.bin", data.len() as u64, None).await.unwrap();
    // Out of order, and the first chunk sent twice.
    put_chunk(&state, &admin(), &start.file.id, 1, &data[CHUNK_BYTES..]).await.unwrap();
    put_chunk(&state, &admin(), &start.file.id, 0, &data[..CHUNK_BYTES]).await.unwrap();
    put_chunk(&state, &admin(), &start.file.id, 0, &data[..CHUNK_BYTES]).await.unwrap();
    finish_upload(&state, &admin(), &start.file.id).await.unwrap();
    assert_eq!(read_all(&state, &admin(), &start.file.id).await.unwrap(), data);
}

#[tokio::test]
async fn chunks_of_the_wrong_size_or_position_are_refused() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let start = begin_upload(&state, &admin(), &f.id, "x.bin", (CHUNK_BYTES + 5) as u64, None).await.unwrap();
    let id = &start.file.id;
    assert!(put_chunk(&state, &admin(), id, 0, &[0u8; 10]).await.is_err(), "a short first chunk");
    assert!(put_chunk(&state, &admin(), id, 1, &[0u8; 6]).await.is_err(), "a long last chunk");
    assert!(put_chunk(&state, &admin(), id, 2, &[0u8; 5]).await.is_err(), "past the end");
    assert!(put_chunk(&state, &admin(), id, 1, &[0u8; 5]).await.is_ok());
}

#[tokio::test]
async fn only_the_person_who_started_an_upload_can_feed_it() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Partagé", "everyone").await;
    let start = begin_upload(&state, &employee(), &f.id, "x.bin", 4, None).await.unwrap();
    let err = put_chunk(&state, &admin(), &start.file.id, 0, &[1, 2, 3, 4]).await.unwrap_err();
    assert!(matches!(err, LocalError::Forbidden));
    assert!(put_chunk(&state, &employee(), &start.file.id, 0, &[1, 2, 3, 4]).await.is_ok());
}

#[tokio::test]
async fn an_empty_file_is_a_file_too() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let file = upload(&state, &admin(), &f.id, "vide.txt", &[]).await;
    assert_eq!(file.chunk_count, 0);
    assert!(read_all(&state, &admin(), &file.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn files_that_are_too_big_are_refused_before_anything_is_stored() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let err = begin_upload(&state, &admin(), &f.id, "enorme.iso", MAX_FILE_BYTES + 1, None).await.unwrap_err();
    assert!(matches!(err, LocalError::BadRequest(m) if m.contains("volumineux")));
    assert_eq!(count(&state, "SELECT COUNT(*) FROM shared_files").await, 0);
}

#[tokio::test]
async fn a_name_already_taken_gets_a_number() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let a = upload(&state, &admin(), &f.id, "releve.pdf", b"1").await;
    let b = upload(&state, &admin(), &f.id, "Releve.PDF", b"2").await;
    let c = upload(&state, &admin(), &f.id, "releve.pdf", b"3").await;
    assert_eq!(a.name, "releve.pdf");
    assert_eq!(b.name, "Releve (2).PDF");
    assert_eq!(c.name, "releve (3).pdf");
}

#[tokio::test]
async fn a_folder_hidden_from_employees_does_not_exist_for_them() {
    let state = ready_shop().await;
    let secret = folder(&state, None, "Salaires", "admins").await;
    let open = folder(&state, None, "Général", "everyone").await;
    let inside_secret = folder(&state, Some(&secret.id), "Public en apparence", "everyone").await;
    let file = upload(&state, &admin(), &secret.id, "paies.xlsx", b"confidentiel").await;
    let nested = upload(&state, &admin(), &inside_secret.id, "note.txt", b"note").await;

    // The top level lists only what the employee may see.
    let top = browse(&state, &employee(), None).await.unwrap();
    assert_eq!(top.folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["Général"]);
    assert!(!top.can_manage);
    assert_eq!(browse(&state, &admin(), None).await.unwrap().folders.len(), 2);

    // Reaching for a hidden folder, or one nested under it, looks like a missing one.
    assert!(matches!(browse(&state, &employee(), Some(&secret.id)).await, Err(LocalError::NotFound)));
    assert!(matches!(browse(&state, &employee(), Some(&inside_secret.id)).await, Err(LocalError::NotFound)));
    assert!(matches!(open_download(&state, &employee(), &file.id).await, Err(LocalError::NotFound)));
    assert!(matches!(open_download(&state, &employee(), &nested.id).await, Err(LocalError::NotFound)));
    assert!(browse(&state, &employee(), Some(&open.id)).await.is_ok());
}

#[tokio::test]
async fn employees_may_read_and_add_in_visible_folders_but_not_manage_them() {
    let state = ready_shop().await;
    let open = folder(&state, None, "Général", "everyone").await;
    let file = upload(&state, &employee(), &open.id, "photo.jpg", b"jpeg").await;
    assert_eq!(read_all(&state, &employee(), &file.id).await.unwrap(), b"jpeg");

    assert!(matches!(create_folder(&state, &employee(), None, "Mien", "everyone").await, Err(LocalError::Forbidden)));
    assert!(matches!(
        update_folder(&state, &employee(), &open.id, FolderPatch { name: Some("Autre".into()), ..Default::default() }).await,
        Err(LocalError::Forbidden)
    ));
    assert!(matches!(delete_folder(&state, &employee(), &open.id).await, Err(LocalError::Forbidden)));
    assert!(matches!(delete_file(&state, &employee(), &file.id).await, Err(LocalError::Forbidden)));
    assert!(matches!(
        update_file(&state, &employee(), &file.id, FilePatch { name: Some("x.jpg".into()), folder_id: None }).await,
        Err(LocalError::Forbidden)
    ));
}

#[tokio::test]
async fn making_a_folder_private_takes_it_away_from_employees_at_once() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Général", "everyone").await;
    let file = upload(&state, &admin(), &f.id, "a.txt", b"a").await;
    assert!(open_download(&state, &employee(), &file.id).await.is_ok());

    update_folder(&state, &admin(), &f.id, FolderPatch { visibility: Some("admins".into()), ..Default::default() })
        .await
        .unwrap();
    assert!(matches!(open_download(&state, &employee(), &file.id).await, Err(LocalError::NotFound)));
    assert!(open_download(&state, &admin(), &file.id).await.is_ok());
}

#[tokio::test]
async fn folders_keep_unique_names_and_cannot_loop_or_nest_too_deep() {
    let state = ready_shop().await;
    let a = folder(&state, None, "A", "admins").await;
    assert!(matches!(create_folder(&state, &admin(), None, "a", "admins").await, Err(LocalError::Conflict(_))));
    let b = folder(&state, Some(&a.id), "B", "admins").await;
    // A cannot move under its own child.
    let err = update_folder(&state, &admin(), &a.id, FolderPatch { parent_id: Some(Some(b.id.clone())), ..Default::default() })
        .await
        .unwrap_err();
    assert!(matches!(err, LocalError::BadRequest(m) if m.contains("dans lui-même")));

    let mut parent = a.id.clone();
    let mut refused = false;
    for depth in 0..12 {
        match create_folder(&state, &admin(), Some(&parent), &format!("niveau {depth}"), "admins").await {
            Ok(f) => parent = f.id,
            Err(LocalError::BadRequest(_)) => {
                refused = true;
                break;
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!(refused, "nesting stops somewhere");
}

#[tokio::test]
async fn deleting_a_file_erases_its_content_but_leaves_a_tombstone() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let file = upload(&state, &admin(), &f.id, "a.bin", &body(CHUNK_BYTES + 1)).await;
    delete_file(&state, &admin(), &file.id).await.unwrap();

    assert_eq!(count(&state, "SELECT COUNT(*) FROM shared_file_chunks").await, 0, "the encrypted content is gone");
    let (tomb,): (Option<String>,) =
        sqlx::query_as("SELECT deleted_at FROM shared_files WHERE id = $1").bind(&file.id).fetch_one(&state.db).await.unwrap();
    assert!(tomb.is_some(), "the row stays so the deletion can reach the other PCs");
    assert!(matches!(open_download(&state, &admin(), &file.id).await, Err(LocalError::NotFound)));
    assert!(browse(&state, &admin(), Some(&f.id)).await.unwrap().files.is_empty());
}

#[tokio::test]
async fn deleting_a_folder_deletes_everything_under_it() {
    let state = ready_shop().await;
    let top = folder(&state, None, "Archives", "admins").await;
    let sub = folder(&state, Some(&top.id), "2024", "admins").await;
    let f1 = upload(&state, &admin(), &top.id, "a.txt", b"a").await;
    let f2 = upload(&state, &admin(), &sub.id, "b.txt", b"b").await;
    delete_folder(&state, &admin(), &top.id).await.unwrap();

    assert!(browse(&state, &admin(), None).await.unwrap().folders.is_empty());
    for id in [&f1.id, &f2.id] {
        assert!(matches!(open_download(&state, &admin(), id).await, Err(LocalError::NotFound)));
    }
    assert_eq!(count(&state, "SELECT COUNT(*) FROM shared_file_chunks").await, 0);
    // The name can be used again.
    assert!(create_folder(&state, &admin(), None, "Archives", "admins").await.is_ok());
}

#[tokio::test]
async fn a_person_can_cancel_their_own_unfinished_upload() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Général", "everyone").await;
    let start = begin_upload(&state, &employee(), &f.id, "x.bin", 3, None).await.unwrap();
    put_chunk(&state, &employee(), &start.file.id, 0, &[1, 2, 3]).await.unwrap();
    delete_file(&state, &employee(), &start.file.id).await.unwrap();
    assert_eq!(count(&state, "SELECT COUNT(*) FROM shared_files").await, 0);
    assert_eq!(count(&state, "SELECT COUNT(*) FROM shared_file_chunks").await, 0);
}

#[tokio::test]
async fn uploads_abandoned_for_a_day_are_cleaned_up() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let start = begin_upload(&state, &admin(), &f.id, "old.bin", 2, None).await.unwrap();
    put_chunk(&state, &admin(), &start.file.id, 0, &[1, 2]).await.unwrap();
    let long_ago = (Utc::now() - Duration::hours(30)).to_rfc3339();
    sqlx::query("UPDATE shared_files SET created_at = $1").bind(long_ago).execute(&state.db).await.unwrap();

    // The next upload that starts sweeps it away.
    begin_upload(&state, &admin(), &f.id, "new.bin", 1, None).await.unwrap();
    assert_eq!(count(&state, "SELECT COUNT(*) FROM shared_files WHERE name = 'old.bin'").await, 0);
    assert_eq!(count(&state, "SELECT COUNT(*) FROM shared_file_chunks").await, 0);
}

#[tokio::test]
async fn stored_content_that_was_tampered_with_or_cut_short_is_not_returned_as_good() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let data = body(CHUNK_BYTES * 2);
    let file = upload(&state, &admin(), &f.id, "a.bin", &data).await;

    // One byte flipped in a chunk.
    sqlx::query("UPDATE shared_file_chunks SET data = substr(data, 1, 40) || x'00' || substr(data, 42) WHERE idx = 0")
        .execute(&state.db)
        .await
        .unwrap();
    assert!(read_all(&state, &admin(), &file.id).await.is_err());

    // A chunk removed (a truncated file).
    let file2 = upload(&state, &admin(), &f.id, "b.bin", &data).await;
    let (content,): (String,) =
        sqlx::query_as("SELECT content_id FROM shared_files WHERE id = $1").bind(&file2.id).fetch_one(&state.db).await.unwrap();
    sqlx::query("DELETE FROM shared_file_chunks WHERE content_id = $1 AND idx = 1").bind(content).execute(&state.db).await.unwrap();
    assert!(read_all(&state, &admin(), &file2.id).await.is_err());
}

#[tokio::test]
async fn another_shops_secret_cannot_open_the_files() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    let file = upload(&state, &admin(), &f.id, "a.txt", b"secret").await;
    *state.mesh_secret.write().await = Some("a-different-shop-secret".into());
    assert!(open_download(&state, &admin(), &file.id).await.is_err());
}

#[tokio::test]
async fn names_with_path_characters_are_refused() {
    for bad in ["", "   ", "..", "a/b", "a\\b", "con:x", "q?.txt", "a\u{0}b", "fin."] {
        assert!(clean_name(bad).is_err(), "{bad:?}");
    }
    assert_eq!(clean_name("  Relevé   mars  2025.pdf ").unwrap(), "Relevé mars 2025.pdf");
    assert!(clean_name(&"x".repeat(201)).is_err());
}

#[tokio::test]
async fn moving_and_renaming_files_keeps_names_unique() {
    let state = ready_shop().await;
    let a = folder(&state, None, "A", "admins").await;
    let b = folder(&state, None, "B", "admins").await;
    let f1 = upload(&state, &admin(), &a.id, "x.pdf", b"1").await;
    upload(&state, &admin(), &b.id, "x.pdf", b"2").await;

    let clash = update_file(&state, &admin(), &f1.id, FilePatch { name: None, folder_id: Some(b.id.clone()) }).await;
    assert!(matches!(clash, Err(LocalError::Conflict(_))));
    let ok = update_file(&state, &admin(), &f1.id, FilePatch { name: Some("y.pdf".into()), folder_id: Some(b.id.clone()) })
        .await
        .unwrap();
    assert_eq!((ok.name.as_str(), ok.folder_id.as_str()), ("y.pdf", b.id.as_str()));
    assert_eq!(read_all(&state, &admin(), &f1.id).await.unwrap(), b"1", "the content moved with it");
}

#[tokio::test]
async fn opening_adding_and_removing_are_recorded_for_administrators() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Général", "everyone").await;
    let file = upload(&state, &employee(), &f.id, "a.txt", b"a").await;
    open_download(&state, &employee(), &file.id).await.unwrap();
    delete_file(&state, &admin(), &file.id).await.unwrap();

    let log = recent_audit(&state, &admin(), 50).await.unwrap();
    let actions: Vec<(&str, &str)> =
        log.iter().rev().map(|e| (e.action.as_str(), e.user_name.as_deref().unwrap_or(""))).collect();
    assert_eq!(
        actions,
        vec![("folder_created", "Alice"), ("file_added", "Bob"), ("file_opened", "Bob"), ("file_deleted", "Alice")]
    );
    assert!(matches!(recent_audit(&state, &employee(), 50).await, Err(LocalError::Forbidden)));
}

#[tokio::test]
async fn usage_counts_only_finished_files() {
    let state = ready_shop().await;
    let f = folder(&state, None, "Docs", "admins").await;
    upload(&state, &admin(), &f.id, "a.bin", &body(1000)).await;
    begin_upload(&state, &admin(), &f.id, "pending.bin", 5000, None).await.unwrap();
    let u = usage(&state).await.unwrap();
    assert_eq!((u.files, u.bytes), (1, 1000));
}
