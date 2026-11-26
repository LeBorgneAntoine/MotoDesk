//! What this PC remembers about its assistant while the app runs (port of the device half of
//! `desktop/src/lib/assistant/store.svelte.ts`): the model it talks to, whether the list of
//! conversations is folded, and the demonstration conversations of a PC without a key.
//!
//! The demonstration conversations live here, not in the screen, so that leaving the screen
//! and coming back finds them. They are not kept between two runs.

use std::path::Path;

use motodesk_client::assistant::{Conversation, Prefs, PrefsStore};
use motodesk_ui::gpui::{App, AppContext as _, Context, Entity, Global};

/// Identifiers for what exists only on screen: a message not yet recorded, a demonstration.
#[derive(Debug, Default)]
pub struct Ids(u64);

impl Ids {
    pub fn next(&mut self, what: &str) -> String {
        self.0 += 1;
        format!("local-{what}-{}", self.0)
    }
}

pub struct AssistantState {
    store: PrefsStore,
    pub prefs: Prefs,
    /// The conversations of a PC without a key, the latest first.
    pub demo: Vec<Conversation>,
    pub ids: Ids,
}

struct GlobalAssistant(Entity<AssistantState>);

impl Global for GlobalAssistant {}

impl AssistantState {
    /// Read this PC's choices from its data folder.
    pub fn init(data_dir: &Path, cx: &mut App) {
        let store = PrefsStore::in_dir(data_dir);
        let prefs = store.load();
        let state = cx.new(|_| AssistantState { store, prefs, demo: Vec::new(), ids: Ids::default() });
        cx.set_global(GlobalAssistant(state));
    }

    pub fn global(cx: &App) -> Entity<AssistantState> {
        cx.global::<GlobalAssistant>().0.clone()
    }

    /// Keep new choices, on screen and on disk.
    pub fn save(&mut self, prefs: Prefs, cx: &mut Context<Self>) -> std::io::Result<()> {
        self.prefs = prefs;
        cx.notify();
        self.store.save(&self.prefs)
    }

    /// Put a demonstration conversation back after it changed.
    pub fn keep_demo(&mut self, conversation: &Conversation, cx: &mut Context<Self>) {
        match self.demo.iter_mut().find(|kept| kept.id == conversation.id) {
            Some(kept) => *kept = conversation.clone(),
            None => self.demo.insert(0, conversation.clone()),
        }
        cx.notify();
    }
}
