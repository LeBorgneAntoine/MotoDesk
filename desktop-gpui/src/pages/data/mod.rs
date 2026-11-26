//! Données (port of `routes/DataManager.svelte`): the administrator's tools on the local
//! database, one per tab. The CSV import is here; the two tabs that browse and edit the
//! tables arrive with their own migration.

mod import;

use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::router::Router;
use import::ImportWizard;

const BASIC: &str = "basic";
const ADVANCED: &str = "advanced";
const IMPORT: &str = "import";

pub struct DataPage {
    tab: SharedString,
    import: Entity<ImportWizard>,
}

impl DataPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `tab=import` opens a tab directly. Until the two others are migrated, the import
        // is the one the screen opens on.
        let asked = Router::global(cx).read(cx).param("tab").map(str::to_string);
        let tab = asked.filter(|tab| [BASIC, ADVANCED, IMPORT].contains(&tab.as_str())).unwrap_or_else(|| IMPORT.to_string());
        Self { tab: tab.into(), import: cx.new(|cx| ImportWizard::new(window, cx)) }
    }

    fn select(&mut self, tab: &SharedString, cx: &mut Context<Self>) {
        self.tab = tab.clone();
        Router::global(cx).update(cx, |router, _| router.set_param("tab", Some(tab.to_string())));
        cx.notify();
    }
}

impl Render for DataPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let tabs = Tabs::new(
            "data-tabs",
            vec![
                TabItem::new(BASIC, "Simple").icon(IconName::Table),
                TabItem::new(ADVANCED, "Avancé").icon(IconName::Braces),
                TabItem::new(IMPORT, "Import CSV").icon(IconName::Upload),
            ],
            self.tab.clone(),
        )
        .on_change(cx.listener(|this, tab: &SharedString, _, cx| this.select(tab, cx)));

        let body = if self.tab.as_ref() == IMPORT {
            self.import.clone().into_any_element()
        } else {
            StateView::empty()
                .icon(IconName::Hourglass)
                .title("Pas encore dans cette interface")
                .body("Le gestionnaire des tables arrive avec la migration de l'écran Données. L'import CSV, lui, est prêt.")
                .action("Ouvrir l'import CSV", cx.listener(|this, _, _, cx| this.select(&IMPORT.into(), cx)))
                .into_any_element()
        };

        col()
            .size_full()
            .bg(c.bg_app)
            .child(
                PageToolbar::new("Données")
                    .icon(IconName::Database)
                    .subtitle("Gestionnaire de la base locale SQLite")
                    .trailing(Tag::new("Réservé aux administrateurs").icon(IconName::Shield).medium())
                    .below(tabs),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}
