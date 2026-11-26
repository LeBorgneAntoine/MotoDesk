//! Keeping the PC in the network when nobody has the window open (port of
//! `desktop/src-tauri/src/background.rs`).
//!
//! The data service and the mesh live inside the app's process, so a PC is reachable by the
//! others only while that process runs. With the background mode on:
//!
//! - closing the window **hides** it instead of ending the process (the app stays near the
//!   clock; its icon opens the window again, its menu really quits);
//! - the app **starts with the Windows session**, hidden (`--background`);
//! - starting it a second time shows the running one instead of starting another (two copies
//!   would fight over the same ports and the same database).
//!
//! The service decides whether the mode is on (`motodesk_server::background`); this module
//! carries it out. This is not a Windows *service*: it starts when the user signs in to
//! Windows, not at boot.
//!
//! Windows only for now. Elsewhere there is no icon, so the window is never hidden: closing it
//! ends the app whatever the setting says.

use std::sync::atomic::{AtomicBool, Ordering};

use motodesk_server::local_server::secrets;
use motodesk_ui::gpui::{AnyWindowHandle, App, Global, Window};
use tokio::sync::mpsc::unbounded_channel;

/// What the start-up entry passes, so that a start with Windows opens hidden.
pub const BACKGROUND_ARG: &str = "--background";

/// Whether this build can stay near the clock at all.
pub const SUPPORTED: bool = cfg!(windows);

static KEEP_RUNNING: AtomicBool = AtomicBool::new(false);
/// The icon is near the clock: there is a way back to a hidden window.
static ICON_SHOWN: AtomicBool = AtomicBool::new(false);
/// The window was hidden by its close button.
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// What the icon, its menu or a second start asks of the running app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Command {
    Show,
    Quit,
}

/// Whether closing the window keeps the app running.
pub fn keep_running() -> bool {
    KEEP_RUNNING.load(Ordering::Relaxed)
}

/// Whether this run was started by the Windows session (hidden), not by the user.
pub fn started_in_background() -> bool {
    std::env::args().any(|arg| arg == BACKGROUND_ARG)
}

/// A development build, or an isolated profile, must not register itself with Windows.
pub fn registers_with_windows() -> bool {
    !cfg!(debug_assertions) && secrets::profile().is_none()
}

/// Turn the background mode on or off: the window's close button, and the Windows start-up
/// entry. Called by the service, from its own threads, at start and whenever the choice changes.
pub fn apply(enabled: bool) {
    KEEP_RUNNING.store(enabled, Ordering::Relaxed);
    if !registers_with_windows() {
        return;
    }
    if let Err(error) = platform::start_with_session(enabled) {
        tracing::warn!(error = %error, enabled, "could not update the start-with-Windows entry");
    }
}

/// The name both this app and a second start of it agree on: one per data folder name, so an
/// isolated profile never takes the place of the real installation.
pub fn instance_name(folder: &str) -> String {
    format!("Local\\{folder}.instance")
}

/// This process's place as the one running copy.
pub enum Instance {
    /// The only one. Holds what tells it of a later start.
    First(platform::Listener),
    /// Another copy runs: it has been asked to show its window, and this one has nothing to do.
    Second,
}

/// What a restart passes to the copy that replaces this one: it is not a second start, and
/// waits for the closing copy to be gone instead of stepping aside for it.
pub const RESTARTED_ARG: &str = "--restarted";

/// How long a restarted copy waits for the one it replaces.
const RESTART_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Find out whether a copy already runs, before anything touches the ports or the database.
pub fn claim(folder: &str) -> Instance {
    let name = instance_name(folder);
    if std::env::args().any(|arg| arg == RESTARTED_ARG) { platform::claim_after(&name, RESTART_WAIT) } else { platform::claim(&name) }
}

/// The arguments a restarted copy is started with: those of this run, and the mark of a restart.
pub fn restart_arguments(current: impl Iterator<Item = String>) -> Vec<String> {
    current.filter(|arg| arg != RESTARTED_ARG).chain(std::iter::once(RESTARTED_ARG.to_string())).collect()
}

