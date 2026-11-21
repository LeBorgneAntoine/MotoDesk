//! Joining the shop's network from a PC that has never been part of it.
//!
//! A new PC knows a username and a password, and nothing else: no company, no mesh secret, no
//! accounts. It finds a PC of the network on the LAN and proves it knows the password **without
//! sending it** (SRP-6a, with an Argon2id-derived secret as the "password"). Both sides end up
//! with the same session key; the PC that is joined seals the network's secrets with it:
//!
//! ```text
//! new PC                                      PC of the network
//!   | -- username, A -------------------------> |   /mesh/join/start
//!   | <------------------- kdf salt, B --------- |
//!   | -- M1 (proof of the password) -----------> |   /mesh/join/finish
//!   | <---- M2 (proof of the server), sealed --- |   sealed = AES-256-GCM(session key,
//!   |                                            |     company id, mesh secret, accounts)
//! ```
//!
//! What this protects against: someone listening on the network learns nothing usable (no
//! password, no secret); a PC that advertises itself but does not hold the account cannot
//! produce M2, so the new PC notices; a wrong guess costs an Argon2id derivation and a pause,
//! and five wrong guesses lock the username for a minute. What it does not: a weak password is
//! still guessable by someone who can reach the PC, at that rate. The `srp` crate has not had an
//! independent audit (its own README says so).
//!
//! A PC with no network yet **cannot** join when no PC of the network is on: there is nobody to
//! prove the password to. That is the design (see `docs/standalone.md`).

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use srp::groups::G_2048;
use srp::server::{SrpServer, SrpServerVerifier};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::local_accounts::{self, Account, Joined};
use crate::local_server::mesh::PeerInfo;
use crate::local_server::state::SharedState;

const SESSION_TTL: Duration = Duration::from_secs(30);
const MAX_SESSIONS: usize = 64;
const LOCK_AFTER_FAILURES: u32 = 5;
const LOCK_WINDOW: Duration = Duration::from_secs(60);
const WRONG_PROOF_DELAY: Duration = Duration::from_millis(400);
const PER_PEER_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_CANDIDATES: usize = 8;

// ---- Wire format ----------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinStart {
    pub username: String,
    /// SRP's A, hex.
    pub a_pub: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinStartReply {
    pub session: String,
    /// Salt of the Argon2id derivation and of SRP, hex.
    pub salt: String,
    /// SRP's B, hex.
    pub b_pub: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinFinish {
    pub session: String,
    /// The client's proof, hex.
    pub m1: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinFinishReply {
    /// The server's proof, hex: the new PC checks it before trusting anything below.
    pub m2: String,
    /// `nonce || ciphertext`, hex.
    pub sealed: String,
}

/// What the PC that is joined hands over, sealed.
#[derive(Debug, Serialize, Deserialize)]
struct JoinPackage {
    company_id: Uuid,
    mesh_secret: String,
    company_name: Option<String>,
    legal_name: Option<String>,
    accounts: Vec<Account>,
}

fn unhex(s: &str) -> LocalResult<Vec<u8>> {
    hex::decode(s).map_err(|_| LocalError::BadRequest("échange invalide".into()))
}

fn seal_key(session_key: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"motodesk-join-v1");
    h.update(session_key);
    h.finalize().into()
}

fn seal(session_key: &[u8], plain: &[u8]) -> LocalResult<String> {
    let cipher = Aes256Gcm::new_from_slice(&seal_key(session_key)).map_err(|e| LocalError::Internal(anyhow::anyhow!("{e}")))?;
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|e| LocalError::Internal(anyhow::anyhow!("seal: {e}")))?;
    Ok(hex::encode([nonce.as_slice(), &ct].concat()))
}

