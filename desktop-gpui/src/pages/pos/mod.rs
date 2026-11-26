//! La Caisse: counter sales, full screen. The cart stays in memory until the checkout seals a
//! ticket; a correction after that is a credit note, never an edit.

mod cart;

use std::time::Duration;

use motodesk_client::pos::{self, Checkout, HeldTicket, PosDocument, PosSession};
use motodesk_client::{ApiResult, clients, fiscal, products, sales, settings};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};
use crate::pos::PosState;
use cart::{Article, Cart, Method, TICKET_ON_REQUEST_NOTICE, Tender, change_cents, must_print_ticket, remaining_cents, typed_cents};

/// How many articles a search shows.
const HITS_SHOWN: usize = 16;
/// How long the till waits after a keystroke before asking whether what is typed is a code.
const SCAN_PAUSE: Duration = Duration::from_millis(80);
const CLOCK_TICK: Duration = Duration::from_secs(15);
const CLIENTS_OFFERED: u32 = 30;
const QUICK_CASH_EUROS: [i64; 4] = [10, 20, 50, 100];

/// Where the payment dialog is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayStep {
    Pick,
    Cash,
    Card,
    Cheque,
    /// The ticket is sealed: change to give back, and whether to print.
    Done,
}

/// A sale that was just sealed.
struct Settled {
    sale_id: String,
    change_cents: i64,
    /// The ticket was sent to the printer, or asked for.
    printed: bool,
    /// The ticket as text, when there is no printer to send it to.
    ticket: Option<String>,
}

/// What the till reads when it opens.
struct Opening {
    session: Option<PosSession>,
    products: Vec<products::ProductListItem>,
    register_code: String,
    frequents: Vec<pos::FrequentItem>,
    held: Vec<HeldTicket>,
    allow_negative_stock: bool,
}

/// What a closed session is summed up as.
fn closing_summary(closed: &pos::SessionClosed) -> String {
    format!(
        "{} attendu, {} compté ({} {}).",
        format::euros(closed.expected_cash_cents),
        format::euros(closed.counted_cents),
        closed.verdict(),
        format::euros(closed.variance_cents.abs())
    )
}

fn clock_now() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

pub struct Till {
    loading: bool,
    error: Option<String>,
    session: Option<PosSession>,
    register_code: String,
    products: Vec<Article>,
    frequents: Vec<Article>,
    allow_negative_stock: bool,

    search: TextModel,
    /// Counts the keystrokes of the search: a code looked up for an older text is dropped.
    scans: u64,
    cart: Cart,
    tenders: Vec<Tender>,
    busy: bool,

    float: TextModel,
    /// Why the service would not open the till (the day is already closed, for one). It stays
    /// on the opening screen: a toast is gone before the cashier has read it.
    open_error: Option<SharedString>,
    client: ComboModel,
    client_searches: u64,
    held: Vec<HeldTicket>,

    pay: Option<PayStep>,
    cash: TextModel,
    cheque_drawer: TextModel,
    settled: Option<Settled>,
    discount_key: Option<u64>,
    discount: TextModel,
    counted: TextModel,
    close_day: bool,
    /// What closing found, and whether the drawer matched.
    close_result: Option<(String, bool)>,

    clock: String,
    _subscriptions: Vec<Subscription>,
}

impl Till {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = TextModel::builder().placeholder("Article, code-barres…").build(window, cx);
        let float = TextModel::builder().value("0").build(window, cx);
        let cash = TextModel::new(window, cx);
        let cheque_drawer = TextModel::new(window, cx);
        let discount = TextModel::new(window, cx);
        let counted = TextModel::builder().placeholder("245,50").build(window, cx);

        let client = ComboModel::new("Choisir le client de la facture — nom, téléphone, courriel…", window, cx);
        let (on_query, on_pick) = (cx.entity().downgrade(), cx.entity().downgrade());
        client.on_query(cx, move |query, _, cx| {
            let query = query.to_string();
            let _ = on_query.update(cx, |till, cx| till.search_clients(query, cx));
        });
        client.on_pick(cx, move |_, _, cx| {
            let _ = on_pick.update(cx, |_, cx| cx.notify());
        });

