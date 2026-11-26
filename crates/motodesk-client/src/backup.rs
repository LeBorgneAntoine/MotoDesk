//! Backups: one encrypted file holds the database, the fiscal archives and the signing keys,
//! so a new PC can pick up where this one stopped. Administrators only.

use axum::http::Method;
use serde::Deserialize;

use crate::error::ApiResult;
use crate::session::Client;

/// The shortest password the service accepts for a backup.
pub const MIN_PASSWORD_CHARS: usize = 10;

/// Why a password cannot protect a backup, or `None` when it can.
pub fn password_problem(password: &str) -> Option<String> {
    (password.chars().count() < MIN_PASSWORD_CHARS)
        .then(|| format!("Le mot de passe doit faire au moins {MIN_PASSWORD_CHARS} caractères."))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BackupFile {
    pub filename: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size_bytes: i64,
    #[serde(default)]
    pub created_at: String,
    /// `automatic`, `manual`, or `legacy` (an old ZIP from a previous version).
    #[serde(default)]
    pub kind: String,
}

impl BackupFile {
    pub fn kind_label(&self) -> &'static str {
        match self.kind.as_str() {
            "automatic" => "Automatique",
            "legacy" => "Ancien format",
            _ => "Manuelle",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct BackupStatus {
    pub backups_dir: String,
    pub last_backup_at: Option<String>,
    pub auto_enabled: bool,
    pub frequency_days: i64,
    /// Automatic backups write nothing until the owner has chosen a password.
    pub auto_password_set: bool,
    /// A restore is queued and replaces the data at the next start.
    pub restore_pending: bool,
    pub files: Vec<BackupFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct RestoreResult {
    pub restart_required: bool,
    /// Where the data being replaced is kept.
    pub safety_copy_note: String,
}

pub async fn status(client: &Client) -> ApiResult<BackupStatus> {
    client.get("/api/backup/status").await
}

/// Write a backup now, protected by `password`.
pub async fn export(client: &Client, password: &str) -> ApiResult<BackupFile> {
    client.post("/api/backup/export", &serde_json::json!({ "password": password })).await
}

/// Queue the restore of a backup of this PC: the data is replaced at the next start.
pub async fn restore(client: &Client, filename: &str, password: &str) -> ApiResult<RestoreResult> {
    client.post("/api/backup/restore", &serde_json::json!({ "filename": filename, "password": password })).await
}

/// On a PC with nothing set up: bring the shop back from the backup file at `path`, without
/// first creating a throwaway network. Refused once the PC is set up. The accounts that come
/// back are the backup's; the data is in place at the next start.
pub async fn restore_on_first_run(client: &Client, path: &str, password: &str) -> ApiResult<RestoreResult> {
    client.call(Method::POST, "/api/auth/local/restore").public().json(&serde_json::json!({ "path": path, "password": password })).send().await
}

/// Choose the password of the automatic backups. It is kept in the system's keychain.
pub async fn set_auto_password(client: &Client, password: &str) -> ApiResult<()> {
    client.call(Method::PUT, "/api/backup/auto-password").json(&serde_json::json!({ "password": password })).done().await
}

/// Forget that password: automatic backups stop until another is chosen.
pub async fn clear_auto_password(client: &Client) -> ApiResult<()> {
    client.call(Method::DELETE, "/api/backup/auto-password").done().await
}

// ---- the reminder ------------------------------------------------------------------------------

/// Without automatic backups, how old the last one may be before the owner is reminded.
pub const MANUAL_GRACE_DAYS: i64 = 7;
/// With automatic backups, how many days past the chosen frequency count as stuck.
pub const AUTOMATIC_SLACK_DAYS: i64 = 3;

/// Why the owner is told that the shop's data is not being backed up. Automatic backups write
/// nothing until a password has been chosen, so a shop can run for months believing it is
/// protected when it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reminder {
    /// No automatic backup is set up, and the last backup of any kind is old or missing
    /// (`days` is `None` when there has never been one).
    NoAutomatic { days: Option<i64> },
    /// Automatic backups are set up but none has been written for a long time.
    Stale { days: i64 },
}

impl Reminder {
    /// What the banner says.
    pub fn message(self) -> String {
        match self {
            Reminder::NoAutomatic { days: None } => "Aucune sauvegarde n'a jamais été faite et aucune sauvegarde automatique n'est active : un incident sur ce poste ferait perdre les données de l'atelier. Choisissez un mot de passe de sauvegarde pour les activer.".into(),
            Reminder::NoAutomatic { days: Some(days) } => format!("La dernière sauvegarde date de {days} jours et aucune sauvegarde automatique n'est active. Choisissez un mot de passe de sauvegarde pour les activer."),
            Reminder::Stale { days } => format!("La dernière sauvegarde automatique date de {days} jours : elles semblent s'être arrêtées. Vérifiez Réglages › Sauvegardes."),
        }
    }
}

/// Whole days since `last_backup_at`; `None` when there is none or it cannot be read.
fn age_in_days(last_backup_at: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
    let when = chrono::DateTime::parse_from_rfc3339(last_backup_at?.trim()).ok()?;
    Some((now - when.with_timezone(&chrono::Utc)).num_days().max(0))
}

/// Whether, and why, to remind the owner about backups now.
pub fn reminder(status: &BackupStatus, now: chrono::DateTime<chrono::Utc>) -> Option<Reminder> {
    let days = age_in_days(status.last_backup_at.as_deref(), now);
    if !(status.auto_enabled && status.auto_password_set) {
        return days.is_none_or(|days| days >= MANUAL_GRACE_DAYS).then_some(Reminder::NoAutomatic { days });
    }
    // The first automatic backup comes a couple of minutes after the app starts.
    let days = days?;
    (days > status.frequency_days.max(1) + AUTOMATIC_SLACK_DAYS).then_some(Reminder::Stale { days })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(auto_enabled: bool, auto_password_set: bool, days_ago: Option<i64>) -> (BackupStatus, chrono::DateTime<chrono::Utc>) {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-22T12:00:00Z").unwrap().with_timezone(&chrono::Utc);
        let last_backup_at = days_ago.map(|days| (now - chrono::Duration::days(days)).to_rfc3339());
        (BackupStatus { last_backup_at, auto_enabled, auto_password_set, frequency_days: 7, ..Default::default() }, now)
    }

    fn reminded(auto_enabled: bool, auto_password_set: bool, days_ago: Option<i64>) -> Option<Reminder> {
        let (status, now) = status(auto_enabled, auto_password_set, days_ago);
        reminder(&status, now)
    }

    #[test]
    fn the_owner_is_reminded_when_nothing_protects_the_data() {
        // Nothing was ever backed up and nothing is automatic: ask for a password.
        assert_eq!(reminded(true, false, None), Some(Reminder::NoAutomatic { days: None }));
        // Quiet after a recent manual backup; back once it is a week old.
        assert_eq!(reminded(true, false, Some(2)), None);
        assert_eq!(reminded(true, false, Some(7)), Some(Reminder::NoAutomatic { days: Some(7) }));
        // Automatic backups switched off count as missing.
        assert_eq!(reminded(false, true, None), Some(Reminder::NoAutomatic { days: None }));
        // With automatic backups: wait for the first one, stay quiet while they keep up, warn when they stop.
        assert_eq!(reminded(true, true, None), None);
        assert_eq!(reminded(true, true, Some(9)), None);
        assert_eq!(reminded(true, true, Some(11)), Some(Reminder::Stale { days: 11 }));
        // A date that cannot be read is no date.
        let (mut unreadable, now) = status(true, true, None);
        unreadable.last_backup_at = Some("not a date".into());
        assert_eq!(reminder(&unreadable, now), None);
        assert!(Reminder::Stale { days: 11 }.message().contains("11 jours"));
        assert!(Reminder::NoAutomatic { days: None }.message().contains("jamais"));
    }

    #[test]
    fn a_backup_password_has_ten_characters_at_least() {
        assert!(password_problem("neuf-car.").is_some());
        assert_eq!(password_problem("dix-carac."), None);
        // Counted in characters, not bytes.
        assert_eq!(password_problem("éééééééééé"), None);
    }

    #[test]
    fn a_backup_says_how_it_was_made() {
        let file = |kind: &str| BackupFile { filename: "a.mgbak".into(), path: String::new(), size_bytes: 0, created_at: String::new(), kind: kind.into() };
        assert_eq!(file("automatic").kind_label(), "Automatique");
        assert_eq!(file("manual").kind_label(), "Manuelle");
        assert_eq!(file("legacy").kind_label(), "Ancien format");
    }
}
