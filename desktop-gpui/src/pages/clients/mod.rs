//! Customers and their vehicles.
//!
//! The page is a paged, searchable list. A row opens the customer's panel; a vehicle of the
//! customer opens a second panel above it. Each panel shows its record, or the form that
//! creates or edits it.

mod forms;

use std::rc::Rc;
use std::time::Duration;

use motodesk_client::clients::{
    self, ClientDetail, ClientListItem, ListQuery, Vehicle, format_client_name, format_vehicle_label,
};
use motodesk_client::sales::{self, SaleListItem, status_label};
use motodesk_client::workshop::{self, WorkshopOrder};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::menus;

use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};
use crate::router::Router;
use crate::pages::vehicle::{self, VehicleForm, VehicleFormHost, vehicle_icon, vehicle_meta};
use forms::ClientForm;

const PAGE_SIZE: usize = 50;
/// How long the list waits after the last keystroke before searching.
const SEARCH_DELAY: Duration = Duration::from_millis(250);

const CLIENT: &str = "client";
const VEHICLE: &str = "vehicle";
/// The key of a panel that creates a record instead of showing one.
const NEW: &str = "new";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    View,
    Create,
    Edit,
}

pub struct ClientsPage {
    // The list.
    rows: Rc<Vec<ClientListItem>>,
    total: usize,
    page: usize,
    loading: bool,
    list_error: Option<SharedString>,
    search: TextModel,
    searches: usize,

    // The panels.
    panels: Entity<PanelStack>,
    client_mode: Mode,
    detail: Option<ClientDetail>,
    detail_loading: bool,
    detail_error: Option<SharedString>,
    /// What the customer has bought and had repaired lately, shown under their vehicles.
    history: History,
    pub(self) client_form: Option<ClientForm>,
    client_saving: bool,
    client_save_error: Option<SharedString>,

    vehicle_mode: Mode,
    vehicle: Option<Vehicle>,
    vehicle_form: Option<VehicleForm>,
    vehicle_saving: bool,
    vehicle_save_error: Option<SharedString>,

    _subscriptions: Vec<Subscription>,
}

/// How many documents and orders a customer's panel lists.
const HISTORY_ROWS: usize = 8;

/// A customer's recent documents and repair orders.
#[derive(Default)]
struct History {
    sales: Vec<SaleListItem>,
    orders: Vec<WorkshopOrder>,
}

/// « Facture · 05/11/2026 · 83,39 € »
fn sale_meta(sale: &SaleListItem) -> String {
    let mut parts = vec![status_label(&sale.status).to_string()];
    parts.extend(sale.sale_date.as_deref().map(format::date));
    parts.extend(sale.total_ttc_cents.map(format::euros));
    parts.join(" · ")
}

/// « Yamaha MT-07 (2023) · 28/11/2026 »: the status has its own tag.
fn order_meta(order: &WorkshopOrder) -> String {
    let mut parts: Vec<String> = order.vehicle_label.clone().filter(|label| !label.trim().is_empty()).into_iter().collect();
    parts.extend(order.promised_at.as_deref().map(format::date));
    parts.join(" · ")
}

/// « 3 documents récents », « 1 ordre »
fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count > 1 { many } else { one })
}

