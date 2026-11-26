//! Where a register's signing key lives.
//!
//! First choice is the OS keychain, because a signature is only evidence if
//! copying the database is not enough to produce one. Some environments have no
//! usable keychain -- a service account, a locked session, CI -- and refusing to
//! invoice at all would be the wrong trade. So there is a fallback table, and
//! the register records which of the two it used, so an audit can tell the
//! difference between "signed with a key an attacker could not reach" and
//! "signed with a key that sits next to the data it signs".
//!
//! Chaining is unaffected either way: the tamper-evidence of the journal does
//! not depend on where the key is kept.
//!
//! Both places can hold a key at once, and not always the same one: a keychain
//! that answers "not kept" for a write it did keep, a repair that was refused
//! after its key was stored, a restore onto a computer whose keychain still has
//! the key of the data it replaces. Which of them signs is therefore never a
//! matter of where a key was found first. The register's journal names one
//! public key; the key that signs is the one at hand whose public half that is
//! ([`named_by`]), and when none is, nothing signs. Everything that needs "the
//! register's key" goes through that one choice: opening the till
//! ([`load_or_create`]), the diagnosis (`keyrepair`), and what a backup carries.

use motodesk_fiscal::SecretKey;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::secrets;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStore {
    Keychain,
    Database,
}

impl KeyStore {
    pub fn as_str(self) -> &'static str {
        match self {
            KeyStore::Keychain => "keychain",
            KeyStore::Database => "database",
        }
    }
}

/// Set to opt out of the keychain. Tests use it so a run does not leave a key
/// per throwaway register behind in the developer's real credential store.
const NO_KEYCHAIN: &str = "MOTODESK_FISCAL_NO_KEYCHAIN";

fn keychain_allowed() -> bool {
    !matches!(
        std::env::var(NO_KEYCHAIN).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

/// The system keychain, as far as signing keys go. It is passed as a value so
/// that what is done with the keys can be tested on a store that is not the
/// operating system's; the application always passes [`SystemKeychain`].
pub(crate) trait SigningKeychain: Send + Sync {
    /// The key kept for a register, when the keychain can be read and has one.
    fn load(&self, register_code: &str) -> Option<String>;
    /// Keep a register's key. `Err` means "do not count on it", not "it is not
    /// there": Windows can show a write after having answered that it was not
    /// kept, so a key may sit in the keychain although this said no.
    fn store(&self, register_code: &str, key_hex: &str) -> anyhow::Result<()>;
}

/// The operating system's credential store, unless opted out of.
pub(crate) struct SystemKeychain;

impl SigningKeychain for SystemKeychain {
    fn load(&self, register_code: &str) -> Option<String> {
        if !keychain_allowed() {
            return None;
        }
        secrets::load_fiscal_key(register_code).ok().flatten()
    }

    fn store(&self, register_code: &str, key_hex: &str) -> anyhow::Result<()> {
        if !keychain_allowed() {
            anyhow::bail!("keychain disabled by {NO_KEYCHAIN}");
        }
        secrets::store_fiscal_key(register_code, key_hex)
    }
}

/// Every secret key kept for a register on this computer: the keychain's, then
/// the fallback table's. Read-only. An entry that is not a key is left out, so
/// that it cannot hide a real one kept in the other place.
pub(crate) async fn at_hand(
    keychain: &dyn SigningKeychain,
    pool: &SqlitePool,
    register_code: &str,
) -> LocalResult<Vec<(KeyStore, SecretKey)>> {
    let mut found = Vec::new();
    let mut keep = |store: KeyStore, hex: String| match SecretKey::from_hex(&hex) {
        Ok(key) => found.push((store, key)),
        Err(_) => tracing::warn!(
            register = register_code,
            store = store.as_str(),
            "what is kept as this register's signing key is not a key; ignored"
        ),
    };
    if let Some(hex) = keychain.load(register_code) {
        keep(KeyStore::Keychain, hex);
    }
    if let Some(hex) = load_from_db(pool, register_code).await? {
        keep(KeyStore::Database, hex);
    }
    Ok(found)
}

/// Among the keys at hand, the one whose public half is `public_key_hex`: the
/// key a journal names. A key found first, or found in the better place, is not
/// the register's key unless the journal says so; when the same key is kept in
/// both places, the keychain's copy is the one returned.
pub(crate) fn named_by<'a>(
    public_key_hex: &str,
    at_hand: &'a [(KeyStore, SecretKey)],
) -> Option<&'a (KeyStore, SecretKey)> {
    at_hand.iter().find(|(_, key)| key.public().to_hex() == public_key_hex)
}

