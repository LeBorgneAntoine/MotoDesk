//! Données (port of `routes/DataManager.svelte`): the administrator's tools on the local
//! database, one per tab. The tables one page at a time, a SQL console, and the CSV import.

mod import;
mod sql;
mod tables;

use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::router::Router;
use import::ImportWizard;
use sql::SqlConsole;
use tables::TablesTab;

const BASIC: &str = "basic";
const ADVANCED: &str = "advanced";
const IMPORT: &str = "import";

pub struct DataPage {
    tab: SharedString,
    // Each tab keeps what was done in it while another is looked at.
    tables: Entity<TablesTab>,
    sql: Entity<SqlConsole>,
    import: Entity<ImportWizard>,
}

impl DataPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `tab=import` opens a tab directly.
        let asked = Router::global(cx).read(cx).param("tab").map(str::to_string);
        let tab = asked.filter(|tab| [BASIC, ADVANCED, IMPORT].contains(&tab.as_str())).unwrap_or_else(|| BASIC.to_string());
        Self {
            tab: tab.into(),
            tables: cx.new(|cx| TablesTab::new(window, cx)),
            sql: cx.new(|cx| SqlConsole::new(window, cx)),
            import: cx.new(|cx| ImportWizard::new(window, cx)),
        }
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

        let body = match self.tab.as_ref() {
            ADVANCED => self.sql.clone().into_any_element(),
            IMPORT => self.import.clone().into_any_element(),
            _ => self.tables.clone().into_any_element(),
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
