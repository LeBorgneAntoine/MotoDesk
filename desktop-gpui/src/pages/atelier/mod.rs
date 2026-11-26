//! L'Atelier (port of `routes/Atelier.svelte` and `routes/atelier/*`): the repair orders, on
//! a board with one column per step.
//!
//! A card moves by hand through "En attente", "En cours" and "Prêt". "Facturé" is not a move:
//! an order is invoiced when the invoice made from it is validated, on the sales screen.

mod form;

use std::rc::Rc;

use motodesk_client::clients::{self, ClientListItem, Vehicle, format_client_name};
use motodesk_client::employees::{self, Employee};
use motodesk_client::products::{self, ProductListItem};
use motodesk_client::workshop::{self, NewOrder, Step, WorkshopOrder};
use motodesk_client::{ApiError, settings};
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use self::form::WorkshopForm;
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};
use crate::pages::lines::{LineEditor, LinesHost};
use crate::router::Router;

/// How many products the line editor's list holds.
const PRODUCTS_LISTED: u32 = 200;
/// How many customers a customer field offers at a time.
const CLIENTS_OFFERED: u32 = 40;

const ORDER: &str = "order";
const NEW: &str = "new";
const DEFAULT_LABEL: &str = "Ordre de réparation";

fn client_option(client: &ClientListItem) -> ComboOption {
    let detail = [client.phone.as_deref(), client.email.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    ComboOption::new(client.id.clone(), format_client_name(client.first_name.as_deref(), &client.last_name)).detail(detail)
}

/// The line under an order's name on its card: whose, which vehicle, promised for when.
fn card_detail(order: &WorkshopOrder) -> String {
    [order.client_name.clone(), order.vehicle_label.clone(), order.promised_at.as_deref().map(format::date)]
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The technicians whose name or email holds what was typed.
fn matching<'a>(employees: &'a [Employee], query: &str) -> impl Iterator<Item = &'a Employee> {
    let query = query.trim().to_lowercase();
    employees
        .iter()
        .filter(move |e| query.is_empty() || e.full_name.to_lowercase().contains(&query) || e.email.to_lowercase().contains(&query))
}

/// The "Nouvel ordre de réparation" dialog.
struct NewOrderDraft {
    label: TextModel,
    client: ComboModel,
    client_searches: usize,
    promised: TextModel,
    notes: TextModel,
    busy: bool,
}

pub struct AtelierPage {
    orders: Vec<WorkshopOrder>,
    loading: bool,
    error: Option<SharedString>,

    // What the order form's lists are filled from, loaded once.
    products: Rc<Vec<ProductListItem>>,
    /// The people an order can be assigned to. Empty on a local network: the team lives in
    /// the cloud.
    employees: Vec<Employee>,
    default_vat_bp: i64,
    labour_rate_ht_cents: Option<i64>,

    panels: Entity<PanelStack>,
    order: Option<WorkshopOrder>,
    form: Option<WorkshopForm>,
    /// The vehicles of the open order's customer.
    vehicles: Vec<Vehicle>,
    saving: bool,
    save_error: Option<SharedString>,
    invoicing: bool,
    /// "Facturer" was asked on a form with unsaved edits: invoice once it is saved.
    invoice_after_save: bool,

    new_order: Option<NewOrderDraft>,
    _subscriptions: Vec<Subscription>,
}

