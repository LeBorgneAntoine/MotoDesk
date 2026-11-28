//! The way into the application: sign in or sign up, online or on the local network.
//!
//! One card shows one of four forms. The tabs choose between signing in and signing up; the
//! switch under the card chooses between an online account and a local one.
//!
//! Two ways back in hang from the local sign-in: the recovery key when a password is lost,
//! and a backup file on a PC with nothing set up.

use std::path::PathBuf;

use motodesk_client::ApiResult;
use motodesk_client::auth::RegisterPayload;
use motodesk_client::backup;
use motodesk_client::local_account::{self, AccessMode, LocalState, SetupPayload};
use motodesk_client::recovery::RecoverPayload;
use motodesk_client::sync::BootstrapResult;
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::auth::{AuthStore, flows};
use crate::backend::{ApiExt as _, PickExt as _};
use crate::format::humanize_error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Login,
    Register,
}

pub struct AuthPage {
    way: Way,
    submitting: bool,
    error: Option<SharedString>,
    /// Another network was seen on the LAN while creating one: ask before making a second.
    network_seen: bool,
    create_anyway: bool,

    // Online sign-in.
    identifier: TextModel,
    password: TextModel,
    // Online sign-up.
    company: TextModel,
    invite: TextModel,
    full_name: TextModel,
    email: TextModel,
    new_password: TextModel,
    new_confirm: TextModel,
    // Local sign-in.
    username: TextModel,
    local_password: TextModel,
    // Local network creation.
    new_username: TextModel,
    new_local_password: TextModel,
    new_local_confirm: TextModel,

    // « Mot de passe perdu »: the recovery key gives one account a new password.
    recover_key: TextModel,
    recover_username: TextModel,
    recover_password: TextModel,
    recover_confirm: TextModel,
    recovering: bool,
    recover_error: Option<SharedString>,
    // « Restaurer une sauvegarde », on a PC with nothing set up.
    restore_file: Option<PathBuf>,
    restore_password: TextModel,
    restoring: bool,
    restore_error: Option<SharedString>,
    /// The backup is in place for the next start: the only step left is to restart.
    restore_queued: bool,

    _subscriptions: Vec<Subscription>,
}

/// Why the form of « Mot de passe perdu » cannot be sent, or `None` when it can.
fn recover_problem(key: &str, username: &str, password: &str, confirm: &str) -> Option<String> {
    if key.trim().is_empty() || username.trim().is_empty() {
        return Some("Saisissez la clé et l'identifiant du compte.".into());
    }
    local_account::validate_new_local_account(username, password, confirm)
}

/// Whether a chosen file can be a backup of Motodesk, by its name.
fn looks_like_a_backup(path: &std::path::Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| ["mgbak", "zip"].contains(&extension.to_lowercase().as_str()))
}

