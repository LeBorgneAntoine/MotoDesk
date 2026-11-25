//! The form of a repair order (port of `WorkshopForm.svelte`): who and what it is for, who
//! works on it, and the parts and labour that will be invoiced.
//!
//! The lines are edited by the editor shared with the sales documents: see `pages/lines.rs`.

use std::rc::Rc;

use motodesk_client::clients::{Vehicle, format_vehicle_label};
use motodesk_client::products::ProductListItem;
use motodesk_client::workshop::{OrderEdit, WorkshopLineInput, WorkshopOrder};
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::AtelierPage;
use crate::pages::lines::{Line, LineEditor, LineSnapshot, NONE, to_number};

const DEFAULT_LABEL: &str = "Ordre de réparation";
/// (value, label) of how full the tank was when the vehicle came in.
const FUEL_LEVELS: [(&str, &str); 4] = [("1/4", "1/4"), ("1/2", "1/2"), ("3/4", "3/4"), ("plein", "Plein")];

/// The form as text: what "has anything changed?" compares.
#[derive(Debug, Clone, Default, PartialEq)]
struct Snapshot {
    label: String,
    promised: String,
    client: Option<String>,
    vehicle: Option<String>,
    technician: Option<String>,
    mileage: String,
    fuel: Option<String>,
    notes: String,
    lines: Vec<LineSnapshot>,
}

pub struct WorkshopForm {
    label: TextModel,
    promised: TextModel,
    pub client: ComboModel,
    pub client_searches: usize,
    vehicle: SelectModel,
    has_vehicles: bool,
    pub technician: ComboModel,
    mileage: TextModel,
    fuel: SelectModel,
    notes: TextModel,
    pub lines: LineEditor,
    started_as: Snapshot,
    _subscriptions: Vec<Subscription>,
}

fn vehicle_options(vehicles: &[(String, String)]) -> Vec<SelectOption> {
    let nothing = if vehicles.is_empty() { "Aucun véhicule" } else { "Pas de véhicule lié" };
    let mut options = vec![SelectOption::new(NONE, nothing)];
    options.extend(vehicles.iter().map(|(id, label)| SelectOption::new(id.clone(), label.clone())));
    options
}

