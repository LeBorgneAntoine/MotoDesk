//! The forms of the catalogue.
//!
//! A form owns its fields, knows whether they differ from what it started with, and turns
//! them into what the API takes. It is created when its panel or dialog opens and dropped when
//! it closes.

use motodesk_client::products::{
    Category, CategoryInput, Product, ProductInput, Unit, VAT_PRESETS, VEHICLE_TYPES, VariantAttr, VariantInput,
    sanitize_variant_attrs, variant_product_name,
};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::StockPage;
use crate::format::{cents_to_text, euros_to_cents};

/// The value of "nothing chosen" in a list.
const NONE: &str = "-";

fn field(placeholder: &'static str, value: impl Into<String>, window: &mut Window, cx: &mut Context<StockPage>) -> TextModel {
    TextModel::builder().placeholder(placeholder).value(value.into()).build(window, cx)
}

/// Every keystroke rechecks the form; Enter sends it.
fn watch(field: &TextModel, window: &mut Window, cx: &mut Context<StockPage>) -> Subscription {
    field.on_event(window, cx, |page, event, window, cx| match event {
        TextEvent::Changed => page.form_changed(cx),
        TextEvent::Submitted { .. } => page.submit_form(window, cx),
        _ => {}
    })
}

// ---- what was typed, as numbers ----------------------------------------------------------------

/// A stock figure: decimals allowed ("2,5"), kept to the thousandth.
pub fn to_quantity(text: &str) -> Option<f64> {
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == ',' { '.' } else { c }).collect();
    if cleaned.is_empty() {
        return None;
    }
    let value: f64 = cleaned.parse().ok()?;
    value.is_finite().then(|| (value * 1000.).round() / 1000.)
}

/// A whole number; what comes after a decimal point is dropped.
pub fn to_int(text: &str) -> Option<i64> {
    let value: f64 = text.trim().parse().ok()?;
    value.is_finite().then(|| value.trunc() as i64)
}

/// A price in euros → cents. A negative price is no price.
pub fn price_cents(text: &str) -> Option<i64> {
    euros_to_cents(text).filter(|cents| *cents >= 0)
}

// ---- category ----------------------------------------------------------------------------------

pub struct CategoryForm {
    name: TextModel,
    position: TextModel,
    pub name_missing: bool,
    started_as: (String, String),
    _subscriptions: Vec<Subscription>,
}

impl CategoryForm {
    /// `initial` is the category being renamed; `None` for a new one.
    pub fn new(initial: Option<&Category>, window: &mut Window, cx: &mut Context<StockPage>) -> Self {
        let name = field("ex. Pièces vélos", initial.map(|c| c.name.clone()).unwrap_or_default(), window, cx);
        let position = field("50", initial.map(|c| c.position.to_string()).unwrap_or_default(), window, cx);
        let subscriptions = vec![watch(&name, window, cx), watch(&position, window, cx)];
        let started_as = (name.text(cx), position.text(cx));
        Self { name, position, name_missing: false, started_as, _subscriptions: subscriptions }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.name.focus(window, cx);
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        (self.name.text(cx), self.position.text(cx)) != self.started_as
    }

    /// The complaint goes as soon as there is a name.
    pub fn recheck(&mut self, cx: &App) {
        if self.name_missing && self.name.trimmed(cx).is_some() {
            self.name_missing = false;
        }
    }

    /// What to send, or `None` (and the field marked) when the name is missing.
    pub fn validated(&mut self, cx: &App) -> Option<CategoryInput> {
        let name = self.name.trimmed(cx);
        self.name_missing = name.is_none();
        Some(CategoryInput { name: name?, position: to_int(&self.position.text(cx)) })
    }

    pub fn render(&self, error: Option<SharedString>, cx: &App) -> Div {
        let m = cx.ui().m;
        col()
            .gap(m.space_3)
            .children(error.map(|e| Alert::danger().child(e)))
            .child(
                FormField::new("Nom")
                    .required()
                    .error(self.name_missing.then_some("Obligatoire."))
                    .child(Input::new(&self.name).invalid(self.name_missing)),
            )
            .child(
                FormField::new("Ordre d'affichage").hint("Plus petit = en haut de la liste.").child(Input::new(&self.position)),
            )
    }
}

// ---- product -----------------------------------------------------------------------------------

/// The form as text: what "has anything changed?" compares.
#[derive(Debug, Clone, Default, PartialEq)]
struct Snapshot {
    texts: Vec<String>,
    choices: Vec<String>,
    is_active: bool,
}