        let subscriptions = vec![
            cx.observe(&PosState::global(cx), |_, _, cx| cx.notify()),
            search.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                TextEvent::Changed => this.typed(window, cx),
                TextEvent::Submitted { .. } => this.pick_first_hit(window, cx),
                _ => {}
            }),
            float.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.open_session(window, cx);
                }
            }),
            cash.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.confirm_cash(window, cx);
                }
            }),
            cheque_drawer.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.confirm_cheque(window, cx);
                }
            }),
            discount.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.apply_discount(window, cx);
                }
            }),
            counted.on_event(window, cx, |this: &mut Self, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.close_session(window, cx);
                }
            }),
        ];
        // The clock of the title bar.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK_TICK).await;
                let alive = this.update(cx, |this, cx| {
                    this.clock = clock_now();
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        let mut till = Self {
            loading: true,
            error: None,
            session: None,
            register_code: String::new(),
            products: Vec::new(),
            frequents: Vec::new(),
            allow_negative_stock: false,
            search,
            scans: 0,
            cart: Cart::default(),
            tenders: Vec::new(),
            busy: false,
            float,
            open_error: None,
            client,
            client_searches: 0,
            held: Vec::new(),
            pay: None,
            cash,
            cheque_drawer,
            settled: None,
            discount_key: None,
            discount,
            counted,
            close_day: false,
            close_result: None,
            clock: clock_now(),
            _subscriptions: subscriptions,
        };
        till.boot(window, cx);
        till
    }

    fn config(cx: &App) -> pos::PosConfig {
        PosState::global(cx).read(cx).config.clone()
    }

    /// An invoice names its customer (a ticket does not): in invoice mode one must be chosen.
    fn needs_client(cx: &App) -> bool {
        Self::config(cx).default_document == PosDocument::Invoice
    }

    fn due(&self) -> i64 {
        remaining_cents(self.cart.totals().ttc_cents, &self.tenders)
    }

    fn can_pay(&self, cx: &App) -> bool {
        !self.cart.is_empty() && !self.busy && (!Self::needs_client(cx) || self.client.value(cx).is_some())
    }

    // ---- opening -------------------------------------------------------------------------------

    fn boot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api_in(
            window,
            |client| async move {
                let session = pos::session(&client).await?;
                let products = products::list(&client, &products::ListQuery { limit: Some(2000), ..Default::default() }).await?;
                // What follows is not worth refusing to open the till for.
                let register_code = fiscal::register(&client).await.map(|register| register.code).unwrap_or_default();
                let frequents = pos::frequents(&client).await.unwrap_or_default();
                let held = pos::held(&client).await.unwrap_or_default();
                let stored = settings::list(&client).await.unwrap_or_default();
                let allow_negative_stock =
                    stored.iter().find(|setting| setting.key == "stock.allow_negative").and_then(|setting| setting.value.as_bool()).unwrap_or(false);
                Ok(Opening { session, products, register_code, frequents, held, allow_negative_stock })
            },
            |this, result: ApiResult<Opening>, window, cx| {
                this.loading = false;
                match result {
                    Ok(opening) => {
                        this.session = opening.session;
                        this.register_code = opening.register_code;
                        this.products = opening.products.iter().map(Article::from).collect();
                        this.held = opening.held;
                        this.allow_negative_stock = opening.allow_negative_stock;
                        this.set_frequents(opening.frequents, cx);
                        if this.session.is_some() {
                            this.search.focus(window, cx);
                        } else {
                            this.float.focus_selected(window, cx);
                        }
                    }
                    Err(error) => this.error = Some(humanize_error(&error, "La caisse n'a pas pu s'ouvrir.")),
                }
                cx.notify();
            },
        );
    }

    /// The quick strip: what sells most, or the favourites of this PC while nothing has sold.
    fn set_frequents(&mut self, ranked: Vec<pos::FrequentItem>, cx: &App) {
        self.frequents = if ranked.is_empty() {
            let favourites = Self::config(cx).favorite_product_ids;
            self.products.iter().filter(|article| article.is_active && favourites.contains(&article.id)).cloned().collect()
        } else {
            ranked.iter().map(Article::from).collect()
        };
    }

    fn open_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let float_cents = typed_cents(&self.float.text(cx)).unwrap_or(0).max(0);
        self.busy = true;
        self.open_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { pos::open_session(&client, float_cents).await },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(session) => {
                        this.session = Some(session);
                        toast::success("Session ouverte", window, cx);
                        this.search.focus(window, cx);
                    }
                    // The service's own words, kept on screen.
                    Err(error) => this.open_error = Some(humanize_error(&error, "La session n'a pas pu être ouverte.").into()),
                }
                cx.notify();
            },
        );
    }

    // ---- finding articles ----------------------------------------------------------------------

    /// What the search shows for the text typed.
    fn hits(&self, cx: &App) -> Vec<Article> {
        let needle = self.search.text(cx).trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.products.iter().filter(|article| article.matches(&needle)).take(HITS_SHOWN).cloned().collect()
    }

    /// Something was typed, or scanned: a scanner types a whole reference in one go. When
    /// the text is exactly the reference of an article, the article goes straight to the cart.
    fn typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scans += 1;
        let scan = self.scans;
        cx.notify();
        let code = self.search.text(cx).trim().to_string();
        if code.is_empty() {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SCAN_PAUSE).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.scans != scan {
                    return;
                }
                cx.api_in(
                    window,
                    move |client| async move { pos::product_by_sku(&client, &code).await },
                    move |this, found, window, cx| {
                        // The answer comes later than the keyboard: if Enter already took the
                        // article from the list, or the text changed, this one is stale, and
                        // adding it too would ring the same item up twice.
                        let (Ok(Some(product)), true) = (found, this.scans == scan) else {
                            return;
                        };
                        if product.is_active {
                            this.pick(&Article::from(&product), window, cx);
                        }
                    },
                );
            });
        })
        .detach();
    }

    /// Enter in the search: the first article that can be sold.
    fn pick_first_hit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hits = self.hits(cx);
        let Some(article) = hits.iter().find(|article| !article.out_of_stock(self.allow_negative_stock)).or(hits.first()).cloned() else {
            return;
        };
        self.pick(&article, window, cx);
    }

    /// Put one of an article in the cart, and give the keyboard back to the search.
    fn pick(&mut self, article: &Article, window: &mut Window, cx: &mut Context<Self>) {
        if article.out_of_stock(self.allow_negative_stock) {
            toast::error("Article en rupture", window, cx);
        } else {
            self.cart.add(article);
        }
        // What was typed is used up; a code looked up for it is no longer wanted.
        self.scans += 1;
        self.search.clear(window, cx);
        self.search.focus(window, cx);
        cx.notify();
    }

    // ---- the cart ------------------------------------------------------------------------------

    fn ask_discount(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(line) = self.cart.lines.iter().find(|line| line.key == key) else {
            return;
        };
        self.discount.set_text((line.discount_bp / 100).to_string(), window, cx);
        self.discount_key = Some(key);
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Remise").width(px(320.)).open(
            window,
            cx,
            move |_, cx| element(FormField::new("Remise (%)").child(Input::new(&for_body.read(cx).discount))),
            move |_, _| {
                let apply = for_footer.clone();
                vec![
                    element(Button::new("till-discount-cancel").ghost().label("Annuler").on_click(|_, window, cx| close_modal(window, cx))),
                    element(
                        Button::new("till-discount-apply")
                            .primary()
                            .label("Appliquer")
                            .on_click(move |_, window, cx| apply.update(cx, |this, cx| this.apply_discount(window, cx))),
                    ),
                ]
            },
        );
        self.discount.focus_selected(window, cx);
    }

    fn apply_discount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.discount_key.take() else {
            return;
        };
        // Typed in percent, kept in hundredths of a percent.
        let discount_bp = typed_cents(&self.discount.text(cx)).unwrap_or(0);
        self.cart.set_discount(key, discount_bp);
        close_modal(window, cx);
        cx.notify();
    }

    /// Put the cart aside as an unsealed ticket.
    fn park(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cart.is_empty() || self.busy {
            return;
        }
        let lines = self.cart.to_server();
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                pos::park(&client, &lines).await?;
                pos::held(&client).await
            },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(held) => {
                        this.held = held;
                        this.cart.clear();
                        this.tenders.clear();
                        toast::success("Ticket mis en attente", window, cx);
                    }
                    Err(error) => toast::error(humanize_error(&error, "Le ticket n'a pas pu être mis en attente."), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn show_held(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let for_body = cx.entity();
        Modal::new("Tickets en attente").width(px(420.)).open(
            window,
            cx,
            move |_, cx| {
                let till = for_body.read(cx);
                if till.held.is_empty() {
                    return element(div().h(px(140.)).child(StateView::empty().compact().icon(IconName::Inbox).title("Aucun ticket en attente")));
                }
                let rows = till.held.iter().enumerate().map(|(ix, ticket)| {
                    let (page, id) = (for_body.clone(), ticket.id.clone());
                    ListRow::new(ix, format::euros(ticket.total_ttc_cents))
                        .meta(format!("{} · {}", ticket.notes.clone().unwrap_or_else(|| "Ticket".into()), format::date_time(&ticket.updated_at)))
                        .icon(IconName::ShoppingCart)
                        .on_click(move |_, window, cx| page.update(cx, |this, cx| this.recall(id.clone(), window, cx)))
                });
                element(col().children(rows.collect::<Vec<_>>()))
            },
            |_, _| vec![element(Button::new("till-held-close").label("Fermer").on_click(|_, window, cx| close_modal(window, cx)))],
        );
    }

    /// Take a ticket that was put aside back into the cart. It stops waiting.
    fn recall(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                let sale = sales::get(&client, &id).await?;
                sales::delete(&client, &id).await?;
                Ok((sale, pos::held(&client).await.unwrap_or_default()))
            },
            |this, result: ApiResult<(sales::Sale, Vec<HeldTicket>)>, window, cx| {
                this.busy = false;
                match result {
                    Ok((sale, held)) => {
                        this.cart.recall(&sale.lines);
                        this.tenders.clear();
                        this.held = held;
                        close_modal(window, cx);
                        this.search.focus(window, cx);
                    }
                    Err(error) => toast::error(humanize_error(&error, "Le ticket n'a pas pu être repris."), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn search_clients(&mut self, query: String, cx: &mut Context<Self>) {
        self.client_searches += 1;
        let this_search = self.client_searches;
        self.client.set_loading(true, cx);
        let query = clients::ListQuery { q: query, limit: Some(CLIENTS_OFFERED), offset: None };
        cx.api(
            move |client| async move { clients::list(&client, &query).await },
            move |this, result, cx| {
                // An answer to an older keystroke must not replace a newer one.
                if this.client_searches != this_search {
                    return;
                }
                let options = result
                    .map(|page| page.items)
                    .unwrap_or_default()
                    .iter()
                    .map(|c| {
                        let contact = [c.phone.as_deref(), c.email.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                        ComboOption::new(c.id.clone(), clients::format_client_name(c.first_name.as_deref(), &c.last_name)).detail(contact)
                    })
                    .collect();
                this.client.set_options(options, cx);
            },
        );
    }

    // ---- paying --------------------------------------------------------------------------------

    fn open_pay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_pay(cx) {
            return;
        }
        self.pay = Some(PayStep::Pick);
        self.settled = None;
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Encaisser").width(px(440.)).keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| for_body.update(cx, |this, cx| element(this.pay_body(cx))),
            move |_, cx| for_footer.update(cx, |this, cx| this.pay_footer(cx)),
        );
    }

    fn go_to_step(&mut self, step: PayStep, window: &mut Window, cx: &mut Context<Self>) {
        self.pay = Some(step);
        match step {
            PayStep::Cash => {
                self.cash.set_text(format::cents_to_text(Some(self.due())), window, cx);
                self.cash.focus_selected(window, cx);
            }
            PayStep::Cheque => {
                self.cheque_drawer.clear(window, cx);
                self.cheque_drawer.focus(window, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    /// Record a payment; when everything is covered, seal the sale.
    fn tender(&mut self, tender: Tender, window: &mut Window, cx: &mut Context<Self>) {
        self.tenders.push(tender);
        if self.due() == 0 {
            self.settle(window, cx);
        } else {
            // Part of it is paid: back to the ways of paying, for the rest.
            self.pay = Some(PayStep::Pick);
            cx.notify();
        }
    }

    fn confirm_cash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pay != Some(PayStep::Cash) || self.busy {
            return;
        }
        let Some(tendered) = typed_cents(&self.cash.text(cx)).filter(|cents| *cents > 0) else {
            toast::error("Saisissez le montant remis par le client.", window, cx);
            return;
        };
        self.tender(Tender::cash(tendered, self.due()), window, cx);
    }

    fn confirm_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (due, mode) = (self.due(), Self::config(cx).tpe_mode);
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { pos::charge_card(&client, due, mode).await },
            move |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(charge) => match charge.accepted() {
                        Some(auth) => {
                            let tender = Tender {
                                method: Method::Card,
                                tendered_cents: due,
                                recorded_cents: due,
                                auth_ref: Some(auth.to_string()),
                                cheque_drawer: None,
                            };
                            this.tender(tender, window, cx);
                        }
                        None => toast::error(charge.reason.unwrap_or_else(|| "Paiement carte refusé".into()), window, cx),
                    },
                    Err(error) => toast::error(humanize_error(&error, "Le terminal n'a pas répondu."), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn confirm_cheque(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pay != Some(PayStep::Cheque) || self.busy {
            return;
        }
        let due = self.due();
        let tender = Tender {
            method: Method::Cheque,
            tendered_cents: due,
            recorded_cents: due,
            auth_ref: None,
            cheque_drawer: self.cheque_drawer.trimmed(cx),
        };
        self.tender(tender, window, cx);
    }

    /// Seal the cart as a ticket with the payments made.
    fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.due() != 0 || self.cart.is_empty() || self.busy {
            return;
        }
        let config = Self::config(cx);
        let needs_client = config.default_document == PosDocument::Invoice;
        let input = Checkout {
            client_id: self.client.value(cx).filter(|_| needs_client),
            kind: config.default_document,
            lines: self.cart.to_server(),
            payments: self.tenders.iter().map(Tender::to_server).collect(),
        };
        // What was handed over beyond what was owed, as the cashier counted it.
        let counted_change = change_cents(&self.tenders);
        let print_now = must_print_ticket(false, self.cart.service_ttc_cents());
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { pos::checkout(&client, &input).await },
            move |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(sold) => {
                        // The service works the change out; what the cashier counted stands in for it.
                        let change = if sold.change_cents > 0 { sold.change_cents } else { counted_change };
                        this.settled = Some(Settled { sale_id: sold.sale.id.clone(), change_cents: change, printed: false, ticket: None });
                        this.cart.clear();
                        this.tenders.clear();
                        this.client.clear(window, cx);
                        this.pay = Some(PayStep::Done);
                        this.kick_drawer(sold.kick_drawer && config.drawer_kick_on_cash, cx);
                        if print_now {
                            this.print(window, cx);
                        }
                        this.refresh_frequents(cx);
                    }
                    // The payments stay: the sale can be sealed again without asking for them twice.
                    Err(error) => {
                        this.pay = Some(PayStep::Pick);
                        toast::error(humanize_error(&error, "La vente n'a pas pu être enregistrée."), window, cx);
                    }
                }
                cx.notify();
            },
        );
    }

    fn refresh_frequents(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { pos::frequents(&client).await },
            |this, result, cx| {
                if let Ok(ranked) = result {
                    this.set_frequents(ranked, cx);
                    cx.notify();
                }
            },
        );
    }

    /// Pulse the drawer through the printer, when this PC has one. A failure here is not a
    /// failed sale: the ticket is sealed either way.
    fn kick_drawer(&mut self, wanted: bool, cx: &mut Context<Self>) {
        let Some(target) = Self::config(cx).printer().map(str::to_string).filter(|_| wanted) else {
            return;
        };
        cx.api(move |client| async move { pos::kick_drawer(&client, &target).await }, |_, _, _| {});
    }

    /// Print the ticket of the sale just sealed. Without a printer, the ticket is shown.
    fn print(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sale_id) = self.settled.as_mut().map(|settled| {
            settled.printed = true;
            settled.sale_id.clone()
        }) else {
            return;
        };
        let target = Self::config(cx).printer().map(str::to_string);
        let on_paper = target.is_some();
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { pos::print_ticket(&client, &sale_id, target.as_deref()).await },
            move |this, result, window, cx| {
                match result {
                    Ok(printed) => {
                        if let Some(settled) = this.settled.as_mut().filter(|_| !on_paper) {
                            settled.ticket = Some(printed.text);
                        }
                    }
                    Err(error) => toast::error(humanize_error(&error, "Ticket enregistré, impression impossible"), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn end_sale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pay = None;
        self.settled = None;
        close_modal(window, cx);
        self.search.focus(window, cx);
        cx.notify();
    }

    fn pay_body(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let due = self.due();
        let amount = |label: &'static str, cents: i64| {
            col()
                .items_center()
                .gap(m.space_1)
                .child(div().fs_tight(m.fs_xs).font_weight(FontWeight::SEMIBOLD).text_color(c.text_muted).child(label))
                .child(
                    div()
                        .font_family(FONT_MONO)
                        .text_size(m.fs_xl)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c.text_strong)
                        .child(format::euros(cents)),
                )
        };
        let body = col().gap(m.space_4);
        match self.pay.unwrap_or(PayStep::Pick) {
            // Everything is paid and the sale is not sealed: the service refused it once.
            PayStep::Pick if due == 0 && !self.tenders.is_empty() => body
                .child(
                    Alert::warning()
                        .title("Paiement reçu, vente non enregistrée")
                        .child("Les règlements saisis sont conservés. Enregistrez la vente à nouveau, ou annulez-les pour reprendre."),
                )
                .child(row().child(Button::new("till-forget-tenders").link().small().label("Annuler les règlements").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.tenders.clear();
                        cx.notify();
                    },
                )))),
            PayStep::Pick => {
                let paid_part = !self.tenders.is_empty();
                // What was already handed over stays in sight while the rest is settled.
                let received = self
                    .tenders
                    .iter()
                    .map(|tender| format!("{} ({})", format::euros(tender.recorded_cents), tender.method.label()))
                    .collect::<Vec<_>>();
                body.child(amount(if paid_part { "RESTE À PAYER" } else { "TOTAL" }, due))
                    .when(paid_part, |this| {
                        this.child(div().fs(m.fs_sm).text_color(c.text_muted).text_center().child(format!("Déjà reçu : {}", received.join(", "))))
                    })
                    .child(
                        row()
                            .gap(m.space_2)
                            .child(
                                PosKey::new("till-pay-cash", "Espèces")
                                    .icon(IconName::Banknote)
                                    .primary()
                                    .on_click(cx.listener(|this, _, window, cx| this.go_to_step(PayStep::Cash, window, cx))),
                            )
                            .child(
                                PosKey::new("till-pay-card", "Carte")
                                    .icon(IconName::CreditCard)
                                    .on_click(cx.listener(|this, _, window, cx| this.go_to_step(PayStep::Card, window, cx))),
                            )
                            .child(
                                PosKey::new("till-pay-cheque", "Chèque")
                                    .icon(IconName::FileText)
                                    .on_click(cx.listener(|this, _, window, cx| this.go_to_step(PayStep::Cheque, window, cx))),
                            ),
                    )
            }
            PayStep::Cash => body.child(amount("RESTE À PAYER", due)).child(FormField::new("Montant remis (€)").child(Input::new(&self.cash))).child(
                row().gap(m.space_2).children(QUICK_CASH_EUROS.into_iter().map(|euros| {
                    PosKey::new(("till-quick-cash", euros as usize), format!("{euros} €")).disabled(self.busy).on_click(cx.listener(
                        move |this, _, window, cx| {
                            let tender = Tender::cash(euros * 100, this.due());
                            this.tender(tender, window, cx);
                        },
                    ))
                })),
            ),
            PayStep::Card => body.child(amount("À ENCAISSER PAR CARTE", due)).child(div().fs(m.fs_sm).text_color(c.text_muted).child(format!(
                "Présentez {} au terminal ({}), puis validez quand le paiement est accepté.",
                format::euros(due),
                Self::config(cx).tpe_mode.short()
            ))),
            PayStep::Cheque => {
                body.child(amount("À ENCAISSER PAR CHÈQUE", due)).child(FormField::new("Émetteur (optionnel)").child(Input::new(&self.cheque_drawer)))
            }
            PayStep::Done => {
                let settled = self.settled.as_ref();
                let change = settled.map_or(0, |settled| settled.change_cents);
                let printed = settled.is_some_and(|settled| settled.printed);
                let ticket = settled.and_then(|settled| settled.ticket.clone());
                body.child(
                    col()
                        .items_center()
                        .gap(m.space_2)
                        .child(Icon::new(IconName::CircleCheck).xl().color(c.success))
                        .child(Heading::new("Vente enregistrée").size(HeadingSize::Lg)),
                )
                .when(change > 0, |this| this.child(amount("À RENDRE", change)))
                .map(|this| match (printed, ticket) {
                    (_, Some(text)) => this
                        .child(div().fs(m.fs_xs).text_color(c.text_muted).child("Aucune imprimante n'est réglée sur ce poste : voici le ticket."))
                        .child(div().id("till-ticket").max_h(px(260.)).overflow_y_scroll().child(Preformatted::new(text))),
                    (true, None) => this.child(div().fs(m.fs_sm).text_color(c.text_muted).text_center().child("Ticket envoyé à l'imprimante.")),
                    (false, None) => this.child(div().fs(m.fs_sm).text_color(c.text_muted).text_center().child(TICKET_ON_REQUEST_NOTICE)),
                })
            }
        }
    }

    fn pay_footer(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let due = self.due();
        let back = || {
            Button::new("till-pay-back")
                .ghost()
                .label("Retour")
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, window, cx| this.go_to_step(PayStep::Pick, window, cx)))
        };
        match self.pay.unwrap_or(PayStep::Pick) {
            PayStep::Pick if due == 0 && !self.tenders.is_empty() => vec![element(
                Button::new("till-settle-again")
                    .primary()
                    .label("Enregistrer la vente")
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.settle(window, cx))),
            )],
            PayStep::Pick => vec![element(Button::new("till-pay-cancel").ghost().label("Annuler").on_click(cx.listener(|this, _, window, cx| {
                this.pay = None;
                close_modal(window, cx);
                cx.notify();
            })))],
            PayStep::Cash => vec![
                element(back()),
                element(
                    Button::new("till-cash-confirm")
                        .primary()
                        .label("Encaisser")
                        .loading(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.confirm_cash(window, cx))),
                ),
            ],
            PayStep::Card => vec![
                element(back()),
                element(
                    Button::new("till-card-confirm")
                        .primary()
                        .label("Valider la carte")
                        .loading(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.confirm_card(window, cx))),
                ),
            ],
            PayStep::Cheque => vec![
                element(back()),
                element(
                    Button::new("till-cheque-confirm")
                        .primary()
                        .label(format!("Encaisser {}", format::euros(due)))
                        .loading(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.confirm_cheque(window, cx))),
                ),
            ],
            PayStep::Done => {
                let printed = self.settled.as_ref().is_some_and(|settled| settled.printed);
                let done = |id: &'static str, label: &'static str, cx: &mut Context<Self>| {
                    Button::new(id).label(label).on_click(cx.listener(|this, _, window, cx| this.end_sale(window, cx)))
                };
                if printed {
                    vec![element(done("till-sale-done", "Vente suivante", cx).primary())]
                } else {
                    vec![
                        element(done("till-no-ticket", "Sans ticket", cx).ghost()),
                        element(
                            Button::new("till-print")
                                .primary()
                                .icon(IconName::Printer)
                                .label("Imprimer")
                                .on_click(cx.listener(|this, _, window, cx| this.print(window, cx))),
                        ),
                    ]
                }
            }
        }
    }

    // ---- the drawer and the end of the day -----------------------------------------------------

    fn open_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            |client| async move { pos::open_drawer(&client).await },
            |this, result, window, cx| match result {
                Ok(()) => {
                    this.kick_drawer(true, cx);
                    toast::success("Tiroir ouvert (mouvement journalisé)", window, cx);
                }
                Err(error) => toast::error(humanize_error(&error, "Le tiroir n'a pas pu être ouvert."), window, cx),
            },
        );
    }

    fn ask_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.counted.clear(window, cx);
        self.close_day = false;
        self.close_result = None;
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Clôture de session").width(px(440.)).keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let till = for_body.read(cx);
                if let Some((result, matched)) = till.close_result.clone() {
                    let alert = if matched { Alert::success() } else { Alert::warning() };
                    return element(col().gap(m.space_3).child(alert.title("Session clôturée").child(result)));
                }
                let toggle = for_body.clone();
                element(
                    col()
                        .gap(m.space_4)
                        .child("Comptez le tiroir sans regarder le théorique.")
                        .child(FormField::new("Montant compté (€)").child(Input::new(&till.counted)))
                        .child(
                            Switch::new("till-close-day", till.close_day)
                                .label("Clôturer aussi la journée (Z)")
                                .hint("Fige les totaux d'aujourd'hui. Plus aucune vente ni aucun règlement ne sera possible avant demain.")
                                .on_change(move |on, _, cx| {
                                    toggle.update(cx, |this, cx| {
                                        this.close_day = on;
                                        cx.notify();
                                    })
                                }),
                        ),
                )
            },
            move |_, cx| {
                let till = for_footer.read(cx);
                if till.close_result.is_some() {
                    return vec![element(Button::new("till-close-ok").primary().label("OK").on_click(|_, window, cx| close_modal(window, cx)))];
                }
                let close = for_footer.clone();
                vec![
                    element(
                        Button::new("till-close-cancel")
                            .ghost()
                            .label("Annuler")
                            .disabled(till.busy)
                            .on_click(|_, window, cx| close_modal(window, cx)),
                    ),
                    element(
                        Button::new("till-close-confirm")
                            .primary()
                            .label("Clôturer")
                            .loading(till.busy)
                            .on_click(move |_, window, cx| close.update(cx, |this, cx| this.close_session(window, cx))),
                    ),
                ]
            },
        );
        self.counted.focus(window, cx);
    }

    fn close_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.close_result.is_some() || self.session.is_none() {
            return;
        }
        // An empty field is not "0 €": closing on it would record the whole drawer as missing.
        let typed = self.counted.text(cx);
        if typed.trim().is_empty() {
            toast::error("Saisissez le montant compté dans le tiroir.", window, cx);
            return;
        }
        let Some(counted) = typed_cents(&typed).filter(|cents| *cents >= 0) else {
            toast::error("Montant invalide : saisissez un nombre, par exemple 245,50.", window, cx);
            return;
        };
        let close_day = self.close_day;
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { pos::close_session(&client, counted, close_day).await },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(closed) => {
                        this.session = None;
                        this.close_result = Some((closing_summary(&closed), closed.variance_cents == 0));
                        if let Some(why) = &closed.day_closure_error {
                            // The till is closed; only the day is still open.
                            toast::error(format!("Session fermée, mais la journée n'a pas été clôturée : {why}"), window, cx);
                        } else if closed.day_closure.is_some() {
                            toast::success("Session fermée et journée clôturée.", window, cx);
                        }
                    }
                    Err(error) => toast::error(humanize_error(&error, "La session n'a pas pu être clôturée."), window, cx),
                }
                cx.notify();
            },
        );
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn chrome(&self, cx: &mut Context<Self>) -> TitleBar {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let auth = AuthStore::global(cx).read(cx);
        let operator =
            auth.user.as_ref().map(|user| if user.full_name.is_empty() { user.email.clone() } else { user.full_name.clone() }).unwrap_or_default();
        let is_admin = auth.is_admin();
        let bar = TitleBar::new()
            .child(Icon::new(IconName::Store).color(c.accent))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Caisse"))
            .when(!self.register_code.is_empty(), |bar| bar.child(Tag::new(self.register_code.clone()).status(Status::Accent)))
            .child(div().text_color(c.text_muted).child(operator))
            .child(div().font_family(FONT_MONO).fs_tight(m.fs_sm).text_color(c.text_muted).child(self.clock.clone()));
        if !is_admin {
            return bar;
        }
        // An administrator can step out to the office screens; the PC stays a till.
        bar.trailing(Button::new("till-office").ghost().small().icon(IconName::LayoutDashboard).label("Bureau").on_click(|_, _, cx| {
            PosState::global(cx).update(cx, |pos, cx| pos.enter_office(cx));
        }))
    }

    fn gate(&self, cx: &mut Context<Self>) -> Div {
        div().size_full().flex().items_center().justify_center().child(
            div().w(px(420.)).child(
                SectionCard::new()
                    .title("Ouverture de session")
                    .subtitle("Comptez le fond de caisse avant d'encaisser. Le montant est journalisé.")
                    .icon(IconName::Store)
                    .child(FormField::new("Fond de caisse (€)").error(self.open_error.clone()).child(Input::new(&self.float)))
                    .footer(
                        Button::new("till-open")
                            .primary()
                            .label("Ouvrir la caisse")
                            .loading(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.open_session(window, cx))),
                    ),
            ),
        )
    }

    fn item(&self, id: ElementId, article: &Article, cx: &mut Context<Self>) -> PosItem {
        let out = article.out_of_stock(self.allow_negative_stock);
        let picked = article.clone();
        let item = PosItem::new(id, article.name.clone(), article.price_ttc_cents.map(format::euros).unwrap_or_else(|| "—".into()))
            .icon(if article.is_service { IconName::Wrench } else { IconName::Package })
            .disabled(out)
            .on_click(cx.listener(move |this, _, window, cx| this.pick(&picked, window, cx)));
        match article.stock_badge(self.allow_negative_stock) {
            Some((label, true)) => item.badge(label, Status::Danger),
            Some((label, false)) => item.badge(label, Status::Warning),
            None => item,
        }
    }

    fn frequent_rail(&self, cx: &mut Context<Self>) -> FilterRail {
        let rail = FilterRail::new("till-frequents", "Achats fréquents");
        let count = self.frequents.len();
        if count == 0 {
            return rail.child(
                div().h(px(200.)).child(
                    StateView::empty()
                        .compact()
                        .icon(IconName::Package)
                        .title("Aucun achat fréquent")
                        .body("Les articles les plus vendus apparaîtront ici."),
                ),
            );
        }
        let items: Vec<PosItem> = self
            .frequents
            .iter()
            .enumerate()
            .map(|(ix, article)| self.item(ElementId::NamedInteger("till-frequent".into(), ix as u64), article, cx).compact())
            .collect();
        rail.children(items).footer(format!("{count} article{}", if count > 1 { "s" } else { "" }))
    }

    fn finder(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let query = self.search.text(cx).trim().to_string();
        let hits = self.hits(cx);
        let results: AnyElement = if query.is_empty() {
            div()
                .h(px(220.))
                .child(
                    StateView::empty()
                        .compact()
                        .icon(IconName::Search)
                        .title("Recherchez ou scannez")
                        .body("Tapez un nom ou un code, ou touchez un achat fréquent."),
                )
                .into_any_element()
        } else if hits.is_empty() {
            div()
                .h(px(220.))
                .child(StateView::empty().compact().icon(IconName::Package).title("Aucun article").body(format!("Aucun résultat pour « {query} ».")))
                .into_any_element()
        } else {
            let rows: Vec<PosItem> = hits
                .iter()
                .enumerate()
                .map(|(ix, article)| self.item(ElementId::NamedInteger("till-hit".into(), ix as u64), article, cx).meta(article.meta()))
                .collect();
            div()
                .id("till-hits")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .border_1()
                .border_color(c.border)
                .bg(c.bg_surface)
                .children(rows)
                .into_any_element()
        };
        col().flex_1().min_w_0().items_center().p(m.space_5).child(
            col()
                .w_full()
                .max_w(px(720.))
                .h_full()
                .gap(m.space_4)
                .child(div().flex_none().child(Input::new(&self.search).icon(IconName::Search).tall()))
                .child(results),
        )
    }

    fn cart_panel(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let totals = self.cart.totals();
        let due = self.due();
        let tool = |id: &'static str, icon: IconName, title: SharedString| IconButton::new(id, icon).title(title).size(IconButtonSize::Lg);

        let head = row()
            .flex_none()
            .justify_between()
            .px(m.space_3)
            .py(m.space_2)
            .border_b_1()
            .border_color(c.border)
            .child(
                row()
                    .gap(m.space_2)
                    .child(Heading::new(if Self::needs_client(cx) { "Facture" } else { "Ticket" }).size(HeadingSize::Sm).caps())
                    .when(!self.cart.is_empty(), |this| this.child(Tag::new(self.cart.lines.len().to_string()).status(Status::Accent))),
            )
            .child(
                row()
                    .gap(m.space_1)
                    .child(
                        tool("till-park", IconName::Clock, "Mettre en attente".into())
                            .disabled(self.cart.is_empty() || self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.park(window, cx))),
                    )
                    .child(
                        tool("till-held", IconName::Inbox, format!("Tickets en attente ({})", self.held.len()).into())
                            .on_click(cx.listener(|this, _, window, cx| this.show_held(window, cx))),
                    )
                    .child(
                        tool("till-drawer", IconName::Banknote, "Ouvrir le tiroir".into())
                            .on_click(cx.listener(|this, _, window, cx| this.open_drawer(window, cx))),
                    )
                    .child(
                        tool("till-close", IconName::Lock, "Clôturer la session".into())
                            .on_click(cx.listener(|this, _, window, cx| this.ask_close(window, cx))),
                    ),
            );

        let lines: AnyElement = if self.cart.is_empty() {
            StateView::empty()
                .compact()
                .icon(IconName::ShoppingCart)
                .title("Panier vide")
                .body("Recherchez un article ou touchez un achat fréquent.")
                .into_any_element()
        } else {
            let rows = self.cart.lines.iter().map(|line| {
                let key = line.key;
                PosCartLine::new(
                    ElementId::NamedInteger("till-line".into(), key),
                    line.label.clone(),
                    line.quantity_label(),
                    format::euros(line.total_ttc_cents()),
                )
                .icon(if line.is_service { IconName::Wrench } else { IconName::Package })
                .meta(line.meta())
                // A scanner types where the keyboard is: each button hands it back to the search.
                .on_minus(cx.listener(move |this, _, window, cx| {
                    this.cart.bump(key, -1);
                    this.search.focus(window, cx);
                    cx.notify();
                }))
                .on_plus(cx.listener(move |this, _, window, cx| {
                    this.cart.bump(key, 1);
                    this.search.focus(window, cx);
                    cx.notify();
                }))
                .on_discount(cx.listener(move |this, _, window, cx| this.ask_discount(key, window, cx)))
                .on_remove(cx.listener(move |this, _, window, cx| {
                    this.cart.remove(key);
                    this.search.focus(window, cx);
                    cx.notify();
                }))
            });
            div().id("till-lines").size_full().overflow_y_scroll().children(rows.collect::<Vec<_>>()).into_any_element()
        };

        let hint = if due != totals.ttc_cents { format!("Reste {}", format::euros(due)) } else { TICKET_ON_REQUEST_NOTICE.to_string() };
        let foot = col()
            .flex_none()
            .gap(m.space_3)
            .p(m.space_3)
            .border_t_1()
            .border_color(c.border)
            .child(
                InfoList::new(vec![
                    InfoItem::new("HT", Some(format::euros(totals.ht_cents))).icon(IconName::FileText),
                    InfoItem::new("TVA", Some(format::euros(totals.vat_cents))).icon(IconName::Tag),
                ])
                .dense(),
            )
            .child(KpiTile::new("Total TTC", format::euros(totals.ttc_cents)).icon(IconName::Receipt).hint(hint).status(Status::Accent))
            // A key shares the width of a row; on its own it takes all of it.
            .child(
                row().child(
                    PosKey::new("till-pay", "Encaisser")
                        .icon(IconName::Check)
                        .primary()
                        .disabled(!self.can_pay(cx))
                        .on_click(cx.listener(|this, _, window, cx| this.open_pay(window, cx))),
                ),
            );

        col()
            .w(px(420.))
            .flex_none()
            .bg(c.bg_surface)
            .border_l_1()
            .border_color(c.border)
            .child(head)
            .when(Self::needs_client(cx), |this| {
                this.child(div().flex_none().p(m.space_3).border_b_1().border_color(c.border_subtle).child(Combobox::new(&self.client)))
            })
            .child(div().flex_1().min_h_0().child(lines))
            .child(row().flex_none().child(foot.flex_1()))
    }
}

