//! What the screens that show vehicles share (port of `VehicleForm.svelte` and of the body of
//! `VehiclePanel.svelte`): the form, the sections of the detail panel, the icon and the summary
//! line of a vehicle. Used by Clients & Véhicules and by Parc VN/VO.

use motodesk_client::clients::{OdometerUnit, Vehicle, VehicleInput, VehicleType};
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::format::{self, cents_to_text, euros_to_cents};

pub fn vehicle_icon(kind: VehicleType) -> IconName {
    match kind {
        VehicleType::Bicycle => IconName::Bike,
        VehicleType::Electric => IconName::Zap,
        VehicleType::Moto => IconName::Wrench,
    }
}

/// "Moto · AB-123-CD · 12 500 km", and the price of a vehicle for sale.
pub fn vehicle_meta(vehicle: &Vehicle) -> String {
    let mut bits = vec![vehicle.vehicle_type.label().to_string()];
    if let Some(plate) = vehicle.plate_number.as_ref().filter(|p| !p.is_empty()) {
        bits.push(plate.clone());
    }
    bits.push(format!("{} {}", format::integer(vehicle.odometer_value), vehicle.odometer_unit.label()));
    if let (true, Some(price)) = (vehicle.is_stock, vehicle.selling_price_ttc_cents) {
        bits.push(format::euros(price));
    }
    bits.join(" · ")
}

/// The body of a vehicle's detail panel: identification, prices of a stock vehicle, notes.
pub fn sections(panel: Panel, vehicle: &Vehicle) -> Panel {
    let identity = InfoList::new(vec![
        InfoItem::new("Type", Some(vehicle.vehicle_type.label())),
        InfoItem::new("Plaque", vehicle.plate_number.clone()).mono(),
        InfoItem::new("Année", vehicle.year.map(|y| y.to_string())),
        InfoItem::new("Cylindrée", vehicle.cylinder_capacity.filter(|c| *c > 0).map(|c| format!("{c} cm³"))),
        InfoItem::new("VIN / série", vehicle.vin_or_serial.clone()).mono(),
        InfoItem::new(
            "Compteur",
            Some(format!("{} {}", format::integer(vehicle.odometer_value), vehicle.odometer_unit.label())),
        ),
        InfoItem::new("Ajouté le", Some(format::date(&vehicle.created_at))),
    ]);
    let mut panel = panel.child(PanelSection::new("Identification").icon(IconName::IdCard).child(identity));
    if vehicle.is_stock {
        panel = panel.child(PanelSection::new("Commercial").icon(IconName::Banknote).child(InfoList::new(vec![
            InfoItem::new("Prix d'achat HT", vehicle.purchase_price_ht_cents.map(format::euros)),
            InfoItem::new("Prix de vente TTC", vehicle.selling_price_ttc_cents.map(format::euros)),
        ])));
    }
    if let Some(notes) = vehicle.notes.clone().filter(|n| !n.trim().is_empty()) {
        panel = panel.child(
            PanelSection::new("Notes")
                .icon(IconName::StickyNote)
                .child(InfoList::new(vec![InfoItem::new("Notes", Some(notes)).block()])),
        );
    }
    if vehicle.client_id.is_none() {
        panel = panel.child(
            row().child(Tag::new("Sans propriétaire").status(Status::Warning).icon(IconName::CircleAlert).medium()),
        );
    }
    panel
}

// ---- the form -------------------------------------------------------------------------------

/// A screen that shows a [`VehicleForm`] in one of its panels.
pub trait VehicleFormHost: Sized + 'static {
    /// The form on screen, if there is one.
    fn vehicle_form_mut(&mut self) -> Option<&mut VehicleForm>;
    /// A field changed: update the panel's "unsaved edits" mark, redraw.
    fn vehicle_form_changed(&mut self, cx: &mut Context<Self>);
    /// Ctrl+Enter in a field: send the form.
    fn submit_vehicle(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

pub struct VehicleForm {
    vehicle_type: SelectModel,
    plate: TextModel,
    make: TextModel,
    model: TextModel,
    year: TextModel,
    cylinder: TextModel,
    vin: TextModel,
    odometer: TextModel,
    unit: SelectModel,
    purchase: TextModel,
    selling: TextModel,
    notes: TextModel,
    pub is_stock: bool,
    pub make_missing: bool,
    pub model_missing: bool,
    started_as: VehicleInput,
    _subscriptions: Vec<Subscription>,
}

fn type_value(kind: VehicleType) -> &'static str {
    match kind {
        VehicleType::Moto => "moto",
        VehicleType::Bicycle => "bicycle",
        VehicleType::Electric => "electric",
    }
}

