//! Backups: one encrypted file holds the database, the fiscal archives and the signing keys,
//! so a new PC can pick up where this one stopped (port of `desktop/src/lib/api/backup.ts`).
//! Administrators only.

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

/// Choose the password of the automatic backups. It is kept in the system's keychain.
pub async fn set_auto_password(client: &Client, password: &str) -> ApiResult<()> {
    client.call(Method::PUT, "/api/backup/auto-password").json(&serde_json::json!({ "password": password })).done().await
}

/// Forget that password: automatic backups stop until another is chosen.
pub async fn clear_auto_password(client: &Client) -> ApiResult<()> {
    client.call(Method::DELETE, "/api/backup/auto-password").done().await
}

#[cfg(test)]
mod tests {
    use super::*;

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
