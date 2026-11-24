//! The network's recovery key (not to be confused with `recovery`, which fetches data back
//! from a cloud).
//!
//! A local network has no server to send a "forgot my password" link: if the only
//! administrator forgets theirs, nobody can manage the accounts again. When the network is
//! created, a random key is shown once for the owner to write down. Whoever holds it can give
//! one account a new password and the administrator role, on any PC of the network. Using it
//! replaces it. Only its hash is stored.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::Utc;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::local_accounts;
use crate::local_server::state::SharedState;

/// Letters and digits that cannot be mistaken for one another once written by hand
/// (Crockford's base 32: no I, L, O or U).
const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const GROUPS: usize = 5;
const GROUP_CHARS: usize = 5;
const KEY_CHARS: usize = GROUPS * GROUP_CHARS;

const LOCK_AFTER_FAILURES: u32 = 5;
const LOCK_WINDOW: Duration = Duration::from_secs(60);
const WRONG_KEY_DELAY: Duration = Duration::from_millis(500);

/// A new key, 125 random bits: `XXXXX-XXXXX-XXXXX-XXXXX-XXXXX`.
pub fn generate() -> String {
    let mut bytes = [0u8; KEY_CHARS];
    OsRng.fill_bytes(&mut bytes);
    // 32 divides 256: every character is as likely as the others.
    let chars: Vec<char> = bytes.iter().map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char).collect();
    chars
        .chunks(GROUP_CHARS)
        .map(|group| group.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// What was typed, as it is compared: case, spaces and hyphens do not matter, and the letters
/// the alphabet leaves out are read as the digits they look like.
fn normalise(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect()
}

/// The hash as stored and replicated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct Stored {
    pub key_hash: String,
    pub confirmed: bool,
    pub updated_at: String,
}

pub async fn stored(db: &SqlitePool) -> LocalResult<Option<Stored>> {
    Ok(sqlx::query_as::<_, Stored>("SELECT key_hash, confirmed, updated_at FROM network_recovery WHERE id = 1")
        .fetch_optional(db)
        .await?)
}

async fn write<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(executor: E, s: &Stored) -> LocalResult<()> {
    sqlx::query(
        "INSERT INTO network_recovery (id, key_hash, confirmed, updated_at) VALUES (1, $1, $2, $3)
         ON CONFLICT(id) DO UPDATE SET key_hash = $1, confirmed = $2, updated_at = $3",
    )
    .bind(&s.key_hash)
    .bind(s.confirmed)
    .bind(&s.updated_at)
    .execute(executor)
    .await?;
    Ok(())
}

fn when(stamp: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(stamp).map_or(i64::MIN, |t| t.timestamp_micros())
}

/// Strictly after the copy being replaced, even if the clock stood still or runs behind.
fn next_stamp(previous: Option<&Stored>) -> String {
    let now = Utc::now();
    match previous.and_then(|p| chrono::DateTime::parse_from_rfc3339(&p.updated_at).ok()) {
        Some(p) if p.with_timezone(&Utc) >= now => (p.with_timezone(&Utc) + chrono::Duration::seconds(1)).to_rfc3339(),
        _ => now.to_rfc3339(),
    }
}

/// Take a peer's copy if it is newer. Returns whether it changed anything.
pub async fn merge(db: &SqlitePool, incoming: Option<Stored>) -> LocalResult<bool> {
    let Some(incoming) = incoming else { return Ok(false) };
    if !incoming.key_hash.starts_with("$argon2") || when(&incoming.updated_at) == i64::MIN {
        tracing::warn!("ignored an implausible recovery record from a peer");
        return Ok(false);
    }
    if stored(db).await?.is_some_and(|mine| when(&mine.updated_at) >= when(&incoming.updated_at)) {
        return Ok(false);
    }
    write(db, &incoming).await?;
    Ok(true)
}

/// Replace the network's key and return the new one, the only time it exists in clear.
pub async fn issue(state: &SharedState) -> LocalResult<String> {
    let key = generate();
    let plain = normalise(&key);
    let key_hash = tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default().hash_password(plain.as_bytes(), &salt).map(|h| h.to_string())
    })
    .await
    .map_err(|e| LocalError::Internal(e.into()))?
    .map_err(|e| LocalError::Internal(anyhow::anyhow!("hash failed: {e}")))?;

    let previous = stored(&state.db).await?;
    let record = Stored { key_hash, confirmed: false, updated_at: next_stamp(previous.as_ref()) };
    write(&state.db, &record).await?;
    crate::local_server::mesh::catchup::request(state);
    Ok(key)
}

