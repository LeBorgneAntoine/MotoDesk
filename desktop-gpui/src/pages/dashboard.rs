//! Tableau de bord: the screen the app opens on. Four figures, a way into every module,
//! and the documents touched last.

use std::rc::Rc;

use motodesk_client::dashboard::{self, DashboardStats, RecentActivity};
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::menus;

use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};
use crate::router::{self, Router};

const LOAD_ERROR: &str = "Impossible de charger le tableau de bord.";

/// Under this width the tiles go two by two instead of four by four.
const NARROW: Pixels = px(1100.);

/// A module of the navigation rail: where its tile leads.
#[derive(Clone)]
pub struct Module {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: IconName,
}

/// The line under a module's name.
fn description(module: &str) -> Option<&'static str> {
    Some(match module {
        "clients" => "Recherche clients et parc véhicules.",
        "atelier" => "Kanban atelier — En attente, En cours, Prêt, Facturé.",
        "stock" => "Inventaire, réceptions et mouvements.",
        "sales" => "Devis, factures, avoirs et tickets.",
        "billing" => "Journaux, TVA, FEC et clôtures.",
        "templates" => "Modèles d'impression.",
        "employees" => "Comptes et invitations.",
        "data" => "Import CSV et gestion des tables.",
        "settings" => "Entreprise, fiscal, caisse et sauvegardes.",
        _ => return None,
    })
}

/// "+3", "-12"; nothing when the figure did not move or the service does not say.
fn count_trend(delta: Option<i64>) -> Option<(String, Trend)> {
    match delta? {
        0 => None,
        n if n > 0 => Some((format!("+{n}"), Trend::Up)),
        n => Some((n.to_string(), Trend::Down)),
    }
}

/// "+8,2 %", "-3 %": one decimal at most, written the French way.
fn percent_trend(delta: Option<f64>) -> Option<(String, Trend)> {
    let delta = delta.filter(|d| d.is_finite() && *d != 0.)?;
    let rounded = format!("{:.1}", delta.abs());
    let rounded = rounded.strip_suffix(".0").unwrap_or(&rounded).replace('.', ",");
    let (sign, direction) = if delta > 0. { ("+", Trend::Up) } else { ("-", Trend::Down) };
    // Rounded down to nothing: the direction is still said, the sign would be a lie.
    let sign = if rounded == "0" { "" } else { sign };
    Some((format!("{sign}{rounded} %"), direction))
}

/// The colour of a status chip, from the words of the status.
fn status_tone(status: &str) -> Status {
    let status = status.to_lowercase();
    if status.contains("attente") {
        Status::Warning
    } else if status.contains("cours") {
        Status::Info
    } else if status.contains("prêt") || status.contains("pret") {
        Status::Success
    } else {
        Status::Muted
    }
}

fn greeting(full_name: Option<&str>) -> String {
    match full_name.and_then(|name| name.split_whitespace().next()) {
        Some(first) => format!("Bonjour, {first} — voici un aperçu de votre activité."),
        None => "Bonjour — voici un aperçu de votre activité.".to_string(),
    }
}

pub struct DashboardPage {
    /// Every module the signed-in person may open, the dashboard itself left out.
    modules: Vec<Module>,
    stats: Option<DashboardStats>,
    activity: Rc<Vec<RecentActivity>>,
    loading: bool,
    load_error: Option<SharedString>,
}

