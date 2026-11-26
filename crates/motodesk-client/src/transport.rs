//! The local API, called in the same process.

use axum::body::Body;
use axum::http::{header, HeaderValue, Method, Request};
use axum::Router;
use tower::ServiceExt;

use crate::error::{ApiError, ApiResult};

/// Must match `local_server::local_auth::LOCAL_TOKEN_HEADER`.
const LOCAL_TOKEN_HEADER: &str = "x-motodesk-local-token";

pub(crate) const JSON: &str = "application/json";

/// The largest answer read into memory. The server caps what it accepts at 16 MB; its answers
/// (a page of a list, a document) are far smaller.
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

/// The loopback API of the embedded server, as a value: a request is routed straight to its
/// handler, through the same middleware (local token, body limit) as a request on the socket.
#[derive(Clone)]
pub struct Local {
    router: Router,
    token: HeaderValue,
}

impl Local {
    /// `router` is `local_server::Started::api`; `local_token` the one the server was started with.
    pub fn new(router: Router, local_token: &str) -> Self {
        let token = HeaderValue::from_str(local_token).unwrap_or_else(|_| HeaderValue::from_static(""));
        Self { router, token }
    }

    /// Send one request with a JSON body, or none; the answer's bytes on success, the
    /// server's error otherwise.
    pub async fn send(&self, method: Method, path: &str, body: Option<Vec<u8>>) -> ApiResult<Vec<u8>> {
        self.send_as(method, path, body.map(|bytes| (JSON.to_string(), bytes))).await
    }

    /// Send one request whose body, when there is one, comes with its own content type (an
    /// image, for a logo).
    pub async fn send_as(&self, method: Method, path: &str, body: Option<(String, Vec<u8>)>) -> ApiResult<Vec<u8>> {
        let mut request = Request::builder().method(method).uri(path).header(LOCAL_TOKEN_HEADER, &self.token);
        let body = match body {
            Some((content_type, bytes)) => {
                request = request.header(header::CONTENT_TYPE, content_type);
                Body::from(bytes)
            }
            None => Body::empty(),
        };
        let request = request
            .body(body)
            .map_err(|e| ApiError::new(0, "bad_request", format!("Requête invalide ({path}) : {e}")))?;

        // A `Router` never fails as a service: an unknown path is a 404 answer.
        let response = match self.router.clone().oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), MAX_RESPONSE_BYTES)
            .await
            .map_err(|e| ApiError::network(format!("Réponse illisible du service de données : {e}")))?;
        if status.is_success() {
            Ok(bytes.to_vec())
        } else {
            Err(ApiError::from_response(status.as_u16(), &bytes))
        }
    }
}