/// Load the register's key.
///
/// `recorded_public_key` is the key the register's journal names, and `None`
/// exactly when the register itself is new. A register that exists signs with
/// the key at hand that its journal names, wherever it is kept, and with no
/// other: when that key is not at hand it was lost, and quietly minting another
/// (or using one that happens to be there) would sign every later entry with a
/// key nobody can verify against the public key on record. Only a new register
/// takes a key already kept under its code or, failing that, has one made.
pub(crate) async fn load_or_create(
    keychain: &dyn SigningKeychain,
    pool: &SqlitePool,
    register_code: &str,
    recorded_public_key: Option<&str>,
) -> LocalResult<(SecretKey, KeyStore)> {
    let mut found = at_hand(keychain, pool, register_code).await?;

    if let Some(recorded) = recorded_public_key {
        if found.is_empty() {
            return Err(LocalError::BadRequest(format!(
                "la clé de signature de la caisse {register_code} est introuvable (trousseau du système vidé ou base copiée depuis un autre poste) ; \
                 les écritures fiscales sont bloquées tant que la clé n'est pas restaurée"
            )));
        }
        let Some(at) = found.iter().position(|(_, key)| key.public().to_hex() == recorded) else {
            return Err(LocalError::BadRequest(format!(
                "la clé de signature de la caisse {register_code} ne correspond pas à la clé publique enregistrée ; \
                 les écritures fiscales sont bloquées pour ne pas produire de signatures invalides"
            )));
        };
        let (store, key) = found.swap_remove(at);
        return Ok((key, store));
    }

    if !found.is_empty() {
        let (store, key) = found.swap_remove(0);
        return Ok((key, store));
    }

    let key = SecretKey::generate();
    let hex = key.to_hex();

    match keychain.store(register_code, &hex) {
        Ok(()) => Ok((key, KeyStore::Keychain)),
        Err(err) => {
            tracing::warn!(
                register = register_code,
                error = %err,
                "no usable keychain; the fiscal signing key falls back to the local database"
            );
            store_in_db(pool, register_code, &hex).await?;
            Ok((key, KeyStore::Database))
        }
    }
}

async fn load_from_db(pool: &SqlitePool, register_code: &str) -> LocalResult<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT secret_key FROM fiscal_key_fallback WHERE register_code = $1")
            .bind(register_code)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0))
}