/// The icon near the clock, kept for as long as the app runs.
struct Tray(#[allow(dead_code)] Option<platform::TrayHandle>);

impl Global for Tray {}

/// Put the icon near the clock and listen to it, to its menu, and to later starts of the app.
pub fn install(window: AnyWindowHandle, listener: platform::Listener, cx: &mut App) {
    let (commands, mut asked) = unbounded_channel();
    let tray = platform::tray(commands.clone());
    ICON_SHOWN.store(tray.is_some(), Ordering::Relaxed);
    cx.set_global(Tray(tray));
    listener.forward(commands);

    // A killed process leaves its icon behind until the pointer passes over it: on a normal
    // exit it is taken away.
    cx.on_app_quit(|cx| {
        cx.remove_global::<Tray>();
        async {}
    })
    .detach();

    // Closing the window hides it when the mode is on and there is an icon to come back by.
    let _ = window.update(cx, |_, window, cx| window.on_window_should_close(cx, |window, _| !hide_instead_of_closing(window)));

    cx.spawn(async move |cx| {
        while let Some(command) = asked.recv().await {
            cx.update(|cx| match command {
                Command::Show => show(window, cx),
                Command::Quit => cx.quit(),
            });
        }
    })
    .detach();
}

/// The close button was pressed. Returns whether the window was hidden rather than left to close.
fn hide_instead_of_closing(window: &mut Window) -> bool {
    if !(keep_running() && ICON_SHOWN.load(Ordering::Relaxed)) {
        return false;
    }
    if platform::set_visible(window, false) {
        HIDDEN.store(true, Ordering::Relaxed);
        return true;
    }
    false
}

/// Bring the window back: from the icon, or because the app was started a second time.
pub fn show(window: AnyWindowHandle, cx: &mut App) {
    let _ = window.update(cx, |_, window, _| {
        // A window that opened hidden (a start with Windows) is placed and shown by its first
        // activation; one hidden by its close button has to be shown first.
        if HIDDEN.swap(false, Ordering::Relaxed) {
            platform::set_visible(window, true);
        }
        window.activate_window();
    });
}

#[cfg(windows)]
pub mod platform {
    use std::path::Path;

    use auto_launch::{AutoLaunch, AutoLaunchBuilder};
    use motodesk_client::templates::render::Picture;
    use motodesk_ui::gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use tokio::sync::mpsc::UnboundedSender;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOW, ShowWindow};

    use super::{BACKGROUND_ARG, Command, Instance};

    pub type TrayHandle = TrayIcon;

    /// The name of the start-up entry. It is the one the Tauri shell wrote: this app takes that
    /// entry over instead of adding a second one.
    const STARTUP_ENTRY: &str = "Motodesk";

    // ---- starting with the session -------------------------------------------------------------

    /// The start-up entry of the program at `exe`: its path in quotes (it may hold spaces),
    /// then the mark of a hidden start.
    pub fn startup_entry(exe: &Path) -> auto_launch::Result<AutoLaunch> {
        AutoLaunchBuilder::new().set_app_name(STARTUP_ENTRY).set_app_path(&format!("\"{}\"", exe.display())).set_args(&[BACKGROUND_ARG]).build()
    }

    /// Write or remove the entry that starts this program with the Windows session.
    pub fn start_with_session(enabled: bool) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let entry = startup_entry(&exe).map_err(|error| error.to_string())?;
        if enabled { entry.enable() } else { entry.disable() }.map_err(|error| error.to_string())
    }

    // ---- one copy at a time --------------------------------------------------------------------

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// What the first copy waits on, and a later one sets.
    fn signal_name(instance: &str) -> Vec<u16> {
        wide(&format!("{instance}.show"))
    }

    /// The first copy's place and its ear for later starts. Once it listens, both last as long
    /// as the process; dropped before that, the place is free again.
    pub struct Listener {
        mutex: HANDLE,
        signal: HANDLE,
    }

    // SAFETY: the two handles name kernel objects; they can be used and closed from any thread.
    unsafe impl Send for Listener {}

    impl Drop for Listener {
        fn drop(&mut self) {
            // SAFETY: the handles are this value's own, closed once.
            unsafe {
                for handle in [self.signal, self.mutex] {
                    if !handle.is_null() {
                        CloseHandle(handle);
                    }
                }
            }
        }
    }

    /// Take the place of a copy that is closing: wait, up to `patience`, for it to be gone.
    /// If it never goes, this is a second start after all.
    pub fn claim_after(instance: &str, patience: std::time::Duration) -> Instance {
        let started = std::time::Instant::now();
        loop {
            // SAFETY: as in `claim`.
            let taken = unsafe {
                let mutex = CreateMutexW(std::ptr::null(), 0, wide(instance).as_ptr());
                let taken = !mutex.is_null() && GetLastError() == ERROR_ALREADY_EXISTS;
                if !mutex.is_null() {
                    CloseHandle(mutex);
                }
                taken
            };
            if !taken || started.elapsed() >= patience {
                return claim(instance);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    pub fn claim(instance: &str) -> Instance {
        // SAFETY: plain calls with nul-terminated names that outlive them; the handles returned
        // are checked before use.
        unsafe {
            let mutex = CreateMutexW(std::ptr::null(), 0, wide(instance).as_ptr());
            if !mutex.is_null() && GetLastError() == ERROR_ALREADY_EXISTS {
                let signal = OpenEventW(EVENT_MODIFY_STATE, 0, signal_name(instance).as_ptr());
                if !signal.is_null() {
                    SetEvent(signal);
                    CloseHandle(signal);
                }
                CloseHandle(mutex);
                return Instance::Second;
            }
            // The mutex is what a later copy finds for as long as this one holds it.
            let signal = CreateEventW(std::ptr::null(), 0, 0, signal_name(instance).as_ptr());
            Instance::First(Listener { mutex, signal })
        }
    }

    impl Listener {
        /// Each later start of the app asks this one to show its window.
        pub fn forward(self, commands: UnboundedSender<Command>) {
            if self.signal.is_null() {
                return;
            }
            let signal = self.signal as usize;
            // From here on the place is held until the process ends.
            std::mem::forget(self);
            let waiting = std::thread::Builder::new().name("motodesk-second-start".into()).spawn(move || {
                // SAFETY: the event handle is never closed while the process runs.
                while unsafe { WaitForSingleObject(signal as HANDLE, INFINITE) } == WAIT_OBJECT_0 {
                    if commands.send(Command::Show).is_err() {
                        break;
                    }
                }
            });
            if let Err(error) = waiting {
                tracing::warn!(error = %error, "a second start will not show this window");
            }
        }
    }

    // ---- the window ----------------------------------------------------------------------------

    /// Hide or show the window without closing it. Returns whether the system was asked.
    pub fn set_visible(window: &Window, visible: bool) -> bool {
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
        let RawWindowHandle::Win32(native) = handle.as_raw() else { return false };
        // SAFETY: the handle is this window's, alive for the length of the call.
        unsafe { ShowWindow(native.hwnd.get() as _, if visible { SW_SHOW } else { SW_HIDE }) };
        true
    }

    // ---- the icon ------------------------------------------------------------------------------

    const OPEN: &str = "open";
    const QUIT: &str = "quit";

    /// The icon near the clock and its menu. `None` when it could not be shown: the window is
    /// then never hidden.
    pub fn tray(commands: UnboundedSender<Command>) -> Option<TrayIcon> {
        let built = (|| -> Result<TrayIcon, String> {
            let picture = Picture::decode(include_bytes!("../assets/tray.png")).ok_or("the icon does not decode")?;
            let icon = Icon::from_rgba(picture.rgba, picture.width, picture.height).map_err(|error| error.to_string())?;
            let menu = Menu::new();
            let open = MenuItem::with_id(OPEN, "Ouvrir Motodesk", true, None);
            let quit = MenuItem::with_id(QUIT, "Quitter (quitte le réseau)", true, None);
            menu.append_items(&[&open, &quit]).map_err(|error| error.to_string())?;
            TrayIconBuilder::new()
                .with_tooltip("Motodesk")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .map_err(|error| error.to_string())
        })();
        let tray = match built {
            Ok(tray) => tray,
            Err(error) => {
                tracing::warn!(error = %error, "notification-area icon not installed");
                return None;
            }
        };

        // Both are called on the main thread, from the system's own message loop: they only
        // pass the request on.
        let from_menu = commands.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let command = match event.id.as_ref() {
                OPEN => Command::Show,
                QUIT => Command::Quit,
                _ => return,
            };
            let _ = from_menu.send(command);
        }));
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            let clicked = matches!(
                event,
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                    | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
            );
            if clicked {
                let _ = commands.send(Command::Show);
            }
        }));
        Some(tray)
    }
}