async fn matches(record: &Stored, typed: &str) -> bool {
    let (plain, hash) = (normalise(typed), record.key_hash.clone());
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash)
            .ok()
            .is_some_and(|h| Argon2::default().verify_password(plain.as_bytes(), &h).is_ok())
    })
    .await
    .unwrap_or(false)
}

/// The owner typed the key back: it is written down somewhere.
pub async fn confirm(state: &SharedState, typed: &str) -> LocalResult<()> {
    let record = stored(&state.db).await?.ok_or(LocalError::NotFound)?;
    if !matches(&record, typed).await {
        return Err(LocalError::BadRequest("Ce n'est pas la clé de récupération actuelle.".into()));
    }
    let confirmed = Stored { confirmed: true, updated_at: next_stamp(Some(&record)), ..record };
    write(&state.db, &confirmed).await?;
    crate::local_server::mesh::catchup::request(state);
    Ok(())
}

/// Failed attempts on this PC, one counter for the whole application run.
fn failures() -> &'static Mutex<(u32, Option<Instant>)> {
    static FAILURES: OnceLock<Mutex<(u32, Option<Instant>)>> = OnceLock::new();
    FAILURES.get_or_init(Default::default)
}

#[derive(Debug, Deserialize)]
pub struct Recover {
    pub recovery_key: String,
    pub username: String,
    pub password: String,
    pub confirm: String,
}

