//! The frame around every screen once signed in (port of `Shell.svelte`): title bar,
//! navigation rail, the screen itself, status bar.

use std::time::Duration;

use chrono::Datelike as _;
use motodesk_client::network::{self, MeshStatus};
use motodesk_client::settings;
use motodesk_client::sync::{self, SyncStatus};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::appearance;
use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::humanize_error;
use crate::pages::clients::ClientsPage;
use crate::pages::dashboard::{DashboardPage, Module};
use crate::pages::employees;
use crate::pages::parc::ParcPage;
use crate::pages::sales::SalesPage;
use crate::pages::stock::StockPage;
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

/// The "Reconnecter ce poste" dialog: an administrator signs in here to receive the network's
/// current key. What this PC recorded meanwhile is kept.
struct Reconnect {
    username: TextModel,
    password: TextModel,
    busy: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

pub struct Shell {
    dashboard: Option<Entity<DashboardPage>>,
    clients: Option<Entity<ClientsPage>>,
    parc: Option<Entity<ParcPage>>,
    stock: Option<Entity<StockPage>>,
    sales: Option<Entity<SalesPage>>,
    /// The team of an online company, or the accounts of a local network.
    employees: Option<AnyView>,
    sync: Option<SyncStatus>,
    /// Where this PC stands among the PCs of the shop (local networks only).
    mesh: MeshStatus,
    reconnect: Option<Reconnect>,
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
                let alive = this.update(cx, |this, cx| {
                    this.refresh_sync(cx);
                    this.refresh_mesh(cx);
                });
                if alive.is_err() {
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
        Self {
            clients: None,
            dashboard: None,
            parc: None,
            stock: None,
            sales: None,
            employees: None,
            sync: None,
            mesh: MeshStatus::default(),
            reconnect: None,
            _subscriptions: subscriptions,
        }
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

    fn refresh_mesh(&mut self, cx: &mut Context<Self>) {
        // Only a local network has a key to fall behind on.
        if !AuthStore::global(cx).read(cx).is_local() {
            self.mesh = MeshStatus::default();
            return;
        }
        cx.api(
            |client| async move { network::mesh_status(&client).await },
            |this, status, cx| {
                match status {
                    Ok(status) => this.mesh = status,
                    // No answer says nothing about the key: keep that banner as it is.
                    Err(_) => this.mesh.other_version_count = 0,
                }
                cx.notify();
            },
        );
    }

    fn start_reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let username = TextModel::new(window, cx);
        let password = TextModel::builder().password().build(window, cx);
        let subscriptions = [&username, &password]
            .into_iter()
            .map(|field| {
                field.on_event(window, cx, |this, event, window, cx| {
                    if matches!(event, TextEvent::Submitted { .. }) {
                        this.submit_reconnect(window, cx);
                    }
                })
            })
            .collect();
        self.reconnect =
            Some(Reconnect { username: username.clone(), password, busy: false, error: None, _subscriptions: subscriptions });

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Reconnecter ce poste").keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let Some(form) = for_body.read(cx).reconnect.as_ref() else { return element(div()) };
                element(
                    col()
                        .gap(m.space_4)
                        .child(
                            "Un administrateur saisit son identifiant et son mot de passe actuel. Un autre poste du réseau doit être allumé. Les données de ce poste sont conservées.",
                        )
                        .children(form.error.clone().map(|e| Alert::danger().child(e)))
                        .child(FormField::new("Identifiant de l'administrateur").required().child(Input::new(&form.username)))
                        .child(FormField::new("Mot de passe").required().child(Input::new(&form.password).reveal_toggle())),
                )
            },
            move |_, cx| {
                let busy = for_footer.read(cx).reconnect.as_ref().is_some_and(|form| form.busy);
                let shell = for_footer.clone();
                vec![
                    element(Button::new("reconnect-cancel").label("Annuler").disabled(busy).on_click(|_, window, cx| close_modal(window, cx))),
                    element(Button::new("reconnect-submit").primary().label("Reconnecter").loading(busy).on_click(
                        move |_, window, cx| shell.update(cx, |this, cx| this.submit_reconnect(window, cx)),
                    )),
                ]
            },
        );
        // After the dialog has opened (it takes the focus when it does).
        username.focus(window, cx);
    }

    fn submit_reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.reconnect.as_mut() else { return };
        let (Some(username), password) = (form.username.trimmed(cx), form.password.text(cx)) else { return };
        if form.busy || password.is_empty() {
            return;
        }
        form.busy = true;
        form.error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { network::reconnect(&client, &username, &password).await },
            |this, result, window, cx| {
                match result {
                    Ok(()) => {
                        this.reconnect = None;
                        this.mesh.needs_reconnect = false;
                        close_modal(window, cx);
                        toast::success("Poste reconnecté au réseau.", window, cx);
                        this.refresh_mesh(cx);
                    }
                    Err(error) => {
                        if let Some(form) = this.reconnect.as_mut() {
                            form.busy = false;
                            form.error = Some(humanize_error(&error, "La reconnexion a échoué.").into());
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    /// What the shop must know about its network, above every screen.
    fn banners(&self, is_admin: bool, cx: &mut Context<Self>) -> Vec<Alert> {
        let m = cx.ui().m;
        let mut banners = Vec::new();
        let (behind, other) = (self.mesh.behind_count, self.mesh.other_version_count);
        if self.mesh.needs_reconnect {
            banners.push(
                Alert::warning()
                    .flush()
                    .title("Ce poste n'échange plus avec le réseau")
                    .child("La clé du réseau a changé (un poste a été retiré) ou ce poste n'a plus la sienne.")
                    .child(row().mt(m.space_1).child(
                        Button::new("reconnect-open").primary().small().label("Reconnecter ce poste").on_click(
                            cx.listener(|this, _, window, cx| this.start_reconnect(window, cx)),
                        ),
                    )),
            );
        } else if behind > 0 && is_admin {
            banners.push(Alert::info().flush().title("Postes à reconnecter").child(format!(
                "{} encore l'ancienne clé du réseau : ouvrez-y « Reconnecter ce poste ». Un poste que vous avez retiré peut aussi apparaître ici.",
                if behind > 1 { format!("{behind} postes ont") } else { "Un poste a".to_string() }
            )));
        }
        if other > 0 {
            banners.push(Alert::warning().flush().title("Versions différentes sur le réseau").child(format!(
                "{} une autre version de Motodesk : rien n'est échangé avec {}. Mettez tous les postes à jour.",
                if other > 1 { format!("{other} postes utilisent") } else { "Un autre poste utilise".to_string() },
                if other > 1 { "eux" } else { "lui" }
            )));
        }
        banners
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

        // A screen left is a screen closed: coming back starts it afresh, panels included.
        if current.as_ref() != router::DASHBOARD {
            self.dashboard = None;
        }
        if current.as_ref() != router::CLIENTS {
            self.clients = None;
        }
        if current.as_ref() != router::PARC {
            self.parc = None;
        }
        if current.as_ref() != router::STOCK {
            self.stock = None;
        }
        if current.as_ref() != router::SALES {
            self.sales = None;
        }
        if current.as_ref() != router::EMPLOYEES {
            self.employees = None;
        }

        let screen: AnyElement = match current.as_ref() {
            router::DASHBOARD => {
                let page = self
                    .dashboard
                    .get_or_insert_with(|| {
                        // The tiles lead where the rail leads: the modules this person may open.
                        let modules = MODULES
                            .iter()
                            .filter(|(_, _, _, admin_only)| !admin_only || is_admin)
                            .map(|(id, label, icon, _)| Module { id: (*id).into(), label: (*label).into(), icon: *icon })
                            .collect();
                        cx.new(|cx| DashboardPage::new(modules, cx))
                    })
                    .clone();
                page.into_any_element()
            }
            router::CLIENTS => {
                let page = self.clients.get_or_insert_with(|| cx.new(|cx| ClientsPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::PARC => {
                let page = self.parc.get_or_insert_with(|| cx.new(|cx| ParcPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::STOCK => {
                let page = self.stock.get_or_insert_with(|| cx.new(|cx| StockPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::SALES => {
                let page = self.sales.get_or_insert_with(|| cx.new(|cx| SalesPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::EMPLOYEES => {
                let page = self.employees.get_or_insert_with(|| employees::open(window, cx)).clone();
                page.into_any_element()
            }
            _ => not_migrated(title, cx).into_any_element(),
        };

        let (dot, sync_label) = self.sync_state(cx);
        let banners = self.banners(is_admin, cx);

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
                    .child(
                        col()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .children(banners)
                            .child(div().flex_1().min_h_0().child(screen)),
                    ),
            )
            .child(
                StatusBar::new()
                    .left(StatusBarButton::new("sync-state", sync_label).dot(dot))
                    .left(div().opacity(0.72).child(today()))
                    .right(div().opacity(0.72).child(concat!("v", env!("CARGO_PKG_VERSION")))),
            )
    }
}
