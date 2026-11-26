//! Application updates (port of `desktop/src-tauri/src/updates.rs`, of what Tauri's updater did
//! for it, and of the rules in `desktop/src/lib/updates.ts`).
//!
//! The shop is told when a new version exists and decides when to install it; nothing is
//! installed on its own. What makes an update trustworthy:
//!
//! - It is looked for at one address, fixed here: the project's published releases, over HTTPS.
//! - The installer is signed with a private key held only by the maintainer; the matching
//!   public key is compiled in ([`PUBLIC_KEY`]). An installer whose signature does not verify is
//!   never handed over, whoever served it.
//! - A version that is not newer than the running one is never offered (no downgrade), and the
//!   signature must name the version that was announced.
//!
//! This module looks, downloads and verifies. Running the installer is the app's part.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use base64::Engine as _;
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::device_store::DeviceFile;

/// Where a newer version is announced: the manifest attached to the latest published release.
pub const ENDPOINT: &str = "https://github.com/LeBorgneAntoine/MotoDesk/releases/latest/download/latest.json";

/// The key installers are verified against: the public half of the maintainer's signing key
/// (docs/mises-a-jour.md). Changing it cuts every installed copy off from its updates.
pub const PUBLIC_KEY: &str =
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDk3QUE4ODg0MzI4MDE5MjEKUldRaEdZQXloSWlxbHpZRFdhZldEbVVtVExJZkRFSU9yWlZrbzMvNmRtd04zQStrUkpyM2tJWVAK";

/// The entry of the manifest this program installs.
pub const TARGET: &str = "windows-x86_64";

/// First look a little after start, so it never competes with the start itself.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
/// Then look again at this interval while the application stays open.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// « Plus tard » hides the notice for this long.
pub const SNOOZE: Duration = Duration::from_secs(24 * 60 * 60);

/// An installer larger than this is not one of ours.
const MAX_INSTALLER_BYTES: u64 = 500 * 1024 * 1024;

/// Where updates come from and how they are verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub endpoint: String,
    /// A minisign public key, base64 of its file.
    pub public_key: String,
    /// Plain HTTP is accepted. Never for [`Source::official`]: only a stand-in on this PC.
    pub insecure: bool,
}

impl Source {
    /// The project's releases, verified with the key compiled in.
    pub fn official() -> Self {
        Source { endpoint: ENDPOINT.into(), public_key: PUBLIC_KEY.into(), insecure: false }
    }
}

/// Why looking for an update, or getting it, failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// The release server did not answer.
    Unreachable(String),
    /// It answered that no release is published.
    NoRelease,
    /// What it announced cannot be used: unreadable, not for this PC, or not over HTTPS.
    Manifest(String),
    /// The installer is not signed by the maintainer's key, or not for the version announced.
    Signature(String),
    /// The installer could not be written or started.
    File(String),
}

impl UpdateError {
    /// What to tell the user.
    pub fn message(&self) -> &'static str {
        match self {
            UpdateError::Unreachable(_) => "Impossible de joindre le serveur de mises à jour. Vérifiez la connexion Internet.",
            UpdateError::NoRelease => "Aucune version publiée n'a été trouvée sur le serveur de mises à jour.",
            UpdateError::Signature(_) => "La signature de la mise à jour est invalide : elle n'a pas été installée.",
            UpdateError::Manifest(_) | UpdateError::File(_) => "La mise à jour a échoué.",
        }
    }

    /// The technical reason, for the log.
    pub fn detail(&self) -> &str {
        match self {
            UpdateError::NoRelease => "no published release",
            UpdateError::Unreachable(detail) | UpdateError::Manifest(detail) | UpdateError::Signature(detail) | UpdateError::File(detail) => detail,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    pub_date: Option<String>,
    #[serde(default)]
    platforms: HashMap<String, Platform>,
}

#[derive(Debug, Deserialize)]
struct Platform {
    signature: String,
    url: String,
}

/// A newer version, as announced. Nothing of it is trusted until its installer is verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// The version that can be installed, e.g. `0.2.0`.
    pub version: String,
    /// The version running now.
    pub current_version: String,
    /// The release notes: this version's section of the changelog, as markdown.
    pub notes: String,
    /// Publication date, `YYYY-MM-DD`.
    pub date: Option<String>,
    url: String,
    signature: String,
}

fn version(text: &str) -> Option<Version> {
    Version::parse(text.trim().trim_start_matches('v')).ok()
}