pub struct ProductForm {
    /// The product being edited; `None` for a new one.
    initial: Option<Product>,
    name: TextModel,
    category: SelectModel,
    brand: TextModel,
    sku: TextModel,
    unit: SelectModel,
    description: TextModel,
    selling: TextModel,
    purchase: TextModel,
    vat: SelectModel,
    stock: TextModel,
    threshold: TextModel,
    vehicle_type: SelectModel,
    year: TextModel,
    cylinder: TextModel,
    plate: TextModel,
    vin: TextModel,
    odometer: TextModel,
    odometer_unit: SelectModel,
    notes: TextModel,
    pub is_active: bool,
    /// The vehicle block starts folded unless the record already uses it.
    pub vehicle_collapsed: bool,
    pub name_missing: bool,
    /// The stock figure the form opened with: sent back only if it was changed.
    stock_at_start: String,
    started_as: Snapshot,
    _subscriptions: Vec<Subscription>,
}

impl ProductForm {
    /// `default_vat_bp` and `default_threshold` are the shop's usual values, for a new product.
    pub fn new(
        initial: Option<&Product>,
        categories: &[Category],
        default_vat_bp: i64,
        default_threshold: Option<i64>,
        window: &mut Window,
        cx: &mut Context<StockPage>,
    ) -> Self {
        let p = initial;
        let text = |value: Option<&String>| value.cloned().unwrap_or_default();
        let number = |value: Option<i64>| value.map(|n| n.to_string()).unwrap_or_default();

        let mut category_options = vec![SelectOption::new(NONE, "— Aucune catégorie —")];
        category_options.extend(categories.iter().map(|c| SelectOption::new(c.id.clone(), c.name.clone())));
        let unit_options = Unit::ALL.into_iter().map(|unit| SelectOption::new(unit.value(), unit.label())).collect();
        let vat_bp = p.map(|p| p.vat_rate_bp).unwrap_or(default_vat_bp);
        let mut vat_options: Vec<SelectOption> =
            VAT_PRESETS.iter().map(|(label, bp)| SelectOption::new(bp.to_string(), *label)).collect();
        // A rate that is none of the usual ones stays choosable, or saving would replace it.
        if !VAT_PRESETS.iter().any(|(_, bp)| *bp == vat_bp) {
            let label = format!("{} %", format!("{:.2}", vat_bp as f64 / 100.).replace('.', ","));
            vat_options.push(SelectOption::new(vat_bp.to_string(), label));
        }
        let mut vehicle_options = vec![SelectOption::new(NONE, "— Non précisé —")];
        vehicle_options.extend(VEHICLE_TYPES.iter().map(|(value, label)| SelectOption::new(*value, *label)));
        let odometer_options = vec![
            SelectOption::new(NONE, "— Non précisé —"),
            SelectOption::new("km", "km"),
            SelectOption::new("miles", "mi"),
            SelectOption::new("hours", "h"),
        ];

        // A new product starts with the shop's usual alert threshold (clear it for none).
        let threshold = match p {
            Some(p) => number(p.low_stock_threshold),
            None => number(default_threshold),
        };
        let stock = p.map(|p| p.stock_quantity.to_string()).unwrap_or_else(|| "0".to_string());

        let mut form = Self {
            name: field("ex. Plaquette de frein avant", p.map(|p| p.name.clone()).unwrap_or_default(), window, cx),
            category: SelectModel::new(
                category_options,
                Some(p.and_then(|p| p.category_id.as_deref()).unwrap_or(NONE)),
                window,
                cx,
            ),
            brand: field("ex. Brembo", text(p.and_then(|p| p.brand.as_ref())), window, cx),
            sku: field("ex. BRMB-PLQ-001", text(p.and_then(|p| p.sku.as_ref())), window, cx),
            unit: SelectModel::new(unit_options, Some(p.map(|p| p.unit).unwrap_or_default().value()), window, cx),
            description: TextModel::builder()
                .placeholder("Compatibilité, dimensions, caractéristiques…")
                .value(text(p.and_then(|p| p.description.as_ref())))
                .multiline(2)
                .build(window, cx),
            selling: field("0,00", cents_to_text(p.and_then(|p| p.selling_price_ttc_cents)), window, cx),
            purchase: field("0,00", cents_to_text(p.and_then(|p| p.purchase_price_ht_cents)), window, cx),
            vat: SelectModel::new(vat_options, Some(vat_bp.to_string().as_str()), window, cx),
            stock: field("0", stock.clone(), window, cx),
            threshold: field("ex. 5", threshold, window, cx),
            vehicle_type: SelectModel::new(
                vehicle_options,
                Some(p.and_then(|p| p.vehicle_type.as_deref()).unwrap_or(NONE)),
                window,
                cx,
            ),
            year: field("2020", number(p.and_then(|p| p.year)), window, cx),
            cylinder: field("600", number(p.and_then(|p| p.cylinder_capacity)), window, cx),
            plate: field("AA-123-BB", text(p.and_then(|p| p.plate_number.as_ref())), window, cx),
            vin: field("JH2PC4001…", text(p.and_then(|p| p.vin_or_serial.as_ref())), window, cx),
            odometer: field("0", number(p.and_then(|p| p.odometer_value)), window, cx),
            odometer_unit: SelectModel::new(
                odometer_options,
                Some(p.and_then(|p| p.odometer_unit.as_deref()).unwrap_or(NONE)),
                window,
                cx,
            ),
            notes: TextModel::builder()
                .placeholder("Remarques, état, fournisseur…")
                .value(text(p.and_then(|p| p.notes.as_ref())))
                .multiline(2)
                .build(window, cx),
            is_active: p.is_none_or(|p| p.is_active),
            vehicle_collapsed: !p.is_some_and(Product::has_vehicle_data),
            name_missing: false,
            stock_at_start: stock,
            started_as: Snapshot::default(),
            initial: initial.cloned(),
            _subscriptions: Vec::new(),
        };
        form.started_as = form.snapshot(cx);
        let mut subscriptions: Vec<Subscription> = form.texts().into_iter().map(|field| watch(field, window, cx)).collect();
        for choice in form.choices() {
            subscriptions.push(choice.on_change(window, cx, |page, _, _, cx| page.form_changed(cx)));
        }
        form._subscriptions = subscriptions;
        form
    }

