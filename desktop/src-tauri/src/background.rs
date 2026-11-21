//! Keeping the PC in the network when nobody has the window open.
//!
//! The data service and the mesh live inside the app's process, so a PC is reachable by the
//! others only while that process runs. With the background mode on:
//!
//! - closing the window **hides** it instead of ending the process (the app stays in the
//!   notification area; its menu opens the window again or really quits);
//! - the app **starts with the Windows session**, hidden (`--background`);
//! - starting it a second time shows the running one instead of starting another (two copies
//!   would fight over the same ports and the same database).
//!
//! This is not a Windows *service*: it starts when the user signs in to Windows, not at boot.
//! A real service would first need the data service split out of the window's process.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Window, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

/// What the autostart entry passes, so a start with Windows opens hidden.
pub const BACKGROUND_ARG: &str = "--background";

static KEEP_RUNNING: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn keep_running() -> bool {
    KEEP_RUNNING.load(Ordering::Relaxed)
}

/// Whether this run was started by the Windows session (hidden), not by the user.
pub fn started_in_background() -> bool {
    std::env::args().any(|a| a == BACKGROUND_ARG)
}

/// The setting is the owner's if they have made one; otherwise a PC of a local network stays
/// on (its place in the network depends on it) and any other installation does not.
pub fn effective(stored: Option<bool>, local_network: bool) -> bool {
    stored.unwrap_or(local_network)
}

/// A development build, or an isolated profile, must not register itself with Windows.
fn may_register_with_windows() -> bool {
    !cfg!(debug_assertions) && std::env::var("MOTODESK_PROFILE").is_err()
}

/// Turn the background mode on or off: the window's close button, and the Windows startup entry.
pub fn apply(enabled: bool) {
    KEEP_RUNNING.store(enabled, Ordering::Relaxed);
    let (Some(app), true) = (APP.get(), may_register_with_windows()) else {
        return;
    };
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    if let Err(e) = result {
        tracing::warn!(error = %e, enabled, "could not update the start-with-Windows entry");
    }
}

/// Show the main window again (from the tray, or because the app was started a second time).
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Closing the window hides it when the background mode is on.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if keep_running() {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

/// The notification-area icon and its menu; remembers the app for [`apply`].
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let _ = APP.set(app.clone());

    let open = MenuItem::with_id(app, "open", "Ouvrir Motodesk", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter (quitte le réseau)", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Motodesk")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owners_choice_wins_and_a_network_pc_stays_on_by_default() {
        assert!(effective(None, true), "a network PC is on by default");
        assert!(!effective(None, false), "any other installation is off by default");
        assert!(!effective(Some(false), true), "the owner turned it off");
        assert!(effective(Some(true), false), "the owner turned it on");
    }
}
