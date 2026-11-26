//! The menus of the title bar.
//!
//! Each screen says which menus it offers (`menus()` on its page); the shell shows them in the
//! title bar and runs the entry whose keys are pressed. What is here is what the screens share:
//! how an entry reaches its page, the entries that go from one screen to another, and « Aide ».

use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::router::{self, Router};
use crate::updates::VERSION;

/// An entry that runs `run` on `page`. Nothing happens once the page is gone.
pub fn on<V: 'static>(page: &Entity<V>, run: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static) -> impl Fn(&mut Window, &mut App) + 'static {
    let page = page.downgrade();
    move |window, cx| {
        let _ = page.update(cx, |page, cx| run(page, window, cx));
    }
}

/// An entry that goes to another screen.
pub fn go(screen: &'static str) -> impl Fn(&mut Window, &mut App) + 'static {
    move |_, cx| Router::global(cx).update(cx, |router, cx| router.go(screen, cx))
}

/// An entry that goes to another screen and opens one of its panels there (`sale:new`).
pub fn go_panel(screen: &'static str, panel: &'static str) -> impl Fn(&mut Window, &mut App) + 'static {
    move |_, cx| Router::global(cx).update(cx, |router, cx| router.go_with(screen, [("panel", panel)], cx))
}

/// « Tableau de bord » and « L'Atelier », with their keys: the same from every screen that
/// lists them.
pub fn to_dashboard() -> MenuEntry {
    MenuEntry::new("Tableau de bord", go(router::DASHBOARD)).icon(IconName::LayoutDashboard).shortcut("Ctrl+1")
}

pub fn to_workshop() -> MenuEntry {
    MenuEntry::new("L'Atelier", go(router::ATELIER)).icon(IconName::Wrench).shortcut("Ctrl+2")
}

pub fn to_stock() -> MenuEntry {
    MenuEntry::new("Inventaire", go(router::STOCK)).icon(IconName::Package).shortcut("Ctrl+3")
}

const DOCUMENTATION: &str = "Motodesk est le système d'exploitation du magasin : clients, atelier, stock, ventes et comptabilité fiscale.\n\n\
- **Ventes** — devis, factures, avoirs. Une validation scelle le document.\n\
- **Caisse** — à activer par machine dans Réglages › Caisse.\n\
- **Atelier** — colonnes En attente / En cours / Prêt / Facturé.\n\
- **Comptabilité** — journaux, TVA, FEC, clôtures. La chaîne ISCA commence au premier document validé dans Motodesk.\n\
- **Sauvegardes** — Réglages › Sauvegardes pour exporter la boutique vers un nouveau PC.\n\n\
L'historique Motogest importé reste consultable ; il n'entre pas dans le journal fiscal de Motodesk.";

const SHORTCUTS: &str = "- **Ctrl+N** — nouveau devis\n\
- **Ctrl+Maj+R** — nouvel ordre d'atelier\n\
- **Ctrl+Maj+C** — nouveau client\n\
- **Ctrl+1** — tableau de bord · **Ctrl+2** atelier · **Ctrl+3** stock\n\
- **F5** — actualiser · **Échap** — fermer le panneau\n\n\
Chaque écran rappelle ses propres raccourcis dans ses menus, en haut de la fenêtre.";

/// What « À propos » says.
fn about() -> String {
    format!(
        "Motodesk {VERSION} — application de gestion pour ateliers moto et cycle, hors ligne d'abord, synchro cloud et réseau local.\n\n\
         Les ventes validées sont chaînées et signées (ISCA). Motogest historique reste l'archive légale des années antérieures à la bascule."
    )
}

/// One of the help windows: a title and a few lines.
fn open_help(title: &'static str, text: String, window: &mut Window, cx: &mut App) {
    Modal::new(title).width(px(560.)).open(
        window,
        cx,
        move |_, _| element(Prose::new("help-text", text.clone()).compact()),
        |_, _| vec![element(Button::new("help-close").primary().label("Fermer").on_click(|_, window, cx| close_modal(window, cx)))],
    );
}

/// « Aide »: what Motodesk is, its shortcuts, and which version runs.
pub fn help() -> MenuGroup {
    MenuGroup::new(
        "Aide",
        vec![
            MenuEntry::new("Documentation", |window, cx| open_help("Documentation", DOCUMENTATION.to_string(), window, cx)).icon(IconName::FileText),
            MenuEntry::new("Raccourcis clavier", |window, cx| open_help("Raccourcis clavier", SHORTCUTS.to_string(), window, cx))
                .icon(IconName::Keyboard)
                .shortcut("Ctrl+/"),
            MenuEntry::separator(),
            MenuEntry::new("À propos de Motodesk…", |window, cx| open_help("À propos de Motodesk", about(), window, cx)).icon(IconName::Info),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{SHORTCUTS, about};

    #[test]
    fn the_help_names_the_version_and_the_keys() {
        assert!(about().starts_with(&format!("Motodesk {} — ", env!("CARGO_PKG_VERSION"))));
        for keys in ["Ctrl+N", "Ctrl+Maj+R", "Ctrl+Maj+C", "Ctrl+1", "F5"] {
            assert!(SHORTCUTS.contains(keys), "{keys}");
        }
    }
}
