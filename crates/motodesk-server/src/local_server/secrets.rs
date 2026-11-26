//! OS keychain storage for session tokens and the company mesh secret.
//!
//! The OpenRouter key also has a device-local file fallback: Windows Credential
//! Manager (`CRED_PERSIST_ENTERPRISE`) can report a successful write that
//! `CredRead` cannot see. The file lives next to SQLite and is not meshed.

use std::path::Path;

use keyring::Entry;
use sqlx::SqlitePool;

const SERVICE: &str = "com.motodesk.app";
/// The keychain service name of installations made before the product was renamed.
/// Secrets found there are copied under `SERVICE` the first time they are read.
const FORMER_SERVICE: &str = "com.motogest.lite";
const TOKENS_USER: &str = "session-tokens";
const MESH_USER: &str = "mesh-secret";
const BACKUP_USER: &str = "backup-password";
const OPENROUTER_USER: &str = "openrouter-api-key";
const OPENROUTER_FILE: &str = "openrouter.key";
/// Same opt-out as fiscal keys so tests do not pollute the developer keychain.
const NO_KEYCHAIN: &str = "MOTODESK_FISCAL_NO_KEYCHAIN";

/// Set to run a second, isolated copy of the app (screenshots, demos, tests against
/// a throwaway cloud): its secrets go under their own keychain service name, so it
/// can neither read nor overwrite the real installation's session and keys.
pub const PROFILE_VAR: &str = "MOTODESK_PROFILE";

pub fn profile() -> Option<String> {
    std::env::var(PROFILE_VAR).ok().map(|p| p.trim().to_string()).filter(|p| !p.is_empty())
}

fn service_for(profile: Option<&str>) -> String {
    match profile {
        Some(p) => format!("{SERVICE}.{p}"),
        None => SERVICE.to_string(),
    }
}

/// Where secrets are kept: the keychain service names, and the store they are in.
struct Keychain {
    /// The name secrets are written under, and looked for first.
    service: String,
    /// The name secrets were kept under before the product was renamed. An isolated
    /// profile has none: it never borrows from the real installation.
    former: Option<String>,
    /// Opens an entry of the store, which is the platform's credential store. Tests of
    /// what is done with a store put one of their own here.
    open: fn(&str, &str) -> keyring::Result<Entry>,
}

impl Keychain {
    /// This run's: the real installation's names, or an isolated profile's.
    fn current() -> Self {
        Self::of(profile().as_deref())
    }

    fn of(profile: Option<&str>) -> Self {
        Self {
            service: service_for(profile),
            former: profile.is_none().then(|| FORMER_SERVICE.to_string()),
            open: Entry::new,
        }
    }

    fn entry(&self, user: &str) -> keyring::Result<Entry> {
        (self.open)(&self.service, user)
    }

    /// Read a secret, looking under the product's former name when the current one has
    /// nothing, so a renamed install keeps its session, mesh secret and signing keys.
    fn read_secret(&self, user: &str) -> anyhow::Result<Option<String>> {
        match self.entry(user)?.get_password() {
            Ok(s) if !s.is_empty() => return Ok(Some(s)),
            Ok(_) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e.into()),
        }
        self.adopt_former(user)
    }

    fn adopt_former(&self, user: &str) -> anyhow::Result<Option<String>> {
        let Some(former) = &self.former else { return Ok(None) };
        let found = match (self.open)(former, user).and_then(|e| e.get_password()) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) | Err(keyring::Error::NoEntry) => return Ok(None),
            Err(e) => {
                tracing::warn!(error = %e, "could not read the keychain entry kept under the former name");
                return Ok(None);
            }
        };
        // The former entry is left where it is: a copy that failed must not lose the secret.
        match self.set_verified(user, &found) {
            Ok(()) => tracing::info!(entry = user, "keychain entry carried over from the former name"),
            Err(e) => tracing::warn!(error = %e, entry = user, "keychain entry read under the former name but not copied"),
        }
        Ok(Some(found))
    }

    /// Delete a secret under both names: signing out must not leave a copy behind.
    fn delete_everywhere(&self, user: &str) -> keyring::Result<()> {
        if let Some(former) = &self.former {
            if let Ok(former) = (self.open)(former, user) {
                let _ = former.delete_credential();
            }
        }
        self.entry(user)?.delete_credential()
    }

    /// Write a secret and prove it can be read back through a *fresh* handle.
    ///
    /// A platform without a real credential backend (or a build that forgot to
    /// enable one) hands out in-memory entries: `set_password` returns `Ok`, the
    /// value is dropped, and the next `Entry::new` finds nothing. Reading back
    /// through the same handle would not notice, so a new one is opened. Callers
    /// use the `Err` to fall back to another store instead of believing the secret
    /// was kept.
    fn set_verified(&self, user: &str, value: &str) -> anyhow::Result<()> {
        self.entry(user)?.set_password(value)?;
        match self.entry(user)?.get_password() {
            Ok(read) if read == value => Ok(()),
            Ok(_) => anyhow::bail!("le trousseau du système a renvoyé une autre valeur que celle écrite"),
            Err(keyring::Error::NoEntry) => anyhow::bail!(
                "le trousseau du système a accepté l'écriture mais ne la conserve pas"
            ),
            Err(e) => Err(e.into()),
        }
    }
}

