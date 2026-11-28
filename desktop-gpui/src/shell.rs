//! The frame around every screen once signed in: title bar, navigation rail, the
//! screen itself, status bar.

use std::time::Duration;

use chrono::Datelike as _;
use motodesk_client::backup::{self, Reminder};
use motodesk_client::fiscal::{self, ProfileCheck};
use motodesk_client::network::{self, MeshStatus};
use motodesk_client::recovery;
use motodesk_client::settings;
use motodesk_client::sync::{self, SyncStatus};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::appearance;
use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};
use crate::notices::NoticesState;
use crate::notifications::{self, Action, Notification};
use crate::pages::atelier::AtelierPage;
use crate::pages::billing::BillingPage;
use crate::pages::data::DataPage;
use crate::pages::settings::SettingsPage;
use crate::pages::assistant::AssistantPage;
use crate::pages::shared::SharedPage;
use crate::pages::clients::ClientsPage;
use crate::pages::dashboard::{DashboardPage, Module};
use crate::pages::employees;
use crate::pages::parc::ParcPage;
use crate::pages::sales::SalesPage;
use crate::pages::stock::StockPage;
use crate::pages::templates::TemplatesPage;
use crate::pages::not_migrated;
use crate::pages::updates as update_screens;
use crate::pos::PosState;
use crate::router::{self, Router};
use crate::updates::UpdateState;

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
    atelier: Option<Entity<AtelierPage>>,
    parc: Option<Entity<ParcPage>>,
    stock: Option<Entity<StockPage>>,
    sales: Option<Entity<SalesPage>>,
    billing: Option<Entity<BillingPage>>,
    settings: Option<Entity<SettingsPage>>,
    data: Option<Entity<DataPage>>,
    shared: Option<Entity<SharedPage>>,
    assistant: Option<Entity<AssistantPage>>,
    templates: Option<Entity<TemplatesPage>>,
    /// The team of an online company, or the accounts of a local network.
    employees: Option<AnyView>,
    sync: Option<SyncStatus>,
    /// Where this PC stands among the PCs of the shop (local networks only).
    mesh: MeshStatus,
    reconnect: Option<Reconnect>,
    // What the shop must be told, in the notification centre (see `notices`).
    /// What an invoice issued now would be missing.
    profile: Option<ProfileCheck>,
    /// Past days with trading and no daily closure, oldest first. Closing is manual and
    /// mandatory, and nothing else reminds anyone to do it.
    late_closures: Vec<String>,
    /// Changes the cloud holds from this PC that this base lacks (a restored backup).
    restore_gap: i64,
    recovering_gap: bool,
    backup_note: Option<Reminder>,
    /// A local network without a recovery key written down.
    recovery_missing: bool,
    /// The screen the notices were last read for: they are read again when it changes.
    noticed_for: SharedString,
    /// The menus of the screen shown, as last drawn: what a pressed shortcut is looked up in.
    menus: Vec<MenuGroup>,
    /// The page whose menus are shown is watched: what its menus can do changes with it
    /// (a customer opened, a document sealed), not only with the screen.
    watched_page: SharedString,
    _page_watch: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

/// « Une journée d'activité n'est pas clôturée (la plus ancienne : 20/11/2026). … »
fn late_closures_message(days: &[String]) -> String {
    let count = match days.len() {
        1 => "Une journée d'activité n'est pas clôturée".to_string(),
        many => format!("{many} journées d'activité ne sont pas clôturées"),
    };
    let oldest = days.first().map(|day| format::date(day)).unwrap_or_default();
    format!("{count} (la plus ancienne : {oldest}). La clôture journalière est obligatoire et se fait dans l'ordre.")
}