impl DashboardPage {
    pub fn new(modules: Vec<Module>, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            modules: modules.into_iter().filter(|module| module.id.as_ref() != router::DASHBOARD).collect(),
            stats: None,
            activity: Rc::new(Vec::new()),
            loading: true,
            load_error: None,
        };
        page.refresh(cx);
        page
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.load_error = None;
        cx.notify();
        cx.api(
            |client| async move {
                let stats = dashboard::stats(&client).await?;
                let recent = dashboard::recent(&client, 10).await?;
                Ok::<_, motodesk_client::ApiError>((stats, recent))
            },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok((stats, recent)) => {
                        this.stats = Some(stats);
                        this.activity = Rc::new(recent);
                    }
                    Err(error) => this.load_error = Some(humanize_error(&error, LOAD_ERROR).into()),
                }
                cx.notify();
            },
        );
    }

    fn kpis(&self, columns: u16, cx: &App) -> Div {
        let m = cx.ui().m;
        let stats = self.stats.as_ref();
        let figure = |value: Option<String>| value.unwrap_or_else(|| "—".to_string());
        let with_trend = |tile: KpiTile, trend: Option<(String, Trend)>| match trend {
            Some((label, direction)) => tile.trend(label, direction),
            None => tile,
        };
        div()
            .grid()
            .grid_cols(columns)
            .gap(m.space_3)
            .child(with_trend(
                KpiTile::new("Clients actifs", figure(stats.map(|s| format::integer(s.active_clients))))
                    .icon(IconName::Users)
                    .status(Status::Accent)
                    .loading(self.loading),
                count_trend(stats.and_then(|s| s.active_clients_delta)),
            ))
            .child(with_trend(
                KpiTile::new("Véhicules en atelier", figure(stats.map(|s| format::integer(s.vehicles_in_workshop))))
                    .icon(IconName::Wrench)
                    .loading(self.loading),
                count_trend(stats.and_then(|s| s.vehicles_in_workshop_delta)),
            ))
            .child(with_trend(
                KpiTile::new("Stock disponible", figure(stats.map(|s| format::integer(s.stock_available))))
                    .icon(IconName::Package)
                    .loading(self.loading),
                count_trend(stats.and_then(|s| s.stock_available_delta)),
            ))
            .child(with_trend(
                KpiTile::new("CA du mois", figure(stats.map(|s| format::euros_whole(s.revenue_this_month_cents))))
                    .icon(IconName::Banknote)
                    .status(Status::Success)
                    .loading(self.loading),
                percent_trend(stats.and_then(|s| s.revenue_this_month_delta_pct)),
            ))
    }

    fn modules(&self, columns: u16) -> SectionCard {
        let tiles = self.modules.iter().enumerate().map(|(index, module)| {
            let id = module.id.clone();
            let tile = TileButton::new(("module", index), module.label.clone())
                .icon(module.icon)
                .divided(index % columns as usize != 0)
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    Router::global(cx).update(cx, |router, cx| router.go(id, cx))
                });
            match description(module.id.as_ref()) {
                Some(line) => tile.description(line),
                None => tile,
            }
        });
        SectionCard::new()
            .title("Modules")
            .icon(IconName::LayoutDashboard)
            .flush()
            .child(div().grid().grid_cols(columns).children(tiles))
    }

    fn activity(&self, cx: &App) -> SectionCard {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let card = SectionCard::new().title("Activité récente").icon(IconName::Activity).flush();
        if self.loading {
            return card.child(div().h(px(180.)).child(StateView::loading().title("Chargement de l'activité…")));
        }
        if self.activity.is_empty() {
            return card.child(
                div().h(px(220.)).child(
                    StateView::empty()
                        .icon(IconName::Inbox)
                        .title("Aucune activité récente")
                        .body("Les devis, factures, tickets et ordres d'atelier apparaissent ici."),
                ),
            );
        }

        let (rows, for_click) = (self.activity.clone(), self.activity.clone());
        let muted = move |text: String| div().one_line().text_color(c.text_muted).child(text).into_any_element();
        let or_dash = |text: &Option<String>| text.clone().unwrap_or_else(|| "—".to_string());
        card.action(Button::new("activity-all").link().label("Tout voir").on_click(|_, _, cx| {
            Router::global(cx).update(cx, |router, cx| router.go("billing", cx))
        }))
        .child(
            DataTable::new(
                "recent-activity",
                vec![
                    Column::new("N°").fixed(px(120.)).mono(),
                    Column::new("Type").fixed(px(120.)),
                    Column::new("Client").flex(1.),
                    Column::new("Véhicule").flex(1.),
                    Column::new("Statut").fixed(px(130.)),
                    Column::new("Date").fixed(px(160.)).right(),
                ],
                rows.len(),
                move |ix, column, _, _| {
                    let line = &rows[ix];
                    match column {
                        0 => div().one_line().child(line.doc_number.clone()).into_any_element(),
                        1 => muted(line.doc_type.clone()),
                        2 => div().one_line().child(or_dash(&line.client_name)).into_any_element(),
                        3 => muted(or_dash(&line.vehicle_summary)),
                        4 => Tag::new(dashboard::status_label(&line.status).to_string())
                            .status(status_tone(&line.status))
                            .into_any_element(),
                        _ => muted(format::date_time(&line.created_at)),
                    }
                },
            )
            .compact()
            .auto_height()
            .on_row_click(move |ix, _, cx| {
                let Some(line) = for_click.get(*ix) else { return };
                let panel = format!("sale:{}", line.id);
                Router::global(cx).update(cx, |router, cx| router.go_with("sales", [("panel", panel.as_str())], cx));
            }),
        )
        // The card's own padding is off (the table goes edge to edge): keep the last row off
        // the border.
        .child(div().h(m.space_1))
    }
}