#[cfg(not(windows))]
pub mod platform {
    use motodesk_ui::gpui::Window;
    use tokio::sync::mpsc::UnboundedSender;

    use super::{Command, Instance};

    pub struct TrayHandle;
    pub struct Listener;

    pub fn start_with_session(_: bool) -> Result<(), String> {
        Ok(())
    }

    pub fn claim(_: &str) -> Instance {
        Instance::First(Listener)
    }

    pub fn claim_after(_: &str, _: std::time::Duration) -> Instance {
        Instance::First(Listener)
    }

    impl Listener {
        pub fn forward(self, _: UnboundedSender<Command>) {}
    }

    pub fn set_visible(_: &Window, _: bool) -> bool {
        false
    }

    pub fn tray(_: UnboundedSender<Command>) -> Option<TrayHandle> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{BACKGROUND_ARG, instance_name};

    #[test]
    fn each_data_folder_has_its_own_running_copy() {
        assert_eq!(instance_name("com.motodesk.app"), "Local\\com.motodesk.app.instance");
        assert_ne!(instance_name("com.motodesk.app"), instance_name("com.motodesk.app.essai"));
    }

    #[cfg(windows)]
    #[test]
    fn the_start_up_entry_starts_this_program_hidden() {
        let entry = super::platform::startup_entry(std::path::Path::new(r"C:\Program Files\Motodesk\motodesk.exe")).unwrap();
        // The entry the Tauri shell wrote, taken over; the path survives its space.
        assert_eq!(entry.get_app_name(), "Motodesk");
        assert_eq!(entry.get_app_path(), r#""C:\Program Files\Motodesk\motodesk.exe""#);
        assert_eq!(entry.get_args(), [BACKGROUND_ARG]);
    }

    #[cfg(windows)]
    #[test]
    fn a_second_copy_finds_the_first_and_wakes_it() {
        use tokio::sync::mpsc::unbounded_channel;

        use super::platform::claim;
        use super::{Command, Instance};

        let name = instance_name(&format!("motodesk-test-{}", std::process::id()));
        let Instance::First(listener) = claim(&name) else { panic!("nothing else holds this name") };
        let (commands, mut asked) = unbounded_channel();
        listener.forward(commands);
        assert!(asked.try_recv().is_err(), "nobody has asked yet");

        // A later start under the same name steps aside and asks the first to show itself.
        assert!(matches!(claim(&name), Instance::Second));
        assert_eq!(asked.blocking_recv(), Some(Command::Show));
        // Another name is another installation: it is the first of its own.
        assert!(matches!(claim(&format!("{name}-other")), Instance::First(_)));
    }

    #[cfg(windows)]
    #[test]
    fn a_restarted_copy_waits_for_the_one_it_replaces() {
        use std::time::{Duration, Instant};

        use super::Instance;
        use super::platform::{claim, claim_after};

        let name = instance_name(&format!("motodesk-restart-{}", std::process::id()));
        let Instance::First(closing) = claim(&name) else { panic!("nothing else holds this name") };
        // The copy being replaced goes away a little later, as an app does when it quits.
        let leaving = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            drop(closing);
        });
        let started = Instant::now();
        assert!(matches!(claim_after(&name, Duration::from_secs(10)), Instance::First(_)), "the restarted copy takes the place");
        assert!(started.elapsed() >= Duration::from_millis(300), "it waited for the other to go");
        leaving.join().unwrap();

        // A copy that never goes: the restarted one gives up and steps aside.
        let stuck = instance_name(&format!("motodesk-stuck-{}", std::process::id()));
        let Instance::First(_staying) = claim(&stuck) else { panic!("nothing else holds this name") };
        assert!(matches!(claim_after(&stuck, Duration::from_millis(300)), Instance::Second));
    }

    #[test]
    fn a_restart_keeps_the_arguments_and_says_it_is_one() {
        let arguments = |given: &[&str]| super::restart_arguments(given.iter().map(|arg| arg.to_string()));
        assert_eq!(arguments(&[]), ["--restarted"]);
        assert_eq!(arguments(&["--background"]), ["--background", "--restarted"]);
        assert_eq!(arguments(&["--restarted", "--background"]), ["--background", "--restarted"], "said once");
    }
}