fn open(session_key: &[u8], sealed: &str) -> Option<Vec<u8>> {
    let raw = hex::decode(sealed).ok()?;
    if raw.len() < 13 {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(&seal_key(session_key)).ok()?;
    cipher.decrypt(Nonce::from_slice(&raw[..12]), &raw[12..]).ok()
}

// ---- The PC that is joined ---------------------------------------------------------------------

struct JoinSession {
    verifier: SrpServerVerifier<Sha256>,
    account: Option<Account>,
    username: String,
    created: Instant,
}

/// The in-flight handshakes and the failed attempts of the PC that is being joined.
#[derive(Default)]
pub struct JoinState {
    sessions: Mutex<HashMap<String, JoinSession>>,
    failures: Mutex<HashMap<String, (u32, Instant)>>,
}

impl JoinState {
    async fn check_not_locked(&self, username: &str) -> LocalResult<()> {
        let mut failures = self.failures.lock().await;
        failures.retain(|_, (_, at)| at.elapsed() < LOCK_WINDOW);
        match failures.get(username) {
            Some((n, _)) if *n >= LOCK_AFTER_FAILURES => Err(LocalError::TooManyAttempts),
            _ => Ok(()),
        }
    }

    async fn record_failure(&self, username: &str) {
        let mut failures = self.failures.lock().await;
        let entry = failures.entry(username.to_string()).or_insert((0, Instant::now()));
        entry.0 += 1;
        entry.1 = Instant::now();
    }

    async fn clear_failures(&self, username: &str) {
        self.failures.lock().await.remove(username);
    }
}

/// Only a PC that belongs to a network, and holds its secret, can be joined.
async fn hosting(state: &SharedState) -> bool {
    let secret = state.mesh_secret.read().await.clone();
    let status = local_accounts::status(&state.db).await;
    secret.is_some_and(|s| !s.is_empty()) && status.is_ok_and(|s| s.standalone && s.has_account)
}

/// What to answer for a username that has no account, indistinguishable (until the proof)
/// from one that has: the same shape, stable for a given name, unpredictable to a stranger.
fn decoy(username: &str) -> (Vec<u8>, Vec<u8>) {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    let key = KEY.get_or_init(|| {
        let mut k = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut k);
        k
    });
    let derive = |label: &[u8]| -> Vec<u8> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(label);
        mac.update(username.as_bytes());
        mac.finalize().into_bytes().to_vec()
    };
    let salt = derive(b"salt")[..16].to_vec();
    let secret = derive(b"secret");
    let verifier = srp::client::SrpClient::<Sha256>::new(&G_2048).compute_verifier(b"decoy", &secret, &salt);
    (salt, verifier)
}

/// First step: the new PC names an account and sends A.
pub async fn start(state: &SharedState, req: JoinStart) -> LocalResult<JoinStartReply> {
    if !hosting(state).await {
        return Err(LocalError::NotFound);
    }
    let username = req.username.trim().to_lowercase();
    if username.is_empty() || username.len() > 64 {
        return Err(LocalError::BadRequest("échange invalide".into()));
    }
    state.join.check_not_locked(&username).await?;
    let a_pub = unhex(&req.a_pub)?;

    let account = local_accounts::find_account(&state.db, &username).await?;
    let (salt, verifier) = match &account {
        Some(a) => (a.kdf_salt.clone(), a.srp_verifier.clone()),
        None => decoy(&username),
    };

    let mut b = [0u8; 64];
    rand::rngs::OsRng.fill_bytes(&mut b);
    let server = SrpServer::<Sha256>::new(&G_2048);
    let b_pub = server.compute_public_ephemeral(&b, &verifier);
    let verifier_state = server
        .process_reply(&b, &verifier, &a_pub)
        .map_err(|_| LocalError::BadRequest("échange invalide".into()))?;

    let session = Uuid::new_v4().to_string();
    let mut sessions = state.join.sessions.lock().await;
    sessions.retain(|_, s| s.created.elapsed() < SESSION_TTL);
    if sessions.len() >= MAX_SESSIONS {
        return Err(LocalError::TooManyAttempts);
    }
    sessions.insert(
        session.clone(),
        JoinSession { verifier: verifier_state, account, username, created: Instant::now() },
    );
    Ok(JoinStartReply { session, salt: hex::encode(salt), b_pub: hex::encode(b_pub) })
}