fn entry(user: &str) -> anyhow::Result<Entry> {
    Ok(Keychain::current().entry(user)?)
}

fn read_secret(user: &str) -> anyhow::Result<Option<String>> {
    Keychain::current().read_secret(user)
}

fn delete_everywhere(user: &str) -> keyring::Result<()> {
    Keychain::current().delete_everywhere(user)
}

fn set_verified(user: &str, value: &str) -> anyhow::Result<()> {
    Keychain::current().set_verified(user, value)
}

pub fn store_tokens(json: &str) -> anyhow::Result<()> {
    // With the keychain opted out (tests, headless runs) the caller keeps the session in
    // the local database, exactly as it does when the keychain cannot keep it.
    if !keychain_allowed() {
        anyhow::bail!("trousseau du système désactivé");
    }
    set_verified(TOKENS_USER, json)
}

pub fn load_tokens() -> anyhow::Result<Option<String>> {
    if !keychain_allowed() {
        return Ok(None);
    }
    read_secret(TOKENS_USER)
}

/// Pull `access_token` out of a token-bundle JSON blob.
pub fn access_token_from_json(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("access_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Cloud access token: keychain first, then the (usually empty) SQLite cache.
///
/// Login stores tokens in the OS keychain and clears `cached_session.tokens_json`
/// to `{}`. Callers that only read the DB column silently skip cloud bootstrap.
pub async fn session_access_token(db: &SqlitePool) -> anyhow::Result<Option<String>> {
    // The one place every cloud call (push, pull, import, company refresh) gets its
    // token from: a session for another company than this PC's data gets none.
    // A standalone installation has no cloud to present a token to.
    if crate::local_server::local_accounts::is_standalone(db).await {
        return Ok(None);
    }
    if crate::local_server::company::session_mismatches_install(db).await {
        tracing::warn!("the signed-in account belongs to another company than this PC's data; cloud access refused");
        return Ok(None);
    }
    if let Ok(Some(json)) = load_tokens() {
        if let Some(t) = access_token_from_json(&json) {
            return Ok(Some(t));
        }
    }
    let row: Option<(String,)> =
        sqlx::query_as("SELECT tokens_json FROM cached_session WHERE id = 1")
            .fetch_optional(db)
            .await?;
    Ok(row.and_then(|(json,)| access_token_from_json(&json)))
}

pub fn clear_tokens() -> anyhow::Result<()> {
    // Opted out (tests, headless runs): the real credential store is never touched.
    if !keychain_allowed() {
        return Ok(());
    }
    match delete_everywhere(TOKENS_USER) {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn store_mesh_secret(secret: &str) -> anyhow::Result<()> {
    if !keychain_allowed() {
        anyhow::bail!("trousseau du système désactivé");
    }
    set_verified(MESH_USER, secret)
}

/// The password automatic backups are encrypted with. The owner chooses it and
/// is told to keep it elsewhere: it is what opens the files on a new computer.
pub fn store_backup_password(password: &str) -> anyhow::Result<()> {
    set_verified(BACKUP_USER, password)
}

pub fn load_backup_password() -> anyhow::Result<Option<String>> {
    read_secret(BACKUP_USER)
}

pub fn clear_backup_password() -> anyhow::Result<()> {
    match delete_everywhere(BACKUP_USER) {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn load_mesh_secret() -> anyhow::Result<Option<String>> {
    if !keychain_allowed() {
        return Ok(None);
    }
    read_secret(MESH_USER)
}

pub fn clear_mesh_secret() -> anyhow::Result<()> {
    if !keychain_allowed() {
        return Ok(());
    }
    match delete_everywhere(MESH_USER) {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn keychain_allowed() -> bool {
    !matches!(
        std::env::var(NO_KEYCHAIN).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn openrouter_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(OPENROUTER_FILE)
}

fn load_openrouter_file(data_dir: &Path) -> anyhow::Result<Option<String>> {
    let path = openrouter_path(data_dir);
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let trimmed = s.trim().to_string();
            Ok((!trimmed.is_empty()).then_some(trimmed))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn store_openrouter_file(data_dir: &Path, key: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = openrouter_path(data_dir);
    std::fs::write(&path, key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }
    Ok(())
}

fn load_openrouter_keychain() -> anyhow::Result<Option<String>> {
    if !keychain_allowed() {
        return Ok(None);
    }
    match read_secret(OPENROUTER_USER) {
        Ok(found) => Ok(found),
        Err(e) => {
            tracing::warn!(error = %e, "OpenRouter keychain read failed");
            Ok(None)
        }
    }
}

pub fn store_openrouter_key(data_dir: &Path, key: &str) -> anyhow::Result<()> {
    store_openrouter_file(data_dir, key)?;
    if keychain_allowed() {
        match entry(OPENROUTER_USER)?.set_password(key) {
            Ok(()) => {
                if load_openrouter_keychain()?.as_deref() != Some(key) {
                    tracing::warn!(
                        "OpenRouter keychain write did not round-trip; device file is the source of truth"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "OpenRouter key stored on disk; OS keychain unavailable"
                );
            }
        }
    }
    Ok(())
}

pub fn load_openrouter_key(data_dir: &Path) -> anyhow::Result<Option<String>> {
    if let Some(key) = load_openrouter_file(data_dir)? {
        return Ok(Some(key));
    }
    load_openrouter_keychain()
}

pub fn clear_openrouter_key(data_dir: &Path) -> anyhow::Result<()> {
    if keychain_allowed() {
        match delete_everywhere(OPENROUTER_USER) {
            Ok(()) => {}
            Err(keyring::Error::NoEntry) => {}
            Err(e) => tracing::warn!(error = %e, "could not clear OpenRouter key from keychain"),
        }
    }
    match std::fs::remove_file(openrouter_path(data_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Store a register's Ed25519 signing key.
///
/// The keychain is the right home for this: the whole point of signing journal
/// entries is that someone holding a copy of the database still cannot forge a
/// chain. Callers must be prepared for this to fail — a headless build or a
/// locked keyring has no keychain — and fall back to database storage, which
/// keeps chaining intact even though the signature stops being independent
/// evidence. See `local_server::fiscal::keys`.
pub fn store_fiscal_key(register_code: &str, key_hex: &str) -> anyhow::Result<()> {
    set_verified(&fiscal_user(register_code), key_hex)
}

pub fn load_fiscal_key(register_code: &str) -> anyhow::Result<Option<String>> {
    read_secret(&fiscal_user(register_code))
}

fn fiscal_user(register_code: &str) -> String {
    format!("fiscal-key-{register_code}")
}

/// Whether signing out forgets the network's secret. It does for a cloud account, which hands
/// it over again at the next sign-in. It must not on a local network: the secret is what makes
/// the PC a member, nothing can give it back, and without it the PC silently stops sharing.
fn sign_out_forgets_network(local_network: bool) -> bool {
    !local_network
}

/// What signing out removes from the credential store.
pub fn clear_on_sign_out(local_network: bool) {
    let _ = clear_tokens();
    if sign_out_forgets_network(local_network) {
        let _ = clear_mesh_secret();
    }
    // Fiscal keys are deliberately not cleared: a sign-out must not destroy the
    // ability to verify signatures on entries already written.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    // What is done with a credential store (carrying a secret over from the former name,
    // deleting it everywhere, noticing a store that keeps nothing) is tested on stores
    // held in memory. The operating system's store is not a place to assert on step by
    // step: under load Windows reports a write as done before a read can see it, and
    // can bring an entry back after it was deleted. One test, at the end, goes to it.

    thread_local! {
        /// A store that keeps what it is given. Each test runs on a thread of its own,
        /// so each has its own.
        static KEPT: RefCell<HashMap<(String, String), Vec<u8>>> = RefCell::default();
    }

    struct KeptEntry(String, String);

    impl KeptEntry {
        fn key(&self) -> (String, String) {
            (self.0.clone(), self.1.clone())
        }
    }

    impl keyring::credential::CredentialApi for KeptEntry {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            KEPT.with(|kept| kept.borrow_mut().insert(self.key(), secret.to_vec()));
            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            KEPT.with(|kept| kept.borrow().get(&self.key()).cloned()).ok_or(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            KEPT.with(|kept| kept.borrow_mut().remove(&self.key())).map(|_| ()).ok_or(keyring::Error::NoEntry)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn keeping(service: &str, user: &str) -> keyring::Result<Entry> {
        Ok(Entry::new_with_credential(Box::new(KeptEntry(service.to_string(), user.to_string()))))
    }

    /// The store a build without a platform backend gets from `keyring`: every handle
    /// accepts a write, and the next handle knows nothing of it.
    fn forgetting(service: &str, user: &str) -> keyring::Result<Entry> {
        keyring::mock::default_credential_builder()
            .build(None, service, user)
            .map(Entry::new_with_credential)
    }

    /// An installation that was renamed, on the given store.
    fn renamed(open: fn(&str, &str) -> keyring::Result<Entry>) -> Keychain {
        Keychain { service: "current".to_string(), former: Some("former".to_string()), open }
    }

    const USER: &str = "a-secret";

    /// Deletes what a test wrote under `USER`, however the test ends: a failed assertion
    /// must not leave an entry behind.
    struct Cleanup<'a> {
        keychain: &'a Keychain,
        /// How long to keep deleting. The operating system's store can show a write
        /// late, after a delete found nothing to delete.
        patience: Duration,
    }

    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let keychain = self.keychain;
            let until = Instant::now() + self.patience;
            loop {
                for name in [Some(&keychain.service), keychain.former.as_ref()].into_iter().flatten() {
                    if let Ok(entry) = (keychain.open)(name, USER) {
                        let _ = entry.delete_credential();
                    }
                }
                if Instant::now() >= until {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }

    #[test]
    fn signing_out_of_a_local_network_keeps_its_secret() {
        assert!(!sign_out_forgets_network(true));
        assert!(sign_out_forgets_network(false));
    }

    #[test]
    fn reads_access_token_from_bundle() {
        assert_eq!(
            access_token_from_json(r#"{"access_token":"abc","refresh_token":"r"}"#).as_deref(),
            Some("abc")
        );
        assert_eq!(access_token_from_json("{}"), None);
        assert_eq!(access_token_from_json("{\"tokens_json\":{}}"), None);
        assert_eq!(access_token_from_json(""), None);
    }

    /// A platform without a real credential backend must be reported as such,
    /// not treated as a successful save.
    #[test]
    fn set_verified_is_honest_about_the_backend() {
        let kept = renamed(keeping);
        kept.set_verified(USER, "value").unwrap();
        assert_eq!(kept.entry(USER).unwrap().get_password().unwrap(), "value");

        let dropped = renamed(forgetting);
        let outcome = dropped.set_verified(USER, "value");
        assert!(outcome.is_err(), "a store that keeps nothing was believed");
        assert!(matches!(dropped.entry(USER).unwrap().get_password(), Err(keyring::Error::NoEntry)));
    }

    /// A secret saved by an installation made before the rename is found, and carried
    /// over so the next read no longer needs the former name.
    #[test]
    fn a_secret_kept_under_the_former_name_is_found_and_carried_over() {
        let keychain = renamed(keeping);
        keeping("former", USER).unwrap().set_password("kept").unwrap();

        assert_eq!(keychain.read_secret(USER).unwrap().as_deref(), Some("kept"));
        let carried = keeping("current", USER).unwrap().get_password();
        assert_eq!(carried.as_deref().ok(), Some("kept"), "copied under the current name: {carried:?}");
        let left = keeping("former", USER).unwrap().get_password();
        assert_eq!(left.as_deref().ok(), Some("kept"), "the former entry is left where it was: {left:?}");

        // Signing out removes it from both places.
        keychain.delete_everywhere(USER).unwrap();
        assert_eq!(keychain.read_secret(USER).unwrap(), None);
        assert!(KEPT.with(|kept| kept.borrow().is_empty()));
    }

    /// The secret is still handed over when it cannot be copied under the current name.
    #[test]
    fn a_secret_under_the_former_name_is_read_even_if_it_cannot_be_copied() {
        fn only_the_former_is_kept(service: &str, user: &str) -> keyring::Result<Entry> {
            if service == "former" {
                keeping(service, user)
            } else {
                forgetting(service, user)
            }
        }
        let keychain = renamed(only_the_former_is_kept);
        keeping("former", USER).unwrap().set_password("kept").unwrap();
        assert_eq!(keychain.read_secret(USER).unwrap().as_deref(), Some("kept"));
        assert_eq!(keeping("former", USER).unwrap().get_password().unwrap(), "kept");
    }

    /// An isolated profile gets its own keychain service, apart from the real one.
    #[test]
    fn a_profile_has_its_own_keychain_service() {
        assert_eq!(service_for(None), SERVICE);
        assert_eq!(service_for(Some("shots")), format!("{SERVICE}.shots"));
        assert_ne!(service_for(Some("shots")), FORMER_SERVICE);
    }

    /// An isolated profile neither reads nor deletes what the real installation kept
    /// under the former name.
    #[test]
    fn an_isolated_profile_leaves_the_former_name_alone() {
        assert_eq!(Keychain::of(None).former.as_deref(), Some(FORMER_SERVICE));
        assert_eq!(Keychain::of(Some("shots")).former, None);

        let isolated = Keychain { former: None, ..renamed(keeping) };
        keeping("former", USER).unwrap().set_password("kept").unwrap();
        assert_eq!(isolated.read_secret(USER).unwrap(), None);
        isolated.set_verified(USER, "its own").unwrap();
        isolated.delete_everywhere(USER).unwrap();
        assert_eq!(keeping("former", USER).unwrap().get_password().unwrap(), "kept");
    }

    /// The guard the test of the real store relies on.
    #[test]
    fn a_keychain_test_that_fails_leaves_nothing_behind() {
        let keychain = renamed(keeping);
        let failed = std::panic::catch_unwind(|| {
            let _cleanup = Cleanup { keychain: &keychain, patience: Duration::ZERO };
            keychain.set_verified(USER, "value").unwrap();
            keeping("former", USER).unwrap().set_password("value").unwrap();
            assert_eq!(KEPT.with(|kept| kept.borrow().len()), 2);
            panic!("a failed assertion (expected: this test checks what it leaves behind)");
        });
        assert!(failed.is_err());
        assert!(KEPT.with(|kept| kept.borrow().is_empty()));
    }

    /// The profile the test of the real store writes under. Its service name is the
    /// test's own, never the real installation's, and it is the same at every run: an
    /// entry Windows brings back after its deletion is overwritten and deleted again by
    /// the next run, where a name drawn at random would add one more each time.
    const SELFTEST_PROFILE: &str = "selftest";

    fn selftest() -> Keychain {
        Keychain::of(Some(SELFTEST_PROFILE))
    }

    #[test]
    fn the_test_of_the_real_store_has_names_of_its_own() {
        let (selftest, real) = (selftest(), Keychain::of(None));
        assert_eq!(selftest.service, format!("{SERVICE}.selftest"));
        assert_ne!(selftest.service, real.service);
        assert_eq!(selftest.former, None, "nothing is written under the former name");
        let kept_by_the_app = [TOKENS_USER, MESH_USER, BACKUP_USER, OPENROUTER_USER];
        assert!(!kept_by_the_app.contains(&USER) && !USER.starts_with(&fiscal_user("")));
    }

    /// The build is linked with the platform's credential store, and that store keeps
    /// what `set_verified` gives it. Windows always has Credential Manager: this is the
    /// regression test for the build that shipped without the `windows-native` backend,
    /// where every secret went to an in-memory stand-in and was lost.
    #[test]
    fn the_platform_store_keeps_a_secret() {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        assert!(
            !matches!(
                keyring::default::default_credential_builder().persistence(),
                keyring::credential::CredentialPersistence::EntryOnly
            ),
            "keyring was built without this platform's backend: secrets would only be kept in memory"
        );

        let keychain = selftest();
        // Credential Manager can answer "no such entry" just after a write it reported
        // as done (seen on Windows 11 while the whole suite runs); the entry is there a
        // moment later. A store that keeps nothing never passes, however long it is given.
        let patience = if cfg!(windows) { Duration::from_secs(10) } else { Duration::ZERO };
        let _cleanup = Cleanup { keychain: &keychain, patience: patience.min(Duration::from_millis(250)) };
        let until = Instant::now() + patience;
        let outcome = loop {
            let outcome = keychain.set_verified(USER, "value");
            if outcome.is_ok() || Instant::now() >= until {
                break outcome;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        if cfg!(windows) {
            assert!(outcome.is_ok(), "Credential Manager did not keep the secret: {outcome:?}");
        }
    }

    #[test]
    fn openrouter_file_round_trips_without_keychain() {
        std::env::set_var(NO_KEYCHAIN, "1");
        let dir = std::env::temp_dir().join(format!("motodesk_or_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = "sk-or-v1-abcdefghijkl";
        store_openrouter_key(&dir, key).unwrap();
        assert_eq!(load_openrouter_key(&dir).unwrap().as_deref(), Some(key));
        assert!(openrouter_path(&dir).exists());
        clear_openrouter_key(&dir).unwrap();
        assert_eq!(load_openrouter_key(&dir).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
