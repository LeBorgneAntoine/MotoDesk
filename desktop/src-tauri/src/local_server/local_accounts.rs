//! Local network mode: the shop's own accounts, with no cloud behind them.
//!
//! The first run creates the network by creating its first administrator, with just a username
//! and a password. The accounts are then **replicated between the PCs** of the network (see
//! `mesh::catchup`), so an account created on one PC signs in on the others; a PC that has
//! never been part of the network joins it with such an account (`mesh::join`).
//!
//! The session a sign-in opens is the same `cached_session` row a cloud sign-in writes, marked
//! `auth_mode: "local"`, so every route that asks "who is signed in and are they an admin"
//! works unchanged. No cloud token is kept (see `secrets::session_access_token`).
//!
//! Going online later is `routes::auth::promote`; the design is in `docs/standalone.md`.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::Utc;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;

pub const MIN_PASSWORD_CHARS: usize = 8;
pub const MAX_PASSWORD_CHARS: usize = 128;
const MIN_USERNAME_CHARS: usize = 3;
const MAX_USERNAME_CHARS: usize = 32;
const SRP_SALT_LEN: usize = 16;

/// Pause after a wrong password: cheap, and turns guessing into a slow business.
const FAILED_LOGIN_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

/// Bytes travel as hex in JSON.
mod hexbytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        hex::decode(text).map_err(serde::de::Error::custom)
    }
}

/// One account of the network, as stored and as replicated between the PCs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct Account {
    pub id: String,
    pub username: String,
    /// Argon2id PHC string: signs in on a PC that holds this account.
    pub password_hash: String,
    #[serde(with = "hexbytes")]
    pub kdf_salt: Vec<u8>,
    /// SRP-6a verifier: proves the password to a PC on the LAN without sending it.
    #[serde(with = "hexbytes")]
    pub srp_verifier: Vec<u8>,
    pub role: String,
    pub full_name: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Tombstone: a deleted account stays, so the deletion replicates.
    pub deleted_at: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LocalStatus {
    /// This installation is part of a local network (it runs without a cloud).
    pub standalone: bool,
    /// An account exists (the Local side then asks for a sign-in).
    pub has_account: bool,
    /// Nothing is set up on this PC yet: it can create a network, or join one.
    pub can_setup: bool,
}

// ---- Rules ------------------------------------------------------------------------------

/// A username as stored: trimmed, 3 to 32 letters, digits, `.`, `_` or `-`.
pub fn clean_username(raw: &str) -> LocalResult<String> {
    let name = raw.trim();
    let len = name.chars().count();
    if !(MIN_USERNAME_CHARS..=MAX_USERNAME_CHARS).contains(&len) {
        return Err(LocalError::BadRequest(format!(
            "L'identifiant doit faire de {MIN_USERNAME_CHARS} à {MAX_USERNAME_CHARS} caractères."
        )));
    }
    if !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return Err(LocalError::BadRequest(
            "L'identifiant ne peut contenir que des lettres, des chiffres, « . », « _ » et « - ».".into(),
        ));
    }
    Ok(name.to_string())
}

pub fn check_new_password(password: &str, confirm: &str) -> LocalResult<()> {
    let len = password.chars().count();
    if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&len) {
        return Err(LocalError::BadRequest(format!(
            "Le mot de passe doit faire de {MIN_PASSWORD_CHARS} à {MAX_PASSWORD_CHARS} caractères."
        )));
    }
    if password != confirm {
        return Err(LocalError::BadRequest("Les deux mots de passe ne correspondent pas.".into()));
    }
    Ok(())
}

// ---- Password material ------------------------------------------------------------------

/// The identity SRP works with: the username, lower-cased, as the database compares them.
pub fn srp_identity(username: &str) -> Vec<u8> {
    username.trim().to_lowercase().into_bytes()
}

/// The secret SRP treats as "the password": an Argon2id derivation, so a stolen verifier costs
/// as much to guess against as a stolen password hash.
pub fn derive_secret(password: &str, kdf_salt: &[u8]) -> anyhow::Result<[u8; 32]> {
    let mut out = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), kdf_salt, &mut out)
        .map_err(|e| anyhow::anyhow!("key derivation failed: {e}"))?;
    Ok(out)
}

pub fn compute_verifier(username: &str, password: &str, kdf_salt: &[u8]) -> anyhow::Result<Vec<u8>> {
    let secret = derive_secret(password, kdf_salt)?;
    Ok(srp::client::SrpClient::<Sha256>::new(&srp::groups::G_2048).compute_verifier(
        &srp_identity(username),
        &secret,
        kdf_salt,
    ))
}

/// Build a new account: both the PHC hash and the SRP material come from the password here,
/// the only moment it is known.
pub async fn new_account(
    username: &str,
    password: &str,
    role: &str,
    full_name: Option<String>,
) -> LocalResult<Account> {
    let username = clean_username(username)?;
    let (name, secret) = (username.clone(), password.to_string());
    let (password_hash, kdf_salt, srp_verifier) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let phc_salt = SaltString::generate(&mut OsRng);
        let password_hash = Argon2::default()
            .hash_password(secret.as_bytes(), &phc_salt)
            .map_err(|e| anyhow::anyhow!("hash failed: {e}"))?
            .to_string();
        let mut kdf_salt = vec![0u8; SRP_SALT_LEN];
        OsRng.fill_bytes(&mut kdf_salt);
        let verifier = compute_verifier(&name, &secret, &kdf_salt)?;
        Ok((password_hash, kdf_salt, verifier))
    })
    .await
    .map_err(|e| LocalError::Internal(e.into()))?
    .map_err(LocalError::Internal)?;

    let now = Utc::now().to_rfc3339();
    Ok(Account {
        id: Uuid::new_v4().to_string(),
        username,
        password_hash,
        kdf_salt,
        srp_verifier,
        role: role.to_string(),
        full_name,
        is_active: true,
        created_at: now.clone(),
        updated_at: now,
        deleted_at: None,
    })
}

async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash)
            .ok()
            .is_some_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
    })
    .await
    .unwrap_or(false)
}

// ---- Storage and replication --------------------------------------------------------------

const ACCOUNT_COLUMNS: &str = "id, username, password_hash, kdf_salt, srp_verifier, role, full_name, \
                               is_active, created_at, updated_at, deleted_at";

