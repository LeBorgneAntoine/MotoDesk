//! A second place for the backups, away from this PC's disk.
//!
//! A backup that sits beside the data it protects is lost with it: a dead disk or a stolen PC
//! takes both. Each backup is therefore copied to a folder the owner chooses (a USB drive, a
//! network share, a synchronised folder).
//!
//! The choice belongs to this PC, not to the shop's data: it is kept in a small file of the data
//! folder, which neither sync nor a restore carries anywhere else. A copy that fails never fails
//! the backup it follows; it is remembered until a later one succeeds.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::local_server::error::{LocalError, LocalResult};

const SETTINGS_FILE: &str = "backup-copy.json";
/// Automatic backups kept in the folder, as many as beside the data.
const AUTO_KEPT: usize = 8;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    folder: String,
    last_copy_at: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopyStatus {
    pub folder: String,
    pub last_copy_at: Option<String>,
    /// Why the latest copy did not get there; `None` once one does.
    pub error: Option<String>,
    /// The folder is on the same disk as the data: it survives a mistake, not a dead disk.
    pub same_drive: bool,
}

fn load(data_dir: &Path) -> Option<Stored> {
    let text = fs::read_to_string(data_dir.join(SETTINGS_FILE)).ok()?;
    serde_json::from_str::<Stored>(&text).ok().filter(|stored| !stored.folder.is_empty())
}

fn save(data_dir: &Path, stored: &Stored) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(stored).unwrap_or_default();
    fs::write(data_dir.join(SETTINGS_FILE), text)
}

pub fn status(data_dir: &Path) -> Option<CopyStatus> {
    load(data_dir).map(|stored| CopyStatus {
        same_drive: same_drive(data_dir, Path::new(&stored.folder)),
        folder: stored.folder,
        last_copy_at: stored.last_copy_at,
        error: stored.last_error,
    })
}

/// Choose the folder. Refused when it could not receive a backup, or is inside the data folder.
pub fn set(data_dir: &Path, folder: &str) -> LocalResult<()> {
    let folder = folder.trim();
    let path = PathBuf::from(folder);
    if folder.is_empty() || !path.is_absolute() || !path.is_dir() {
        return Err(LocalError::BadRequest("ce dossier est introuvable".into()));
    }
    if let (Ok(chosen), Ok(data)) = (path.canonicalize(), data_dir.canonicalize()) {
        if chosen.starts_with(&data) {
            return Err(LocalError::BadRequest("choisissez un dossier en dehors de celui des données de Motodesk".into()));
        }
    }
    let probe = path.join(".motodesk-essai");
    fs::write(&probe, b"motodesk").map_err(|e| LocalError::BadRequest(format!("impossible d'écrire dans ce dossier ({e})")))?;
    let _ = fs::remove_file(probe);
    save(data_dir, &Stored { folder: folder.to_string(), ..Default::default() }).map_err(|e| LocalError::Internal(e.into()))
}

pub fn clear(data_dir: &Path) -> LocalResult<()> {
    match fs::remove_file(data_dir.join(SETTINGS_FILE)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(LocalError::Internal(e.into())),
    }
}

/// Copy a finished backup into the chosen folder, if one is chosen, and remember how it went.
/// Blocking: a slow drive or share is waited for.
pub fn copy(data_dir: &Path, backup: &Path) {
    let Some(mut stored) = load(data_dir) else { return };
    match copy_into(Path::new(&stored.folder), backup) {
        Ok(()) => {
            stored.last_copy_at = Some(Utc::now().to_rfc3339());
            stored.last_error = None;
        }
        Err(error) => {
            tracing::warn!(error, folder = %stored.folder, "the backup was not copied to the chosen folder");
            stored.last_error = Some(error);
        }
    }
    if let Err(error) = save(data_dir, &stored) {
        tracing::warn!(%error, "could not remember how the backup copy went");
    }
}

fn copy_into(folder: &Path, backup: &Path) -> Result<(), String> {
    // Not created when missing: a drive that is unplugged must be reported, not replaced.
    if !folder.is_dir() {
        return Err("le dossier est introuvable (disque débranché ou réseau absent ?)".into());
    }
    let name = backup.file_name().ok_or("nom de sauvegarde illisible")?;
    let target = folder.join(name);
    let partial = folder.join(format!("{}.partial", name.to_string_lossy()));
    let copied = fs::copy(backup, &partial).and_then(|written| {
        // Visible under its final name only once it is complete.
        fs::rename(&partial, &target).map(|()| written)
    });
    let expected = fs::metadata(backup).map(|m| m.len()).unwrap_or(0);
    match copied {
        Ok(written) if written == expected => {
            prune(folder);
            Ok(())
        }
        Ok(_) => {
            let _ = fs::remove_file(&target);
            Err("la copie est incomplète".into())
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(format!("la copie a échoué ({error})"))
        }
    }
}

