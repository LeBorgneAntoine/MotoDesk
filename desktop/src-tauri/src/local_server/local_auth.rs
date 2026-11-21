//! Per-request authentication for the loopback business API.

use axum::{
    extract::State,
    http::{header, HeaderName, Request},
    middleware::Next,
    response::Response,
};

use crate::local_server::error::LocalError;
use crate::local_server::mesh_auth::constant_time_eq;
use crate::local_server::state::SharedState;

pub const LOCAL_TOKEN_HEADER: HeaderName = HeaderName::from_static("x-motodesk-local-token");

pub fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub async fn require_local_token(
    State(state): State<SharedState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, LocalError> {
    let provided = req
        .headers()
        .get(&LOCAL_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !constant_time_eq(provided, &state.local_token) {
        return Err(LocalError::Unauthorized);
    }
    Ok(next.run(req).await)
}

pub fn cors_layer() -> tower_http::cors::CorsLayer {
    use axum::http::{HeaderValue, Method};
    use tower_http::cors::CorsLayer;

    let origins = [
        "http://localhost:1420",
        "http://127.0.0.1:1420",
        "http://tauri.localhost",
        "tauri://localhost",
        "https://tauri.localhost",
    ]
    .into_iter()
    .filter_map(|o| HeaderValue::from_str(o).ok())
    .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            LOCAL_TOKEN_HEADER,
        ])
}
