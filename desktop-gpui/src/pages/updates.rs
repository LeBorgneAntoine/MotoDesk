//! A new version on screen (port of `routes/UpdateDialog.svelte` and `routes/UpdatesCard.svelte`):
//! what it brings and the button that installs it, and the card of the settings.
//!
//! The installer is downloaded and its signature verified before it runs; nothing here can
//! install an update that is not signed by the maintainer (see `crate::updates`).

use motodesk_client::updates::{download_percent, format_release_date};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::updates::{UpdateState, VERSION};

/// « Vous utilisez la version 0.1.0 · version 0.2.0 publiée le 22/11/2026. »
fn versions_line(current: &str, version: &str, date: Option<&str>) -> String {
    match format_release_date(date) {
        published if published.is_empty() => format!("Vous utilisez la version {current}."),
        published => format!("Vous utilisez la version {current} · version {version} publiée le {published}."),
    }
}

/// Show the version that is waiting: its release notes, and « Installer et redémarrer ».
pub fn open_dialog(window: &mut Window, cx: &mut App) {
    let title = match UpdateState::global(cx).read(cx).available.as_ref() {
        Some(update) => format!("Motodesk {} est disponible", update.version),
        None => "Mise à jour".to_string(),
    };
    Modal::new(title).width(px(640.)).keep_on_overlay_click().open(
        window,
        cx,
        |_, cx| {
            let theme = cx.ui().clone();
            let (c, m) = (theme.colors, theme.m);
            let (update, error, note, installing, percent) = {
                let state = UpdateState::global(cx).read(cx);
                (state.available.clone(), state.error.clone(), state.note.clone(), state.installing, download_percent(state.downloaded, state.total))
            };
            let Some(update) = update else { return element(div()) };
            let notes = if update.notes.trim().is_empty() {
                div().fs(m.fs_sm).text_color(c.text_muted).child("Cette version n'a pas de notes de version.").into_any_element()
            } else {
                Prose::new("update-notes", update.notes.clone()).compact().into_any_element()
            };
            element(
                col()
                    .gap(m.space_3)
                    .child(div().fs(m.fs_sm).text_color(c.text_muted).child(versions_line(
                        &update.current_version,
                        &update.version,
                        update.date.as_deref(),
                    )))
                    .when_some(error, |this, error| this.child(Alert::danger().child(error)))
                    .when_some(note, |this, note| this.child(Alert::success().child(note)))
                    .child(
                        div()
                            .id("update-notes-box")
                            .max_h(px(260.))
                            .overflow_y_scroll()
                            .p(m.space_3)
                            .bg(c.bg_app)
                            .border_1()
                            .border_color(c.border)
                            .child(notes),
                    )
                    .child(Alert::info().child(
                        "Motodesk se ferme pendant l'installation puis se rouvre tout seul. Vos données sont conservées : une copie de la base est \
                         faite avant sa mise à jour. Pensez à mettre à jour tous les postes de l'atelier. L'installateur est vérifié avec la clé de \
                         signature de Motodesk avant d'être lancé.",
                    ))
                    .when(installing, |this| {
                        this.child(match percent {
                            Some(percent) => row()
                                .gap(m.space_3)
                                .items_center()
                                .child(div().flex_1().child(ProgressBar::new(percent.into())))
                                .child(div().fs(m.fs_sm).text_color(c.text_muted).child(format!("{percent}\u{a0}%"))),
                            None => row().child(div().fs(m.fs_sm).text_color(c.text_muted).child("Téléchargement…")),
                        })
                    }),
            )
        },
        |_, cx| {
            let installing = UpdateState::global(cx).read(cx).installing;
            vec![
                element(
                    Button::new("update-later").ghost().label("Plus tard").disabled(installing).on_click(|_, window, cx| close_modal(window, cx)),
                ),
                element(
                    Button::new("update-install")
                        .primary()
                        .label("Installer et redémarrer")
                        .loading(installing)
                        .on_click(|_, _, cx| UpdateState::global(cx).update(cx, |updates, cx| updates.install(cx))),
                ),
            ]
        },
    );
}

/// Paramètres › Sauvegardes: the running version, a look asked for by hand, and whether this PC
/// looks by itself. A look contacts the project's release page and says nothing of the shop.
pub struct UpdatesCard {
    _subscription: Subscription,
}

impl UpdatesCard {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { _subscription: cx.observe(&UpdateState::global(cx), |_, _, cx| cx.notify()) }
    }
}

impl Render for UpdatesCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let (available, error, up_to_date, checking, auto) = {
            let state = UpdateState::global(cx).read(cx);
            (state.available.as_ref().map(|update| update.version.clone()), state.error.clone(), state.up_to_date, state.checking, state.prefs.auto)
        };
        let body = col()
            .gap(m.space_3)
            .when_some(error, |this, error| this.child(Alert::danger().child(error)))
            .map(|this| match available {
                Some(version) => this.child(Alert::info().title(format!("La version {version} est disponible")).child(row().mt(m.space_1).child(
                    Button::new("updates-open").primary().small().label("Voir les nouveautés").on_click(|_, window, cx| open_dialog(window, cx)),
                ))),
                None if up_to_date => this.child(Alert::success().child("Motodesk est à jour.")),
                None => this,
            })
            .child(row().gap(m.space_1).child("Version installée :").child(div().font_weight(FontWeight::SEMIBOLD).child(VERSION)))
            .child(
                row().child(
                    Button::new("updates-check")
                        .icon(IconName::RefreshCw)
                        .label("Rechercher une mise à jour")
                        .loading(checking)
                        .on_click(|_, _, cx| UpdateState::global(cx).update(cx, |updates, cx| updates.check(true, cx))),
                ),
            )
            .child(
                Switch::new("updates-auto", auto)
                    .label("Rechercher automatiquement")
                    .hint("Une fois au démarrage puis toutes les quelques heures, sur ce poste.")
                    .on_change(|on, _, cx| UpdateState::global(cx).update(cx, |updates, cx| updates.set_auto(on, cx))),
            )
            // Said rather than left to be found out.
            .when(cfg!(debug_assertions), |this| {
                this.child(div().fs(m.fs_sm).text_color(c.text_muted).child(
                    "Version de développement : elle ne lance aucun installateur, et ne cherche seule qu'auprès d'un serveur de démonstration.",
                ))
            });
        SectionCard::new()
            .title("Mises à jour")
            .subtitle("Motodesk vous prévient quand une nouvelle version existe ; rien ne s'installe sans votre accord.")
            .icon(IconName::RefreshCw)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::versions_line;

    #[test]
    fn the_dialog_says_which_version_runs_and_which_waits() {
        assert_eq!(versions_line("0.1.0", "0.2.0", Some("2026-11-22")), "Vous utilisez la version 0.1.0 · version 0.2.0 publiée le 22/11/2026.");
        assert_eq!(versions_line("0.1.0", "0.2.0", None), "Vous utilisez la version 0.1.0.");
    }
}
