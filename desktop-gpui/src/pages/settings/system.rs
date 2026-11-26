//! Backups and the application's own logs (port of `routes/BackupSettings.svelte` and
//! `routes/DiagnosticsCard.svelte`). Administrators only.
//!
//! One encrypted file holds the database, the fiscal archives and the signing keys, so a new
//! PC can pick up where this one stopped.

use motodesk_client::backup::{self, BackupFile, BackupStatus};
use motodesk_client::settings;
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;
use serde_json::Value;

use super::registry::{AUTO_BACKUP, BACKUP_FREQUENCY, def_by_key, same};
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};
use crate::pages::updates::UpdatesCard;

/// What the confirmation of a restore says: which file, from when, and what is lost.
fn restore_warning(file: &BackupFile) -> String {
    format!(
        "Les données actuelles seront remplacées par celles de « {} » ({}).\n\nTout ce qui a été enregistré depuis cette date sera absent de la base restaurée. Une copie des données actuelles est conservée.\n\nLe remplacement a lieu au redémarrage.",
        file.filename,
        format::date_time(&file.created_at)
    )
}

/// One line about a backup: how it was made, its size, when.
fn file_meta(file: &BackupFile) -> String {
    format!("{} · {} · {}", file.kind_label(), format::bytes(file.size_bytes), format::date_time(&file.created_at))
}

pub struct SystemView {
    status: Option<BackupStatus>,
    loading: bool,
    error: Option<String>,
    auto_enabled: bool,
    frequency: TextModel,
    /// The frequency the service holds: what is typed is saved only when it differs.
    saved_frequency: Value,
    frequency_problem: Option<String>,
    auto_password: TextModel,
    editing_auto_password: bool,
    export_password: TextModel,
    restore_password: TextModel,
    restore_target: Option<BackupFile>,
    busy: bool,
    /// Set once a restore is queued: from then on the only sensible step is to restart.
    restore_note: Option<String>,
    /// The running version and the look for a newer one: a card with a life of its own.
    updates: Entity<UpdatesCard>,
    _subscriptions: Vec<Subscription>,
}