    fn texts(&self) -> [&TextModel; 14] {
        [
            &self.name,
            &self.brand,
            &self.sku,
            &self.description,
            &self.selling,
            &self.purchase,
            &self.stock,
            &self.threshold,
            &self.year,
            &self.cylinder,
            &self.plate,
            &self.vin,
            &self.odometer,
            &self.notes,
        ]
    }

    fn choices(&self) -> [&SelectModel; 5] {
        [&self.category, &self.unit, &self.vat, &self.vehicle_type, &self.odometer_unit]
    }

    fn snapshot(&self, cx: &App) -> Snapshot {
        Snapshot {
            texts: self.texts().into_iter().map(|field| field.text(cx)).collect(),
            choices: self.choices().into_iter().map(|choice| choice.value(cx).unwrap_or_default()).collect(),
            is_active: self.is_active,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.name.focus(window, cx);
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.snapshot(cx) != self.started_as
    }

    /// The complaint goes as soon as there is a name.
    pub fn recheck(&mut self, cx: &App) {
        if self.name_missing && self.name.trimmed(cx).is_some() {
            self.name_missing = false;
        }
    }

    fn unit(&self, cx: &App) -> Unit {
        Unit::from_value(&self.unit.value(cx).unwrap_or_default())
    }

    /// A service has no stock. Choosing the unit "service" makes the product one.
    fn is_service(&self, cx: &App) -> bool {
        self.unit(cx) == Unit::Service || self.initial.as_ref().is_some_and(|p| p.is_service)
    }

    /// What to send, or `None` (and the field marked) when the name is missing.
    pub fn validated(&mut self, cx: &App) -> Option<ProductInput> {
        let name = self.name.trimmed(cx);
        self.name_missing = name.is_none();
        let name = name?;

        let chosen = |choice: &SelectModel| choice.value(cx).filter(|value| value != NONE && !value.is_empty());
        let vehicle = !self.vehicle_collapsed;
        let stock = self.stock.text(cx);
        // Only a stock figure that was changed is sent: the one on screen may be older than
        // the shelf (sales and receptions since the form opened).
        let stock_quantity =
            (self.initial.is_none() || stock != self.stock_at_start).then(|| to_quantity(&stock).unwrap_or(0.));
        let initial = self.initial.as_ref();

        Some(ProductInput {
            name,
            category_id: chosen(&self.category),
            sku: self.sku.trimmed(cx),
            brand: self.brand.trimmed(cx),
            description: self.description.trimmed(cx),
            unit: self.unit(cx),
            is_service: self.is_service(cx),
            selling_price_ttc_cents: price_cents(&self.selling.text(cx)),
            purchase_price_ht_cents: price_cents(&self.purchase.text(cx)),
            vat_rate_bp: self.vat.value(cx).and_then(|bp| bp.parse().ok()).filter(|bp| *bp >= 0).unwrap_or(2000),
            stock_quantity,
            low_stock_threshold: to_int(&self.threshold.text(cx)),
            is_active: self.is_active,
            // A folded vehicle block says "this is not a vehicle": its fields are cleared.
            vehicle_type: chosen(&self.vehicle_type).filter(|_| vehicle),
            year: to_int(&self.year.text(cx)).filter(|_| vehicle),
            cylinder_capacity: to_int(&self.cylinder.text(cx)).filter(|_| vehicle),
            vin_or_serial: self.vin.trimmed(cx).filter(|_| vehicle),
            plate_number: self.plate.trimmed(cx).filter(|_| vehicle),
            odometer_value: to_int(&self.odometer.text(cx)).filter(|_| vehicle),
            odometer_unit: chosen(&self.odometer_unit).filter(|_| vehicle),
            notes: self.notes.trimmed(cx),
            parent_id: initial.and_then(|p| p.parent_id.clone()),
            variant_label: initial.and_then(|p| p.variant_label.clone()),
            variant_attrs: initial.and_then(|p| p.variant_attrs.clone()),
        })
    }

    /// The blocks of the form, one section each.
    pub fn render(&self, error: Option<SharedString>, cx: &mut Context<StockPage>) -> Vec<AnyElement> {
        let m = cx.ui().m;
        let editing = self.initial.is_some();
        let cell = |field: FormField| div().flex_1().min_w_0().child(field);
        let pair = |a: FormField, b: FormField| row().gap(m.space_3).items_start().child(cell(a)).child(cell(b));
        let triple =
            |a: FormField, b: FormField, c: FormField| row().gap(m.space_3).items_start().child(cell(a)).child(cell(b)).child(cell(c));
        let page = cx.entity();

        let mut blocks: Vec<AnyElement> = Vec::new();
        if let Some(error) = error {
            blocks.push(Alert::danger().child(error).into_any_element());
        }
        if self.name_missing {
            blocks.push(
                Alert::danger().child("Le nom du produit est obligatoire. Renseignez-le pour continuer.").into_any_element(),
            );
        }

        blocks.push(
            PanelSection::new("Identité")
                .icon(IconName::Package)
                .child(
                    col()
                        .gap(m.space_3)
                        .child(
                            FormField::new("Nom")
                                .required()
                                .error(self.name_missing.then_some("Obligatoire."))
                                .child(Input::new(&self.name).invalid(self.name_missing)),
                        )
                        .child(pair(
                            FormField::new("Catégorie").child(Select::new(&self.category)),
                            FormField::new("Marque").child(Input::new(&self.brand)),
                        ))
                        .child(pair(
                            FormField::new("Référence / SKU").hint("Identifiant interne unique.").child(Input::new(&self.sku).mono()),
                            FormField::new("Unité de vente").child(Select::new(&self.unit)),
                        ))
                        .child(FormField::new("Description").child(Textarea::new(&self.description))),
                )
                .into_any_element(),
        );

        blocks.push(
            PanelSection::new("Prix & TVA")
                .icon(IconName::Banknote)
                .child(triple(
                    FormField::new("Prix de vente TTC (€)").child(Input::new(&self.selling)),
                    FormField::new("Prix d'achat HT (€)").hint("Visible uniquement en interne.").child(Input::new(&self.purchase)),
                    FormField::new("TVA").child(Select::new(&self.vat)),
                ))
                .into_any_element(),
        );

        if !self.is_service(cx) {
            let quantity = FormField::new("Quantité disponible");
            blocks.push(
                PanelSection::new("Stock")
                    .icon(IconName::Boxes)
                    .child(pair(
                        if editing { quantity.hint("Modifier ce chiffre enregistre une correction de stock.") } else { quantity }
                            .child(Input::new(&self.stock)),
                        FormField::new("Seuil d'alerte").hint("Vide = pas d'alerte.").child(Input::new(&self.threshold)),
                    ))
                    .into_any_element(),
            );
        }

        blocks.push(
            PanelSection::new("Attributs véhicule")
                .subtitle("Facultatif — pour les produits véhicule")
                .collapsible(
                    self.vehicle_collapsed,
                    cx.listener(|page, _, _, cx| {
                        if let Some(form) = page.product_form.as_mut() {
                            form.vehicle_collapsed = !form.vehicle_collapsed;
                        }
                        cx.notify();
                    }),
                )
                .child(
                    col()
                        .gap(m.space_3)
                        .child(triple(
                            FormField::new("Type").child(Select::new(&self.vehicle_type)),
                            FormField::new("Année").child(Input::new(&self.year)),
                            FormField::new("Cylindrée (cm³)").child(Input::new(&self.cylinder)),
                        ))
                        .child(pair(
                            FormField::new("Immatriculation").child(Input::new(&self.plate).mono()),
                            FormField::new("VIN / N° de série").child(Input::new(&self.vin).mono()),
                        ))
                        .child(pair(
                            FormField::new("Compteur").child(Input::new(&self.odometer)),
                            FormField::new("Unité").child(Select::new(&self.odometer_unit)),
                        )),
                )
                .into_any_element(),
        );

        blocks.push(
            PanelSection::new("Divers")
                .icon(IconName::StickyNote)
                .child(
                    col().gap(m.space_3).child(FormField::new("Notes internes").child(Textarea::new(&self.notes))).child(
                        Checkbox::new("product-active", self.is_active)
                            .label("Produit actif")
                            .hint("Décochez pour le masquer du catalogue sans le supprimer.")
                            .on_change(move |on, _, cx| {
                                page.update(cx, |page, cx| {
                                    if let Some(form) = page.product_form.as_mut() {
                                        form.is_active = on;
                                    }
                                    page.form_changed(cx);
                                })
                            }),
                    ),
                )
                .into_any_element(),
        );
        blocks
    }
}

// ---- variant -----------------------------------------------------------------------------------

/// The "Nouvelle variante" dialog: a colour, a size or any other version of a model.
pub struct VariantForm {
    parent_name: String,
    name: TextModel,
    attrs: Vec<(TextModel, TextModel)>,
    sku: TextModel,
    stock: TextModel,
    price: TextModel,
    /// The name the form last proposed: while the field still holds it, it follows the attributes.
    suggested: String,
    /// The start of a reference the form proposed ("BRMB-PLQ-001-"): left as it is, it is not one.
    sku_prefix: String,
    pub attr_missing: bool,
    pub busy: bool,
    pub error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl VariantForm {
    pub fn new(
        parent_name: String,
        suggested_sku: String,
        suggested_price: String,
        window: &mut Window,
        cx: &mut Context<StockPage>,
    ) -> Self {
        let mut form = Self {
            name: field("BIKE BMW Bleu", parent_name.clone(), window, cx),
            attrs: Vec::new(),
            sku: field("BMW-BLEU", suggested_sku.clone(), window, cx),
            sku_prefix: suggested_sku,
            stock: field("0", "0", window, cx),
            price: field("0,00", suggested_price, window, cx),
            suggested: parent_name.clone(),
            parent_name,
            attr_missing: false,
            busy: false,
            error: None,
            _subscriptions: Vec::new(),
        };
        for field in [&form.name, &form.sku, &form.stock, &form.price] {
            let subscription = field.on_event(window, cx, |page, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    page.submit_variant(window, cx);
                }
            });
            form._subscriptions.push(subscription);
        }
        form.add_attr(window, cx);
        form
    }

