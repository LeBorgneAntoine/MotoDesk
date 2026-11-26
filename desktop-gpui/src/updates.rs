//! What the application knows about updates: whether one is waiting, and the install in
//! progress (port of `desktop/src/lib/stores/updates.svelte.ts` and of the last step of
//! `src-tauri/src/updates.rs`: running the installer).
//!
//! It looks by itself a little after start and then every few hours, unless the shop switched
//! that off; a failed automatic look is silent (a shop without Internet is normal). Nothing is
//! installed without an administrator asking for it, and nothing is run that
//! `motodesk_client::updates` has not verified.
//!
//! A development build never runs an installer, and does not look by itself: it would replace
//! the installed application with a published one. It can be pointed at a stand-in release
//! server to look at the screens (see `dev`).

use std::path::{Path, PathBuf};

use motodesk_client::updates::{self, Prefs, PrefsStore, Source, Update, UpdateError};
use motodesk_ui::gpui::{App, AppContext as _, Context, Entity, Global, SharedString, Task};

use crate::backend::ApiExt as _;

/// The version running now.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Development only: a stand-in for the release server, so that the notice, the release notes
/// and the download can be looked at. `MOTODESK_DEV_UPDATE_ENDPOINT` is the address of its
/// manifest (plain HTTP on this PC) and `MOTODESK_DEV_UPDATE_KEY` the public key its installers
/// are signed with (`examples/release_server.rs` prints both). Neither exists in a release build.
#[cfg(debug_assertions)]
mod dev {
    use motodesk_client::updates::Source;

    pub fn source() -> Option<Source> {
        let endpoint = std::env::var("MOTODESK_DEV_UPDATE_ENDPOINT").ok().filter(|address| !address.is_empty())?;
        let public_key = std::env::var("MOTODESK_DEV_UPDATE_KEY").ok()?;
        Some(Source { endpoint, public_key, insecure: true })
    }
}

#[cfg(not(debug_assertions))]
mod dev {
    use motodesk_client::updates::Source;

    pub fn source() -> Option<Source> {
        None
    }
}

/// Where a downloaded installer waits to be run.
fn installer_folder() -> PathBuf {
    std::env::temp_dir().join("motodesk").join("update")
}

/// Remove the installer of an update that has been through. Called when the app starts.
pub fn sweep() {
    let _ = std::fs::remove_dir_all(installer_folder());
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis() as u64).unwrap_or(0)
}

/// Hand the verified installer to Windows, to be run the way an update is.
#[cfg(windows)]
fn run_installer(installer: &Path, parameters: &str) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

    let wide = |text: &std::ffi::OsStr| -> Vec<u16> { text.encode_wide().chain(std::iter::once(0)).collect() };
    let (verb, file, parameters) = (wide("open".as_ref()), wide(installer.as_os_str()), wide(parameters.as_ref()));
    // SAFETY: every string is nul-terminated and outlives the call.
    let started = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), parameters.as_ptr(), std::ptr::null(), SW_SHOW) };
    // Anything up to 32 is a refusal; the reason is the thread's last error.
    if started as isize <= 32 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(not(windows))]
fn run_installer(_: &Path, _: &str) -> std::io::Result<()> {
    Err(std::io::Error::other("updates are installed on Windows only"))
}

pub struct UpdateState {
    store: PrefsStore,
    pub prefs: Prefs,
    source: Source,
    /// The installer is verified and then left alone: a development build.
    dry_run: bool,
    /// Whether this build looks by itself at all.
    looks_alone: bool,

    pub available: Option<Update>,
    pub checking: bool,
    /// A look asked for by hand found nothing newer.
    pub up_to_date: bool,
    /// Why the last look asked for by hand, or the install, failed.
    pub error: Option<SharedString>,
    /// Said once an installer was verified but not run.
    pub note: Option<SharedString>,

    pub installing: bool,
    pub downloaded: u64,
    pub total: Option<u64>,

    started: bool,
    _install: Option<Task<()>>,
}

struct GlobalUpdates(Entity<UpdateState>);

impl Global for GlobalUpdates {}

impl UpdateState {
    /// Read this PC's choices from its data folder.
    pub fn init(data_dir: &Path, cx: &mut App) {
        let store = PrefsStore::in_dir(data_dir);
        let prefs = store.load();
        let stand_in = dev::source();
        let state = cx.new(|_| UpdateState {
            store,
            prefs,
            looks_alone: !cfg!(debug_assertions) || stand_in.is_some(),
            dry_run: cfg!(debug_assertions),
            source: stand_in.unwrap_or_else(Source::official),
            available: None,
            checking: false,
            up_to_date: false,
            error: None,
            note: None,
            installing: false,
            downloaded: 0,
            total: None,
            started: false,
            _install: None,
        });
        cx.set_global(GlobalUpdates(state));
    }

