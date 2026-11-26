//! Whether this PC is a till (port of the state half of `desktop/src/lib/pos/store.svelte.ts`).
//!
//! The choice is this PC's own (see `motodesk_client::pos::PosConfig`). A PC set up as a till
//! opens on the till, full screen; an administrator can step out to the office screens
//! without giving the role up.

use std::path::Path;

use motodesk_client::pos::{PosConfig, PosConfigStore};
use motodesk_ui::gpui::{App, AppContext as _, Context, Entity, Global, Window};

pub struct PosState {
    store: PosConfigStore,
    pub config: PosConfig,
    /// An administrator stepped out to the office screens; the PC is still a till.
    in_office: bool,
}

struct GlobalPos(Entity<PosState>);

impl Global for GlobalPos {}

impl PosState {
    /// Read the role of this PC from its data folder.
    pub fn init(data_dir: &Path, cx: &mut App) {
        let store = PosConfigStore::in_dir(data_dir);
        let config = store.load();
        let state = cx.new(|_| PosState { store, config, in_office: false });
        cx.set_global(GlobalPos(state));
    }

    pub fn global(cx: &App) -> Entity<PosState> {
        cx.global::<GlobalPos>().0.clone()
    }

    /// The till is what this PC shows.
    pub fn active(&self) -> bool {
        self.config.enabled && !self.in_office
    }

    /// Keep a new configuration, on screen and on disk.
    pub fn save(&mut self, config: PosConfig, cx: &mut Context<Self>) -> std::io::Result<()> {
        self.config = config;
        cx.notify();
        self.store.save(&self.config)
    }

    pub fn enter_office(&mut self, cx: &mut Context<Self>) {
        self.in_office = true;
        cx.notify();
    }

    pub fn enter_till(&mut self, cx: &mut Context<Self>) {
        self.in_office = false;
        cx.notify();
    }
}

/// Put the window in or out of full screen. A till fills the screen; the office does not.
///
/// Development only: `MOTODESK_DEV_WINDOWED` keeps the window as it is, to look at the till
/// without it taking the whole screen.
pub fn set_fullscreen(on: bool, window: &mut Window) {
    #[cfg(debug_assertions)]
    if std::env::var_os("MOTODESK_DEV_WINDOWED").is_some() {
        return;
    }
    if window.is_fullscreen() != on {
        window.toggle_fullscreen();
    }
}