async fn insert_account<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    a: &Account,
) -> LocalResult<()> {
    sqlx::query(&format!(
        "INSERT INTO local_accounts ({ACCOUNT_COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
    ))
    .bind(&a.id)
    .bind(&a.username)
    .bind(&a.password_hash)
    .bind(&a.kdf_salt)
    .bind(&a.srp_verifier)
    .bind(&a.role)
    .bind(&a.full_name)
    .bind(a.is_active)
    .bind(&a.created_at)
    .bind(&a.updated_at)
    .bind(&a.deleted_at)
    .execute(executor)
    .await?;
    Ok(())
}

/// Write an account over the one with its id, in a single statement: if it cannot be written
/// (its username is taken), the account that was there is still there.
async fn save_account<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    a: &Account,
) -> LocalResult<()> {
    sqlx::query(&format!(
        "INSERT INTO local_accounts ({ACCOUNT_COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT(id) DO UPDATE SET
             username = excluded.username, password_hash = excluded.password_hash,
             kdf_salt = excluded.kdf_salt, srp_verifier = excluded.srp_verifier,
             role = excluded.role, full_name = excluded.full_name, is_active = excluded.is_active,
             created_at = excluded.created_at, updated_at = excluded.updated_at,
             deleted_at = excluded.deleted_at"
    ))
    .bind(&a.id)
    .bind(&a.username)
    .bind(&a.password_hash)
    .bind(&a.kdf_salt)
    .bind(&a.srp_verifier)
    .bind(&a.role)
    .bind(&a.full_name)
    .bind(a.is_active)
    .bind(&a.created_at)
    .bind(&a.updated_at)
    .bind(&a.deleted_at)
    .execute(executor)
    .await?;
    Ok(())
}

// ---- Usernames ----------------------------------------------------------------------------------

/// How usernames are compared: without regard to case, accented letters included. SQLite's own
/// case-insensitive comparison stops at ASCII, so `Élodie` and `élodie` were two people to it.
pub fn fold_username(name: &str) -> String {
    name.trim().to_lowercase()
}

/// The account of a username among these: the one spelled exactly so, else the first that
/// matches without regard to case.
fn pick<'a>(accounts: &'a [Account], username: &str) -> Option<&'a Account> {
    let wanted = fold_username(username);
    accounts
        .iter()
        .find(|a| a.username == username.trim())
        .or_else(|| accounts.iter().find(|a| fold_username(&a.username) == wanted))
}

/// The accounts that have not been deleted, disabled ones included.
async fn live_accounts<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(executor: E) -> LocalResult<Vec<Account>> {
    Ok(sqlx::query_as::<_, Account>(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM local_accounts WHERE deleted_at IS NULL ORDER BY created_at, id"
    ))
    .fetch_all(executor)
    .await?)
}

/// Every account, tombstones included: this is what is replicated.
pub async fn all_accounts(db: &SqlitePool) -> LocalResult<Vec<Account>> {
    Ok(sqlx::query_as::<_, Account>(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM local_accounts ORDER BY created_at, id"
    ))
    .fetch_all(db)
    .await?)
}

/// The account of a username that can sign in, if there is one.
pub async fn find_account(db: &SqlitePool, username: &str) -> LocalResult<Option<Account>> {
    let active: Vec<Account> = live_accounts(db).await?.into_iter().filter(|a| a.is_active).collect();
    Ok(pick(&active, username).cloned())
}

fn plausible(a: &Account) -> bool {
    Uuid::parse_str(&a.id).is_ok()
        && clean_username(&a.username).is_ok()
        && a.password_hash.starts_with("$argon2")
        && (8..=64).contains(&a.kdf_salt.len())
        && !a.srp_verifier.is_empty()
        && a.srp_verifier.len() <= 600
        && matches!(a.role.as_str(), "admin" | "staff" | "employee")
        && chrono::DateTime::parse_from_rfc3339(&a.updated_at).is_ok()
}

/// Fold accounts received from a peer into this PC's: the newest `updated_at` of an id wins.
/// A new account whose username is already taken by another live account is set aside rather
/// than allowed to shadow it. Returns how many accounts changed here.
pub async fn merge_accounts(db: &SqlitePool, incoming: Vec<Account>) -> LocalResult<usize> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let changed = merge_into(&mut tx, incoming).await?;
    tx.commit().await?;
    Ok(changed)
}

async fn merge_into(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, incoming: Vec<Account>) -> LocalResult<usize> {
    let mut changed = 0;
    for a in incoming {
        if !plausible(&a) {
            tracing::warn!(id = %a.id, "ignored an implausible account from a peer");
            continue;
        }
        let held: Option<(String,)> = sqlx::query_as("SELECT updated_at FROM local_accounts WHERE id = $1")
            .bind(&a.id)
            .fetch_optional(&mut **tx)
            .await?;
        match held {
            Some((mine,)) if mine >= a.updated_at => continue,
            Some(_) => {}
            None => {
                let wanted = fold_username(&a.username);
                let taken = live_accounts(&mut **tx)
                    .await?
                    .iter()
                    .any(|other| other.id != a.id && fold_username(&other.username) == wanted);
                if taken && a.deleted_at.is_none() {
                    tracing::warn!(username = %a.username, "an account from a peer reuses a username already taken; set aside");
                    continue;
                }
            }
        }
        match save_account(&mut **tx, &a).await {
            Ok(()) => changed += 1,
            Err(e) => tracing::warn!(error = %e, username = %a.username, "an account from a peer could not be stored"),
        }
    }
    Ok(changed)
}

// ---- State of the installation ------------------------------------------------------------

pub async fn is_standalone(db: &SqlitePool) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT standalone FROM sync_state WHERE id = 1")
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some_and(|n| n != 0)
}

