//! Motodesk, native interface.
//!
//! One process holds two things: the data service (`motodesk-server`: local API, SQLite, sync,
//! on Tokio threads) and this interface (GPUI, on the main thread), which calls the service
//! directly. It is the same service the Tauri shell hosts; the two shells share the data
//! folder and the ports, so only one of them runs at a time.
//!
//! The window opens at once and shows a spinner until the service is up.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod auth;
mod backend;
mod format;
mod pages;
mod router;
mod shell;

use std::path::PathBuf;

use motodesk_client::{Client, Cloud, Local, TokenStore};
use motodesk_server::local_server::{self, local_auth, secrets};
use motodesk_server::{logs, previous_install, startup};
use motodesk_ui::TitleBar;
use motodesk_ui::gpui::*;

use app::Root;
use auth::AuthStore;
use backend::Backend;
use router::Router;

/// Where the app keeps its data: `MOTODESK_DATA_DIR` when set (development, demonstrations),
/// otherwise the same folder as the Tauri shell (`%APPDATA%\com.motodesk.app`). An isolated
/// profile (`MOTODESK_PROFILE`) gets a folder of its own, so it can never open the shop's data.
fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MOTODESK_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    #[cfg(windows)]
    let root = var("APPDATA");
    #[cfg(target_os = "macos")]
    let root = var("HOME").map(|h| h.join("Library").join("Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let root = var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share")));

    let folder = match secrets::profile() {
        Some(profile) => {
            let safe: String = profile.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
            format!("{}.{safe}", previous_install::IDENTIFIER)
        }
        None => previous_install::IDENTIFIER.to_string(),
    };
    root.map(|root| root.join(folder))
}

fn main() {
    logs::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Motodesk (native interface) starting");

    // Before anything reads the data folder: an install made under the former product name
    // hands its data over to the current folder.
    previous_install::adopt_previous_install();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("motodesk-service")
        .build()
        .expect("the service's threads start");
    let handle = runtime.handle().clone();

    // Start the service now; the window does not wait for it.
    let (started, service) = tokio::sync::oneshot::channel();
    let local_token = local_auth::generate_token();
    let token = local_token.clone();
    let dir = data_dir();
    runtime.spawn(async move {
        let result = match dir {
            Some(dir) => local_server::start(dir, token).await,
            None => Err(anyhow::anyhow!("the folder for the app's data could not be found")),
        };
        let _ = started.send(result);
    });

    motodesk_ui::application().run(move |cx| {
        motodesk_ui::init(cx);
        Router::init(cx);
        AuthStore::init(cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(800.)), cx))),
            window_min_size: Some(size(px(960.), px(600.))),
            ..TitleBar::window_options()
        };
        let (_, root) = motodesk_ui::gpui::open_window(options, cx, |window, cx| cx.new(|cx| Root::new(window, cx)))
            .expect("the main window opens");
        cx.activate(true);

        // Closing the window ends the app (and so the service, and this PC's place in the network).
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.spawn(async move |cx| {
            let outcome = service.await.unwrap_or_else(|_| Err(anyhow::anyhow!("the data service stopped before it started")));
            cx.update(|cx| match outcome {
                Ok(service) => {
                    let state = service.state;
                    let client = Client::new(
                        Local::new(service.api, &local_token),
                        Cloud::new(state.cloud_url.clone()),
                        TokenStore::in_dir(&state.data_dir),
                    );
                    cx.set_global(Backend { runtime: handle, client });
                    AuthStore::global(cx).update(cx, |auth, cx| auth.start(cx));
                    root.update(cx, |_, cx| cx.notify());
                }
                Err(error) => {
                    tracing::error!(error = %error, "the data service failed to start");
                    let failure = startup::describe(&error);
                    root.update(cx, |root, cx| root.failed(failure, cx));
                }
            });
        })
        .detach();
    });

    // The window is closed: stop the service's threads without waiting on its long-lived tasks.
    runtime.shutdown_background();
}