/// Only the automatic backups of this app are removed: the folder is the owner's.
fn prune(folder: &Path) {
    let mut autos: Vec<PathBuf> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("auto-") && name.ends_with(&format!(".{}", crate::local_server::backup::EXT)))
        })
        .collect();
    autos.sort();
    while autos.len() > AUTO_KEPT {
        let _ = fs::remove_file(autos.remove(0));
    }
}

/// Whether both paths are on one Windows drive (or one share).
fn same_drive(a: &Path, b: &Path) -> bool {
    let drive = |path: &Path| match path.canonicalize().ok()?.components().next()? {
        std::path::Component::Prefix(prefix) => Some(prefix.as_os_str().to_ascii_lowercase()),
        _ => None,
    };
    matches!((drive(a), drive(b)), (Some(a), Some(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::backup::export;
    use crate::local_server::test_support::shop;
    use uuid::Uuid;

    const PASSWORD: &str = "un-mot-de-passe-solide";

    fn folder() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motodesk_copy_{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn each_backup_is_also_written_to_the_chosen_folder() {
        let state = shop().await;
        let elsewhere = folder();
        assert!(status(&state.data_dir).is_none());
        set(&state.data_dir, &elsewhere.display().to_string()).unwrap();

        let file = export(&state, PASSWORD).await.unwrap();

        let copy = elsewhere.join(&file.filename);
        assert_eq!(fs::read(&copy).unwrap(), fs::read(&file.path).unwrap());
        let status = status(&state.data_dir).unwrap();
        assert!(status.last_copy_at.is_some() && status.error.is_none());
        // Both test folders are in the system's temporary folder.
        assert_eq!(status.same_drive, cfg!(windows));
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 1, "nothing half-written is left");
    }

    #[tokio::test]
    async fn a_missing_folder_does_not_fail_the_backup_and_is_remembered() {
        let state = shop().await;
        let drive = folder();
        set(&state.data_dir, &drive.display().to_string()).unwrap();
        // The drive is unplugged.
        fs::remove_dir_all(&drive).unwrap();

        let file = export(&state, PASSWORD).await.expect("the backup itself is made");

        assert!(Path::new(&file.path).is_file());
        assert!(status(&state.data_dir).unwrap().error.unwrap().contains("introuvable"));
        assert!(!drive.exists(), "a missing drive is not replaced by a new folder");

        // Plugged back in: the next copy clears the error.
        fs::create_dir_all(&drive).unwrap();
        copy(&state.data_dir, Path::new(&file.path));
        assert!(status(&state.data_dir).unwrap().error.is_none());
        assert!(drive.join(&file.filename).is_file());
    }

    #[tokio::test]
    async fn a_folder_that_cannot_protect_anything_is_refused() {
        let state = shop().await;
        let inside = state.data_dir.join("backups");
        fs::create_dir_all(&inside).unwrap();
        for bad in ["", "relative/folder", "Z:/does/not/exist", &inside.display().to_string(), &state.data_dir.display().to_string()] {
            assert!(set(&state.data_dir, bad).is_err(), "{bad:?} was accepted");
        }
        assert!(status(&state.data_dir).is_none());
        clear(&state.data_dir).expect("clearing nothing is fine");
    }

    #[test]
    fn only_the_latest_automatic_copies_are_kept_and_nothing_else_is_removed() {
        let elsewhere = folder();
        let source = folder();
        for n in 0..12 {
            let backup = source.join(format!("auto-202611{:02}-000000.mgbak", n + 1));
            fs::write(&backup, b"x").unwrap();
            copy_into(&elsewhere, &backup).unwrap();
        }
        fs::write(elsewhere.join("photos.zip"), b"mine").unwrap();
        let manual = source.join("motodesk-20261101-000000.mgbak");
        fs::write(&manual, b"x").unwrap();
        copy_into(&elsewhere, &manual).unwrap();

        let mut left: Vec<String> = fs::read_dir(&elsewhere).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left.iter().filter(|name| name.starts_with("auto-")).count(), AUTO_KEPT);
        assert!(left.contains(&"auto-20261112-000000.mgbak".to_string()) && !left.contains(&"auto-20261101-000000.mgbak".to_string()));
        assert!(left.contains(&"photos.zip".to_string()) && left.contains(&"motodesk-20261101-000000.mgbak".to_string()));
    }
}