/// Second step: the new PC proves it knows the password; if so it receives the network's secrets.
pub async fn finish(state: &SharedState, req: JoinFinish) -> LocalResult<JoinFinishReply> {
    let session = state
        .join
        .sessions
        .lock()
        .await
        .remove(&req.session)
        .filter(|s| s.created.elapsed() < SESSION_TTL)
        .ok_or(LocalError::Unauthorized)?;
    let m1 = unhex(&req.m1)?;

    let proven = session.verifier.verify_client(&m1).is_ok();
    let Some(account) = session.account.filter(|_| proven) else {
        state.join.record_failure(&session.username).await;
        tokio::time::sleep(WRONG_PROOF_DELAY).await;
        return Err(LocalError::InvalidCredentials);
    };
    state.join.clear_failures(&session.username).await;

    let company_id = state
        .company_id
        .read()
        .await
        .ok_or_else(|| LocalError::Internal(anyhow::anyhow!("a network PC without a company id")))?;
    let mesh_secret = state
        .mesh_secret
        .read()
        .await
        .clone()
        .ok_or_else(|| LocalError::Internal(anyhow::anyhow!("a network PC without a mesh secret")))?;
    let identity: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT name, legal_name FROM company_cache WHERE id = 1")
            .fetch_optional(&state.db)
            .await?;
    let (company_name, legal_name) = identity.unwrap_or((None, None));

    let package = JoinPackage {
        company_id,
        mesh_secret,
        company_name,
        legal_name,
        accounts: local_accounts::all_accounts(&state.db).await?,
    };
    tracing::info!(username = %account.username, "a PC joined the network");
    let plain = serde_json::to_vec(&package).map_err(|e| LocalError::Internal(e.into()))?;
    Ok(JoinFinishReply {
        m2: hex::encode(session.verifier.proof()),
        sealed: seal(session.verifier.key(), &plain)?,
    })
}

// ---- The new PC ------------------------------------------------------------------------------------

/// Why one PC could not be joined.
#[derive(Debug)]
enum Attempt {
    /// Did not answer.
    Unreachable,
    /// Answered, but is not a PC of a network (or not one that can be joined).
    NotAHost,
    /// Is a PC of a network and did not accept the proof.
    BadCredentials,
    /// Sent something that does not hold together.
    Protocol,
    Locked,
}

fn base_url(peer: &PeerInfo) -> String {
    let host = if peer.host.contains(':') && !peer.host.starts_with('[') {
        format!("[{}]", peer.host)
    } else {
        peer.host.clone()
    };
    format!("http://{host}:{}", peer.port)
}

async fn join_via(
    state: &SharedState,
    peer: &PeerInfo,
    username: &str,
    password: &str,
) -> Result<JoinPackage, Attempt> {
    let base = base_url(peer);

    let mut a = [0u8; 64];
    rand::rngs::OsRng.fill_bytes(&mut a);
    let client = srp::client::SrpClient::<Sha256>::new(&G_2048);
    let a_pub = client.compute_public_ephemeral(&a);

    let res = state
        .http
        .post(format!("{base}/mesh/join/start"))
        .timeout(PER_PEER_TIMEOUT)
        .json(&JoinStart { username: username.to_string(), a_pub: hex::encode(&a_pub) })
        .send()
        .await
        .map_err(|_| Attempt::Unreachable)?;
    match res.status().as_u16() {
        200 => {}
        404 => return Err(Attempt::NotAHost),
        429 => return Err(Attempt::Locked),
        _ => return Err(Attempt::Protocol),
    }
    let reply: JoinStartReply = res.json().await.map_err(|_| Attempt::Protocol)?;
    let salt = hex::decode(&reply.salt).map_err(|_| Attempt::Protocol)?;
    let b_pub = hex::decode(&reply.b_pub).map_err(|_| Attempt::Protocol)?;
    if !(8..=64).contains(&salt.len()) {
        return Err(Attempt::Protocol);
    }

    // The slow part: stretching the password. Off the async threads.
    let (pw, s) = (password.to_string(), salt.clone());
    let secret = tokio::task::spawn_blocking(move || local_accounts::derive_secret(&pw, &s))
        .await
        .map_err(|_| Attempt::Protocol)?
        .map_err(|_| Attempt::Protocol)?;
    let proven = client
        .process_reply(&a, &local_accounts::srp_identity(username), &secret, &salt, &b_pub)
        .map_err(|_| Attempt::Protocol)?;

    let res = state
        .http
        .post(format!("{base}/mesh/join/finish"))
        .timeout(PER_PEER_TIMEOUT)
        .json(&JoinFinish { session: reply.session, m1: hex::encode(proven.proof()) })
        .send()
        .await
        .map_err(|_| Attempt::Unreachable)?;
    match res.status().as_u16() {
        200 => {}
        401 => return Err(Attempt::BadCredentials),
        429 => return Err(Attempt::Locked),
        _ => return Err(Attempt::Protocol),
    }
    let reply: JoinFinishReply = res.json().await.map_err(|_| Attempt::Protocol)?;

    // The PC must prove it holds the account too: a stranger advertising itself cannot.
    let m2 = hex::decode(&reply.m2).map_err(|_| Attempt::Protocol)?;
    proven.verify_server(&m2).map_err(|_| Attempt::Protocol)?;
    let plain = open(proven.key(), &reply.sealed).ok_or(Attempt::Protocol)?;
    serde_json::from_slice(&plain).map_err(|_| Attempt::Protocol)
}