    /// One more "caractéristique / valeur" line.
    pub fn add_attr(&mut self, window: &mut Window, cx: &mut Context<StockPage>) {
        let pair = (field("Couleur, taille…", "", window, cx), field("Bleu, 10…", "", window, cx));
        for field in [&pair.0, &pair.1] {
            let subscription = field.on_event(window, cx, |page, event, window, cx| match event {
                TextEvent::Changed => page.variant_attrs_changed(window, cx),
                TextEvent::Submitted { .. } => page.submit_variant(window, cx),
                _ => {}
            });
            self._subscriptions.push(subscription);
        }
        self.attrs.push(pair);
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        if let Some((name, _)) = self.attrs.first() {
            name.focus(window, cx);
        }
    }

    fn typed_attrs(&self, cx: &App) -> Vec<VariantAttr> {
        self.attrs.iter().map(|(name, value)| VariantAttr { name: name.text(cx), value: value.text(cx) }).collect()
    }

    /// The attributes changed: the name follows them, unless it was rewritten by hand.
    pub fn attrs_changed(&mut self, window: &mut Window, cx: &mut App) {
        self.attr_missing = false;
        let untouched = self.name.text(cx) == self.suggested;
        self.suggested = variant_product_name(&self.parent_name, &self.typed_attrs(cx), None);
        if untouched {
            self.name.set_text(self.suggested.clone(), window, cx);
        }
    }