fn unit_value(unit: OdometerUnit) -> &'static str {
    match unit {
        OdometerUnit::Km => "km",
        OdometerUnit::Miles => "miles",
        OdometerUnit::Hours => "hours",
    }
}

fn to_int(text: &str) -> Option<i32> {
    text.trim().parse().ok()
}

fn field(placeholder: &'static str, value: Option<&str>, window: &mut Window, cx: &mut App) -> TextModel {
    TextModel::builder().placeholder(placeholder).value(value.unwrap_or_default().to_string()).build(window, cx)
}

impl VehicleForm {
    /// `initial` is the vehicle being edited; `None` for a new one.
    pub fn new<H: VehicleFormHost>(initial: Option<&Vehicle>, window: &mut Window, cx: &mut Context<H>) -> Self {
        let v = initial;
        let number = |n: Option<i64>| n.map(|n| n.to_string()).unwrap_or_default();
        let mut form = Self {
            vehicle_type: SelectModel::new(
                VehicleType::ALL.iter().map(|t| SelectOption::new(type_value(*t), t.label())).collect(),
                Some(type_value(v.map(|v| v.vehicle_type).unwrap_or_default())),
                window,
                cx,
            ),
            plate: field("AA-123-BB", v.and_then(|v| v.plate_number.as_deref()), window, cx),
            make: field("Honda", v.map(|v| v.make.as_str()), window, cx),
            model: field("CBR 600", v.map(|v| v.model.as_str()), window, cx),
            year: field("2020", Some(&number(v.and_then(|v| v.year.map(i64::from)))), window, cx),
            cylinder: field("600", Some(&number(v.and_then(|v| v.cylinder_capacity.map(i64::from)))), window, cx),
            vin: field("JH2PC4001…", v.and_then(|v| v.vin_or_serial.as_deref()), window, cx),
            odometer: field("0", Some(&number(v.map(|v| v.odometer_value).filter(|n| *n != 0))), window, cx),
            unit: SelectModel::new(
                OdometerUnit::ALL.iter().map(|u| SelectOption::new(unit_value(*u), u.label())).collect(),
                Some(unit_value(v.map(|v| v.odometer_unit).unwrap_or_default())),
                window,
                cx,
            ),
            purchase: field("0,00", Some(&cents_to_text(v.and_then(|v| v.purchase_price_ht_cents))), window, cx),
            selling: field("0,00", Some(&cents_to_text(v.and_then(|v| v.selling_price_ttc_cents))), window, cx),
            notes: TextModel::builder()
                .placeholder("État, historique, particularités…")
                .value(v.and_then(|v| v.notes.clone()).unwrap_or_default())
                .multiline(2)
                .build(window, cx),
            is_stock: v.is_some_and(|v| v.is_stock),
            make_missing: false,
            model_missing: false,
            started_as: VehicleInput::default(),
            _subscriptions: Vec::new(),
        };
        form.started_as = form.input(cx);
        let mut subscriptions: Vec<Subscription> = [
            &form.plate,
            &form.make,
            &form.model,
            &form.year,
            &form.cylinder,
            &form.vin,
            &form.odometer,
            &form.purchase,
            &form.selling,
            &form.notes,
        ]
        .into_iter()
        .map(|model| {
            model.on_event(window, cx, |host: &mut H, event, window, cx| match event {
                TextEvent::Changed => host.vehicle_form_changed(cx),
                TextEvent::Submitted { with_modifier: true } => host.submit_vehicle(window, cx),
                _ => {}
            })
        })
        .collect();
        for select in [&form.vehicle_type, &form.unit] {
            subscriptions.push(select.on_change(window, cx, |host: &mut H, _, _, cx| host.vehicle_form_changed(cx)));
        }
        form._subscriptions = subscriptions;
        form
    }

    /// For a vehicle created from Parc VN/VO: the "stock" box starts ticked.
    pub fn starting_in_stock(mut self, cx: &App) -> Self {
        self.is_stock = true;
        self.started_as = self.input(cx);
        self
    }

