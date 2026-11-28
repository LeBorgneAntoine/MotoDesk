use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// What a request is refused with. Every message here is read at the counter: in French, saying
/// what happened and, when there is something to do, what.
#[derive(Debug, Error)]
pub enum LocalError {
    #[error("Identifiant ou mot de passe incorrect.")]
    InvalidCredentials,

    #[error("Votre session a expiré : reconnectez-vous.")]
    Unauthorized,

    #[error("Votre compte n'a pas les droits pour cette action. Demandez à un administrateur.")]
    Forbidden,

    #[error("Élément introuvable : il a peut-être été supprimé, ici ou sur un autre poste.")]
    NotFound,

    #[error("{0}")]
    BadRequest(String),

    /// The request is fine but not possible in the state the installation is in.
    #[error("{0}")]
    Conflict(String),

    /// A network of the shop is visible on the LAN: creating another one is probably a mistake.
    #[error("{0}")]
    NetworkExists(String),

    /// A new PC must reach a PC of the network to join it, and none answered.
    #[error("Aucun poste du réseau local n'est joignable. Allumez un poste déjà connecté, sur le même réseau, puis réessayez : un nouveau poste doit d'abord rejoindre le réseau avec lui.")]
    NoNetwork,

    /// The password was right, but only an administrator may add a PC to the network.
    #[error("Seul un administrateur peut ajouter un poste au réseau : saisissez ici un compte administrateur. Les employés se connecteront ensuite.")]
    AdminRequiredToJoin,

    #[error("Trop d'essais : réessayez dans une minute.")]
    TooManyAttempts,

    /// The account signing in belongs to another company than the data on this PC.
    #[error("{0}")]
    WrongCompany(String),

    #[error("Le serveur cloud n'est pas joignable. Vérifiez votre connexion internet et l'adresse du serveur.")]
    CloudRequired,

    /// Something failed that the request is not to blame for. This text is for the log; the person
    /// is told [`internal_message`].
    #[error("internal error: {0:#}")]
    Internal(#[from] anyhow::Error),
}

/// What to say about a failure that is not a refusal: the common causes are named, because there
/// is something to do about them; anything else points to the logs, which hold the detail.
fn internal_message(error: &anyhow::Error) -> &'static str {
    const BUSY: &str = "La base de données est occupée (un échange avec un autre poste est en cours). Réessayez dans un instant.";
    const FULL: &str = "Le disque est plein : libérez de la place, puis réessayez.";
    const DENIED: &str = "Windows refuse l'accès à un fichier (droits, antivirus, ou fichier ouvert dans un autre programme). Fermez ce qui pourrait l'utiliser, puis réessayez.";
    for cause in error.chain() {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            // 32 and 33: Windows says another program holds the file, or part of it.
            if cfg!(windows) && matches!(io.raw_os_error(), Some(32 | 33)) {
                return DENIED;
            }
            match io.kind() {
                std::io::ErrorKind::StorageFull => return FULL,
                std::io::ErrorKind::PermissionDenied => return DENIED,
                std::io::ErrorKind::NotFound => return "Un fichier ou un dossier attendu est introuvable (disque débranché ?).",
                _ => {}
            }
        }
        let text = cause.to_string().to_lowercase();
        if text.contains("database is locked") || text.contains("table is locked") || text.contains("pool timed out") {
            return BUSY;
        }
        if text.contains("disk is full") {
            return FULL;
        }
        if text.contains("disk i/o error") || text.contains("disk image is malformed") {
            return "La base de données signale un problème de disque. N'écrivez plus rien : faites une sauvegarde et demandez de l'aide.";
        }
    }
    "Une erreur inattendue s'est produite. Réessayez ; si elle revient, joignez les journaux (Paramètres › Sauvegardes › Diagnostic) à votre demande d'aide."
}