impl AtelierPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panels = cx.new(PanelStack::new);
        let subscriptions = vec![cx.observe(&panels, |this, _, cx| this.panels_changed(cx))];
        // The order to come back to: the one another screen sent here, or the one that was
        // open when this screen was left.
        let remembered = Router::global(cx).read(cx).param("panel").map(PanelStack::parse).unwrap_or_default();
        let reopen = remembered.into_iter().find(|(kind, _)| kind == ORDER).map(|(_, key)| key);

        let mut page = Self {
            orders: Vec::new(),
            loading: true,
            error: None,
            products: Rc::new(Vec::new()),
            employees: Vec::new(),
            default_vat_bp: 2000,
            labour_rate_ht_cents: None,
            panels,
            order: None,
            form: None,
            vehicles: Vec::new(),
            saving: false,
            save_error: None,
            invoicing: false,
            invoice_after_save: false,
            new_order: None,
            _subscriptions: subscriptions,
        };
        page.refresh(cx);
        page.load_reference(reopen, window, cx);
        page
    }

    // ---- loading ----------------------------------------------------------------------------

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            |client| async move { workshop::list_board(&client).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(orders) => this.orders = orders,
                    Err(error) => this.error = Some(humanize_error(&error, "Impossible de charger l'atelier.").into()),
                }
                cx.notify();
            },
        );
    }

    /// The products, the technicians and the shop's rates the order form uses. Each is asked
    /// on its own: one that fails leaves the others usable. `reopen` is the panel to open once
    /// they are there, so that its form is complete.
    fn load_reference(&mut self, reopen: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            |client| async move {
                let catalogue = products::list(&client, &products::ListQuery { limit: Some(PRODUCTS_LISTED), ..Default::default() }).await;
                let stored = settings::list(&client).await;
                let team = employees::list(&client).await;
                Ok::<_, ApiError>((catalogue.ok(), stored.ok(), team.ok()))
            },
            move |this, result, window, cx| {
                let Ok((catalogue, stored, team)) = result else { return };
                if let Some(catalogue) = catalogue {
                    this.products = Rc::new(catalogue);
                }
                for setting in stored.into_iter().flatten() {
                    match setting.key.as_str() {
                        "vat.default_rate_bp" => {
                            let rate = setting.value.as_i64();
                            // A rate the lists cannot show would be saved without being seen.
                            this.default_vat_bp =
                                rate.filter(|rate| products::VAT_PRESETS.iter().any(|(_, bp)| bp == rate)).unwrap_or(2000);
                        }
                        "workshop.labor_rate_ht_cents" => {
                            this.labour_rate_ht_cents = setting.value.as_f64().filter(|rate| rate.is_finite()).map(|rate| rate.round() as i64);
                        }
                        _ => {}
                    }
                }
                this.employees = team.unwrap_or_default().into_iter().filter(|e| e.is_active.unwrap_or(true)).collect();
                match reopen.as_deref() {
                    Some(NEW) => this.open_new_order(window, cx),
                    Some(id) => this.open_order(id.to_string(), window, cx),
                    None => {}
                }
                cx.notify();
            },
        );
    }

    // ---- the panel --------------------------------------------------------------------------

    fn panels_changed(&mut self, cx: &mut Context<Self>) {
        let (open, serialized) = {
            let stack = self.panels.read(cx);
            (stack.is_open(), stack.serialize())
        };
        if !open {
            self.order = None;
            self.form = None;
            self.vehicles.clear();
            self.save_error = None;
            self.invoice_after_save = false;
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

    fn layer(&self, cx: &App) -> Option<SharedString> {
        self.panels.read(cx).top().map(|layer| layer.id.clone())
    }

    /// Read the order, then open its panel: an order that cannot be read opens nothing, and
    /// says why.
    fn open_order(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            move |client| async move {
                let order = workshop::get(&client, &id).await?;
                let vehicles = match order.client_id.as_deref() {
                    Some(owner) => clients::list_vehicles(&client, owner).await.unwrap_or_default(),
                    None => Vec::new(),
                };
                Ok::<_, ApiError>((order, vehicles))
            },
            |this, result, window, cx| match result {
                Ok((order, vehicles)) => {
                    this.vehicles = vehicles;
                    this.save_error = None;
                    this.invoice_after_save = false;
                    let layer = PanelLayer::new(ORDER, order.id.clone(), order.title()).icon(IconName::Wrench).width(PanelWidth::Lg);
                    let layer = match order.client_name.clone().filter(|name| !name.trim().is_empty()) {
                        Some(name) => layer.subtitle(name),
                        None => layer,
                    };
                    this.show_order(order, window, cx);
                    this.update_panels(cx, |stack| {
                        stack.close_all();
                        stack.open(layer);
                    });
                }
                Err(error) => toast::error(humanize_error(&error, "Impossible d'ouvrir l'ordre."), window, cx),
            },
        );
    }

    /// Show `order` as the service holds it: its form starts from it, so later edits are
    /// measured from here.
    fn show_order(&mut self, order: WorkshopOrder, window: &mut Window, cx: &mut Context<Self>) {
        self.form = (order.status != Step::Invoiced).then(|| {
            let mut form = WorkshopForm::new(&order, self.products.clone(), self.default_vat_bp, self.labour_rate_ht_cents, window, cx);
            form.set_vehicles(&self.vehicles, order.vehicle_id.clone(), true, window, cx);
            if let Some(id) = order.assigned_to.clone() {
                // The team may not be readable (a local network has none): the assignment is
                // kept all the same, under a name that says so.
                let name = self.employees.iter().find(|e| e.id == id).map(|e| e.full_name.clone());
                form.show_technician(ComboOption::new(id, name.unwrap_or_else(|| "Technicien assigné".into())), window, cx);
            }
            form.technician.set_options(
                self.employees.iter().map(|e| ComboOption::new(e.id.clone(), e.full_name.clone()).detail(e.email.clone())).collect(),
                cx,
            );
            form
        });
        self.order = Some(order);
        cx.notify();
    }

    /// A field of the order form changed.
    pub(super) fn form_changed(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_ref() else { return };
        let dirty = form.is_dirty(cx);
        if let Some(id) = self.layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, dirty));
        }
        cx.notify();
    }

    /// The order form's customer field asks for the customers matching what is typed.
    pub(super) fn search_form_clients(&mut self, query: String, cx: &mut Context<Self>) {
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

    /// Another customer was chosen (or none): a vehicle belongs to a customer, so the link to
    /// the vehicle goes and the list follows.
    pub(super) fn client_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else { return };
        self.vehicles.clear();
        form.set_vehicles(&[], None, false, window, cx);
        let Some(client_id) = form.client.value(cx) else { return self.form_changed(cx) };
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
                this.vehicles = result.unwrap_or_default();
                form.set_vehicles(&this.vehicles, None, false, window, cx);
                this.form_changed(cx);
            },
        );
        self.form_changed(cx);
    }

    /// The technician field asks for the people matching what is typed.
    pub(super) fn search_technicians(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_ref() else { return };
        let options =
            matching(&self.employees, &query).map(|e| ComboOption::new(e.id.clone(), e.full_name.clone()).detail(e.email.clone())).collect();
        form.technician.set_options(options, cx);
    }

    fn save_order(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let (Some(order), Some(form)) = (self.order.as_ref(), self.form.as_ref()) else { return };
        let (id, edit) = (order.id.clone(), form.edit(cx));
        self.saving = true;
        self.save_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { workshop::update(&client, &id, &edit).await },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok(saved) => {
                        if let Some(card) = this.orders.iter_mut().find(|o| o.id == saved.id) {
                            *card = saved.clone();
                        }
                        if let Some(layer) = this.layer(cx) {
                            let (title, owner) = (saved.title(), saved.client_name.clone().map(SharedString::from));
                            this.update_panels(cx, |stack| {
                                stack.retitle(&layer, title, owner);
                                stack.set_dirty(&layer, false);
                            });
                        }
                        // The form is now what the service holds: later edits are measured
                        // from here.
                        this.show_order(saved, window, cx);
                        if std::mem::take(&mut this.invoice_after_save) {
                            this.invoice(window, cx);
                        } else {
                            toast::success("Ordre enregistré.", window, cx);
                        }
                    }
                    Err(error) => {
                        this.invoice_after_save = false;
                        this.save_error = Some(humanize_error(&error, "Impossible d'enregistrer l'ordre.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    /// "Facturer": what is on screen is what is invoiced, so unsaved edits are saved first.
    fn request_invoice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.as_ref().is_some_and(|form| form.is_dirty(cx)) {
            self.invoice_after_save = true;
            self.save_order(window, cx);
        } else {
            self.invoice(window, cx);
        }
    }

    fn invoice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.order.as_ref().map(|order| order.id.clone()) else { return };
        if self.invoicing {
            return;
        }
        self.invoicing = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { workshop::invoice(&client, &id).await },
            |this, result, window, cx| {
                this.invoicing = false;
                match result {
                    Ok(sale) => {
                        toast::success(
                            if sale.refreshed_from_order {
                                "Facture brouillon mise à jour d'après l'ordre."
                            } else {
                                "Facture brouillon créée."
                            },
                            window,
                            cx,
                        );
                        open_sale(&sale.id, cx);
                    }
                    Err(error) => {
                        toast::error(humanize_error(&error, "Impossible de facturer."), window, cx);
                        cx.notify();
                    }
                }
            },
        );
    }

    // ---- the board --------------------------------------------------------------------------

    fn move_to(&mut self, id: String, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            move |client| async move { workshop::set_step(&client, &id, step).await },
            |this, result, window, cx| {
                match result {
                    Ok(moved) => {
                        if let Some(card) = this.orders.iter_mut().find(|o| o.id == moved.id) {
                            *card = moved;
                        }
                    }
                    Err(error) => toast::error(humanize_error(&error, "Impossible de changer le statut."), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn ask_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        Confirm::new(
            "Supprimer cet ordre ?",
            "L'ordre disparaît de l'atelier. L'historique Motogest importé reste en base s'il a un identifiant d'origine.",
        )
        .confirm_label("Supprimer")
        .danger()
        .open(window, cx, move |window, cx| {
            let id = id.clone();
            page.update(cx, |_, cx| {
                cx.api_in(
                    window,
                    {
                        let id = id.clone();
                        move |client| async move { workshop::delete(&client, &id).await }
                    },
                    move |this, result, window, cx| {
                        match result {
                            Ok(()) => {
                                this.orders.retain(|order| order.id != id);
                                toast::success("Ordre supprimé.", window, cx);
                            }
                            Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
                        }
                        cx.notify();
                    },
                )
            })
        });
    }

    // ---- a new order ------------------------------------------------------------------------

    fn open_new_order(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let client = ComboModel::new("Sans client — nom, téléphone, courriel…", window, cx);
        let (on_query, on_pick) = (cx.entity().downgrade(), cx.entity().downgrade());
        client.on_query(cx, move |query, _, cx| {
            let query = query.to_string();
            let _ = on_query.update(cx, |this, cx| this.search_new_order_clients(query, cx));
        });
        client.on_pick(cx, move |_, _, cx| {
            let _ = on_pick.update(cx, |_, cx| cx.notify());
        });
        let label = TextModel::builder().value(DEFAULT_LABEL).build(window, cx);
        self.new_order = Some(NewOrderDraft {
            label: label.clone(),
            client,
            client_searches: 0,
            promised: TextModel::builder().placeholder("AAAA-MM-JJ").build(window, cx),
            notes: TextModel::builder().multiline(3).build(window, cx),
            busy: false,
        });
        self.search_new_order_clients(String::new(), cx);

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Nouvel ordre de réparation").keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let Some(draft) = for_body.read(cx).new_order.as_ref() else { return element(div()) };
                element(
                    col()
                        .gap(m.space_3)
                        .child(FormField::new("Libellé").child(Input::new(&draft.label)))
                        .child(FormField::new("Client").child(Combobox::new(&draft.client)))
                        .child(FormField::new("Promis pour").child(Input::new(&draft.promised)))
                        .child(FormField::new("Notes").child(Textarea::new(&draft.notes))),
                )
            },
            move |_, cx| {
                let busy = for_footer.read(cx).new_order.as_ref().is_some_and(|draft| draft.busy);
                let page = for_footer.clone();
                vec![
                    element(
                        Button::new("order-cancel").ghost().label("Annuler").disabled(busy).on_click(|_, window, cx| close_modal(window, cx)),
                    ),
                    element(Button::new("order-create").primary().label("Créer").loading(busy).on_click(move |_, window, cx| {
                        page.update(cx, |this, cx| this.create_order(window, cx))
                    })),
                ]
            },
        );
        // After the dialog has opened (it takes the focus when it does).
        label.focus(window, cx);
    }

    fn search_new_order_clients(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(draft) = self.new_order.as_mut() else { return };
        draft.client_searches += 1;
        let this_search = draft.client_searches;
        draft.client.set_loading(true, cx);
        let query = clients::ListQuery { q: query, limit: Some(CLIENTS_OFFERED), offset: None };
        cx.api(
            move |client| async move { clients::list(&client, &query).await },
            move |this, result, cx| {
                let Some(draft) = this.new_order.as_ref() else { return };
                if draft.client_searches != this_search {
                    return;
                }
                draft.client.set_options(result.map(|page| page.items).unwrap_or_default().iter().map(client_option).collect(), cx);
            },
        );
    }

    fn create_order(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.new_order.as_mut() else { return };
        if draft.busy {
            return;
        }
        let input = NewOrder {
            label: draft.label.trimmed(cx).unwrap_or_else(|| DEFAULT_LABEL.to_string()),
            client_id: draft.client.value(cx),
            promised_at: draft.promised.trimmed(cx),
            notes: draft.notes.trimmed(cx),
            status: Step::Pending,
        };
        draft.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { workshop::create(&client, &input).await },
            |this, result, window, cx| match result {
                Ok(created) => {
                    this.new_order = None;
                    close_modal(window, cx);
                    toast::success("Ordre créé.", window, cx);
                    let id = created.id.clone();
                    this.orders.insert(0, created);
                    // A new order is there to be filled in: its panel opens.
                    this.open_order(id, window, cx);
                }
                Err(error) => {
                    if let Some(draft) = this.new_order.as_mut() {
                        draft.busy = false;
                    }
                    toast::error(humanize_error(&error, "Impossible de créer l'ordre."), window, cx);
                    cx.notify();
                }
            },
        );
    }

    // ---- drawing ----------------------------------------------------------------------------

    fn card(&self, index: usize, order: &WorkshopOrder, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let id = order.id.clone();
        let open = {
            let id = id.clone();
            move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| this.open_order(id.clone(), window, cx)
        };
        let mut actions = row().flex_wrap().gap(m.space_1).px(m.space_2).pb(m.space_2);

        if let Some(back) = order.status.previous() {
            let id = id.clone();
            actions = actions.child(Button::new(("order-back", index)).ghost().small().label("Reculer").on_click(cx.listener(
                move |this, _, window, cx| this.move_to(id.clone(), back, window, cx),
            )));
        }
        if let Some(next) = order.status.next() {
            let id = id.clone();
            actions = actions.child(Button::new(("order-next", index)).small().label(next.label()).on_click(cx.listener(
                move |this, _, window, cx| this.move_to(id.clone(), next, window, cx),
            )));
        } else if order.status == Step::Ready {
            // The last step is not a move: the order is opened to be invoiced.
            let open = open.clone();
            actions = actions.child(
                Button::new(("order-invoice", index))
                    .primary()
                    .small()
                    .icon(IconName::Receipt)
                    .label("Facturer…")
                    .on_click(cx.listener(move |this, _, window, cx| open(this, window, cx))),
            );
        } else if let (Step::Invoiced, Some(sale)) = (order.status, order.sale_id.clone()) {
            actions = actions.child(
                Button::new(("order-sale", index))
                    .small()
                    .icon(IconName::Receipt)
                    .label("Ouvrir la facture")
                    .on_click(move |_, _, cx| open_sale(&sale, cx)),
            );
        }
        if order.status != Step::Invoiced {
            let id = id.clone();
            actions = actions.child(Button::new(("order-delete", index)).ghost().small().label("Supprimer").on_click(cx.listener(
                move |this, _, window, cx| this.ask_delete(id.clone(), window, cx),
            )));
        }

        let title = order.label.clone().filter(|label| !label.trim().is_empty()).unwrap_or_else(|| "OR".into());
        let card = ListRow::new(("order", index), title).icon(IconName::Wrench).on_click(cx.listener(move |this, _, window, cx| {
            open(this, window, cx)
        }));
        let detail = card_detail(order);
        col()
            .flex_none()
            .bg(c.bg_app)
            .border_1()
            .border_color(c.border)
            .child(if detail.is_empty() { card } else { card.meta(detail) })
            .child(actions)
    }

    fn board(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if let Some(error) = self.error.clone() {
            return StateView::error()
                .title("Atelier indisponible")
                .body(error)
                .action("Réessayer", cx.listener(|this, _, _, cx| this.refresh(cx)))
                .into_any_element();
        }
        if self.loading && self.orders.is_empty() {
            return StateView::loading().title("Chargement de l'atelier…").into_any_element();
        }

        let columns = Step::ALL.into_iter().enumerate().map(|(position, step)| {
            let cards: Vec<Div> = self
                .orders
                .iter()
                .enumerate()
                .filter(|(_, order)| order.status == step)
                .map(|(index, order)| self.card(index, order, cx))
                .collect();
            let count = cards.len();
            col()
                .flex_1()
                .min_w(px(180.))
                .min_h_0()
                .bg(c.bg_surface)
                .border_1()
                .border_color(c.border)
                .child(
                    row()
                        .flex_none()
                        .justify_between()
                        .px(m.space_3)
                        .py(m.space_2)
                        .hairline_bottom(c.border)
                        .fs(m.fs_sm)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c.text_strong)
                        .child(step.label())
                        .child(Tag::new(count.to_string()).status(Status::Muted)),
                )
                .child(div().id(("board-column", position)).flex_1().min_h_0().overflow_y_scroll().child(if count == 0 {
                    div().m(m.space_3).fs(m.fs_sm).text_color(c.text_muted).child("Aucun ordre")
                } else {
                    col().p(m.space_2).gap(m.space_2).children(cards)
                }))
        });
        row().size_full().items_stretch().gap(m.space_3).p(m.space_4).children(columns).into_any_element()
    }

    fn panel(&self, layer: &PanelLayer, cx: &mut Context<Self>) -> Panel {
        let m = cx.ui().m;
        let panel = Panel::new(&self.panels, layer.id.clone());
        let Some(order) = self.order.as_ref() else { return panel };
        let stack = self.panels.clone();
        let invoiced = order.status == Step::Invoiced;
        let panel = panel.badge(order.status.label(), if invoiced { Status::Success } else { Status::Muted });

        if invoiced {
            // An invoiced order is part of a sealed sale's story: it is read, not edited.
            let summary = InfoList::new(vec![
                InfoItem::new("Client", order.client_name.clone()).placeholder("Sans client"),
                InfoItem::new("Véhicule", order.vehicle_label.clone()),
                InfoItem::new("Promis pour", order.promised_at.as_deref().map(format::date)),
                InfoItem::new("Kilométrage", order.mileage.map(format::integer)),
                InfoItem::new("Carburant", order.fuel_level.clone()),
            ]);
            let section = PanelSection::new("Ordre facturé").icon(IconName::Wrench).child(col().gap(m.space_3).child(summary).children(
                order.sale_id.clone().map(|sale| {
                    row().child(
                        Button::new("order-open-sale")
                            .icon(IconName::Receipt)
                            .label("Ouvrir la facture")
                            .on_click(move |_, _, cx| open_sale(&sale, cx)),
                    )
                }),
            ));
            let panel = panel.child(section);
            let panel = if order.lines.is_empty() {
                panel
            } else {
                panel.child(PanelSection::new("Lignes").icon(IconName::List).child(InfoList::new(
                    order
                        .lines
                        .iter()
                        .map(|line| {
                            let label = line.label.clone().filter(|l| !l.trim().is_empty()).unwrap_or_else(|| "Ligne".into());
                            InfoItem::new(label, Some(format::euros(line.unit_price_ht_cents)))
                        })
                        .collect(),
                )))
            };
            return panel.footer(
                Button::new("order-close").ghost().label("Fermer").on_click(move |_, window, cx| request_close(&stack, window, cx)),
            );
        }

        let Some(form) = self.form.as_ref() else { return panel };
        let c = cx.ui().colors;
        panel
            .child(div().p(m.space_4).bg(c.bg_surface).border_1().border_color(c.border).child(form.render(self.save_error.clone(), cx)))
            .footer(Button::new("order-cancel").ghost().label("Annuler").on_click(move |_, window, cx| request_close(&stack, window, cx)))
            .footer(
                Button::new("order-save")
                    .label("Enregistrer")
                    .loading(self.saving)
                    .on_click(cx.listener(|this, _, window, cx| this.save_order(window, cx))),
            )
            .footer(
                Button::new("order-invoice")
                    .primary()
                    .icon(IconName::Receipt)
                    .label("Facturer")
                    .loading(self.invoicing)
                    .on_click(cx.listener(|this, _, window, cx| this.request_invoice(window, cx))),
            )
    }
}

