//! What a release of Motodesk needs besides the installer itself (see `docs/mises-a-jour.md`):
//!
//! ```text
//! motodesk-release notes <version> <CHANGELOG.md> <notes.md>
//! motodesk-release sign <installer.exe> <version>
//! motodesk-release manifest <version> <installer.exe> <owner/repository> <notes.md> <latest.json>
//! ```
//!
//! - `notes` cuts the section of that version out of the changelog: what users read before
//!   they install.
//! - `sign` writes `<installer.exe>.sig` with the maintainer's private key, read from
//!   `MOTODESK_SIGNING_PRIVATE_KEY` (and `MOTODESK_SIGNING_PRIVATE_KEY_PASSWORD`). The signature
//!   records the version it was made for: installed copies refuse an installer announced under
//!   another version, and one whose signature names none.
//! - `manifest` writes `latest.json`, the file installed copies read to learn that a newer
//!   version exists. It needs no protection of its own: the worst a tampered manifest can do is
//!   make an update fail.

use std::io::Cursor;
use std::path::Path;
use std::process::ExitCode;

use base64::Engine as _;
use minisign::{SecretKey, SecretKeyBox};

/// The entry of the manifest the Windows installer goes under.
const TARGET: &str = "windows-x86_64";

type Outcome<T> = Result<T, String>;

fn base64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

fn is_version(text: &str) -> bool {
    let (numbers, suffix) = text.split_once('-').map_or((text, None), |(numbers, suffix)| (numbers, Some(suffix)));
    let parts: Vec<&str> = numbers.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        && suffix.is_none_or(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
}

fn is_repository(text: &str) -> bool {
    let name = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    text.split_once('/').is_some_and(|(owner, repository)| name(owner) && name(repository))
}

/// The section of `version` in a changelog: what is between `## [version]` and the next `## [`.
fn release_notes(changelog: &str, version: &str) -> Outcome<String> {
    let heading = format!("## [{version}]");
    let mut notes = Vec::new();
    let mut inside = false;
    for line in changelog.lines() {
        if line.starts_with("## [") {
            if inside {
                break;
            }
            inside = line.starts_with(&heading);
            continue;
        }
        if inside {
            notes.push(line);
        }
    }
    let notes = notes.join("\n").trim().to_string();
    if notes.is_empty() {
        Err(format!("the changelog has no section for {version}"))
    } else {
        Ok(notes)
    }
}

/// Read the maintainer's private key as the secrets hold it: base64 of its file, protected by
/// `password` (an empty one when the key was made without).
fn private_key(encoded: &str, password: &str) -> Outcome<SecretKey> {
    let bytes =
        base64::engine::general_purpose::STANDARD.decode(encoded.trim()).map_err(|error| format!("the private key is not base64: {error}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "the private key is not text".to_string())?;
    let boxed = SecretKeyBox::from_string(&text).map_err(|error| format!("not a private key: {error}"))?;
    boxed.into_secret_key(Some(password.to_string())).map_err(|error| format!("the private key does not open (wrong password?): {error}"))
}

/// The signature of `installer`, as the manifest carries it. It says which file and which
/// version it was made for; the application reads the version once the signature has verified.
fn signature(installer: &[u8], key: &SecretKey, file_name: &str, version: &str, timestamp: i64) -> Outcome<String> {
    if !is_version(version) {
        return Err(format!("not a version: {version}"));
    }
    let trusted = format!("timestamp:{timestamp}\tfile:{file_name}\tversion:{version}");
    let signed = minisign::sign(None, key, Cursor::new(installer), Some(&trusted), Some("signature from the Motodesk release key"))
        .map_err(|error| format!("signing failed: {error}"))?;
    Ok(base64(&signed.to_string()))
}

/// `latest.json`: the version, its notes, and where its signed installer is.
fn manifest(version: &str, installer_name: &str, signature: &str, repository: &str, notes: &str, published: &str) -> Outcome<String> {
    if !is_version(version) {
        return Err(format!("not a version: {version}"));
    }
    if !is_repository(repository) {
        return Err(format!("not a repository: {repository}"));
    }
    if signature.trim().is_empty() {
        return Err("the signature is empty".into());
    }
    let manifest = serde_json::json!({
        "version": version,
        "notes": notes.trim(),
        "pub_date": published,
        "platforms": {
            TARGET: {
                "signature": signature.trim(),
                "url": format!("https://github.com/{repository}/releases/download/v{version}/{installer_name}"),
            },
        },
    });
    serde_json::to_string_pretty(&manifest).map(|text| text + "\n").map_err(|error| error.to_string())
}

fn file_name(path: &str) -> Outcome<String> {
    Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).ok_or_else(|| format!("not a file: {path}"))
}

fn read(path: &str) -> Outcome<Vec<u8>> {
    std::fs::read(path).map_err(|error| format!("{path}: {error}"))
}

fn read_text(path: &str) -> Outcome<String> {
    std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))
}