/// Get back in with the recovery key: the account named gets the new password, is active and
/// is an administrator, and the key is replaced. Returns the session, with the new key in
/// `recovery_key`.
pub async fn recover(state: &SharedState, input: Recover) -> LocalResult<Value> {
    if !local_accounts::is_standalone(&state.db).await {
        return Err(LocalError::BadRequest("Ce poste ne fait pas partie d'un réseau local.".into()));
    }
    local_accounts::check_new_password(&input.password, &input.confirm)?;

    // Held for the whole check, so that attempts sent together are counted one by one.
    // The counter is shared by every network a test process creates, so tests reset it.
    let mut tries = failures().lock().await;
    if tries.1.is_some_and(|at| at.elapsed() >= LOCK_WINDOW) {
        *tries = (0, None);
    }
    if tries.0 >= LOCK_AFTER_FAILURES {
        return Err(LocalError::TooManyAttempts);
    }
    let record = stored(&state.db).await?.ok_or_else(|| {
        LocalError::BadRequest("Aucune clé de récupération n'a été créée pour ce réseau.".into())
    })?;
    if !matches(&record, &input.recovery_key).await {
        *tries = (tries.0 + 1, Some(Instant::now()));
        tokio::time::sleep(WRONG_KEY_DELAY).await;
        return Err(LocalError::BadRequest("Clé de récupération incorrecte.".into()));
    }
    *tries = (0, None);

    let account_id = local_accounts::restore_administrator(state, &input.username, &input.password).await?;
    let key = issue(state).await?;
    sqlx::query(
        "INSERT INTO audit_log (id, actor_id, action, entity_type, entity_id, payload, created_at)
         VALUES ($1, $2, 'recovery_key_used', 'local_account', $2, '{}', $3)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&account_id)
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await?;

    let mut bundle = local_accounts::login(state, &input.username, &input.password).await?;
    bundle["recovery_key"] = json!(key);
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::local_accounts::{find_account, session_user, setup, AccountPatch, NewAccount};
    use crate::local_server::test_support::shop;

    /// The attempt counter is process-wide: these tests take turns.
    async fn turn() -> tokio::sync::MutexGuard<'static, ()> {
        static TURN: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = TURN.get_or_init(Default::default).lock().await;
        *failures().lock().await = (0, None);
        guard
    }

    fn attempt(key: &str, username: &str) -> Recover {
        Recover {
            recovery_key: key.into(),
            username: username.into(),
            password: "un nouveau mot de passe".into(),
            confirm: "un nouveau mot de passe".into(),
        }
    }

    async fn network() -> (SharedState, String) {
        let state = shop().await;
        let bundle = setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        let key = bundle["recovery_key"].as_str().expect("creating a network gives a key").to_string();
        (state, key)
    }

    #[test]
    fn a_key_is_readable_and_forgiving_to_type() {
        let key = generate();
        assert_eq!(key.len(), 29, "{key}");
        assert_eq!(key.split('-').count(), 5);
        assert!(!key.contains(['I', 'L', 'O', 'U']), "{key}");
        assert_ne!(key, generate());
        assert_eq!(normalise(" ab1oi-l "), "AB1011");
    }

    #[tokio::test]
    async fn creating_a_network_gives_a_key_that_is_only_kept_hashed() {
        let _turn = turn().await;
        let (state, key) = network().await;
        let record = stored(&state.db).await.unwrap().unwrap();
        assert!(record.key_hash.starts_with("$argon2"));
        assert!(!record.confirmed);
        let (session,): (String,) = sqlx::query_as("SELECT user_json || tokens_json FROM cached_session")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert!(!session.contains(&key));

        assert!(confirm(&state, "pas la clé").await.is_err());
        confirm(&state, &key.to_lowercase().replace('-', " ")).await.unwrap();
        assert!(stored(&state.db).await.unwrap().unwrap().confirmed);
    }

    #[tokio::test]
    async fn the_key_gets_the_only_administrator_back_in_and_is_then_replaced() {
        let _turn = turn().await;
        let (state, key) = network().await;
        sqlx::query("DELETE FROM cached_session").execute(&state.db).await.unwrap();

        let bundle = recover(&state, attempt(&key, "Camille")).await.unwrap();
        let next = bundle["recovery_key"].as_str().unwrap().to_string();
        assert_ne!(next, key);
        assert!(session_user(&state.db).await.unwrap().is_some(), "signed in");
        assert!(local_accounts::login(&state, "camille", "mot de passe").await.is_err());
        assert!(local_accounts::login(&state, "camille", "un nouveau mot de passe").await.is_ok());

        assert!(recover(&state, attempt(&key, "camille")).await.is_err(), "used keys are dead");
        assert!(recover(&state, attempt(&next, "camille")).await.is_ok());
    }

    #[tokio::test]
    async fn the_key_makes_an_employee_or_a_disabled_account_an_administrator() {
        let _turn = turn().await;
        let (state, key) = network().await;
        let admin = find_account(&state.db, "camille").await.unwrap().unwrap().id;
        let lea = local_accounts::create_account(
            &state,
            NewAccount {
                username: "lea".into(),
                password: "mot de passe lea".into(),
                confirm: "mot de passe lea".into(),
                role: "employee".into(),
                full_name: None,
            },
        )
        .await
        .unwrap();
        let off = AccountPatch { is_active: Some(false), ..Default::default() };
        local_accounts::update_account(&state, &admin, &lea.id, off).await.unwrap();

        let bundle = recover(&state, attempt(&key, "lea")).await.unwrap();
        assert_eq!(bundle["user"]["role"], "admin");
        assert!(find_account(&state.db, "lea").await.unwrap().is_some(), "active again");
    }

    #[tokio::test]
    async fn an_unknown_account_does_not_use_up_the_key() {
        let _turn = turn().await;
        let (state, key) = network().await;
        assert!(recover(&state, attempt(&key, "personne")).await.is_err());
        assert!(recover(&state, attempt(&key, "camille")).await.is_ok());
    }

    #[tokio::test]
    async fn wrong_keys_lock_recovery_for_a_while() {
        let _turn = turn().await;
        let (state, key) = network().await;
        for _ in 0..LOCK_AFTER_FAILURES {
            assert!(recover(&state, attempt("AAAAA-AAAAA-AAAAA-AAAAA-AAAAA", "camille")).await.is_err());
        }
        let locked = recover(&state, attempt(&key, "camille")).await;
        assert!(matches!(locked, Err(LocalError::TooManyAttempts)), "{locked:?}");
        *failures().lock().await = (0, None);
    }

    #[tokio::test]
    async fn the_newest_record_wins_between_pcs() {
        let _turn = turn().await;
        let (a, _) = network().await;
        let b = shop().await;
        let first = stored(&a.db).await.unwrap().unwrap();
        assert!(merge(&b.db, Some(first.clone())).await.unwrap());
        assert!(!merge(&b.db, Some(first.clone())).await.unwrap(), "nothing new");

        issue(&a).await.unwrap();
        let second = stored(&a.db).await.unwrap().unwrap();
        assert!(merge(&b.db, Some(second.clone())).await.unwrap());
        assert!(!merge(&b.db, Some(first)).await.unwrap(), "an old key does not come back");
        assert_eq!(stored(&b.db).await.unwrap().unwrap(), second);
    }
}
