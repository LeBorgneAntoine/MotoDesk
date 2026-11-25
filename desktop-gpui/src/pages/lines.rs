//! The line editor of a document: the same for a sale (`SaleForm.svelte`) and for a repair
//! order (`WorkshopForm.svelte`), which in Svelte each carried a copy of it.
//!
//! Lines are edited as text, because a half-typed number is not a number; they are turned
//! into whole cents and thousandths when they are totalled and when they are sent. The totals
//! on screen come from `motodesk_fiscal::vat`, the arithmetic the service seals with.

use std::rc::Rc;

use motodesk_client::products::{ProductListItem, VAT_PRESETS};
use motodesk_fiscal::vat::{self, DocumentTotals, LineInput};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::format;

/// "Nothing chosen" in a list.
pub const NONE: &str = "-";

// ---- what is typed, as numbers -----------------------------------------------------------------

/// "1 234,56" → 1234.56. What is not a number counts as nothing.
pub fn to_number(text: &str) -> f64 {
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

// ---- a line, in and out ------------------------------------------------------------------------

/// A line as a document stores it and as the editor hands it back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    /// The stored line this one is; `None` for a line added in the editor.
    pub id: Option<String>,
    pub product_id: Option<String>,
    pub label: Option<String>,
    pub quantity_milli: i64,
    pub unit_price_ht_cents: i64,
    /// A shelf price: when set it is the price of record, and the HT price a display figure.
    pub shelf_price_cents: Option<i64>,
    pub vat_rate_bp: i64,
    pub discount_bp: i64,
}

/// A line as text: what "has anything changed?" compares.
pub type LineSnapshot = (Option<String>, String, String, String, Option<i64>, String, String);

/// The view that holds the editor: it is told when a line changes.
pub trait LinesHost: Sized + 'static {
    /// The editor on screen, if there is one.
    fn lines_mut(&mut self) -> Option<&mut LineEditor>;
    /// A line changed, was added or removed: update the "unsaved edits" mark, redraw.
    fn lines_changed(&mut self, cx: &mut Context<Self>);
}

struct LineDraft {
    /// Tells the lines apart while they are added and removed.
    uid: usize,
    id: Option<String>,
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

    fn line(&self, cx: &App) -> Line {
        let priced = self.priced(cx);
        Line {
            id: self.id.clone(),
            product_id: self.product_id(cx),
            label: self.label.trimmed(cx),
            quantity_milli: priced.quantity_milli,
            unit_price_ht_cents: priced.unit_price_ht_cents,
            shelf_price_cents: priced.unit_price_ttc_cents,
            vat_rate_bp: priced.vat_rate_bp,
            discount_bp: priced.discount_bp,
        }
    }
}

// ---- the editor --------------------------------------------------------------------------------

pub struct LineEditor {
    lines: Vec<LineDraft>,
    next_uid: usize,
    products: Rc<Vec<ProductListItem>>,
    default_vat_bp: i64,
    /// The workshop's hourly rate, HT: the price of a service that has none of its own.
    labour_rate_ht_cents: Option<i64>,
}

impl LineEditor {
    /// `stored` are the lines of the document being edited. An editor always shows one line
    /// to type in: without any, it starts with a blank.
    pub fn new<H: LinesHost>(
        stored: Vec<Line>,
        products: Rc<Vec<ProductListItem>>,
        default_vat_bp: i64,
        window: &mut Window,
        cx: &mut Context<H>,
    ) -> Self {
        let mut editor = Self { lines: Vec::new(), next_uid: 1, products, default_vat_bp, labour_rate_ht_cents: None };
        for line in stored {
            editor.push_line(Some(line), window, cx);
        }
        if editor.lines.is_empty() {
            editor.push_line(None, window, cx);
        }
        editor
    }

    /// A service picked from the catalogue without a price of its own is labour, priced by
    /// the hour at this rate.
    pub fn set_labour_rate(&mut self, rate_ht_cents: Option<i64>) {
        self.labour_rate_ht_cents = rate_ht_cents;
    }

    fn push_line<H: LinesHost>(&mut self, stored: Option<Line>, window: &mut Window, cx: &mut Context<H>) {
        let uid = self.next_uid;
        self.next_uid += 1;
        let stored = stored.as_ref();

        let mut product_options = vec![SelectOption::new(NONE, "Ligne libre")];
        product_options.extend(self.products.iter().map(|p| SelectOption::new(p.id.clone(), p.name.clone())));
        let vat_bp = stored.map(|line| line.vat_rate_bp).unwrap_or(self.default_vat_bp);
        let mut vat_options: Vec<SelectOption> =
            VAT_PRESETS.iter().map(|(label, bp)| SelectOption::new(bp.to_string(), *label)).collect();
        // A rate that is none of the usual ones stays choosable, or saving would replace it.
        if !VAT_PRESETS.iter().any(|(_, bp)| *bp == vat_bp) {
            vat_options.push(SelectOption::new(vat_bp.to_string(), percent(vat_bp)));
        }
        let unit_price = stored.map(|line| from_cents(line.unit_price_ht_cents)).unwrap_or_else(|| "0,00".into());
        let text = |value: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<H>| {
            TextModel::builder().placeholder(placeholder).value(value).build(window, cx)
        };

        let mut line = LineDraft {
            uid,
            id: stored.and_then(|line| line.id.clone()),
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
            shelf_price: stored.and_then(|line| line.shelf_price_cents),
            shelf_text: unit_price,
            _subscriptions: Vec::new(),
        };
        for field in [&line.label, &line.quantity, &line.discount] {
            line._subscriptions.push(field.on_event(window, cx, |host: &mut H, event, _, cx| {
                if event == TextEvent::Changed {
                    host.lines_changed(cx);
                }
            }));
        }
        line._subscriptions.push(line.unit_price.on_event(window, cx, move |host: &mut H, event, _, cx| {
            if event == TextEvent::Changed {
                if let Some(editor) = host.lines_mut() {
                    editor.price_typed(uid, cx);
                }
                host.lines_changed(cx);
            }
        }));
        line._subscriptions.push(line.product.on_change(window, cx, move |host: &mut H, _, window, cx| {
            if let Some(editor) = host.lines_mut() {
                editor.apply_product(uid, window, cx);
            }
            host.lines_changed(cx);
        }));
        line._subscriptions.push(line.vat.on_change(window, cx, |host: &mut H, _, _, cx| host.lines_changed(cx)));
        self.lines.push(line);
    }

