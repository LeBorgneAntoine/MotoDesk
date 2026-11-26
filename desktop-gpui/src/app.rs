//! The root of the window: one of four things is on screen.
//!
//! - the data service could not start: one clear page replaces every screen;
//! - the stored session is being read: a spinner;
//! - nobody is signed in: the sign-in page;
//! - someone is: the shell.
//!
//! It also owns the one dialog that can appear over any of them: the recovery key to write
//! down, right after a local network is created.

use std::path::PathBuf;

use motodesk_client::recovery;
use motodesk_server::startup::StartupFailure;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::auth::{AuthStatus, AuthStore};
use crate::backend::{self, ApiExt as _};
use crate::format::{humanize_error, last_group_matches};
use crate::pages::auth::AuthPage;
use crate::pages::pos::Till;
use crate::pos::{self, PosState};
use crate::printing;
use crate::shell::Shell;

pub struct Root {
    failure: Option<StartupFailure>,
    auth_page: Entity<AuthPage>,
    shell: Option<Entity<Shell>>,
    /// The till, on a PC set up as one.
    till: Option<Entity<Till>>,
    /// The last characters of the recovery key, typed back to prove it was written down.
    key_check: TextModel,
    key_saving: bool,
    key_shown: bool,
    /// The key as written for the PDF reader, to be removed once it is confirmed.
    key_files: Vec<PathBuf>,
    _subscriptions: Vec<Subscription>,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key_check = TextModel::builder().placeholder("5 caractères").build(window, cx);
        let subscriptions = vec![
            cx.observe_in(&AuthStore::global(cx), window, |this, _, window, cx| {
                this.session_changed(window, cx);
                cx.notify();
            }),
            key_check.on_event(window, cx, |_, _, _, cx| cx.notify()),
            cx.observe_in(&PosState::global(cx), window, |this, _, window, cx| {
                this.screen_changed(window, cx);
                cx.notify();
            }),
        ];
        Self {
            failure: None,
            auth_page: cx.new(|cx| AuthPage::new(window, cx)),
            shell: None,
            till: None,
            key_check,
            key_saving: false,
            key_shown: false,
            key_files: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    /// The data service did not start.
    pub fn failed(&mut self, failure: StartupFailure, cx: &mut Context<Self>) {
        self.failure = Some(failure);
        cx.notify();
    }

    fn session_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (signed_in, has_key) = {
            let auth = AuthStore::global(cx).read(cx);
            (auth.is_authenticated(), auth.recovery_key.is_some())
        };
        // A new session starts on fresh screens: nothing of the previous account is kept.
        if !signed_in {
            self.shell = None;
            self.till = None;
        }
        self.screen_changed(window, cx);
        if has_key && !self.key_shown {
            self.key_shown = true;
            self.show_recovery_key(window, cx);
        }
        if !has_key {
            self.key_shown = false;
        }
    }

    /// Whether the till is what this PC shows now: signed in, on a PC set up as a till, and
    /// not stepped out to the office.
    fn at_the_till(cx: &App) -> bool {
        AuthStore::global(cx).read(cx).is_authenticated() && PosState::global(cx).read(cx).active()
    }

