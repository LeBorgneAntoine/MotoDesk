//! The cloud's account API (port of `desktop/src/lib/api/cloudClient.ts`).
//!
//! Only sign-in goes to the cloud directly: everything else is asked of the local API, which
//! does its own syncing.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{ApiError, ApiResult};
use crate::types::{PublicUser, TokenBundle};

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct Cloud {
    http: reqwest::Client,
    base: String,
}

impl Cloud {
    /// `base` is the cloud server's address, as the local server holds it (`LocalState::cloud_url`).
    pub fn new(base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder().timeout(TIMEOUT).build().unwrap_or_default();
        Self { http, base: base.into().trim_end_matches('/').to_string() }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    async fn send<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
        access_token: Option<&str>,
    ) -> ApiResult<T> {
        let request = match access_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        let response = request.send().await.map_err(|e| {
            ApiError::network(if e.is_timeout() {
                "Délai dépassé en contactant le serveur cloud.".to_string()
            } else {
                format!(
                    "Impossible de joindre le serveur cloud ({}). Vérifiez votre connexion internet et l'adresse du serveur.",
                    self.base
                )
            })
        })?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| ApiError::network(e.to_string()))?;
        if !status.is_success() {
            return Err(ApiError::from_response(status.as_u16(), &bytes));
        }
        crate::session::decode(&bytes)
    }

    pub async fn post<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
        access_token: Option<&str>,
    ) -> ApiResult<T> {
        self.send(self.http.post(format!("{}{path}", self.base)).json(body), access_token).await
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str, access_token: Option<&str>) -> ApiResult<T> {
        self.send(self.http.get(format!("{}{path}", self.base)), access_token).await
    }

    pub async fn patch<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
        access_token: Option<&str>,
    ) -> ApiResult<T> {
        self.send(self.http.patch(format!("{}{path}", self.base)).json(body), access_token).await
    }

    /// Send a file as it is (a logo), under its own content type.
    pub async fn put_bytes<T: DeserializeOwned>(
        &self,
        path: &str,
        bytes: Vec<u8>,
        content_type: &str,
        access_token: Option<&str>,
    ) -> ApiResult<T> {
        let request = self.http.put(format!("{}{path}", self.base)).header(reqwest::header::CONTENT_TYPE, content_type).body(bytes);
        self.send(request, access_token).await
    }

    pub async fn delete<T: DeserializeOwned>(&self, path: &str, access_token: Option<&str>) -> ApiResult<T> {
        self.send(self.http.delete(format!("{}{path}", self.base)), access_token).await
    }

    pub async fn login(&self, identifier: &str, password: &str) -> ApiResult<TokenBundle> {
        self.post("/api/auth/login", &serde_json::json!({ "identifier": identifier, "password": password }), None)
            .await
    }

    pub async fn register(&self, payload: &crate::auth::RegisterPayload) -> ApiResult<TokenBundle> {
        self.post("/api/auth/register", payload, None).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> ApiResult<TokenBundle> {
        self.post("/api/auth/refresh", &serde_json::json!({ "refresh_token": refresh_token }), None).await
    }

    pub async fn me(&self, access_token: &str) -> ApiResult<PublicUser> {
        self.get("/api/auth/me", Some(access_token)).await
    }

    /// Close the cloud session. Best effort: it expires on its own.
    pub async fn logout(&self, refresh_token: &str) {
        let _: ApiResult<serde::de::IgnoredAny> =
            self.post("/api/auth/logout", &serde_json::json!({ "refresh_token": refresh_token }), None).await;
    }
}
