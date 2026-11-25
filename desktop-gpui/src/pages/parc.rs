//! The vehicles for sale, new and used (port of `Parc.svelte`).
//!
//! A list of the stock, narrowed by type in the rail and by a search. A row opens the vehicle's
//! panel; "Vendre" asks for the buyer (and a trade-in, if there is one) and creates the draft
//! invoice.

use std::rc::Rc;
use std::time::Duration;

use motodesk_client::clients::{
    self, ListQuery, Reprise, Vehicle, VehicleSellInput, VehicleType, format_client_name, format_vehicle_label,
    vehicle_margin_cents,
};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::backend::ApiExt as _;
use crate::format::{self, euros_to_cents, humanize_error};
use crate::pages::vehicle::{self, VehicleForm, VehicleFormHost};
use crate::router::Router;

/// How long the list waits after the last keystroke before searching.
const SEARCH_DELAY: Duration = Duration::from_millis(250);
/// How many customers the buyer field offers at a time.
const BUYERS_SHOWN: u32 = 40;

const VEHICLE: &str = "vehicle";
const NEW: &str = "new";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    View,
    Create,
    Edit,
}

/// The sale being prepared in the "Vendre" dialog.
struct Sell {
    vehicle: Vehicle,
    buyer: ComboModel,
    buyer_searches: usize,
    with_reprise: bool,
    make: TextModel,
    model: TextModel,
    vin: TextModel,
    plate: TextModel,
    value: TextModel,
    busy: bool,
}

pub struct ParcPage {
    vehicles: Vec<Vehicle>,
    /// The vehicles of the chosen type: what the table shows.
    shown: Rc<Vec<Vehicle>>,
    filter: Option<VehicleType>,
    loading: bool,
    error: Option<SharedString>,
    search: TextModel,
    searches: usize,
    /// A vehicle to open once the list is loaded (the one that was open when the screen was left).
    reopen: Option<String>,

    panels: Entity<PanelStack>,
    mode: Mode,
    current: Option<Vehicle>,
    vehicle_form: Option<VehicleForm>,
    saving: bool,
    save_error: Option<SharedString>,

    sell: Option<Sell>,
    _subscriptions: Vec<Subscription>,
}

