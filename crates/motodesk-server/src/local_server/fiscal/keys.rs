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

use motodesk_fiscal::SecretKey;
use sqlx::SqlitePool;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::secrets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Load the register's key. A new one is generated only when `may_create` is
/// set, which the caller does exactly when the register itself is new: for a
/// register that already exists, a missing key means it was lost, and quietly
/// minting another would sign every later entry with a key nobody can verify
/// against the public key on record.
pub async fn load_or_create(
    pool: &SqlitePool,
    register_code: &str,
    may_create: bool,
) -> LocalResult<(SecretKey, KeyStore)> {
    if keychain_allowed() {
        if let Ok(Some(hex)) = secrets::load_fiscal_key(register_code) {
            let key = SecretKey::from_hex(&hex).map_err(|e| LocalError::Internal(e.into()))?;
            return Ok((key, KeyStore::Keychain));
        }
    }

    if let Some(hex) = load_from_db(pool, register_code).await? {
        let key = SecretKey::from_hex(&hex).map_err(|e| LocalError::Internal(e.into()))?;
        return Ok((key, KeyStore::Database));
    }

    if !may_create {
        return Err(LocalError::BadRequest(format!(
            "la clé de signature de la caisse {register_code} est introuvable (trousseau du système vidé ou base copiée depuis un autre poste) ;              les écritures fiscales sont bloquées tant que la clé n'est pas restaurée"
        )));
    }

    let key = SecretKey::generate();
    let hex = key.to_hex();

    let stored = if keychain_allowed() {
        secrets::store_fiscal_key(register_code, &hex)
    } else {
        Err(anyhow::anyhow!("keychain disabled by {NO_KEYCHAIN}"))
    };

    match stored {
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
