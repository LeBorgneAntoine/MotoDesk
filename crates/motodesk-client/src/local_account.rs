//! Standalone mode: an installation with no cloud behind it
//! (port of `desktop/src/lib/api/localAccount.ts` and `desktop/src/lib/stores/accessMode.ts`).
//!
//! Its first administrator is created on this PC (a username and a password), and an
//! administrator can later put the installation online. The rules mirror
//! `local_server/local_accounts.rs`, which enforces them.

use axum::http::Method;
use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client;
use crate::types::TokenBundle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct LocalStatus {
    /// This installation is run without a cloud.
    pub standalone: bool,
    /// A local administrator exists.
    pub has_account: bool,
    /// Nothing is set up on this PC yet: the first administrator can be created.
    pub can_setup: bool,
}

pub const MIN_PASSWORD_CHARS: usize = 8;
pub const MAX_PASSWORD_CHARS: usize = 128;

/// The first thing wrong with a new local account, or `None` when it is fine.
pub fn validate_new_local_account(username: &str, password: &str, confirm: &str) -> Option<String> {
    let name = username.trim();
    let length = name.chars().count();
    if !(3..=32).contains(&length) {
        return Some("L'identifiant doit faire de 3 à 32 caractères.".into());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return Some("L'identifiant ne peut contenir que des lettres, des chiffres, « . », « _ » et « - ».".into());
    }
    let chars = password.chars().count();
    if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&chars) {
        return Some(format!(
            "Le mot de passe doit faire de {MIN_PASSWORD_CHARS} à {MAX_PASSWORD_CHARS} caractères."
        ));
    }
    if password != confirm {
        return Some("Les deux mots de passe ne correspondent pas.".into());
    }
    None
}

pub async fn status(client: &Client) -> ApiResult<LocalStatus> {
    client.call(Method::GET, "/api/auth/local/status").public().send().await
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupPayload {
    pub username: String,
    pub password: String,
    pub confirm: String,
    /// Create a new network although another one is visible on the LAN.
    pub force: bool,
}

/// A session and, when the server made one, the network's recovery key to write down.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionWithKey {
    #[serde(default)]
    pub recovery_key: Option<String>,
    #[serde(flatten)]
    pub bundle: TokenBundle,
}

pub async fn setup(client: &Client, payload: &SetupPayload) -> ApiResult<SessionWithKey> {
    client.call(Method::POST, "/api/auth/local/setup").public().json(payload).send().await
}

pub async fn login(client: &Client, username: &str, password: &str) -> ApiResult<TokenBundle> {
    client
        .call(Method::POST, "/api/auth/local/login")
        .public()
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
}

#[derive(Debug, Clone, Serialize)]
pub struct PromotePayload {
    pub email: String,
    pub password: String,
    pub confirm: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_name: Option<String>,
}

/// Register this installation's company on the cloud; returns the cloud session.
pub async fn promote_to_online(client: &Client, payload: &PromotePayload) -> ApiResult<TokenBundle> {
    client.call(Method::POST, "/api/auth/promote").public().json(payload).send().await
}

// ---- which way into the app is on offer -----------------------------------------------------

/// The sign-in screen has two ways in: an online account (the cloud) or a local one (this PC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessMode {
    #[default]
    Online,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalState {
    /// The local server has not answered yet.
    Unknown,
    /// Nothing is set up on this PC; the first local administrator can be created.
    Setup,
    /// A local administrator exists and the installation runs without a cloud.
    SignIn,
    /// It began as local and has been put online since.
    Online,
    /// This PC already belongs to an online company.
    Tied,
}

pub fn local_state(status: Option<LocalStatus>) -> LocalState {
    match status {
        None => LocalState::Unknown,
        Some(s) if s.standalone && s.has_account => LocalState::SignIn,
        Some(s) if s.can_setup => LocalState::Setup,
        Some(s) if s.has_account => LocalState::Online,
        Some(_) => LocalState::Tied,
    }
}

/// The local side of the switch is usable only while it can create or sign in an account.
pub fn local_offered(status: Option<LocalStatus>) -> bool {
    matches!(local_state(status), LocalState::Unknown | LocalState::Setup | LocalState::SignIn)
}

/// The side to start on: a PC that runs without a cloud opens on its own sign-in.
pub fn initial_access_mode(status: Option<LocalStatus>) -> AccessMode {
    if local_state(status) == LocalState::SignIn {
        AccessMode::Local
    } else {
        AccessMode::Online
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PW: &str = "mot de passe";

    #[test]
    fn accepts_a_plain_account() {
        assert_eq!(validate_new_local_account("camille", PW, PW), None);
        assert_eq!(validate_new_local_account("  Léa_2 ", PW, PW), None);
    }

    #[test]
    fn checks_the_username_first() {
        let long = "x".repeat(33);
        assert!(validate_new_local_account("ab", PW, PW).unwrap().contains("identifiant"));
        assert!(validate_new_local_account(&long, PW, PW).unwrap().contains("identifiant"));
        assert!(validate_new_local_account("camille martin", PW, PW).unwrap().contains("lettres"));
        assert!(validate_new_local_account("camille@example.fr", PW, PW).unwrap().contains("lettres"));
    }

    #[test]
    fn checks_the_password_length_and_the_confirmation() {
        let long = "x".repeat(129);
        assert!(validate_new_local_account("camille", "court", "court").unwrap().contains("8 à 128"));
        assert!(validate_new_local_account("camille", &long, &long).unwrap().contains("8 à 128"));
        assert!(validate_new_local_account("camille", PW, "autre chose").unwrap().contains("correspondent"));
    }

    fn status(standalone: bool, has_account: bool, can_setup: bool) -> Option<LocalStatus> {
        Some(LocalStatus { standalone, has_account, can_setup })
    }

    #[test]
    fn tells_the_situations_of_a_pc_apart() {
        assert_eq!(local_state(None), LocalState::Unknown);
        assert_eq!(local_state(status(false, false, true)), LocalState::Setup); // a fresh PC
        assert_eq!(local_state(status(true, true, false)), LocalState::SignIn); // runs without a cloud
        assert_eq!(local_state(status(false, true, false)), LocalState::Online); // was local, now online
        assert_eq!(local_state(status(false, false, false)), LocalState::Tied); // belongs to an online company
    }

    #[test]
    fn offers_the_local_side_only_where_it_can_create_or_sign_in_an_account() {
        assert!(local_offered(None)); // do not block while the server starts
        assert!(local_offered(status(false, false, true)));
        assert!(local_offered(status(true, true, false)));
        assert!(!local_offered(status(false, true, false)));
        assert!(!local_offered(status(false, false, false)));
    }

    #[test]
    fn starts_on_the_local_side_only_for_an_installation_that_runs_without_a_cloud() {
        assert_eq!(initial_access_mode(status(true, true, false)), AccessMode::Local);
        assert_eq!(initial_access_mode(status(false, false, true)), AccessMode::Online);
        assert_eq!(initial_access_mode(status(false, true, false)), AccessMode::Online);
        assert_eq!(initial_access_mode(None), AccessMode::Online);
    }
}