impl LocalError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            LocalError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            LocalError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            LocalError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            LocalError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            LocalError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            LocalError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            LocalError::NetworkExists(_) => (StatusCode::CONFLICT, "network_exists"),
            LocalError::NoNetwork => (StatusCode::SERVICE_UNAVAILABLE, "no_network"),
            LocalError::AdminRequiredToJoin => (StatusCode::FORBIDDEN, "admin_required"),
            LocalError::TooManyAttempts => (StatusCode::TOO_MANY_REQUESTS, "too_many_attempts"),
            LocalError::WrongCompany(_) => (StatusCode::CONFLICT, "company_mismatch"),
            LocalError::CloudRequired => (StatusCode::SERVICE_UNAVAILABLE, "cloud_required"),
            LocalError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        }
    }
}

impl IntoResponse for LocalError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let message = match &self {
            LocalError::Internal(error) => {
                tracing::error!(error = %format!("{error:#}"), "a request failed unexpectedly");
                internal_message(error).to_string()
            }
            other => other.to_string(),
        };
        let body = json!({ "error": { "code": code, "message": message } });
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for LocalError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => LocalError::NotFound,
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                LocalError::BadRequest(unique_violation_message(db.message()))
            }
            other => LocalError::Internal(anyhow::anyhow!(other)),
        }
    }
}

/// A duplicate key is something the user can fix, so it is said in plain words
/// instead of surfacing as a raw "UNIQUE constraint failed" server error.
fn unique_violation_message(raw: &str) -> String {
    let column = raw.rsplit("failed: ").next().unwrap_or(raw);
    let text = if column.contains("products.sku") {
        "Cette référence (SKU) est déjà utilisée par un autre produit."
    } else if column.contains("vin_or_serial") {
        "Ce numéro de série / VIN est déjà utilisé."
    } else if column.contains("categories.") {
        "Une catégorie porte déjà ce nom."
    } else if column.contains("clients.") {
        "Un client avec cette référence existe déjà."
    } else {
        "Cette valeur existe déjà : elle doit être unique."
    };
    text.to_string()
}

impl From<serde_json::Error> for LocalError {
    fn from(err: serde_json::Error) -> Self {
        LocalError::Internal(err.into())
    }
}

pub type LocalResult<T> = Result<T, LocalError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// What reaches the screen, whatever the kind of refusal.
    async fn said(error: LocalError) -> String {
        let body = axum::body::to_bytes(error.into_response().into_body(), 64 * 1024).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"]["message"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn everyday_refusals_are_said_in_french() {
        assert_eq!(said(LocalError::InvalidCredentials).await, "Identifiant ou mot de passe incorrect.");
        assert!(said(LocalError::Unauthorized).await.contains("reconnectez-vous"));
        assert!(said(LocalError::Forbidden).await.contains("administrateur"));
        assert!(said(LocalError::NotFound).await.starts_with("Élément introuvable"));
        assert_eq!(said(LocalError::BadRequest("aucune session ouverte".into())).await, "aucune session ouverte");
    }

    #[tokio::test]
    async fn an_unexpected_failure_names_its_cause_when_it_can_and_shows_no_internals() {
        use std::io::{Error, ErrorKind};
        let internal = |error: anyhow::Error| LocalError::Internal(error);
        let full = anyhow::Error::new(Error::from(ErrorKind::StorageFull)).context("écriture (D:/x)");
        assert!(said(internal(full)).await.starts_with("Le disque est plein"));
        let denied = anyhow::Error::new(Error::from(ErrorKind::PermissionDenied));
        assert!(said(internal(denied)).await.starts_with("Windows refuse"));
        let busy = anyhow::anyhow!("error returned from database: (code: 5) database is locked");
        assert!(said(internal(busy)).await.contains("occupée"));
        let other = said(internal(anyhow::anyhow!("no such column: secret_key at offset 12"))).await;
        assert!(other.starts_with("Une erreur inattendue") && !other.contains("column"), "{other}");
    }

    #[test]
    fn duplicates_are_reported_in_plain_french() {
        assert!(unique_violation_message("UNIQUE constraint failed: products.sku").contains("référence"));
        assert!(unique_violation_message("UNIQUE constraint failed: vehicles.vin_or_serial").contains("VIN"));
        assert!(unique_violation_message("UNIQUE constraint failed: categories.name").contains("catégorie"));
    }
}