impl ParcPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = TextModel::builder().placeholder("VIN, plaque, marque…").build(window, cx);
        let panels = cx.new(PanelStack::new);
        let subscriptions = vec![
            search.on_event(window, cx, |this, event, _, cx| {
                if event == TextEvent::Changed {
                    this.search_changed(cx);
                }
            }),
            cx.observe(&panels, |this, _, cx| this.panels_changed(cx)),
        ];
        let remembered = Router::global(cx).read(cx).param("panel").map(PanelStack::parse).unwrap_or_default();
        let reopen = remembered.into_iter().find(|(kind, key)| kind == VEHICLE && key != NEW).map(|(_, key)| key);

        let mut page = Self {
            vehicles: Vec::new(),
            shown: Rc::new(Vec::new()),
            filter: None,
            loading: true,
            error: None,
            search,
            searches: 0,
            reopen,
            panels,
            mode: Mode::View,
            current: None,
            vehicle_form: None,
            saving: false,
            save_error: None,
            sell: None,
            _subscriptions: subscriptions,
        };
        page.load(cx);
        page
    }

    // ---- the list ---------------------------------------------------------------------------

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let query = self.search.text(cx);
        cx.notify();
        cx.api(
            move |client| async move { clients::list_stock_vehicles(&client, &query).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(vehicles) => {
                        this.vehicles = vehicles;
                        if let Some(id) = this.reopen.take()
                            && let Some(found) = this.vehicles.iter().find(|v| v.id == id).cloned()
                        {
                            this.open_vehicle(found, cx);
                        }
                    }
                    Err(error) => {
                        this.vehicles = Vec::new();
                        this.error = Some(humanize_error(&error, "Impossible de charger le parc.").into());
                    }
                }
                this.apply_filter();
                cx.notify();
            },
        );
    }

    fn apply_filter(&mut self) {
        let filter = self.filter;
        self.shown =
            Rc::new(self.vehicles.iter().filter(|v| filter.is_none_or(|kind| v.vehicle_type == kind)).cloned().collect());
    }

    fn set_filter(&mut self, filter: Option<VehicleType>, cx: &mut Context<Self>) {
        self.filter = filter;
        self.apply_filter();
        cx.notify();
    }

    fn count(&self, kind: VehicleType) -> usize {
        self.vehicles.iter().filter(|v| v.vehicle_type == kind).count()
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        self.searches += 1;
        let this_search = self.searches;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.searches == this_search {
                    this.load(cx);
                }
            });
        })
        .detach();
    }

    // ---- the panel --------------------------------------------------------------------------

    fn panels_changed(&mut self, cx: &mut Context<Self>) {
        let (open, serialized) = {
            let stack = self.panels.read(cx);
            (stack.is_open(), stack.serialize())
        };
        if !open {
            self.current = None;
            self.vehicle_form = None;
            self.save_error = None;
            self.mode = Mode::View;
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

    fn open_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.current = None;
        self.mode = Mode::Create;
        self.save_error = None;
        let form = VehicleForm::new(None, window, cx).starting_in_stock(cx);
        form.focus(window, cx);
        self.vehicle_form = Some(form);
        let layer = PanelLayer::new(VEHICLE, NEW, "Nouveau stock").icon(IconName::Car);
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });
    }

    fn open_vehicle(&mut self, vehicle: Vehicle, cx: &mut Context<Self>) {
        let layer =
            PanelLayer::new(VEHICLE, vehicle.id.clone(), format_vehicle_label(&vehicle.make, &vehicle.model, vehicle.year))
                .icon(IconName::Car);
        self.current = Some(vehicle);
        self.mode = Mode::View;
        self.vehicle_form = None;
        self.save_error = None;
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });
    }

    fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current.is_none() {
            return;
        }
        let form = VehicleForm::new(self.current.as_ref(), window, cx);
        form.focus(window, cx);
        self.vehicle_form = Some(form);
        self.mode = Mode::Edit;
        self.save_error = None;
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.mode = Mode::View;
        self.vehicle_form = None;
        self.save_error = None;
        if let Some(id) = self.layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, false));
        }
        cx.notify();
    }

    fn save_vehicle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(form) = self.vehicle_form.as_mut() else { return };
        if !form.validate(cx) {
            return cx.notify();
        }
        let input = form.input(cx);
        let editing = (self.mode == Mode::Edit).then(|| self.current.as_ref().map(|v| v.id.clone())).flatten();
        self.saving = true;
        self.save_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                match editing {
                    Some(id) => clients::update_vehicle(&client, &id, &input).await,
                    None => clients::create_stock_vehicle(&client, &input).await,
                }
            },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok(saved) => {
                        let created = this.mode == Mode::Create;
                        toast::success(if created { "Véhicule ajouté au parc." } else { "Véhicule enregistré." }, window, cx);
                        this.open_vehicle(saved, cx);
                        this.load(cx);
                    }
                    Err(error) => {
                        this.save_error = Some(humanize_error(&error, "Impossible d'enregistrer le véhicule.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    fn ask_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        Confirm::new("Retirer ce véhicule ?", "Il disparaît du parc. Une vente déjà facturée n'est pas annulée.")
            .confirm_label("Supprimer")
            .danger()
            .open(window, cx, move |window, cx| {
                let id = id.clone();
                page.update(cx, |_, cx| {
                    cx.api_in(
                        window,
                        move |client| async move { clients::delete_vehicle(&client, &id).await },
                        |this, result, window, cx| match result {
                            Ok(()) => {
                                toast::success("Véhicule retiré du parc.", window, cx);
                                this.update_panels(cx, |stack| stack.close_all());
                                this.load(cx);
                            }
                            Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
                        },
                    )
                })
            });
    }

    // ---- selling ----------------------------------------------------------------------------

    fn start_sell(&mut self, vehicle: Vehicle, window: &mut Window, cx: &mut Context<Self>) {
        let buyer = ComboModel::new("Nom, téléphone…", window, cx);
        let (on_query, on_pick) = (cx.entity().downgrade(), cx.entity().downgrade());
        buyer.on_query(cx, move |query, _, cx| {
            let query = query.to_string();
            let _ = on_query.update(cx, |this, cx| this.search_buyers(query, cx));
        });
        buyer.on_pick(cx, move |_, _, cx| {
            let _ = on_pick.update(cx, |_, cx| cx.notify());
        });
        let line = |window: &mut Window, cx: &mut Context<Self>| TextModel::new(window, cx);
        self.sell = Some(Sell {
            vehicle,
            buyer: buyer.clone(),
            buyer_searches: 0,
            with_reprise: false,
            make: line(window, cx),
            model: line(window, cx),
            vin: line(window, cx),
            plate: line(window, cx),
            value: TextModel::builder().placeholder("0,00").build(window, cx),
            busy: false,
        });
        self.search_buyers(String::new(), cx);

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Vendre").keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let theme = cx.ui().clone();
                let (c, m) = (theme.colors, theme.m);
                let Some(sell) = for_body.read(cx).sell.as_ref() else { return element(div()) };
                let page = for_body.clone();
                let label = format_vehicle_label(&sell.vehicle.make, &sell.vehicle.model, sell.vehicle.year);
                let pair = |a: FormField, b: FormField| {
                    row().gap(m.space_3).items_start().child(div().flex_1().child(a)).child(div().flex_1().child(b))
                };
                element(
                    col()
                        .gap(m.space_4)
                        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(label))
                        .child(FormField::new("Acheteur").required().child(Combobox::new(&sell.buyer)))
                        .child(
                            Checkbox::new("with-reprise", sell.with_reprise)
                                .label("Reprise d'un véhicule")
                                .hint("Ligne négative sur la facture et entrée en stock.")
                                .on_change(move |on, _, cx| {
                                    page.update(cx, |this, cx| {
                                        if let Some(sell) = this.sell.as_mut() {
                                            sell.with_reprise = on;
                                        }
                                        cx.notify();
                                    })
                                }),
                        )
                        .when(sell.with_reprise, |this| {
                            this.child(pair(
                                FormField::new("Marque").child(Input::new(&sell.make)),
                                FormField::new("Modèle").child(Input::new(&sell.model)),
                            ))
                            .child(pair(
                                FormField::new("VIN").child(Input::new(&sell.vin).mono()),
                                FormField::new("Plaque").child(Input::new(&sell.plate).mono()),
                            ))
                            .child(FormField::new("Valeur de reprise HT (€)").child(Input::new(&sell.value)))
                        }),
                )
            },
            move |_, cx| {
                let (chosen, busy) = for_footer
                    .read(cx)
                    .sell
                    .as_ref()
                    .map(|sell| (sell.buyer.value(cx).is_some(), sell.busy))
                    .unwrap_or((false, false));
                let page = for_footer.clone();
                vec![
                    element(Button::new("sell-cancel").ghost().label("Annuler").on_click(|_, window, cx| close_modal(window, cx))),
                    element(
                        Button::new("sell-confirm").primary().label("Créer la facture").disabled(!chosen).loading(busy).on_click(
                            move |_, window, cx| page.update(cx, |this, cx| this.confirm_sell(window, cx)),
                        ),
                    ),
                ]
            },
        );
        // After the dialog has opened (it takes the focus when it does): start in the buyer field.
        buyer.focus(window, cx);
    }

    fn search_buyers(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(sell) = self.sell.as_mut() else { return };
        sell.buyer_searches += 1;
        let this_search = sell.buyer_searches;
        sell.buyer.set_loading(true, cx);
        let query = ListQuery { q: query, limit: Some(BUYERS_SHOWN), offset: None };
        cx.api(
            move |client| async move { clients::list(&client, &query).await },
            move |this, result, cx| {
                let Some(sell) = this.sell.as_ref() else { return };
                // An answer to an older keystroke must not replace a newer one.
                if sell.buyer_searches != this_search {
                    return;
                }
                let options = result
                    .map(|page| page.items)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|client| {
                        let detail = [client.phone.as_deref(), client.email.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                        ComboOption::new(client.id.clone(), format_client_name(client.first_name.as_deref(), &client.last_name))
                            .detail(detail)
                    })
                    .collect();
                sell.buyer.set_options(options, cx);
            },
        );
    }

    fn confirm_sell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sell) = self.sell.as_mut() else { return };
        let Some(client_id) = sell.buyer.value(cx) else { return };
        if sell.busy {
            return;
        }
        // A trade-in counts only once it names a vehicle.
        let reprise = match (sell.with_reprise, sell.make.trimmed(cx), sell.model.trimmed(cx)) {
            (true, Some(make), Some(model)) => Some(Reprise {
                make,
                model,
                vin_or_serial: sell.vin.trimmed(cx),
                plate_number: sell.plate.trimmed(cx),
                purchase_price_ht_cents: euros_to_cents(&sell.value.text(cx)).unwrap_or(0),
            }),
            _ => None,
        };
        let vehicle_id = sell.vehicle.id.clone();
        sell.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                clients::sell_stock_vehicle(&client, &vehicle_id, &VehicleSellInput { client_id, reprise }).await
            },
            |this, result, window, cx| {
                if let Some(sell) = this.sell.as_mut() {
                    sell.busy = false;
                }
                match result {
                    Ok(sale) => {
                        this.sell = None;
                        close_modal(window, cx);
                        toast::success("Facture brouillon créée.", window, cx);
                        // The invoice opens on the Sales screen.
                        let panel = format!("sale:{}", sale.id);
                        Router::global(cx).update(cx, |router, cx| router.go_with("sales", [("panel", panel.as_str())], cx));
                    }
                    Err(error) => toast::error(humanize_error(&error, "Impossible de vendre ce véhicule."), window, cx),
                }
                cx.notify();
            },
        );
    }

    // ---- drawing ----------------------------------------------------------------------------

    fn rail(&self, cx: &mut Context<Self>) -> FilterRail {
        let choice = |id: &'static str, title: &'static str, icon: IconName, kind: Option<VehicleType>, count: usize| {
            ListRow::new(id, title)
                .icon(icon)
                .count(count.to_string(), None)
                .selected(self.filter == kind)
                .on_click(cx.listener(move |this, _, _, cx| this.set_filter(kind, cx)))
        };
        FilterRail::new("parc-rail", "Parc")
            .child(choice("parc-all", "Tous", IconName::Car, None, self.vehicles.len()))
            .child(choice(
                "parc-moto",
                VehicleType::Moto.label(),
                IconName::Wrench,
                Some(VehicleType::Moto),
                self.count(VehicleType::Moto),
            ))
            .child(choice(
                "parc-bicycle",
                VehicleType::Bicycle.label(),
                IconName::Bike,
                Some(VehicleType::Bicycle),
                self.count(VehicleType::Bicycle),
            ))
            .child(choice(
                "parc-electric",
                VehicleType::Electric.label(),
                IconName::Zap,
                Some(VehicleType::Electric),
                self.count(VehicleType::Electric),
            ))
    }

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = cx.ui().colors;
        if let Some(error) = self.error.clone() {
            return StateView::error()
                .title("Parc indisponible")
                .body(error)
                .action("Réessayer", cx.listener(|this, _, _, cx| this.load(cx)))
                .into_any_element();
        }
        if self.loading && self.vehicles.is_empty() {
            return StateView::loading().title("Chargement du parc…").into_any_element();
        }
        if self.shown.is_empty() {
            return StateView::empty()
                .icon(IconName::Car)
                .title("Aucun véhicule en stock")
                .body("Les motos et cycles à vendre (VN/VO) apparaissent ici. Une reprise à la caisse les ajoute automatiquement.")
                .action("Ajouter un véhicule", cx.listener(|this, _, window, cx| this.open_create(window, cx)))
                .into_any_element();
        }

        let (rows, for_click) = (self.shown.clone(), self.shown.clone());
        let (page_click, page_sell) = (cx.entity(), cx.entity());
        let selected = self.current.as_ref().and_then(|open| rows.iter().position(|v| v.id == open.id));
        let dash = move || div().text_color(c.text_subtle).child("—").into_any_element();
        let amount = move |cents: Option<i64>| cents.map(|c| format::euros(c).into_any_element()).unwrap_or_else(dash);

        DataTable::new(
            "parc-table",
            vec![
                Column::new("Véhicule").flex(2.),
                Column::new("VIN").fixed(px(190.)).mono(),
                Column::new("Plaque").fixed(px(120.)),
                Column::new("Achat HT").fixed(px(120.)).right(),
                Column::new("Vente TTC").fixed(px(120.)).right(),
                Column::new("Marge").fixed(px(120.)).right(),
                Column::new("").fixed(px(96.)).right(),
            ],
            rows.len(),
            move |row, column, _, _| {
                let vehicle = &rows[row];
                match column {
                    0 => div()
                        .one_line()
                        .font_weight(FontWeight::MEDIUM)
                        .child(format_vehicle_label(&vehicle.make, &vehicle.model, vehicle.year))
                        .into_any_element(),
                    1 => vehicle.vin_or_serial.clone().map(|v| v.into_any_element()).unwrap_or_else(dash),
                    2 => vehicle.plate_number.clone().map(|p| p.into_any_element()).unwrap_or_else(dash),
                    3 => amount(vehicle.purchase_price_ht_cents),
                    4 => amount(vehicle.selling_price_ttc_cents),
                    5 => amount(vehicle_margin_cents(vehicle.purchase_price_ht_cents, vehicle.selling_price_ttc_cents)),
                    _ => {
                        let (page, to_sell) = (page_sell.clone(), vehicle.clone());
                        Button::new(("sell", row))
                            .ghost()
                            .small()
                            .label("Vendre")
                            .on_click(move |_, window, cx| {
                                // The click is for the button, not for the row under it.
                                cx.stop_propagation();
                                let vehicle = to_sell.clone();
                                page.update(cx, |this, cx| this.start_sell(vehicle, window, cx))
                            })
                            .into_any_element()
                    }
                }
            },
        )
        .compact()
        .selected(selected)
        .on_row_click(move |row, _, cx| {
            let Some(vehicle) = for_click.get(*row).cloned() else { return };
            page_click.update(cx, |this, cx| this.open_vehicle(vehicle, cx));
        })
        .into_any_element()
    }

    fn panel(&self, layer: &PanelLayer, cx: &mut Context<Self>) -> Panel {
        let mut panel = Panel::new(&self.panels, layer.id.clone());
        let stack = self.panels.clone();

        if self.mode != Mode::View {
            let Some(form) = self.vehicle_form.as_ref() else { return panel };
            let creating = self.mode == Mode::Create;
            return panel
                .child(form.render(self.save_error.clone(), cx))
                .footer(Button::new("parc-cancel").ghost().label("Annuler").disabled(self.saving).on_click(cx.listener(
                    move |this, _, window, cx| {
                        if creating { request_close(&stack, window, cx) } else { this.cancel_edit(cx) }
                    },
                )))
                .footer(
                    Button::new("parc-save")
                        .primary()
                        .loading(self.saving)
                        .label(if creating { "Ajouter le véhicule" } else { "Enregistrer" })
                        .on_click(cx.listener(|this, _, window, cx| this.save_vehicle(window, cx))),
                );
        }

        let Some(vehicle) = self.current.as_ref() else { return panel };
        if vehicle.is_stock {
            panel = panel.badge("Stock", Status::Accent);
        }
        let (to_delete, to_sell) = (vehicle.id.clone(), vehicle.clone());
        let panel = panel
            .action(IconButton::new("parc-edit", IconName::Pencil).title("Modifier le véhicule").size(IconButtonSize::Md).on_click(
                cx.listener(|this, _, window, cx| this.start_edit(window, cx)),
            ))
            .action(
                IconButton::new("parc-delete", IconName::Trash).title("Supprimer le véhicule").size(IconButtonSize::Md).danger().on_click(
                    cx.listener(move |this, _, window, cx| this.ask_delete(to_delete.clone(), window, cx)),
                ),
            );
        let modify = Button::new("parc-modify").icon(IconName::Pencil).label("Modifier").on_click(
            cx.listener(|this, _, window, cx| this.start_edit(window, cx)),
        );

        vehicle::sections(panel, vehicle)
            .footer(Button::new("parc-close").ghost().label("Fermer").on_click(move |_, window, cx| request_close(&stack, window, cx)))
            .when(vehicle.is_stock, |panel| {
                panel.footer(Button::new("parc-sell").primary().icon(IconName::ShoppingCart).label("Vendre").on_click(
                    cx.listener(move |this, _, window, cx| this.start_sell(to_sell.clone(), window, cx)),
                ))
            })
            // "Vendre" is the main action of a stock vehicle; without it, "Modifier" is.
            .footer(if vehicle.is_stock { modify } else { modify.primary() })
    }
}