impl ClientsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = TextModel::builder().placeholder("Rechercher un client…").build(window, cx);
        let panels = cx.new(PanelStack::new);
        let subscriptions = vec![
            search.on_event(window, cx, |this, event, _, cx| {
                if event == TextEvent::Changed {
                    this.search_changed(cx);
                }
            }),
            cx.observe(&panels, |this, _, cx| this.panels_changed(cx)),
        ];

        let mut page = Self {
            rows: Rc::new(Vec::new()),
            total: 0,
            page: 0,
            loading: true,
            list_error: None,
            search,
            searches: 0,
            panels,
            client_mode: Mode::View,
            detail: None,
            detail_loading: false,
            detail_error: None,
            history: History::default(),
            client_form: None,
            client_saving: false,
            client_save_error: None,
            vehicle_mode: Mode::View,
            vehicle: None,
            vehicle_form: None,
            vehicle_saving: false,
            vehicle_save_error: None,
            _subscriptions: subscriptions,
        };
        page.load_list(cx);

        // Come back to the customer that was open when this screen was left.
        let remembered = Router::global(cx).read(cx).param("panel").map(PanelStack::parse).unwrap_or_default();
        if let Some((_, id)) = remembered.iter().find(|(kind, key)| kind == CLIENT && key != NEW) {
            page.open_client(id.clone(), None, cx);
        } else if remembered.iter().any(|(kind, key)| kind == CLIENT && key == NEW) {
            // Asked for from elsewhere (« Nouveau client… » in the menus of the dashboard).
            page.open_create_client(window, cx);
        }
        page
    }

    // ---- the list ---------------------------------------------------------------------------

    fn load_list(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.list_error = None;
        let query = ListQuery {
            q: self.search.text(cx),
            limit: Some(PAGE_SIZE as u32),
            offset: Some((self.page * PAGE_SIZE) as u32),
        };
        cx.notify();
        cx.api(
            move |client| async move { clients::list(&client, &query).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(page) => {
                        this.total = page.total.max(0) as usize;
                        this.rows = Rc::new(page.items);
                        // The last row of the last page was deleted: step back one page.
                        if this.rows.is_empty() && this.page > 0 {
                            this.page -= 1;
                            return this.load_list(cx);
                        }
                    }
                    Err(error) => {
                        this.list_error = Some(humanize_error(&error, "Chargement des clients impossible.").into())
                    }
                }
                cx.notify();
            },
        );
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        self.searches += 1;
        let this_search = self.searches;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                // Only the last keystroke of a burst searches.
                if this.searches == this_search {
                    this.page = 0;
                    this.load_list(cx);
                }
            });
        })
        .detach();
    }

    fn selected_row(&self, cx: &App) -> Option<usize> {
        let open = self.panels.read(cx).key_of(CLIENT)?;
        self.rows.iter().position(|row| row.id.as_str() == open.as_ref())
    }

    // ---- the panels -------------------------------------------------------------------------

    /// The stack changed, by this page or by the user (cross, Escape, a click beside it).
    fn panels_changed(&mut self, cx: &mut Context<Self>) {
        let (has_client, has_vehicle, serialized) = {
            let stack = self.panels.read(cx);
            (stack.key_of(CLIENT).is_some(), stack.key_of(VEHICLE).is_some(), stack.serialize())
        };
        if !has_vehicle {
            self.vehicle = None;
            self.vehicle_form = None;
            self.vehicle_save_error = None;
            self.vehicle_mode = Mode::View;
        }
        if !has_client {
            self.detail = None;
            self.detail_error = None;
            self.client_form = None;
            self.client_save_error = None;
            self.client_mode = Mode::View;
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

    fn open_client(&mut self, id: String, name: Option<String>, cx: &mut Context<Self>) {
        self.client_mode = Mode::View;
        self.client_form = None;
        self.client_save_error = None;
        self.detail = None;
        self.detail_error = None;
        self.detail_loading = true;
        self.history = History::default();
        let layer = PanelLayer::new(CLIENT, id.clone(), name.unwrap_or_else(|| "Client".into())).icon(IconName::CircleUser);
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });
        self.load_detail(id, cx);
    }

    /// Read what the customer has bought and had repaired lately. Nothing is said when it
    /// cannot be read: the panel is the customer's, with or without their history.
    fn load_history(&mut self, id: String, cx: &mut Context<Self>) {
        let asked = id.clone();
        cx.api(
            move |client| async move {
                let query = sales::ListQuery { client_id: Some(id.clone()), limit: Some(HISTORY_ROWS as u32), ..Default::default() };
                let sales = sales::list(&client, &query).await.unwrap_or_default();
                let mut orders = workshop::list_for_client(&client, &id).await.unwrap_or_default();
                orders.truncate(HISTORY_ROWS);
                History { sales, orders }
            },
            move |this, history, cx| {
                if this.panels.read(cx).key_of(CLIENT).as_deref() == Some(asked.as_str()) {
                    this.history = history;
                    cx.notify();
                }
            },
        );
    }

    fn load_detail(&mut self, id: String, cx: &mut Context<Self>) {
        self.load_history(id.clone(), cx);
        let asked = id.clone();
        cx.api(
            move |client| async move { clients::get(&client, &id).await },
            move |this, result, cx| {
                // The user may have opened another customer since.
                if this.panels.read(cx).key_of(CLIENT).as_deref() != Some(asked.as_str()) {
                    return;
                }
                this.detail_loading = false;
                match result {
                    Ok(detail) => {
                        let title = format_client_name(detail.client.first_name.as_deref(), &detail.client.last_name);
                        let subtitle = format!("Client depuis le {}", format::date(&detail.client.created_at));
                        let layer = format!("{CLIENT}:{asked}");
                        this.update_panels(cx, |stack| stack.retitle(&layer, title, Some(subtitle.into())));
                        // The vehicle panel shows a copy: refresh it too.
                        if let Some(open) = this.vehicle.as_ref().map(|v| v.id.clone()) {
                            this.vehicle = detail.vehicles.iter().find(|v| v.id == open).cloned().or(this.vehicle.take());
                        }
                        this.detail = Some(detail);
                    }
                    Err(error) => this.detail_error = Some(humanize_error(&error, "Fiche indisponible.").into()),
                }
                cx.notify();
            },
        );
    }

    fn open_create_client(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.client_mode = Mode::Create;
        self.detail = None;
        self.detail_error = None;
        self.detail_loading = false;
        self.client_save_error = None;
        let form = ClientForm::new(None, window, cx);
        form.focus(window, cx);
        self.client_form = Some(form);
        let layer = PanelLayer::new(CLIENT, NEW, "Nouveau client")
            .subtitle("Renseignez les coordonnées du client.")
            .icon(IconName::CircleUser);
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });
    }

    fn start_edit_client(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.detail.is_none() {
            return;
        }
        let form = ClientForm::new(self.detail.as_ref(), window, cx);
        form.focus(window, cx);
        self.client_form = Some(form);
        self.client_mode = Mode::Edit;
        self.client_save_error = None;
        // Editing happens on the customer's own panel: close what is above it.
        if let Some(id) = self.client_layer(cx) {
            self.update_panels(cx, |stack| stack.close_above(&id));
        }
        cx.notify();
    }

    fn cancel_edit_client(&mut self, cx: &mut Context<Self>) {
        self.client_mode = Mode::View;
        self.client_form = None;
        self.client_save_error = None;
        if let Some(id) = self.client_layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, false));
        }
        cx.notify();
    }

    fn client_layer(&self, cx: &App) -> Option<SharedString> {
        let key = self.panels.read(cx).key_of(CLIENT)?;
        Some(format!("{CLIENT}:{key}").into())
    }

    fn vehicle_layer(&self, cx: &App) -> Option<SharedString> {
        let key = self.panels.read(cx).key_of(VEHICLE)?;
        Some(format!("{VEHICLE}:{key}").into())
    }

    pub(self) fn client_form_changed(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.client_form.as_mut() else { return };
        if form.last_name_missing && !form.input(cx).last_name.is_empty() {
            form.last_name_missing = false;
        }
        let dirty = form.is_dirty(cx);
        if let Some(id) = self.client_layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, dirty));
        }
        cx.notify();
    }

    pub(self) fn submit_client(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.client_saving {
            return;
        }
        let Some(form) = self.client_form.as_mut() else { return };
        let input = form.input(cx);
        if input.last_name.is_empty() {
            form.last_name_missing = true;
            return cx.notify();
        }
        let editing = (self.client_mode == Mode::Edit).then(|| self.detail.as_ref().map(|d| d.client.id.clone())).flatten();
        self.client_saving = true;
        self.client_save_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                match editing {
                    Some(id) => clients::update(&client, &id, &input).await,
                    None => clients::create(&client, &input).await,
                }
            },
            |this, result, window, cx| {
                this.client_saving = false;
                match result {
                    Ok(saved) => {
                        let created = this.client_mode == Mode::Create;
                        toast::success(if created { "Client créé." } else { "Client enregistré." }, window, cx);
                        let name = format_client_name(saved.first_name.as_deref(), &saved.last_name);
                        this.open_client(saved.id, Some(name), cx);
                        this.load_list(cx);
                    }
                    Err(error) => {
                        this.client_save_error = Some(humanize_error(&error, "Enregistrement impossible.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    fn ask_delete_client(&mut self, id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        Confirm::new(
            "Supprimer ce client ?",
            format!(
                "Êtes-vous sûr de vouloir supprimer {name} ? Cette action est définitive. Les véhicules associés seront conservés comme « sans propriétaire »."
            ),
        )
        .confirm_label("Supprimer")
        .danger()
        .open(window, cx, move |window, cx| {
            let id = id.clone();
            page.update(cx, |_, cx| {
                cx.api_in(
                    window,
                    move |client| async move { clients::delete(&client, &id).await },
                    |this, result, window, cx| match result {
                        Ok(()) => {
                            toast::success("Client supprimé.", window, cx);
                            this.update_panels(cx, |stack| stack.close_all());
                            this.load_list(cx);
                        }
                        Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
                    },
                )
            })
        });
    }

    // ---- vehicles ---------------------------------------------------------------------------

    fn open_vehicle(&mut self, vehicle: Vehicle, cx: &mut Context<Self>) {
        let owner = self.detail.as_ref().map(|d| format_client_name(d.client.first_name.as_deref(), &d.client.last_name));
        let mut layer = PanelLayer::new(VEHICLE, vehicle.id.clone(), format_vehicle_label(&vehicle.make, &vehicle.model, vehicle.year))
            .icon(vehicle_icon(vehicle.vehicle_type));
        if let Some(owner) = owner {
            layer = layer.subtitle(owner);
        }
        self.vehicle_mode = Mode::View;
        self.vehicle_form = None;
        self.vehicle_save_error = None;
        self.vehicle = Some(vehicle);
        self.update_panels(cx, |stack| {
            stack.open(layer);
        });
    }

    fn open_create_vehicle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.detail.is_none() {
            return;
        }
        self.vehicle_mode = Mode::Create;
        self.vehicle = None;
        self.vehicle_save_error = None;
        let form = VehicleForm::new(None, window, cx);
        form.focus(window, cx);
        self.vehicle_form = Some(form);
        let layer = PanelLayer::new(VEHICLE, NEW, "Nouveau véhicule").icon(IconName::Wrench);
        self.update_panels(cx, |stack| {
            stack.open(layer);
        });
    }

    fn start_edit_vehicle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.vehicle.is_none() {
            return;
        }
        let form = VehicleForm::new(self.vehicle.as_ref(), window, cx);
        form.focus(window, cx);
        self.vehicle_form = Some(form);
        self.vehicle_mode = Mode::Edit;
        self.vehicle_save_error = None;
        cx.notify();
    }

    fn cancel_edit_vehicle(&mut self, cx: &mut Context<Self>) {
        self.vehicle_mode = Mode::View;
        self.vehicle_form = None;
        self.vehicle_save_error = None;
        if let Some(id) = self.vehicle_layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, false));
        }
        cx.notify();
    }

    fn save_vehicle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.vehicle_saving {
            return;
        }
        let Some(owner) = self.detail.as_ref().map(|d| d.client.id.clone()) else { return };
        let Some(form) = self.vehicle_form.as_mut() else { return };
        if !form.validate(cx) {
            return cx.notify();
        }
        let input = form.input(cx);
        let editing = (self.vehicle_mode == Mode::Edit).then(|| self.vehicle.as_ref().map(|v| v.id.clone())).flatten();
        self.vehicle_saving = true;
        self.vehicle_save_error = None;
        cx.notify();
        let for_owner = owner.clone();
        cx.api_in(
            window,
            move |client| async move {
                match editing {
                    Some(id) => clients::update_vehicle(&client, &id, &input).await,
                    None => clients::create_vehicle(&client, &for_owner, &input).await,
                }
            },
            move |this, result, window, cx| {
                this.vehicle_saving = false;
                match result {
                    Ok(saved) => {
                        let created = this.vehicle_mode == Mode::Create;
                        toast::success(if created { "Véhicule ajouté." } else { "Véhicule enregistré." }, window, cx);
                        // Back to the customer, whose list of vehicles is read again.
                        if let Some(layer) = this.client_layer(cx) {
                            this.update_panels(cx, |stack| stack.close_above(&layer));
                        }
                        this.open_vehicle(saved, cx);
                        this.load_detail(owner, cx);
                        this.load_list(cx);
                    }
                    Err(error) => {
                        this.vehicle_save_error = Some(humanize_error(&error, "Enregistrement impossible.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    fn ask_delete_vehicle(&mut self, vehicle: &Vehicle, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        let id = vehicle.id.clone();
        let owner = self.detail.as_ref().map(|d| d.client.id.clone());
        Confirm::new(
            "Supprimer ce véhicule ?",
            format!(
                "{} sera supprimé. Cette action est définitive.",
                format_vehicle_label(&vehicle.make, &vehicle.model, vehicle.year)
            ),
        )
        .confirm_label("Supprimer")
        .danger()
        .open(window, cx, move |window, cx| {
            let (id, owner) = (id.clone(), owner.clone());
            page.update(cx, |_, cx| {
                cx.api_in(
                    window,
                    move |client| async move { clients::delete_vehicle(&client, &id).await },
                    move |this, result, window, cx| match result {
                        Ok(()) => {
                            toast::success("Véhicule supprimé.", window, cx);
                            if let Some(layer) = this.client_layer(cx) {
                                this.update_panels(cx, |stack| stack.close_above(&layer));
                            }
                            if let Some(owner) = owner {
                                this.load_detail(owner, cx);
                            }
                            this.load_list(cx);
                        }
                        Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
                    },
                )
            })
        });
    }

    // ---- drawing ----------------------------------------------------------------------------

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let searching = !self.search.text(cx).trim().is_empty();

        if let Some(error) = self.list_error.clone() {
            return StateView::error()
                .title("Impossible de charger les clients")
                .body(error)
                .action("Réessayer", cx.listener(|this, _, _, cx| this.load_list(cx)))
                .into_any_element();
        }
        if self.loading && self.rows.is_empty() {
            return StateView::loading().title("Chargement de votre fichier client…").into_any_element();
        }
        if self.rows.is_empty() {
            let empty = StateView::empty().icon(IconName::Users);
            return if searching {
                empty.title("Aucun résultat").body("Aucun client ne correspond à votre recherche.").into_any_element()
            } else {
                empty
                    .title("Aucun client")
                    .body("Commencez par ajouter votre premier client. Vous pourrez ensuite enregistrer ses véhicules et son historique.")
                    .action("Ajouter un client", cx.listener(|this, _, window, cx| this.open_create_client(window, cx)))
                    .into_any_element()
            };
        }

        let (rows, for_click, for_menu) = (self.rows.clone(), self.rows.clone(), self.rows.clone());
        let (page_click, page_menu, page_turn) = (cx.entity(), cx.entity(), cx.entity());
        let dash = move || div().text_color(c.text_subtle).child("—").into_any_element();

        let table = DataTable::new(
            "clients-table",
            vec![
                Column::new("Client").flex(2.2),
                Column::new("Téléphone").fixed(px(150.)),
                Column::new("Courriel").flex(2.),
                Column::new("Véhicules").fixed(px(110.)).align(Align::Center),
                Column::new("Client depuis").fixed(px(150.)).right(),
            ],
            rows.len(),
            move |row, column, _, _| {
                let client = &rows[row];
                match column {
                    0 => row_name(client, c.accent_soft, c.accent_active, m.space_3, m.fs_micro),
                    1 => client.phone.clone().map(|p| p.into_any_element()).unwrap_or_else(dash),
                    2 => client
                        .email
                        .clone()
                        .map(|e| div().text_color(c.text_muted).one_line().child(e).into_any_element())
                        .unwrap_or_else(dash),
                    3 if client.vehicle_count > 0 => {
                        Tag::new(client.vehicle_count.to_string()).status(Status::Muted).into_any_element()
                    }
                    3 => dash(),
                    _ => div().text_color(c.text_muted).child(format::date(&client.created_at)).into_any_element(),
                }
            },
        )
        .compact()
        .selected(self.selected_row(cx))
        .on_row_click(move |row, _, cx| {
            let Some(client) = for_click.get(*row) else { return };
            let (id, name) = (client.id.clone(), format_client_name(client.first_name.as_deref(), &client.last_name));
            page_click.update(cx, |this, cx| this.open_client(id, Some(name), cx));
        })
        .row_menu(move |row, _, _| {
            let Some(client) = for_menu.get(row) else { return Vec::new() };
            let (id, name) = (client.id.clone(), format_client_name(client.first_name.as_deref(), &client.last_name));
            let (open, delete) = (page_menu.clone(), page_menu.clone());
            let (open_id, open_name) = (id.clone(), name.clone());
            vec![
                MenuEntry::new("Ouvrir la fiche", move |_, cx| {
                    let (id, name) = (open_id.clone(), open_name.clone());
                    open.update(cx, |this, cx| this.open_client(id, Some(name), cx))
                })
                .icon(IconName::Eye),
                MenuEntry::separator(),
                MenuEntry::new("Supprimer le client…", move |window, cx| {
                    let (id, name) = (id.clone(), name.clone());
                    delete.update(cx, |this, cx| this.ask_delete_client(id, name, window, cx))
                })
                .icon(IconName::Trash),
            ]
        });

        col()
            .size_full()
            .min_h_0()
            .child(div().flex_1().min_h_0().child(table))
            .child(Pagination::new(self.page, PAGE_SIZE, self.total).disabled(self.loading).on_page(move |page, _, cx| {
                page_turn.update(cx, |this, cx| {
                    this.page = page;
                    this.load_list(cx);
                })
            }))
            .into_any_element()
    }

    fn client_panel(&self, layer: &PanelLayer, cx: &mut Context<Self>) -> Panel {
        let m = cx.ui().m;
        let is_form = self.client_mode != Mode::View;
        let mut panel = Panel::new(&self.panels, layer.id.clone());
        if self.client_mode == Mode::Edit {
            panel = panel.badge("Édition", Status::Accent);
        }

        if is_form {
            let Some(form) = self.client_form.as_ref() else { return panel };
            let creating = self.client_mode == Mode::Create;
            let stack = self.panels.clone();
            return panel
                .child(form.render(self.client_save_error.clone(), cx))
                .footer(Button::new("client-cancel").ghost().label("Annuler").disabled(self.client_saving).on_click(
                    cx.listener(move |this, _, window, cx| {
                        if creating { request_close(&stack, window, cx) } else { this.cancel_edit_client(cx) }
                    }),
                ))
                .footer(
                    Button::new("client-save")
                        .primary()
                        .loading(self.client_saving)
                        .label(if creating { "Créer le client" } else { "Enregistrer" })
                        .on_click(cx.listener(|this, _, window, cx| this.submit_client(window, cx))),
                );
        }

        if self.detail_loading {
            return panel.child(StateView::loading().title("Chargement de la fiche…"));
        }
        if let Some(error) = self.detail_error.clone() {
            return panel.child(StateView::error().title("Fiche indisponible").body(error));
        }
        let Some(detail) = self.detail.as_ref() else { return panel };
        let client = &detail.client;
        let (id, name) = (client.id.clone(), format_client_name(client.first_name.as_deref(), &client.last_name));
        let stack = self.panels.clone();

        let contact = InfoList::new(vec![
            InfoItem::new("Téléphone", client.phone.clone()).icon(IconName::Phone),
            InfoItem::new("Courriel", client.email.clone()).icon(IconName::Mail),
            InfoItem::new("Adresse", client.address.clone()).icon(IconName::MapPin).block(),
            InfoItem::new("Encours", client.outstanding_cents.map(format::euros)),
            InfoItem::new(
                "Plafond",
                Some(if client.credit_blocked {
                    "Compte bloqué".to_string()
                } else {
                    client.credit_limit_cents.map(format::euros).unwrap_or_else(|| "Illimité".into())
                }),
            ),
            InfoItem::new("Notes", client.notes.clone()).icon(IconName::StickyNote).block(),
        ]);

        let count = detail.vehicles.len();
        let vehicles = PanelSection::new("Véhicules")
            .subtitle(format!("{count} enregistré{}", if count > 1 { "s" } else { "" }))
            .icon(IconName::Wrench)
            .flush()
            .action(Button::new("vehicle-add").small().icon(IconName::Plus).label("Ajouter").on_click(
                cx.listener(|this, _, window, cx| this.open_create_vehicle(window, cx)),
            ));
        let vehicles = if detail.vehicles.is_empty() {
            vehicles.child(
                StateView::empty()
                    .compact()
                    .icon(IconName::Wrench)
                    .title("Aucun véhicule")
                    .body("Ajoutez le premier véhicule de ce client pour suivre son entretien.")
                    .action("Ajouter un véhicule", cx.listener(|this, _, window, cx| this.open_create_vehicle(window, cx))),
            )
        } else {
            let open = self.vehicle.as_ref().map(|v| v.id.clone());
            vehicles.child(col().children(detail.vehicles.iter().map(|vehicle| {
                let to_open = vehicle.clone();
                ListRow::new(ElementId::Name(format!("vehicle-{}", vehicle.id).into()), format_vehicle_label(&vehicle.make, &vehicle.model, vehicle.year))
                    .meta(vehicle_meta(vehicle))
                    .icon(vehicle_icon(vehicle.vehicle_type))
                    .selected(open.as_deref() == Some(vehicle.id.as_str()))
                    .when(vehicle.is_stock, |row| row.trailing(Tag::new("Stock").status(Status::Accent)))
                    .trailing(Icon::new(IconName::ChevronRight).sm())
                    .on_click(cx.listener(move |this, _, _, cx| this.open_vehicle(to_open.clone(), cx)))
            })))
        };

        let (delete_id, delete_name) = (id.clone(), name.clone());
        panel
            .action(IconButton::new("client-edit", IconName::Pencil).title("Modifier la fiche").size(IconButtonSize::Md).on_click(
                cx.listener(|this, _, window, cx| this.start_edit_client(window, cx)),
            ))
            .action(
                IconButton::new("client-delete", IconName::Trash).title("Supprimer le client").size(IconButtonSize::Md).danger().on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.ask_delete_client(delete_id.clone(), delete_name.clone(), window, cx)
                    }),
                ),
            )
            .child(PanelSection::new("Coordonnées").icon(IconName::CircleUser).child(contact))
            .child(vehicles)
            .children((!self.history.sales.is_empty()).then(|| {
                PanelSection::new("Ventes").subtitle(counted(self.history.sales.len(), "document récent", "documents récents")).icon(IconName::Receipt).flush().child(
                    col().children(self.history.sales.iter().map(|sale| {
                        let title = sale.reference.clone().filter(|reference| !reference.is_empty()).unwrap_or_else(|| status_label(&sale.status).to_string());
                        let panel = format!("sale:{}", sale.id);
                        ListRow::new(ElementId::Name(format!("client-sale-{}", sale.id).into()), title)
                            .meta(sale_meta(sale))
                            .icon(IconName::Receipt)
                            .trailing(Icon::new(IconName::ChevronRight).sm())
                            .on_click(move |_, _, cx| Router::global(cx).update(cx, |router, cx| router.go_with("sales", [("panel", panel.as_str())], cx)))
                    })),
                )
            }))
            .children((!self.history.orders.is_empty()).then(|| {
                PanelSection::new("Atelier").subtitle(counted(self.history.orders.len(), "ordre", "ordres")).icon(IconName::Wrench).flush().child(
                    col().children(self.history.orders.iter().map(|order| {
                        let panel = format!("order:{}", order.id);
                        ListRow::new(ElementId::Name(format!("client-order-{}", order.id).into()), order.title())
                            .meta(order_meta(order))
                            .icon(IconName::Wrench)
                            .trailing(Tag::new(order.status.label()).status(Status::Muted))
                            .on_click(move |_, _, cx| Router::global(cx).update(cx, |router, cx| router.go_with("atelier", [("panel", panel.as_str())], cx)))
                    })),
                )
            }))
            .child(div().h(m.space_1))
            .footer(Button::new("client-close").ghost().label("Fermer").on_click(move |_, window, cx| request_close(&stack, window, cx)))
            .footer(Button::new("client-modify").primary().icon(IconName::Pencil).label("Modifier").on_click(
                cx.listener(|this, _, window, cx| this.start_edit_client(window, cx)),
            ))
    }

    fn vehicle_panel(&self, layer: &PanelLayer, cx: &mut Context<Self>) -> Panel {
        let is_form = self.vehicle_mode != Mode::View;
        let mut panel = Panel::new(&self.panels, layer.id.clone());
        let stack = self.panels.clone();

        if is_form {
            let Some(form) = self.vehicle_form.as_ref() else { return panel };
            let creating = self.vehicle_mode == Mode::Create;
            return panel
                .child(form.render(self.vehicle_save_error.clone(), cx))
                .footer(Button::new("vehicle-cancel").ghost().label("Annuler").disabled(self.vehicle_saving).on_click(
                    cx.listener(move |this, _, window, cx| {
                        if creating { request_close(&stack, window, cx) } else { this.cancel_edit_vehicle(cx) }
                    }),
                ))
                .footer(
                    Button::new("vehicle-save")
                        .primary()
                        .loading(self.vehicle_saving)
                        .label(if creating { "Ajouter le véhicule" } else { "Enregistrer" })
                        .on_click(cx.listener(|this, _, window, cx| this.save_vehicle(window, cx))),
                );
        }

        let Some(vehicle) = self.vehicle.as_ref() else { return panel };
        if vehicle.is_stock {
            panel = panel.badge("Stock", Status::Accent);
        }
        let to_delete = vehicle.clone();
        let panel = panel
            .action(IconButton::new("vehicle-edit", IconName::Pencil).title("Modifier le véhicule").size(IconButtonSize::Md).on_click(
                cx.listener(|this, _, window, cx| this.start_edit_vehicle(window, cx)),
            ))
            .action(
                IconButton::new("vehicle-delete", IconName::Trash).title("Supprimer le véhicule").size(IconButtonSize::Md).danger().on_click(
                    cx.listener(move |this, _, window, cx| this.ask_delete_vehicle(&to_delete, window, cx)),
                ),
            );

        vehicle::sections(panel, vehicle)
            .footer(Button::new("vehicle-close").ghost().label("Fermer").on_click(move |_, window, cx| request_close(&stack, window, cx)))
            .footer(Button::new("vehicle-modify").primary().icon(IconName::Pencil).label("Modifier").on_click(
                cx.listener(|this, _, window, cx| this.start_edit_vehicle(window, cx)),
            ))
    }
}

