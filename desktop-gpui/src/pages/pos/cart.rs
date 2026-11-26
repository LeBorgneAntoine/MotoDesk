//! The rules of the till's cart: what a line is, what it comes to, what is still owed,
//! and when a paper ticket is printed without being asked for.
//!
//! The amounts come from `motodesk_fiscal::vat`, the arithmetic the service seals with: what
//! the cart shows is what the ticket will say.

use motodesk_client::pos::{FrequentItem, PosPayment};
use motodesk_client::products::{self, Product, ProductListItem};
use motodesk_client::sales::{SaleLine, SaleLineInput};
use motodesk_fiscal::vat::{self, LineInput};

use crate::format;

/// An article as the till offers it, wherever it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct Article {
    pub id: String,
    pub name: String,
    pub sku: Option<String>,
    pub brand: Option<String>,
    pub variant: Option<String>,
    pub is_service: bool,
    pub price_ttc_cents: Option<i64>,
    pub vat_rate_bp: i64,
    pub stock_quantity: f64,
    pub low_stock_threshold: Option<i64>,
    pub is_active: bool,
}

impl From<&ProductListItem> for Article {
    fn from(p: &ProductListItem) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            sku: p.sku.clone(),
            brand: p.brand.clone(),
            variant: p.variant_label.clone(),
            is_service: p.is_service,
            price_ttc_cents: p.selling_price_ttc_cents,
            vat_rate_bp: p.vat_rate_bp,
            stock_quantity: p.stock_quantity,
            low_stock_threshold: p.low_stock_threshold,
            is_active: p.is_active,
        }
    }
}

impl From<&FrequentItem> for Article {
    fn from(p: &FrequentItem) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            sku: p.sku.clone(),
            brand: p.brand.clone(),
            variant: None,
            is_service: p.is_service,
            price_ttc_cents: p.selling_price_ttc_cents,
            vat_rate_bp: p.vat_rate_bp,
            stock_quantity: p.stock_quantity,
            low_stock_threshold: p.low_stock_threshold,
            // Only what is still sold is ranked.
            is_active: true,
        }
    }
}

impl From<&Product> for Article {
    fn from(p: &Product) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            sku: p.sku.clone(),
            brand: p.brand.clone(),
            variant: p.variant_label.clone(),
            is_service: p.is_service,
            price_ttc_cents: p.selling_price_ttc_cents,
            vat_rate_bp: p.vat_rate_bp,
            stock_quantity: p.stock_quantity,
            low_stock_threshold: p.low_stock_threshold,
            is_active: p.is_active,
        }
    }
}

impl Article {
    /// What tells two articles of the same name apart: variant, brand, reference.
    pub fn meta(&self) -> String {
        [&self.variant, &self.brand, &self.sku]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// Nothing left to sell. A service is never out; nor is anything when the shop allows
    /// selling below zero.
    pub fn out_of_stock(&self, allow_negative: bool) -> bool {
        !allow_negative && !self.is_service && self.stock_quantity <= 0.
    }

    /// "Rupture" or "Stock bas", and whether it is the first.
    pub fn stock_badge(&self, allow_negative: bool) -> Option<(&'static str, bool)> {
        if self.out_of_stock(allow_negative) {
            Some(("Rupture", true))
        } else if products::is_low_stock(self.is_service, self.stock_quantity, self.low_stock_threshold) {
            Some(("Stock bas", false))
        } else {
            None
        }
    }

    /// Whether the article answers a search typed at the till: by name, reference or brand.
    pub fn matches(&self, needle: &str) -> bool {
        let has = |text: &Option<String>| text.as_deref().is_some_and(|text| text.to_lowercase().contains(needle));
        self.is_active && (self.name.to_lowercase().contains(needle) || has(&self.sku) || has(&self.brand))
    }
}

/// One line of the cart. Nothing is stored until the checkout.
#[derive(Debug, Clone, PartialEq)]
pub struct CartLine {
    pub key: u64,
    pub product_id: Option<String>,
    pub label: String,
    pub quantity_milli: i64,
    /// For a shelf price, only the figure shown beside it.
    pub unit_price_ht_cents: i64,
    /// A shelf price: the price of record. At 20 %, 9,99 has no net amount in whole cents.
    pub unit_price_ttc_cents: Option<i64>,
    pub discount_bp: i64,
    pub vat_rate_bp: i64,
    pub is_service: bool,
}

impl CartLine {
    fn input(&self) -> LineInput {
        let line = match self.unit_price_ttc_cents {
            Some(ttc) => LineInput::new_ttc(self.quantity_milli, ttc, self.vat_rate_bp),
            None => LineInput::new(self.quantity_milli, self.unit_price_ht_cents, self.vat_rate_bp),
        };
        line.with_discount(self.discount_bp)
    }

