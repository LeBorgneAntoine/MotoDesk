//! Whether this PC is a till, and what is plugged into it. Administrators only. Nothing here goes
//! to the shop's settings: it stays on this PC.

use motodesk_client::pos::{self, PosConfig, PosDocument, TpeMode};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::pos::PosState;

pub struct TillSettings {
    printer: TextModel,
    favourites: TextModel,
    document: SelectModel,
    terminal: SelectModel,
    _subscriptions: Vec<Subscription>,
}

impl TillSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = PosState::global(cx).read(cx).config.clone();
        let printer = TextModel::builder().placeholder("COM3").value(config.printer_target.clone()).build(window, cx);
        let favourites = TextModel::builder().placeholder("identifiant, identifiant").value(config.favorite_product_ids.join(", ")).build(window, cx);
        let document = SelectModel::new(
            PosDocument::ALL.into_iter().map(|kind| SelectOption::new(kind.id(), kind.label())).collect(),
            Some(config.default_document.id()),
            window,
            cx,
        );
        let terminal = SelectModel::new(
            TpeMode::ALL.into_iter().map(|mode| SelectOption::new(mode.id(), mode.label())).collect(),
            Some(config.tpe_mode.id()),
            window,
            cx,
        );

        // A field is kept when it is left or Enter is pressed, not at every letter.
        let left = |event: &TextEvent| matches!(event, TextEvent::Blurred | TextEvent::Submitted { .. });
        let subscriptions = vec![
            cx.observe(&PosState::global(cx), |_, _, cx| cx.notify()),
            printer.on_event(window, cx, move |this: &mut Self, event, window, cx| {
                if left(&event) {
                    let target = this.printer.text(cx).trim().to_string();
                    this.change(window, cx, |config| config.printer_target = target);
                }
            }),
            favourites.on_event(window, cx, move |this: &mut Self, event, window, cx| {
                if left(&event) {
                    let ids = pos::parse_favorites(&this.favourites.text(cx));
                    this.change(window, cx, |config| config.favorite_product_ids = ids);
                }
            }),
            document.on_change(window, cx, |this: &mut Self, chosen, window, cx| {
                if let Some(chosen) = chosen {
                    this.change(window, cx, |config| config.default_document = PosDocument::from_id(&chosen));
                }
            }),
            terminal.on_change(window, cx, |this: &mut Self, chosen, window, cx| {
                if let Some(chosen) = chosen {
                    this.change(window, cx, |config| config.tpe_mode = TpeMode::from_id(&chosen));
                }
            }),
        ];
        Self { printer, favourites, document, terminal, _subscriptions: subscriptions }
    }

    /// Change the role of this PC and keep it at once. Nothing is written when nothing changed.
    fn change(&mut self, window: &mut Window, cx: &mut Context<Self>, edit: impl FnOnce(&mut PosConfig)) {
        let state = PosState::global(cx);
        let before = state.read(cx).config.clone();
        let mut after = before.clone();
        edit(&mut after);
        if after == before {
            return;
        }
        match state.update(cx, |state, cx| state.save(after, cx)) {
            Ok(()) => toast::success("Enregistré sur cet appareil.", window, cx),
            Err(error) => toast::error(format!("Ce réglage n'a pas pu être enregistré : {error}"), window, cx),
        }
    }
}

impl Render for TillSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = cx.ui().m;
        let config = PosState::global(cx).read(cx).config.clone();
        let (role, drawer) = (cx.entity(), cx.entity());
        col()
            .max_w(px(640.))
            .gap(m.space_4)
            .child(Alert::info().child("Ces réglages restent sur cet ordinateur. Les activer n'affecte pas les autres postes de l'atelier."))
            .child(
                Switch::new("till-enabled", config.enabled)
                    .label("Utiliser cet appareil comme caisse")
                    .hint("Ouvre l'encaissement en plein écran, dès maintenant et à chaque démarrage.")
                    .on_change(move |on, window, cx| role.update(cx, |this, cx| this.change(window, cx, |config| config.enabled = on))),
            )
            .child(FormField::new("Document par défaut").child(Select::new(&self.document)))
            .child(FormField::new("Terminal de paiement").child(Select::new(&self.terminal)))
            .child(
                FormField::new("Imprimante ticket")
                    .hint("Port COM (COM3) ou chemin RAW Windows. Vide : le ticket est montré à l'écran, rien n'est imprimé.")
                    .child(Input::new(&self.printer)),
            )
            .child(
                Switch::new("till-drawer-on-cash", config.drawer_kick_on_cash)
                    .label("Ouvrir le tiroir sur un encaissement espèces")
                    .on_change(move |on, window, cx| drawer.update(cx, |this, cx| this.change(window, cx, |config| config.drawer_kick_on_cash = on))),
            )
            .child(
                FormField::new("Articles favoris")
                    .hint("Identifiants séparés par une virgule. Ils remplissent les achats fréquents tant que rien n'a été vendu.")
                    .child(Input::new(&self.favourites)),
            )
            .when(config.enabled, |this| {
                this.child(
                    row().child(
                        Button::new("till-enter")
                            .primary()
                            .icon(IconName::Store)
                            .label("Ouvrir la caisse maintenant")
                            .on_click(|_, _, cx| PosState::global(cx).update(cx, |pos, cx| pos.enter_till(cx))),
                    ),
                )
            })
    }
}
