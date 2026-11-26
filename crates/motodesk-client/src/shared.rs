//! The shared folder (port of `desktop/src/lib/api/shared.ts` and `desktop/src/lib/sharedFolder.ts`):
//! the shop's sensitive files, encrypted on disk and kept on every PC of the network.
//!
//! A file travels in chunks, so a big one is never held whole in memory: it is read from the
//! disk a chunk at a time on its way in, and written to the disk as it arrives on its way out.

use std::io::{Read as _, Write as _};
use std::path::Path;

use axum::http::Method;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::session::Client;
use crate::settings::encode;

/// Who sees a folder and what is in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Admins,
    Everyone,
}

impl Visibility {
    pub const ALL: [Visibility; 2] = [Visibility::Admins, Visibility::Everyone];

    pub fn id(self) -> &'static str {
        match self {
            Visibility::Admins => "admins",
            Visibility::Everyone => "everyone",
        }
    }

    /// Anything unknown is read as the stricter of the two.
    pub fn from_id(id: &str) -> Visibility {
        if id == "everyone" { Visibility::Everyone } else { Visibility::Admins }
    }

    pub fn label(self) -> &'static str {
        match self {
            Visibility::Admins => "Administrateurs",
            Visibility::Everyone => "Tous les employés",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Visibility::Admins => "Seuls les administrateurs voient ce dossier et son contenu.",
            Visibility::Everyone => "Tous les employés connectés voient ce dossier et peuvent y ajouter des fichiers.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SharedFolder {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub created_at: String,
}