/// Show a sale on the sales screen.
fn open_sale(id: &str, cx: &mut App) {
    let panel = format!("sale:{id}");
    Router::global(cx).update(cx, |router, cx| router.go_with("sales", [("panel", panel.as_str())], cx));
}

impl LinesHost for AtelierPage {
    fn lines_mut(&mut self) -> Option<&mut LineEditor> {
        self.form.as_mut().map(|form| &mut form.lines)
    }

    fn lines_changed(&mut self, cx: &mut Context<Self>) {
        self.form_changed(cx);
    }
}

impl Render for AtelierPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let layers: Vec<PanelLayer> = self.panels.read(cx).layers().to_vec();
        let panels: Vec<Panel> = layers.iter().map(|layer| self.panel(layer, cx)).collect();

        col()
            .relative()
            .size_full()
            .bg(c.bg_app)
            .on_key_down(PanelHost::key_handler(&self.panels))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key.as_str() == "f5" {
                    this.refresh(cx);
                }
            }))
            .child(
                PageToolbar::new("L'Atelier")
                    .icon(IconName::Wrench)
                    .subtitle("Suivi des ordres de réparation.")
                    .trailing(
                        Button::new("atelier-refresh")
                            .icon(IconName::RefreshCw)
                            .label("Actualiser")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )
                    .trailing(Button::new("atelier-new").primary().icon(IconName::Plus).label("Nouvel OR").on_click(
                        cx.listener(|this, _, window, cx| this.open_new_order(window, cx)),
                    )),
            )
            .child(div().flex_1().min_h_0().child(self.board(cx)))
            .child(PanelHost::new(&self.panels).children(panels))
    }
}

