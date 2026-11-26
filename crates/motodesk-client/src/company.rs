//! The identity of the company: its names and its logo.
//!
//! Reads come from the copy this PC keeps, so the name and the logo are there offline. Writes
//! are for administrators: an online company writes to the cloud and refreshes the copy, a
//! local network has no cloud and writes to the copy itself.

use axum::http::Method;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::session::Client;

/// The largest logo accepted, by the cloud and by the local service alike.
pub const MAX_LOGO_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Company {
    pub id: Option<String>,
    pub name: Option<String>,
    pub legal_name: Option<String>,
    pub has_logo: bool,
    /// Changes with every new logo: what tells a cached image it is stale.
    pub logo_updated_at: Option<String>,
}

impl Company {
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or_default()
    }

    pub fn legal_name(&self) -> &str {
        self.legal_name.as_deref().unwrap_or_default()
    }
}

/// A local network has no cloud: its identity is written to the local service.
fn is_standalone(client: &Client) -> bool {
    client.tokens().is_some_and(|session| session.user.is_local())
}

fn token(client: &Client) -> ApiResult<String> {
    client.tokens().map(|session| session.access_token).ok_or_else(|| ApiError::new(401, "unauthorized", "Session expirée."))
}

pub async fn get(client: &Client) -> ApiResult<Company> {
    client.call(Method::GET, "/api/company").public().send().await
}

/// Ask the local service to fetch the identity and the logo from the cloud again. Quiet when
/// the cloud is out of reach: the copy is refreshed at the next sign-in.
async fn refresh_local_copy(client: &Client) {
    let _ = client.call(Method::POST, "/api/company/refresh").public().done().await;
}

/// Change the names. An empty `legal_name` removes it.
pub async fn update(client: &Client, name: &str, legal_name: &str) -> ApiResult<Company> {
    let (name, legal_name) = (name.trim(), legal_name.trim());
    if is_standalone(client) {
        // The local service keeps the legal name it has when sent `null`: an empty text is
        // what clears it.
        let body = serde_json::json!({ "name": name, "legal_name": legal_name });
        return client.call(Method::PATCH, "/api/company").public().json(&body).send().await;
    }
    let body = serde_json::json!({ "name": name, "legal_name": (!legal_name.is_empty()).then_some(legal_name) });
    let updated: Company = client.cloud().patch("/api/company", &body, Some(&token(client)?)).await?;
    refresh_local_copy(client).await;
    Ok(get(client).await.unwrap_or(updated))
}

/// The logo as it was sent, or `None` when the company has none.
pub async fn logo(client: &Client) -> ApiResult<Option<Vec<u8>>> {
    match client.call(Method::GET, "/api/company/logo").public().bytes().await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.is_not_found() => Ok(None),
        Err(error) => Err(error),
    }
}

pub async fn upload_logo(client: &Client, content_type: &str, bytes: Vec<u8>) -> ApiResult<Company> {
    if is_standalone(client) {
        return client.call(Method::PUT, "/api/company/logo").public().raw(content_type, bytes).send().await;
    }
    let updated: Company = client.cloud().put_bytes("/api/company/logo", bytes, content_type, Some(&token(client)?)).await?;
    refresh_local_copy(client).await;
    Ok(get(client).await.unwrap_or(updated))
}

/// The content type of an image, from the name of its file. `None` for anything that is not
/// an image a logo can be.
pub fn image_type(file_name: &str) -> Option<&'static str> {
    let extension = file_name.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase())?;
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "svg" => Some("image/svg+xml"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// Why a file cannot be the logo, in words for the person who chose it.
pub fn logo_problem(file_name: &str, size: usize) -> Option<&'static str> {
    if image_type(file_name).is_none() {
        return Some("Veuillez sélectionner une image (PNG, JPG, SVG, WebP ou GIF).");
    }
    if size == 0 {
        return Some("Ce fichier est vide.");
    }
    (size > MAX_LOGO_BYTES).then_some("Image trop volumineuse (max 1 Mo). Réduisez sa taille puis réessayez.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_is_known_by_its_extension() {
        assert_eq!(image_type("logo.PNG"), Some("image/png"));
        assert_eq!(image_type("mon.logo.jpeg"), Some("image/jpeg"));
        assert_eq!(image_type("logo.svg"), Some("image/svg+xml"));
        assert_eq!(image_type("logo.pdf"), None);
        assert_eq!(image_type("logo"), None);
    }

    #[test]
    fn a_logo_is_an_image_of_one_megabyte_at_most() {
        assert_eq!(logo_problem("logo.png", 20_000), None);
        assert_eq!(logo_problem("logo.png", MAX_LOGO_BYTES), None);
        assert!(logo_problem("logo.png", MAX_LOGO_BYTES + 1).is_some_and(|why| why.contains("1 Mo")));
        assert!(logo_problem("logo.png", 0).is_some());
        assert!(logo_problem("facture.pdf", 20_000).is_some_and(|why| why.contains("image")));
    }
}
