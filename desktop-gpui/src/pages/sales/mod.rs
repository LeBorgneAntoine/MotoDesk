//! Ventes (port of `routes/Sales.svelte` and `routes/sales/*`): quotes, invoices, credit notes
//! and their payments.
//!
//! A document is freely edited until it is validated. Validation seals it into the fiscal
//! journal: from then on the screen offers no form, only a credit note to correct it and the
//! payments it receives.
//!
//! A sealed document and a quote are printed from here, as a PDF handed to the PC's reader
//! (see `motodesk_client::templates::print` for what the fiscal journal keeps of it).

mod forms;

use std::rc::Rc;
use std::time::Duration;

use motodesk_client::clients::{self, ClientListItem, format_client_name};
use motodesk_client::fiscal::{self, ProfileCheck};
use motodesk_client::products::{self, ProductListItem};
use motodesk_client::sales::{self, ListQuery, Payment, Sale, SaleListItem, payment_method_label, status_label};
use motodesk_client::templates::print::{self, PrintError};
use motodesk_client::{ApiError, settings, workshop};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use self::forms::{PaymentForm, SaleForm};
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};
use crate::pages::lines::{LineEditor, LinesHost, percent};
use crate::printing;
use crate::router::Router;

/// How long the list waits after the last keystroke before searching.
const SEARCH_DELAY: Duration = Duration::from_millis(250);
/// How many customers and products the form's lists hold.
const REFERENCE_ROWS: u32 = 500;
/// How many customers the form's customer field offers at a time.
const CLIENTS_OFFERED: u32 = 40;

const SALE: &str = "sale";
const PAYMENT: &str = "payment";
const NEW: &str = "new";
/// "Every status" in the list's filter.
const ALL: &str = "all";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    View,
    Create,
    Edit,
}

/// Why a document was not printed, in words.
fn print_failure(error: PrintError) -> String {
    match error {
        PrintError::Draft => "Seul un document validé ou un devis peut être imprimé.".into(),
        PrintError::NoTemplate(kind) => format!("Aucun modèle de {} n'est défini. Créez-en un dans Modèles.", kind.label().to_lowercase()),
        PrintError::Service(error) => humanize_error(&error, "Impossible d'imprimer le document."),
    }
}

/// What to say once a document is in the reader. `copy` is which copy the journal counted:
/// `None` for a quote, which the journal does not follow.
fn printed_message(reference: &str, copy: Option<(bool, i64)>) -> String {
    match copy {
        None => format!("Devis {reference} ouvert pour impression."),
        Some((false, _)) => format!("{reference} ouvert pour impression. L'impression est tracée au journal."),
        Some((true, number)) => format!("Duplicata n° {number} de {reference} ouvert pour impression. La réimpression est tracée au journal."),
    }
}

/// What to say when the PDF could not be written. When the journal has already counted the
/// print, that is said too: the next one is a copy.
fn unwritten_message(traced: bool, error: &str) -> String {
    if traced {
        format!(
            "L'impression est tracée au journal, mais le PDF n'a pas pu être écrit : {error}. \
             La prochaine impression portera la mention DUPLICATA."
        )
    } else {
        format!("Le PDF n'a pas pu être écrit : {error}")
    }
}

/// The colour of a status chip.
fn status_tone(status: &str) -> Status {
    match status {
        "paid" => Status::Success,
        "credit_note" => Status::Warning,
        "invoice" | "receipt" => Status::Info,
        "quote" => Status::Accent,
        _ => Status::Muted,
    }
}