/// The day of a publication date: `2026-11-22T10:00:00Z` reads `2026-11-22`.
fn day(date: &str) -> Option<String> {
    let day = date.trim().get(..10)?;
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok().map(|_| day.to_string())
}

/// Read a manifest: the update it announces to a program running `current`, or `None` when
/// that program is current. `insecure` accepts an installer served over plain HTTP.
pub fn offered(manifest: &[u8], current: &str, insecure: bool) -> Result<Option<Update>, UpdateError> {
    let manifest: Manifest = serde_json::from_slice(manifest).map_err(|error| UpdateError::Manifest(format!("unreadable manifest: {error}")))?;
    let announced = version(&manifest.version).ok_or_else(|| UpdateError::Manifest(format!("not a version: {}", manifest.version)))?;
    let running = version(current).ok_or_else(|| UpdateError::Manifest(format!("not a version: {current}")))?;
    // Never a downgrade, never the same version again.
    if announced <= running {
        return Ok(None);
    }
    let platform = manifest.platforms.get(TARGET).ok_or_else(|| UpdateError::Manifest(format!("no installer for {TARGET}")))?;
    if !(platform.url.starts_with("https://") || insecure && platform.url.starts_with("http://")) {
        return Err(UpdateError::Manifest(format!("the installer is not served over HTTPS: {}", platform.url)));
    }
    Ok(Some(Update {
        version: announced.to_string(),
        current_version: running.to_string(),
        notes: manifest.notes.unwrap_or_default(),
        date: manifest.pub_date.as_deref().and_then(day),
        url: platform.url.clone(),
        signature: platform.signature.clone(),
    }))
}

fn http(source: &Source) -> Result<reqwest::Client, UpdateError> {
    // Says only that a Motodesk asks: nothing about the shop.
    reqwest::Client::builder()
        .user_agent("motodesk-updater")
        .connect_timeout(Duration::from_secs(15))
        .https_only(!source.insecure)
        .build()
        .map_err(|error| UpdateError::Unreachable(error.to_string()))
}

/// Ask the release server whether a version newer than `current` exists.
///
/// An unreachable server is an error the screen shows on a manual look and ignores on the
/// automatic one; « no update » is `Ok(None)`.
pub async fn check(source: &Source, current: &str) -> Result<Option<Update>, UpdateError> {
    if !(source.endpoint.starts_with("https://") || source.insecure) {
        return Err(UpdateError::Manifest(format!("updates are not looked for over plain HTTP: {}", source.endpoint)));
    }
    let unreachable = |error: reqwest::Error| UpdateError::Unreachable(error.to_string());
    let answer = http(source)?.get(&source.endpoint).timeout(Duration::from_secs(30)).send().await.map_err(unreachable)?;
    match answer.status().as_u16() {
        204 => return Ok(None),
        404 => return Err(UpdateError::NoRelease),
        status if !(200..300).contains(&status) => return Err(UpdateError::Unreachable(format!("the release server answered {status}"))),
        _ => {}
    }
    offered(&answer.bytes().await.map_err(unreachable)?, current, source.insecure)
}

fn text_of(base64_text: &str) -> Result<String, UpdateError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_text.trim())
        .map_err(|error| UpdateError::Signature(format!("not base64: {error}")))?;
    String::from_utf8(bytes).map_err(|_| UpdateError::Signature("not text".into()))
}

/// The version a signature was made for, when it says: the signing tool writes it in the
/// trusted comment, as `timestamp:…<tab>file:…<tab>version:1.2.3`.
fn signed_version(trusted_comment: &str) -> Option<&str> {
    trusted_comment.split('\t').find_map(|field| field.strip_prefix("version:"))
}

/// Check that `installer` is what the maintainer signed, as the version announced.
///
/// The manifest itself is not signed: on its own, its version does not prove which release
/// its address and signature point at. The signature's own version (read only once the
/// signature has verified: it is covered by it) is what stops a tampered manifest from
/// dressing an older installer up as a new version. A signature that names no version is
/// refused: the release tool always writes it (`tools/release`).
pub fn verify(installer: &[u8], signature: &str, public_key: &str, announced_version: &str) -> Result<(), UpdateError> {
    let invalid = |error: minisign_verify::Error| UpdateError::Signature(error.to_string());
    let key = PublicKey::decode(&text_of(public_key)?).map_err(invalid)?;
    let signature = Signature::decode(&text_of(signature)?).map_err(invalid)?;
    key.verify(installer, &signature, true).map_err(invalid)?;

    let Some(signed) = signed_version(signature.trusted_comment()) else {
        return Err(UpdateError::Signature("the signature does not say which version it was made for".into()));
    };
    let same = match (version(signed), version(announced_version)) {
        (Some(signed), Some(announced)) => signed == announced,
        _ => signed == announced_version,
    };
    if same {
        Ok(())
    } else {
        Err(UpdateError::Signature(format!("signed as version {signed}, announced as {announced_version}")))
    }
}

