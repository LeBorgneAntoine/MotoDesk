//! The accounts and the PCs of a local network.
//!
//! Three things an administrator looks after here: who can sign in on the shop's PCs, the
//! recovery key that saves the network if the administrator's password is lost, and the PCs
//! themselves, one of which can be shut out if it is lost or stolen.

use std::rc::Rc;

use motodesk_client::employees::Role;
use motodesk_client::local_account::validate_new_local_account;
use motodesk_client::network::{self, NetworkDevice, RemoveDevice};
use motodesk_client::network_accounts::{self, NetworkAccount, NetworkAccountPatch, NewNetworkAccount};
use motodesk_client::recovery::{self, RecoveryStatus};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};

/// Which account the form on screen is for.
enum Target {
    Create,
    /// A new password for this account.
    Password(NetworkAccount),
}

struct AccountForm {
    target: Target,
    username: TextModel,
    full_name: TextModel,
    role: SelectModel,
    password: TextModel,
    confirm: TextModel,
    saving: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

/// The administrator's password, asked before a new recovery key is made.
struct KeyForm {
    password: TextModel,
    saving: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

/// A PC about to be shut out of the network.
struct Removal {
    device: NetworkDevice,
    password: TextModel,
    new_password: TextModel,
    confirm: TextModel,
    saving: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

pub struct LocalAccounts {
    accounts: Rc<Vec<NetworkAccount>>,
    loading: bool,
    load_error: Option<SharedString>,
    form: Option<AccountForm>,

    recovery: Option<RecoveryStatus>,
    key_form: Option<KeyForm>,
    /// A new key is on screen, waiting to be written down.
    key_shown: bool,

    devices: Vec<NetworkDevice>,
    removal: Option<Removal>,
    /// The PC just removed: the others must now be reconnected.
    removed_name: Option<SharedString>,

