use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LocalError {
    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("not found")]
    NotFound,

    #[error("bad request: {0}")]
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

    #[error("internal error: {0:#}")]
    Internal(#[from] anyhow::Error),
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
            LocalError::BadRequest(msg) => msg.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_are_reported_in_plain_french() {
        assert!(unique_violation_message("UNIQUE constraint failed: products.sku").contains("référence"));
        assert!(unique_violation_message("UNIQUE constraint failed: vehicles.vin_or_serial").contains("VIN"));
        assert!(unique_violation_message("UNIQUE constraint failed: categories.name").contains("catégorie"));
    }
}

impl From<serde_json::Error> for LocalError {
    fn from(err: serde_json::Error) -> Self {
        LocalError::Internal(err.into())
    }
}

pub type LocalResult<T> = Result<T, LocalError>;
