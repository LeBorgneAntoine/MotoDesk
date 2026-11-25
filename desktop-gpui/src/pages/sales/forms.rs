//! The forms of the sales screen (ports of `SaleForm.svelte` and `PaymentPanel.svelte`).
//!
//! A document is edited as text, because a half-typed number is not a number; it is turned
//! into whole cents and thousandths when it is totalled and when it is sent. The totals on
//! screen come from `motodesk_fiscal::vat`, the arithmetic the service seals with.

use std::rc::Rc;

use motodesk_client::clients::{Vehicle, format_vehicle_label};
use motodesk_client::products::{ProductListItem, VAT_PRESETS};
use motodesk_client::sales::{DraftStatus, PAYMENT_METHODS, PaymentInput, Sale, SaleInput, SaleLine, SaleLineInput};
use motodesk_fiscal::vat::{self, DocumentTotals, LineInput};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::SalesPage;
use crate::format;

/// "Nothing chosen" in a list.
const NONE: &str = "-";

// ---- what is typed, as numbers -----------------------------------------------------------------

/// "1 234,56" → 1234.56. What is not a number counts as nothing.
fn to_number(text: &str) -> f64 {
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == ',' { '.' } else { c }).collect();
    cleaned.parse::<f64>().ok().filter(|value| value.is_finite()).unwrap_or(0.)
}

pub fn to_cents(text: &str) -> i64 {
    (to_number(text) * 100.).round() as i64
}

fn to_milli(text: &str) -> i64 {
    (to_number(text) * 1000.).round() as i64
}

/// A percentage → hundredths of a percent: "5,5" → 550.
fn to_bp(text: &str) -> i64 {
    (to_number(text) * 100.).round() as i64
}

/// Cents → what goes back into an amount field: "1234,50".
pub fn from_cents(cents: i64) -> String {
    format!("{:.2}", cents as f64 / 100.).replace('.', ",")
}

/// A plain decimal with a comma and no padding: 1500 thousandths → "1,5".
fn from_scaled(value: i64, scale: f64) -> String {
    (value as f64 / scale).to_string().replace('.', ",")
}

/// "20 %", "5,5 %".
pub fn percent(bp: i64) -> String {
    format!("{} %", from_scaled(bp, 100.))
}

// ---- one line ----------------------------------------------------------------------------------

struct LineDraft {
    /// Tells the lines apart while they are added and removed.
    uid: usize,
    product: SelectModel,
    label: TextModel,
    quantity: TextModel,
    unit_price: TextModel,
    discount: TextModel,
    vat: SelectModel,
    /// Set while the line carries a shelf price (picked from the catalogue and not retyped).
    /// It is then the price of record, and the HT field only shows its rounded figure:
    /// 9,99 € TTC has no HT price in whole cents.
    shelf_price: Option<i64>,
    /// What the HT field showed when the shelf price was set: typing anything else drops it.
    shelf_text: String,
    _subscriptions: Vec<Subscription>,
}

/// A line as text: what "has anything changed?" compares.
type LineSnapshot = (Option<String>, String, String, String, Option<i64>, String, String);

impl LineDraft {
    fn product_id(&self, cx: &App) -> Option<String> {
        self.product.value(cx).filter(|id| id != NONE && !id.is_empty())
    }

    fn vat_bp(&self, cx: &App) -> i64 {
        self.vat.value(cx).and_then(|bp| bp.parse().ok()).unwrap_or(2000)
    }

    fn snapshot(&self, cx: &App) -> LineSnapshot {
        (
            self.product_id(cx),
            self.label.text(cx),
            self.quantity.text(cx),
            self.unit_price.text(cx),
            self.shelf_price,
            self.vat.value(cx).unwrap_or_default(),
            self.discount.text(cx),
        )
    }

    /// The line as the totals engine reads it.
    fn priced(&self, cx: &App) -> LineInput {
        LineInput {
            quantity_milli: to_milli(&self.quantity.text(cx)),
            unit_price_ht_cents: to_cents(&self.unit_price.text(cx)),
            unit_price_ttc_cents: self.shelf_price,
            discount_bp: to_bp(&self.discount.text(cx)),
            vat_rate_bp: self.vat_bp(cx),
        }
    }

    /// A line the user started but left empty should not reach the service.
    fn is_filled(&self, cx: &App) -> bool {
        self.label.trimmed(cx).is_some() || self.product_id(cx).is_some()
    }