// ---- menus ----------------------------------------------------------------------------------

impl AtelierPage {
    /// The menus of the title bar while this screen is shown.
    pub fn menus(&self, cx: &mut Context<Self>) -> Vec<MenuGroup> {
        let page = cx.entity();
        vec![
            MenuGroup::new(
                "Fichier",
                vec![
                    MenuEntry::new("Nouvel ordre de réparation", crate::menus::on(&page, |this, window, cx| this.open_new_order(window, cx)))
                        .icon(IconName::Wrench)
                        .shortcut("Ctrl+Maj+R"),
                ],
            ),
            MenuGroup::new(
                "Affichage",
                vec![
                    crate::menus::to_dashboard(),
                    crate::menus::to_workshop(),
                    MenuEntry::new("Actualiser", crate::menus::on(&page, |this, _, cx| this.refresh(cx))).icon(IconName::RefreshCw).shortcut("F5"),
                ],
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::employees::Employee;
    use motodesk_client::workshop::WorkshopOrder;

    use super::{card_detail, matching};

    #[test]
    fn a_card_says_whose_which_vehicle_and_for_when() {
        let full: WorkshopOrder = serde_json::from_value(serde_json::json!({
            "id": "o1", "status": "pending", "client_name": "Le Gall", "vehicle_label": "Yamaha MT-07",
            "promised_at": "2026-12-01"
        }))
        .unwrap();
        assert_eq!(card_detail(&full), "Le Gall · Yamaha MT-07 · 01/12/2026");
        let bare: WorkshopOrder = serde_json::from_value(serde_json::json!({ "id": "o2", "status": "ready", "client_name": " " })).unwrap();
        assert_eq!(card_detail(&bare), "");
    }

    #[test]
    fn a_technician_is_found_by_a_piece_of_their_name_or_email() {
        let team: Vec<Employee> = serde_json::from_value(serde_json::json!([
            { "id": "1", "email": "lea@atelier.fr", "full_name": "Léa Martin", "role": "employee", "created_at": "" },
            { "id": "2", "email": "noel@atelier.fr", "full_name": "Noël Garçon", "role": "employee", "created_at": "" }
        ]))
        .unwrap();
        let names = |query: &str| matching(&team, query).map(|e| e.full_name.as_str()).collect::<Vec<_>>();
        assert_eq!(names(""), ["Léa Martin", "Noël Garçon"]);
        assert_eq!(names("mart"), ["Léa Martin"]);
        assert_eq!(names(" NOEL@ "), ["Noël Garçon"]);
        assert!(names("camille").is_empty());
    }
}
