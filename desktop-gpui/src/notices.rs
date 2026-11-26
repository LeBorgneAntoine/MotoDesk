//! The notices this PC has put off, kept between two runs (see `motodesk_client::notices`).

use std::path::Path;

use motodesk_client::notices::{Notices, NoticesStore};
use motodesk_ui::gpui::{App, AppContext as _, Context, Entity, Global};

pub struct NoticesState {
    store: NoticesStore,
    pub notices: Notices,
}

struct GlobalNotices(Entity<NoticesState>);

impl Global for GlobalNotices {}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis() as u64).unwrap_or(0)
}

impl NoticesState {
    /// Read what this PC has put off from its data folder.
    pub fn init(data_dir: &Path, cx: &mut App) {
        let store = NoticesStore::in_dir(data_dir);
        let notices = store.load();
        let state = cx.new(|_| NoticesState { store, notices });
        cx.set_global(GlobalNotices(state));
    }

    pub fn global(cx: &App) -> Entity<NoticesState> {
        cx.global::<GlobalNotices>().0.clone()
    }

    pub fn backup_reminder_snoozed(&self) -> bool {
        self.notices.backup_reminder_snoozed(now_ms())
    }

    /// « Demain »: the backup reminder comes back in a day.
    pub fn snooze_backup_reminder(&mut self, cx: &mut Context<Self>) {
        self.notices.snooze_backup_reminder(now_ms());
        let _ = self.store.save(&self.notices);
        cx.notify();
    }

    /// « Plus tard » on the hint about the till: it is not shown again on this PC.
    pub fn dismiss_till_hint(&mut self, cx: &mut Context<Self>) {
        self.notices.till_hint_dismissed = true;
        let _ = self.store.save(&self.notices);
        cx.notify();
    }
}