pub async fn status(db: &SqlitePool) -> LocalResult<LocalStatus> {
    let (accounts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM local_accounts WHERE deleted_at IS NULL")
        .fetch_one(db)
        .await?;
    let row: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT standalone, company_id FROM sync_state WHERE id = 1")
            .fetch_optional(db)
            .await?;
    let (standalone, company) = row.unwrap_or((0, None));
    Ok(LocalStatus {
        standalone: standalone != 0,
        has_account: accounts > 0,
        // A PC already tied to a company (online, or offline after a sign-in) neither creates
        // nor joins a network: that would mix two companies' books.
        can_setup: accounts == 0 && company.as_deref().unwrap_or("").is_empty(),
    })
}

/// Keep the company's mesh secret where the other secrets are: the keychain, or the database
/// when there is none.
pub async fn install_mesh_secret(state: &SharedState, secret: &str) -> LocalResult<()> {
    match crate::local_server::secrets::store_mesh_secret(secret) {
        Ok(()) => {
            sqlx::query("UPDATE sync_state SET mesh_secret = NULL WHERE id = 1")
                .execute(&state.db)
                .await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "OS keychain unavailable; keeping the mesh secret in the local database");
            sqlx::query("UPDATE sync_state SET mesh_secret = $1 WHERE id = 1")
                .bind(secret)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(())
}

fn new_mesh_secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

// ---- Create, join, sign in ----------------------------------------------------------------

/// Create the shop's network: its first administrator, its company id and its mesh secret.
pub async fn setup(
    state: &SharedState,
    username: &str,
    password: &str,
    confirm: &str,
) -> LocalResult<Value> {
    clean_username(username)?;
    check_new_password(password, confirm)?;
    let account = new_account(username, password, "admin", None).await?;
    let company_id = Uuid::new_v4();

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let (accounts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM local_accounts WHERE deleted_at IS NULL")
        .fetch_one(&mut *tx)
        .await?;
    let company: Option<String> = sqlx::query_scalar("SELECT company_id FROM sync_state WHERE id = 1")
        .fetch_one(&mut *tx)
        .await?;
    if accounts > 0 || !company.unwrap_or_default().is_empty() {
        return Err(LocalError::Conflict(
            "Ce poste est déjà configuré : connectez-vous.".into(),
        ));
    }
    insert_account(&mut *tx, &account).await?;
    // The secret goes in with the account, so that no crash can leave a network without one;
    // it moves to the credential store once the whole thing is written.
    let secret = new_mesh_secret();
    sqlx::query("UPDATE sync_state SET company_id = $1, standalone = 1, mesh_secret = $2 WHERE id = 1")
        .bind(company_id.to_string())
        .bind(&secret)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    install_mesh_secret(state, &secret).await?;
    // A network PC stays on in the background by default.
    crate::local_server::routes::background::refresh(state).await;
    let mut bundle = open_session(state, &account, company_id).await?;
    // Shown once to the owner: the way back in if the password is ever lost.
    bundle["recovery_key"] = crate::local_server::recovery_key::issue(state).await?.into();
    Ok(bundle)
}

/// What a PC that joins an existing network brings home from the PC it joined.
pub struct Joined {
    pub company_id: Uuid,
    pub mesh_secret: String,
    pub company_name: Option<String>,
    pub legal_name: Option<String>,
    pub accounts: Vec<Account>,
    pub recovery: Option<crate::local_server::recovery_key::Stored>,
}

/// Become a member of an existing network: adopt its company, its mesh secret and its accounts,
/// then sign in as `username`.
pub async fn adopt_network(state: &SharedState, username: &str, joined: Joined) -> LocalResult<Value> {
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let company: Option<String> = sqlx::query_scalar("SELECT company_id FROM sync_state WHERE id = 1")
        .fetch_one(&mut *tx)
        .await?;
    let (accounts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM local_accounts WHERE deleted_at IS NULL")
        .fetch_one(&mut *tx)
        .await?;
    if accounts > 0 || !company.unwrap_or_default().is_empty() {
        return Err(LocalError::Conflict("Ce poste est déjà configuré : connectez-vous.".into()));
    }
    sqlx::query("UPDATE sync_state SET company_id = $1, standalone = 1, mesh_secret = $2 WHERE id = 1")
        .bind(joined.company_id.to_string())
        .bind(&joined.mesh_secret)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        r#"
        INSERT INTO company_cache (id, company_id, name, legal_name, cached_at)
        VALUES (1, $1, $2, $3, $4)
        ON CONFLICT(id) DO UPDATE SET company_id = $1, name = $2, legal_name = $3, cached_at = $4
        "#,
    )
    .bind(joined.company_id.to_string())
    .bind(&joined.company_name)
    .bind(&joined.legal_name)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *tx)
    .await?;
    merge_into(&mut tx, joined.accounts).await?;
    crate::local_server::recovery_key::merge_into(&mut tx, joined.recovery).await?;
    let active: Vec<Account> = live_accounts(&mut *tx).await?.into_iter().filter(|a| a.is_active).collect();
    // Leaving here drops the transaction: the PC stays as it was, free to try again.
    let account = pick(&active, username)
        .cloned()
        .ok_or_else(|| LocalError::Internal(anyhow::anyhow!("the joined network does not hold the account")))?;
    tx.commit().await?;

    install_mesh_secret(state, &joined.mesh_secret).await?;
    crate::local_server::routes::background::refresh(state).await;
    open_session(state, &account, joined.company_id).await
}

/// Undo a join that stopped halfway (an earlier version could): a PC marked as part of a network
/// with no account at all and nothing recorded. It could neither sign in nor join again. Returns
/// whether there was anything to undo. Run before the application starts using the database.
pub async fn repair_half_joined(db: &SqlitePool) -> LocalResult<bool> {
    if !is_standalone(db).await {
        return Ok(false);
    }
    let (traces,): (i64,) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM local_accounts) + (SELECT COUNT(*) FROM change_log)
              + (SELECT COUNT(*) FROM clients) + (SELECT COUNT(*) FROM fiscal_journal)",
    )
    .fetch_one(db)
    .await?;
    if traces > 0 {
        return Ok(false);
    }
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("UPDATE sync_state SET company_id = NULL, standalone = 0, mesh_secret = NULL WHERE id = 1")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM company_cache WHERE id = 1").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

/// Sign in with an account this PC holds.
pub async fn login(state: &SharedState, username: &str, password: &str) -> LocalResult<Value> {
    if !is_standalone(&state.db).await {
        return Err(LocalError::Conflict(
            "Cette installation est reliée à un cloud : utilisez l'accès en ligne.".into(),
        ));
    }
    let account = find_account(&state.db, username).await?;

    // Verified even when the account does not exist, so the answer takes as long.
    let hash = match &account {
        Some(a) => a.password_hash.clone(),
        None => new_account("decoy-user", "not-a-real-password", "employee", None)
            .await?
            .password_hash,
    };
    let ok = verify_password(password.to_string(), hash).await && account.is_some();
    let Some(account) = account.filter(|_| ok) else {
        tokio::time::sleep(FAILED_LOGIN_DELAY).await;
        return Err(LocalError::InvalidCredentials);
    };

    let company: Option<String> = sqlx::query_scalar("SELECT company_id FROM sync_state WHERE id = 1")
        .fetch_one(&state.db)
        .await?;
    let company_id = company
        .and_then(|c| Uuid::parse_str(&c).ok())
        .ok_or_else(|| LocalError::Internal(anyhow::anyhow!("a network member without a company id")))?;
    open_session(state, &account, company_id).await
}

async fn open_session(state: &SharedState, account: &Account, company_id: Uuid) -> LocalResult<Value> {
    let user = json!({
        "id": account.id,
        "company_id": company_id,
        "username": account.username,
        "email": "",
        "full_name": account.full_name.clone().unwrap_or_else(|| account.username.clone()),
        "role": account.role,
        "is_active": account.is_active,
        "created_at": account.created_at,
        "auth_mode": "local",
    });
    // A local session holds no cloud token: what it holds is which password opened it.
    sqlx::query(
        r#"
        INSERT INTO cached_session (id, user_json, tokens_json, cached_at)
        VALUES (1, $1, $2, $3)
        ON CONFLICT(id) DO UPDATE SET user_json = $1, tokens_json = $2, cached_at = $3
        "#,
    )
    .bind(user.to_string())
    .bind(json!({ "credential": credential_mark(account) }).to_string())
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await?;
    state.reload_company().await;
    Ok(json!({
        "access_token": "local",
        "access_expires_in": 0,
        "refresh_token": "",
        "refresh_expires_in": 0,
        "user": user,
    }))
}

// ---- The open session --------------------------------------------------------------------------

/// Which password opened a session, without keeping anything that helps guess it: a digest of
/// the (salted) hash, which changes with every new password.
fn credential_mark(account: &Account) -> String {
    hex::encode(&Sha256::digest(account.password_hash.as_bytes())[..16])
}

/// The signed-in user, if the session still stands.
///
/// A session of a local network is only as good as the account behind it, which an
/// administrator may have changed from another PC since: disabled or deleted, or given a new
/// password, ends the session; a new role or name takes effect at once. A cloud session is the
/// cloud's to judge and is returned as it is.
pub async fn session_user(db: &SqlitePool) -> LocalResult<Option<Value>> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT user_json, tokens_json FROM cached_session WHERE id = 1")
            .fetch_optional(db)
            .await?;
    let Some((user_json, held)) = row else {
        return Ok(None);
    };
    let mut user: Value = serde_json::from_str(&user_json).map_err(|e| LocalError::Internal(e.into()))?;
    if !is_standalone(db).await {
        return Ok(Some(user));
    }

    let id = user.get("id").and_then(Value::as_str).unwrap_or_default();
    let account = sqlx::query_as::<_, Account>(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM local_accounts WHERE id = $1 AND deleted_at IS NULL AND is_active = 1"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?;
    let mark = held
        .and_then(|h| serde_json::from_str::<Value>(&h).ok())
        .and_then(|h| h.get("credential").and_then(Value::as_str).map(str::to_string));
    let Some(account) = account.filter(|a| mark.as_deref() == Some(credential_mark(a).as_str())) else {
        sqlx::query("DELETE FROM cached_session WHERE id = 1").execute(db).await?;
        return Ok(None);
    };

    let full_name = account.full_name.clone().unwrap_or_else(|| account.username.clone());
    let current = [("role", account.role.as_str()), ("username", account.username.as_str()), ("full_name", full_name.as_str())];
    if current.iter().any(|(key, value)| user.get(*key).and_then(Value::as_str) != Some(*value)) {
        for (key, value) in current {
            user[key] = value.into();
        }
        sqlx::query("UPDATE cached_session SET user_json = $1 WHERE id = 1")
            .bind(user.to_string())
            .execute(db)
            .await?;
    }
    Ok(Some(user))
}

// ---- Managing the network's accounts (an administrator) -------------------------------------

/// An account as the screens see it: never the hashes.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct AccountView {
    pub id: String,
    pub username: String,
    pub full_name: Option<String>,
    pub role: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&Account> for AccountView {
    fn from(a: &Account) -> Self {
        Self {
            id: a.id.clone(),
            username: a.username.clone(),
            full_name: a.full_name.clone(),
            role: a.role.clone(),
            is_active: a.is_active,
            created_at: a.created_at.clone(),
            updated_at: a.updated_at.clone(),
        }
    }
}

pub async fn list_views(db: &SqlitePool) -> LocalResult<Vec<AccountView>> {
    Ok(all_accounts(db)
        .await?
        .iter()
        .filter(|a| a.deleted_at.is_none())
        .map(AccountView::from)
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct NewAccount {
    pub username: String,
    pub password: String,
    pub confirm: String,
    pub role: String,
    pub full_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AccountPatch {
    pub role: Option<String>,
    pub full_name: Option<String>,
    pub is_active: Option<bool>,
    pub password: Option<String>,
    pub confirm: Option<String>,
}

fn check_role(role: &str) -> LocalResult<()> {
    if matches!(role, "admin" | "employee") {
        Ok(())
    } else {
        Err(LocalError::BadRequest("Le rôle doit être administrateur ou employé.".into()))
    }
}

/// A change replicates when its `updated_at` is newer than the copy a peer holds, so it must be
/// strictly newer than the one it replaces even if the clock stood still.
fn later_than(previous: &str) -> String {
    let now = Utc::now();
    match chrono::DateTime::parse_from_rfc3339(previous) {
        Ok(prev) if prev.with_timezone(&Utc) >= now => (prev.with_timezone(&Utc) + chrono::Duration::seconds(1)).to_rfc3339(),
        _ => now.to_rfc3339(),
    }
}

async fn require_network(state: &SharedState) -> LocalResult<()> {
    if is_standalone(&state.db).await {
        Ok(())
    } else {
        Err(LocalError::Conflict(
            "Les comptes du réseau local n'existent que sans cloud : les comptes d'une installation en ligne sont gérés par le cloud.".into(),
        ))
    }
}

async fn store(state: &SharedState, account: &Account) -> LocalResult<()> {
    save_account(&state.db, account).await?;
    // Tell the other PCs now rather than at the next round.
    crate::local_server::mesh::catchup::request(state);
    Ok(())
}

/// There must always be an administrator able to sign in.
async fn other_active_admins(db: &SqlitePool, except: &str) -> LocalResult<i64> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM local_accounts
          WHERE deleted_at IS NULL AND is_active = 1 AND role = 'admin' AND id <> $1",
    )
    .bind(except)
    .fetch_one(db)
    .await?;
    Ok(n)
}

pub async fn create_account(state: &SharedState, input: NewAccount) -> LocalResult<AccountView> {
    require_network(state).await?;
    check_role(&input.role)?;
    check_new_password(&input.password, &input.confirm)?;
    let name = clean_username(&input.username)?;
    let existing = live_accounts(&state.db).await?;
    if let Some(other) = pick(&existing, &name) {
        return Err(LocalError::BadRequest(if other.is_active {
            "Cet identifiant est déjà utilisé.".into()
        } else {
            "Cet identifiant appartient à un compte désactivé : réactivez-le.".into()
        }));
    }
    let full_name = input.full_name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let account = new_account(&name, &input.password, &input.role, full_name).await?;
    store(state, &account).await?;
    Ok((&account).into())
}

pub async fn update_account(
    state: &SharedState,
    actor_id: &str,
    id: &str,
    patch: AccountPatch,
) -> LocalResult<AccountView> {
    require_network(state).await?;
    let mut account = all_accounts(&state.db)
        .await?
        .into_iter()
        .find(|a| a.id == id && a.deleted_at.is_none())
        .ok_or(LocalError::NotFound)?;
    let was_admin_able = account.role == "admin" && account.is_active;

    if let Some(role) = &patch.role {
        check_role(role)?;
        account.role = role.clone();
    }
    if let Some(name) = patch.full_name {
        account.full_name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
    }
    if let Some(active) = patch.is_active {
        account.is_active = active;
    }
    if let Some(password) = &patch.password {
        check_new_password(password, patch.confirm.as_deref().unwrap_or(""))?;
        let fresh = new_account(&account.username, password, &account.role, None).await?;
        account.password_hash = fresh.password_hash;
        account.kdf_salt = fresh.kdf_salt;
        account.srp_verifier = fresh.srp_verifier;
    }

    // Do not lock the network out of itself.
    let still_admin_able = account.role == "admin" && account.is_active;
    if was_admin_able && !still_admin_able && other_active_admins(&state.db, id).await? == 0 {
        return Err(LocalError::BadRequest(
            "Il doit rester au moins un administrateur actif sur le réseau.".into(),
        ));
    }
    if id == actor_id && (!account.is_active || account.role != "admin") {
        return Err(LocalError::BadRequest(
            "Vous ne pouvez pas retirer vos propres droits ni désactiver votre propre compte.".into(),
        ));
    }

    account.updated_at = later_than(&account.updated_at);
    store(state, &account).await?;
    // Changing one's own password must not sign one out of the PC it was changed on.
    if id == actor_id && patch.password.is_some() {
        sqlx::query(
            "UPDATE cached_session SET tokens_json = $1
              WHERE id = 1 AND json_extract(user_json, '$.id') = $2",
        )
        .bind(json!({ "credential": credential_mark(&account) }).to_string())
        .bind(id)
        .execute(&state.db)
        .await?;
    }
    Ok((&account).into())
}

/// What the recovery key does to the account it is used on: a new password, active, and an
/// administrator. Returns its id. The caller has checked the key and the password rules.
pub async fn restore_administrator(
    state: &SharedState,
    username: &str,
    password: &str,
) -> LocalResult<String> {
    require_network(state).await?;
    let accounts = live_accounts(&state.db).await?;
    let mut account = pick(&accounts, username)
        .cloned()
        .ok_or_else(|| LocalError::BadRequest("Aucun compte ne porte cet identifiant.".into()))?;
    let fresh = new_account(&account.username, password, "admin", None).await?;
    account.password_hash = fresh.password_hash;
    account.kdf_salt = fresh.kdf_salt;
    account.srp_verifier = fresh.srp_verifier;
    account.role = "admin".into();
    account.is_active = true;
    account.updated_at = later_than(&account.updated_at);
    store(state, &account).await?;
    Ok(account.id)
}

/// Ask the signed-in user for their password again, before something that cannot be undone.
pub async fn check_current_password(db: &SqlitePool, user_id: &str, password: &str) -> LocalResult<()> {
    let hash: Option<String> = sqlx::query_scalar(
        "SELECT password_hash FROM local_accounts WHERE id = $1 AND deleted_at IS NULL AND is_active = 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let ok = match hash {
        Some(hash) => verify_password(password.to_string(), hash).await,
        None => false,
    };
    if !ok {
        tokio::time::sleep(FAILED_LOGIN_DELAY).await;
        return Err(LocalError::BadRequest("Mot de passe incorrect.".into()));
    }
    Ok(())
}

/// Delete an account (it stays as a tombstone so the deletion replicates).
pub async fn delete_account(state: &SharedState, actor_id: &str, id: &str) -> LocalResult<()> {
    require_network(state).await?;
    if id == actor_id {
        return Err(LocalError::BadRequest("Vous ne pouvez pas supprimer votre propre compte.".into()));
    }
    let mut account = all_accounts(&state.db)
        .await?
        .into_iter()
        .find(|a| a.id == id && a.deleted_at.is_none())
        .ok_or(LocalError::NotFound)?;
    if account.role == "admin" && account.is_active && other_active_admins(&state.db, id).await? == 0 {
        return Err(LocalError::BadRequest(
            "Il doit rester au moins un administrateur actif sur le réseau.".into(),
        ));
    }
    let now = later_than(&account.updated_at);
    account.deleted_at = Some(now.clone());
    account.updated_at = now;
    store(state, &account).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::shop;

    #[test]
    fn usernames_are_checked() {
        assert_eq!(clean_username("  camille.martin ").unwrap(), "camille.martin");
        assert!(clean_username("ab").is_err());
        assert!(clean_username(&"x".repeat(33)).is_err());
        assert!(clean_username("camille martin").is_err(), "no spaces");
        assert!(clean_username("camille@example.fr").is_err(), "an email is not a username");
        assert!(clean_username("Léa_2").is_ok());
    }

    #[test]
    fn passwords_are_checked() {
        assert!(check_new_password("short", "short").is_err());
        assert!(check_new_password("long enough", "long enough!").is_err(), "must match");
        assert!(check_new_password("long enough", "long enough").is_ok());
        assert!(check_new_password(&"x".repeat(129), &"x".repeat(129)).is_err());
    }

    #[tokio::test]
    async fn the_first_run_creates_the_network_and_signs_in_its_administrator() {
        let state = shop().await;
        let before = status(&state.db).await.unwrap();
        assert_eq!(before, LocalStatus { standalone: false, has_account: false, can_setup: true });

        let bundle = setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        assert_eq!(bundle["user"]["role"], "admin");
        assert_eq!(bundle["user"]["auth_mode"], "local");

        let after = status(&state.db).await.unwrap();
        assert_eq!(after, LocalStatus { standalone: true, has_account: true, can_setup: false });
        assert!(crate::local_server::auth_guard::is_admin(&state).await);
        assert!(state.company_id.read().await.is_some());
        // The network has the secret its PCs will authenticate each other with.
        assert!(state.mesh_secret.read().await.as_deref().is_some_and(|s| s.len() == 64));
        // A standalone install hands out no cloud token.
        assert_eq!(crate::local_server::secrets::session_access_token(&state.db).await.unwrap(), None);
    }

    #[tokio::test]
    async fn setup_cannot_be_used_twice() {
        let state = shop().await;
        setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        let again = setup(&state, "intrus", "autre mot de passe", "autre mot de passe").await;
        assert!(matches!(again, Err(LocalError::Conflict(_))), "{again:?}");
        assert_eq!(all_accounts(&state.db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_install_tied_to_a_company_does_not_become_standalone() {
        let state = shop().await;
        sqlx::query("UPDATE sync_state SET company_id = '11111111-1111-1111-1111-111111111111' WHERE id = 1")
            .execute(&state.db)
            .await
            .unwrap();
        assert!(!status(&state.db).await.unwrap().can_setup);
        let refused = setup(&state, "camille", "mot de passe", "mot de passe").await;
        assert!(matches!(refused, Err(LocalError::Conflict(_))), "{refused:?}");
    }

    #[tokio::test]
    async fn signing_in_checks_the_password_and_ignores_the_case_of_the_name() {
        let state = shop().await;
        setup(&state, "Camille", "mot de passe", "mot de passe").await.unwrap();
        sqlx::query("DELETE FROM cached_session").execute(&state.db).await.unwrap();

        let wrong = login(&state, "camille", "pas le bon").await;
        assert!(matches!(wrong, Err(LocalError::InvalidCredentials)), "{wrong:?}");
        let unknown = login(&state, "personne", "mot de passe").await;
        assert!(matches!(unknown, Err(LocalError::InvalidCredentials)), "{unknown:?}");
        assert!(!crate::local_server::auth_guard::is_admin(&state).await, "no session after a refusal");

        let ok = login(&state, "CAMILLE", "mot de passe").await.unwrap();
        assert_eq!(ok["user"]["username"], "Camille");
        assert!(crate::local_server::auth_guard::is_admin(&state).await);
    }

    #[tokio::test]
    async fn passwords_are_not_stored_in_clear_and_the_srp_material_matches() {
        let state = shop().await;
        setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        let account = find_account(&state.db, "camille").await.unwrap().unwrap();
        assert!(account.password_hash.starts_with("$argon2id$"), "{}", account.password_hash);
        assert!(!account.password_hash.contains("mot de passe"));
        assert_eq!(
            compute_verifier("Camille", "mot de passe", &account.kdf_salt).unwrap(),
            account.srp_verifier,
            "the verifier is the one this password makes, whatever the username's case"
        );
        assert_ne!(
            compute_verifier("camille", "autre", &account.kdf_salt).unwrap(),
            account.srp_verifier
        );
    }

    #[tokio::test]
    async fn a_sign_in_is_refused_once_the_install_is_online() {
        let state = shop().await;
        setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        sqlx::query("UPDATE sync_state SET standalone = 0 WHERE id = 1").execute(&state.db).await.unwrap();
        let refused = login(&state, "camille", "mot de passe").await;
        assert!(matches!(refused, Err(LocalError::Conflict(_))), "{refused:?}");
    }

    #[tokio::test]
    async fn accounts_merge_newest_wins_and_a_taken_username_is_not_shadowed() {
        let state = shop().await;
        setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        let mine = find_account(&state.db, "camille").await.unwrap().unwrap();

        // A peer holds a newer copy of the same account (a new role) and a new employee.
        let mut newer = mine.clone();
        newer.role = "staff".into();
        newer.updated_at = (Utc::now() + chrono::Duration::seconds(5)).to_rfc3339();
        let employee = new_account("lea", "un autre mot de passe", "employee", Some("Léa".into())).await.unwrap();
        assert_eq!(merge_accounts(&state.db, vec![newer, employee.clone()]).await.unwrap(), 2);
        assert_eq!(find_account(&state.db, "camille").await.unwrap().unwrap().role, "staff");
        assert!(find_account(&state.db, "LEA").await.unwrap().is_some());

        // An older copy changes nothing; merging again changes nothing either.
        let mut older = mine.clone();
        older.role = "employee".into();
        older.updated_at = "2000-01-01T00:00:00+00:00".into();
        assert_eq!(merge_accounts(&state.db, vec![older, employee]).await.unwrap(), 0);
        assert_eq!(find_account(&state.db, "camille").await.unwrap().unwrap().role, "staff");

        // A different account claiming a username already in use is set aside.
        let rival = new_account("camille", "encore un autre", "admin", None).await.unwrap();
        assert_eq!(merge_accounts(&state.db, vec![rival.clone()]).await.unwrap(), 0);
        assert_eq!(all_accounts(&state.db).await.unwrap().len(), 2);

        // A deletion replicates as a tombstone, and frees the username.
        let mut gone = find_account(&state.db, "lea").await.unwrap().unwrap();
        gone.deleted_at = Some(Utc::now().to_rfc3339());
        gone.updated_at = (Utc::now() + chrono::Duration::seconds(10)).to_rfc3339();
        assert_eq!(merge_accounts(&state.db, vec![gone]).await.unwrap(), 1);
        assert!(find_account(&state.db, "lea").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn implausible_accounts_from_a_peer_are_ignored() {
        let state = shop().await;
        let mut bad = new_account("lea", "un autre mot de passe", "employee", None).await.unwrap();
        bad.password_hash = "plaintext".into();
        assert_eq!(merge_accounts(&state.db, vec![bad]).await.unwrap(), 0);
        let mut odd_role = new_account("leo", "un autre mot de passe", "employee", None).await.unwrap();
        odd_role.role = "root".into();
        assert_eq!(merge_accounts(&state.db, vec![odd_role]).await.unwrap(), 0);
        assert!(all_accounts(&state.db).await.unwrap().is_empty());
    }

    async fn network_with_admin() -> (SharedState, String) {
        let state = shop().await;
        let bundle = setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        let id = bundle["user"]["id"].as_str().unwrap().to_string();
        (state, id)
    }

    fn new_employee(name: &str) -> NewAccount {
        NewAccount {
            username: name.into(),
            password: "mot de passe lea".into(),
            confirm: "mot de passe lea".into(),
            role: "employee".into(),
            full_name: Some("Léa Martin".into()),
        }
    }

    #[tokio::test]
    async fn an_admin_creates_an_account_that_can_sign_in_and_never_shows_a_hash() {
        let (state, _) = network_with_admin().await;
        let view = create_account(&state, new_employee("lea")).await.unwrap();
        assert_eq!(view.role, "employee");
        assert_eq!(view.full_name.as_deref(), Some("Léa Martin"));
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("argon2") && !json.contains("verifier"), "{json}");
        assert!(login(&state, "LEA", "mot de passe lea").await.is_ok());
        assert_eq!(list_views(&state.db).await.unwrap().len(), 2);

        let twice = create_account(&state, new_employee("Lea")).await;
        assert!(matches!(twice, Err(LocalError::BadRequest(_))), "a username is taken whatever its case");
        let mut bad = new_employee("leo");
        bad.role = "root".into();
        assert!(create_account(&state, bad).await.is_err());
        let mut mismatch = new_employee("leo");
        mismatch.confirm = "autre".into();
        assert!(create_account(&state, mismatch).await.is_err());
    }

    #[tokio::test]
    async fn changing_a_password_changes_both_the_hash_and_the_srp_material() {
        let (state, admin) = network_with_admin().await;
        let lea = create_account(&state, new_employee("lea")).await.unwrap();
        let before = find_account(&state.db, "lea").await.unwrap().unwrap();

        update_account(
            &state,
            &admin,
            &lea.id,
            AccountPatch { password: Some("un nouveau mot de passe".into()), confirm: Some("un nouveau mot de passe".into()), ..Default::default() },
        )
        .await
        .unwrap();

        let after = find_account(&state.db, "lea").await.unwrap().unwrap();
        assert_ne!(after.password_hash, before.password_hash);
        assert_ne!(after.srp_verifier, before.srp_verifier);
        assert!(after.updated_at > before.updated_at, "newer, so it replicates");
        assert_eq!(compute_verifier("lea", "un nouveau mot de passe", &after.kdf_salt).unwrap(), after.srp_verifier);
        assert!(login(&state, "lea", "mot de passe lea").await.is_err());
        assert!(login(&state, "lea", "un nouveau mot de passe").await.is_ok());
    }

    #[tokio::test]
    async fn a_disabled_or_deleted_account_cannot_sign_in_and_the_network_keeps_an_admin() {
        let (state, admin) = network_with_admin().await;
        let lea = create_account(&state, new_employee("lea")).await.unwrap();

        update_account(&state, &admin, &lea.id, AccountPatch { is_active: Some(false), ..Default::default() })
            .await
            .unwrap();
        assert!(login(&state, "lea", "mot de passe lea").await.is_err(), "disabled");
        update_account(&state, &admin, &lea.id, AccountPatch { is_active: Some(true), ..Default::default() })
            .await
            .unwrap();
        assert!(login(&state, "lea", "mot de passe lea").await.is_ok());

        delete_account(&state, &admin, &lea.id).await.unwrap();
        assert!(login(&state, "lea", "mot de passe lea").await.is_err(), "deleted");
        assert_eq!(list_views(&state.db).await.unwrap().len(), 1);
        assert_eq!(all_accounts(&state.db).await.unwrap().len(), 2, "the tombstone stays, to replicate");

        // The administrator cannot remove or demote themself, and cannot be the last one removed.
        assert!(delete_account(&state, &admin, &admin).await.is_err());
        let demote = update_account(&state, &admin, &admin, AccountPatch { role: Some("employee".into()), ..Default::default() }).await;
        assert!(demote.is_err());
        let disable = update_account(&state, &admin, &admin, AccountPatch { is_active: Some(false), ..Default::default() }).await;
        assert!(disable.is_err());
    }

    #[tokio::test]
    async fn the_last_active_admin_cannot_be_removed_by_another_admin() {
        let (state, admin) = network_with_admin().await;
        let mut second = new_employee("hugo");
        second.role = "admin".into();
        let hugo = create_account(&state, second).await.unwrap();
        // Two admins: one may remove the other...
        let mut third = new_employee("ines");
        third.role = "admin".into();
        let ines = create_account(&state, third).await.unwrap();
        delete_account(&state, &hugo.id, &ines.id).await.unwrap();
        // ...but not when that leaves nobody able to administer.
        delete_account(&state, &hugo.id, &admin).await.unwrap();
        let last = delete_account(&state, "someone-else", &hugo.id).await;
        assert!(matches!(last, Err(LocalError::BadRequest(_))), "{last:?}");
    }

    #[tokio::test]
    async fn accounts_are_managed_only_on_a_network_without_a_cloud() {
        let (state, _) = network_with_admin().await;
        sqlx::query("UPDATE sync_state SET standalone = 0 WHERE id = 1").execute(&state.db).await.unwrap();
        let refused = create_account(&state, new_employee("lea")).await;
        assert!(matches!(refused, Err(LocalError::Conflict(_))), "{refused:?}");
    }

    /// Signs `lea` in on this PC, the administrator having created the account.
    async fn lea_signed_in() -> (SharedState, String, String) {
        let (state, admin) = network_with_admin().await;
        let lea = create_account(&state, new_employee("lea")).await.unwrap();
        login(&state, "lea", "mot de passe lea").await.unwrap();
        (state, admin, lea.id)
    }

    #[tokio::test]
    async fn a_session_ends_when_its_account_is_disabled_or_deleted() {
        for delete in [false, true] {
            let (state, admin, lea) = lea_signed_in().await;
            assert!(session_user(&state.db).await.unwrap().is_some());
            if delete {
                delete_account(&state, &admin, &lea).await.unwrap();
            } else {
                let off = AccountPatch { is_active: Some(false), ..Default::default() };
                update_account(&state, &admin, &lea, off).await.unwrap();
            }
            assert!(session_user(&state.db).await.unwrap().is_none(), "delete={delete}");
            let (left,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cached_session")
                .fetch_one(&state.db)
                .await
                .unwrap();
            assert_eq!(left, 0, "gone for good, not only hidden");
        }
    }

    #[tokio::test]
    async fn a_password_reset_ends_the_sessions_it_did_not_open() {
        let (state, admin, lea) = lea_signed_in().await;
        let reset = AccountPatch {
            password: Some("un nouveau mot de passe".into()),
            confirm: Some("un nouveau mot de passe".into()),
            ..Default::default()
        };
        update_account(&state, &admin, &lea, reset).await.unwrap();
        assert!(session_user(&state.db).await.unwrap().is_none());
        login(&state, "lea", "un nouveau mot de passe").await.unwrap();
        assert!(session_user(&state.db).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn changing_ones_own_password_keeps_one_signed_in() {
        let (state, admin) = network_with_admin().await;
        let own = AccountPatch {
            password: Some("un nouveau mot de passe".into()),
            confirm: Some("un nouveau mot de passe".into()),
            ..Default::default()
        };
        update_account(&state, &admin, &admin, own).await.unwrap();
        assert!(session_user(&state.db).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_role_changed_on_another_pc_applies_to_the_open_session() {
        let (state, _) = network_with_admin().await;
        assert!(crate::local_server::auth_guard::is_admin(&state).await);
        let mut demoted = find_account(&state.db, "camille").await.unwrap().unwrap();
        demoted.role = "employee".into();
        demoted.updated_at = (Utc::now() + chrono::Duration::seconds(5)).to_rfc3339();
        merge_accounts(&state.db, vec![demoted]).await.unwrap();

        let user = session_user(&state.db).await.unwrap().expect("still signed in");
        assert_eq!(user["role"], "employee");
        assert!(!crate::local_server::auth_guard::is_admin(&state).await);
    }

    #[tokio::test]
    async fn an_account_that_cannot_be_saved_leaves_the_old_one_in_place() {
        let (state, _) = network_with_admin().await;
        create_account(&state, new_employee("lea")).await.unwrap();
        let mut clash = find_account(&state.db, "lea").await.unwrap().unwrap();
        clash.username = "camille".into();
        clash.updated_at = (Utc::now() + chrono::Duration::seconds(5)).to_rfc3339();

        assert_eq!(merge_accounts(&state.db, vec![clash.clone()]).await.unwrap(), 0);
        assert!(find_account(&state.db, "lea").await.unwrap().is_some(), "lea is still there");
        assert!(store(&state, &clash).await.is_err());
        assert!(find_account(&state.db, "lea").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn accented_names_are_one_person_whatever_the_case() {
        let state = shop().await;
        setup(&state, "Élodie", "mot de passe", "mot de passe").await.unwrap();
        for typed in ["Élodie", "élodie", "ÉLODIE"] {
            assert!(login(&state, typed, "mot de passe").await.is_ok(), "{typed}");
        }
        assert!(create_account(&state, new_employee("élodie")).await.is_err(), "the name is taken");
        let rival = new_account("élodie", "encore un autre", "employee", None).await.unwrap();
        assert_eq!(merge_accounts(&state.db, vec![rival]).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_disabled_accounts_name_is_refused_with_the_reason() {
        let (state, admin) = network_with_admin().await;
        let lea = create_account(&state, new_employee("lea")).await.unwrap();
        let off = AccountPatch { is_active: Some(false), ..Default::default() };
        update_account(&state, &admin, &lea.id, off).await.unwrap();
        let again = create_account(&state, new_employee("Lea")).await;
        assert!(matches!(again, Err(LocalError::BadRequest(m)) if m.contains("désactivé")));
    }

    #[tokio::test]
    async fn the_network_secret_is_written_with_the_first_account() {
        let (state, _) = network_with_admin().await;
        let stored = crate::local_server::db::current_mesh_secret(&state.db).await.unwrap();
        assert!(stored.is_some());
        assert_eq!(*state.mesh_secret.read().await, stored);
    }

    #[tokio::test]
    async fn a_join_that_fails_leaves_the_pc_free_to_try_again() {
        let state = shop().await;
        let lea = new_account("lea", "un autre mot de passe", "employee", None).await.unwrap();
        let joined = Joined {
            company_id: Uuid::new_v4(),
            mesh_secret: "secret du réseau".into(),
            company_name: Some("Garage".into()),
            legal_name: None,
            accounts: vec![lea],
            recovery: None,
        };
        assert!(adopt_network(&state, "personne", joined).await.is_err());
        assert!(status(&state.db).await.unwrap().can_setup);
        assert!(all_accounts(&state.db).await.unwrap().is_empty());
        assert_eq!(crate::local_server::db::load_mesh_secret(&state.db).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_pc_stuck_halfway_through_a_join_is_freed() {
        let state = shop().await;
        sqlx::query("UPDATE sync_state SET company_id = $1, standalone = 1 WHERE id = 1")
            .bind(Uuid::new_v4().to_string())
            .execute(&state.db)
            .await
            .unwrap();
        assert!(!status(&state.db).await.unwrap().can_setup, "stuck");
        assert!(repair_half_joined(&state.db).await.unwrap());
        assert!(status(&state.db).await.unwrap().can_setup);

        // A working network is left alone.
        let (working, _) = network_with_admin().await;
        assert!(!repair_half_joined(&working.db).await.unwrap());
    }
}
