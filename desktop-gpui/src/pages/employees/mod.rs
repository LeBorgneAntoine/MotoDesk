//! Employés: who can open the shop's data.
//!
//! Two screens share the place. An online company manages its team in the cloud: roles,
//! invitations, accounts switched on and off. A local network has no cloud: its accounts live
//! on the PCs of the shop, next to the recovery key and the list of those PCs.

mod local;
mod online;

use motodesk_ui::gpui::*;

use crate::auth::AuthStore;

/// Development only: the address of a stand-in for the cloud's team service
/// (`examples/team_cloud.rs`). When set, a debug build shows the online team and asks that
/// address for it, whatever the way the shop signs in.
#[cfg(debug_assertions)]
pub const DEV_TEAM_CLOUD: &str = "MOTODESK_DEV_TEAM_CLOUD";

/// The screen that fits how this shop signs in.
pub fn open(window: &mut Window, cx: &mut App) -> AnyView {
    #[cfg(debug_assertions)]
    if std::env::var_os(DEV_TEAM_CLOUD).is_some() {
        return cx.new(|cx| online::CloudTeam::new(window, cx)).into();
    }
    if AuthStore::global(cx).read(cx).is_local() {
        cx.new(|cx| local::LocalAccounts::new(window, cx)).into()
    } else {
        cx.new(|cx| online::CloudTeam::new(window, cx)).into()
    }
}