impl SharedFolder {
    pub fn visibility(&self) -> Visibility {
        Visibility::from_id(&self.visibility)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SharedFile {
    pub id: String,
    #[serde(default)]
    pub folder_id: String,
    pub name: String,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    /// `uploading` until its last chunk is in, then `ready`.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct SharedListing {
    /// The folder shown; `None` for the top level.
    pub folder: Option<SharedFolder>,
    /// From the top level down to `folder`.
    pub breadcrumb: Vec<SharedFolder>,
    pub folders: Vec<SharedFolder>,
    pub files: Vec<SharedFile>,
    /// Whether this user may create, rename and delete here.
    pub can_manage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub files: u64,
    /// Known from another PC, content still being copied here.
    pub syncing_files: u64,
    pub bytes: u64,
    pub max_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct SharedStatus {
    /// False until the shop has a network (or an online account): there is no key before.
    pub available: bool,
    pub is_admin: bool,
    /// Files are copied to the other PCs of the shop's local network (not yet when online).
    pub replicated: bool,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AuditEntry {
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub detail: Option<String>,
}

impl AuditEntry {
    pub fn who(&self) -> &str {
        self.user_name.as_deref().filter(|name| !name.trim().is_empty()).unwrap_or("Quelqu'un")
    }
}

// ---- the folders and what is in them -----------------------------------------------------------

pub async fn status(client: &Client) -> ApiResult<SharedStatus> {
    client.get("/api/shared/status").await
}

/// What a folder holds; `None` for the top level.
pub async fn browse(client: &Client, folder_id: Option<&str>) -> ApiResult<SharedListing> {
    match folder_id {
        Some(id) => client.get(format!("/api/shared/browse?folder={}", encode(id))).await,
        None => client.get("/api/shared/browse").await,
    }
}

pub async fn create_folder(client: &Client, parent_id: Option<&str>, name: &str, visibility: Visibility) -> ApiResult<SharedFolder> {
    let body = serde_json::json!({ "parent_id": parent_id, "name": name, "visibility": visibility.id() });
    client.post("/api/shared/folders", &body).await
}

pub async fn update_folder(client: &Client, id: &str, name: &str, visibility: Visibility) -> ApiResult<SharedFolder> {
    let body = serde_json::json!({ "name": name, "visibility": visibility.id() });
    client.patch(format!("/api/shared/folders/{}", encode(id)), &body).await
}

pub async fn delete_folder(client: &Client, id: &str) -> ApiResult<()> {
    client.delete(format!("/api/shared/folders/{}", encode(id))).await
}

pub async fn rename_file(client: &Client, id: &str, name: &str) -> ApiResult<SharedFile> {
    client.patch(format!("/api/shared/files/{}", encode(id)), &serde_json::json!({ "name": name })).await
}

pub async fn delete_file(client: &Client, id: &str) -> ApiResult<()> {
    client.delete(format!("/api/shared/files/{}", encode(id))).await
}

/// Who opened, added or removed what (administrators). Kept on this PC only.
pub async fn audit(client: &Client, limit: u32) -> ApiResult<Vec<AuditEntry>> {
    client.get(format!("/api/shared/audit?limit={limit}")).await
}

// ---- a file on its way in ----------------------------------------------------------------------

const CHUNK_RETRIES: usize = 2;

#[derive(Deserialize)]
struct UploadStart {
    file: SharedFile,
    chunk_bytes: usize,
}

/// The content type of a file, from its name, for the few kinds a shop keeps. The service
/// only uses it to hand the file back.
fn mime_of(name: &str) -> Option<&'static str> {
    let extension = name.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase())?;
    Some(match extension.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "zip" => "application/zip",
        _ => return None,
    })
}

/// Fill `buffer` from the file, as far as the file goes. Answers how much was read.
fn read_chunk(file: &mut std::fs::File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

async fn send_chunk(client: &Client, file_id: &str, index: usize, bytes: &[u8]) -> ApiResult<()> {
    let path = format!("/api/shared/files/{}/chunks/{index}", encode(file_id));
    let mut attempt = 0;
    loop {
        match client.call(Method::PUT, path.clone()).raw("application/octet-stream", bytes.to_vec()).done().await {
            Ok(()) => return Ok(()),
            // A refusal will not get better by trying again; a hiccup might.
            Err(error) if (400..500).contains(&error.status) || attempt >= CHUNK_RETRIES => return Err(error),
            Err(_) => attempt += 1,
        }
    }
}

fn unreadable(path: &Path, error: std::io::Error) -> ApiError {
    ApiError::new(0, "unreadable", format!("Impossible de lire « {} » : {error}", path.display()))
}

/// Send a file of this PC into a folder: announce it, send its chunks, then finish.
/// `progress` hears the bytes sent so far. A failure removes the half-sent file, so nothing
/// partial is left behind.
pub async fn upload(client: &Client, folder_id: &str, path: &Path, progress: impl Fn(u64)) -> ApiResult<SharedFile> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let mut source = std::fs::File::open(path).map_err(|error| unreadable(path, error))?;
    let size = source.metadata().map_err(|error| unreadable(path, error))?.len();
    let body = serde_json::json!({ "folder_id": folder_id, "name": name, "size_bytes": size, "mime": mime_of(&name) });
    let start: UploadStart = client.post("/api/shared/files", &body).await?;
    let id = start.file.id.clone();

    let sent = async {
        let mut buffer = vec![0u8; start.chunk_bytes.max(1)];
        let (mut index, mut done) = (0usize, 0u64);
        progress(0);
        loop {
            let read = read_chunk(&mut source, &mut buffer).map_err(|error| unreadable(path, error))?;
            if read == 0 {
                break;
            }
            send_chunk(client, &id, index, &buffer[..read]).await?;
            index += 1;
            done += read as u64;
            progress(done);
        }
        client.post::<SharedFile, _>(format!("/api/shared/files/{}/finish", encode(&id)), &serde_json::json!({})).await
    }
    .await;
    if sent.is_err() {
        // Best effort: an upload nobody finishes is also swept away after a day.
        let _ = delete_file(client, &id).await;
    }
    sent
}

// ---- a file on its way out ---------------------------------------------------------------------

/// Write the decrypted file to `destination`, chunk by chunk as it arrives. Answers its size.
/// A failure half-way removes what was written.
pub async fn download_to(client: &Client, file_id: &str, destination: &Path) -> ApiResult<u64> {
    let unwritable = |error: std::io::Error| ApiError::new(0, "unwritable", format!("Impossible d'écrire « {} » : {error}", destination.display()));
    let mut target = std::fs::File::create(destination).map_err(unwritable)?;
    let path = format!("/api/shared/files/{}/content", encode(file_id));
    let outcome = client.download(&path, |bytes| target.write_all(bytes)).await;
    drop(target);
    if outcome.is_err() {
        let _ = std::fs::remove_file(destination);
    }
    outcome
}

// ---- words -------------------------------------------------------------------------------------

/// `1,5 Mo`, `820 Ko`, `12 octets`.
pub fn file_size(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["Ko", "Mo", "Go"];
    if bytes < 1024 {
        return format!("{bytes} {}", if bytes == 1 { "octet" } else { "octets" });
    }
    let (mut value, mut unit) = (bytes as f64 / 1024., 0);
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    let text = if value >= 100. { format!("{value:.0}") } else { format!("{value:.1}").trim_end_matches(".0").to_string() };
    format!("{} {}", text.replace('.', ","), UNITS[unit])
}

/// How far an upload is, between 0 and 100.
pub fn percent(done: u64, total: u64) -> u32 {
    if total == 0 {
        return 100;
    }
    ((done.min(total) as f64 / total as f64) * 100.).round() as u32
}

/// What the audit trail's action codes mean, in the shop's words.
pub fn audit_label(action: &str) -> &str {
    match action {
        "file_added" => "a ajouté",
        "file_opened" => "a ouvert",
        "file_deleted" => "a supprimé",
        "folder_created" => "a créé le dossier",
        "folder_deleted" => "a supprimé le dossier",
        "folder_visibility" => "a changé la visibilité de",
        other => other,
    }
}

/// The line under the title: where the files live.
pub fn storage_sentence(replicated: bool) -> &'static str {
    if replicated {
        "Vos fichiers sensibles (comptabilité, contrats, pièces d'identité), chiffrés et copiés sur les postes de votre réseau."
    } else {
        "Vos fichiers sensibles (comptabilité, contrats, pièces d'identité), chiffrés sur ce poste."
    }
}

