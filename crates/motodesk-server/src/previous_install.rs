//! Pick up the data of an installation made under the product's former name.
//!
//! The app's data folder is named after its identifier, and the identifier changed
//! with the product's name (`com.motogest.lite` -> `com.motodesk.app`). Without this
//! the renamed app would start on an empty database beside the shop's real one.
//!
//! This runs at the very start of `run()`, before the window exists: the webview and
//! the frontend's stores read their files from the folder as soon as they are created.
//! A folder is moved with a rename, which is instant whatever its size.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// What installations made before the rename were called.
pub const FORMER_IDENTIFIER: &str = "com.motogest.lite";
pub const IDENTIFIER: &str = "com.motodesk.app";

/// Set when the former data folder could not be moved (usually because the previous
/// version of the app is still running and holds its files). The app then keeps
/// working on the former folder instead of starting empty, and tries again next time.
static KEPT_IN_PLACE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, PartialEq, Eq)]
pub enum Adoption {
    /// There was no former folder.
    NothingToAdopt,
    /// The former folder now lives at the current path.
    Moved,
    /// Both exist with content: the current one is used and the former is left alone.
    BothPresent,
    /// The move failed; the former folder is still where it was.
    Failed(String),
}

/// Move `former` to `current` unless `current` already holds something.
pub fn adopt_folder(former: &Path, current: &Path) -> Adoption {
    if !former.is_dir() {
        return Adoption::NothingToAdopt;
    }
    if current.exists() {
        let empty = std::fs::read_dir(current).map(|mut it| it.next().is_none()).unwrap_or(false);
        if !empty {
            return Adoption::BothPresent;
        }
        if let Err(e) = std::fs::remove_dir(current) {
            return Adoption::Failed(e.to_string());
        }
    }
    match std::fs::rename(former, current) {
        Ok(()) => Adoption::Moved,
        Err(e) => Adoption::Failed(e.to_string()),
    }
}

/// The folders an app's data lives in: the roaming one (database, stores) and, where
/// the platform has one, the machine-local one (the webview's own data on Windows).
fn data_roots() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        roots.extend(var("APPDATA"));
        roots.extend(var("LOCALAPPDATA"));
    }
    #[cfg(target_os = "macos")]
    {
        roots.extend(var("HOME").map(|h| h.join("Library").join("Application Support")));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        roots.extend(var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share"))));
    }
    roots.dedup();
    roots
}

/// Called once at startup, before anything reads the data folder.
pub fn adopt_previous_install() {
    // An isolated profile (see `secrets::PROFILE_VAR`) must not take the real data.
    if crate::local_server::secrets::profile().is_some() {
        return;
    }
    for (i, root) in data_roots().iter().enumerate() {
        let former = root.join(FORMER_IDENTIFIER);
        let current = root.join(IDENTIFIER);
        match adopt_folder(&former, &current) {
            Adoption::NothingToAdopt => {}
            Adoption::Moved => tracing::info!(from = %former.display(), to = %current.display(), "data of the previous install moved"),
            Adoption::BothPresent => tracing::warn!(
                former = %former.display(),
                "a folder from the previous name still exists next to the current data; it was left alone"
            ),
            Adoption::Failed(why) => {
                tracing::error!(from = %former.display(), error = %why, "could not move the previous install's data");
                // The first root holds the database: keep using it where it is.
                if i == 0 {
                    let _ = KEPT_IN_PLACE.set(former);
                }
            }
        }
    }
}

/// The folder the database lives in: the default one, unless the previous install's
/// could not be moved and is being used in place.
pub fn data_dir_in_use(default: PathBuf) -> PathBuf {
    KEPT_IN_PLACE.get().cloned().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!("motodesk_adopt_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_former_folder_moves_with_its_content() {
        let root = scratch();
        let (former, current) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(&former).unwrap();
        std::fs::write(former.join("local.db"), b"shop data").unwrap();
        assert_eq!(adopt_folder(&former, &current), Adoption::Moved);
        assert_eq!(std::fs::read(current.join("local.db")).unwrap(), b"shop data");
        assert!(!former.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_to_adopt_changes_nothing() {
        let root = scratch();
        assert_eq!(adopt_folder(&root.join("old"), &root.join("new")), Adoption::NothingToAdopt);
        assert!(!root.join("new").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_existing_current_folder_is_never_overwritten() {
        let root = scratch();
        let (former, current) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(&former).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(former.join("local.db"), b"old").unwrap();
        std::fs::write(current.join("local.db"), b"new").unwrap();
        assert_eq!(adopt_folder(&former, &current), Adoption::BothPresent);
        assert_eq!(std::fs::read(current.join("local.db")).unwrap(), b"new");
        assert_eq!(std::fs::read(former.join("local.db")).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_current_folder_is_replaced() {
        let root = scratch();
        let (former, current) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(&former).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(former.join("local.db"), b"shop data").unwrap();
        assert_eq!(adopt_folder(&former, &current), Adoption::Moved);
        assert_eq!(std::fs::read(current.join("local.db")).unwrap(), b"shop data");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_move_that_cannot_happen_leaves_the_former_folder_untouched() {
        let root = scratch();
        let former = root.join("old");
        std::fs::create_dir_all(&former).unwrap();
        std::fs::write(former.join("local.db"), b"shop data").unwrap();
        // The destination's parent does not exist, so the rename fails.
        let current = root.join("missing").join("new");
        assert!(matches!(adopt_folder(&former, &current), Adoption::Failed(_)));
        assert_eq!(std::fs::read(former.join("local.db")).unwrap(), b"shop data");
        let _ = std::fs::remove_dir_all(&root);
    }
}
