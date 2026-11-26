//! The notices a PC has put off (the Svelte interface kept these in the browser's storage): the
//! backup reminder until tomorrow, the hint about the till for good.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::device_store::DeviceFile;

const FILE: &str = "notices.store.json";
const KEY: &str = "notices.v1";

/// « Demain » hides the backup reminder for this long.
pub const BACKUP_SNOOZE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notices {
    /// The backup reminder stays hidden until then (milliseconds since 1970).
    pub backup_reminder_until: u64,
    /// The hint « this PC is not a till yet » was dismissed.
    pub till_hint_dismissed: bool,
}

impl Notices {
    pub fn backup_reminder_snoozed(&self, now_ms: u64) -> bool {
        self.backup_reminder_until > now_ms
    }

    pub fn snooze_backup_reminder(&mut self, now_ms: u64) {
        self.backup_reminder_until = now_ms + BACKUP_SNOOZE.as_millis() as u64;
    }
}

/// Where [`Notices`] are kept between two runs.
#[derive(Debug, Clone)]
pub struct NoticesStore {
    file: DeviceFile,
}

impl NoticesStore {
    /// `data_dir` is the app's data folder.
    pub fn in_dir(data_dir: &Path) -> Self {
        Self { file: DeviceFile::in_dir(data_dir, FILE) }
    }

    pub fn load(&self) -> Notices {
        self.file.get(KEY).unwrap_or_default()
    }

    pub fn save(&self, notices: &Notices) -> std::io::Result<()> {
        self.file.set(KEY, notices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_put_off_comes_back_the_next_day_and_is_kept_between_two_runs() {
        let mut notices = Notices::default();
        assert!(!notices.backup_reminder_snoozed(1_000));
        notices.snooze_backup_reminder(1_000);
        let day = BACKUP_SNOOZE.as_millis() as u64;
        assert!(notices.backup_reminder_snoozed(1_000 + day - 1));
        assert!(!notices.backup_reminder_snoozed(1_000 + day));

        let dir = std::env::temp_dir().join(format!("motodesk-notices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = NoticesStore::in_dir(&dir);
        assert_eq!(store.load(), Notices::default());
        notices.till_hint_dismissed = true;
        store.save(&notices).unwrap();
        assert_eq!(NoticesStore::in_dir(&dir).load(), notices);
        let _ = std::fs::remove_dir_all(dir);
    }
}