impl AuthPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let line = |window: &mut Window, cx: &mut Context<Self>| TextModel::new(window, cx);
        let secret = |window: &mut Window, cx: &mut Context<Self>| TextModel::builder().password().build(window, cx);

        let mut page = Self {
            way: Way::Login,
            submitting: false,
            error: None,
            network_seen: false,
            create_anyway: false,
            identifier: line(window, cx),
            password: secret(window, cx),
            company: line(window, cx),
            invite: line(window, cx),
            full_name: line(window, cx),
            email: line(window, cx),
            new_password: secret(window, cx),
            new_confirm: secret(window, cx),
            username: line(window, cx),
            local_password: secret(window, cx),
            new_username: line(window, cx),
            new_local_password: secret(window, cx),
            new_local_confirm: secret(window, cx),
            recover_key: TextModel::builder().placeholder("XXXXX-XXXXX-XXXXX-XXXXX-XXXXX").build(window, cx),
            recover_username: line(window, cx),
            recover_password: secret(window, cx),
            recover_confirm: secret(window, cx),
            recovering: false,
            recover_error: None,
            restore_file: None,
            restore_password: secret(window, cx),
            restoring: false,
            restore_error: None,
            restore_queued: false,
            _subscriptions: Vec::new(),
        };

        // Enter in any field sends the form on screen.
        let fields = [
            &page.identifier,
            &page.password,
            &page.company,
            &page.invite,
            &page.full_name,
            &page.email,
            &page.new_password,
            &page.new_confirm,
            &page.username,
            &page.local_password,
            &page.new_username,
            &page.new_local_password,
            &page.new_local_confirm,
        ];
        let mut subscriptions: Vec<Subscription> = fields
            .into_iter()
            .map(|field| {
                field.on_event(window, cx, |this, event, window, cx| {
                    if matches!(event, TextEvent::Submitted { .. }) {
                        this.submit(window, cx);
                    }
                })
            })
            .collect();
        // The two dialogs have their own Enter.
        for field in [&page.recover_key, &page.recover_username, &page.recover_password, &page.recover_confirm] {
            subscriptions.push(field.on_event(window, cx, |this, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.submit_recover(window, cx);
                }
            }));
        }
        subscriptions.push(page.restore_password.on_event(window, cx, |this, event, window, cx| match event {
            TextEvent::Submitted { .. } => this.submit_restore(window, cx),
            TextEvent::Changed => cx.notify(),
            _ => {}
        }));
        subscriptions.push(cx.observe(&AuthStore::global(cx), |_, _, cx| cx.notify()));
        page._subscriptions = subscriptions;
        page
    }

    fn set_way(&mut self, way: Way, cx: &mut Context<Self>) {
        self.way = way;
        self.error = None;
        cx.notify();
    }

    fn fail(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        self.submitting = false;
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitting {
            return;
        }
        let mode = AuthStore::global(cx).read(cx).access_mode;
        self.error = None;

        match (mode, self.way) {
            (AccessMode::Local, Way::Login) => {
                let (username, password) = (self.username.text(cx), self.local_password.text(cx));
                if username.trim().is_empty() || password.is_empty() {
                    return self.fail("Saisissez l'identifiant et le mot de passe.", cx);
                }
                self.send(window, cx, move |client| flows::sign_in_local(client, username, password));
            }
            (AccessMode::Local, Way::Register) => {
                let (username, password, confirm) =
                    (self.new_username.text(cx), self.new_local_password.text(cx), self.new_local_confirm.text(cx));
                if let Some(problem) = local_account::validate_new_local_account(&username, &password, &confirm) {
                    return self.fail(problem, cx);
                }
                let payload =
                    SetupPayload { username: username.trim().to_string(), password, confirm, force: self.create_anyway };
                self.send(window, cx, move |client| flows::set_up_local(client, payload));
            }
            (AccessMode::Online, Way::Login) => {
                let (identifier, password) = (self.identifier.text(cx), self.password.text(cx));
                if identifier.trim().is_empty() || password.is_empty() {
                    return self.fail("Saisissez l'email ou l'identifiant, et le mot de passe.", cx);
                }
                self.send(window, cx, move |client| flows::sign_in_online(client, identifier.trim().to_string(), password));
            }
            (AccessMode::Online, Way::Register) => {
                let (password, confirm) = (self.new_password.text(cx), self.new_confirm.text(cx));
                if password != confirm {
                    return self.fail("Les mots de passe ne correspondent pas.", cx);
                }
                if password.chars().count() < local_account::MIN_PASSWORD_CHARS {
                    return self.fail("Le mot de passe doit comporter au moins 8 caractères.", cx);
                }
                let (Some(email), Some(full_name)) = (self.email.trimmed(cx), self.full_name.trimmed(cx)) else {
                    return self.fail("Saisissez votre nom complet et votre email.", cx);
                };
                let payload = RegisterPayload {
                    email,
                    password,
                    full_name,
                    company_name: self.company.trimmed(cx),
                    invite_code: self.invite.trimmed(cx),
                };
                self.send(window, cx, move |client| flows::register_online(client, payload));
            }
        }
    }

    fn send<Fut>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(motodesk_client::Client) -> Fut + Send + 'static,
    ) where
        Fut: std::future::Future<Output = ApiResult<flows::SignedIn>> + Send + 'static,
    {
        self.submitting = true;
        cx.notify();
        cx.api_in(window, work, |this, result, window, cx| {
            this.submitting = false;
            match result {
                Ok(session) => {
                    let online = !session.bundle.user.is_local();
                    if online {
                        toast::success(BootstrapResult::message(session.bootstrap.as_ref()), window, cx);
                    }
                    for secret in [&this.password, &this.local_password, &this.new_password, &this.new_confirm, &this.new_local_password, &this.new_local_confirm] {
                        secret.clear(window, cx);
                    }
                    AuthStore::global(cx).update(cx, |auth, cx| auth.adopt(session, cx));
                }
                Err(error) => {
                    this.network_seen = error.code == "network_exists";
                    this.error = Some(
                        if error.code == "cloud_required" {
                            "Le serveur cloud n'est pas accessible. Vérifiez votre connexion internet et l'adresse du serveur (Réglages › Synchronisation).".to_string()
                        } else {
                            humanize_error(&error, "Erreur inattendue, veuillez réessayer.")
                        }
                        .into(),
                    );
                }
            }
            cx.notify();
        });
    }

    // ---- a lost password ----------------------------------------------------------------------

    fn open_recover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.recover_error = None;
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Mot de passe perdu").width(px(520.)).keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let page = for_body.read(cx);
                element(
                    col()
                        .gap(m.space_3)
                        .child(Alert::info().child(
                            "Employé ? Demandez un nouveau mot de passe à un administrateur. La clé de récupération sert quand aucun \
                             administrateur ne peut plus se connecter : le compte indiqué recevra le nouveau mot de passe et le rôle \
                             d'administrateur.",
                        ))
                        .children(page.recover_error.clone().map(|error| Alert::danger().child(error)))
                        .child(FormField::new("Clé de récupération").required().child(Input::new(&page.recover_key).mono()))
                        .child(FormField::new("Identifiant du compte").required().child(Input::new(&page.recover_username)))
                        .child(
                            FormField::new("Nouveau mot de passe")
                                .required()
                                .hint("Minimum 8 caractères.")
                                .child(Input::new(&page.recover_password).reveal_toggle()),
                        )
                        .child(FormField::new("Confirmer le mot de passe").required().child(Input::new(&page.recover_confirm).reveal_toggle())),
                )
            },
            move |_, cx| {
                let saving = for_footer.read(cx).recovering;
                let page = for_footer.clone();
                vec![
                    element(Button::new("recover-cancel").ghost().label("Annuler").disabled(saving).on_click(|_, window, cx| close_modal(window, cx))),
                    element(Button::new("recover-submit").primary().label("Reprendre la main").loading(saving).on_click(move |_, window, cx| {
                        page.update(cx, |this, cx| this.submit_recover(window, cx))
                    })),
                ]
            },
        );
    }

    fn submit_recover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recovering {
            return;
        }
        let (key, username) = (self.recover_key.text(cx), self.recover_username.text(cx).trim().to_string());
        let (password, confirm) = (self.recover_password.text(cx), self.recover_confirm.text(cx));
        self.recover_error = recover_problem(&key, &username, &password, &confirm).map(Into::into);
        if self.recover_error.is_some() {
            cx.notify();
            return;
        }
        self.recovering = true;
        cx.notify();
        let payload = RecoverPayload { recovery_key: key, username, password, confirm };
        cx.api_in(
            window,
            move |client| flows::recover_local(client, payload),
            |this, result, window, cx| {
                this.recovering = false;
                match result {
                    Ok(session) => {
                        for field in [&this.recover_key, &this.recover_username, &this.recover_password, &this.recover_confirm] {
                            field.clear(window, cx);
                        }
                        close_modal(window, cx);
                        // The key that replaces the one just used is shown next, to be written down.
                        AuthStore::global(cx).update(cx, |auth, cx| auth.adopt(session, cx));
                    }
                    Err(error) => this.recover_error = Some(humanize_error(&error, "Erreur inattendue, veuillez réessayer.").into()),
                }
                cx.notify();
            },
        );
    }

    // ---- a backup, on a PC with nothing set up -------------------------------------------------

    fn open_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_error = None;
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Restaurer une sauvegarde").width(px(520.)).keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let theme = cx.ui().clone();
                let (c, m) = (theme.colors, theme.m);
                let page = for_body.read(cx);
                let body = col().gap(m.space_3).children(page.restore_error.clone().map(|error| Alert::danger().child(error)));
                if page.restore_queued {
                    return element(body.child(Alert::success().child(
                        "La sauvegarde est prête. Motodesk doit redémarrer ; connectez-vous ensuite avec un compte qui existait au moment de la sauvegarde.",
                    )));
                }
                let name = page.restore_file.as_ref().and_then(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned());
                let choose = for_body.clone();
                element(
                    body.child(div().fs(m.fs_sm).text_color(c.text).child("Choisissez le fichier .mgbak de l'atelier."))
                        .child(
                            row()
                                .gap(m.space_3)
                                .items_center()
                                .child(Button::new("restore-choose").icon(IconName::FolderOpen).label("Choisir le fichier…").disabled(page.restoring).on_click(
                                    move |_, window, cx| choose.update(cx, |this, cx| this.choose_backup(window, cx)),
                                ))
                                .children(name.map(|name| div().min_w_0().fs(m.fs_sm).font_family(FONT_MONO).text_color(c.text_muted).child(name))),
                        )
                        .child(FormField::new("Mot de passe de la sauvegarde").required().child(Input::new(&page.restore_password).reveal_toggle())),
                )
            },
            move |_, cx| {
                let page = for_footer.read(cx);
                let (busy, queued) = (page.restoring, page.restore_queued);
                let ready = page.restore_file.is_some() && !page.restore_password.text(cx).is_empty();
                if queued {
                    return vec![element(Button::new("restore-restart").primary().label("Redémarrer").on_click(|_, _, cx| crate::app::restart(cx)))];
                }
                let send = for_footer.clone();
                vec![
                    element(Button::new("restore-cancel").ghost().label("Annuler").disabled(busy).on_click(|_, window, cx| close_modal(window, cx))),
                    element(Button::new("restore-submit").primary().label("Restaurer").loading(busy).disabled(!ready).on_click(move |_, window, cx| {
                        send.update(cx, |this, cx| this.submit_restore(window, cx))
                    })),
                ]
            },
        );
    }

    fn choose_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_error = None;
        cx.pick_files(window, "Choisir une sauvegarde Motodesk", |this, chosen, _, cx| {
            match chosen.map(|paths| paths.into_iter().next()) {
                Ok(Some(path)) if looks_like_a_backup(&path) => this.restore_file = Some(path),
                Ok(Some(_)) => this.restore_error = Some("Ce fichier n'est pas une sauvegarde Motodesk (.mgbak).".into()),
                // The dialog was closed: the file chosen before stays.
                Ok(None) => {}
                Err(error) => this.restore_error = Some(format!("Impossible d'ouvrir le choix de fichier : {error}").into()),
            }
            cx.notify();
        });
    }

    fn submit_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.restore_password.text(cx);
        let Some(path) = self.restore_file.clone().filter(|_| !self.restoring && !self.restore_queued && !password.is_empty()) else { return };
        self.restoring = true;
        self.restore_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { backup::restore_on_first_run(&client, &path.to_string_lossy(), &password).await },
            |this, result, window, cx| {
                this.restoring = false;
                match result {
                    Ok(_) => {
                        this.restore_password.clear(window, cx);
                        this.restore_queued = true;
                    }
                    Err(error) => this.restore_error = Some(humanize_error(&error, "La restauration a échoué.").into()),
                }
                cx.notify();
            },
        );
    }

    // ---- the four forms ---------------------------------------------------------------------

    fn head(title: &'static str, subtitle: &'static str, cx: &App) -> Div {
        let theme = cx.ui();
        col().gap(theme.m.space_1).child(Heading::new(title).size(HeadingSize::Xl2)).child(
            div().fs(theme.m.fs_sm).text_color(theme.colors.text_muted).child(subtitle),
        )
    }

    fn note(text: &'static str, cx: &App) -> Div {
        let theme = cx.ui();
        div().fs(theme.m.fs_sm).text_color(theme.colors.text_muted).child(text)
    }

    fn submit_button(&self, label: &'static str, cx: &mut Context<Self>) -> Button {
        Button::new("auth-submit")
            .primary()
            .full_width()
            .loading(self.submitting)
            .label(label)
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
    }

    fn online_login(&self, cx: &mut Context<Self>) -> Div {
        let m = cx.ui().m;
        col()
            .gap(m.space_4)
            .child(Self::head("Connexion", "Accédez à votre espace de travail.", cx))
            .children(self.error.clone().map(|e| Alert::danger().child(e)))
            .child(FormField::new("Email ou identifiant").required().child(Input::new(&self.identifier)))
            .child(FormField::new("Mot de passe").required().child(Input::new(&self.password).reveal_toggle()))
            .child(self.submit_button("Se connecter", cx))
    }

    fn online_register(&self, cx: &mut Context<Self>) -> Div {
        let m = cx.ui().m;
        let joining = self.invite.trimmed(cx).is_some();
        col()
            .gap(m.space_4)
            .child(Self::head("Créer un compte", "Configurez votre espace Motodesk.", cx))
            .children(self.error.clone().map(|e| Alert::danger().child(e)))
            .child(
                FormField::new("Nom de l'entreprise")
                    .hint(if joining {
                        "Inutile avec un code d'invitation : vous rejoignez une entreprise existante."
                    } else {
                        "Le nom de votre atelier ou de votre concession."
                    })
                    .child(Input::new(&self.company).disabled(joining)),
            )
            .child(
                FormField::new("Code d'invitation")
                    .hint("Pour rejoindre une entreprise existante. Laissez vide pour créer la vôtre (premier compte seulement).")
                    .child(Input::new(&self.invite)),
            )
            .child(FormField::new("Nom complet").required().child(Input::new(&self.full_name)))
            .child(FormField::new("Email").required().child(Input::new(&self.email)))
            .child(
                row()
                    .gap(m.space_3)
                    .items_start()
                    .child(FormField::new("Mot de passe").required().hint("Minimum 8 caractères").child(Input::new(&self.new_password)))
                    .child(FormField::new("Confirmer").required().child(Input::new(&self.new_confirm))),
            )
            .child(self.submit_button("Créer le compte", cx))
    }

    fn local_login(&self, state: LocalState, cx: &mut Context<Self>) -> Div {
        let m = cx.ui().m;
        let joining = state == LocalState::Setup;
        let form = col().gap(m.space_4).child(Self::head(
            "Réseau local",
            "Connectez-vous avec un compte du réseau de l'atelier.",
            cx,
        ));
        match state {
            LocalState::Online => form.child(Alert::info().child(
                "Cette installation est maintenant en ligne : repassez sur « En ligne » et connectez-vous avec l'adresse e-mail de mise en ligne.",
            )),
            LocalState::Tied => {
                form.child(Alert::info().child("Ce poste est déjà lié à une entreprise en ligne : repassez sur « En ligne »."))
            }
            // A backup waits for the next start: nothing else is worth doing here.
            _ if self.restore_queued => form.child(
                Alert::success().title("Sauvegarde prête").child("Motodesk doit redémarrer pour la mettre en place.").child(
                    row().mt(m.space_1).child(Button::new("restore-restart-page").primary().small().label("Redémarrer").on_click(|_, _, cx| crate::app::restart(cx))),
                ),
            ),
            _ => form
                .children(self.error.clone().map(|e| Alert::danger().child(e)))
                .child(FormField::new("Identifiant").required().child(Input::new(&self.username)))
                .child(FormField::new("Mot de passe").required().child(Input::new(&self.local_password).reveal_toggle()))
                .when(joining, |this| {
                    this.child(Self::note(
                        "Ce poste n'a pas encore rejoint le réseau : saisissez le compte d'un administrateur. Il va chercher un poste allumé de l'atelier et s'y connecter ; les employés pourront ensuite ouvrir leur session ici.",
                        cx,
                    ))
                })
                .child(self.submit_button(if joining { "Rejoindre le réseau" } else { "Se connecter" }, cx))
                .when(!joining, |this| {
                    this.child(row().child(
                        Button::new("to-recover").link().small().label("Mot de passe perdu ?").on_click(cx.listener(|this, _, window, cx| this.open_recover(window, cx))),
                    ))
                })
                .when(joining, |this| {
                    this.child(row().child(
                        Button::new("to-restore").link().small().label("Restaurer une sauvegarde").on_click(cx.listener(|this, _, window, cx| this.open_restore(window, cx))),
                    ))
                })
                .when(joining, |this| {
                    this.child(
                        row().gap(m.space_2).fs(m.fs_sm).child("Pas encore de réseau dans cet atelier ?").child(
                            Button::new("to-create").link().small().label("Créer le réseau local").on_click(
                                cx.listener(|this, _, _, cx| this.set_way(Way::Register, cx)),
                            ),
                        ),
                    )
                }),
        }
    }

    fn local_register(&self, state: LocalState, cx: &mut Context<Self>) -> Div {
        let m = cx.ui().m;
        let form = col().gap(m.space_4).child(Self::head(
            "Créer le réseau local",
            "Créez l'administrateur du réseau de l'atelier. Aucune connexion internet n'est nécessaire ; vous pourrez passer en ligne plus tard depuis les réglages.",
            cx,
        ));
        match state {
            LocalState::SignIn => form.child(
                Alert::info().child("Ce poste fait déjà partie d'un réseau local.").child(
                    Button::new("to-login").link().small().label("Se connecter").on_click(
                        cx.listener(|this, _, _, cx| this.set_way(Way::Login, cx)),
                    ),
                ),
            ),
            LocalState::Online => {
                form.child(Alert::info().child("Cette installation est maintenant en ligne : repassez sur « En ligne »."))
            }
            LocalState::Tied => {
                form.child(Alert::info().child("Ce poste est déjà lié à une entreprise en ligne : repassez sur « En ligne »."))
            }
            _ => {
                let this = cx.entity();
                form.children(self.error.clone().map(|e| Alert::danger().child(e)))
                    .child(
                        FormField::new("Identifiant")
                            .required()
                            .hint("3 à 32 caractères : lettres, chiffres, « . », « _ », « - »")
                            .child(Input::new(&self.new_username)),
                    )
                    .child(
                        FormField::new("Mot de passe")
                            .required()
                            .hint("Minimum 8 caractères. Une clé de récupération vous sera donnée juste après.")
                            .child(Input::new(&self.new_local_password)),
                    )
                    .child(FormField::new("Confirmer le mot de passe").required().child(Input::new(&self.new_local_confirm)))
                    .when(self.network_seen, |form| {
                        form.child(
                            Checkbox::new("create-anyway", self.create_anyway)
                                .label("Créer quand même un nouveau réseau, séparé de celui qui existe")
                                .on_change(move |on, _, cx| {
                                    this.update(cx, |this, cx| {
                                        this.create_anyway = on;
                                        cx.notify();
                                    })
                                }),
                        )
                    })
                    .child(self.submit_button("Créer le réseau", cx))
            }
        }
    }
}

