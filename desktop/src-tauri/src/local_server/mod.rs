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
pub mod recovery_key;
pub mod mesh_auth;
pub mod network_devices;
pub mod pos;
pub mod routes;
pub mod secrets;
pub mod shared_folder;
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

/// A listening socket that a program started by this one does not inherit.
///
/// When Motodesk restarts itself (after a restore, say) the new process is a child of the old
/// one and, on Windows, receives every inheritable handle. A listening socket is one: the new
/// process then held port 17300 itself, for as long as it ran, and could never bind it.
fn bind_listener(addr: &str) -> std::io::Result<tokio::net::TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};

    let addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    #[cfg(windows)]
    socket.set_no_inherit(true)?;
    // No SO_REUSEADDR: on Windows it would let a second program share the port.
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(socket.into())
}

/// How long a starting Motodesk waits for the previous one to let go of its ports.
const PORT_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Bind a listener, trying again while the previous Motodesk is still closing. A restart (after
/// a restore, say) launches the new process before the old one has exited, and both want the
/// same ports; failing on the first refusal left the new one without a data service.
async fn bind_when_free(addr: &str, port: u16) -> Result<tokio::net::TcpListener, crate::startup::StartupError> {
    bind_within(addr, port, PORT_WAIT).await
}

async fn bind_within(
    addr: &str,
    port: u16,
    wait: std::time::Duration,
) -> Result<tokio::net::TcpListener, crate::startup::StartupError> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        match bind_listener(addr) {
            Ok(listener) => return Ok(listener),
            Err(source) if tokio::time::Instant::now() >= deadline => {
                return Err(crate::startup::StartupError::PortInUse { port, source })
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
        }
    }
}

pub async fn start(app: &tauri::AppHandle, local_token: String) -> anyhow::Result<()> {
    // Before touching the database: the previous process may still have it open, and a restore
    // waiting to be applied must not be applied under it. Its ports are the sign it has left.
    drop(bind_when_free(&format!("127.0.0.1:{LOCAL_API_PORT}"), LOCAL_API_PORT).await?);

    let data_dir = crate::previous_install::data_dir_in_use(app.path().app_data_dir()?);
    std::fs::create_dir_all(&data_dir)?;
    // A restore that cannot be applied must not stop the app from starting on its
    // current data: the failure is logged and the restore stays queued.
    if let Err(e) = backup::apply_pending_restore(&data_dir) {
        tracing::error!(error = %e, "could not apply the pending restore; starting on the current data");
    }
    let db_path = data_dir.join("local.db");

    let pool = db::init_pool(&db_path).await?;
    db::migrate_safely(&pool, &data_dir).await?;
    db::ensure_device_id(&pool).await?;
    backup::finish_restore(&pool, &data_dir).await?;
    let cloud_url = db::ensure_cloud_url(&pool).await?;
    if local_accounts::repair_half_joined(&pool).await? {
        tracing::warn!("a join that stopped halfway was undone: this PC can create or join a network again");
        let _ = secrets::clear_mesh_secret();
    }
    let mesh_secret = db::current_mesh_secret(&pool).await?;
    if let Some(ref secret) = mesh_secret {
        // Move a database copy into the keychain, and only forget the
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
    let loopback_listener = bind_when_free(&loopback_addr, LOCAL_API_PORT).await?;
    state
        .local_port
        .store(LOCAL_API_PORT, std::sync::atomic::Ordering::Relaxed);

    let mesh_router = Router::new()
        .merge(routes::mesh::lan_router())
        .layer(RequestBodyLimitLayer::new(16 * 1024 * 1024))
        .with_state(state.clone());
    let mesh_addr = format!("0.0.0.0:{MESH_API_PORT}");
    let mesh_listener = bind_when_free(&mesh_addr, MESH_API_PORT).await?;
    state
        .mesh_port
        .store(MESH_API_PORT, std::sync::atomic::Ordering::Relaxed);
    let _ = sqlx::query("UPDATE sync_state SET local_port = $1 WHERE id = 1")
        .bind(MESH_API_PORT as i32)
        .execute(&state.db)
        .await;

    if let Err(e) = fiscal::announce_unsent(&state).await {
        tracing::warn!(error = %e, "could not queue unsent journal entries");
    }
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

#[cfg(test)]
mod port_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn a_port_the_previous_process_is_about_to_release_is_waited_for() {
        let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = held.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(600)).await;
            drop(held);
        });
        let started = std::time::Instant::now();
        let listener = bind_within(&addr, 0, Duration::from_secs(10)).await.expect("bound once it was free");
        assert!(started.elapsed() >= Duration::from_millis(400), "it did wait");
        drop(listener);
    }

    #[tokio::test]
    async fn a_port_that_stays_taken_is_reported_after_the_wait() {
        let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = held.local_addr().unwrap().to_string();
        let err = bind_within(&addr, 4242, Duration::from_millis(500)).await.unwrap_err();
        assert!(matches!(err, crate::startup::StartupError::PortInUse { port: 4242, .. }));
        drop(held);
    }
}