impl VehicleFormHost for ClientsPage {
    fn vehicle_form_mut(&mut self) -> Option<&mut VehicleForm> {
        self.vehicle_form.as_mut()
    }

    fn vehicle_form_changed(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.vehicle_form.as_mut() else { return };
        form.recheck(cx);
        let dirty = form.is_dirty(cx);
        if let Some(id) = self.vehicle_layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, dirty));
        }
        cx.notify();
    }

    fn submit_vehicle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_vehicle(window, cx);
    }
}

fn row_name(client: &ClientListItem, square: Hsla, ink: Hsla, gap: Pixels, size: Pixels) -> AnyElement {
    row()
        .gap(gap)
        .min_w_0()
        .child(
            row()
                .flex_none()
                .justify_center()
                .size(px(24.))
                .bg(square)
                .text_color(ink)
                .text_size(size)
                .font_weight(FontWeight::SEMIBOLD)
                .child(format::initials(client.first_name.as_deref(), &client.last_name)),
        )
        .child(div().one_line().font_weight(FontWeight::MEDIUM).child(format_client_name(client.first_name.as_deref(), &client.last_name)))
        .into_any_element()
}

impl Render for ClientsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let query = self.search.text(cx);
        let query = query.trim();
        let count = if self.loading && self.rows.is_empty() {
            "Chargement…".to_string()
        } else {
            let plural = if self.total > 1 { "s" } else { "" };
            let filter = if query.is_empty() { String::new() } else { format!(" · « {query} »") };
            format!("{} client{plural}{filter}", self.total)
        };

        let layers: Vec<PanelLayer> = self.panels.read(cx).layers().to_vec();
        let panels: Vec<Panel> = layers
            .iter()
            .map(|layer| if layer.kind.as_ref() == VEHICLE { self.vehicle_panel(layer, cx) } else { self.client_panel(layer, cx) })
            .collect();

        col()
            .relative()
            .size_full()
            .bg(c.bg_surface)
            .on_key_down(PanelHost::key_handler(&self.panels))
            .child(
                PageToolbar::new("Clients & Véhicules")
                    .icon(IconName::Users)
                    .meta(count)
                    .trailing(SearchField::new(&self.search))
                    .trailing(Button::new("client-new").primary().icon(IconName::Plus).label("Nouveau").on_click(
                        cx.listener(|this, _, window, cx| this.open_create_client(window, cx)),
                    )),
            )
            .child(div().flex_1().min_h_0().child(self.list(cx)))
            .child(PanelHost::new(&self.panels).children(panels))
    }
}