impl Render for DashboardPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let user = AuthStore::global(cx).read(cx).user.as_ref().map(|user| user.full_name.clone());
        let columns = if window.viewport_size().width < NARROW { 2 } else { 4 };

        let body: AnyElement = match self.load_error.clone() {
            Some(error) => StateView::error()
                .title(LOAD_ERROR)
                .body(error)
                .action("Réessayer", cx.listener(|this, _, _, cx| this.refresh(cx)))
                .into_any_element(),
            None => div()
                .id("dashboard-body")
                .size_full()
                .overflow_y_scroll()
                .child(
                    col()
                        .py(m.space_4)
                        .px(m.space_5)
                        .gap(m.space_4)
                        .child(self.kpis(columns, cx))
                        .child(self.modules(columns))
                        .child(self.activity(cx)),
                )
                .into_any_element(),
        };

        col()
            .size_full()
            .bg(c.bg_app)
            .child(
                PageToolbar::new("Tableau de bord")
                    .icon(IconName::LayoutDashboard)
                    .subtitle(greeting(user.as_deref()))
                    .trailing(
                        Button::new("dashboard-refresh")
                            .icon(IconName::RefreshCw)
                            .label("Actualiser")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}

// ---- menus ----------------------------------------------------------------------------------

impl DashboardPage {
    /// The menus of the title bar while this screen is shown.
    pub fn menus(&self, cx: &mut Context<Self>) -> Vec<MenuGroup> {
        let page = cx.entity();
        let may_import = crate::auth::AuthStore::global(cx).read(cx).is_admin();
        let mut file = vec![
            MenuEntry::new("Nouveau devis…", menus::go_panel(router::SALES, "sale:new")).icon(IconName::FileText).shortcut("Ctrl+N"),
            MenuEntry::new("Nouvel ordre de réparation", menus::go_panel(router::ATELIER, "order:new")).icon(IconName::Wrench).shortcut("Ctrl+Maj+R"),
            MenuEntry::new("Nouveau client…", menus::go_panel(router::CLIENTS, "client:new")).icon(IconName::Users).shortcut("Ctrl+Maj+C"),
        ];
        if may_import {
            file.push(MenuEntry::separator());
            file.push(MenuEntry::new("Importer CSV…", menus::go(router::DATA)).icon(IconName::Upload));
        }
        vec![
            MenuGroup::new("Fichier", file),
            MenuGroup::new(
                "Affichage",
                vec![
                    menus::to_dashboard(),
                    menus::to_workshop(),
                    menus::to_stock(),
                    MenuEntry::separator(),
                    MenuEntry::new("Actualiser", menus::on(&page, |this, _, cx| this.refresh(cx))).icon(IconName::RefreshCw).shortcut("F5"),
                ],
            ),
            menus::help(),
        ]
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use super::{Status, Trend, count_trend, description, greeting, percent_trend, status_tone};

    #[test]
    fn a_figure_that_moved_says_which_way() {
        assert_eq!(count_trend(Some(3)), Some(("+3".to_string(), Trend::Up)));
        assert_eq!(count_trend(Some(-12)), Some(("-12".to_string(), Trend::Down)));
        assert_eq!(count_trend(Some(0)), None);
        assert_eq!(count_trend(None), None);
    }

    #[test]
    fn a_percentage_is_written_the_french_way_with_one_decimal_at_most() {
        assert_eq!(percent_trend(Some(8.2)), Some(("+8,2 %".to_string(), Trend::Up)));
        assert_eq!(percent_trend(Some(8.0)), Some(("+8 %".to_string(), Trend::Up)));
        assert_eq!(percent_trend(Some(-3.26)), Some(("-3,3 %".to_string(), Trend::Down)));
        assert_eq!(percent_trend(Some(-0.01)), Some(("0 %".to_string(), Trend::Down)));
        assert_eq!(percent_trend(Some(0.)), None);
        assert_eq!(percent_trend(Some(f64::NAN)), None);
        assert_eq!(percent_trend(None), None);
    }

    #[test]
    fn the_greeting_uses_the_first_name_when_there_is_one() {
        assert_eq!(greeting(Some("Léa Martin")), "Bonjour, Léa — voici un aperçu de votre activité.");
        assert_eq!(greeting(Some("  demo ")), "Bonjour, demo — voici un aperçu de votre activité.");
        assert_eq!(greeting(Some("")), "Bonjour — voici un aperçu de votre activité.");
        assert_eq!(greeting(None), "Bonjour — voici un aperçu de votre activité.");
    }

    #[test]
    fn a_status_chip_takes_its_colour_from_the_words_of_the_status() {
        assert_eq!(status_tone("En attente"), Status::Warning);
        assert_eq!(status_tone("en cours"), Status::Info);
        assert_eq!(status_tone("Prêt"), Status::Success);
        assert_eq!(status_tone("paid"), Status::Muted);
    }

    #[test]
    fn the_modules_people_open_most_say_what_is_in_them() {
        assert!(description("clients").is_some_and(|line| line.contains("clients")));
        assert_eq!(description("assistant"), None);
    }
}