/// `2 fichiers sont en cours de copie depuis un autre poste.`; `None` when there is none.
pub fn syncing_sentence(count: u64) -> Option<String> {
    match count {
        0 => None,
        1 => Some("1 fichier est en cours de copie depuis un autre poste.".into()),
        many => Some(format!("{many} fichiers sont en cours de copie depuis un autre poste.")),
    }
}

/// The line at the foot of a folder: how much is kept, and the limits.
pub fn usage_sentence(usage: &Usage) -> String {
    format!(
        "{} fichier{} · {} · chaque fichier jusqu'à {} · chiffrement AES-256",
        usage.files,
        if usage.files > 1 { "s" } else { "" },
        file_size(usage.bytes),
        file_size(usage.max_file_bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_in_the_units_of_the_shop() {
        assert_eq!(file_size(0), "0 octets");
        assert_eq!(file_size(1), "1 octet");
        assert_eq!(file_size(820 * 1024), "820 Ko");
        assert_eq!(file_size(1024 * 1024 * 3 / 2), "1,5 Mo");
        assert_eq!(file_size(100 * 1024 * 1024), "100 Mo");
        // No lone decimal zero.
        assert_eq!(file_size(2 * 1024 * 1024), "2 Mo");
        assert_eq!(file_size(5 * 1024 * 1024 * 1024), "5 Go");
    }

    #[test]
    fn progress_stays_between_0_and_100() {
        assert_eq!(percent(0, 10), 0);
        assert_eq!(percent(5, 10), 50);
        assert_eq!(percent(20, 10), 100);
        assert_eq!(percent(0, 0), 100);
    }

    #[test]
    fn visibilities_and_audit_actions_have_names() {
        assert_eq!(Visibility::from_id("everyone"), Visibility::Everyone);
        // Anything unknown is the stricter one.
        assert_eq!(Visibility::from_id("partners"), Visibility::Admins);
        assert_eq!(Visibility::Everyone.label(), "Tous les employés");
        assert_eq!(audit_label("file_opened"), "a ouvert");
        assert_eq!(audit_label("something_new"), "something_new");
        let nameless = AuditEntry { at: String::new(), user_name: Some(" ".into()), action: "file_added".into(), detail: None };
        assert_eq!(nameless.who(), "Quelqu'un");
    }

    #[test]
    fn copies_on_other_pcs_are_only_claimed_when_they_are_made() {
        assert!(storage_sentence(true).contains("postes de votre réseau"));
        assert!(storage_sentence(false).contains("sur ce poste") && !storage_sentence(false).contains("réseau"));
        assert_eq!(syncing_sentence(0), None);
        assert!(syncing_sentence(1).unwrap().starts_with("1 fichier est"));
        assert!(syncing_sentence(3).unwrap().starts_with("3 fichiers sont"));
        let usage = Usage { files: 2, syncing_files: 0, bytes: 1536, max_file_bytes: 100 * 1024 * 1024 };
        assert_eq!(usage_sentence(&usage), "2 fichiers · 1,5 Ko · chaque fichier jusqu'à 100 Mo · chiffrement AES-256");
    }

    #[test]
    fn a_file_is_read_a_chunk_at_a_time_to_its_last_byte() {
        let path = std::env::temp_dir().join(format!("motodesk-chunks-{}.bin", std::process::id()));
        std::fs::write(&path, vec![7u8; 2500]).unwrap();
        let mut file = std::fs::File::open(&path).unwrap();
        let mut buffer = vec![0u8; 1000];
        let sizes: Vec<usize> = std::iter::from_fn(|| match read_chunk(&mut file, &mut buffer).unwrap() {
            0 => None,
            read => Some(read),
        })
        .collect();
        assert_eq!(sizes, [1000, 1000, 500]);
        let _ = std::fs::remove_file(&path);
        assert_eq!(mime_of("Bilan 2026.PDF"), Some("application/pdf"));
        assert_eq!(mime_of("notes"), None);
    }
}
