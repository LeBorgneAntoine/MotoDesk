//! The screens. One module per screen of `desktop/src/routes`, migrated one at a time.
//!
//! A screen imports its controls from `motodesk_ui` and nothing else: no colour written by
//! hand, no rounded corner, no GPUI Kit type (see `tests/ui_rules.rs`).

pub mod atelier;
pub mod auth;
pub mod clients;
pub mod dashboard;
pub mod employees;
pub mod lines;
pub mod parc;
pub mod sales;
pub mod stock;
pub mod vehicle;

use motodesk_ui::gpui::*;
use motodesk_ui::*;

/// What the shell shows for a module whose screen has not been migrated yet.
pub fn not_migrated(module: &str, cx: &App) -> impl IntoElement + use<> {
    let c = cx.ui().colors;
    col().size_full().bg(c.bg_surface).child(PageToolbar::new(module.to_string())).child(
        StateView::empty()
            .icon(IconName::Construction)
            .title("Écran pas encore migré")
            .body("Cet écran existe dans l'interface actuelle de Motodesk. Il arrivera ici lors d'une prochaine étape de la migration."),
    )
}