// ---- menus ----------------------------------------------------------------------------------

impl ClientsPage {
    /// The menus of the title bar while this screen is shown.
    pub fn menus(&self, cx: &mut Context<Self>) -> Vec<MenuGroup> {
        let page = cx.entity();
        let open = self.detail.as_ref().filter(|_| self.client_mode == Mode::View);
        let none_open = open.is_none();
        let target = open.map(|detail| (detail.client.id.clone(), format_client_name(detail.client.first_name.as_deref(), &detail.client.last_name)));
        vec![
            MenuGroup::new(
                "Fichier",
                vec![
                    MenuEntry::new("Nouveau client…", menus::on(&page, |this, window, cx| this.open_create_client(window, cx)))
                        .icon(IconName::UserPlus)
                        .shortcut("Ctrl+Maj+C"),
                    MenuEntry::new("Ajouter un véhicule…", menus::on(&page, |this, window, cx| this.open_create_vehicle(window, cx)))
                        .icon(IconName::Wrench)
                        .disabled(none_open),
                ],
            ),
            MenuGroup::new(
                "Édition",
                vec![
                    MenuEntry::new("Modifier la fiche client…", menus::on(&page, |this, window, cx| this.start_edit_client(window, cx)))
                        .icon(IconName::Pencil)
                        .disabled(none_open)
                        .shortcut("F2"),
                    MenuEntry::separator(),
                    MenuEntry::new(
                        "Supprimer le client…",
                        menus::on(&page, move |this, window, cx| {
                            if let Some((id, name)) = target.clone() {
                                this.ask_delete_client(id, name, window, cx);
                            }
                        }),
                    )
                    .icon(IconName::Trash)
                    .disabled(none_open),
                ],
            ),
            MenuGroup::new(
                "Affichage",
                vec![MenuEntry::new("Actualiser la liste", menus::on(&page, |this, _, cx| this.load_list(cx))).icon(IconName::RefreshCw).shortcut("F5")],
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::sales::SaleListItem;
    use motodesk_client::workshop::WorkshopOrder;

    use super::{counted, order_meta, sale_meta};

    #[test]
    fn a_document_and_an_order_say_what_matters_in_one_line() {
        let sale: SaleListItem =
            serde_json::from_value(serde_json::json!({ "id": "s1", "status": "invoice", "sale_date": "2026-11-05", "total_ttc_cents": 8339 })).unwrap();
        assert_eq!(sale_meta(&sale), "Facture · 05/11/2026 · 83,39\u{a0}€");
        let draft: SaleListItem = serde_json::from_value(serde_json::json!({ "id": "s2", "status": "draft" })).unwrap();
        assert_eq!(sale_meta(&draft), "Brouillon");

        let order: WorkshopOrder =
            serde_json::from_value(serde_json::json!({ "id": "o1", "vehicle_label": "Yamaha MT-07 (2023)", "promised_at": "2026-11-28" })).unwrap();
        assert_eq!(order_meta(&order), "Yamaha MT-07 (2023) · 28/11/2026");
        let bare: WorkshopOrder = serde_json::from_value(serde_json::json!({ "id": "o2", "vehicle_label": " " })).unwrap();
        assert_eq!(order_meta(&bare), "");

        assert_eq!(counted(1, "ordre", "ordres"), "1 ordre");
        assert_eq!(counted(3, "document récent", "documents récents"), "3 documents récents");
    }
}