    /// What the customer pays for the line.
    pub fn total_ttc_cents(&self) -> i64 {
        vat::document_totals(&[self.input()]).map(|totals| totals.total_ttc_cents).unwrap_or(0)
    }

    /// `2`, `1,5`: the quantity as the cart shows it.
    pub fn quantity_label(&self) -> String {
        let text = format!("{:.3}", self.quantity_milli as f64 / 1000.);
        text.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
    }

    /// `2 × 9,99 € TTC · −10 %`.
    pub fn meta(&self) -> String {
        let unit = match self.unit_price_ttc_cents {
            Some(ttc) => format!("{} TTC", format::euros(ttc)),
            None => format!("{} HT", format::euros(self.unit_price_ht_cents)),
        };
        let discount = if self.discount_bp > 0 { format!(" · −{} %", self.discount_bp / 100) } else { String::new() };
        format!("{} × {unit}{discount}", self.quantity_label())
    }

    /// The line as the service expects it: a shelf price travels as TTC.
    pub fn to_server(&self) -> SaleLineInput {
        SaleLineInput {
            product_id: self.product_id.clone(),
            label: Some(self.label.clone()),
            quantity_milli: self.quantity_milli,
            unit_price_ht_cents: self.unit_price_ht_cents,
            unit_price_ttc_cents: self.unit_price_ttc_cents,
            vat_rate_bp: self.vat_rate_bp,
            discount_bp: self.discount_bp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub ht_cents: i64,
    pub vat_cents: i64,
    pub ttc_cents: i64,
}

/// The cart: lines in memory until the checkout seals a ticket.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cart {
    pub lines: Vec<CartLine>,
    next_key: u64,
}

impl Cart {
    fn key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Add one of an article. The same article at the same price and discount is one line.
    pub fn add(&mut self, article: &Article) {
        let ttc = article.price_ttc_cents.unwrap_or(0);
        let ht = vat::ht_from_ttc(ttc, article.vat_rate_bp).unwrap_or(0);
        let same = self.lines.iter_mut().find(|line| {
            line.product_id.as_deref() == Some(article.id.as_str())
                && line.discount_bp == 0
                && line.unit_price_ttc_cents == Some(ttc)
                && line.vat_rate_bp == article.vat_rate_bp
        });
        match same {
            Some(line) => line.quantity_milli += 1000,
            None => {
                let key = self.key();
                self.lines.push(CartLine {
                    key,
                    product_id: Some(article.id.clone()),
                    label: article.name.clone(),
                    quantity_milli: 1000,
                    unit_price_ht_cents: ht,
                    unit_price_ttc_cents: Some(ttc),
                    discount_bp: 0,
                    vat_rate_bp: article.vat_rate_bp,
                    is_service: article.is_service,
                });
            }
        }
    }

    /// One more or one less of a line. A line brought to nothing leaves the cart.
    pub fn bump(&mut self, key: u64, units: i64) {
        if let Some(line) = self.lines.iter_mut().find(|line| line.key == key) {
            line.quantity_milli += units * 1000;
        }
        self.lines.retain(|line| line.quantity_milli > 0);
    }

    /// A discount on a line, kept between nothing and everything.
    pub fn set_discount(&mut self, key: u64, discount_bp: i64) {
        if let Some(line) = self.lines.iter_mut().find(|line| line.key == key) {
            line.discount_bp = discount_bp.clamp(0, 10_000);
        }
    }

    pub fn remove(&mut self, key: u64) {
        self.lines.retain(|line| line.key != key);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Take up the lines of a ticket that was put aside.
    pub fn recall(&mut self, lines: &[SaleLine]) {
        self.lines.clear();
        for stored in lines {
            let key = self.key();
            self.lines.push(CartLine {
                key,
                product_id: stored.product_id.clone(),
                label: stored.label.clone().unwrap_or_default(),
                quantity_milli: stored.quantity_milli,
                unit_price_ht_cents: stored.unit_price_ht_cents.unwrap_or(0),
                unit_price_ttc_cents: stored.unit_price_ttc_cents.filter(|_| stored.price_basis == "ttc"),
                discount_bp: stored.discount_bp,
                vat_rate_bp: stored.vat_rate_bp,
                // A ticket put aside does not say; a service put aside is asked about its ticket.
                is_service: false,
            });
        }
    }

    fn sum(lines: &[LineInput]) -> Totals {
        vat::document_totals(lines)
            .map(|totals| Totals { ht_cents: totals.total_ht_cents, vat_cents: totals.total_vat_cents, ttc_cents: totals.total_ttc_cents })
            .unwrap_or_default()
    }

    pub fn totals(&self) -> Totals {
        Self::sum(&self.lines.iter().map(CartLine::input).collect::<Vec<_>>())
    }

    /// What the services of the cart come to, all taxes included.
    pub fn service_ttc_cents(&self) -> i64 {
        Self::sum(&self.lines.iter().filter(|line| line.is_service).map(CartLine::input).collect::<Vec<_>>()).ttc_cents
    }

    pub fn to_server(&self) -> Vec<SaleLineInput> {
        self.lines.iter().map(CartLine::to_server).collect()
    }
}

// ---- what is paid ------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Cash,
    Card,
    Cheque,
}

impl Method {
    pub fn id(self) -> &'static str {
        match self {
            Method::Cash => "cash",
            Method::Card => "card",
            Method::Cheque => "cheque",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Method::Cash => "espèces",
            Method::Card => "carte",
            Method::Cheque => "chèque",
        }
    }
}

/// One payment of a sale being settled.
#[derive(Debug, Clone, PartialEq)]
pub struct Tender {
    pub method: Method,
    /// What the customer hands over; cash may exceed what is owed.
    pub tendered_cents: i64,
    /// What of it goes to the sale.
    pub recorded_cents: i64,
    pub auth_ref: Option<String>,
    pub cheque_drawer: Option<String>,
}

impl Tender {
    /// Cash handed over against what is still owed.
    pub fn cash(tendered_cents: i64, due_cents: i64) -> Self {
        Self {
            method: Method::Cash,
            tendered_cents,
            recorded_cents: recorded_cash_cents(tendered_cents, due_cents),
            auth_ref: None,
            cheque_drawer: None,
        }
    }

