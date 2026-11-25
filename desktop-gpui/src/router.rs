//! Which screen is shown (port of `desktop/src/lib/router/router.svelte.ts`).
//!
//! A route is an id (`clients`, `stock`, ...) and a few parameters. The parameters carry the
//! state worth finding again, most importantly the open panel stack.

use std::collections::BTreeMap;

use motodesk_ui::gpui::{App, AppContext as _, Context, Entity, Global, SharedString};

pub const CLIENTS: &str = "clients";

/// The screen the app opens on. It will be the dashboard once that screen is migrated; until
/// then, the first screen that is.
pub const HOME: &str = CLIENTS;

pub struct Router {
    current: SharedString,
    params: BTreeMap<String, String>,
}

struct GlobalRouter(Entity<Router>);

impl Global for GlobalRouter {}

impl Router {
    pub fn init(cx: &mut App) {
        #[allow(unused_mut)]
        let mut params = BTreeMap::new();
        // Development only: start with a panel open (`MOTODESK_DEV_PANEL=client:<id>`), to look
        // at a panel without clicking through to it.
        #[cfg(debug_assertions)]
        if let Ok(panel) = std::env::var("MOTODESK_DEV_PANEL") {
            params.insert("panel".to_string(), panel);
        }
        let router = cx.new(|_| Router { current: HOME.into(), params });
        cx.set_global(GlobalRouter(router));
    }

    pub fn global(cx: &App) -> Entity<Router> {
        cx.global::<GlobalRouter>().0.clone()
    }

    pub fn current(&self) -> &SharedString {
        &self.current
    }

    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }

    /// Go to a screen, forgetting the parameters of the one that is left.
    pub fn go(&mut self, id: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.go_with(id, [], cx);
    }

    pub fn go_with<const N: usize>(
        &mut self,
        id: impl Into<SharedString>,
        params: [(&str, &str); N],
        cx: &mut Context<Self>,
    ) {
        self.current = id.into();
        self.params = params.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        cx.notify();
    }

    /// Set or remove (`None`) one parameter of the current screen, without redrawing it: the
    /// screen that calls this already shows the state it records.
    pub fn set_param(&mut self, key: &str, value: Option<String>) {
        match value.filter(|v| !v.is_empty()) {
            Some(value) => {
                self.params.insert(key.to_string(), value);
            }
            None => {
                self.params.remove(key);
            }
        }
    }
}