    /// The till fills the screen; everything else is a window. A till that is left starts
    /// again empty.
    fn screen_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at_the_till = Self::at_the_till(cx);
        if !at_the_till {
            self.till = None;
        }
        pos::set_fullscreen(at_the_till, window);
    }

    fn show_recovery_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Votre clé de récupération").width(px(520.)).locked().open(
            window,
            cx,
            move |_, cx| {
                let theme = cx.ui().clone();
                let (c, m) = (theme.colors, theme.m);
                let key = AuthStore::global(cx).read(cx).recovery_key.clone().unwrap_or_default();
                let check = for_body.read(cx).key_check.clone();
                let to_copy = key.clone();
                let page = for_body.clone();
                element(
                    col()
                        .gap(m.space_4)
                        .child(
                            div()
                                .child("Si un mot de passe d'administrateur est perdu, seule cette clé permet de reprendre la main. ")
                                .child(div().font_weight(FontWeight::SEMIBOLD).child("Elle ne sera plus jamais affichée.")),
                        )
                        .child(
                            div()
                                .p(m.space_3)
                                .bg(c.bg_app)
                                .border_1()
                                .border_color(c.border)
                                .font_family(FONT_MONO)
                                .text_size(m.fs_xl)
                                .text_color(c.text_strong)
                                .text_center()
                                .child(key),
                        )
                        .child(
                            row()
                                .gap(m.space_2)
                                .child(Button::new("key-copy").icon(IconName::Copy).label("Copier").on_click(move |_, window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(to_copy.clone()));
                                    toast::success("Clé copiée. Collez-la dans un gestionnaire de mots de passe.", window, cx);
                                }))
                                .child(Button::new("key-print").icon(IconName::Printer).label("Imprimer").on_click(move |_, window, cx| {
                                    page.update(cx, |this, cx| this.print_recovery_key(window, cx))
                                })),
                        )
                        .child(Alert::warning().child("Rangez-la hors de ce poste. Qui la détient peut prendre le contrôle des comptes."))
                        .child(FormField::new("Retapez les 5 derniers caractères").required().child(Input::new(&check).mono())),
                )
            },
            move |_, cx| {
                let key = AuthStore::global(cx).read(cx).recovery_key.clone().unwrap_or_default();
                let root = for_footer.read(cx);
                let ready = last_group_matches(&key, &root.key_check.text(cx));
                let page = for_footer.clone();
                vec![element(
                    Button::new("key-noted").primary().label("J'ai noté la clé").disabled(!ready).loading(root.key_saving).on_click(
                        move |_, window, cx| page.update(cx, |this, cx| this.confirm_recovery_key(window, cx)),
                    ),
                )]
            },
        );
    }

    /// Hand the key, alone on a page, to the program this PC reads PDF files with. The file
    /// holds the key in clear: it is removed once the key is confirmed, and whatever a
    /// reader still holds then goes when the app next starts.
    fn print_recovery_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = AuthStore::global(cx).read(cx).recovery_key.clone() else { return };
        let page = recovery::key_page(&key, chrono::Local::now().date_naive());
        match printing::write("Clé de récupération Motodesk", &page) {
            Ok(path) => {
                printing::open(&path, cx);
                if !self.key_files.contains(&path) {
                    self.key_files.push(path);
                }
                toast::success("Clé ouverte pour impression. Son fichier est effacé de ce poste une fois la clé confirmée.", window, cx);
            }
            Err(_) => toast::error("Impression impossible : recopiez la clé à la main.", window, cx),
        }
    }

    fn confirm_recovery_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = AuthStore::global(cx).read(cx).recovery_key.clone() else { return };
        if self.key_saving || !last_group_matches(&key, &self.key_check.text(cx)) {
            return;
        }
        self.key_saving = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { recovery::confirm(&client, &key).await },
            |this, result, window, cx| {
                this.key_saving = false;
                match result {
                    Ok(()) => {
                        for file in this.key_files.drain(..) {
                            printing::forget(&file);
                        }
                        this.key_check.clear(window, cx);
                        close_modal(window, cx);
                        AuthStore::global(cx).update(cx, |auth, cx| auth.recovery_key_noted(cx));
                    }
                    Err(error) => {
                        toast::error(humanize_error(&error, "Impossible d'enregistrer la confirmation."), window, cx)
                    }
                }
                cx.notify();
            },
        );
    }

    /// A page with nothing but a message, under a title bar that keeps the window movable.
    fn bare(content: impl IntoElement, cx: &App) -> Div {
        let theme = cx.ui();
        col()
            .size_full()
            .bg(theme.colors.bg_app)
            .text_color(theme.colors.text)
            .font_family(FONT_UI)
            .fs(theme.m.fs_md)
            .child(TitleBar::new().transparent())
            .child(div().flex_1().min_h_0().child(content))
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(failure) = self.failure.clone() {
            return Self::bare(
                StateView::error()
                    .title(failure.title)
                    .body(failure.message)
                    .action("Relancer Motodesk", |_, _, cx| restart(cx))
                    .secondary("Ouvrir les journaux", |_, _, _| {
                        let _ = motodesk_server::logs::open_dir();
                    }),
                cx,
            )
            .into_any_element();
        }

        let status = AuthStore::global(cx).read(cx).status;
        if backend::client(cx).is_none() || status == AuthStatus::Initializing {
            return Self::bare(StateView::loading(), cx).into_any_element();
        }

        if status == AuthStatus::Authenticated && Self::at_the_till(cx) {
            let till = self.till.get_or_insert_with(|| cx.new(|cx| Till::new(window, cx))).clone();
            return till.into_any_element();
        }

        if status == AuthStatus::Authenticated {
            let shell = self.shell.get_or_insert_with(|| cx.new(|cx| Shell::new(window, cx))).clone();
            return shell.into_any_element();
        }

        // The sign-in scene paints up to the top edge; the title bar floats over it.
        let theme = cx.ui();
        div()
            .relative()
            .size_full()
            .text_color(theme.colors.text)
            .font_family(FONT_UI)
            .fs(theme.m.fs_md)
            .child(self.auth_page.clone())
            .child(div().absolute().top_0().left_0().right_0().child(TitleBar::new().transparent()))
            .into_any_element()
    }
}

/// Start the application again and leave this run: a restore waiting to be applied, or a
/// second try after a failed start.
pub fn restart(cx: &mut App) {
    if let Ok(exe) = std::env::current_exe() {
        // Marked as a restart: the new copy waits for this one to be gone instead of taking
        // itself for a second start (see `background`).
        let _ = std::process::Command::new(exe).args(crate::background::restart_arguments(std::env::args().skip(1))).spawn();
    }
    cx.quit();
}
