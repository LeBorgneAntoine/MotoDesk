//! Log files, and a record of crashes.
//!
//! The release build has no console, so without a file a shop's problem leaves nothing to
//! look at. Logs go to a daily file under the machine-local data folder, 14 days are kept,
//! and a panic is written there with its location before the process dies.
//!
//! The folder is deliberately not inside the app's data folder (`com.motodesk.app`): that
//! folder is where a previous install's data is moved to at start-up, and it must not exist
//! yet when that happens.

use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

/// How many daily files are kept.
const KEPT_FILES: usize = 14;
const FILE_PREFIX: &str = "motodesk";

/// What is logged unless `RUST_LOG` says otherwise. The mDNS library reports every failed send
/// on every network adapter (a VPN's IPv6 address, say) again every second, which would fill
/// the file; the mesh module logs whether the shop's own advertisement worked.
const DEFAULT_FILTER: &str = "info,mdns_sd=off";

/// Keeps the background writer alive for the life of the process.
static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Where the log files are written.
pub fn dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    #[cfg(windows)]
    let base = var("LOCALAPPDATA").map(|p| p.join("Motodesk"));
    #[cfg(target_os = "macos")]
    let base = var("HOME").map(|h| h.join("Library").join("Logs").join("Motodesk"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = var("XDG_STATE_HOME")
        .or_else(|| var("HOME").map(|h| h.join(".local").join("state")))
        .map(|p| p.join("motodesk"));
    let base = base?;
    // A test copy of the app keeps its own logs.
    Some(match crate::local_server::secrets::profile() {
        Some(profile) => {
            let safe: String = profile.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
            base.join(format!("logs-{safe}"))
        }
        None => base.join("logs"),
    })
}

/// Start logging to the console and to the daily file. Never fails: a folder that cannot be
/// written to only means there is no file.
pub fn init() {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let console = fmt::layer().compact().with_filter(filter());

    let file_writer = dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        let appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix(FILE_PREFIX)
            .filename_suffix("log")
            .max_log_files(KEPT_FILES)
            .build(&dir)
            .ok()?;
        Some(tracing_appender::non_blocking(appender))
    });

    match file_writer {
        Some((writer, guard)) => {
            let _ = GUARD.set(guard);
            let file = fmt::layer().with_ansi(false).with_writer(writer).with_filter(filter());
            let _ = tracing_subscriber::registry().with(console).with(file).try_init();
        }
        None => {
            let _ = tracing_subscriber::registry().with(console).try_init();
        }
    }
    install_panic_hook();
}

/// Put a panic in the log, with where it happened, then let the default behaviour go on.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".into());
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".into());
        let thread = std::thread::current();
        tracing::error!(
            thread = thread.name().unwrap_or("unnamed"),
            at = %place,
            "panic: {message}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        // The writer is asynchronous: give it a moment to reach the disk.
        std::thread::sleep(std::time::Duration::from_millis(200));
        previous(info);
    }));
}

/// Open the folder in the system's file manager.
pub fn open_dir() -> Result<(), String> {
    let dir = dir().ok_or("dossier des journaux introuvable")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let mut command = std::process::Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    command.arg(&dir).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_filter_is_valid_and_keeps_info() {
        let filter = EnvFilter::try_new(DEFAULT_FILTER).expect("a valid filter");
        assert!(filter.to_string().contains("mdns_sd=off"));
    }

    #[test]
    fn the_log_folder_is_not_the_apps_data_folder() {
        let dir = dir().expect("a folder on this platform");
        assert!(!dir.to_string_lossy().contains(crate::previous_install::IDENTIFIER));
        assert!(dir.ends_with("logs") || dir.to_string_lossy().contains("logs-"));
    }
}