impl SystemView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let frequency = TextModel::new(window, cx);
        let password = |window: &mut Window, cx: &mut Context<Self>| TextModel::builder().password().build(window, cx);
        let (auto_password, export_password, restore_password) = (password(window, cx), password(window, cx), password(window, cx));
        let subscriptions = vec![
            frequency.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                // Saved when the field is left or Enter is pressed, not at every digit.
                TextEvent::Submitted { .. } | TextEvent::Blurred => this.save_frequency(window, cx),
                TextEvent::Changed => {
                    this.frequency_problem = None;
                    cx.notify();
                }
                TextEvent::Focused => {}
            }),
            auto_password.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.save_auto_password(window, cx);
                }
            }),
            export_password.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.export(window, cx);
                }
            }),
            restore_password.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.ask_restore(window, cx);
                }
            }),
        ];
        let mut view = Self {
            status: None,
            loading: true,
            error: None,
            auto_enabled: true,
            frequency,
            saved_frequency: Value::Null,
            frequency_problem: None,
            auto_password,
            editing_auto_password: false,
            export_password,
            restore_password,
            restore_target: None,
            busy: false,
            restore_note: None,
            updates: cx.new(UpdatesCard::new),
            _subscriptions: subscriptions,
        };
        view.load(window, cx);
        view
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            |client| async move { (backup::status(&client).await, settings::list(&client).await) },
            |this, (status, stored), window, cx| {
                this.loading = false;
                match status {
                    Ok(status) => {
                        this.status = Some(status);
                        this.error = None;
                    }
                    Err(error) => this.error = Some(humanize_error(&error, "Impossible de lire les sauvegardes.")),
                }
                // The two settings of the automatic backup, or their defaults.
                let stored = stored.unwrap_or_default();
                let value = |key: &str| {
                    let def = def_by_key(key);
                    stored.iter().find(|setting| setting.key == key).map(|setting| setting.value.clone()).or_else(|| def.map(|def| def.default_value()))
                };
                this.auto_enabled = value(AUTO_BACKUP).and_then(|value| value.as_bool()).unwrap_or(true);
                if let (Some(def), Some(days)) = (def_by_key(BACKUP_FREQUENCY), value(BACKUP_FREQUENCY)) {
                    this.frequency.set_text(def.shown(&days), window, cx);
                    this.saved_frequency = days;
                }
                cx.notify();
            },
        );
    }

    /// Read the list of backups again, after one was written.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { backup::status(&client).await },
            |this, status, cx| {
                if let Ok(status) = status {
                    this.status = Some(status);
                }
                cx.notify();
            },
        );
    }

    fn set_auto(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.auto_enabled = on;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { settings::set(&client, AUTO_BACKUP, &Value::from(on)).await },
            move |this, result, window, cx| {
                if let Err(error) = result {
                    this.auto_enabled = !on;
                    toast::error(humanize_error(&error, "Impossible d'enregistrer ce réglage."), window, cx);
                    cx.notify();
                }
            },
        );
    }

    fn save_frequency(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(def) = def_by_key(BACKUP_FREQUENCY) else { return };
        let days = match def.read(&self.frequency.text(cx)) {
            Ok(days) => days,
            Err(problem) => {
                self.frequency_problem = Some(problem);
                cx.notify();
                return;
            }
        };
        if same(&days, &self.saved_frequency) {
            return;
        }
        // Remembered at once: leaving the field right after Enter asks for the same save twice.
        self.saved_frequency = days.clone();
        cx.api_in(
            window,
            move |client| async move { settings::set(&client, BACKUP_FREQUENCY, &days).await },
            |this, result, window, cx| match result {
                Ok(_) => toast::success("Fréquence de sauvegarde enregistrée.", window, cx),
                Err(error) => {
                    this.saved_frequency = Value::Null;
                    toast::error(humanize_error(&error, "Impossible d'enregistrer la fréquence."), window, cx);
                }
            },
        );
    }

    fn save_auto_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.auto_password.text(cx);
        if let Some(problem) = backup::password_problem(&password) {
            toast::error(problem, window, cx);
            return;
        }
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { backup::set_auto_password(&client, &password).await },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.auto_password.clear(window, cx);
                        this.editing_auto_password = false;
                        toast::success("Mot de passe enregistré. Les sauvegardes automatiques peuvent démarrer.", window, cx);
                        this.refresh(cx);
                    }
                    Err(error) => toast::error(humanize_error(&error, "Impossible d'enregistrer le mot de passe."), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn remove_auto_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            |client| async move { backup::clear_auto_password(&client).await },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        toast::success("Mot de passe retiré : les sauvegardes automatiques sont suspendues.", window, cx);
                        this.refresh(cx);
                    }
                    Err(error) => toast::error(humanize_error(&error, "Impossible de retirer le mot de passe."), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.export_password.text(cx);
        if let Some(problem) = backup::password_problem(&password) {
            toast::error(problem, window, cx);
            return;
        }
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { backup::export(&client, &password).await },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(file) => {
                        this.export_password.clear(window, cx);
                        toast::success(format!("Sauvegarde écrite : {}", file.filename), window, cx);
                        this.refresh(cx);
                    }
                    Err(error) => toast::error(humanize_error(&error, "Échec de la sauvegarde."), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn choose_restore(&mut self, file: BackupFile, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_target = Some(file);
        self.restore_password.clear(window, cx);
        self.restore_password.focus(window, cx);
        cx.notify();
    }

    fn ask_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.restore_target.clone() else { return };
        if self.restore_password.text(cx).is_empty() {
            toast::error("Saisissez le mot de passe de cette archive.", window, cx);
            return;
        }
        let view = cx.entity();
        Confirm::new("Remplacer les données de cet ordinateur ?", restore_warning(&file))
            .confirm_label("Oui, restaurer")
            .danger()
            .open(window, cx, move |window, cx| view.update(cx, |this, cx| this.restore(window, cx)));
    }

    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.restore_target.clone() else { return };
        if self.busy {
            return;
        }
        let password = self.restore_password.text(cx);
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { backup::restore(&client, &file.filename, &password).await },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(result) => {
                        this.restore_target = None;
                        this.restore_password.clear(window, cx);
                        if result.restart_required {
                            this.restore_note = Some(result.safety_copy_note);
                        }
                    }
                    Err(error) => toast::error(humanize_error(&error, "Échec de la restauration."), window, cx),
                }
                cx.notify();
            },
        );
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn text(text: impl Into<SharedString>, cx: &App) -> Div {
        let theme = cx.ui();
        div().fs(theme.m.fs_sm).text_color(theme.colors.text_muted).child(text.into())
    }

    fn automatic(&self, status: &BackupStatus, cx: &mut Context<Self>) -> SectionCard {
        let m = cx.ui().m;
        let view = cx.entity();
        let password: AnyElement = if status.auto_password_set && !self.editing_auto_password {
            row()
                .gap(m.space_2)
                .child(Self::text("Mot de passe défini.", cx))
                .child(Button::new("backup-auto-change").ghost().small().label("Changer").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.editing_auto_password = true;
                        this.auto_password.focus(window, cx);
                        cx.notify();
                    },
                )))
                .child(
                    Button::new("backup-auto-remove")
                        .ghost()
                        .small()
                        .label("Retirer")
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.remove_auto_password(window, cx))),
                )
                .into_any_element()
        } else {
            col()
                .gap(m.space_3)
                .child(
                    FormField::new("Mot de passe des sauvegardes automatiques")
                        .hint(format!(
                            "{} caractères au moins. Notez-le et gardez-le hors de l'atelier : il sert à ouvrir ces fichiers sur un nouvel ordinateur.",
                            backup::MIN_PASSWORD_CHARS
                        ))
                        .child(div().max_w(px(360.)).child(Input::new(&self.auto_password).reveal_toggle())),
                )
                .child(
                    row()
                        .gap(m.space_2)
                        .when(self.editing_auto_password, |this| {
                            this.child(Button::new("backup-auto-cancel").ghost().label("Annuler").on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.editing_auto_password = false;
                                    this.auto_password.clear(window, cx);
                                    cx.notify();
                                },
                            )))
                        })
                        .child(
                            Button::new("backup-auto-save")
                                .primary()
                                .label("Enregistrer le mot de passe")
                                .loading(self.busy)
                                .on_click(cx.listener(|this, _, window, cx| this.save_auto_password(window, cx))),
                        ),
                )
                .into_any_element()
        };

        SectionCard::new().title("Automatique").icon(IconName::CalendarClock).child(
            col()
                .gap(m.space_4)
                .child(
                    Switch::new("backup-auto", self.auto_enabled)
                        .label("Sauvegarde automatique")
                        .hint("Écrit une archive chiffrée dans le dossier de l'application et garde les 8 dernières.")
                        .on_change(move |on, window, cx| view.update(cx, |this, cx| this.set_auto(on, window, cx))),
                )
                .when(self.auto_enabled && !status.auto_password_set, |this| {
                    this.child(
                        Alert::warning()
                            .title("Sauvegarde automatique suspendue")
                            .child("Choisissez un mot de passe : aucune sauvegarde automatique n'est écrite sans lui."),
                    )
                })
                .child(password)
                .child(div().w(px(200.)).child(
                    FormField::new("Fréquence (jours)")
                        .error(self.frequency_problem.clone())
                        .child(Input::new(&self.frequency).invalid(self.frequency_problem.is_some())),
                )),
        )
    }

    fn export_card(&self, status: &BackupStatus, cx: &mut Context<Self>) -> SectionCard {
        let m = cx.ui().m;
        let last = status.last_backup_at.as_deref().map(|at| format!("Dernière sauvegarde {}", format::date_time(at)));
        SectionCard::new()
            .title("Exporter cet ordinateur")
            .subtitle(last.unwrap_or_else(|| "Aucune sauvegarde pour l'instant".into()))
            .icon(IconName::Download)
            .child(
                col()
                    .gap(m.space_3)
                    .child(Self::text("Dossier des sauvegardes :", cx))
                    .child(Preformatted::new(&status.backups_dir))
                    .child(
                        FormField::new("Mot de passe de l'archive")
                            .hint(format!(
                                "{} caractères au moins. Protège toute la sauvegarde. À conserver hors de l'atelier : sans lui, elle est illisible.",
                                backup::MIN_PASSWORD_CHARS
                            ))
                            .child(div().max_w(px(360.)).child(Input::new(&self.export_password).reveal_toggle())),
                    )
                    .child(
                        row().child(
                            Button::new("backup-export")
                                .primary()
                                .icon(IconName::Download)
                                .label("Créer une sauvegarde")
                                .loading(self.busy)
                                .on_click(cx.listener(|this, _, window, cx| this.export(window, cx))),
                        ),
                    ),
            )
    }

    fn files(&self, status: &BackupStatus, cx: &mut Context<Self>) -> SectionCard {
        let card = SectionCard::new().title("Archives sur cet ordinateur").icon(IconName::Database).flush();
        if status.files.is_empty() {
            return card.child(
                div().h(px(180.)).child(
                    StateView::empty()
                        .compact()
                        .icon(IconName::Database)
                        .title("Aucune archive")
                        .body("Créez une sauvegarde, ou copiez un fichier .mgbak dans le dossier ci-dessus."),
                ),
            );
        }
        let chosen = self.restore_target.as_ref().map(|file| file.filename.as_str());
        let rows = status.files.iter().enumerate().map(|(ix, file)| {
            let target = file.clone();
            ListRow::new(ix, file.filename.clone())
                .meta(file_meta(file))
                .icon(IconName::Database)
                .selected(chosen == Some(file.filename.as_str()))
                .trailing(Button::new(("backup-restore", ix)).small().label("Restaurer").on_click(cx.listener(
                    move |this, _, window, cx| this.choose_restore(target.clone(), window, cx),
                )))
        });
        card.children(rows.collect::<Vec<_>>())
    }

    fn restore_card(&self, file: &BackupFile, cx: &mut Context<Self>) -> SectionCard {
        let m = cx.ui().m;
        SectionCard::new().title(format!("Restaurer {}", file.filename)).icon(IconName::ArchiveRestore).child(
            col()
                .gap(m.space_3)
                .child(Self::text(
                    "Remplace les données de cet ordinateur au redémarrage. Ce qui a été saisi depuis la date de cette sauvegarde (ventes, règlements) n'y figure pas. Les données actuelles sont conservées à côté de la base au cas où.",
                    cx,
                ))
                .child(
                    FormField::new("Mot de passe de l'archive")
                        .child(div().max_w(px(360.)).child(Input::new(&self.restore_password).reveal_toggle())),
                )
                .child(
                    row()
                        .gap(m.space_2)
                        .child(Button::new("backup-restore-cancel").ghost().label("Annuler").on_click(cx.listener(|this, _, _, cx| {
                            this.restore_target = None;
                            cx.notify();
                        })))
                        .child(
                            Button::new("backup-restore-go")
                                .danger()
                                .label("Restaurer cette sauvegarde")
                                .loading(self.busy)
                                .on_click(cx.listener(|this, _, window, cx| this.ask_restore(window, cx))),
                        ),
                ),
        )
    }

    /// Where the log files are: what a shop sends along with a request for help.
    fn diagnostics(cx: &mut Context<Self>) -> Option<SectionCard> {
        let m = cx.ui().m;
        let folder = motodesk_server::logs::dir()?;
        Some(
            SectionCard::new()
                .title("Diagnostic")
                .subtitle("Les journaux de Motodesk, à joindre à une demande d'assistance.")
                .icon(IconName::FileText)
                .child(
                    col()
                        .gap(m.space_3)
                        .child(Self::text(
                            "Motodesk écrit un journal par jour et garde les 14 derniers. Ils décrivent ce que l'application a fait (démarrages, erreurs) et ne contiennent pas les données de l'atelier.",
                            cx,
                        ))
                        .child(Preformatted::new(folder.display().to_string()))
                        .child(row().child(Button::new("logs-open").label("Ouvrir le dossier des journaux").on_click(
                            |_, window, cx| {
                                if let Err(error) = motodesk_server::logs::open_dir() {
                                    toast::error(format!("Impossible d'ouvrir le dossier : {error}"), window, cx);
                                }
                            },
                        ))),
                ),
        )
    }
}

