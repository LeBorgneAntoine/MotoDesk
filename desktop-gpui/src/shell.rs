//! The frame around every screen once signed in (port of `Shell.svelte`): title bar,
//! navigation rail, the screen itself, status bar.

use std::time::Duration;

use chrono::Datelike as _;
use motodesk_client::settings;
use motodesk_client::sync::{self, SyncStatus};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::appearance;
use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::pages::clients::ClientsPage;
use crate::pages::not_migrated;
use crate::router::{self, Router};

/// How often the status bar asks the service where the sync stands.
const SYNC_POLL: Duration = Duration::from_secs(60);

/// (id, label, icon, administrators only), in the order of the rail.
const MODULES: [(&str, &str, IconName, bool); 13] = [
    ("dashboard", "Tableau de bord", IconName::LayoutDashboard, false),
    ("assistant", "Assistant", IconName::Sparkles, false),
    ("clients", "Clients & Véhicules", IconName::Users, false),
    ("atelier", "L'Atelier", IconName::Wrench, false),
    ("parc", "Parc VN/VO", IconName::Car, false),
    ("stock", "Inventaire", IconName::Package, false),
    ("sales", "Ventes", IconName::ShoppingCart, false),
    ("billing", "Comptabilité", IconName::Scale, false),
    ("templates", "Modèles", IconName::LayoutTemplate, false),
    ("shared", "Dossier partagé", IconName::Folder, false),
    ("employees", "Employés", IconName::IdCard, true),
    ("data", "Données", IconName::Database, true),
    ("settings", "Paramètres", IconName::Settings, false),
];

pub struct Shell {
    clients: Option<Entity<ClientsPage>>,
    sync: Option<SyncStatus>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&Router::global(cx), |_, _, cx| cx.notify()),
            cx.observe(&AuthStore::global(cx), |_, _, cx| cx.notify()),
        ];
        cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |this, cx| this.refresh_sync(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(SYNC_POLL).await;
            }
        })
        .detach();
        // The look the shop chose, from the same settings as the Svelte interface.
        cx.api(
            |client| async move { settings::list(&client).await },
            |_, settings, cx| {
                let Ok(settings) = settings else { return };
                let system_is_dark = matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
                let look = appearance::from_settings(&settings, system_is_dark);
                UiTheme::update(cx, |theme| {
                    theme.mode = look.mode;
                    theme.accent = look.accent;
                    theme.density = look.density;
                });
            },
        );
        Self { clients: None, sync: None, _subscriptions: subscriptions }
    }

    fn refresh_sync(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { sync::status(&client).await },
            |this, status, cx| {
                this.sync = status.ok();
                cx.notify();
            },
        );
    }

    /// (colour of the dot, label) of the sync state.
    fn sync_state(&self, cx: &App) -> (Hsla, String) {
        let c = cx.ui().colors;
        if AuthStore::global(cx).read(cx).is_local() {
            let peers = self.sync.as_ref().map(|s| s.lan_peer_count).unwrap_or(0);
            let label = if peers > 0 { format!("Mode local · {peers} appareil(s) LAN") } else { "Mode local".to_string() };
            return (c.state_ok, label);
        }
        match &self.sync {
            None => (c.state_idle, "API locale…".into()),
            Some(status) if !status.cloud_reachable => (c.state_idle, "Cloud hors ligne".into()),
            Some(status) => {
                let mut parts = vec!["Synchronisé".to_string()];
                if status.lan_peer_count > 0 {
                    parts.push(format!("{} appareil(s) LAN", status.lan_peer_count));
                }
                if status.pending_cloud_ops > 0 {
                    parts.push(format!("{} en attente", status.pending_cloud_ops));
                    return (c.state_syncing, parts.join(" · "));
                }
                (c.state_ok, parts.join(" · "))
            }
        }
    }
}

/// "mardi 25 novembre 2026".
fn today() -> String {
    const DAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
    const MONTHS: [&str; 12] = [
        "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
        "décembre",
    ];
    let now = chrono::Local::now();
    let day = DAYS[now.weekday().num_days_from_monday() as usize];
    let month = MONTHS[now.month0() as usize];
    let number = if now.day() == 1 { "1er".to_string() } else { now.day().to_string() };
    format!("{day} {number} {month} {}", now.year())
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let (is_admin, user_name) = {
            let auth = AuthStore::global(cx).read(cx);
            (auth.is_admin(), auth.user.as_ref().map(|u| if u.full_name.is_empty() { u.email.clone() } else { u.full_name.clone() }))
        };
        let mut current = Router::global(cx).read(cx).current().clone();
        // A module the account may not open, or one that does not exist.
        let allowed = MODULES.iter().any(|(id, _, _, admin_only)| *id == current.as_ref() && (!admin_only || is_admin));
        if !allowed {
            current = router::HOME.into();
        }
        let title = MODULES.iter().find(|(id, ..)| *id == current.as_ref()).map(|(_, label, ..)| *label).unwrap_or("Motodesk");

        let items: Vec<SidebarItem> = MODULES
            .iter()
            .filter(|(_, _, _, admin_only)| !admin_only || is_admin)
            .map(|(id, label, icon, _)| SidebarItem::new(*id, *label, *icon))
            .collect();

        let screen: AnyElement = match current.as_ref() {
            router::CLIENTS => {
                let page = self.clients.get_or_insert_with(|| cx.new(|cx| ClientsPage::new(window, cx))).clone();
                page.into_any_element()
            }
            _ => not_migrated(title, cx).into_any_element(),
        };

        let (dot, sync_label) = self.sync_state(cx);

        col()
            .size_full()
            .bg(c.bg_app)
            .text_color(c.text)
            .font_family(FONT_UI)
            .fs(m.fs_md)
            .child(
                TitleBar::new()
                    .child(Icon::new(IconName::Bike).color(c.accent))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Motodesk"))
                    .child(div().text_color(c.text_muted).child(title))
                    .when_some(user_name, |bar, name| {
                        bar.trailing(row().gap(m.space_2).text_color(c.text_muted).child(Icon::new(IconName::CircleUser)).child(name))
                    })
                    .trailing(IconButton::new("logout", IconName::LogOut).title("Se déconnecter").size(IconButtonSize::Md).on_click(
                        |_, _, cx| AuthStore::global(cx).update(cx, |auth, cx| auth.logout(cx)),
                    )),
            )
            .child(
                row()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(Sidebar::new(items, current).on_select(|id, _, cx| {
                        let id = id.clone();
                        Router::global(cx).update(cx, |router, cx| router.go(id, cx))
                    }))
                    .child(div().flex_1().min_w_0().h_full().child(screen)),
            )
            .child(
                StatusBar::new()
                    .left(StatusBarButton::new("sync-state", sync_label).dot(dot))
                    .left(div().opacity(0.72).child(today()))
                    .right(div().opacity(0.72).child(concat!("v", env!("CARGO_PKG_VERSION")))),
            )
    }
}
