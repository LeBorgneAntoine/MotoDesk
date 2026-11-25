//! The forms of the sales screen (ports of `SaleForm.svelte` and `PaymentPanel.svelte`).
//!
//! The lines of a document are edited by the editor shared with the workshop's repair orders:
//! see `pages/lines.rs`.

use std::rc::Rc;

use motodesk_client::clients::{Vehicle, format_vehicle_label};
use motodesk_client::products::ProductListItem;
use motodesk_client::sales::{DraftStatus, PAYMENT_METHODS, PaymentInput, Sale, SaleInput, SaleLineInput};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::SalesPage;
use crate::format;
use crate::pages::lines::{Line, LineEditor, LineSnapshot, NONE, from_cents, to_cents};

// ---- the document ------------------------------------------------------------------------------

/// The form as text: what "has anything changed?" compares.
#[derive(Debug, Clone, Default, PartialEq)]
struct Snapshot {
    client: Option<String>,
    vehicle: Option<String>,
    date: String,
    notes: String,
    kind: String,
    lines: Vec<LineSnapshot>,
}

pub struct SaleForm {
    pub client: ComboModel,
    pub client_searches: usize,
    vehicle: SelectModel,
    has_vehicles: bool,
    date: TextModel,
    kind: SelectModel,
    notes: TextModel,
    pub lines: LineEditor,
    /// A save was attempted with no line worth keeping.
    no_lines: bool,
    started_as: Snapshot,
    _subscriptions: Vec<Subscription>,
}

fn vehicle_options(vehicles: &[Vehicle]) -> Vec<SelectOption> {
    let nothing = if vehicles.is_empty() { "Aucun véhicule" } else { "Pas de véhicule lié" };
    let mut options = vec![SelectOption::new(NONE, nothing)];
    options.extend(
        vehicles.iter().map(|v| SelectOption::new(v.id.clone(), format_vehicle_label(&v.make, &v.model, v.year))),
    );
    options
}

impl SaleForm {
    /// `initial` is the document being edited (`None` for a new one) and `client`, its
    /// customer as the field shows it.
    pub fn new(
        initial: Option<&Sale>,
        client: Option<ComboOption>,
        products: Rc<Vec<ProductListItem>>,
        default_vat_bp: i64,
        window: &mut Window,
        cx: &mut Context<SalesPage>,
    ) -> Self {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let date = initial.and_then(|s| s.sale_date.as_deref()).map(|d| d.chars().take(10).collect()).unwrap_or(today);
        let quote = initial.is_some_and(Sale::is_quote);

        let client_field = ComboModel::new("Client de passage — nom, téléphone, courriel…", window, cx);
        let (on_query, on_pick) = (cx.entity().downgrade(), cx.entity().downgrade());
        client_field.on_query(cx, move |query, _, cx| {
            let query = query.to_string();
            let _ = on_query.update(cx, |page, cx| page.search_clients(query, cx));
        });
        client_field.on_pick(cx, move |_, window, cx| {
            let _ = on_pick.update(cx, |page, cx| page.client_changed(window, cx));
        });
        if let Some(client) = client {
            client_field.set_picked(client, window, cx);
        }

        let stored = initial
            .map(|sale| sale.lines.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|line| Line {
                id: Some(line.id.clone()),
                product_id: line.product_id.clone(),
                label: line.label.clone(),
                quantity_milli: line.quantity_milli,
                unit_price_ht_cents: line.unit_price_ht_cents.unwrap_or(0),
                shelf_price_cents: line.shelf_price_cents(),
                vat_rate_bp: line.vat_rate_bp,
                discount_bp: line.discount_bp,
            })
            .collect();

        let mut form = Self {
            client: client_field,
            client_searches: 0,
            // Filled once the customer's vehicles are known.
            vehicle: SelectModel::new(vehicle_options(&[]), Some(NONE), window, cx),
            has_vehicles: false,
            date: TextModel::builder().placeholder("AAAA-MM-JJ").value(date).build(window, cx),
            kind: SelectModel::new(
                vec![SelectOption::new("draft", "Brouillon"), SelectOption::new("quote", "Devis")],
                Some(if quote { "quote" } else { "draft" }),
                window,
                cx,
            ),
            notes: TextModel::builder()
                .placeholder("Conditions particulières…")
                .value(initial.and_then(|s| s.notes.clone()).unwrap_or_default())
                .multiline(2)
                .build(window, cx),
            lines: LineEditor::new(stored, products, default_vat_bp, window, cx),
            no_lines: false,
            started_as: Snapshot::default(),
            _subscriptions: Vec::new(),
        };
        // An untouched new document is not "changed" by the blank line it starts with.
        form.started_as = form.snapshot(cx);
        let watch = |field: &TextModel, window: &mut Window, cx: &mut Context<SalesPage>| {
            field.on_event(window, cx, |page, event, _, cx| {
                if event == TextEvent::Changed {
                    page.form_changed(cx);
                }
            })
        };
        form._subscriptions = vec![
            watch(&form.date, window, cx),
            watch(&form.notes, window, cx),
            form.kind.on_change(window, cx, |page, _, _, cx| page.form_changed(cx)),
        ];
        form
    }