impl Render for Till {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let c = theme.colors;
        let body: AnyElement = if self.loading {
            StateView::loading().title("Ouverture de la caisse…").into_any_element()
        } else if let Some(error) = self.error.clone() {
            StateView::error()
                .title("Caisse indisponible")
                .body(error)
                .action("Réessayer", cx.listener(|this, _, window, cx| this.boot(window, cx)))
                .into_any_element()
        } else if self.session.is_none() {
            self.gate(cx).into_any_element()
        } else {
            row().size_full().items_stretch().child(self.frequent_rail(cx)).child(self.finder(cx)).child(self.cart_panel(cx)).into_any_element()
        };
        col()
            .size_full()
            .bg(c.bg_app)
            .text_color(c.text)
            .font_family(FONT_UI)
            .fs(theme.m.fs_md)
            .child(self.chrome(cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::pos::SessionClosed;

    use super::closing_summary;

    #[test]
    fn a_closed_session_says_what_was_expected_counted_and_the_difference() {
        let closed = SessionClosed { expected_cash_cents: 6_998, counted_cents: 7_000, variance_cents: 2, ..Default::default() };
        assert_eq!(closing_summary(&closed), "69,98\u{a0}€ attendu, 70,00\u{a0}€ compté (excédent 0,02\u{a0}€).");
        let short = SessionClosed { expected_cash_cents: 6_998, counted_cents: 6_000, variance_cents: -998, ..Default::default() };
        assert!(closing_summary(&short).ends_with("(manquant 9,98\u{a0}€)."));
        let exact = SessionClosed { expected_cash_cents: 5_000, counted_cents: 5_000, variance_cents: 0, ..Default::default() };
        assert!(closing_summary(&exact).ends_with("(conforme 0,00\u{a0}€)."));
    }
}