/// Download the installer of `update` and verify it. `progress` is told how many bytes have
/// come and how many are expected. What is returned is safe to run; nothing else ever is.
pub async fn download(source: &Source, update: &Update, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, UpdateError> {
    let unreachable = |error: reqwest::Error| UpdateError::Unreachable(error.to_string());
    let mut answer = http(source)?.get(&update.url).send().await.map_err(unreachable)?;
    if !answer.status().is_success() {
        return Err(UpdateError::Unreachable(format!("the release server answered {}", answer.status().as_u16())));
    }
    let total = answer.content_length();
    let mut installer = Vec::with_capacity(total.unwrap_or(0).min(MAX_INSTALLER_BYTES) as usize);
    progress(0, total);
    while let Some(chunk) = answer.chunk().await.map_err(unreachable)? {
        installer.extend_from_slice(&chunk);
        if installer.len() as u64 > MAX_INSTALLER_BYTES {
            return Err(UpdateError::Manifest("the installer is larger than any installer of Motodesk".into()));
        }
        progress(installer.len() as u64, total);
    }
    verify(&installer, &update.signature, &source.public_key, &update.version)?;
    if !installer.starts_with(b"MZ") {
        return Err(UpdateError::Manifest("what was signed is not a Windows program".into()));
    }
    Ok(installer)
}

/// What the installer of a version is called on disk.
pub fn installer_name(version: &str) -> String {
    format!("motodesk_{version}_x64-setup.exe")
}

/// The command line the installer is run with, in the words of the installer cargo-packager
/// builds: no questions asked (`/P`: it closes the running program and installs over it), the
/// program started again afterwards (`/R`), and the shortcuts left as the user has them
/// (`/NS`: without it an update puts back a desktop shortcut that was removed).
pub const INSTALLER_PARAMETERS: &str = "/P /R /NS";

// ---- what this PC keeps about updates --------------------------------------------------------

const PREFS_FILE: &str = "updates.store.json";
const PREFS_KEY: &str = "updates.v1";

/// A notice put off: until when, and for which version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snooze {
    pub version: String,
    /// Milliseconds since 1970.
    pub until: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Whether this PC looks for updates by itself (on unless switched off).
    pub auto: bool,
    pub snooze: Option<Snooze>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { auto: true, snooze: None }
    }
}

impl Prefs {
    /// Hide the notice for `version` until tomorrow. A newer version shows it again.
    pub fn snooze(&mut self, version: &str, now_ms: u64) {
        self.snooze = Some(Snooze { version: version.into(), until: now_ms + SNOOZE.as_millis() as u64 });
    }

    /// Whether the banner is shown for this update right now.
    pub fn notice_visible(&self, update: Option<&Update>, now_ms: u64) -> bool {
        let Some(update) = update else { return false };
        !self.snooze.as_ref().is_some_and(|snooze| snooze.version == update.version && snooze.until > now_ms)
    }
}

/// Where [`Prefs`] are kept between two runs.
#[derive(Debug, Clone)]
pub struct PrefsStore {
    file: DeviceFile,
}

impl PrefsStore {
    /// `data_dir` is the app's data folder.
    pub fn in_dir(data_dir: &Path) -> Self {
        Self { file: DeviceFile::in_dir(data_dir, PREFS_FILE) }
    }

    pub fn load(&self) -> Prefs {
        self.file.get(PREFS_KEY).unwrap_or_default()
    }

    pub fn save(&self, prefs: &Prefs) -> std::io::Result<()> {
        self.file.set(PREFS_KEY, prefs)
    }
}

// ---- words -----------------------------------------------------------------------------------

/// Download progress as a whole percentage, `None` while the size is unknown.
pub fn download_percent(downloaded: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|total| *total > 0)?;
    Some((downloaded.saturating_mul(100) / total).min(100) as u8)
}