fn client_option(client: &ClientListItem) -> ComboOption {
    let detail = [client.phone.as_deref(), client.email.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    ComboOption::new(client.id.clone(), format_client_name(client.first_name.as_deref(), &client.last_name)).detail(detail)
}

/// "2 × 9,99 € TTC · TVA 20 % · remise 10 %".
fn line_detail(line: &sales::SaleLine) -> String {
    let quantity = format::number(line.quantity_milli as f64 / 1000.);
    let unit = match line.shelf_price_cents() {
        Some(ttc) => format!("{} TTC", format::euros(ttc)),
        None => format!("{} HT", format::euros(line.unit_price_ht_cents.unwrap_or(0))),
    };
    let mut parts = vec![format!("{quantity} × {unit}"), format!("TVA {}", percent(line.vat_rate_bp))];
    if line.discount_bp > 0 {
        parts.push(format!("remise {}", percent(line.discount_bp)));
    }
    parts.join(" · ")
}

/// "05/11/2026 · Chèque · annulation".
fn payment_detail(payment: &Payment) -> String {
    let mut parts = vec![
        payment.paid_at.as_deref().map(format::date).unwrap_or_else(|| "—".into()),
        payment_method_label(payment.method.as_deref()).to_string(),
    ];
    if payment.reverses_payment_id.is_some() {
        parts.push("annulation".into());
    }
    parts.join(" · ")
}

pub struct SalesPage {
    items: Rc<Vec<SaleListItem>>,
    loading: bool,
    error: Option<SharedString>,
    search: TextModel,
    status: SelectModel,
    searches: usize,
    /// Counts the list requests, so a slow answer never replaces a newer one.
    loads: usize,

    // What the form's lists are filled from, loaded once.
    clients: Vec<ClientListItem>,
    products: Rc<Vec<ProductListItem>>,
    profile: Option<ProfileCheck>,
    default_vat_bp: i64,

    panels: Entity<PanelStack>,
    mode: Mode,
    sale: Option<Sale>,
    sale_loading: bool,
    sale_error: Option<SharedString>,
    form: Option<SaleForm>,
    saving: bool,
    save_error: Option<SharedString>,
    /// A validation, a credit note or a conversion is on its way.
    busy: bool,

    payment: Option<PaymentForm>,
    payment_saving: bool,
    payment_error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl SalesPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = TextModel::builder().placeholder("Numéro ou client…").build(window, cx);
        let mut statuses = vec![SelectOption::new(ALL, "Tous les états")];
        statuses.extend(sales::STATUS_FILTERS.iter().map(|(value, label)| SelectOption::new(*value, *label)));
        let status = SelectModel::new(statuses, Some(ALL), window, cx);
        let panels = cx.new(PanelStack::new);
        let subscriptions = vec![
            search.on_event(window, cx, |this, event, _, cx| {
                if event == TextEvent::Changed {
                    this.search_changed(cx);
                }
            }),
            status.on_change(window, cx, |this, _, _, cx| this.load_list(cx)),
            cx.observe(&panels, |this, _, cx| this.panels_changed(cx)),
        ];
        // The panel to come back to: the document another screen sent here, or the one that
        // was open when this screen was left.
        let remembered = Router::global(cx).read(cx).param("panel").map(PanelStack::parse).unwrap_or_default();
        let reopen = remembered.into_iter().find(|(kind, _)| kind == SALE).map(|(_, key)| key);

        let mut page = Self {
            items: Rc::new(Vec::new()),
            loading: true,
            error: None,
            search,
            status,
            searches: 0,
            loads: 0,
            clients: Vec::new(),
            products: Rc::new(Vec::new()),
            profile: None,
            default_vat_bp: 2000,
            panels,
            mode: Mode::View,
            sale: None,
            sale_loading: false,
            sale_error: None,
            form: None,
            saving: false,
            save_error: None,
            busy: false,
            payment: None,
            payment_saving: false,
            payment_error: None,
            _subscriptions: subscriptions,
        };
        page.load_list(cx);
        page.load_reference(reopen.as_deref() == Some(NEW), window, cx);
        if let Some(id) = reopen.filter(|key| key != NEW) {
            page.open_sale(id, cx);
        }
        page
    }

    // ---- loading ----------------------------------------------------------------------------

    fn load_list(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.loads += 1;
        let this_load = self.loads;
        let query = ListQuery {
            q: self.search.text(cx),
            status: self.status.value(cx).filter(|status| status != ALL).unwrap_or_default(),
            ..Default::default()
        };
        cx.notify();
        cx.api(
            move |client| async move { sales::list(&client, &query).await },
            move |this, result, cx| {
                if this.loads != this_load {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(found) => this.items = Rc::new(found),
                    Err(error) => {
                        this.items = Rc::new(Vec::new());
                        this.error = Some(humanize_error(&error, "Impossible de charger les ventes.").into());
                    }
                }
                cx.notify();
            },
        );
    }

    /// The customers and the products the form offers, the company profile validation checks,
    /// and the shop's usual VAT rate. Each is asked on its own: one that fails leaves the
    /// others usable. `then_create` opens a new document once they are there.
    fn load_reference(&mut self, then_create: bool, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            |client| async move {
                let customers = clients::list(&client, &clients::ListQuery { limit: Some(REFERENCE_ROWS), ..Default::default() }).await;
                let catalogue = products::list(&client, &products::ListQuery { limit: Some(REFERENCE_ROWS), ..Default::default() }).await;
                let profile = fiscal::check_profile(&client).await;
                let stored = settings::list(&client).await;
                Ok::<_, ApiError>((customers.ok(), catalogue.ok(), profile.ok(), stored.ok()))
            },
            move |this, result, window, cx| {
                let Ok((customers, catalogue, profile, stored)) = result else { return };
                if let Some(customers) = customers {
                    this.clients = customers.items;
                }
                if let Some(catalogue) = catalogue {
                    this.products = Rc::new(catalogue);
                }
                this.profile = profile.or(this.profile.take());
                let rate = stored.into_iter().flatten().find(|s| s.key == "vat.default_rate_bp").and_then(|s| s.value.as_i64());
                // A rate the lists cannot show would be saved without being seen.
                this.default_vat_bp = rate.filter(|rate| products::VAT_PRESETS.iter().any(|(_, bp)| bp == rate)).unwrap_or(2000);
                if then_create {
                    this.open_create(window, cx);
                }
                cx.notify();
            },
        );
    }

    fn refresh_profile(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { fiscal::check_profile(&client).await },
            |this, result, cx| {
                if let Ok(profile) = result {
                    this.profile = Some(profile);
                    cx.notify();
                }
            },
        );
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        self.searches += 1;
        let this_search = self.searches;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.searches == this_search {
                    this.load_list(cx);
                }
            });
        })
        .detach();
    }

    // ---- the panel stack --------------------------------------------------------------------

    fn panels_changed(&mut self, cx: &mut Context<Self>) {
        let (has_sale, has_payment, serialized) = {
            let stack = self.panels.read(cx);
            (stack.key_of(SALE).is_some(), stack.key_of(PAYMENT).is_some(), stack.serialize())
        };
        if !has_payment {
            self.payment = None;
            self.payment_error = None;
        }
        if !has_sale {
            self.mode = Mode::View;
            self.sale = None;
            self.sale_loading = false;
            self.sale_error = None;
            self.form = None;
            self.save_error = None;
        }
        Router::global(cx).update(cx, |router, _| router.set_param("panel", Some(serialized)));
        cx.notify();
    }

    fn update_panels(&self, cx: &mut Context<Self>, change: impl FnOnce(&mut PanelStack)) {
        self.panels.update(cx, |stack, cx| {
            change(stack);
            cx.notify();
        });
    }

    /// The id of the layer of this kind, when it is open.
    fn layer(&self, kind: &str, cx: &App) -> Option<SharedString> {
        self.panels.read(cx).layers().iter().find(|layer| layer.kind.as_ref() == kind).map(|layer| layer.id.clone())
    }

    fn subtitle(sale: &Sale) -> SharedString {
        match sale.locked_at.as_deref() {
            Some(sealed) => format!("Validé le {}", format::date_time(sealed)),
            None => format!("Modifié le {}", format::date_time(&sale.updated_at)),
        }
        .into()
    }

    /// Show a document that was just read or changed: the panel's title follows it.
    fn show_sale(&mut self, sale: Sale, cx: &mut Context<Self>) {
        if let Some(layer) = self.layer(SALE, cx) {
            let (title, subtitle) = (sale.title(), Self::subtitle(&sale));
            self.update_panels(cx, |stack| stack.retitle(&layer, title, Some(subtitle)));
        }
        self.sale = Some(sale);
        cx.notify();
    }

    // ---- documents --------------------------------------------------------------------------

    fn open_sale(&mut self, id: String, cx: &mut Context<Self>) {
        self.mode = Mode::View;
        self.sale = None;
        self.form = None;
        self.sale_error = None;
        self.save_error = None;
        self.sale_loading = true;
        let layer = PanelLayer::new(SALE, id.clone(), "Document").icon(IconName::Receipt).width(PanelWidth::Lg);
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });

        let asked = id.clone();
        cx.api(
            move |client| async move { sales::get(&client, &id).await },
            move |this, result, cx| {
                // The answer for a document that is no longer the open one.
                if this.panels.read(cx).key_of(SALE).as_deref() != Some(asked.as_str()) {
                    return;
                }
                this.sale_loading = false;
                match result {
                    Ok(sale) => this.show_sale(sale, cx),
                    Err(error) => {
                        this.sale_error = Some(humanize_error(&error, "Impossible de charger ce document.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    /// Read the open document again, after something changed it.
    fn reload_sale(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.sale.as_ref().map(|sale| sale.id.clone()) else { return };
        let asked = id.clone();
        cx.api(
            move |client| async move { sales::get(&client, &id).await },
            move |this, result, cx| {
                if let Ok(sale) = result
                    && this.sale.as_ref().is_some_and(|open| open.id == asked)
                {
                    this.show_sale(sale, cx);
                }
            },
        );
    }

    fn open_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sale = None;
        self.sale_error = None;
        self.sale_loading = false;
        self.mode = Mode::Create;
        self.save_error = None;
        let form = SaleForm::new(None, None, self.products.clone(), self.default_vat_bp, window, cx);
        form.client.set_options(self.clients.iter().take(CLIENTS_OFFERED as usize).map(client_option).collect(), cx);
        form.focus(window, cx);
        self.form = Some(form);
        let layer = PanelLayer::new(SALE, NEW, "Nouveau document")
            .subtitle("Brouillon ou devis — modifiable jusqu'à la validation.")
            .icon(IconName::Plus)
            .width(PanelWidth::Lg);
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });
    }

    fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sale) = self.sale.as_ref() else { return };
        // The customer's name, when the list of customers has it; otherwise it is fetched.
        let known = sale.client_id.as_ref().and_then(|id| self.clients.iter().find(|c| &c.id == id)).map(client_option);
        let (client_id, vehicle_id, sale_id) = (sale.client_id.clone(), sale.vehicle_id.clone(), sale.id.clone());
        let unknown_client = client_id.clone().filter(|_| known.is_none());
        self.form = Some(SaleForm::new(Some(sale), known, self.products.clone(), self.default_vat_bp, window, cx));
        self.mode = Mode::Edit;
        self.save_error = None;
        cx.notify();

        let Some(client_id) = client_id else { return };
        cx.api_in(
            window,
            move |client| async move {
                let vehicles = clients::list_vehicles(&client, &client_id).await.unwrap_or_default();
                let named = match unknown_client {
                    Some(id) => clients::get(&client, &id).await.ok(),
                    None => None,
                };
                Ok::<_, ApiError>((vehicles, named))
            },
            move |this, result, window, cx| {
                let Ok((vehicles, named)) = result else { return };
                // The form may have been left, or another document opened, meanwhile.
                if this.mode != Mode::Edit || this.sale.as_ref().is_none_or(|open| open.id != sale_id) {
                    return;
                }
                let Some(form) = this.form.as_mut() else { return };
                // Nothing was typed yet if the form is still as it opened: only then is it
                // right to say "this is the stored document" again after filling these in.
                let untouched = !form.is_dirty(cx);
                if let Some(detail) = named {
                    let c = &detail.client;
                    let option = ComboOption::new(c.id.clone(), format_client_name(c.first_name.as_deref(), &c.last_name));
                    form.client.set_picked(option, window, cx);
                }
                form.set_vehicles(&vehicles, vehicle_id.clone(), window, cx);
                if untouched {
                    form.mark_clean(cx);
                }
                this.form_changed(cx);
            },
        );
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.mode = Mode::View;
        self.form = None;
        self.save_error = None;
        if let Some(id) = self.layer(SALE, cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, false));
        }
        cx.notify();
    }

    /// A field of the document form changed.
    pub(super) fn form_changed(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else { return };
        form.recheck(cx);
        let dirty = form.is_dirty(cx);
        if let Some(id) = self.layer(SALE, cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, dirty));
        }
        cx.notify();
    }

    /// The form's customer field asks for the customers matching what is typed.
    pub(super) fn search_clients(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else { return };
        form.client_searches += 1;
        let this_search = form.client_searches;
        form.client.set_loading(true, cx);
        let query = clients::ListQuery { q: query, limit: Some(CLIENTS_OFFERED), offset: None };
        cx.api(
            move |client| async move { clients::list(&client, &query).await },
            move |this, result, cx| {
                let Some(form) = this.form.as_ref() else { return };
                // An answer to an older keystroke must not replace a newer one.
                if form.client_searches != this_search {
                    return;
                }
                form.client.set_options(result.map(|page| page.items).unwrap_or_default().iter().map(client_option).collect(), cx);
            },
        );
    }

    /// Another customer was chosen (or none): the vehicle list follows.
    pub(super) fn client_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else { return };
        let Some(client_id) = form.client.value(cx) else {
            form.set_vehicles(&[], None, window, cx);
            return self.form_changed(cx);
        };
        let keep = form.chosen_vehicle(cx);
        let asked = client_id.clone();
        cx.api_in(
            window,
            move |client| async move { Ok::<_, ApiError>(clients::list_vehicles(&client, &client_id).await.unwrap_or_default()) },
            move |this, result, window, cx| {
                let Some(form) = this.form.as_mut() else { return };
                // The customer changed again meanwhile.
                if form.client.value(cx).as_deref() != Some(asked.as_str()) {
                    return;
                }
                form.set_vehicles(&result.unwrap_or_default(), keep.clone(), window, cx);
                this.form_changed(cx);
            },
        );
        self.form_changed(cx);
    }

    fn save_sale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(form) = self.form.as_mut() else { return };
        let Some(input) = form.validated(cx) else { return cx.notify() };
        let editing = (self.mode == Mode::Edit).then(|| self.sale.as_ref().map(|sale| sale.id.clone())).flatten();
        self.saving = true;
        self.save_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                match editing {
                    Some(id) => sales::update(&client, &id, &input).await.map(|sale| (sale, false)),
                    None => sales::create(&client, &input).await.map(|sale| (sale, true)),
                }
            },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok((saved, created)) => {
                        toast::success(if created { "Document créé." } else { "Document enregistré." }, window, cx);
                        this.load_list(cx);
                        if created {
                            this.open_sale(saved.id, cx);
                        } else {
                            this.mode = Mode::View;
                            this.form = None;
                            if let Some(id) = this.layer(SALE, cx) {
                                this.update_panels(cx, |stack| stack.set_dirty(&id, false));
                            }
                            this.show_sale(saved, cx);
                        }
                    }
                    Err(error) => {
                        this.save_error = Some(humanize_error(&error, "Impossible d'enregistrer le document.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    fn ask_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.sale.as_ref().map(|sale| sale.id.clone()) else { return };
        let page = cx.entity();
        Confirm::new(
            "Supprimer ce brouillon ?",
            "Un brouillon n'a pas de valeur fiscale et peut être supprimé. Cette action est définitive.",
        )
        .confirm_label("Supprimer")
        .danger()
        .open(window, cx, move |window, cx| {
            let id = id.clone();
            page.update(cx, |_, cx| {
                cx.api_in(
                    window,
                    move |client| async move { sales::delete(&client, &id).await },
                    |this, result, window, cx| match result {
                        Ok(()) => {
                            toast::success("Brouillon supprimé.", window, cx);
                            this.update_panels(cx, |stack| stack.close_all());
                            this.load_list(cx);
                        }
                        Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
                    },
                )
            })
        });
    }

    // ---- the irreversible step ----------------------------------------------------------------

    fn ask_validate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.sale.as_ref().map(|sale| sale.id.clone()) else { return };
        let message = match self.profile.as_ref().and_then(ProfileCheck::blocker) {
            Some(missing) => format!(
                "Votre profil d'entreprise est incomplet : {missing}. Complétez Réglages › Légal & fiscal, sinon la validation sera refusée."
            ),
            None => "La validation attribue le numéro de facture, calcule les totaux et scelle le document dans le journal fiscal. Après cela il ne pourra plus être modifié ni supprimé : la seule correction possible sera un avoir.".to_string(),
        };
        let page = cx.entity();
        Confirm::new("Valider définitivement ce document ?", message).confirm_label("Valider et sceller").open(
            window,
            cx,
            move |window, cx| {
                let id = id.clone();
                page.update(cx, |this, cx| {
                    this.busy = true;
                    this.save_error = None;
                    cx.notify();
                    cx.api_in(
                        window,
                        move |client| async move { sales::validate(&client, &id).await },
                        |this, result, window, cx| {
                            this.busy = false;
                            match result {
                                Ok(sealed) => {
                                    let number = sealed.reference.clone().unwrap_or_default();
                                    toast::success(format!("Facture {number} validée et scellée."), window, cx);
                                    this.show_sale(sealed, cx);
                                    this.load_list(cx);
                                }
                                Err(error) => {
                                    let message = humanize_error(&error, "La validation a échoué.");
                                    toast::error(message.clone(), window, cx);
                                    this.save_error = Some(message.into());
                                    // The profile may be what blocked it: read it again so the
                                    // next question agrees.
                                    this.refresh_profile(cx);
                                    cx.notify();
                                }
                            }
                        },
                    )
                })
            },
        );
    }

    fn ask_credit_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sale) = self.sale.as_ref() else { return };
        let (id, number) = (sale.id.clone(), sale.reference.clone().unwrap_or_default());
        let amount = format::euros(-sale.total_ttc_cents.unwrap_or(0));
        let page = cx.entity();
        Confirm::new(
            "Établir un avoir ?",
            format!("Un avoir annulant {number} sera créé et scellé pour {amount}. La facture d'origine reste inchangée : c'est ainsi que la loi veut qu'une erreur soit corrigée."),
        )
        .confirm_label("Établir l'avoir")
        .open(window, cx, move |window, cx| {
            let id = id.clone();
            page.update(cx, |this, cx| {
                this.busy = true;
                cx.notify();
                cx.api_in(
                    window,
                    move |client| async move { sales::create_credit_note(&client, &id).await },
                    |this, result, window, cx| {
                        this.busy = false;
                        match result {
                            Ok(note) => {
                                let number = note.reference.clone().unwrap_or_default();
                                toast::success(format!("Avoir {number} établi."), window, cx);
                                this.load_list(cx);
                                this.open_sale(note.id, cx);
                            }
                            Err(error) => {
                                toast::error(humanize_error(&error, "Impossible d'établir l'avoir."), window, cx);
                                cx.notify();
                            }
                        }
                    },
                )
            })
        });
    }

    // ---- quotes -----------------------------------------------------------------------------

    fn convert_quote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.sale.as_ref().map(|sale| sale.id.clone()) else { return };
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { sales::convert_quote(&client, &id).await },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(draft) => {
                        toast::success("Facture brouillon établie d'après le devis. Le devis est conservé.", window, cx);
                        this.load_list(cx);
                        this.open_sale(draft.id, cx);
                    }
                    Err(error) => {
                        toast::error(humanize_error(&error, "Impossible de transformer le devis."), window, cx);
                        cx.notify();
                    }
                }
            },
        );
    }

    fn quote_to_order(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.sale.as_ref().map(|sale| sale.id.clone()) else { return };
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { workshop::create_order_from_quote(&client, &id).await },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(order) => {
                        toast::success("Ordre de réparation ouvert d'après le devis.", window, cx);
                        // The order opens on the workshop's screen.
                        let panel = format!("order:{}", order.id);
                        Router::global(cx).update(cx, |router, cx| router.go_with("atelier", [("panel", panel.as_str())], cx));
                    }
                    Err(error) => {
                        toast::error(humanize_error(&error, "Impossible d'ouvrir l'ordre de réparation."), window, cx);
                        cx.notify();
                    }
                }
            },
        );
    }

    // ---- printing ---------------------------------------------------------------------------

    /// Print the open document: its PDF goes to the program this PC reads PDF files with.
    ///
    /// The print of a sealed document is written to the fiscal journal before the page is
    /// drawn: that entry says whether this is the original or a copy, and a copy carries
    /// DUPLICATA. Everything that can fail without leaving a trace comes first (the template,
    /// the customer, the folder the file goes to), so that a failure does not use the
    /// original up. A quote takes its number here and leaves no trace.
    fn print(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, quote)) = self.sale.as_ref().map(|sale| (sale.id.clone(), sale.is_quote())) else { return };
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        let asked = id.clone();
        cx.api_in(
            window,
            move |client| async move {
                let job = print::prepare(&client, &id).await.map_err(print_failure)?;
                printing::ready().map_err(|error| format!("Le PDF ne peut pas être écrit sur ce poste : {error}"))?;
                let printed = job.finish(&client).await.map_err(print_failure)?;
                let path = printing::write(&job.file_stem(printed.trace.as_ref()), &printed.pdf)
                    .map_err(|error| unwritten_message(printed.trace.is_some(), &error.to_string()))?;
                Ok::<_, String>((job.sale, printed.trace, path))
            },
            move |this, result, window, cx| {
                this.busy = false;
                let still_open = this.mode == Mode::View && this.sale.as_ref().is_some_and(|open| open.id == asked);
                match result {
                    Ok((sale, trace, path)) => {
                        printing::open(&path, cx);
                        let copy = trace.map(|trace| (trace.duplicata, trace.copy_number));
                        toast::success(printed_message(&sale.title(), copy), window, cx);
                        if quote && still_open {
                            this.show_sale(sale, cx);
                        }
                    }
                    Err(message) => {
                        toast::error(message, window, cx);
                        // The quote may have taken its number before the failure.
                        if quote && still_open {
                            this.reload_sale(cx);
                        }
                    }
                }
                if quote {
                    this.load_list(cx);
                }
                cx.notify();
            },
        );
    }

    // ---- payments ---------------------------------------------------------------------------

    fn open_payment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sale) = self.sale.as_ref().filter(|sale| sale.is_sealed()) else { return };
        let subtitle = format!(
            "{} · {} restant",
            sale.reference.clone().unwrap_or_else(|| "Document".into()),
            format::euros(sale.outstanding_cents)
        );
        let form = PaymentForm::new(sale.outstanding_cents, window, cx);
        form.focus(window, cx);
        self.payment = Some(form);
        self.payment_error = None;
        let layer = PanelLayer::new(PAYMENT, NEW, "Encaisser un règlement")
            .subtitle(subtitle)
            .icon(IconName::Banknote)
            .width(PanelWidth::Sm);
        self.update_panels(cx, |stack| {
            stack.open(layer);
        });
        // The amount is there to be recorded: closing without doing so asks first.
        self.payment_changed(cx);
    }

    /// The amount of the payment changed.
    pub(super) fn payment_changed(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.payment.as_mut() else { return };
        form.amount_error = None;
        let dirty = form.cents(cx) != 0;
        if let Some(id) = self.layer(PAYMENT, cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, dirty));
        }
        cx.notify();
    }

    pub(super) fn save_payment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.payment_saving {
            return;
        }
        let Some(sale_id) = self.sale.as_ref().map(|sale| sale.id.clone()) else { return };
        let Some(form) = self.payment.as_mut() else { return };
        let Some(input) = form.validated(cx) else { return cx.notify() };
        self.payment_saving = true;
        self.payment_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { sales::add_payment(&client, &sale_id, &input).await },
            |this, result, window, cx| {
                this.payment_saving = false;
                match result {
                    Ok(_) => {
                        toast::success("Règlement enregistré.", window, cx);
                        if let Some(sale) = this.layer(SALE, cx) {
                            this.update_panels(cx, |stack| stack.close_above(&sale));
                        }
                        this.reload_sale(cx);
                        this.load_list(cx);
                    }
                    Err(error) => {
                        this.payment_error = Some(humanize_error(&error, "Impossible d'enregistrer le règlement.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    fn ask_reverse(&mut self, payment: &Payment, window: &mut Window, cx: &mut Context<Self>) {
        let (id, amount) = (payment.id.clone(), format::euros(-payment.amount_cents));
        let page = cx.entity();
        Confirm::new(
            "Annuler ce règlement ?",
            format!("Un mouvement inverse de {amount} sera enregistré. Le règlement initial reste visible : il n'est jamais effacé."),
        )
        .confirm_label("Enregistrer l'annulation")
        .danger()
        .open(window, cx, move |window, cx| {
            let id = id.clone();
            page.update(cx, |_, cx| {
                cx.api_in(
                    window,
                    move |client| async move { sales::reverse_payment(&client, &id).await },
                    |this, result, window, cx| match result {
                        Ok(_) => {
                            toast::success("Règlement annulé par un mouvement inverse.", window, cx);
                            this.reload_sale(cx);
                            this.load_list(cx);
                        }
                        Err(error) => toast::error(humanize_error(&error, "Impossible d'annuler ce règlement."), window, cx),
                    },
                )
            })
        });
    }

    // ---- drawing ----------------------------------------------------------------------------

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if let Some(error) = self.error.clone() {
            return StateView::error()
                .title("Ventes indisponibles")
                .body(error)
                .action("Réessayer", cx.listener(|this, _, _, cx| this.load_list(cx)))
                .into_any_element();
        }
        if self.loading && self.items.is_empty() {
            return StateView::loading().title("Chargement des ventes…").into_any_element();
        }
        if self.items.is_empty() {
            let filtered = self.search.trimmed(cx).is_some() || self.status.value(cx).is_some_and(|status| status != ALL);
            let state = StateView::empty().icon(IconName::ShoppingCart);
            return if filtered {
                state.title("Aucun résultat").body("Aucun document ne correspond à ce filtre.")
            } else {
                state
                    .title("Aucune vente pour le moment")
                    .body("Créez un devis ou une facture. Tant qu'il n'est pas validé, un document reste librement modifiable.")
                    .action("Créer un document", cx.listener(|this, _, window, cx| this.open_create(window, cx)))
            }
            .into_any_element();
        }

        let (rows, for_click) = (self.items.clone(), self.items.clone());
        let page = cx.entity();
        let open = self.panels.read(cx).key_of(SALE);
        let selected = open.and_then(|id| rows.iter().position(|row| row.id.as_str() == id.as_ref()));
        let muted = move |text: String| div().one_line().text_color(c.text_muted).child(text).into_any_element();

        DataTable::new(
            "sales-table",
            vec![
                Column::new("N°").fixed(px(180.)).mono(),
                Column::new("État").fixed(px(120.)),
                Column::new("Client").flex(1.),
                Column::new("Date").fixed(px(120.)),
                Column::new("Total TTC").fixed(px(130.)).right(),
                Column::new("Reste dû").fixed(px(130.)).right(),
            ],
            rows.len(),
            move |ix, column, _, _| {
                let sale = &rows[ix];
                match column {
                    0 => row()
                        .gap(m.space_2)
                        .min_w_0()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c.text_strong)
                        .when(sale.locked_at.is_some(), |this| this.child(Icon::new(IconName::Lock).sm().color(c.text_muted)))
                        .child(div().one_line().child(sale.reference.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "—".into())))
                        .into_any_element(),
                    1 => Tag::new(status_label(&sale.status).to_string()).status(status_tone(&sale.status)).into_any_element(),
                    2 => div()
                        .one_line()
                        .child(
                            sale.client_name
                                .as_deref()
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                                .unwrap_or("Client de passage")
                                .to_string(),
                        )
                        .into_any_element(),
                    3 => muted(sale.sale_date.as_deref().map(format::date).unwrap_or_else(|| "—".into())),
                    4 => sale.total_ttc_cents.map(format::euros).unwrap_or_else(|| "—".into()).into_any_element(),
                    _ => match sale.due_cents() {
                        Some(due) => {
                            div().font_weight(FontWeight::MEDIUM).text_color(c.warning).child(format::euros(due)).into_any_element()
                        }
                        None => muted("—".into()),
                    },
                }
            },
        )
        .compact()
        .selected(selected)
        .on_row_click(move |ix, _, cx| {
            let Some(id) = for_click.get(*ix).map(|sale| sale.id.clone()) else { return };
            page.update(cx, |this, cx| this.open_sale(id, cx));
        })
        .into_any_element()
    }

    fn payment_panel(&self, panel: Panel, cx: &mut Context<Self>) -> Panel {
        let Some(form) = self.payment.as_ref() else { return panel };
        let stack = self.panels.clone();
        panel
            .child(form.render(self.payment_error.clone(), cx))
            .footer(
                Button::new("payment-cancel")
                    .ghost()
                    .label("Annuler")
                    .disabled(self.payment_saving)
                    .on_click(move |_, window, cx| request_close(&stack, window, cx)),
            )
            .footer(
                Button::new("payment-save")
                    .primary()
                    .icon(IconName::Check)
                    .loading(self.payment_saving)
                    .label("Enregistrer l'encaissement")
                    .on_click(cx.listener(|this, _, window, cx| this.save_payment(window, cx))),
            )
    }

    /// A line saying where a document comes from or leads to, with the way there.
    fn related(&self, alert: Alert, text: &'static str, link: &'static str, id: String, cx: &mut Context<Self>) -> Alert {
        let m = cx.ui().m;
        alert.child(row().gap(m.space_2).child(text).child(
            Button::new(SharedString::from(format!("open-{id}"))).link().small().label(link).on_click(cx.listener(
                move |this, _, _, cx| this.open_sale(id.clone(), cx),
            )),
        ))
    }

    fn sale_panel(&self, panel: Panel, cx: &mut Context<Self>) -> Panel {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let stack = self.panels.clone();

        if self.sale_loading {
            return panel.child(div().h(px(240.)).child(StateView::loading().title("Chargement du document…")));
        }
        if let Some(error) = self.sale_error.clone() {
            return panel.child(div().h(px(240.)).child(StateView::error().title("Document indisponible").body(error)));
        }

        if self.mode != Mode::View {
            let Some(form) = self.form.as_ref() else { return panel };
            let creating = self.mode == Mode::Create;
            let panel = if creating { panel } else { panel.badge("Édition", Status::Accent) };
            return panel
                .child(
                    div().p(m.space_4).bg(c.bg_surface).border_1().border_color(c.border).child(form.render(self.save_error.clone(), cx)),
                )
                .footer(Button::new("sale-cancel").ghost().label("Annuler").disabled(self.saving).on_click(cx.listener(
                    move |this, _, window, cx| {
                        if creating { request_close(&stack, window, cx) } else { this.cancel_edit(cx) }
                    },
                )))
                .footer(
                    Button::new("sale-save")
                        .primary()
                        .loading(self.saving)
                        .label(if creating { "Créer" } else { "Enregistrer" })
                        .on_click(cx.listener(|this, _, window, cx| this.save_sale(window, cx))),
                );
        }

        let Some(sale) = self.sale.as_ref() else { return panel };
        let (sealed, quote, credit_note, closed_quote) =
            (sale.is_sealed(), sale.is_quote(), sale.is_credit_note(), sale.is_closed_quote());
        let tone = match sale.status.as_str() {
            "paid" => Status::Success,
            "credit_note" => Status::Warning,
            _ if sealed => Status::Info,
            _ => Status::Muted,
        };
        let mut panel = panel.badge(status_label(&sale.status).to_string(), tone);

        // What is handed to a customer: a sealed document, or a quote.
        if sealed || quote {
            panel = panel.action(
                IconButton::new("sale-print", IconName::Printer)
                    .title(if sealed { "Imprimer — toute copie porte la mention DUPLICATA" } else { "Imprimer le devis" })
                    .size(IconButtonSize::Md)
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.print(window, cx))),
            );
        }

        // What is not sealed is still a working document.
        if !sealed && !closed_quote {
            panel = panel
                .action(
                    IconButton::new("sale-edit", IconName::Pencil)
                        .title("Modifier le document")
                        .size(IconButtonSize::Md)
                        .on_click(cx.listener(|this, _, window, cx| this.start_edit(window, cx))),
                )
                .action(
                    IconButton::new("sale-delete", IconName::Trash)
                        .title(if quote { "Supprimer le devis" } else { "Supprimer le brouillon" })
                        .size(IconButtonSize::Md)
                        .danger()
                        .on_click(cx.listener(|this, _, window, cx| this.ask_delete(window, cx))),
                );
        }

        if sealed {
            panel = panel.child(Alert::info().child(row().gap(m.space_2).items_start().child(Icon::new(IconName::Lock).sm()).child(
                div().flex_1().min_w_0().child(
                    "Ce document est scellé dans le journal fiscal. Il ne peut plus être modifié ni supprimé : une correction se fait par avoir (CGI art. 286 I-3° bis).",
                ),
            )));
        }
        if let Some(error) = self.save_error.clone() {
            panel = panel.child(Alert::danger().child(error));
        }
        if let (true, Some(invoice)) = (quote, sale.converted_to_sale_id.clone()) {
            panel = panel.child(self.related(
                Alert::info(),
                "Ce devis a été transformé en facture : il est conservé tel qu'il a été envoyé.",
                "Ouvrir la facture",
                invoice,
                cx,
            ));
        }
        if let Some(source) = sale.source_quote_id.clone() {
            panel = panel.child(self.related(Alert::info(), "Facture établie d'après un devis.", "Ouvrir le devis", source, cx));
        }
        if let Some(corrected) = sale.corrects_sale_id.clone() {
            panel = panel.child(self.related(
                Alert::warning(),
                "Cet avoir corrige un document validé.",
                "Ouvrir la facture d'origine",
                corrected,
                cx,
            ));
        }

        // The customer's name comes from the list, which has it; the document only has an id.
        let customer = self.items.iter().find(|item| item.id == sale.id).and_then(|item| item.client_name.clone());
        let mut summary = vec![
            InfoItem::new("Numéro", sale.reference.clone())
                .icon(IconName::Hash)
                .mono()
                .placeholder(if quote { "Attribué à l'impression du devis" } else { "Attribué à la validation" }),
            InfoItem::new("Client", customer.filter(|name| !name.trim().is_empty()))
                .icon(IconName::CircleUser)
                .placeholder("Client de passage"),
            InfoItem::new("Date", sale.sale_date.as_deref().map(format::date)).icon(IconName::Calendar),
            InfoItem::new("Total HT", sale.total_ht_cents.map(format::euros)).icon(IconName::FileText),
            InfoItem::new("TVA", sale.total_vat_cents.map(format::euros)).icon(IconName::Receipt),
            InfoItem::new("Total TTC", sale.total_ttc_cents.map(format::euros)).icon(IconName::Banknote),
            InfoItem::new("Reste à payer", Some(format::euros(sale.outstanding_cents))).icon(IconName::Clock),
        ];
        if let Some(notes) = sale.notes.clone().filter(|notes| !notes.trim().is_empty()) {
            summary.push(InfoItem::new("Notes", Some(notes)).icon(IconName::StickyNote).block());
        }
        panel = panel.child(PanelSection::new("Récapitulatif").icon(IconName::FileText).child(InfoList::new(summary)));

        let count = sale.lines.len();
        let lines = PanelSection::new("Lignes")
            .subtitle(format!("{count} ligne{}", if count > 1 { "s" } else { "" }))
            .icon(IconName::List)
            .flush();
        panel = panel.child(if count == 0 {
            lines.child(div().h(px(120.)).child(StateView::empty().compact().icon(IconName::List).title("Aucune ligne")))
        } else {
            lines.child(col().children(sale.lines.iter().enumerate().map(|(index, line)| {
                let label = line.label.as_deref().map(str::trim).filter(|l| !l.is_empty()).unwrap_or("Ligne sans désignation");
                ListRow::new(("sale-line", index), label.to_string()).meta(line_detail(line)).icon(IconName::Package).trailing(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c.text_strong)
                        .child(line.line_total_ttc_cents.map(format::euros).unwrap_or_else(|| "—".into())),
                )
            })))
        });

        let settled = sale.outstanding_cents <= 0 && sale.paid_cents != 0;
        let mut payments = PanelSection::new("Règlements")
            .subtitle(if settled { "Soldé".to_string() } else { format!("{} restant", format::euros(sale.outstanding_cents)) })
            .icon(IconName::Banknote)
            .flush();
        if sealed && !credit_note {
            payments = payments.action(
                Button::new("payment-add")
                    .small()
                    .icon(IconName::Plus)
                    .label("Encaisser")
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.open_payment(window, cx))),
            );
        }
        panel = panel.child(if sale.payments.is_empty() {
            payments.child(
                div().h(px(150.)).child(
                    StateView::empty().compact().icon(IconName::Banknote).title("Aucun règlement").body(if sealed {
                        "Enregistrez l'encaissement dès qu'il est reçu."
                    } else {
                        "Les règlements s'enregistrent une fois le document validé."
                    }),
                ),
            )
        } else {
            payments.child(col().children(sale.payments.iter().enumerate().map(|(index, payment)| {
                let line = ListRow::new(("sale-payment", index), format::euros(payment.amount_cents))
                    .meta(payment_detail(payment))
                    .icon(if payment.amount_cents < 0 { IconName::Undo } else { IconName::Banknote });
                if payment.is_reversible() {
                    let to_reverse = payment.clone();
                    line.trailing(
                        IconButton::new(("payment-reverse", index), IconName::Undo).title("Annuler ce règlement").danger().on_click(
                            cx.listener(move |this, _, window, cx| this.ask_reverse(&to_reverse, window, cx)),
                        ),
                    )
                } else {
                    line.trailing(Tag::new("Annulation").status(Status::Muted))
                }
            })))
        });

        let mut panel = panel.footer(
            Button::new("sale-close").ghost().label("Fermer").on_click(move |_, window, cx| request_close(&stack, window, cx)),
        );
        let modify = || {
            Button::new("sale-modify").icon(IconName::Pencil).label("Modifier").on_click(cx.listener(|this, _, window, cx| {
                this.start_edit(window, cx)
            }))
        };
        if quote {
            if !closed_quote {
                panel = panel
                    .footer(modify())
                    .footer(
                        Button::new("quote-order")
                            .icon(IconName::Wrench)
                            .label("Ordre de réparation")
                            .loading(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.quote_to_order(window, cx))),
                    )
                    .footer(
                        Button::new("quote-convert")
                            .primary()
                            .icon(IconName::Receipt)
                            .label("Transformer en facture")
                            .loading(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.convert_quote(window, cx))),
                    );
            }
        } else if !sealed {
            panel = panel.footer(modify()).footer(
                Button::new("sale-validate")
                    .primary()
                    .icon(IconName::Lock)
                    .label("Valider la facture")
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.ask_validate(window, cx))),
            );
        } else if !credit_note {
            panel = panel.footer(
                Button::new("sale-credit")
                    .icon(IconName::Undo)
                    .label("Établir un avoir")
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.ask_credit_note(window, cx))),
            );
        }
        panel
    }

    fn panel(&self, layer: &PanelLayer, cx: &mut Context<Self>) -> Panel {
        let panel = Panel::new(&self.panels, layer.id.clone());
        if layer.kind.as_ref() == PAYMENT { self.payment_panel(panel, cx) } else { self.sale_panel(panel, cx) }
    }
}

