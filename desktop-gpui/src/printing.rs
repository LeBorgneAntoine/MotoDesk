//! Handing a document to the program this PC opens PDF files with: that is where it is printed
//! from. There is no browser here to ask the system for its print dialog.
//!
//! The file is written in a folder of this app under the system's temporary folder. It holds
//! what the document holds (a customer's name and address, or a recovery key), so the folder
//! is emptied each time the app starts, and a recovery key is removed as soon as it is confirmed.

use std::io;
use std::path::{Path, PathBuf};

use motodesk_ui::gpui::App;

use crate::backend;

/// Where the documents on their way to a reader are kept.
fn folder() -> PathBuf {
    std::env::temp_dir().join("motodesk")
}

/// Where documents are written: the folder above, or in development the folder that answers
/// the "save as" dialogs, where they are then left unopened (see `backend`).
fn out_dir() -> PathBuf {
    backend::dev_save_dir().unwrap_or_else(folder)
}

/// Make sure a document can be written. Asked before anything that cannot be undone (the trace
/// of a print in the fiscal journal), so that a missing folder does not use an original up.
pub fn ready() -> io::Result<()> {
    std::fs::create_dir_all(out_dir())
}

/// A file name made of `stem`, without what a file name cannot hold.
fn safe_name(stem: &str) -> String {
    let name: String = stem.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '-' } else { c }).collect();
    let name = name.trim().trim_end_matches('.');
    if name.is_empty() { "Document".to_string() } else { name.to_string() }
}

fn write_in(folder: &Path, stem: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let stem = safe_name(stem);
    let mut attempt = 1;
    loop {
        let name = if attempt == 1 { format!("{stem}.pdf") } else { format!("{stem} ({attempt}).pdf") };
        let path = folder.join(name);
        match std::fs::write(&path, bytes) {
            Ok(()) => return Ok(path),
            // A reader that still has the earlier file open may keep it from being replaced:
            // the document goes under the next name.
            Err(_) if attempt < 9 => attempt += 1,
            Err(error) => return Err(error),
        }
    }
}

/// Write a PDF as `<stem>.pdf` and say where it is.
pub fn write(stem: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    write_in(&out_dir(), stem, bytes)
}

/// Open a document written by [`write`] in the program this PC reads PDF files with.
pub fn open(path: &Path, cx: &mut App) {
    if backend::dev_save_dir().is_none() {
        cx.open_with_system(path);
    }
}

/// Remove a document that must not stay on this PC. A file a reader still holds stays until
/// the next start.
pub fn forget(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn sweep_in(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.is_file() && path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Remove the documents left from earlier runs. Called when the app starts.
pub fn sweep() {
    sweep_in(&folder());
}

#[cfg(test)]
mod tests {
    use super::{safe_name, sweep_in, write_in};

    fn scratch(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("motodesk-printing-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn a_file_name_holds_nothing_a_file_name_cannot() {
        assert_eq!(safe_name("FA-2026-CF60F-00001 - duplicata 2"), "FA-2026-CF60F-00001 - duplicata 2");
        assert_eq!(safe_name("Facture : a/b\\c \"x\" <1>?*|"), "Facture - a-b-c -x- -1----");
        assert_eq!(safe_name("  Devis.  "), "Devis");
        assert_eq!(safe_name(" . "), "Document");
    }

    #[test]
    fn a_document_goes_under_the_next_name_when_its_own_cannot_be_written() {
        let folder = scratch("write");
        let first = write_in(&folder, "FA-1", b"one").unwrap();
        assert_eq!(first, folder.join("FA-1.pdf"));
        // Written again, it replaces itself.
        assert_eq!(write_in(&folder, "FA-1", b"two").unwrap(), first);
        assert_eq!(std::fs::read(&first).unwrap(), b"two");

        // A name that cannot be written (here, a folder is in the way) does not lose the document.
        std::fs::create_dir(folder.join("FA-2.pdf")).unwrap();
        let other = write_in(&folder, "FA-2", b"three").unwrap();
        assert_eq!(other, folder.join("FA-2 (2).pdf"));
        assert_eq!(std::fs::read(other).unwrap(), b"three");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn starting_removes_the_documents_and_nothing_else() {
        let folder = scratch("sweep");
        write_in(&folder, "FA-1", b"one").unwrap();
        std::fs::write(folder.join("notes.txt"), b"kept").unwrap();
        std::fs::create_dir(folder.join("dossier.pdf")).unwrap();
        sweep_in(&folder);
        let mut left: Vec<String> =
            std::fs::read_dir(&folder).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["dossier.pdf", "notes.txt"]);
        // A folder that is not there is nothing to remove.
        sweep_in(&folder.join("absent"));
        let _ = std::fs::remove_dir_all(folder);
    }
}
