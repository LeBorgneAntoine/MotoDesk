//! What a failed call looks like to a screen.

use serde::Deserialize;

/// A refusal from the local API or the cloud, or a call that never arrived (`status == 0`).
///
/// `message` is written for the user (the server answers in French) and can be shown as is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    /// The HTTP status, or 0 when there was no answer.
    pub status: u16,
    /// A stable word to switch on (`not_found`, `conflict`, `company_mismatch`, ...).
    pub code: String,
    pub message: String,
    pub details: Vec<String>,
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Deserialize)]
struct Body {
    error: BodyError,
}

#[derive(Deserialize)]
struct BodyError {
    code: String,
    message: String,
    #[serde(default)]
    details: Vec<String>,
}

impl ApiError {
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { status, code: code.into(), message: message.into(), details: Vec::new() }
    }

    /// The call did not reach its server.
    pub fn network(message: impl Into<String>) -> Self {
        Self::new(0, "network_error", message)
    }

    /// Read `{"error": {"code", "message", "details"}}`; anything else becomes a bare status.
    pub fn from_response(status: u16, body: &[u8]) -> Self {
        match serde_json::from_slice::<Body>(body) {
            Ok(Body { error }) => {
                Self { status, code: error.code, message: error.message, details: error.details }
            }
            Err(_) => Self::new(status, "http_error", bare_status_message(status)),
        }
    }

    /// The server said who is asking is not allowed (as opposed to not answering at all).
    pub fn is_unauthorized(&self) -> bool {
        self.status == 401
    }

    pub fn is_not_found(&self) -> bool {
        self.status == 404
    }

    pub fn is_network(&self) -> bool {
        self.status == 0
    }
}

/// What to say when an answer carries no message of its own (a proxy's page, a request the
/// server could not even read). The number stays: it is what support asks for.
fn bare_status_message(status: u16) -> String {
    let what = match status {
        401 => "Votre session a expiré : reconnectez-vous.",
        403 => "Votre compte n'a pas les droits pour cette action.",
        404 => "Élément introuvable.",
        408 | 504 => "Le serveur a mis trop de temps à répondre. Réessayez.",
        413 => "Ce qui est envoyé est trop volumineux.",
        429 => "Trop d'essais : réessayez dans une minute.",
        500..=599 => "Le serveur a rencontré un problème. Réessayez dans un moment.",
        _ => "La demande n'a pas pu être traitée. Vérifiez que tous les postes sont à jour.",
    };
    format!("{what} (erreur {status})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_servers_own_message_is_kept() {
        let e = ApiError::from_response(
            409,
            r#"{"error":{"code":"conflict","message":"Un client avec cette référence existe déjà."}}"#.as_bytes(),
        );
        assert_eq!((e.status, e.code.as_str()), (409, "conflict"));
        assert_eq!(e.to_string(), "Un client avec cette référence existe déjà.");
    }

    #[test]
    fn a_body_that_is_not_an_error_still_gives_the_status() {
        let e = ApiError::from_response(502, b"<html>Bad gateway</html>");
        assert_eq!((e.status, e.code.as_str()), (502, "http_error"));
        assert_eq!(e.message, "Le serveur a rencontré un problème. Réessayez dans un moment. (erreur 502)");
        assert!(ApiError::from_response(413, b"").message.contains("trop volumineux"));
        assert!(ApiError::from_response(422, b"missing field `x`").message.contains("erreur 422"));
    }
}