impl VehicleFormHost for ParcPage {
    fn vehicle_form_mut(&mut self) -> Option<&mut VehicleForm> {
        self.vehicle_form.as_mut()
    }

    fn vehicle_form_changed(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.vehicle_form.as_mut() else { return };
        form.recheck(cx);
        let dirty = form.is_dirty(cx);
        if let Some(id) = self.layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, dirty));
        }
        cx.notify();
    }

    fn submit_vehicle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_vehicle(window, cx);
    }
}

impl Render for ParcPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let shown = self.shown.len();
        let layers: Vec<PanelLayer> = self.panels.read(cx).layers().to_vec();
        let panels: Vec<Panel> = layers.iter().map(|layer| self.panel(layer, cx)).collect();

        row()
            .relative()
            .size_full()
            .items_stretch()
            .bg(c.bg_surface)
            .on_key_down(PanelHost::key_handler(&self.panels))
            .child(self.rail(cx))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        PageToolbar::new("Parc VN/VO")
                            .icon(IconName::Car)
                            .meta(format!("{shown} véhicule{}", if shown > 1 { "s" } else { "" }))
                            .trailing(SearchField::new(&self.search))
                            .trailing(Button::new("parc-new").primary().icon(IconName::Plus).label("Nouveau").on_click(
                                cx.listener(|this, _, window, cx| this.open_create(window, cx)),
                            )),
                    )
                    .child(div().flex_1().min_h_0().child(self.list(cx))),
            )
            .child(PanelHost::new(&self.panels).children(panels))
    }
}