impl Render for AuthPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let auth = AuthStore::global(cx);
        let (mode, state, offered) = {
            let auth = auth.read(cx);
            (auth.access_mode, auth.local_state(), auth.local_offered())
        };
        let local = mode == AccessMode::Local;

        let form = match (mode, self.way) {
            (AccessMode::Online, Way::Login) => self.online_login(cx),
            (AccessMode::Online, Way::Register) => self.online_register(cx),
            (AccessMode::Local, Way::Login) => self.local_login(state, cx),
            (AccessMode::Local, Way::Register) => self.local_register(state, cx),
        };

        let tabs = cx.entity();
        let card = col()
            .w(px(if self.way == Way::Register { 440. } else { 380. }))
            .p(m.space_6)
            .gap(m.space_4)
            .bg(c.bg_surface)
            .border_1()
            .border_color(c.border)
            .child(
                Tabs::new(
                    "auth-tabs",
                    vec![TabItem::new("login", "Connexion"), TabItem::new("register", "Inscription")],
                    if self.way == Way::Login { "login" } else { "register" },
                )
                .small()
                .on_change(move |id, _, cx| {
                    let way = if id.as_ref() == "register" { Way::Register } else { Way::Login };
                    tabs.update(cx, |this, cx| this.set_way(way, cx))
                }),
            )
            .child(form);

        let hint = if local {
            "Réseau de l'atelier : les postes partagent les données, sans cloud."
        } else {
            match state {
                LocalState::Online => "Cette installation est maintenant en ligne.",
                LocalState::Tied => "Ce poste est déjà lié à une entreprise en ligne.",
                _ => "Compte en ligne, synchronisé avec votre cloud.",
            }
        };
        let side = |label: &'static str, on: bool| {
            div().fs(m.fs_sm).font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).text_color(
                if on { c.text_strong } else { c.text_muted },
            ).child(label)
        };
        let page = cx.entity();
        let access = col()
            .items_center()
            .gap(m.space_2)
            .child(
                row().gap(m.space_3).child(side("En ligne", !local)).child(
                    Switch::new("access", local).disabled(!offered).on_change(move |on, _, cx| {
                        let mode = if on { AccessMode::Local } else { AccessMode::Online };
                        AuthStore::global(cx).update(cx, |auth, cx| auth.choose_access(mode, cx));
                        page.update(cx, |this, cx| {
                            this.error = None;
                            cx.notify();
                        });
                    }),
                ).child(side("Local", local)),
            )
            .child(div().fs(m.fs_xs).text_color(c.text_subtle).child(hint));

        let brand = col()
            .flex_none()
            .w(px(420.))
            .h_full()
            .justify_between()
            .pt(m.titlebar_h + m.space_4)
            .px(m.space_6)
            .pb(m.space_6)
            .bg(c.accent)
            .text_color(c.text_on_accent)
            .child(
                row().gap(m.space_2).child(Logo::new().tiled()).child(
                    div().fs_tight(m.fs_md).font_weight(FontWeight::SEMIBOLD).child("Motodesk"),
                ),
            )
            .child(
                col()
                    .gap(m.space_2)
                    .child(
                        div()
                            .text_size(m.fs_brand)
                            .line_height(px(30.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Votre atelier,")
                            .child("en un seul écran."),
                    )
                    .child(div().fs(m.fs_sm).text_color(c.brand_fg_muted).child("Atelier · Ventes · Facturation · Clients")),
            )
            .child(div().fs(m.fs_xs).text_color(c.brand_fg_subtle).child(concat!("V", env!("CARGO_PKG_VERSION"))));

        row().size_full().items_stretch().child(brand).child(
            div().id("auth-pane").flex_1().min_w_0().h_full().overflow_y_scroll().bg(c.bg_app).child(
                col()
                    .min_h_full()
                    .items_center()
                    .justify_center()
                    .gap(m.space_4)
                    .pt(m.titlebar_h + m.space_6)
                    .px(m.space_6)
                    .pb(m.space_6)
                    .child(card)
                    .child(access),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{looks_like_a_backup, recover_problem};

    #[test]
    fn a_lost_password_needs_the_key_the_account_and_a_password_typed_twice() {
        assert_eq!(recover_problem(" ", "camille", "mot de passe", "mot de passe").as_deref(), Some("Saisissez la clé et l'identifiant du compte."));
        assert_eq!(recover_problem("ABCDE-FGHJK", " ", "mot de passe", "mot de passe").as_deref(), Some("Saisissez la clé et l'identifiant du compte."));
        assert!(recover_problem("ABCDE-FGHJK", "camille", "court", "court").is_some(), "a password too short");
        assert!(recover_problem("ABCDE-FGHJK", "camille", "mot de passe", "autre chose").is_some(), "two different passwords");
        assert_eq!(recover_problem("ABCDE-FGHJK", "camille", "mot de passe", "mot de passe"), None);
    }

    #[test]
    fn a_backup_is_known_by_its_name() {
        assert!(looks_like_a_backup(Path::new(r"D:\Sauvegardes\motodesk-2026-11-20.mgbak")));
        assert!(looks_like_a_backup(Path::new("atelier.MGBAK")) && looks_like_a_backup(Path::new("atelier.zip")));
        assert!(!looks_like_a_backup(Path::new("atelier.pdf")) && !looks_like_a_backup(Path::new("mgbak")));
    }
}
