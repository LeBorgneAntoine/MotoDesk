//! Motodesk.
//!
//! One process holds two things: the data service (`motodesk-server`: SQLite, the fiscal
//! journal, sync, on Tokio threads) and this interface (GPUI, on the main thread), which calls
//! the service directly. Only one copy runs per installation: a second start shows the first.
//!
//! The window opens at once and shows a spinner until the service is up.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assistant;
mod appearance;
mod auth;
mod backend;
mod background;
mod format;
mod menus;
mod notices;
mod notifications;
mod pages;
mod pos;
mod printing;
mod router;
mod shell;
mod updates;

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
/// otherwise `%APPDATA%\com.motodesk.app` (the folder the application has always used). An isolated
/// profile (`MOTODESK_PROFILE`) gets a folder of its own, so it can never open the shop's data.
fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MOTODESK_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let folder = install_name();
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    #[cfg(windows)]
    let root = var("APPDATA");
    #[cfg(target_os = "macos")]
    let root = var("HOME").map(|h| h.join("Library").join("Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let root = var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share")));

    root.map(|root| root.join(folder))
}

/// What this installation is called on this PC: the name of its data folder, and what tells
/// one running copy from another. An isolated profile has a name of its own.
fn install_name() -> String {
    match secrets::profile() {
        Some(profile) => {
            let safe: String = profile.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
            format!("{}.{safe}", previous_install::IDENTIFIER)
        }
        None => previous_install::IDENTIFIER.to_string(),
    }
}

fn main() {
    logs::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Motodesk starting");

    // First, so that a second start never gets as far as the ports: it shows the running app.
    let background::Instance::First(second_starts) = background::claim(&install_name()) else {
        tracing::info!("Motodesk already runs: its window was asked to show");
        return;
    };
    // The service decides whether the background mode is on; this app carries it out.
    motodesk_server::background::set_host(background::apply);
    // What the service's background work has to say goes to the notification centre.
    let (to_center, from_service) = tokio::sync::mpsc::unbounded_channel();
    motodesk_server::notify::set_host(move |message| {
        let _ = to_center.send(message);
    });

    // Before anything reads the data folder: an install made under the former product name
    // hands its data over to the current folder.
    previous_install::adopt_previous_install();
    // The documents handed to a PDF reader during earlier runs do not stay on this PC.
    printing::sweep();
    // Nor does the installer of an update that has been through.
    updates::sweep();

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
    // What this PC keeps for itself (whether it is a till, the model of its assistant) is
    // read from the same folder, before the window opens.
    let pos_dir = dir.clone();
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
        pos::PosState::init(&pos_dir.clone().unwrap_or_default(), cx);
        assistant::AssistantState::init(&pos_dir.clone().unwrap_or_default(), cx);
        updates::UpdateState::init(&pos_dir.clone().unwrap_or_default(), cx);
        notices::NoticesState::init(&pos_dir.clone().unwrap_or_default(), cx);
        notifications::init(from_service, cx);

        // Started by the Windows session: stay out of the way until the user asks.
        let hidden = background::SUPPORTED && background::started_in_background();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(800.)), cx))),
            window_min_size: Some(size(px(960.), px(600.))),
            show: !hidden,
            focus: !hidden,
            ..TitleBar::window_options()
        };
        let (window, root) = motodesk_ui::gpui::open_window(options, cx, |window, cx| cx.new(|cx| Root::new(window, cx)))
            .expect("the main window opens");
        if !hidden {
            cx.activate(true);
        }
        background::install(window, second_starts, cx);

        // A window that really closes ends the app (and so the service, and this PC's place in the
        // network). With the background mode on it is hidden instead, and stays near the clock.
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
                    #[allow(unused_mut)]
                    let mut cloud_url = state.cloud_url.clone();
                    // Development only: a stand-in for the cloud (`examples/team_cloud.rs`).
                    #[cfg(debug_assertions)]
                    if let Ok(stand_in) = std::env::var(pages::employees::DEV_TEAM_CLOUD) {
                        cloud_url = stand_in;
                    }
                    let client = Client::new(
                        Local::new(service.api, &local_token),
                        Cloud::new(cloud_url),
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