/// `2026-11-22` becomes `22/11/2026`; anything else, nothing.
pub fn format_release_date(date: Option<&str>) -> String {
    let Some(day) = date.and_then(day) else { return String::new() };
    format!("{}/{}/{}", &day[8..10], &day[5..7], &day[..4])
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use minisign::KeyPair;

    use super::*;

    /// A maintainer's key made for the test, and what it signs.
    struct Maintainer(KeyPair);

    impl Maintainer {
        fn new() -> Self {
            Maintainer(KeyPair::generate_unencrypted_keypair().unwrap())
        }

        /// The public key, as it is compiled into the program.
        fn public_key(&self) -> String {
            base64::engine::general_purpose::STANDARD.encode(self.0.pk.to_box().unwrap().to_string())
        }

        /// The signature of `installer`, as the release carries it. `version` is what the
        /// signing tool records with it.
        fn sign(&self, installer: &[u8], version: Option<&str>) -> String {
            let comment = match version {
                Some(version) => format!("timestamp:1790000000\tfile:setup.exe\tversion:{version}"),
                None => "timestamp:1790000000\tfile:setup.exe".to_string(),
            };
            let signed = minisign::sign(None, &self.0.sk, Cursor::new(installer), Some(&comment), Some("signature from the test")).unwrap();
            base64::engine::general_purpose::STANDARD.encode(signed.to_string())
        }
    }

    fn manifest(version: &str, url: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": version,
            "notes": "### Ajouté\n- Une nouveauté",
            "pub_date": "2026-11-22T10:00:00.000Z",
            "platforms": { "windows-x86_64": { "signature": "c2ln", "url": url } },
        }))
        .unwrap()
    }

    const URL: &str = "https://github.com/LeBorgneAntoine/MotoDesk/releases/download/v0.2.0/Motodesk_0.2.0_x64-setup.exe";

    #[test]
    fn only_a_newer_version_is_offered() {
        let update = offered(&manifest("0.2.0", URL), "0.1.0", false).unwrap().expect("0.2.0 is newer than 0.1.0");
        assert_eq!((update.version.as_str(), update.current_version.as_str()), ("0.2.0", "0.1.0"));
        assert_eq!((update.notes.as_str(), update.date.as_deref()), ("### Ajouté\n- Une nouveauté", Some("2026-11-22")));

        // The same version, an older one, a pre-release of the running one: nothing to install.
        for (announced, running) in [("0.2.0", "0.2.0"), ("0.1.9", "0.2.0"), ("v0.2.0", "0.2.0"), ("1.0.0-rc.1", "1.0.0")] {
            assert_eq!(offered(&manifest(announced, URL), running, false).unwrap(), None, "{announced} over {running}");
        }
        // Compared as versions, not as text.
        assert!(offered(&manifest("0.10.0", URL), "0.9.0", false).unwrap().is_some());
        assert!(offered(&manifest("v1.0.0", URL), "1.0.0-rc.1", false).unwrap().is_some());
    }

    #[test]
    fn a_manifest_that_cannot_be_used_is_refused() {
        assert!(matches!(offered(b"<html>", "0.1.0", false), Err(UpdateError::Manifest(_))));
        assert!(matches!(offered(&manifest("bientôt", URL), "0.1.0", false), Err(UpdateError::Manifest(_))));
        // Never over plain HTTP, except from a stand-in on this PC.
        let plain = manifest("0.2.0", "http://example.invalid/setup.exe");
        assert!(matches!(offered(&plain, "0.1.0", false), Err(UpdateError::Manifest(_))));
        assert!(offered(&plain, "0.1.0", true).unwrap().is_some());
        assert!(matches!(offered(&manifest("0.2.0", "file:///C:/setup.exe"), "0.1.0", true), Err(UpdateError::Manifest(_))));
        // Nothing for this kind of PC.
        let elsewhere = br#"{"version":"0.2.0","platforms":{"darwin-aarch64":{"signature":"c2ln","url":"https://example.invalid/a"}}}"#;
        assert!(matches!(offered(elsewhere, "0.1.0", false), Err(UpdateError::Manifest(_))));
    }

    #[test]
    fn only_what_the_maintainer_signed_for_this_version_verifies() {
        let maintainer = Maintainer::new();
        let installer = b"MZ the installer of 0.2.0".to_vec();
        let signature = maintainer.sign(&installer, Some("0.2.0"));
        let key = maintainer.public_key();
        assert_eq!(verify(&installer, &signature, &key, "0.2.0"), Ok(()));
        assert_eq!(verify(&installer, &signature, &key, "v0.2.0"), Ok(()), "the same version, spelled with its v");

        // One byte changed, another maintainer, the compiled-in key: never.
        let mut tampered = installer.clone();
        tampered[10] ^= 1;
        assert!(matches!(verify(&tampered, &signature, &key, "0.2.0"), Err(UpdateError::Signature(_))));
        assert!(matches!(verify(&installer, &signature, &Maintainer::new().public_key(), "0.2.0"), Err(UpdateError::Signature(_))));
        assert!(matches!(verify(&installer, &signature, PUBLIC_KEY, "0.2.0"), Err(UpdateError::Signature(_))));
        assert!(matches!(verify(&installer, "pas une signature", &key, "0.2.0"), Err(UpdateError::Signature(_))));

        // An older installer announced as a newer version: its signature says which it is.
        assert!(matches!(verify(&installer, &signature, &key, "0.3.0"), Err(UpdateError::Signature(_))));
        // A signature that names no version proves nothing about which release it is: refused.
        let unnamed = maintainer.sign(&installer, None);
        assert!(matches!(verify(&installer, &unnamed, &key, "0.2.0"), Err(UpdateError::Signature(_))));
    }

    #[test]
    fn updates_come_from_the_projects_releases_under_the_maintainers_key() {
        assert!(ENDPOINT.starts_with("https://github.com/LeBorgneAntoine/MotoDesk/releases/"), "an update may only come from the project's releases");
        let official = Source::official();
        assert!(!official.insecure, "updates are never looked for over plain HTTP");
        let key = text_of(PUBLIC_KEY).unwrap();
        assert!(key.starts_with("untrusted comment: minisign public key"), "{key}");
        assert!(PublicKey::decode(&key).is_ok());
    }

    #[test]
    fn the_installer_is_run_without_questions_and_starts_the_program_again() {
        assert_eq!(INSTALLER_PARAMETERS, "/P /R /NS");
        assert_eq!(installer_name("0.2.0"), "motodesk_0.2.0_x64-setup.exe");
    }

    #[test]
    fn the_notice_can_be_put_off_for_a_day() {
        let update = offered(&manifest("0.2.0", URL), "0.1.0", false).unwrap().unwrap();
        let newer = offered(&manifest("0.3.0", URL), "0.1.0", false).unwrap().unwrap();
        let mut prefs = Prefs::default();
        assert!(prefs.auto, "looking by itself is on until switched off");
        assert!(!prefs.notice_visible(None, 0));
        assert!(prefs.notice_visible(Some(&update), 0));

        let (now, day) = (1_000_000, SNOOZE.as_millis() as u64);
        prefs.snooze("0.2.0", now);
        assert!(!prefs.notice_visible(Some(&update), now + day - 1));
        assert!(prefs.notice_visible(Some(&update), now + day + 1), "it comes back the next day");
        assert!(prefs.notice_visible(Some(&newer), now + 1000), "and at once for a newer version");
    }

    #[test]
    fn what_this_pc_chose_is_kept_between_two_runs() {
        let dir = std::env::temp_dir().join(format!("motodesk-updates-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = PrefsStore::in_dir(&dir);
        assert_eq!(store.load(), Prefs::default());
        let mut prefs = Prefs { auto: false, snooze: None };
        prefs.snooze("0.2.0", 5);
        store.save(&prefs).unwrap();
        assert_eq!(PrefsStore::in_dir(&dir).load(), prefs);
        // A damaged file is a file that says nothing.
        std::fs::write(dir.join(PREFS_FILE), b"{not json").unwrap();
        assert_eq!(store.load(), Prefs::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn progress_dates_and_failures_have_their_words() {
        assert_eq!(download_percent(50, Some(200)), Some(25));
        assert_eq!(download_percent(300, Some(200)), Some(100));
        assert_eq!((download_percent(10, None), download_percent(10, Some(0))), (None, None));
        assert_eq!(format_release_date(Some("2026-11-22")), "22/11/2026");
        assert_eq!((format_release_date(None), format_release_date(Some("bientôt"))), (String::new(), String::new()));
        assert!(UpdateError::Signature("x".into()).message().contains("signature"));
        assert!(UpdateError::Unreachable("x".into()).message().contains("connexion Internet"));
        assert!(UpdateError::NoRelease.message().contains("Aucune version publiée"));
        assert_eq!(UpdateError::Manifest("x".into()).message(), "La mise à jour a échoué.");
    }
}