impl Shell {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&Router::global(cx), |_, _, cx| cx.notify()),
            cx.observe(&AuthStore::global(cx), |_, _, cx| cx.notify()),
            cx.observe(&UpdateState::global(cx), |_, _, cx| cx.notify()),
            cx.observe(&NoticesState::global(cx), |this, _, cx| {
                this.refresh_notices(cx);
                cx.notify();
            }),
            cx.observe(&PosState::global(cx), |_, _, cx| cx.notify()),
            // The bell shows what the centre holds, whoever put it there.
            cx.observe(&notifications::center(cx), |_, _, cx| cx.notify()),
            // Everything a notice is made of ends in this frame being notified.
            cx.observe_self(|this, cx| this.publish_notices(cx)),
            // A key pressed anywhere on the screen runs the menu entry that advertises it. In a
            // text field, copy, paste and undo stay the field's.
            cx.observe_keystrokes(|this, event, window, cx| {
                let Some(pressed) = pressed_shortcut(&event.keystroke, typing_in_a_field(window)) else { return };
                let menus = this.menus.clone();
                // Run once this shell is no longer being told about the key: an entry may
                // change the screen, which the shell follows.
                window.defer(cx, move |window, cx| {
                    MenuGroup::run_shortcut(&menus, &pressed, window, cx);
                });
            }),
        ];
        // What a previous session was told is not this one's.
        notifications::publish("shell", Vec::new(), cx);
        // Someone is signed in: from now on the app looks for a newer version by itself.
        UpdateState::global(cx).update(cx, |updates, cx| updates.start(cx));
        cx.spawn(async move |this, cx| {
            loop {
                let alive = this.update(cx, |this, cx| {
                    this.refresh_sync(cx);
                    this.refresh_mesh(cx);
                    this.refresh_restore_gap(cx);
                });
                if alive.is_err() {
                    break;
                }
                cx.background_executor().timer(SYNC_POLL).await;
            }
        })
        .detach();
        // The look the shop chose, from its settings.
        cx.api(
            |client| async move { settings::list(&client).await },
            |_, settings, cx| {
                if let Ok(settings) = settings {
                    appearance::apply(&settings, cx);
                }
            },
        );
        Self {
            clients: None,
            dashboard: None,
            atelier: None,
            parc: None,
            stock: None,
            sales: None,
            billing: None,
            settings: None,
            data: None,
            shared: None,
            assistant: None,
            templates: None,
            employees: None,
            sync: None,
            mesh: MeshStatus::default(),
            reconnect: None,
            profile: None,
            late_closures: Vec::new(),
            restore_gap: 0,
            recovering_gap: false,
            backup_note: None,
            recovery_missing: false,
            noticed_for: SharedString::default(),
            menus: Vec::new(),
            watched_page: SharedString::default(),
            _page_watch: None,
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

    // ---- notices --------------------------------------------------------------------------------

    /// Read again what the notices are made of. Called when the screen changes: a profile is
    /// completed in the settings, a day is closed in Comptabilité, a key is noted in Employés,
    /// and the notice should be gone by the time one leaves.
    fn refresh_notices(&mut self, cx: &mut Context<Self>) {
        let (is_admin, is_local) = {
            let auth = AuthStore::global(cx).read(cx);
            (auth.is_admin(), auth.is_local())
        };
        cx.api(
            |client| async move { fiscal::check_profile(&client).await },
            |this, profile, cx| {
                if let Ok(profile) = profile {
                    this.profile = Some(profile);
                    cx.notify();
                }
            },
        );
        if !is_admin {
            self.late_closures.clear();
            self.backup_note = None;
            self.recovery_missing = false;
            return;
        }
        cx.api(
            |client| async move { fiscal::pending_closures(&client).await },
            |this, days, cx| {
                this.late_closures = days.unwrap_or_default();
                cx.notify();
            },
        );
        self.refresh_backup_note(cx);
        if !is_local {
            self.recovery_missing = false;
            return;
        }
        cx.api(
            |client| async move { recovery::status(&client).await },
            |this, status, cx| {
                this.recovery_missing = status.is_ok_and(|status| !status.exists || !status.confirmed);
                cx.notify();
            },
        );
    }

    /// The shop's data is only as safe as its last backup: say so, once a day, to those who
    /// can do something about it.
    fn refresh_backup_note(&mut self, cx: &mut Context<Self>) {
        if !AuthStore::global(cx).read(cx).is_admin() || NoticesState::global(cx).read(cx).backup_reminder_snoozed() {
            self.backup_note = None;
            return;
        }
        cx.api(
            |client| async move { backup::status(&client).await },
            |this, status, cx| {
                this.backup_note = status.ok().and_then(|status| backup::reminder(&status, chrono::Utc::now()));
                cx.notify();
            },
        );
    }

    fn refresh_restore_gap(&mut self, cx: &mut Context<Self>) {
        if !AuthStore::global(cx).read(cx).is_admin() {
            self.restore_gap = 0;
            return;
        }
        cx.api(
            |client| async move { sync::restore_gap(&client).await },
            |this, missing, cx| {
                this.restore_gap = missing.unwrap_or(0);
                cx.notify();
            },
        );
    }

    /// Fetch this PC's own changes back from the cloud and rebuild the register from them.
    fn recover_gap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recovering_gap {
            return;
        }
        self.recovering_gap = true;
        cx.notify();
        cx.api_in(
            window,
            |client| async move { sync::recover_from_cloud(&client).await },
            |this, result, window, cx| {
                this.recovering_gap = false;
                match result {
                    Ok(recovered) => {
                        this.restore_gap = 0;
                        toast::success(recovered.message(), window, cx);
                    }
                    Err(error) => toast::error(humanize_error(&error, "La récupération a échoué."), window, cx),
                }
                cx.notify();
            },
        );
    }

    /// Go on without recovering: the notice is not shown again for this gap.
    fn dismiss_gap(&mut self, cx: &mut Context<Self>) {
        self.restore_gap = 0;
        cx.notify();
        cx.api(|client| async move { sync::dismiss_restore_gap(&client).await }, |_, _, _| {});
    }

    /// What the shop is told by this frame: the conditions it watches, as notifications.
    fn notices(&self, cx: &mut Context<Self>) -> Vec<Notification> {
        let is_admin = AuthStore::global(cx).read(cx).is_admin();
        let go = |label: &'static str, screen: &'static str, parameter: Option<(&'static str, &'static str)>| {
            Action::new(label, move |_, cx| {
                Router::global(cx).update(cx, |router, cx| match parameter {
                    Some(parameter) => router.go_with(screen, [parameter], cx),
                    None => router.go(screen, cx),
                })
            })
        };
        let shell = cx.entity().downgrade();
        let mut notices = Vec::new();

        // An invoice cannot be validated without its mandatory mentions: said before anyone
        // has typed a whole document. Otherwise, once, that this PC could be a till.
        let till = PosState::global(cx).read(cx).config.enabled;
        let till_hint_dismissed = NoticesState::global(cx).read(cx).notices.till_hint_dismissed;
        if let Some(blocker) = self.profile.as_ref().and_then(ProfileCheck::blocker) {
            notices.push(
                Notification::new("fiscal.profile", Status::Warning, "Profil fiscal incomplet")
                    .body(format!("{blocker}. Complétez Réglages › Légal & fiscal avant de valider une facture."))
                    .action(go("Ouvrir les réglages", router::SETTINGS, Some(("cat", "legal")))),
            );
        } else if !till && !till_hint_dismissed {
            notices.push(
                Notification::new("till.hint", Status::Info, "Caisse")
                    .body("Cet ordinateur n'est pas encore une caisse. Activez-la dans Réglages › Caisse si vous encaissez au comptoir.")
                    .action(go("Configurer", router::SETTINGS, Some(("cat", "caisse"))))
                    .action(Action::new("Plus tard", |_, cx| NoticesState::global(cx).update(cx, |notices, cx| notices.dismiss_till_hint(cx))).keeps_open()),
            );
        }
        // A network without a recovery key written down is one forgotten password from being stuck.
        let key_on_screen = AuthStore::global(cx).read(cx).recovery_key.is_some();
        if self.recovery_missing && !key_on_screen {
            notices.push(
                Notification::new("network.recovery-key", Status::Warning, "Clé de récupération à noter")
                    .body("Sans elle, un mot de passe administrateur perdu bloque les comptes pour de bon.")
                    .action(go("Créer la clé", router::EMPLOYEES, None)),
            );
        }
        let (behind, other) = (self.mesh.behind_count, self.mesh.other_version_count);
        if self.mesh.needs_reconnect {
            let shell = shell.clone();
            notices.push(
                Notification::new("network.reconnect", Status::Danger, "Ce poste n'échange plus avec le réseau")
                    .body("La clé du réseau a changé (un poste a été retiré) ou ce poste n'a plus la sienne.")
                    .action(
                        Action::new("Reconnecter ce poste", move |window, cx| {
                            let _ = shell.update(cx, |this, cx| this.start_reconnect(window, cx));
                        })
                        .primary(),
                    ),
            );
        } else if behind > 0 && is_admin {
            notices.push(Notification::new("network.behind", Status::Info, "Postes à reconnecter").body(format!(
                "{} encore l'ancienne clé du réseau : ouvrez-y « Reconnecter ce poste ». Un poste que vous avez retiré peut aussi apparaître ici.",
                if behind > 1 { format!("{behind} postes ont") } else { "Un poste a".to_string() }
            )));
        }
        if other > 0 {
            notices.push(Notification::new("network.versions", Status::Warning, "Versions différentes sur le réseau").body(format!(
                "{} une autre version de Motodesk : rien n'est échangé avec {}. Mettez tous les postes à jour.",
                if other > 1 { format!("{other} postes utilisent") } else { "Un autre poste utilise".to_string() },
                if other > 1 { "eux" } else { "lui" }
            )));
        }
        // A newer version, for those who can install it. « Plus tard » brings it back tomorrow.
        let waiting = UpdateState::global(cx).read(cx).notice().map(|update| update.version.clone());
        if let Some(version) = waiting.filter(|_| is_admin) {
            notices.push(
                Notification::new("update.available", Status::Info, format!("Nouvelle version disponible : Motodesk {version}"))
                    .action(Action::new("Voir les nouveautés", update_screens::open_dialog).primary())
                    .action(Action::new("Plus tard", |_, cx| UpdateState::global(cx).update(cx, |updates, cx| updates.later(cx))).keeps_open()),
            );
        }
        if let Some(note) = self.backup_note {
            notices.push(
                Notification::new("backup.reminder", Status::Warning, note.title())
                    .body(note.message())
                    .action(go("Ouvrir les sauvegardes", router::SETTINGS, Some(("cat", "system"))).primary())
                    .action(Action::new("Demain", |_, cx| NoticesState::global(cx).update(cx, |notices, cx| notices.snooze_backup_reminder(cx))).keeps_open()),
            );
        }
        if self.restore_gap > 0 {
            let (recover, skip) = (shell.clone(), shell.clone());
            notices.push(
                Notification::new("sync.restore-gap", Status::Danger, "Base plus ancienne que le cloud")
                    .body(format!(
                        "Le cloud conserve au moins {} modification(s) de ce poste qui ne sont pas dans cette base (une sauvegarde plus ancienne \
                         a-t-elle été restaurée ?). Tant qu'elles ne sont pas récupérées, aucun document ne peut être émis : il bifurquerait du \
                         registre fiscal conservé dans le cloud.",
                        self.restore_gap
                    ))
                    .action(
                        Action::new("Récupérer depuis le cloud", move |window, cx| {
                            let _ = recover.update(cx, |this, cx| this.recover_gap(window, cx));
                        })
                        .primary()
                        .busy(self.recovering_gap)
                        .keeps_open(),
                    )
                    .action(
                        Action::new("Continuer sans récupérer", move |_, cx| {
                            let _ = skip.update(cx, |this, cx| this.dismiss_gap(cx));
                        })
                        .keeps_open(),
                    ),
            );
        }
        if !self.late_closures.is_empty() && is_admin {
            notices.push(
                Notification::new("fiscal.closures", Status::Warning, "Clôtures à faire")
                    .body(late_closures_message(&self.late_closures))
                    .action(go("Clôturer", router::BILLING, Some(("section", "closures")))),
            );
        }
        notices
    }

    /// Hand the centre what holds now. Run whenever this frame is told something changed.
    fn publish_notices(&mut self, cx: &mut Context<Self>) {
        let notices = self.notices(cx);
        notifications::publish("shell", notices, cx);
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
        if current.as_ref() != router::ATELIER {
            self.atelier = None;
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
        if current.as_ref() != router::BILLING {
            self.billing = None;
        }
        if current.as_ref() != router::SETTINGS {
            self.settings = None;
        }
        if current.as_ref() != router::DATA {
            self.data = None;
        }
        if current.as_ref() != router::SHARED {
            self.shared = None;
        }
        if current.as_ref() != router::ASSISTANT {
            self.assistant = None;
        }
        if current.as_ref() != router::TEMPLATES {
            self.templates = None;
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
            router::ATELIER => {
                let page = self.atelier.get_or_insert_with(|| cx.new(|cx| AtelierPage::new(window, cx))).clone();
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
            router::BILLING => {
                let page = self.billing.get_or_insert_with(|| cx.new(|cx| BillingPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::SHARED => {
                let page = self.shared.get_or_insert_with(|| cx.new(|cx| SharedPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::TEMPLATES => {
                let page = self.templates.get_or_insert_with(|| cx.new(|cx| TemplatesPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::ASSISTANT => {
                let page = self.assistant.get_or_insert_with(|| cx.new(|cx| AssistantPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::DATA => {
                let page = self.data.get_or_insert_with(|| cx.new(|cx| DataPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::SETTINGS => {
                let page = self.settings.get_or_insert_with(|| cx.new(|cx| SettingsPage::new(window, cx))).clone();
                page.into_any_element()
            }
            router::EMPLOYEES => {
                let page = self.employees.get_or_insert_with(|| employees::open(window, cx)).clone();
                page.into_any_element()
            }
            _ => not_migrated(title, cx).into_any_element(),
        };

        let (dot, sync_label) = self.sync_state(cx);
        // The menus of the screen shown, with what they can do right now.
        let rewatch = self.watched_page != current;
        if rewatch {
            self.watched_page = current.clone();
            self._page_watch = None;
        }
        // The menus of one page, read now; the page is watched from the moment it is shown.
        macro_rules! menus_of {
            ($page:ident) => {
                match self.$page.clone() {
                    Some(page) => {
                        if rewatch {
                            self._page_watch = Some(cx.observe(&page, |_, _, cx| cx.notify()));
                        }
                        page.update(cx, |page, cx| page.menus(cx))
                    }
                    None => Vec::new(),
                }
            };
        }
        let menus = match current.as_ref() {
            router::DASHBOARD => menus_of!(dashboard),
            router::CLIENTS => menus_of!(clients),
            router::ATELIER => menus_of!(atelier),
            router::PARC => menus_of!(parc),
            router::SALES => menus_of!(sales),
            router::STOCK => menus_of!(stock),
            router::TEMPLATES => menus_of!(templates),
            router::ASSISTANT => menus_of!(assistant),
            _ => Vec::new(),
        };
        self.menus = menus.clone();

        // The notices follow the screen: read again when it changes.
        if self.noticed_for != current {
            self.noticed_for = current.clone();
            self.refresh_notices(cx);
        }

        col()
            .size_full()
            .bg(c.bg_app)
            .text_color(c.text)
            .font_family(FONT_UI)
            .fs(m.fs_md)
            .child(
                TitleBar::new()
                    .child(Logo::new())
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Motodesk"))
                    .child(div().text_color(c.text_muted).child(title))
                    .when(!menus.is_empty(), |bar| bar.child(div().ml(m.space_2).child(MenuStrip::new(menus))))
                    .trailing(notifications::bell(cx))
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

#[cfg(test)]
mod tests {
    #[test]
    fn late_closures_are_counted_and_the_oldest_named() {
        let one = super::late_closures_message(&["2026-11-20".to_string()]);
        assert!(one.starts_with("Une journée d'activité n'est pas clôturée (la plus ancienne : 20/11/2026)."), "{one}");
        let three = super::late_closures_message(&["2026-11-18".to_string(), "2026-11-19".to_string(), "2026-11-20".to_string()]);
        assert!(three.starts_with("3 journées d'activité ne sont pas clôturées (la plus ancienne : 18/11/2026)."), "{three}");
    }
}
