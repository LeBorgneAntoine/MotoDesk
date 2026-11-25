//! The two forms of the Clients page (port of `ClientForm.svelte` and `VehicleForm.svelte`).
//!
//! A form owns its fields, knows whether they differ from what it started with, and turns
//! them into what the API takes. It is created when its panel opens and dropped when it closes.

use motodesk_client::clients::{ClientDetail, ClientInput, OdometerUnit, Vehicle, VehicleInput, VehicleType};
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::ClientsPage;

/// "1 234,50" or "1234.5" → cents. Empty or unreadable → `None`.
pub fn euros_to_cents(text: &str) -> Option<i64> {
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == ',' { '.' } else { c }).collect();
    if cleaned.is_empty() {
        return None;
    }
    let value: f64 = cleaned.parse().ok()?;
    value.is_finite().then(|| (value * 100.).round() as i64)
}

fn cents_to_text(cents: Option<i64>) -> String {
    cents.map(|c| format!("{:.2}", c as f64 / 100.)).unwrap_or_default()
}

fn to_int(text: &str) -> Option<i32> {
    text.trim().parse().ok()
}

fn field(placeholder: &'static str, value: Option<&str>, window: &mut Window, cx: &mut Context<ClientsPage>) -> TextModel {
    TextModel::builder().placeholder(placeholder).value(value.unwrap_or_default().to_string()).build(window, cx)
}

// ---- client ---------------------------------------------------------------------------------

pub struct ClientForm {
    first_name: TextModel,
    last_name: TextModel,
    phone: TextModel,
    email: TextModel,
    address: TextModel,
    notes: TextModel,
    credit_limit: TextModel,
    pub credit_blocked: bool,
    pub last_name_missing: bool,
    started_as: ClientInput,
    _subscriptions: Vec<Subscription>,
}

impl ClientForm {
    /// `initial` is the customer being edited; `None` for a new one.
    pub fn new(initial: Option<&ClientDetail>, window: &mut Window, cx: &mut Context<ClientsPage>) -> Self {
        let c = initial.map(|d| &d.client);
        let mut form = Self {
            first_name: field("Jean", c.and_then(|c| c.first_name.as_deref()), window, cx),
            last_name: field("Dupont", c.map(|c| c.last_name.as_str()), window, cx),
            phone: field("06 12 34 56 78", c.and_then(|c| c.phone.as_deref()), window, cx),
            email: field("jean.dupont@example.com", c.and_then(|c| c.email.as_deref()), window, cx),
            address: TextModel::builder()
                .placeholder("Numéro, rue, ville, code postal")
                .value(c.and_then(|c| c.address.clone()).unwrap_or_default())
                .multiline(2)
                .build(window, cx),
            notes: TextModel::builder()
                .placeholder("Préférences, remarques internes…")
                .value(c.and_then(|c| c.notes.clone()).unwrap_or_default())
                .multiline(3)
                .build(window, cx),
            credit_limit: field("Illimité", Some(&cents_to_text(c.and_then(|c| c.credit_limit_cents))), window, cx),
            credit_blocked: c.is_some_and(|c| c.credit_blocked),
            last_name_missing: false,
            started_as: ClientInput::default(),
            _subscriptions: Vec::new(),
        };
        form.started_as = form.input(cx);
        form._subscriptions = [&form.first_name, &form.last_name, &form.phone, &form.email, &form.address, &form.notes, &form.credit_limit]
            .into_iter()
            .map(|model| {
                model.on_event(window, cx, |page, event, window, cx| match event {
                    TextEvent::Changed => page.client_form_changed(cx),
                    TextEvent::Submitted { with_modifier: true } => page.submit_client(window, cx),
                    _ => {}
                })
            })
            .collect();
        form
    }