    /// What to send, or `None` (and the first line marked) without a complete attribute.
    pub fn validated(&mut self, cx: &App) -> Option<VariantInput> {
        let attrs = sanitize_variant_attrs(&self.typed_attrs(cx));
        self.attr_missing = attrs.is_empty();
        if attrs.is_empty() {
            return None;
        }
        Some(VariantInput {
            name: Some(self.name.trimmed(cx).unwrap_or_else(|| self.suggested.clone())),
            // A proposal nobody completed is sent empty: the service then derives the reference.
            sku: self.sku.trimmed(cx).filter(|sku| *sku != self.sku_prefix.trim()),
            stock_quantity: Some(to_quantity(&self.stock.text(cx)).unwrap_or(0.)),
            selling_price_ttc_cents: price_cents(&self.price.text(cx)),
            attrs,
        })
    }

    pub fn render(&self, page: &Entity<StockPage>, cx: &App) -> Div {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let cell = |field: FormField| div().flex_1().min_w_0().child(field);
        let page = page.clone();
        col()
            .gap(m.space_3)
            .children(self.error.clone().map(|e| Alert::danger().child(e)))
            .child(div().fs(m.fs_sm).text_color(c.text_muted).child(
                "Une variante est un article distinct, avec son propre stock et sa propre référence. Exemple : couleur bleu. Combinez plusieurs caractéristiques si besoin (couleur bleu · taille 10).",
            ))
            .child(
                FormField::new("Nom")
                    .hint("Modifiez-le, ou laissez la suggestion basée sur le modèle.")
                    .child(Input::new(&self.name)),
            )
            .children(self.attrs.iter().enumerate().map(|(index, (name, value))| {
                let missing = self.attr_missing && index == 0;
                row()
                    .gap(m.space_3)
                    .items_start()
                    .child(cell(
                        FormField::new("Caractéristique")
                            .error(missing.then_some("Obligatoire."))
                            .child(Input::new(name).invalid(missing)),
                    ))
                    .child(cell(FormField::new("Valeur").child(Input::new(value))))
            }))
            .child(row().child(Button::new("variant-add-attr").ghost().small().label("Ajouter une caractéristique").on_click(
                move |_, window, cx| {
                    page.update(cx, |page, cx| {
                        if let Some(form) = page.variant.as_mut() {
                            form.add_attr(window, cx);
                        }
                        cx.notify();
                    })
                },
            )))
            .child(
                row()
                    .gap(m.space_3)
                    .items_start()
                    .child(cell(
                        FormField::new("Référence / SKU").hint("Laissée vide, elle est déduite du parent.").child(Input::new(&self.sku).mono()),
                    ))
                    .child(cell(FormField::new("Stock initial").child(Input::new(&self.stock))))
                    .child(cell(FormField::new("Prix TTC (€)").child(Input::new(&self.price)))),
            )
            .when(self.busy, |this| this.opacity(0.6))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use super::{price_cents, to_int, to_quantity};

    #[test]
    fn a_stock_figure_keeps_its_decimals_to_the_thousandth() {
        assert_eq!(to_quantity("2,5"), Some(2.5));
        assert_eq!(to_quantity(" 1 250.75 "), Some(1250.75));
        assert_eq!(to_quantity("0,33333"), Some(0.333));
        assert_eq!(to_quantity("-4"), Some(-4.));
        assert_eq!(to_quantity(""), None);
        assert_eq!(to_quantity("beaucoup"), None);
    }

    #[test]
    fn a_whole_number_drops_what_follows_the_point() {
        assert_eq!(to_int("5"), Some(5));
        assert_eq!(to_int(" 12.9 "), Some(12));
        assert_eq!(to_int("-3"), Some(-3));
        assert_eq!(to_int(""), None);
        assert_eq!(to_int("cinq"), None);
    }

    #[test]
    fn a_negative_price_is_no_price() {
        assert_eq!(price_cents("49,90"), Some(4_990));
        assert_eq!(price_cents("0"), Some(0));
        assert_eq!(price_cents("-5"), None);
        assert_eq!(price_cents(""), None);
    }
}
