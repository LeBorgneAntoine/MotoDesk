//! Application updates.
//!
//! The shop is told when a new version exists and decides when to install it; nothing is
//! installed on its own. What makes an update trustworthy:
//!
//! - It comes from the project's GitHub releases over HTTPS (`plugins.updater.endpoints` in
//!   `tauri.conf.json`).
//! - The installer is signed with a private key held only by the maintainer; the matching
//!   public key is compiled into the application. An installer whose signature does not verify
//!   is never run, whoever served it.
//! - A version that is not newer than the running one is never offered (no downgrade).
//! - The data is migrated by the new version at its first start, after a safety copy of the
//!   database (see `local_server::db::migrate_safely`).

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// What the screen shows about an available version.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// The release notes: this version's section of the changelog.
    pub notes: String,
    /// Publication date, `YYYY-MM-DD`.
    pub date: Option<String>,
}

/// The update found by the last check, kept so that installing it needs no second request.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

/// Progress of a download, sent as the `update-progress` event.
#[derive(Debug, Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

fn describe(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        date: update.date.map(|d| d.date().to_string()),
    }
}

/// Ask the release server whether a newer version exists.
///
/// An unreachable server is an error the screen can show on a manual check and ignore on the
/// automatic one; "no update" is `Ok(None)`.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| {
        tracing::info!(error = %e, "update check failed");
        e.to_string()
    })?;
    let info = found.as_ref().map(describe);
    match &info {
        Some(i) => tracing::info!(version = %i.version, "update available"),
        None => tracing::info!("no update available"),
    }
    *pending.0.lock().unwrap() = found;
    Ok(info)
}

/// Download the update found by the last check, verify its signature and run its installer.
///
/// On Windows the installer takes over and this process exits; the new version starts by
/// itself when the installation ends.
#[tauri::command]
pub async fn update_install(app: AppHandle, pending: State<'_, PendingUpdate>) -> Result<(), String> {
    let update = pending
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "aucune mise à jour à installer : recherchez-en une d'abord".to_string())?;
    tracing::info!(version = %update.version, "installing update");

    let mut downloaded: u64 = 0;
    let events = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = events.emit("update-progress", Progress { downloaded, total });
            },
            || tracing::info!("update downloaded"),
        )
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "update failed");
            e.to_string()
        })?;

    // Reached where the installer does not end the process (not on Windows).
    app.restart();
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_configured_endpoint_is_a_secure_github_release_address() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let updater = &conf["plugins"]["updater"];

        let endpoints = updater["endpoints"].as_array().expect("updater endpoints");
        assert!(!endpoints.is_empty());
        for e in endpoints {
            let url = e.as_str().unwrap();
            assert!(
                url.starts_with("https://github.com/LeBorgneAntoine/MotoDesk/releases/"),
                "an update may only come from the project's releases: {url}"
            );
        }
        assert!(
            updater.get("dangerousInsecureTransportProtocol").is_none(),
            "updates must never be allowed over plain HTTP"
        );
    }

    #[test]
    fn a_public_key_is_compiled_in_and_looks_like_one() {
        use base64::Engine;
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let key = conf["plugins"]["updater"]["pubkey"].as_str().expect("updater pubkey");
        let text = base64::engine::general_purpose::STANDARD.decode(key).expect("base64 public key");
        let text = String::from_utf8(text).expect("utf-8");
        assert!(text.starts_with("untrusted comment: minisign public key"), "{text}");
        assert!(conf["bundle"]["createUpdaterArtifacts"].as_bool().unwrap_or(false));
    }
}
