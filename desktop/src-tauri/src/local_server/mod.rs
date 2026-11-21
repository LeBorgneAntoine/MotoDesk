pub mod assistant;
pub mod auth_guard;
pub mod backup;
pub mod backup_format;
pub mod bootstrap;
pub mod cloud;
pub mod company;
pub mod credit;
pub mod db;
pub mod error;
pub mod fiscal;
pub mod local_accounts;
pub mod local_auth;
pub mod mesh;
pub mod recovery;
pub mod mesh_auth;
pub mod pos;
pub mod routes;
pub mod secrets;
pub mod state;
pub mod sync_engine;
pub mod sync_log;
pub mod validate;
#[cfg(test)]
pub mod test_support;

use std::sync::Arc;

use axum::Router;
use tauri::Manager;
use tower_http::limit::RequestBodyLimitLayer;
use tracing::info;

use crate::local_server::state::LocalState;

/// Loopback business API (must match desktop/src/lib/api/localApi.ts).
pub const LOCAL_API_PORT: u16 = 17300;
pub const LOCAL_API_URL: &str = "http://127.0.0.1:17300";
/// LAN mesh listener — only `/mesh/apply` is mounted here.
pub const MESH_API_PORT: u16 = 17301;

pub async fn start(app: &tauri::AppHandle, local_token: String) -> anyhow::Result<()> {
    let data_dir = crate::previous_install::data_dir_in_use(app.path().app_data_dir()?);
    std::fs::create_dir_all(&data_dir)?;
    // A restore that cannot be applied must not stop the app from starting on its
    // current data: the failure is logged and the restore stays queued.
    if let Err(e) = backup::apply_pending_restore(&data_dir) {
        tracing::error!(error = %e, "could not apply the pending restore; starting on the current data");
    }
    let db_path = data_dir.join("local.db");

    let pool = db::init_pool(&db_path).await?;
    db::run_migrations(&pool).await?;
    db::ensure_device_id(&pool).await?;
    let cloud_url = db::ensure_cloud_url(&pool).await?;
    let mesh_secret = secrets::load_mesh_secret()
        .ok()
        .flatten()
        .or(db::load_mesh_secret(&pool).await?);
    if let Some(ref secret) = mesh_secret {
        // Move a legacy database copy into the keychain, and only forget the
        // database copy once the keychain has demonstrably kept it.
        match secrets::store_mesh_secret(secret) {
            Ok(()) => {
                let _ = sqlx::query("UPDATE sync_state SET mesh_secret = NULL WHERE id = 1")
                    .execute(&pool)
                    .await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "OS keychain unavailable; mesh secret stays in the local database");
            }
        }
    }
    let company_id = db::load_company_id(&pool).await?;

    let state = Arc::new(LocalState::new(
        pool,
        data_dir.clone(),
        cloud_url,
        mesh_secret,
        company_id,
        local_token,
    ));

    // The background mode follows the owner's choice, or the PC's role in the network.
    routes::background::refresh(&state).await;

    let cors = local_auth::cors_layer();

    let loopback = Router::new()
        .merge(routes::health::router())
        .merge(routes::business_router(state.clone()))
        .layer(RequestBodyLimitLayer::new(16 * 1024 * 1024))
        .layer(cors)
        .with_state(state.clone());

    let loopback_addr = format!("127.0.0.1:{LOCAL_API_PORT}");
    let loopback_listener = tokio::net::TcpListener::bind(&loopback_addr).await?;
    state
        .local_port
        .store(LOCAL_API_PORT, std::sync::atomic::Ordering::Relaxed);

    let mesh_router = Router::new()
        .merge(routes::mesh::lan_router())
        .layer(RequestBodyLimitLayer::new(16 * 1024 * 1024))
        .with_state(state.clone());
    let mesh_addr = format!("0.0.0.0:{MESH_API_PORT}");
    let mesh_listener = tokio::net::TcpListener::bind(&mesh_addr).await?;
    state
        .mesh_port
        .store(MESH_API_PORT, std::sync::atomic::Ordering::Relaxed);
    let _ = sqlx::query("UPDATE sync_state SET local_port = $1 WHERE id = 1")
        .bind(MESH_API_PORT as i32)
        .execute(&state.db)
        .await;

    mesh::start_discovery(state.clone(), MESH_API_PORT);
    sync_engine::spawn(state.clone());
    backup::spawn_auto(state.clone());

    let loopback_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(loopback_listener, loopback).await {
            tracing::error!(error = %e, "local business API crashed");
        }
        let _ = loopback_state;
    });
    tokio::spawn(async move {
        if let Err(e) = axum::serve(mesh_listener, mesh_router).await {
            tracing::error!(error = %e, "mesh API crashed");
        }
    });

    info!(base_url = %LOCAL_API_URL, mesh_port = MESH_API_PORT, "local API listening");

    let boot_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = bootstrap::bootstrap_from_cloud(&boot_state).await {
            tracing::warn!(error = %e, "startup cloud bootstrap failed");
        }
    });

    Ok(())
}

#[derive(Clone, serde::Serialize)]
pub struct LocalApiInfo {
    pub url: String,
    pub token: String,
}
