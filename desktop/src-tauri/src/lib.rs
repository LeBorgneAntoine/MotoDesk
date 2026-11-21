mod background;
mod local_server;
mod previous_install;

use local_server::{local_auth, LocalApiInfo, LOCAL_API_URL};
use tauri::Manager;

/// Entry point of `motodesk-keytool` (see `src/bin/motodesk-keytool.rs`): diagnose and
/// repair a register whose signing key was lost. Returns the process exit code.
pub fn keytool_main(args: Vec<String>) -> i32 {
    local_server::fiscal::keytool::main(args)
}

/// Really quit: stops the data service and leaves the network (closing the window does not,
/// when the background mode is on).
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn get_local_api_url(state: tauri::State<'_, LocalApiInfo>) -> LocalApiInfo {
    state.inner().clone()
}

/// Restart the application, used after a restore has been queued so that it is
/// applied without asking the shop owner to close and reopen by hand.
#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .compact()
        .init();

    // Before anything reads the data folder: an install made under the former product
    // name hands its data over to the current folder.
    previous_install::adopt_previous_install();

    let local_token = local_auth::generate_token();
    let api_info = LocalApiInfo {
        url: LOCAL_API_URL.to_string(),
        token: local_token.clone(),
    };

    tauri::Builder::default()
        // First, so a second start never gets as far as the ports: it shows the running app.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            background::show_main(app);
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![background::BACKGROUND_ARG]),
        ))
        .on_window_event(|window, event| background::on_window_event(window, event))
        .invoke_handler(tauri::generate_handler![get_local_api_url, restart_app, quit_app])
        .setup(move |app| {
            app.manage(api_info);

            if let Err(e) = background::install(app.handle()) {
                tracing::warn!(error = %e, "notification-area icon not installed");
            }
            // Started by the Windows session: stay out of the way until the user asks.
            if background::started_in_background() {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.hide();
                }
            }

            #[cfg(windows)]
            if let Some(win) = app.get_webview_window("main") {
                use tauri::window::{Effect, EffectsBuilder};
                if let Err(e) = win.set_effects(EffectsBuilder::new().effect(Effect::Acrylic).build())
                {
                    tracing::warn!(error = %e, "acrylic window effect not applied");
                }
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = local_server::start(&handle, local_token).await {
                    tracing::error!(error = %e, "local server failed to start");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