    pub fn input(&self, cx: &App) -> ClientInput {
        ClientInput {
            first_name: self.first_name.trimmed(cx),
            last_name: self.last_name.text(cx).trim().to_string(),
            phone: self.phone.trimmed(cx),
            email: self.email.trimmed(cx),
            address: self.address.trimmed(cx),
            notes: self.notes.trimmed(cx),
            credit_limit_cents: euros_to_cents(&self.credit_limit.text(cx)),
            credit_blocked: Some(self.credit_blocked),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.input(cx) != self.started_as
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.first_name.focus(window, cx);
    }

    pub fn render(&self, error: Option<SharedString>, cx: &mut Context<ClientsPage>) -> Div {
        let m = cx.ui().m;
        let page = cx.entity();
        let pair = |a: FormField, b: FormField| row().gap(m.space_3).items_start().child(div().flex_1().child(a)).child(div().flex_1().child(b));
        col()
            .gap(m.space_4)
            .children(error.map(|e| Alert::danger().child(e)))
            .child(pair(
                FormField::new("Prénom").child(Input::new(&self.first_name)),
                FormField::new("Nom")
                    .required()
                    .error(self.last_name_missing.then_some("Le nom est obligatoire."))
                    .child(Input::new(&self.last_name).invalid(self.last_name_missing)),
            ))
            .child(pair(
                FormField::new("Téléphone").child(Input::new(&self.phone)),
                FormField::new("Courriel").child(Input::new(&self.email)),
            ))
            .child(FormField::new("Adresse").child(Textarea::new(&self.address)))
            .child(FormField::new("Notes").hint("Visible uniquement par l'équipe.").child(Textarea::new(&self.notes)))
            .child(pair(
                FormField::new("Plafond d'encours TTC (€)").hint("Vide = illimité.").child(Input::new(&self.credit_limit)),
                FormField::new("Compte").hint("Refuse toute nouvelle facture ou vente à ce client.").child(
                    Checkbox::new("credit-blocked", self.credit_blocked).label("Bloquer l'encours").on_change(
                        move |on, _, cx| {
                            page.update(cx, |page, cx| {
                                if let Some(form) = page.client_form.as_mut() {
                                    form.credit_blocked = on;
                                }
                                page.client_form_changed(cx);
                            })
                        },
                    ),
                ),
            ))
    }
}

// ---- vehicle --------------------------------------------------------------------------------

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

impl VehicleForm {
    pub fn new(initial: Option<&Vehicle>, window: &mut Window, cx: &mut Context<ClientsPage>) -> Self {
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
            &form.plate, &form.make, &form.model, &form.year, &form.cylinder, &form.vin, &form.odometer, &form.purchase,
            &form.selling, &form.notes,
        ]
        .into_iter()
        .map(|model| {
            model.on_event(window, cx, |page, event, window, cx| match event {
                TextEvent::Changed => page.vehicle_form_changed(cx),
                TextEvent::Submitted { with_modifier: true } => page.submit_vehicle(window, cx),
                _ => {}
            })
        })
        .collect();
        for select in [&form.vehicle_type, &form.unit] {
            subscriptions.push(select.on_change(window, cx, |page, _, _, cx| page.vehicle_form_changed(cx)));
        }
        form._subscriptions = subscriptions;
        form
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

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.make.focus(window, cx);
    }

    pub fn render(&self, error: Option<SharedString>, cx: &mut Context<ClientsPage>) -> Div {
        let m = cx.ui().m;
        let page = cx.entity();
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
                page.update(cx, |page, cx| {
                    if let Some(form) = page.vehicle_form.as_mut() {
                        form.is_stock = on;
                    }
                    page.vehicle_form_changed(cx);
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

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use super::{cents_to_text, euros_to_cents};

    #[test]
    fn amounts_are_read_the_way_they_are_typed_in_france() {
        assert_eq!(euros_to_cents("1 234,50"), Some(123_450));
        assert_eq!(euros_to_cents("1234.5"), Some(123_450));
        assert_eq!(euros_to_cents("0,019"), Some(2));
        assert_eq!(euros_to_cents(""), None);
        assert_eq!(euros_to_cents("  "), None);
        assert_eq!(euros_to_cents("douze"), None);
        assert_eq!(cents_to_text(Some(123_450)), "1234.50");
        assert_eq!(cents_to_text(None), "");
    }
}