impl Render for SystemView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = cx.ui().m;
        let page = col().gap(m.space_4);
        if self.loading {
            return page.child(div().h(px(160.)).child(StateView::loading().compact().title("Chargement des sauvegardes…")));
        }
        let Some(status) = self.status.clone() else {
            let error = self.error.clone().unwrap_or_else(|| "Impossible de lire les sauvegardes.".into());
            return page.child(Alert::danger().child(error)).children(Self::diagnostics(cx));
        };

        let pending = (self.restore_note.is_some() || status.restore_pending).then(|| {
            Alert::warning().title("Restauration prête — redémarrage nécessaire").child(
                col()
                    .gap(m.space_3)
                    .child(format!(
                        "La sauvegarde choisie remplacera les données de cet ordinateur au prochain démarrage. Tout ce qui est saisi d'ici là sera perdu. {}",
                        self.restore_note.clone().unwrap_or_default()
                    ))
                    .child(row().child(
                        Button::new("backup-restart").primary().label("Redémarrer maintenant").on_click(|_, _, cx| crate::app::restart(cx)),
                    )),
            )
        });

        page.children(pending)
            .child(Alert::info().child(
                "Une sauvegarde est un fichier chiffré (.mgbak) : base de données, journaux fiscaux et clés de caisse. Pour changer d'ordinateur, copiez-le dans le dossier des sauvegardes du nouveau poste, puis restaurez-le ici avec son mot de passe. Sans ce mot de passe, la sauvegarde est inutilisable et personne ne peut le retrouver.",
            ))
            .child(self.automatic(&status, cx))
            .child(self.export_card(&status, cx))
            .child(self.files(&status, cx))
            .children(self.restore_target.clone().map(|file| self.restore_card(&file, cx)))
            .child(self.updates.clone())
            .children(Self::diagnostics(cx))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::backup::BackupFile;

    use super::{file_meta, restore_warning};

    fn file() -> BackupFile {
        BackupFile {
            filename: "motodesk-2026-11-20.mgbak".into(),
            path: String::new(),
            size_bytes: 3_482,
            created_at: "2026-11-20T08:30:00Z".into(),
            kind: "automatic".into(),
        }
    }

    #[test]
    fn a_backup_is_described_by_how_it_was_made_its_size_and_its_date() {
        let meta = file_meta(&file());
        assert!(meta.starts_with("Automatique · 3,4 Ko · "), "{meta}");
        assert!(meta.contains("20/11/2026") || meta.contains("20 nov"), "{meta}");
    }

    #[test]
    fn the_confirmation_of_a_restore_names_the_file_and_what_is_lost() {
        let warning = restore_warning(&file());
        assert!(warning.contains("« motodesk-2026-11-20.mgbak »"));
        assert!(warning.contains("sera absent de la base restaurée"));
        assert!(warning.ends_with("Le remplacement a lieu au redémarrage."));
    }
}