    pub fn global(cx: &App) -> Entity<UpdateState> {
        cx.global::<GlobalUpdates>().0.clone()
    }

    /// Begin the automatic looking. Safe to call more than once.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.started || !self.looks_alone {
            return;
        }
        self.started = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(updates::FIRST_CHECK_DELAY).await;
            loop {
                let alive = this.update(cx, |this, cx| {
                    if this.prefs.auto && !this.installing {
                        this.check(false, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
                cx.background_executor().timer(updates::CHECK_INTERVAL).await;
            }
        })
        .detach();
    }

    /// Whether the notice is to be shown now (to an administrator).
    pub fn notice(&self) -> Option<&Update> {
        self.available.as_ref().filter(|update| self.prefs.notice_visible(Some(update), now_ms()))
    }

    /// « Plus tard »: hide the notice of this version until tomorrow.
    pub fn later(&mut self, cx: &mut Context<Self>) {
        if let Some(version) = self.available.as_ref().map(|update| update.version.clone()) {
            self.prefs.snooze(&version, now_ms());
            let _ = self.store.save(&self.prefs);
        }
        cx.notify();
    }

    /// Whether this PC looks by itself, kept between two runs.
    pub fn set_auto(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.auto = on;
        let _ = self.store.save(&self.prefs);
        cx.notify();
    }

    /// Look now. A look asked for by hand reports its failure; an automatic one stays quiet.
    pub fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.checking || self.installing {
            return;
        }
        self.checking = true;
        if manual {
            self.error = None;
            self.up_to_date = false;
        }
        cx.notify();
        let source = self.source.clone();
        cx.api(
            move |_| async move { updates::check(&source, VERSION).await },
            move |this, found, cx| {
                this.checking = false;
                match found {
                    Ok(found) => {
                        match &found {
                            Some(update) => tracing::info!(version = %update.version, "update available"),
                            None => tracing::info!("no update available"),
                        }
                        this.up_to_date = manual && found.is_none();
                        this.available = found;
                    }
                    Err(error) => {
                        tracing::info!(error = %error.detail(), "update check failed");
                        if manual {
                            this.error = Some(error.message().into());
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    /// Download what the last look found, verify it and run its installer: the installer takes
    /// over and this app ends, to be started again by it.
    pub fn install(&mut self, cx: &mut Context<Self>) {
        let Some(update) = self.available.clone() else { return };
        if self.installing {
            return;
        }
        tracing::info!(version = %update.version, "installing update");
        self.installing = true;
        self.error = None;
        self.note = None;
        self.downloaded = 0;
        self.total = None;
        cx.notify();

        let source = self.source.clone();
        self._install = Some(cx.api_stream(
            move |_, report| async move {
                let installer = updates::download(&source, &update, |downloaded, total| report((downloaded, total))).await?;
                // Written only once verified: what waits in this folder is what was signed.
                let folder = installer_folder();
                let path = folder.join(updates::installer_name(&update.version));
                std::fs::create_dir_all(&folder)
                    .and_then(|()| std::fs::write(&path, installer))
                    .map_err(|error| UpdateError::File(error.to_string()))?;
                Ok::<_, UpdateError>(path)
            },
            |this, (downloaded, total), cx| {
                this.downloaded = downloaded;
                this.total = total;
                cx.notify();
            },
            |this, verified, cx| {
                this.installing = false;
                match verified {
                    Ok(installer) if this.dry_run => {
                        tracing::info!(installer = %installer.display(), "update verified, not run by a development build");
                        this.note = Some("Installateur téléchargé et vérifié. Une version de développement ne le lance pas.".into());
                    }
                    Ok(installer) => {
                        match run_installer(&installer, updates::INSTALLER_PARAMETERS) {
                            Ok(()) => {
                                tracing::info!("update handed to its installer: closing");
                                cx.quit();
                            }
                            Err(error) => {
                                tracing::error!(error = %error, "the installer did not start");
                                this.error = Some(UpdateError::File(error.to_string()).message().into());
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!(error = %error.detail(), "update failed");
                        this.error = Some(error.message().into());
                    }
                }
                cx.notify();
            },
        ));
    }
}
