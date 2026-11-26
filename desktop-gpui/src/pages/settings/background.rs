//! Paramètres › Synchronisation, the card « Arrière-plan »: keep this PC in the shop's
//! network when nobody has the window open. See `crate::background` for what the mode
//! does.

use motodesk_client::background::{self as api, BackgroundSetting};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::backend::ApiExt as _;
use crate::background;
use crate::format::humanize_error;

/// What to say once the mode has changed.
fn changed_message(enabled: bool) -> &'static str {
    if enabled { "Motodesk restera actif en arrière-plan et démarrera avec Windows." } else { "Motodesk se fermera avec sa fenêtre." }
}

pub struct BackgroundCard {
    setting: Option<BackgroundSetting>,
    error: Option<SharedString>,
    saving: bool,
}

impl BackgroundCard {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.api(
            |client| async move { api::get(&client).await },
            |this, result, cx| {
                match result {
                    Ok(setting) => this.setting = Some(setting),
                    Err(error) => this.error = Some(humanize_error(&error, "Réglage indisponible.").into()),
                }
                cx.notify();
            },
        );
        Self { setting: None, error: None, saving: false }
    }

    fn change(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.saving = true;
        self.error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { api::set(&client, on).await },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok(setting) => {
                        this.setting = Some(setting);
                        toast::success(changed_message(setting.enabled), window, cx);
                    }
                    // The switch goes back to what still applies.
                    Err(error) => this.error = Some(humanize_error(&error, "Réglage impossible.").into()),
                }
                cx.notify();
            },
        );
    }
}

impl Render for BackgroundCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let note = |text: &'static str| div().fs(m.fs_sm).text_color(c.text_muted).child(text);

        let mut body = col().gap(m.space_3).when_some(self.error.clone(), |this, error| this.child(Alert::danger().child(error)));
        if let Some(setting) = self.setting {
            let card = cx.entity();
            body = body
                .child(
                    Switch::new("background-enabled", setting.enabled)
                        .label("Rester actif en arrière-plan et démarrer avec Windows")
                        .hint(
                            "Fermer la fenêtre la cache sans arrêter Motodesk : l'icône reste près de l'horloge (clic pour rouvrir, menu pour \
                             quitter). Sans cela, les autres postes du réseau ne peuvent pas joindre celui-ci.",
                        )
                        .disabled(self.saving)
                        .on_change(move |on, window, cx| card.update(cx, |this, cx| this.change(on, window, cx))),
                )
                .when(setting.default_enabled, |this| this.child(note("Activé par défaut pour un poste d'un réseau local.")))
                // Said rather than left to be found out: such a build hides its window, but is
                // never started by Windows.
                .when(!background::registers_with_windows(), |this| {
                    this.child(note("Version de développement ou profil isolé : le démarrage avec Windows n'est pas enregistré."))
                })
                .child(
                    row().child(
                        Button::new("background-quit").icon(IconName::Power).label("Quitter Motodesk maintenant").on_click(|_, _, cx| cx.quit()),
                    ),
                );
        }
        SectionCard::new()
            .title("Arrière-plan")
            .subtitle("Garder ce poste joignable par les autres, fenêtre fermée.")
            .icon(IconName::RefreshCw)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::changed_message;

    #[test]
    fn the_change_is_said_in_what_it_does() {
        assert!(changed_message(true).contains("démarrera avec Windows"));
        assert!(changed_message(false).contains("se fermera avec sa fenêtre"));
    }
}
