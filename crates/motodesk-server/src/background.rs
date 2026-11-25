//! The "keep running in the background" preference, seen from the data service.
//!
//! The service decides whether the mode is on (`routes/background.rs`); what turning it on
//! means (hiding the window instead of closing it, starting with the Windows session) belongs
//! to the host, which registers itself here.

use std::sync::OnceLock;

type Host = Box<dyn Fn(bool) + Send + Sync>;

static HOST: OnceLock<Host> = OnceLock::new();

/// The setting is the owner's if they have made one; otherwise a PC of a local network stays
/// on (its place in the network depends on it) and any other installation does not.
pub fn effective(stored: Option<bool>, local_network: bool) -> bool {
    stored.unwrap_or(local_network)
}

/// Called once by the host, before the service starts: how to turn the mode on or off.
pub fn set_host(apply: impl Fn(bool) + Send + Sync + 'static) {
    let _ = HOST.set(Box::new(apply));
}

/// Turn the background mode on or off. Does nothing under a host that has no such mode.
pub fn apply(enabled: bool) {
    if let Some(host) = HOST.get() {
        host(enabled);
    }
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
