//! What to tell the user when the data service cannot start.
//!
//! The window opens before the service is up, and the service is the whole application: with
//! it down every screen fails in its own confusing way. A failure at start is recorded here, and
//! the interface asks for it (`startup_failure`) and shows one clear page instead.

use std::sync::Mutex;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// The database was written by a newer Motodesk than this one.
    #[error("the database is at schema version {found}, this build knows up to {known}")]
    DatabaseTooNew { found: i64, known: i64 },
    /// Another program holds a port the service needs.
    #[error("port {port} is not available: {source}")]
    PortInUse { port: u16, source: std::io::Error },
    /// An update to the database structure failed (and was rolled back).
    #[error("database update failed: {0}")]
    Migration(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupFailure {
    /// A stable word the interface can switch on.
    pub kind: &'static str,
    pub title: String,
    pub message: String,
}

/// Shown for anything that is not one of the cases above.
fn unknown(err: &anyhow::Error) -> StartupFailure {
    StartupFailure {
        kind: "unknown",
        title: "Motodesk n'a pas pu démarrer".into(),
        message: format!(
            "Le service de données ne s'est pas lancé : {err:#}. Vos données n'ont pas été modifiées. \
             Le dossier des journaux contient le détail à transmettre à l'assistance."
        ),
    }
}

pub fn describe(err: &anyhow::Error) -> StartupFailure {
    match err.downcast_ref::<StartupError>() {
        Some(StartupError::DatabaseTooNew { .. }) => StartupFailure {
            kind: "database_too_new",
            title: "Cette base de données vient d'une version plus récente".into(),
            message: "Les données ont été ouvertes par une version plus récente de Motodesk. \
                      Installez la dernière version pour continuer : rien n'a été modifié, \
                      et revenir en arrière pourrait abîmer les données."
                .into(),
        },
        Some(StartupError::PortInUse { port, .. }) => StartupFailure {
            kind: "port_in_use",
            title: "Un autre programme bloque Motodesk".into(),
            message: format!(
                "Le port {port} est déjà utilisé. Motodesk est peut-être déjà ouvert (cherchez son \
                 icône près de l'horloge), ou un autre logiciel occupe ce port. Fermez-le puis relancez Motodesk."
            ),
        },
        Some(StartupError::Migration(why)) => StartupFailure {
            kind: "migration_failed",
            title: "La mise à jour des données a échoué".into(),
            message: format!(
                "Une copie de vos données a été faite avant la mise à jour (dossier « pre-migration » \
                 à côté de la base) et la base n'a pas été modifiée. Détail : {why}"
            ),
        },
        None => unknown(err),
    }
}

/// The failure of this run, if there was one.
#[derive(Default)]
pub struct StartupStatus(Mutex<Option<StartupFailure>>);

impl StartupStatus {
    pub fn record(&self, err: &anyhow::Error) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(describe(err));
        }
    }

    pub fn get(&self) -> Option<StartupFailure> {
        self.0.lock().ok().and_then(|slot| slot.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_known_failure_has_its_own_message() {
        let too_new: anyhow::Error = StartupError::DatabaseTooNew { found: 30, known: 25 }.into();
        assert_eq!(describe(&too_new).kind, "database_too_new");

        let port: anyhow::Error = StartupError::PortInUse {
            port: 17300,
            source: std::io::Error::from(std::io::ErrorKind::AddrInUse),
        }
        .into();
        let shown = describe(&port);
        assert_eq!(shown.kind, "port_in_use");
        assert!(shown.message.contains("17300"));

        let migration: anyhow::Error = StartupError::Migration("no such column".into()).into();
        assert!(describe(&migration).message.contains("no such column"));
    }

    #[test]
    fn an_unexpected_failure_still_says_the_data_was_not_touched() {
        let shown = describe(&anyhow::anyhow!("disk on fire"));
        assert_eq!(shown.kind, "unknown");
        assert!(shown.message.contains("disk on fire"));
    }

    #[test]
    fn the_status_remembers_the_failure() {
        let status = StartupStatus::default();
        assert!(status.get().is_none());
        status.record(&anyhow::anyhow!("boom"));
        assert_eq!(status.get().map(|f| f.kind), Some("unknown"));
    }
}