    pub fn chosen_vehicle(&self, cx: &App) -> Option<String> {
        self.vehicle.value(cx).filter(|id| id != NONE && !id.is_empty())
    }

    /// The customer's vehicles arrived (none when there is no customer). `keep` is the vehicle
    /// to stay on, when it is still one of them.
    pub fn set_vehicles(&mut self, vehicles: &[Vehicle], keep: Option<String>, window: &mut Window, cx: &mut Context<SalesPage>) {
        let keep = keep.filter(|id| vehicles.iter().any(|v| &v.id == id));
        self.has_vehicles = !vehicles.is_empty();
        self.vehicle = SelectModel::new(vehicle_options(vehicles), Some(keep.as_deref().unwrap_or(NONE)), window, cx);
        self._subscriptions.push(self.vehicle.on_change(window, cx, |page, _, _, cx| page.form_changed(cx)));
    }

    /// Called once the form shows the document as it is stored: from here on, a difference is
    /// an edit.
    pub fn mark_clean(&mut self, cx: &App) {
        self.started_as = self.snapshot(cx);
    }

    fn snapshot(&self, cx: &App) -> Snapshot {
        Snapshot {
            client: self.client.value(cx),
            vehicle: self.chosen_vehicle(cx),
            date: self.date.text(cx),
            notes: self.notes.text(cx),
            kind: self.kind.value(cx).unwrap_or_default(),
            lines: self.lines.snapshot(cx),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.snapshot(cx) != self.started_as
    }

    /// The complaint about lines goes as soon as there is one worth keeping.
    pub fn recheck(&mut self, cx: &App) {
        if self.no_lines && !self.lines.filled(cx).is_empty() {
            self.no_lines = false;
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.client.focus(window, cx);
    }

    /// What to send, or `None` (and the warning shown) when no line is worth keeping.
    pub fn validated(&mut self, cx: &App) -> Option<SaleInput> {
        let lines: Vec<SaleLineInput> = self
            .lines
            .filled(cx)
            .into_iter()
            .map(|line| SaleLineInput {
                product_id: line.product_id,
                label: line.label,
                quantity_milli: line.quantity_milli,
                unit_price_ht_cents: line.unit_price_ht_cents,
                unit_price_ttc_cents: line.shelf_price_cents,
                vat_rate_bp: line.vat_rate_bp,
                discount_bp: line.discount_bp,
            })
            .collect();
        self.no_lines = lines.is_empty();
        if lines.is_empty() {
            return None;
        }
        Some(SaleInput {
            client_id: self.client.value(cx),
            vehicle_id: self.chosen_vehicle(cx),
            sale_date: self.date.trimmed(cx),
            notes: self.notes.trimmed(cx),
            status: if self.kind.value(cx).as_deref() == Some("quote") { DraftStatus::Quote } else { DraftStatus::Draft },
            lines,
        })
    }

    pub fn render(&self, error: Option<SharedString>, cx: &mut Context<SalesPage>) -> Div {
        let m = cx.ui().m;
        let has_client = self.client.value(cx).is_some();
        let weighted = |share: f32, field: FormField| div().flex_grow(share).flex_shrink(1.).flex_basis(px(0.)).min_w_0().child(field);
        let warning =
            self.no_lines.then_some("Ajoutez au moins une ligne : un document sans ligne ne peut pas être validé.");

        col()
            .gap(m.space_4)
            .children(error.map(|e| Alert::danger().child(e)))
            .child(
                row()
                    .gap(m.space_3)
                    .items_start()
                    .child(weighted(1.4, FormField::new("Client").child(Combobox::new(&self.client))))
                    .child(weighted(
                        1.4,
                        FormField::new("Véhicule").child(Select::new(&self.vehicle).disabled(!has_client || !self.has_vehicles)),
                    ))
                    .child(weighted(1., FormField::new("Date").child(Input::new(&self.date))))
                    .child(weighted(1., FormField::new("Type").child(Select::new(&self.kind)))),
            )
            .child(self.lines.render("Lignes", warning, cx))
            .child(self.lines.totals_block(true, cx))
            .child(FormField::new("Notes").hint("Reprises sur le document imprimé.").child(Textarea::new(&self.notes)))
    }
}

// ---- a payment ---------------------------------------------------------------------------------

/// The receipt being recorded over a sealed document.
pub struct PaymentForm {
    /// What is still owed when the panel opens.
    pub due_cents: i64,
    amount: TextModel,
    method: SelectModel,
    bank: TextModel,
    drawer: TextModel,
    notes: TextModel,
    pub amount_error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl PaymentForm {
    pub fn new(due_cents: i64, window: &mut Window, cx: &mut Context<SalesPage>) -> Self {
        // Seeded once with what is owed: the field belongs to the user from the first keystroke.
        let amount = TextModel::builder().value(from_cents(due_cents)).build(window, cx);
        let method = SelectModel::new(
            PAYMENT_METHODS.iter().map(|(value, label)| SelectOption::new(*value, *label)).collect(),
            Some("cash"),
            window,
            cx,
        );
        let subscriptions = vec![
            amount.on_event(window, cx, |page, event, window, cx| match event {
                TextEvent::Changed => page.payment_changed(cx),
                TextEvent::Submitted { .. } => page.save_payment(window, cx),
                _ => {}
            }),
            method.on_change(window, cx, |_, _, _, cx| cx.notify()),
        ];
        Self {
            due_cents,
            amount,
            method,
            bank: TextModel::builder().placeholder("Crédit Agricole").build(window, cx),
            drawer: TextModel::builder().placeholder("Nom porté sur le chèque").build(window, cx),
            notes: TextModel::builder().placeholder("Référence, remarque…").multiline(2).build(window, cx),
            amount_error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.amount.focus(window, cx);
    }

    pub fn cents(&self, cx: &App) -> i64 {
        to_cents(&self.amount.text(cx))
    }

    pub fn pay_in_full(&mut self, window: &mut Window, cx: &mut App) {
        self.amount.set_text(from_cents(self.due_cents), window, cx);
        self.amount_error = None;
    }

    fn method(&self, cx: &App) -> String {
        self.method.value(cx).unwrap_or_else(|| "cash".into())
    }

    /// What to send, or `None` (and the amount marked) when the amount cannot be a receipt.
    pub fn validated(&mut self, cx: &App) -> Option<PaymentInput> {
        let cents = self.cents(cx);
        self.amount_error = if cents == 0 {
            Some("Indiquez un montant.".into())
        } else if cents < 0 {
            Some("Un encaissement est positif ; utilisez l'annulation pour le reprendre.".into())
        } else if cents > self.due_cents {
            Some(
                format!(
                    "Le montant dépasse le solde restant ({}). Saisissez la somme réellement encaissée.",
                    format::euros(self.due_cents)
                )
                .into(),
            )
        } else {
            None
        };
        if self.amount_error.is_some() {
            return None;
        }
        let method = self.method(cx);
        let with_bank = method == "cheque" || method == "transfer";
        Some(PaymentInput {
            amount_cents: cents,
            bank: self.bank.trimmed(cx).filter(|_| with_bank),
            cheque_drawer: self.drawer.trimmed(cx).filter(|_| method == "cheque"),
            notes: self.notes.trimmed(cx),
            method,
        })
    }

    pub fn render(&self, error: Option<SharedString>, cx: &mut Context<SalesPage>) -> Div {
        let m = cx.ui().m;
        let method = self.method(cx);
        let remaining = self.due_cents - self.cents(cx);
        let hint = if remaining > 0 {
            format!("Restera {} à régler.", format::euros(remaining))
        } else if remaining < 0 {
            format!("Dépasse le solde de {} : saisissez la somme réellement encaissée.", format::euros(-remaining))
        } else {
            "Solde le document.".to_string()
        };
        col()
            .gap(m.space_3)
            .children(error.map(|e| Alert::danger().child(e)))
            .child(
                FormField::new("Montant reçu").required().error(self.amount_error.clone()).hint(hint).child(
                    row()
                        .gap(m.space_2)
                        .child(div().flex_1().min_w_0().child(Input::new(&self.amount).invalid(self.amount_error.is_some())))
                        .child(Button::new("payment-full").label("Solder").on_click(cx.listener(|page, _, window, cx| {
                            if let Some(form) = page.payment.as_mut() {
                                form.pay_in_full(window, cx);
                            }
                            page.payment_changed(cx);
                        }))),
                ),
            )
            .child(FormField::new("Mode de règlement").child(Select::new(&self.method)))
            .when(method == "cheque" || method == "transfer", |this| {
                this.child(FormField::new("Banque").child(Input::new(&self.bank)))
            })
            .when(method == "cheque", |this| this.child(FormField::new("Émetteur du chèque").child(Input::new(&self.drawer))))
            .child(FormField::new("Notes").child(Textarea::new(&self.notes)))
    }
}
