//! The customer form (port of `ClientForm.svelte`). The vehicle form is shared with Parc VN/VO:
//! see `pages/vehicle.rs`.
//!
//! A form owns its fields, knows whether they differ from what it started with, and turns
//! them into what the API takes. It is created when its panel opens and dropped when it closes.

use motodesk_client::clients::{ClientDetail, ClientInput};
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::ClientsPage;
use crate::format::{cents_to_text, euros_to_cents};

fn field(placeholder: &'static str, value: Option<&str>, window: &mut Window, cx: &mut Context<ClientsPage>) -> TextModel {
    TextModel::builder().placeholder(placeholder).value(value.unwrap_or_default().to_string()).build(window, cx)
}

// ---- client ---------------------------------------------------------------------------------

pub struct ClientForm {
    first_name: TextModel,
    last_name: TextModel,
    phone: TextModel,
    email: TextModel,
    address: TextModel,
    notes: TextModel,
    credit_limit: TextModel,
    pub credit_blocked: bool,
    pub last_name_missing: bool,
    started_as: ClientInput,
    _subscriptions: Vec<Subscription>,
}

impl ClientForm {
    /// `initial` is the customer being edited; `None` for a new one.
    pub fn new(initial: Option<&ClientDetail>, window: &mut Window, cx: &mut Context<ClientsPage>) -> Self {
        let c = initial.map(|d| &d.client);
        let mut form = Self {
            first_name: field("Jean", c.and_then(|c| c.first_name.as_deref()), window, cx),
            last_name: field("Dupont", c.map(|c| c.last_name.as_str()), window, cx),
            phone: field("06 12 34 56 78", c.and_then(|c| c.phone.as_deref()), window, cx),
            email: field("jean.dupont@example.com", c.and_then(|c| c.email.as_deref()), window, cx),
            address: TextModel::builder()
                .placeholder("Numéro, rue, ville, code postal")
                .value(c.and_then(|c| c.address.clone()).unwrap_or_default())
                .multiline(2)
                .build(window, cx),
            notes: TextModel::builder()
                .placeholder("Préférences, remarques internes…")
                .value(c.and_then(|c| c.notes.clone()).unwrap_or_default())
                .multiline(3)
                .build(window, cx),
            credit_limit: field("Illimité", Some(&cents_to_text(c.and_then(|c| c.credit_limit_cents))), window, cx),
            credit_blocked: c.is_some_and(|c| c.credit_blocked),
            last_name_missing: false,
            started_as: ClientInput::default(),
            _subscriptions: Vec::new(),
        };
        form.started_as = form.input(cx);
        form._subscriptions = [&form.first_name, &form.last_name, &form.phone, &form.email, &form.address, &form.notes, &form.credit_limit]
            .into_iter()
            .map(|model| {
                model.on_event(window, cx, |page, event, window, cx| match event {
                    TextEvent::Changed => page.client_form_changed(cx),
                    TextEvent::Submitted { with_modifier: true } => page.submit_client(window, cx),
                    _ => {}
                })
            })
            .collect();
        form
    }

    pub fn input(&self, cx: &App) -> ClientInput {
        ClientInput {
            first_name: self.first_name.trimmed(cx),
            last_name: self.last_name.text(cx).trim().to_string(),
            phone: self.phone.trimmed(cx),
            email: self.email.trimmed(cx),
            address: self.address.trimmed(cx),
            notes: self.notes.trimmed(cx),
            credit_limit_cents: euros_to_cents(&self.credit_limit.text(cx)),
            credit_blocked: Some(self.credit_blocked),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.input(cx) != self.started_as
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.first_name.focus(window, cx);
    }

    pub fn render(&self, error: Option<SharedString>, cx: &mut Context<ClientsPage>) -> Div {
        let m = cx.ui().m;
        let page = cx.entity();
        let pair = |a: FormField, b: FormField| row().gap(m.space_3).items_start().child(div().flex_1().child(a)).child(div().flex_1().child(b));
        col()
            .gap(m.space_4)
            .children(error.map(|e| Alert::danger().child(e)))
            .child(pair(
                FormField::new("Prénom").child(Input::new(&self.first_name)),
                FormField::new("Nom")
                    .required()
                    .error(self.last_name_missing.then_some("Le nom est obligatoire."))
                    .child(Input::new(&self.last_name).invalid(self.last_name_missing)),
            ))
            .child(pair(
                FormField::new("Téléphone").child(Input::new(&self.phone)),
                FormField::new("Courriel").child(Input::new(&self.email)),
            ))
            .child(FormField::new("Adresse").child(Textarea::new(&self.address)))
            .child(FormField::new("Notes").hint("Visible uniquement par l'équipe.").child(Textarea::new(&self.notes)))
            .child(pair(
                FormField::new("Plafond d'encours TTC (€)").hint("Vide = illimité.").child(Input::new(&self.credit_limit)),
                FormField::new("Compte").hint("Refuse toute nouvelle facture ou vente à ce client.").child(
                    Checkbox::new("credit-blocked", self.credit_blocked).label("Bloquer l'encours").on_change(
                        move |on, _, cx| {
                            page.update(cx, |page, cx| {
                                if let Some(form) = page.client_form.as_mut() {
                                    form.credit_blocked = on;
                                }
                                page.client_form_changed(cx);
                            })
                        },
                    ),
                ),
            ))
    }
}