    /// The payment as the service expects it: for cash, what was handed over (the service
    /// works out the change).
    pub fn to_server(&self) -> PosPayment {
        PosPayment {
            amount_cents: if self.method == Method::Cash { self.tendered_cents } else { self.recorded_cents },
            method: self.method.id(),
            cheque_drawer: self.cheque_drawer.clone(),
            auth_ref: self.auth_ref.clone(),
        }
    }
}

/// How much of a cash tender is the payment: never more than what is owed.
pub fn recorded_cash_cents(tendered_cents: i64, due_cents: i64) -> i64 {
    if tendered_cents <= 0 { 0 } else { tendered_cents.min(due_cents.max(0)) }
}

/// What is still owed after the payments made so far. Never negative.
pub fn remaining_cents(total_cents: i64, tenders: &[Tender]) -> i64 {
    (total_cents - tenders.iter().map(|tender| tender.recorded_cents).sum::<i64>()).max(0)
}

/// What is handed back, when cash exceeded what was owed.
pub fn change_cents(tenders: &[Tender]) -> i64 {
    tenders.iter().map(|tender| tender.tendered_cents - tender.recorded_cents).sum::<i64>().max(0)
}

// ---- the paper ticket --------------------------------------------------------------------------

/// From this amount of services, all taxes included, a note is due to the customer.
pub const SERVICE_NOTE_THRESHOLD_CENTS: i64 = 2_500;

pub const TICKET_ON_REQUEST_NOTICE: &str = "Un ticket de caisse vous est remis sur simple demande.";

/// A paper ticket is not printed by default (C. env. L541-15-10): only when the customer
/// asks, or when the law still requires it, as for services from 25 €.
pub fn must_print_ticket(customer_asked: bool, service_ttc_cents: i64) -> bool {
    customer_asked || service_ttc_cents >= SERVICE_NOTE_THRESHOLD_CENTS
}

/// An amount typed at the till, in cents: `20`, `19,98`, `1 250,50`. `None` when it is not one.
pub fn typed_cents(typed: &str) -> Option<i64> {
    let cleaned: String = typed.chars().filter(|c| !c.is_whitespace() && *c != '\u{202f}' && *c != '€').collect();
    let amount = cleaned.replace(',', ".").parse::<f64>().ok().filter(|amount| amount.is_finite())?;
    Some((amount * 100.).round() as i64)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: a sibling imports GPUI, whose `test` attribute would take over.
    use super::{Article, Cart, Method, Tender, change_cents, must_print_ticket, recorded_cash_cents, remaining_cents, typed_cents};

    fn article(id: &str, price_ttc_cents: i64) -> Article {
        Article {
            id: id.into(),
            name: format!("Article {id}"),
            sku: Some(format!("SKU-{id}")),
            brand: Some("Motul".into()),
            variant: None,
            is_service: false,
            price_ttc_cents: Some(price_ttc_cents),
            vat_rate_bp: 2000,
            stock_quantity: 5.,
            low_stock_threshold: Some(2),
            is_active: true,
        }
    }

    #[test]
    fn a_shelf_price_is_charged_as_it_is_on_the_label() {
        let mut cart = Cart::default();
        cart.add(&article("a", 999));
        // 9,99 has no net amount in whole cents at 20 %: priced net, it would charge 10,00.
        assert_eq!(cart.totals().ttc_cents, 999);
        cart.add(&article("a", 999));
        cart.add(&article("a", 999));
        assert_eq!(cart.lines.len(), 1, "the same article is one line");
        assert_eq!((cart.lines[0].quantity_label().as_str(), cart.totals().ttc_cents), ("3", 2997));
        let totals = cart.totals();
        assert_eq!(totals.ht_cents + totals.vat_cents, totals.ttc_cents);
        assert_eq!(cart.to_server()[0].unit_price_ttc_cents, Some(999));
    }

    #[test]
    fn lines_are_bumped_discounted_and_removed() {
        let mut cart = Cart::default();
        cart.add(&article("a", 1000));
        cart.add(&article("b", 2000));
        let (a, b) = (cart.lines[0].key, cart.lines[1].key);

        cart.bump(a, 1);
        assert_eq!(cart.lines[0].quantity_milli, 2000);
        cart.set_discount(a, 1000);
        assert_eq!(cart.lines[0].total_ttc_cents(), 1800);
        assert_eq!(cart.lines[0].meta(), "2 × 10,00\u{a0}€ TTC · −10 %");
        cart.set_discount(a, 99_999);
        assert_eq!(cart.lines[0].discount_bp, 10_000, "a discount is at most everything");

        // The same article again is a line of its own: the first one carries a discount.
        cart.add(&article("a", 1000));
        assert_eq!(cart.lines.len(), 3);

        cart.bump(b, -1);
        assert!(cart.lines.iter().all(|line| line.key != b), "a line brought to nothing leaves the cart");
        cart.remove(a);
        assert_eq!(cart.lines.len(), 1);
        cart.clear();
        assert!(cart.is_empty());
    }

    #[test]
    fn cash_pays_what_is_owed_and_the_rest_is_change() {
        assert_eq!(recorded_cash_cents(2000, 1998), 1998);
        assert_eq!(recorded_cash_cents(1000, 1998), 1000);
        assert_eq!(recorded_cash_cents(0, 1998), 0);
        assert_eq!(recorded_cash_cents(500, -3), 0);

        let part = vec![Tender::cash(1000, 1998)];
        assert_eq!(remaining_cents(1998, &part), 998);
        let all = vec![part[0].clone(), Tender::cash(2000, 998)];
        assert_eq!(remaining_cents(1998, &all), 0);
        assert_eq!(change_cents(&all), 1002);
        // The service is told what was handed over, and works out the change itself.
        assert_eq!(all[1].to_server().amount_cents, 2000);
        let card = Tender { method: Method::Card, tendered_cents: 998, recorded_cents: 998, auth_ref: Some("A".into()), cheque_drawer: None };
        assert_eq!((card.to_server().amount_cents, card.to_server().method), (998, "card"));
    }

    #[test]
    fn a_ticket_is_printed_when_asked_for_or_for_services_from_25_euros() {
        assert!(!must_print_ticket(false, 0));
        assert!(must_print_ticket(true, 0));
        assert!(!must_print_ticket(false, 2_499));
        assert!(must_print_ticket(false, 2_500));

        let mut cart = Cart::default();
        cart.add(&article("pièce", 9_000));
        assert_eq!(cart.service_ttc_cents(), 0);
        cart.add(&Article { is_service: true, ..article("main d'œuvre", 3_000) });
        assert_eq!(cart.service_ttc_cents(), 3_000);
        assert!(must_print_ticket(false, cart.service_ttc_cents()));
    }

    #[test]
    fn an_article_out_of_stock_is_not_sold_unless_the_shop_allows_it() {
        let empty = Article { stock_quantity: 0., ..article("a", 1000) };
        assert!(empty.out_of_stock(false) && !empty.out_of_stock(true));
        assert_eq!(empty.stock_badge(false), Some(("Rupture", true)));
        let low = Article { stock_quantity: 2., ..article("a", 1000) };
        assert_eq!(low.stock_badge(false), Some(("Stock bas", false)));
        assert_eq!(article("a", 1000).stock_badge(false), None);
        // A service has no shelf.
        assert!(!Article { is_service: true, stock_quantity: 0., ..article("a", 1000) }.out_of_stock(false));

        assert!(article("a", 1000).matches("motul") && article("a", 1000).matches("sku-a"));
        assert!(!Article { is_active: false, ..article("a", 1000) }.matches("article"));
        assert_eq!(article("a", 1000).meta(), "Motul · SKU-a");
    }

    #[test]
    fn amounts_are_typed_the_french_way() {
        assert_eq!(typed_cents("20"), Some(2000));
        assert_eq!(typed_cents("19,98"), Some(1998));
        assert_eq!(typed_cents(" 1 250,50 € "), Some(125_050));
        assert_eq!(typed_cents("0.1"), Some(10));
        assert_eq!(typed_cents(""), None);
        assert_eq!(typed_cents("vingt"), None);
    }
}