fn write(path: &str, content: &str) -> Outcome<()> {
    std::fs::write(path, content).map_err(|error| format!("{path}: {error}"))
}

fn run(arguments: &[String]) -> Outcome<String> {
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match arguments.as_slice() {
        ["notes", version, changelog, out] => {
            write(out, &(release_notes(&read_text(changelog)?, version)? + "\n"))?;
            Ok(format!("wrote {out}: the notes of {version}"))
        }
        ["sign", installer, version] => {
            let encoded = std::env::var("MOTODESK_SIGNING_PRIVATE_KEY").map_err(|_| "MOTODESK_SIGNING_PRIVATE_KEY is not set".to_string())?;
            let password = std::env::var("MOTODESK_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default();
            let key = private_key(&encoded, &password)?;
            let signed = signature(&read(installer)?, &key, &file_name(installer)?, version, chrono::Utc::now().timestamp())?;
            let out = format!("{installer}.sig");
            write(&out, &signed)?;
            Ok(format!("wrote {out}: signed as version {version}"))
        }
        ["manifest", version, installer, repository, notes, out] => {
            let signature = read_text(&format!("{installer}.sig")).map_err(|error| format!("{error} (run `sign` first)"))?;
            let published = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            write(out, &manifest(version, &file_name(installer)?, &signature, repository, &read_text(notes)?, &published)?)?;
            Ok(format!("wrote {out} for {version}"))
        }
        _ => Err("usage:\n  motodesk-release notes <version> <CHANGELOG.md> <notes.md>\n  motodesk-release sign <installer> <version>\n  \
                  motodesk-release manifest <version> <installer> <owner/repository> <notes.md> <latest.json>"
            .into()),
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(done) => {
            println!("{done}");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            // The form a GitHub workflow shows as an error on the run's page.
            eprintln!("::error::{problem}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use minisign::KeyPair;
    use motodesk_client::updates::{self, UpdateError};

    use super::*;

    /// A maintainer's key pair as it is kept: the private half in a secret, under a password.
    fn maintainer(password: &str) -> (String, String) {
        let pair = KeyPair::generate_encrypted_keypair(Some(password.to_string())).unwrap();
        (base64(&pair.sk.to_box(None).unwrap().to_string()), base64(&pair.pk.to_box().unwrap().to_string()))
    }

    const INSTALLER: &[u8] = b"MZ an installer of Motodesk";

    #[test]
    fn what_is_signed_here_is_what_the_application_accepts() {
        let (secret, public) = maintainer("mot de passe du mainteneur");
        let key = private_key(&secret, "mot de passe du mainteneur").unwrap();
        let signed = signature(INSTALLER, &key, "Motodesk_0.2.0_x64-setup.exe", "0.2.0", 1_790_000_000).unwrap();
        assert_eq!(updates::verify(INSTALLER, &signed, &public, "0.2.0"), Ok(()));

        // Announced under another version, or changed by one byte: refused by the application.
        assert!(matches!(updates::verify(INSTALLER, &signed, &public, "0.3.0"), Err(UpdateError::Signature(_))));
        assert!(matches!(updates::verify(b"MZ another installer", &signed, &public, "0.2.0"), Err(UpdateError::Signature(_))));

        // The wrong password does not open the key; a key made without a password opens with none.
        assert!(private_key(&secret, "autre").is_err());
        assert!(private_key("pas une clé", "").is_err());
        let (open_secret, open_public) = maintainer("");
        let signed = signature(INSTALLER, &private_key(&open_secret, "").unwrap(), "setup.exe", "1.0.0-rc.1", 0).unwrap();
        assert_eq!(updates::verify(INSTALLER, &signed, &open_public, "1.0.0-rc.1"), Ok(()));
        assert!(signature(INSTALLER, &key, "setup.exe", "bientôt", 0).is_err());
    }

    #[test]
    fn the_manifest_written_here_is_one_the_application_reads() {
        let written = manifest(
            "0.2.0",
            "Motodesk_0.2.0_x64-setup.exe",
            "c2lnbmF0dXJl\n",
            "LeBorgneAntoine/MotoDesk",
            "### Ajouté\n- Une nouveauté\n",
            "2026-11-22T10:00:00Z",
        )
        .unwrap();
        let update = updates::offered(written.as_bytes(), "0.1.0", false).unwrap().expect("0.2.0 is offered to 0.1.0");
        assert_eq!(
            (update.version.as_str(), update.notes.as_str(), update.date.as_deref()),
            ("0.2.0", "### Ajouté\n- Une nouveauté", Some("2026-11-22"))
        );
        // The installer is where the release of that version keeps it, over HTTPS.
        let read: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(
            read["platforms"]["windows-x86_64"],
            serde_json::json!({
                "signature": "c2lnbmF0dXJl",
                "url": "https://github.com/LeBorgneAntoine/MotoDesk/releases/download/v0.2.0/Motodesk_0.2.0_x64-setup.exe",
            })
        );
        assert_eq!(updates::offered(written.as_bytes(), "0.2.0", false).unwrap(), None);

        assert!(manifest("0.2", "setup.exe", "c2ln", "LeBorgneAntoine/MotoDesk", "", "").is_err());
        assert!(manifest("0.2.0", "setup.exe", "c2ln", "https://example.invalid/x", "", "").is_err());
        assert!(manifest("0.2.0", "setup.exe", " ", "LeBorgneAntoine/MotoDesk", "", "").is_err());
    }

    #[test]
    fn the_notes_of_a_version_are_its_section_of_the_changelog() {
        let changelog = "# Journal\n\n## [Non publié]\n\n### Ajouté\n- À venir\n\n## [0.2.0] - 2026-11-22\n\n### Ajouté\n- Une nouveauté\n\n### Corrigé\n- Un défaut\n\n## [0.1.0] - 2026-10-01\n\n- Première version\n";
        assert_eq!(release_notes(changelog, "0.2.0").unwrap(), "### Ajouté\n- Une nouveauté\n\n### Corrigé\n- Un défaut");
        assert_eq!(release_notes(changelog, "0.1.0").unwrap(), "- Première version");
        // A version without a section is not released: its notes are what users read.
        assert!(release_notes(changelog, "0.3.0").is_err());
        assert!(release_notes(changelog, "0.2").is_err());
    }

    #[test]
    fn versions_and_repositories_are_recognised() {
        for version in ["0.1.0", "12.0.3", "1.2.0-rc.1"] {
            assert!(is_version(version), "{version}");
        }
        for not_one in ["", "1.2", "v1.2.3", "1.2.3-", "1.2.x", "1.2.3 "] {
            assert!(!is_version(not_one), "{not_one}");
        }
        assert!(is_repository("LeBorgneAntoine/MotoDesk"));
        assert!(!is_repository("MotoDesk") && !is_repository("a/b/c") && !is_repository("a b/c"));
    }
}
