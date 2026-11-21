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

fn service() -> String {
    service_for(profile().as_deref())
}

fn entry(user: &str) -> anyhow::Result<Entry> {
    Ok(Entry::new(&service(), user)?)
}

/// Read a secret, looking under the product's former name when the current one has
/// nothing, so a renamed install keeps its session, mesh secret and signing keys.
fn read_secret(user: &str) -> anyhow::Result<Option<String>> {
    match entry(user)?.get_password() {
        Ok(s) if !s.is_empty() => return Ok(Some(s)),
        Ok(_) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(e.into()),
    }
    adopt_former(user)
}

fn adopt_former(user: &str) -> anyhow::Result<Option<String>> {
    // An isolated profile never borrows from the real installation.
    if profile().is_some() {
        return Ok(None);
    }
    let found = match Entry::new(FORMER_SERVICE, user).and_then(|e| e.get_password()) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) | Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => {
            tracing::warn!(error = %e, "could not read the keychain entry kept under the former name");
            return Ok(None);
        }
    };
    // The former entry is left where it is: a copy that failed must not lose the secret.
    match set_verified(user, &found) {
        Ok(()) => tracing::info!(entry = user, "keychain entry carried over from the former name"),
        Err(e) => tracing::warn!(error = %e, entry = user, "keychain entry read under the former name but not copied"),
    }
    Ok(Some(found))
}

/// Delete a secret under both names: signing out must not leave a copy behind.
fn delete_everywhere(user: &str) -> keyring::Result<()> {
    if profile().is_none() {
        if let Ok(former) = Entry::new(FORMER_SERVICE, user) {
            let _ = former.delete_credential();
        }
    }
    Entry::new(&service(), user)?.delete_credential()
}

/// Write a secret and prove it can be read back through a *fresh* handle.
///
/// A platform without a real credential backend (or a build that forgot to
/// enable one) hands out in-memory entries: `set_password` returns `Ok`, the
/// value is dropped, and the next `Entry::new` finds nothing. Reading back
/// through the same handle would not notice, so a new one is opened. Callers
/// use the `Err` to fall back to another store instead of believing the secret
/// was kept.
fn set_verified(user: &str, value: &str) -> anyhow::Result<()> {
    entry(user)?.set_password(value)?;
    match entry(user)?.get_password() {
        Ok(read) if read == value => Ok(()),
        Ok(_) => anyhow::bail!("le trousseau du système a renvoyé une autre valeur que celle écrite"),
        Err(keyring::Error::NoEntry) => anyhow::bail!(
            "le trousseau du système a accepté l'écriture mais ne la conserve pas"
        ),
        Err(e) => Err(e.into()),
    }
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

pub fn clear_all() {
    let _ = clear_tokens();
    let _ = clear_mesh_secret();
    // Fiscal keys are deliberately not cleared: a sign-out must not destroy the
    // ability to verify signatures on entries already written.
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

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
        let user = format!("selftest-{}", Uuid::new_v4());
        let outcome = set_verified(&user, "value");
        let readable = matches!(entry(&user).and_then(|e| Ok(e.get_password())), Ok(Ok(v)) if v == "value");
        // Whatever the host provides, the two answers must agree.
        assert_eq!(outcome.is_ok(), readable);
        // Windows always has Credential Manager: this is the regression test for
        // the build that shipped without the `windows-native` backend.
        #[cfg(windows)]
        assert!(outcome.is_ok(), "Credential Manager did not keep the secret: {outcome:?}");
        let _ = entry(&user).map(|e| e.delete_credential());
    }

    /// A secret saved by an installation made before the rename is found, and carried
    /// over so the next read no longer needs the former name.
    #[test]
    fn a_secret_kept_under_the_former_name_is_found_and_carried_over() {
        let user = format!("rename-selftest-{}", Uuid::new_v4());
        let Ok(former) = Entry::new(FORMER_SERVICE, &user) else { return };
        if former.set_password("kept").is_err() || former.get_password().ok().as_deref() != Some("kept") {
            return; // no usable credential store on this host
        }
        assert_eq!(read_secret(&user).unwrap().as_deref(), Some("kept"));
        let carried = Entry::new(SERVICE, &user).and_then(|e| e.get_password());
        assert_eq!(carried.as_deref().ok(), Some("kept"), "copied under the current name");
        // Signing out removes it from both places.
        delete_everywhere(&user).unwrap();
        assert_eq!(read_secret(&user).unwrap(), None);
        let _ = former.delete_credential();
    }

    /// An isolated profile gets its own keychain service, apart from the real one.
    #[test]
    fn a_profile_has_its_own_keychain_service() {
        assert_eq!(service_for(None), SERVICE);
        assert_eq!(service_for(Some("shots")), format!("{SERVICE}.shots"));
        assert_ne!(service_for(Some("shots")), FORMER_SERVICE);
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