    _subscriptions: Vec<Subscription>,
}

/// Enter in any of these fields sends the dialog they belong to.
fn on_enter(
    fields: &[&TextModel],
    window: &mut Window,
    cx: &mut Context<LocalAccounts>,
    submit: fn(&mut LocalAccounts, &mut Window, &mut Context<LocalAccounts>),
) -> Vec<Subscription> {
    fields
        .iter()
        .map(|field| {
            field.on_event(window, cx, move |this, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    submit(this, window, cx);
                }
            })
        })
        .collect()
}

fn secret(window: &mut Window, cx: &mut App) -> TextModel {
    TextModel::builder().password().build(window, cx)
}

impl LocalAccounts {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&AuthStore::global(cx), |this, auth, cx| {
            // The key that was on screen has been written down: its card can say so.
            let shown = auth.read(cx).recovery_key.is_some();
            if this.key_shown && !shown {
                this.load_recovery(cx);
            }
            this.key_shown = shown;
            cx.notify();
        })];
        let mut page = Self {
            accounts: Rc::new(Vec::new()),
            loading: true,
            load_error: None,
            form: None,
            recovery: None,
            key_form: None,
            key_shown: false,
            devices: Vec::new(),
            removal: None,
            removed_name: None,
            _subscriptions: subscriptions,
        };
        page.load(cx);
        page.load_recovery(cx);
        page.load_devices(cx);
        page
    }

    // ---- accounts ---------------------------------------------------------------------------

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.load_error = None;
        cx.notify();
        cx.api(
            |client| async move { network_accounts::list(&client).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(accounts) => this.accounts = Rc::new(accounts),
                    Err(error) => this.load_error = Some(humanize_error(&error, "Impossible de charger les comptes.").into()),
                }
                cx.notify();
            },
        );
    }

    fn open_form(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let (username, full_name) = (TextModel::new(window, cx), TextModel::new(window, cx));
        let (password, confirm) = (secret(window, cx), secret(window, cx));
        let role = SelectModel::new(
            vec![SelectOption::new("employee", "Employé"), SelectOption::new("admin", "Administrateur")],
            Some("employee"),
            window,
            cx,
        );
        let creating = matches!(target, Target::Create);
        let title = if creating { "Nouveau compte" } else { "Changer le mot de passe" };
        let first = if creating { username.clone() } else { password.clone() };
        let subscriptions = on_enter(&[&username, &full_name, &password, &confirm], window, cx, Self::submit_form);
        self.form = Some(AccountForm {
            target,
            username,
            full_name,
            role,
            password,
            confirm,
            saving: false,
            error: None,
            _subscriptions: subscriptions,
        });

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new(title).open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let Some(form) = for_body.read(cx).form.as_ref() else { return element(div()) };
                let body = col().gap(m.space_4).children(form.error.clone().map(|e| Alert::danger().child(e)));
                let body = match &form.target {
                    Target::Create => body
                        .child(
                            FormField::new("Identifiant")
                                .required()
                                .hint("Il le saisira pour se connecter.")
                                .child(Input::new(&form.username)),
                        )
                        .child(FormField::new("Nom affiché").child(Input::new(&form.full_name)))
                        .child(FormField::new("Rôle").child(Select::new(&form.role))),
                    Target::Password(account) => {
                        body.child(format!("Nouveau mot de passe de {}.", account.username))
                    }
                };
                element(
                    body.child(
                        FormField::new("Mot de passe")
                            .required()
                            .hint("Minimum 8 caractères.")
                            .child(Input::new(&form.password).reveal_toggle()),
                    )
                    .child(FormField::new("Confirmer le mot de passe").required().child(Input::new(&form.confirm))),
                )
            },
            move |_, cx| {
                let saving = for_footer.read(cx).form.as_ref().is_some_and(|form| form.saving);
                let page = for_footer.clone();
                vec![
                    element(Button::new("account-cancel").label("Annuler").on_click(|_, window, cx| close_modal(window, cx))),
                    element(Button::new("account-save").primary().label("Enregistrer").loading(saving).on_click(
                        move |_, window, cx| page.update(cx, |this, cx| this.submit_form(window, cx)),
                    )),
                ]
            },
        );
        first.focus(window, cx);
    }

    fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else { return };
        if form.saving {
            return;
        }
        let (password, confirm) = (form.password.text(cx), form.confirm.text(cx));
        let username = form.username.text(cx);
        // A password change has no username to check: any valid one stands in for it.
        let checked_name = if matches!(form.target, Target::Create) { username.as_str() } else { "xxx" };
        if let Some(problem) = validate_new_local_account(checked_name, &password, &confirm) {
            form.error = Some(problem.into());
            return cx.notify();
        }
        let creating = match &form.target {
            Target::Create => None,
            Target::Password(account) => Some(account.id.clone()),
        };
        let new_account = NewNetworkAccount {
            username: username.trim().to_string(),
            password: password.clone(),
            confirm: confirm.clone(),
            role: Role::from_value(form.role.value(cx).as_deref().unwrap_or("employee")),
            full_name: form.full_name.trimmed(cx),
        };
        form.saving = true;
        form.error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                match creating {
                    None => network_accounts::create(&client, &new_account).await.map(|_| true),
                    Some(id) => {
                        let patch =
                            NetworkAccountPatch { password: Some(password), confirm: Some(confirm), ..Default::default() };
                        network_accounts::update(&client, &id, &patch).await.map(|_| false)
                    }
                }
            },
            |this, result, window, cx| {
                match result {
                    Ok(created) => {
                        this.form = None;
                        close_modal(window, cx);
                        toast::success(
                            if created {
                                "Compte créé : il peut se connecter depuis n'importe quel poste du réseau."
                            } else {
                                "Mot de passe changé."
                            },
                            window,
                            cx,
                        );
                        this.load(cx);
                    }
                    Err(error) => {
                        if let Some(form) = this.form.as_mut() {
                            form.saving = false;
                            form.error = Some(humanize_error(&error, "Enregistrement impossible.").into());
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    fn toggle_active(&mut self, account: &NetworkAccount, window: &mut Window, cx: &mut Context<Self>) {
        let (id, patch) =
            (account.id.clone(), NetworkAccountPatch { is_active: Some(!account.is_active), ..Default::default() });
        cx.api_in(
            window,
            move |client| async move { network_accounts::update(&client, &id, &patch).await },
            |this, result, window, cx| match result {
                Ok(_) => this.load(cx),
                Err(error) => toast::error(humanize_error(&error, "Modification impossible."), window, cx),
            },
        );
    }

    fn ask_delete(&mut self, account: &NetworkAccount, window: &mut Window, cx: &mut Context<Self>) {
        let (page, id) = (cx.entity(), account.id.clone());
        Confirm::new("Supprimer ce compte ?", format!("{} ne pourra plus se connecter au réseau.", account.username))
            .confirm_label("Supprimer")
            .danger()
            .open(window, cx, move |window, cx| {
                let id = id.clone();
                page.update(cx, |_, cx| {
                    cx.api_in(
                        window,
                        move |client| async move { network_accounts::delete(&client, &id).await },
                        |this, result, window, cx| match result {
                            Ok(()) => this.load(cx),
                            Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
                        },
                    )
                })
            });
    }

    // ---- recovery key -----------------------------------------------------------------------

    fn load_recovery(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { recovery::status(&client).await },
            |this, status, cx| {
                this.recovery = status.ok();
                cx.notify();
            },
        );
    }

    fn ask_new_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = secret(window, cx);
        let subscriptions = on_enter(&[&password], window, cx, Self::create_key);
        let mut subscriptions = subscriptions;
        subscriptions.push(password.on_event(window, cx, |_, _, _, cx| cx.notify()));
        self.key_form = Some(KeyForm { password: password.clone(), saving: false, error: None, _subscriptions: subscriptions });

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Créer une nouvelle clé").open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let Some(form) = for_body.read(cx).key_form.as_ref() else { return element(div()) };
                element(
                    col()
                        .gap(m.space_4)
                        .children(form.error.clone().map(|e| Alert::danger().child(e)))
                        .child(FormField::new("Votre mot de passe").required().child(Input::new(&form.password).reveal_toggle())),
                )
            },
            move |_, cx| {
                let (saving, empty) = for_footer
                    .read(cx)
                    .key_form
                    .as_ref()
                    .map(|form| (form.saving, form.password.text(cx).is_empty()))
                    .unwrap_or((false, true));
                let page = for_footer.clone();
                vec![
                    element(Button::new("key-cancel").label("Annuler").on_click(|_, window, cx| close_modal(window, cx))),
                    element(Button::new("key-create").primary().label("Créer la clé").disabled(empty).loading(saving).on_click(
                        move |_, window, cx| page.update(cx, |this, cx| this.create_key(window, cx)),
                    )),
                ]
            },
        );
        password.focus(window, cx);
    }

    fn create_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.key_form.as_mut() else { return };
        let password = form.password.text(cx);
        if form.saving || password.is_empty() {
            return;
        }
        form.saving = true;
        form.error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { recovery::issue(&client, &password).await },
            |this, result, window, cx| {
                match result {
                    Ok(issued) => {
                        this.key_form = None;
                        close_modal(window, cx);
                        // The root of the window shows the key and waits for it to be noted.
                        AuthStore::global(cx).update(cx, |auth, cx| auth.show_recovery_key(issued.recovery_key, cx));
                    }
                    Err(error) => {
                        if let Some(form) = this.key_form.as_mut() {
                            form.saving = false;
                            form.error = Some(humanize_error(&error, "Impossible de créer la clé.").into());
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    // ---- the PCs of the network -------------------------------------------------------------

    fn load_devices(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { network::list_devices(&client).await },
            |this, devices, cx| {
                this.devices = devices.unwrap_or_default();
                cx.notify();
            },
        );
    }

    fn ask_remove(&mut self, device: NetworkDevice, window: &mut Window, cx: &mut Context<Self>) {
        let (password, new_password, confirm) = (secret(window, cx), secret(window, cx), secret(window, cx));
        let subscriptions = on_enter(&[&password, &new_password, &confirm], window, cx, Self::submit_removal);
        let title = format!("Retirer {}", if device.name.is_empty() { "ce poste" } else { device.name.as_str() });
        self.removal = Some(Removal {
            device,
            password: password.clone(),
            new_password,
            confirm,
            saving: false,
            error: None,
            _subscriptions: subscriptions,
        });

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new(title).width(px(520.)).keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let Some(form) = for_body.read(cx).removal.as_ref() else { return element(div()) };
                element(
                    col()
                        .gap(m.space_4)
                        .child(Alert::warning().child(
                            "La clé du réseau change : ce poste ne recevra plus rien, et chaque autre poste devra être reconnecté par un administrateur. Ce qu'il contient déjà ne peut pas être effacé à distance.",
                        ))
                        .children(form.error.clone().map(|e| Alert::danger().child(e)))
                        .child(FormField::new("Votre mot de passe actuel").required().child(Input::new(&form.password)))
                        .child(
                            FormField::new("Votre nouveau mot de passe")
                                .required()
                                .hint("Obligatoire : le poste retiré connaît l'empreinte de l'ancien.")
                                .child(Input::new(&form.new_password)),
                        )
                        .child(FormField::new("Confirmer le nouveau mot de passe").required().child(Input::new(&form.confirm))),
                )
            },
            move |_, cx| {
                let saving = for_footer.read(cx).removal.as_ref().is_some_and(|form| form.saving);
                let page = for_footer.clone();
                vec![
                    element(Button::new("removal-cancel").label("Annuler").disabled(saving).on_click(|_, window, cx| close_modal(window, cx))),
                    element(Button::new("removal-confirm").danger().label("Retirer le poste").loading(saving).on_click(
                        move |_, window, cx| page.update(cx, |this, cx| this.submit_removal(window, cx)),
                    )),
                ]
            },
        );
        password.focus(window, cx);
    }

    fn submit_removal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.removal.as_mut() else { return };
        if form.saving {
            return;
        }
        let payload = RemoveDevice {
            password: form.password.text(cx),
            new_password: form.new_password.text(cx),
            confirm: form.confirm.text(cx),
        };
        let problem = if payload.password.is_empty() {
            Some("Saisissez votre mot de passe actuel.".to_string())
        } else {
            validate_new_local_account("xxx", &payload.new_password, &payload.confirm)
        };
        if let Some(problem) = problem {
            form.error = Some(problem.into());
            return cx.notify();
        }
        let (id, name) = (form.device.device_id.clone(), form.device.name.clone());
        form.saving = true;
        form.error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { network::remove_device(&client, &id, &payload).await },
            move |this, result, window, cx| {
                match result {
                    Ok(()) => {
                        this.removal = None;
                        this.removed_name = Some(name.into());
                        close_modal(window, cx);
                        toast::success("Poste retiré. Reconnectez maintenant les autres postes.", window, cx);
                        this.load_devices(cx);
                    }
                    Err(error) => {
                        if let Some(form) = this.removal.as_mut() {
                            form.saving = false;
                            form.error = Some(humanize_error(&error, "Impossible de retirer ce poste.").into());
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    // ---- drawing ----------------------------------------------------------------------------

    fn recovery_card(&self, cx: &mut Context<Self>) -> Option<Alert> {
        let status = self.recovery.as_ref()?;
        let (card, text) = if !status.exists {
            (
                Alert::warning(),
                "Aucune clé n'existe : si le mot de passe administrateur est perdu, personne ne pourra plus gérer les comptes.".to_string(),
            )
        } else if !status.confirmed {
            (Alert::warning(), "La clé actuelle n'a pas été confirmée : créez-en une nouvelle et notez-la.".to_string())
        } else {
            (
                Alert::info(),
                format!(
                    "Créée ou confirmée le {}. En créer une nouvelle rend l'ancienne inutilisable.",
                    format::date_time(status.updated_at.as_deref().unwrap_or_default())
                ),
            )
        };
        Some(card.title("Clé de récupération").child(text).child(row().child(
            Button::new("key-new").link().small().label("Créer une nouvelle clé").on_click(
                cx.listener(|this, _, window, cx| this.ask_new_key(window, cx)),
            ),
        )))
    }

    fn accounts_table(&self, me: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if let Some(error) = self.load_error.clone() {
            return div()
                .h(px(240.))
                .child(
                    StateView::error()
                        .title("Comptes indisponibles")
                        .body(error)
                        .action("Réessayer", cx.listener(|this, _, _, cx| this.load(cx))),
                )
                .into_any_element();
        }
        if self.loading && self.accounts.is_empty() {
            return div().h(px(240.)).child(StateView::loading().title("Chargement des comptes…")).into_any_element();
        }
        if self.accounts.is_empty() {
            return div().fs(m.fs_sm).text_color(c.text_muted).child("Aucun compte.").into_any_element();
        }

        let (rows, for_muted) = (self.accounts.clone(), self.accounts.clone());
        let page = cx.entity();
        let table = DataTable::new(
            "network-accounts",
            vec![Column::new("Compte").flex(2.), Column::new("Rôle").fixed(px(200.)), Column::new("").fixed(px(120.)).right()],
            rows.len(),
            move |ix, column, _, _| {
                let account = &rows[ix];
                let mine = me.as_deref() == Some(account.id.as_str());
                match column {
                    0 => col()
                        .min_w_0()
                        .gap(px(1.))
                        .child(div().one_line().font_weight(FontWeight::SEMIBOLD).child(account.display_name().to_string()))
                        .child(div().one_line().fs_tight(m.fs_xs).text_color(c.text_muted).child(format!(
                            "{}{}",
                            account.username,
                            if mine { " · vous" } else { "" }
                        )))
                        .into_any_element(),
                    1 => row()
                        .gap(m.space_1)
                        .child(if account.is_admin() {
                            Tag::new("Admin").status(Status::Accent).icon(IconName::Shield)
                        } else {
                            Tag::new("Employé").status(Status::Muted)
                        })
                        .when(!account.is_active, |this| this.child(Tag::new("Désactivé").status(Status::Muted)))
                        .into_any_element(),
                    _ => {
                        let (for_password, for_toggle, for_delete) = (page.clone(), page.clone(), page.clone());
                        let (a, b, d) = (account.clone(), account.clone(), account.clone());
                        row()
                            .gap(m.space_1)
                            .child(IconButton::new(("account-password", ix), IconName::Pencil).title("Changer le mot de passe").on_click(
                                move |_, window, cx| {
                                    let account = a.clone();
                                    for_password.update(cx, |this, cx| this.open_form(Target::Password(account), window, cx))
                                },
                            ))
                            // An administrator does not switch off or delete their own account.
                            .when(!mine, |this| {
                                this.child(
                                    IconButton::new(
                                        ("account-toggle", ix),
                                        if account.is_active { IconName::LogOut } else { IconName::CircleCheck },
                                    )
                                    .title(if account.is_active { "Désactiver" } else { "Réactiver" })
                                    .on_click(move |_, window, cx| {
                                        for_toggle.update(cx, |this, cx| this.toggle_active(&b, window, cx))
                                    }),
                                )
                                .child(
                                    IconButton::new(("account-delete", ix), IconName::Trash).title("Supprimer").danger().on_click(
                                        move |_, window, cx| for_delete.update(cx, |this, cx| this.ask_delete(&d, window, cx)),
                                    ),
                                )
                            })
                            .into_any_element()
                    }
                }
            },
        )
        .auto_height()
        .row_height(px(48.))
        .muted_rows(move |row| for_muted.get(row).is_some_and(|account| !account.is_active));
        div().border_1().border_color(c.border).child(table).into_any_element()
    }

    fn devices_card(&self, cx: &mut Context<Self>) -> Option<SectionCard> {
        if self.devices.is_empty() {
            return None;
        }
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let last = self.devices.len() - 1;
        let card = SectionCard::new()
            .title("Postes du réseau")
            .subtitle("Retirez un poste perdu ou volé : il ne recevra plus rien.")
            .icon(IconName::Database)
            .flush();
        let card = match self.removed_name.clone() {
            Some(name) => card.child(div().p(m.space_4).child(Alert::warning().title(format!("{name} a été retiré")).child(
                "Sur chaque autre poste, un administrateur doit cliquer « Reconnecter ce poste » avec le nouveau mot de passe. Le poste retiré garde les données qu'il avait et les empreintes des mots de passe : changez aussi ceux des autres comptes.",
            ))),
            None => card,
        };
        Some(card.children(self.devices.iter().enumerate().map(|(i, device)| {
            let to_remove = device.clone();
            let seen = if device.this_pc {
                "Ce poste".to_string()
            } else {
                format!("Vu le {}", format::date_time(&device.last_seen))
            };
            row()
                .justify_between()
                .gap(m.space_3)
                .py(m.space_2)
                .px(m.space_4)
                .when(i != last, |this| this.hairline_bottom(c.border_subtle))
                .child(
                    col()
                        .min_w_0()
                        .gap(px(1.))
                        .child(div().one_line().font_weight(FontWeight::SEMIBOLD).child(device.name.clone()))
                        .child(div().fs_tight(m.fs_xs).text_color(c.text_muted).child(seen)),
                )
                .when(device.removed_at.is_some(), |this| this.child(Tag::new("Retiré").status(Status::Muted)))
                .when(device.removed_at.is_none() && !device.this_pc, |this| {
                    this.child(Button::new(("device-remove", i)).link().small().label("Retirer ce poste").on_click(
                        cx.listener(move |this, _, window, cx| this.ask_remove(to_remove.clone(), window, cx)),
                    ))
                })
        })))
    }
}

impl Render for LocalAccounts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let me = AuthStore::global(cx).read(cx).user.as_ref().map(|user| user.id.clone());
        let loaded = self.load_error.is_none() && !(self.loading && self.accounts.is_empty());
        let total = self.accounts.len();
        let count = if !loaded {
            String::new()
        } else {
            format!("{total} compte{}", if total > 1 { "s" } else { "" })
        };

        col()
            .size_full()
            .bg(c.bg_app)
            .child(
                PageToolbar::new("Employés")
                    .icon(IconName::UsersRound)
                    .subtitle("Gérez les accès de votre équipe et leurs rôles.")
                    .meta(count)
                    .trailing(
                        Button::new("account-new").primary().icon(IconName::UserPlus).label("Nouveau compte").on_click(
                            cx.listener(|this, _, window, cx| this.open_form(Target::Create, window, cx)),
                        ),
                    ),
            )
            .child(
                div().id("local-accounts").flex_1().min_h_0().overflow_y_scroll().child(
                    // The whole width, like the online team: no column to leave half the window empty.
                    col()
                        .p(m.space_5)
                        .gap(m.space_4)
                        .child(div().fs(m.fs_sm).text_color(c.text_muted).child(
                            "Ces comptes ouvrent une session sur tous les postes du réseau local de l'atelier. Seul un administrateur peut ajouter un nouveau poste, tant qu'un poste du réseau est allumé.",
                        ))
                        .children(self.recovery_card(cx))
                        .child(self.accounts_table(me, cx))
                        .when(loaded, |this| {
                            this.child(Alert::info().child(
                                "Désactiver ou supprimer un compte l'empêche de se connecter. Un poste qui a déjà rejoint le réseau garde néanmoins une copie des données : retirez-le ci-dessous s'il est perdu ou volé.",
                            ))
                        })
                        .children(if loaded { self.devices_card(cx) } else { None }),
                ),
            )
    }
}