async fn store_in_db(pool: &SqlitePool, register_code: &str, key_hex: &str) -> LocalResult<()> {
    sqlx::query(
        "INSERT INTO fiscal_key_fallback (register_code, secret_key, created_at) VALUES ($1, $2, $3)
         ON CONFLICT(register_code) DO NOTHING",
    )
    .bind(register_code)
    .bind(key_hex)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::{shop, MemoryKeychain, OnStore};

    fn copy(key: &SecretKey) -> SecretKey {
        SecretKey::from_hex(&key.to_hex()).unwrap()
    }

    #[test]
    fn the_key_a_journal_names_is_chosen_wherever_it_is_kept() {
        let named = SecretKey::generate();
        let other = SecretKey::generate();
        let public = named.public().to_hex();

        // Found second, in the lesser place: still the one.
        let at_hand = vec![(KeyStore::Keychain, copy(&other)), (KeyStore::Database, copy(&named))];
        let (store, key) = named_by(&public, &at_hand).unwrap();
        assert_eq!((*store, key.to_hex()), (KeyStore::Database, named.to_hex()));

        // The same key in both places: the keychain's copy.
        let at_hand = vec![(KeyStore::Keychain, copy(&named)), (KeyStore::Database, copy(&named))];
        assert_eq!(named_by(&public, &at_hand).unwrap().0, KeyStore::Keychain);

        // Keys at hand, none of them the named one: no key.
        let at_hand = vec![(KeyStore::Keychain, copy(&other)), (KeyStore::Database, copy(&other))];
        assert!(named_by(&public, &at_hand).is_none());
        assert!(named_by(&public, &[]).is_none());
    }

    #[tokio::test]
    async fn an_existing_register_gets_the_named_key_or_none() {
        let pool = shop().await.db.clone();
        let named = SecretKey::generate();
        let other = SecretKey::generate();
        let public = named.public().to_hex();
        let keychain = MemoryKeychain::new();

        // Nothing kept anywhere: the key is lost, and none is made.
        let lost = load_or_create(&keychain, &pool, "C0001", Some(&public)).await.err().unwrap();
        assert!(matches!(&lost, LocalError::BadRequest(m) if m.contains("introuvable")), "{lost:?}");
        assert_eq!(keychain.get("C0001"), None, "no key is made for a register that exists");

        // Another key in the keychain, the named one in the database.
        keychain.put("C0001", &other.to_hex());
        store_in_db(&pool, "C0001", &named.to_hex()).await.unwrap();
        let (key, store) = load_or_create(&keychain, &pool, "C0001", Some(&public)).await.unwrap();
        assert_eq!((key.to_hex(), store), (named.to_hex(), KeyStore::Database));

        // The other way round.
        keychain.put("C0002", &named.to_hex());
        store_in_db(&pool, "C0002", &other.to_hex()).await.unwrap();
        let (key, store) = load_or_create(&keychain, &pool, "C0002", Some(&public)).await.unwrap();
        assert_eq!((key.to_hex(), store), (named.to_hex(), KeyStore::Keychain));

        // Keys in both places, neither the named one: refused, not "the first found".
        keychain.put("C0003", &other.to_hex());
        store_in_db(&pool, "C0003", &SecretKey::generate().to_hex()).await.unwrap();
        let refused = load_or_create(&keychain, &pool, "C0003", Some(&public)).await.err().unwrap();
        assert!(matches!(&refused, LocalError::BadRequest(m) if m.contains("ne correspond pas")), "{refused:?}");
    }

    /// What sits under the register's name in the keychain but is not a key must not hide
    /// the key kept in the database.
    #[tokio::test]
    async fn what_is_not_a_key_does_not_hide_the_one_kept_elsewhere() {
        let pool = shop().await.db.clone();
        let named = SecretKey::generate();
        let keychain = MemoryKeychain::new();
        keychain.put("C0001", "ceci n'est pas une clé");
        store_in_db(&pool, "C0001", &named.to_hex()).await.unwrap();
        let (key, store) =
            load_or_create(&keychain, &pool, "C0001", Some(&named.public().to_hex())).await.unwrap();
        assert_eq!((key.to_hex(), store), (named.to_hex(), KeyStore::Database));
    }

    /// A new register whose keychain says "not kept" for a write it did keep ends up with
    /// its key in both places. It is the same key, and the register finds it afterwards.
    #[tokio::test]
    async fn a_new_register_whose_keychain_keeps_the_key_it_refused_has_one_key() {
        let pool = shop().await.db.clone();
        let keychain = MemoryKeychain::that(OnStore::KeepsButSaysNo);
        let (key, store) = load_or_create(&keychain, &pool, "C0001", None).await.unwrap();
        assert_eq!(store, KeyStore::Database, "declared where the key is known to be");
        assert_eq!(keychain.get("C0001"), Some(key.to_hex()));
        assert_eq!(load_from_db(&pool, "C0001").await.unwrap(), Some(key.to_hex()));

        let (again, _) = load_or_create(&keychain, &pool, "C0001", Some(&key.public().to_hex())).await.unwrap();
        assert_eq!(again.to_hex(), key.to_hex());

        // A keychain that keeps nothing: the database alone.
        let none = MemoryKeychain::that(OnStore::Refuses);
        let (key, store) = load_or_create(&none, &pool, "C0002", None).await.unwrap();
        assert_eq!(store, KeyStore::Database);
        assert_eq!(none.get("C0002"), None);
        assert_eq!(load_from_db(&pool, "C0002").await.unwrap(), Some(key.to_hex()));

        // One that works: the keychain alone.
        let works = MemoryKeychain::new();
        let (key, store) = load_or_create(&works, &pool, "C0003", None).await.unwrap();
        assert_eq!(store, KeyStore::Keychain);
        assert_eq!(works.get("C0003"), Some(key.to_hex()));
        assert_eq!(load_from_db(&pool, "C0003").await.unwrap(), None);
    }
}