impl WorkshopForm {
    /// The form of `order`, as the service holds it. `labour_rate_ht_cents` prices a service
    /// that has no price of its own.
    pub fn new(
        order: &WorkshopOrder,
        products: Rc<Vec<ProductListItem>>,
        default_vat_bp: i64,
        labour_rate_ht_cents: Option<i64>,
        window: &mut Window,
        cx: &mut Context<AtelierPage>,
    ) -> Self {
        let client = ComboModel::new("Sans client — nom, téléphone, courriel…", window, cx);
        let (on_query, on_pick) = (cx.entity().downgrade(), cx.entity().downgrade());
        client.on_query(cx, move |query, _, cx| {
            let query = query.to_string();
            let _ = on_query.update(cx, |page, cx| page.search_form_clients(query, cx));
        });
        client.on_pick(cx, move |_, window, cx| {
            let _ = on_pick.update(cx, |page, cx| page.client_changed(window, cx));
        });
        if let Some(id) = order.client_id.clone() {
            let name = order.client_name.clone().filter(|name| !name.trim().is_empty()).unwrap_or_else(|| "Client sélectionné".into());
            client.set_picked(ComboOption::new(id, name), window, cx);
        }

        let technician = ComboModel::new("Non assigné — nom…", window, cx);
        let (on_query, on_pick) = (cx.entity().downgrade(), cx.entity().downgrade());
        technician.on_query(cx, move |query, _, cx| {
            let query = query.to_string();
            let _ = on_query.update(cx, |page, cx| page.search_technicians(query, cx));
        });
        technician.on_pick(cx, move |_, _, cx| {
            let _ = on_pick.update(cx, |page, cx| page.form_changed(cx));
        });

        // Until the customer's vehicles are read, the list holds the order's own vehicle: the
        // form then starts as the order is, not as "no vehicle".
        let own_vehicle: Vec<(String, String)> = order
            .vehicle_id
            .clone()
            .map(|id| (id, order.vehicle_label.clone().unwrap_or_else(|| "Véhicule lié".into())))
            .into_iter()
            .collect();

        let mut fuel_options = vec![SelectOption::new(NONE, "Non renseigné")];
        fuel_options.extend(FUEL_LEVELS.iter().map(|(value, label)| SelectOption::new(*value, *label)));
        // A level written another way (imported history) stays choosable, or saving would lose it.
        if let Some(other) = order.fuel_level.as_deref().filter(|level| !FUEL_LEVELS.iter().any(|(value, _)| value == level)) {
            fuel_options.push(SelectOption::new(other.to_string(), other.to_string()));
        }

        let stored = order
            .lines
            .iter()
            .map(|line| Line {
                id: Some(line.id.clone()),
                product_id: line.product_id.clone(),
                label: line.label.clone(),
                quantity_milli: line.quantity_milli,
                unit_price_ht_cents: line.unit_price_ht_cents,
                shelf_price_cents: line.unit_price_ttc_cents,
                vat_rate_bp: line.vat_rate_bp,
                discount_bp: line.discount_bp,
            })
            .collect();
        let mut lines = LineEditor::new(stored, products, default_vat_bp, window, cx);
        lines.set_labour_rate(labour_rate_ht_cents);

        let text = |value: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<AtelierPage>| {
            TextModel::builder().placeholder(placeholder).value(value).build(window, cx)
        };
        let mut form = Self {
            label: text(order.title(), "", window, cx),
            promised: text(
                order.promised_at.as_deref().map(|d| d.chars().take(10).collect()).unwrap_or_default(),
                "AAAA-MM-JJ",
                window,
                cx,
            ),
            client,
            client_searches: 0,
            vehicle: SelectModel::new(vehicle_options(&own_vehicle), Some(order.vehicle_id.as_deref().unwrap_or(NONE)), window, cx),
            has_vehicles: !own_vehicle.is_empty(),
            technician,
            mileage: text(order.mileage.map(|km| km.to_string()).unwrap_or_default(), "km", window, cx),
            fuel: SelectModel::new(fuel_options, Some(order.fuel_level.as_deref().unwrap_or(NONE)), window, cx),
            notes: TextModel::builder().value(order.notes.clone().unwrap_or_default()).multiline(2).build(window, cx),
            lines,
            started_as: Snapshot::default(),
            _subscriptions: Vec::new(),
        };
        let mut subscriptions: Vec<Subscription> = [&form.label, &form.promised, &form.mileage, &form.notes]
            .into_iter()
            .map(|field| {
                field.on_event(window, cx, |page, event, _, cx| {
                    if event == TextEvent::Changed {
                        page.form_changed(cx);
                    }
                })
            })
            .collect();
        subscriptions.push(form.fuel.on_change(window, cx, |page, _, _, cx| page.form_changed(cx)));
        subscriptions.push(form.vehicle.on_change(window, cx, |page, _, _, cx| page.form_changed(cx)));
        form._subscriptions = subscriptions;
        form.started_as = form.snapshot(cx);
        form
    }

    /// The technician in charge, as the field shows them. Does not count as an edit.
    pub fn show_technician(&mut self, option: ComboOption, window: &mut Window, cx: &mut App) {
        let untouched = !self.is_dirty(cx);
        self.technician.set_picked(option, window, cx);
        if untouched {
            self.started_as = self.snapshot(cx);
        }
    }

    pub fn chosen_vehicle(&self, cx: &App) -> Option<String> {
        self.vehicle.value(cx).filter(|id| id != NONE && !id.is_empty())
    }