impl LinesHost for SalesPage {
    fn lines_mut(&mut self) -> Option<&mut LineEditor> {
        self.form.as_mut().map(|form| &mut form.lines)
    }

    fn lines_changed(&mut self, cx: &mut Context<Self>) {
        self.form_changed(cx);
    }
}

impl Render for SalesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let shown = self.items.len();
        let count = if self.loading {
            "Chargement…".to_string()
        } else {
            let searched = self.search.trimmed(cx).map(|term| format!(" · « {term} »")).unwrap_or_default();
            format!("{shown} document{}{searched}", if shown > 1 { "s" } else { "" })
        };
        let layers: Vec<PanelLayer> = self.panels.read(cx).layers().to_vec();
        let panels: Vec<Panel> = layers.iter().map(|layer| self.panel(layer, cx)).collect();

        col()
            .relative()
            .size_full()
            .bg(c.bg_surface)
            .on_key_down(PanelHost::key_handler(&self.panels))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key.as_str() == "f5" {
                    this.load_list(cx);
                }
            }))
            .child(
                PageToolbar::new("Ventes")
                    .icon(IconName::ShoppingCart)
                    .meta(count)
                    .trailing(div().w(px(170.)).child(Select::new(&self.status)))
                    .trailing(SearchField::new(&self.search))
                    .trailing(Button::new("sale-new").primary().icon(IconName::Plus).label("Nouveau").on_click(
                        cx.listener(|this, _, window, cx| this.open_create(window, cx)),
                    )),
            )
            .child(div().flex_1().min_h_0().child(self.list(cx)))
            .child(PanelHost::new(&self.panels).children(panels))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::ApiError;
    use motodesk_client::sales::{Payment, SaleLine};
    use motodesk_client::templates::TemplateKind;
    use motodesk_client::templates::print::PrintError;

    use super::{Status, line_detail, payment_detail, print_failure, printed_message, status_tone, unwritten_message};

    #[test]
    fn a_line_says_how_it_is_priced() {
        let shelf: SaleLine = serde_json::from_value(serde_json::json!({
            "id": "l1", "quantity_milli": 2000, "unit_price_ht_cents": 833, "unit_price_ttc_cents": 999,
            "price_basis": "ttc", "vat_rate_bp": 2000, "discount_bp": 0
        }))
        .unwrap();
        assert_eq!(line_detail(&shelf), "2 × 9,99\u{a0}€ TTC · TVA 20 %");

        let labour: SaleLine = serde_json::from_value(serde_json::json!({
            "id": "l2", "quantity_milli": 1500, "unit_price_ht_cents": 6000, "unit_price_ttc_cents": 7200,
            "price_basis": "ht", "vat_rate_bp": 550, "discount_bp": 1000
        }))
        .unwrap();
        assert_eq!(line_detail(&labour), "1,5 × 60,00\u{a0}€ HT · TVA 5,5 % · remise 10 %");
    }

    #[test]
    fn a_payment_says_when_how_and_whether_it_takes_one_back() {
        let receipt: Payment = serde_json::from_value(serde_json::json!({
            "id": "p1", "amount_cents": 5000, "paid_at": "2026-11-05T10:00:00Z", "method": "cheque"
        }))
        .unwrap();
        assert_eq!(payment_detail(&receipt), "05/11/2026 · Chèque");
        let reversal: Payment = serde_json::from_value(serde_json::json!({
            "id": "p2", "amount_cents": -5000, "paid_at": "2026-11-06", "method": "cheque", "reverses_payment_id": "p1"
        }))
        .unwrap();
        assert_eq!(payment_detail(&reversal), "06/11/2026 · Chèque · annulation");
    }

    #[test]
    fn a_print_says_what_it_was_and_a_failure_says_what_to_do() {
        assert_eq!(printed_message("DE-2026-C1-00003", None), "Devis DE-2026-C1-00003 ouvert pour impression.");
        assert_eq!(
            printed_message("FA-2026-C1-00007", Some((false, 1))),
            "FA-2026-C1-00007 ouvert pour impression. L'impression est tracée au journal."
        );
        assert_eq!(
            printed_message("FA-2026-C1-00007", Some((true, 2))),
            "Duplicata n° 2 de FA-2026-C1-00007 ouvert pour impression. La réimpression est tracée au journal."
        );
        assert_eq!(print_failure(PrintError::NoTemplate(TemplateKind::Devis)), "Aucun modèle de devis n'est défini. Créez-en un dans Modèles.");
        let missing = print_failure(PrintError::NoTemplate(TemplateKind::Facture));
        assert_eq!(missing, "Aucun modèle de facture n'est défini. Créez-en un dans Modèles.");
        assert!(unwritten_message(true, "disque plein").contains("tracée au journal") && unwritten_message(true, "x").contains("DUPLICATA"));
        assert_eq!(unwritten_message(false, "disque plein"), "Le PDF n'a pas pu être écrit : disque plein");
        let refused = ApiError::new(400, "bad_request", "seul un document validé peut être imprimé");
        assert_eq!(print_failure(PrintError::Service(refused)), "seul un document validé peut être imprimé");
        assert_eq!(print_failure(PrintError::Service(ApiError::network(""))), "Impossible d'imprimer le document.");
    }

    #[test]
    fn each_status_has_its_colour() {
        assert_eq!(status_tone("paid"), Status::Success);
        assert_eq!(status_tone("credit_note"), Status::Warning);
        assert_eq!(status_tone("invoice"), Status::Info);
        assert_eq!(status_tone("quote"), Status::Accent);
        assert_eq!(status_tone("draft"), Status::Muted);
    }
}