    fn add_line<H: LinesHost>(&mut self, window: &mut Window, cx: &mut Context<H>) {
        self.push_line(None, window, cx);
        if let Some(line) = self.lines.last() {
            line.label.focus(window, cx);
        }
    }

    /// Removing the last line leaves a blank one to type in.
    fn remove_line<H: LinesHost>(&mut self, uid: usize, window: &mut Window, cx: &mut Context<H>) {
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
        match (product.selling_price_ttc_cents, self.labour_rate_ht_cents) {
            (Some(ttc), _) => {
                // The product's own price wins, service or part. Catalogue prices are TTC
                // because that is what the shelf label says, and the line keeps it that way:
                // the customer pays the label.
                let ht = vat::ht_from_ttc(ttc, product.vat_rate_bp).unwrap_or(0);
                line.shelf_price = Some(ttc);
                line.shelf_text = from_cents(ht);
                line.unit_price.set_text(line.shelf_text.clone(), window, cx);
            }
            (None, Some(rate)) if product.is_service => {
                // A service with no price of its own is labour, priced by the hour, HT.
                line.shelf_price = None;
                line.unit_price.set_text(from_cents(rate), window, cx);
            }
            (None, _) => line.shelf_price = None,
        }
    }

    pub fn snapshot(&self, cx: &App) -> Vec<LineSnapshot> {
        self.lines.iter().map(|line| line.snapshot(cx)).collect()
    }

    /// The totals of what is typed; `None` while a rate or a discount is out of range, so
    /// that nothing is shown rather than a wrong figure.
    pub fn totals(&self, cx: &App) -> Option<DocumentTotals> {
        let lines: Vec<LineInput> = self.lines.iter().map(|line| line.priced(cx)).collect();
        vat::document_totals(&lines).ok()
    }

    /// The lines worth keeping: those with a name or a product.
    pub fn filled(&self, cx: &App) -> Vec<Line> {
        self.lines.iter().filter(|line| line.is_filled(cx)).map(|line| line.line(cx)).collect()
    }

    /// The heading, the "add" button, the column names and one row per line. `warning` is
    /// shown above the rows when it is set.
    pub fn render<H: LinesHost>(&self, title: &'static str, warning: Option<&'static str>, cx: &mut Context<H>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let totals = self.totals(cx);
        // The columns, shared by the header and the rows.
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
                // The column is too narrow to read a product's name: the list opens wider.
                .child(flexible(1.4).child(Select::new(&line.product).menu_width(px(340.))))
                .child(flexible(1.8).child(Input::new(&line.label)))
                .child(fixed(70.).child(Input::new(&line.quantity)))
                .child(fixed(100.).child(Input::new(&line.unit_price)))
                .child(fixed(80.).child(Input::new(&line.discount)))
                .child(fixed(96.).child(Select::new(&line.vat)))
                .child(fixed(100.).text_right().font_weight(FontWeight::MEDIUM).child(format::euros(net)))
                .child(fixed(28.).child(
                    IconButton::new(("line-remove", uid), IconName::Trash).title("Supprimer la ligne").danger().on_click(
                        cx.listener(move |host: &mut H, _, window, cx| {
                            if let Some(editor) = host.lines_mut() {
                                editor.remove_line(uid, window, cx);
                            }
                            host.lines_changed(cx);
                        }),
                    ),
                ))
        });

        col()
            .gap(m.space_2)
            .child(
                row()
                    .justify_between()
                    .child(div().fs(m.fs_md).font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(title))
                    .child(Button::new("line-add").small().icon(IconName::Plus).label("Ajouter une ligne").on_click(
                        cx.listener(|host: &mut H, _, window, cx| {
                            if let Some(editor) = host.lines_mut() {
                                editor.add_line(window, cx);
                            }
                            host.lines_changed(cx);
                        }),
                    )),
            )
            .children(warning.map(|text| Alert::warning().child(text)))
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
            .children(rows)
    }

    /// The totals under the lines. `by_rate` adds one line of VAT per rate, as an invoice
    /// must show it.
    pub fn totals_block(&self, by_rate: bool, cx: &App) -> Div {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
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
        let block = match self.totals(cx) {
            Some(t) => col()
                .gap(m.space_1)
                .children(t.buckets.iter().filter(|_| by_rate).map(|bucket| {
                    total_line(
                        format!("TVA {} sur {}", percent(bucket.rate_bp), format::euros(bucket.base_ht_cents)),
                        bucket.vat_cents,
                        false,
                    )
                }))
                .child(total_line("Total HT".into(), t.total_ht_cents, false))
                .child(total_line("Total TTC".into(), t.total_ttc_cents, true)),
            None => col()
                .fs(m.fs_sm)
                .text_color(c.text_muted)
                .child("Une remise ou un taux est hors limites : les totaux reviennent dès qu'il est corrigé."),
        };
        row().justify_end().child(div().w(px(320.)).child(block))
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