/// Join the network of one of `candidates` with an account's username and password, and sign in.
/// Tries each PC until one lets this PC in.
pub async fn join_network(
    state: &SharedState,
    username: &str,
    password: &str,
    candidates: &[PeerInfo],
) -> LocalResult<serde_json::Value> {
    let mut wrong_password = false;
    let mut locked = false;
    for peer in candidates.iter().take(MAX_CANDIDATES) {
        match join_via(state, peer, username, password).await {
            Ok(package) => {
                let joined = Joined {
                    company_id: package.company_id,
                    mesh_secret: package.mesh_secret,
                    company_name: package.company_name,
                    legal_name: package.legal_name,
                    accounts: package.accounts,
                };
                let bundle = local_accounts::adopt_network(state, username, joined).await?;
                // The PC that let us in is a peer from now on, and has the data to give.
                crate::local_server::mesh::adopt_peer(state, peer.clone()).await;
                crate::local_server::mesh::catchup::request(state);
                return Ok(bundle);
            }
            Err(Attempt::BadCredentials) => wrong_password = true,
            Err(Attempt::Locked) => locked = true,
            Err(other) => tracing::debug!(peer = %peer.host, why = ?other, "join attempt did not work"),
        }
    }
    if wrong_password {
        return Err(LocalError::InvalidCredentials);
    }
    if locked {
        return Err(LocalError::TooManyAttempts);
    }
    Err(LocalError::NoNetwork)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::shop;

    #[test]
    fn what_is_sealed_opens_only_with_the_same_session_key() {
        let sealed = seal(b"session-key", b"the network's secrets").unwrap();
        assert_eq!(open(b"session-key", &sealed).as_deref(), Some(b"the network's secrets".as_slice()));
        assert_eq!(open(b"other-key", &sealed), None);
        assert_eq!(open(b"session-key", "zz"), None);
        assert_eq!(open(b"session-key", &sealed[..20]), None, "truncated");
    }

    #[tokio::test]
    async fn a_decoy_looks_like_an_account_and_is_stable_for_a_name() {
        let (salt, verifier) = decoy("personne");
        assert_eq!(salt.len(), 16);
        assert!(!verifier.is_empty());
        assert_eq!(decoy("personne"), (salt.clone(), verifier));
        assert_ne!(decoy("quelquun").0, salt);
    }

    #[tokio::test]
    async fn only_a_pc_with_a_network_can_be_joined() {
        let state = shop().await;
        let refused = start(&state, JoinStart { username: "camille".into(), a_pub: "02".into() }).await;
        assert!(matches!(refused, Err(LocalError::NotFound)), "{refused:?}");
    }
}