    pub fn input(&self, cx: &App) -> VehicleInput {
        let vehicle_type = match self.vehicle_type.value(cx).as_deref() {
            Some("bicycle") => VehicleType::Bicycle,
            Some("electric") => VehicleType::Electric,
            _ => VehicleType::Moto,
        };
        let unit = match self.unit.value(cx).as_deref() {
            Some("miles") => OdometerUnit::Miles,
            Some("hours") => OdometerUnit::Hours,
            _ => OdometerUnit::Km,
        };
        VehicleInput {
            vehicle_type,
            make: self.make.text(cx).trim().to_string(),
            model: self.model.text(cx).trim().to_string(),
            year: to_int(&self.year.text(cx)),
            cylinder_capacity: to_int(&self.cylinder.text(cx)),
            vin_or_serial: self.vin.trimmed(cx),
            plate_number: self.plate.trimmed(cx),
            odometer_value: Some(to_int(&self.odometer.text(cx)).unwrap_or(0).into()),
            odometer_unit: Some(unit),
            is_stock: Some(self.is_stock),
            purchase_price_ht_cents: euros_to_cents(&self.purchase.text(cx)),
            selling_price_ttc_cents: euros_to_cents(&self.selling.text(cx)),
            notes: self.notes.trimmed(cx),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.input(cx) != self.started_as
    }

    /// Mark the fields a vehicle cannot do without. Returns whether the form can be sent.
    pub fn validate(&mut self, cx: &App) -> bool {
        let input = self.input(cx);
        self.make_missing = input.make.is_empty();
        self.model_missing = input.model.is_empty();
        !(self.make_missing || self.model_missing)
    }

    /// Clear a "missing" mark as soon as its field is filled.
    pub fn recheck(&mut self, cx: &App) {
        let input = self.input(cx);
        self.make_missing &= input.make.is_empty();
        self.model_missing &= input.model.is_empty();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.make.focus(window, cx);
    }

    pub fn render<H: VehicleFormHost>(&self, error: Option<SharedString>, cx: &mut Context<H>) -> Div {
        let m = cx.ui().m;
        let host = cx.entity();
        let cells = |fields: Vec<AnyElement>| {
            row().gap(m.space_3).items_start().children(fields.into_iter().map(|f| div().flex_1().min_w_0().child(f)))
        };
        col()
            .gap(m.space_4)
            .children(error.map(|e| Alert::danger().child(e)))
            .child(cells(vec![
                FormField::new("Type").required().child(Select::new(&self.vehicle_type)).into_any_element(),
                FormField::new("Plaque d'immatriculation").child(Input::new(&self.plate).mono()).into_any_element(),
            ]))
            .child(cells(vec![
                FormField::new("Marque")
                    .required()
                    .error(self.make_missing.then_some("Obligatoire."))
                    .child(Input::new(&self.make).invalid(self.make_missing))
                    .into_any_element(),
                FormField::new("Modèle")
                    .required()
                    .error(self.model_missing.then_some("Obligatoire."))
                    .child(Input::new(&self.model).invalid(self.model_missing))
                    .into_any_element(),
            ]))
            .child(cells(vec![
                FormField::new("Année").child(Input::new(&self.year)).into_any_element(),
                FormField::new("Cylindrée (cm³)").child(Input::new(&self.cylinder)).into_any_element(),
                FormField::new("VIN / N° de série").child(Input::new(&self.vin).mono()).into_any_element(),
            ]))
            .child(cells(vec![
                FormField::new("Kilométrage / heures").child(Input::new(&self.odometer)).into_any_element(),
                FormField::new("Unité").child(Select::new(&self.unit)).into_any_element(),
            ]))
            .child(Checkbox::new("is-stock", self.is_stock).label("Stock atelier (à vendre)").on_change(move |on, _, cx| {
                host.update(cx, |host, cx| {
                    if let Some(form) = host.vehicle_form_mut() {
                        form.is_stock = on;
                    }
                    host.vehicle_form_changed(cx);
                })
            }))
            .child(cells(vec![
            FormField::new("Prix d'achat HT (€)")
                .hint("Visible uniquement en interne.")
                .child(Input::new(&self.purchase))
                .into_any_element(),
            FormField::new("Prix de vente TTC (€)").child(Input::new(&self.selling)).into_any_element(),
        ]))
        .child(FormField::new("Notes").child(Textarea::new(&self.notes)))
    }
}