    /// The customer's vehicles arrived (none when there is no customer). `keep` is the vehicle
    /// to stay on, when it is still one of them. `as_stored` says the list is only being
    /// completed, which is not an edit.
    pub fn set_vehicles(
        &mut self,
        vehicles: &[Vehicle],
        keep: Option<String>,
        as_stored: bool,
        window: &mut Window,
        cx: &mut Context<AtelierPage>,
    ) {
        let untouched = as_stored && !self.is_dirty(cx);
        let keep = keep.filter(|id| vehicles.iter().any(|v| &v.id == id));
        let listed: Vec<(String, String)> =
            vehicles.iter().map(|v| (v.id.clone(), format_vehicle_label(&v.make, &v.model, v.year))).collect();
        self.has_vehicles = !listed.is_empty();
        self.vehicle = SelectModel::new(vehicle_options(&listed), Some(keep.as_deref().unwrap_or(NONE)), window, cx);
        self._subscriptions.push(self.vehicle.on_change(window, cx, |page, _, _, cx| page.form_changed(cx)));
        if untouched {
            self.started_as = self.snapshot(cx);
        }
    }

    fn snapshot(&self, cx: &App) -> Snapshot {
        Snapshot {
            label: self.label.text(cx),
            promised: self.promised.text(cx),
            client: self.client.value(cx),
            vehicle: self.chosen_vehicle(cx),
            technician: self.technician.value(cx),
            mileage: self.mileage.text(cx),
            fuel: self.fuel.value(cx).filter(|level| level != NONE),
            notes: self.notes.text(cx),
            lines: self.lines.snapshot(cx),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.snapshot(cx) != self.started_as
    }

    /// What to send: the whole order, an empty field as nothing (which clears it).
    pub fn edit(&self, cx: &App) -> OrderEdit {
        let client_id = self.client.value(cx);
        OrderEdit {
            label: self.label.trimmed(cx).unwrap_or_else(|| DEFAULT_LABEL.to_string()),
            // A vehicle belongs to a customer: without one, there is no vehicle to link.
            vehicle_id: self.chosen_vehicle(cx).filter(|_| client_id.is_some()),
            client_id,
            assigned_to: self.technician.value(cx),
            mileage: self.mileage.trimmed(cx).map(|km| to_number(&km).round() as i64),
            fuel_level: self.fuel.value(cx).filter(|level| level != NONE && !level.is_empty()),
            promised_at: self.promised.trimmed(cx),
            notes: self.notes.trimmed(cx),
            lines: self
                .lines
                .filled(cx)
                .into_iter()
                .map(|line| WorkshopLineInput {
                    id: line.id,
                    product_id: line.product_id,
                    label: line.label,
                    quantity_milli: line.quantity_milli,
                    unit_price_ht_cents: line.unit_price_ht_cents,
                    unit_price_ttc_cents: line.shelf_price_cents,
                    vat_rate_bp: line.vat_rate_bp,
                    discount_bp: line.discount_bp,
                })
                .collect(),
        }
    }

    pub fn render(&self, error: Option<SharedString>, cx: &mut Context<AtelierPage>) -> Div {
        let m = cx.ui().m;
        let has_client = self.client.value(cx).is_some();
        let cell = |field: Option<FormField>| div().flex_1().min_w_0().children(field);
        let pair = |a: FormField, b: Option<FormField>| row().gap(m.space_3).items_start().child(cell(Some(a))).child(cell(b));

        col()
            .gap(m.space_4)
            .children(error.map(|e| Alert::danger().child(e)))
            .child(
                col()
                    .gap(m.space_3)
                    .child(pair(
                        FormField::new("Libellé").child(Input::new(&self.label)),
                        Some(FormField::new("Promis pour").child(Input::new(&self.promised))),
                    ))
                    .child(pair(
                        FormField::new("Client").child(Combobox::new(&self.client)),
                        Some(
                            FormField::new("Véhicule")
                                .child(Select::new(&self.vehicle).disabled(!has_client || !self.has_vehicles)),
                        ),
                    ))
                    .child(pair(
                        FormField::new("Technicien").child(Combobox::new(&self.technician)),
                        Some(FormField::new("Kilométrage").child(Input::new(&self.mileage))),
                    ))
                    .child(pair(FormField::new("Niveau de carburant").child(Select::new(&self.fuel)), None)),
            )
            .child(self.lines.render("Pièces et main-d'œuvre", None, cx))
            .child(self.lines.totals_block(false, cx))
            .child(FormField::new("Notes").child(Textarea::new(&self.notes)))
    }
}