    fn input(&self, cx: &App) -> SaleLineInput {
        let priced = self.priced(cx);
        SaleLineInput {
            product_id: self.product_id(cx),
            label: self.label.trimmed(cx),
            quantity_milli: priced.quantity_milli,
            unit_price_ht_cents: priced.unit_price_ht_cents,
            unit_price_ttc_cents: priced.unit_price_ttc_cents,
            vat_rate_bp: priced.vat_rate_bp,
            discount_bp: priced.discount_bp,
        }
    }
}

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
    lines: Vec<LineDraft>,
    next_uid: usize,
    /// A save was attempted with no line worth keeping.
    pub no_lines: bool,
    products: Rc<Vec<ProductListItem>>,
    default_vat_bp: i64,
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
            lines: Vec::new(),
            next_uid: 1,
            no_lines: false,
            products,
            default_vat_bp,
            started_as: Snapshot::default(),
            _subscriptions: Vec::new(),
        };
        for line in initial.map(|s| s.lines.as_slice()).unwrap_or_default() {
            form.push_line(Some(line), window, cx);
        }
        if form.lines.is_empty() {
            form.push_line(None, window, cx);
        }
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

    fn push_line(&mut self, stored: Option<&SaleLine>, window: &mut Window, cx: &mut Context<SalesPage>) {
        let uid = self.next_uid;
        self.next_uid += 1;

        let mut product_options = vec![SelectOption::new(NONE, "Ligne libre")];
        product_options.extend(self.products.iter().map(|p| SelectOption::new(p.id.clone(), p.name.clone())));
        let vat_bp = stored.map(|line| line.vat_rate_bp).unwrap_or(self.default_vat_bp);
        let mut vat_options: Vec<SelectOption> =
            VAT_PRESETS.iter().map(|(label, bp)| SelectOption::new(bp.to_string(), *label)).collect();
        // A rate that is none of the usual ones stays choosable, or saving would replace it.
        if !VAT_PRESETS.iter().any(|(_, bp)| *bp == vat_bp) {
            vat_options.push(SelectOption::new(vat_bp.to_string(), percent(vat_bp)));
        }
        let unit_price = stored.map(|line| from_cents(line.unit_price_ht_cents.unwrap_or(0))).unwrap_or_else(|| "0,00".into());
        let shelf_price = stored.and_then(SaleLine::shelf_price_cents);
        let text = |value: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<SalesPage>| {
            TextModel::builder().placeholder(placeholder).value(value).build(window, cx)
        };

        let mut line = LineDraft {
            uid,
            product: SelectModel::new(
                product_options,
                Some(stored.and_then(|line| line.product_id.as_deref()).unwrap_or(NONE)),
                window,
                cx,
            ),
            label: text(stored.and_then(|line| line.label.clone()).unwrap_or_default(), "Désignation", window, cx),
            quantity: text(stored.map(|line| from_scaled(line.quantity_milli, 1000.)).unwrap_or_else(|| "1".into()), "", window, cx),
            unit_price: text(unit_price.clone(), "", window, cx),
            discount: text(stored.map(|line| from_scaled(line.discount_bp, 100.)).unwrap_or_else(|| "0".into()), "", window, cx),
            vat: SelectModel::new(vat_options, Some(vat_bp.to_string().as_str()), window, cx),
            shelf_price,
            shelf_text: unit_price,
            _subscriptions: Vec::new(),
        };
        for field in [&line.label, &line.quantity, &line.discount] {
            line._subscriptions.push(field.on_event(window, cx, |page, event, _, cx| {
                if event == TextEvent::Changed {
                    page.form_changed(cx);
                }
            }));
        }
        line._subscriptions.push(line.unit_price.on_event(window, cx, move |page, event, _, cx| {
            if event == TextEvent::Changed {
                if let Some(form) = page.form.as_mut() {
                    form.price_typed(uid, cx);
                }
                page.form_changed(cx);
            }
        }));
        line._subscriptions.push(line.product.on_change(window, cx, move |page, _, window, cx| {
            if let Some(form) = page.form.as_mut() {
                form.apply_product(uid, window, cx);
            }
            page.form_changed(cx);
        }));
        line._subscriptions.push(line.vat.on_change(window, cx, |page, _, _, cx| page.form_changed(cx)));
        self.lines.push(line);
    }

    pub fn add_line(&mut self, window: &mut Window, cx: &mut Context<SalesPage>) {
        self.push_line(None, window, cx);
        self.no_lines = false;
        if let Some(line) = self.lines.last() {
            line.label.focus(window, cx);
        }
    }

    /// A document always shows one line to type in: removing the last one leaves a blank.
    pub fn remove_line(&mut self, uid: usize, window: &mut Window, cx: &mut Context<SalesPage>) {
        self.lines.retain(|line| line.uid != uid);
        if self.lines.is_empty() {
            self.push_line(None, window, cx);
        }
    }

    /// The HT price was typed over: the line is no longer priced at its shelf price.
    fn price_typed(&mut self, uid: usize, cx: &App) {
        if let Some(line) = self.lines.iter_mut().find(|line| line.uid == uid)
            && line.shelf_price.is_some()
            && line.unit_price.text(cx) != line.shelf_text
        {
            line.shelf_price = None;
        }
    }

    /// A product was picked: it fills the line, and every field stays editable after.
    fn apply_product(&mut self, uid: usize, window: &mut Window, cx: &mut App) {
        let Some(line) = self.lines.iter_mut().find(|line| line.uid == uid) else { return };
        let Some(product) = line.product_id(cx).and_then(|id| self.products.iter().find(|p| p.id == id)) else { return };
        line.label.set_text(product.name.clone(), window, cx);
        // The line's list offers the usual rates; a product at another one keeps the rate shown.
        line.vat.set_value(&product.vat_rate_bp.to_string(), window, cx);
        match product.selling_price_ttc_cents {
            Some(ttc) => {
                // Catalogue prices are TTC because that is what the shelf label says, and the
                // line keeps it that way: the customer pays the label.
                let ht = vat::ht_from_ttc(ttc, product.vat_rate_bp).unwrap_or(0);
                line.shelf_price = Some(ttc);
                line.shelf_text = from_cents(ht);
                line.unit_price.set_text(line.shelf_text.clone(), window, cx);
            }
            None => line.shelf_price = None,
        }
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
            lines: self.lines.iter().map(|line| line.snapshot(cx)).collect(),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.snapshot(cx) != self.started_as
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.client.focus(window, cx);
    }

    /// The totals of what is typed; `None` while a rate or a discount is out of range, so
    /// that nothing is shown rather than a wrong figure.
    fn totals(&self, cx: &App) -> Option<DocumentTotals> {
        let lines: Vec<LineInput> = self.lines.iter().map(|line| line.priced(cx)).collect();
        vat::document_totals(&lines).ok()
    }

    /// What to send, or `None` (and the warning shown) when no line is worth keeping.
    pub fn validated(&mut self, cx: &App) -> Option<SaleInput> {
        let lines: Vec<SaleLineInput> = self.lines.iter().filter(|line| line.is_filled(cx)).map(|line| line.input(cx)).collect();
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
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let totals = self.totals(cx);
        let has_client = self.client.value(cx).is_some();
        let weighted = |share: f32, field: FormField| div().flex_grow(share).flex_shrink(1.).flex_basis(px(0.)).min_w_0().child(field);
        // The columns of the line editor, shared by its header and its rows.
        let flexible = |share: f32| div().flex_grow(share).flex_shrink(1.).flex_basis(px(0.)).min_w_0();
        let fixed = |width: f32| div().flex_none().w(px(width));
        let heading = |cell: Div, text: &'static str, right: bool| {
            cell.when(right, |this| this.text_right()).fs_tight(m.fs_xs).text_color(c.text_muted).child(text)
        };

        let rows = self.lines.iter().enumerate().map(|(index, line)| {
            let uid = line.uid;
            let net = totals.as_ref().and_then(|t| t.lines.get(index)).map(|l| l.net_ht_cents).unwrap_or(0);
            row()
                .gap(m.space_2)
                .child(flexible(1.4).child(Select::new(&line.product).menu_width(px(340.))))
                .child(flexible(1.8).child(Input::new(&line.label)))
                .child(fixed(70.).child(Input::new(&line.quantity)))
                .child(fixed(100.).child(Input::new(&line.unit_price)))
                .child(fixed(80.).child(Input::new(&line.discount)))
                .child(fixed(96.).child(Select::new(&line.vat)))
                .child(fixed(100.).text_right().font_weight(FontWeight::MEDIUM).child(format::euros(net)))
                .child(fixed(28.).child(
                    IconButton::new(("line-remove", uid), IconName::Trash).title("Supprimer la ligne").danger().on_click(
                        cx.listener(move |page, _, window, cx| {
                            if let Some(form) = page.form.as_mut() {
                                form.remove_line(uid, window, cx);
                            }
                            page.form_changed(cx);
                        }),
                    ),
                ))
        });

        let total_line = |label: String, amount: i64, grand: bool| {
            row()
                .justify_between()
                .gap(m.space_4)
                .when(grand, |this| {
                    this.pt(m.space_2)
                        .hairline_top(c.border)
                        .fs(m.fs_lg)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c.text_strong)
                })
                .when(!grand, |this| this.fs(m.fs_sm).text_color(c.text_muted))
                .child(label)
                .child(format::euros(amount))
        };
        let totals_block = match totals.as_ref() {
            Some(t) => col()
                .gap(m.space_1)
                .children(t.buckets.iter().map(|bucket| {
                    total_line(
                        format!("TVA {} sur {}", percent(bucket.rate_bp), format::euros(bucket.base_ht_cents)),
                        bucket.vat_cents,
                        false,
                    )
                }))
                .child(total_line("Total HT".into(), t.total_ht_cents, false))
                .child(total_line("Total TTC".into(), t.total_ttc_cents, true)),
            None => col().fs(m.fs_sm).text_color(c.text_muted).child("Une remise ou un taux est hors limites : les totaux reviennent dès qu'il est corrigé."),
        };

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
            .child(
                col()
                    .gap(m.space_2)
                    .child(
                        row()
                            .justify_between()
                            .child(div().fs(m.fs_md).font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child("Lignes"))
                            .child(Button::new("line-add").small().icon(IconName::Plus).label("Ajouter une ligne").on_click(
                                cx.listener(|page, _, window, cx| {
                                    if let Some(form) = page.form.as_mut() {
                                        form.add_line(window, cx);
                                    }
                                    page.form_changed(cx);
                                }),
                            )),
                    )
                    .when(self.no_lines, |this| {
                        this.child(
                            Alert::warning().child("Ajoutez au moins une ligne : un document sans ligne ne peut pas être validé."),
                        )
                    })
                    .child(
                        row()
                            .gap(m.space_2)
                            .child(heading(flexible(1.4), "Article", false))
                            .child(heading(flexible(1.8), "Désignation", false))
                            .child(heading(fixed(70.), "Qté", false))
                            .child(heading(fixed(100.), "P.U. HT", false))
                            .child(heading(fixed(80.), "Remise %", false))
                            .child(heading(fixed(96.), "TVA", false))
                            .child(heading(fixed(100.), "Total HT", true))
                            .child(fixed(28.)),
                    )
                    .children(rows),
            )
            .child(row().justify_end().child(div().w(px(320.)).child(totals_block)))
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

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use super::{from_cents, from_scaled, percent, to_bp, to_cents, to_milli};

    #[test]
    fn what_is_typed_the_french_way_becomes_whole_numbers() {
        assert_eq!(to_cents("1 234,56"), 123_456);
        assert_eq!(to_cents("9.99"), 999);
        assert_eq!(to_cents("-12,5"), -1_250);
        assert_eq!(to_cents(""), 0);
        assert_eq!(to_cents("douze"), 0);
        assert_eq!(to_milli("1,5"), 1_500);
        assert_eq!(to_milli("0,3333"), 333);
        assert_eq!(to_bp("5,5"), 550);
        assert_eq!(to_bp("10"), 1_000);
    }

    #[test]
    fn whole_numbers_go_back_into_fields_the_french_way() {
        assert_eq!(from_cents(123_450), "1234,50");
        assert_eq!(from_cents(0), "0,00");
        assert_eq!(from_cents(-999), "-9,99");
        assert_eq!(from_scaled(1_500, 1000.), "1,5");
        assert_eq!(from_scaled(2_000, 1000.), "2");
        assert_eq!(from_scaled(550, 100.), "5,5");
        assert_eq!(percent(2000), "20 %");
        assert_eq!(percent(550), "5,5 %");
    }

    #[test]
    fn a_field_is_read_back_as_it_was_written() {
        for cents in [0, 1, 999, 123_450, 9_720] {
            assert_eq!(to_cents(&from_cents(cents)), cents);
        }
        for milli in [1_000, 1_500, 250, 12_345] {
            assert_eq!(to_milli(&from_scaled(milli, 1000.)), milli);
        }
    }
}
