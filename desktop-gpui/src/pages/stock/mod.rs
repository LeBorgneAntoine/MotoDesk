//! Inventaire (port of `routes/Stock.svelte` and `routes/stock/*`): the catalogue.
//!
//! A rail that narrows the list (everything, low stock, one category), a search, the table of
//! products. A row opens the product's panel: read it, edit it in place, add a variant. A
//! supplier's delivery is recorded from the toolbar.

mod forms;

use std::rc::Rc;
use std::time::Duration;

use motodesk_client::products::{
    self, Category, ListQuery, Product, ProductListItem, is_low_stock, variant_root_name, vehicle_type_label,
};
use motodesk_client::stock::{self, MovementInput, MovementKind, StockMovement, Supplier, SupplierInput, movement_kind_label};
use motodesk_client::{ApiError, settings};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use self::forms::{CategoryForm, ProductForm, VariantForm, price_cents, to_quantity};
use crate::backend::ApiExt as _;
use crate::format::{self, cents_to_text, humanize_error};
use crate::router::Router;

/// How long the list waits after the last keystroke before searching.
const SEARCH_DELAY: Duration = Duration::from_millis(250);
/// How many products the reception's product field offers at a time.
const PRODUCTS_OFFERED: u32 = 40;
/// How many movements a product's panel shows.
const MOVEMENTS_SHOWN: usize = 12;

const LOAD_ERROR: &str = "Impossible de charger l'inventaire.";
const PRODUCT: &str = "product";
const CATEGORY: &str = "category";
const NEW: &str = "new";
/// "No supplier" in the reception's list.
const NO_SUPPLIER: &str = "-";

/// What the rail narrows the list to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Filter {
    All,
    Low,
    Category(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    View,
    Create,
    Edit,
}

/// The delivery being recorded in the "Réception fournisseur" dialog.
struct Reception {
    product: ComboModel,
    searches: usize,
    quantity: TextModel,
    supplier: SelectModel,
    new_supplier: TextModel,
    cost: TextModel,
    busy: bool,
}

/// The colour of a stock figure: none left, at or under the alert threshold, or fine.
fn stock_status(is_service: bool, quantity: f64, threshold: Option<i64>, alerts: bool) -> Status {
    if is_service {
        Status::Neutral
    } else if quantity == 0. {
        Status::Danger
    } else if alerts && is_low_stock(is_service, quantity, threshold) {
        Status::Warning
    } else {
        Status::Neutral
    }
}

/// "12 pièce", "2,5 L".
fn quantity_label(quantity: f64, unit: products::Unit) -> String {
    format!("{} {}", format::number(quantity), unit.label())
}

/// The line under a product's name in the list: its brand, then its variants or which one it is.
fn list_detail(product: &ProductListItem) -> Option<String> {
    let mut parts: Vec<String> = product.brand.clone().into_iter().collect();
    if product.variant_count > 0 {
        parts.push(format!("{} variante{}", product.variant_count, if product.variant_count > 1 { "s" } else { "" }));
    } else if let Some(label) = product.variant_label.clone().filter(|label| !label.is_empty()) {
        parts.push(label);
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The line under a variant's name in its model's panel.
fn variant_detail(variant: &ProductListItem) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.extend(variant.variant_label.clone().filter(|label| !label.is_empty()));
    parts.extend(variant.sku.clone().filter(|sku| !sku.is_empty()));
    if !variant.is_service {
        parts.push(quantity_label(variant.stock_quantity, variant.unit));
    }
    parts.extend(variant.selling_price_ttc_cents.map(format::euros));
    parts.join(" · ")
}

pub struct StockPage {
    categories: Vec<Category>,
    loading_categories: bool,
    /// What the table shows: the answer to the current filter and search.
    products: Rc<Vec<ProductListItem>>,
    loading: bool,
    error: Option<SharedString>,
    filter: Filter,
    search: TextModel,
    searches: usize,
    /// Counts the list requests, so a slow answer never replaces a newer one.
    loads: usize,

    // The shop's settings this screen follows.
    low_stock_alerts: bool,
    default_vat_bp: i64,
    default_threshold: Option<i64>,

    panels: Entity<PanelStack>,
    mode: Mode,
    product: Option<Product>,
    product_loading: bool,
    product_error: Option<SharedString>,
    product_form: Option<ProductForm>,
    variants: Vec<ProductListItem>,
    movements: Vec<StockMovement>,
    /// The category being renamed; `None` while creating one.
    category: Option<Category>,
    category_form: Option<CategoryForm>,
    saving: bool,
    save_error: Option<SharedString>,

    reception: Option<Reception>,
    variant: Option<VariantForm>,
    _subscriptions: Vec<Subscription>,
}

impl StockPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = TextModel::builder().placeholder("Rechercher (nom, marque, SKU)…").build(window, cx);
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
        let reopen = remembered.into_iter().find(|(kind, key)| kind == PRODUCT && key != NEW).map(|(_, key)| key);

        let mut page = Self {
            categories: Vec::new(),
            loading_categories: true,
            products: Rc::new(Vec::new()),
            loading: true,
            error: None,
            filter: Filter::All,
            search,
            searches: 0,
            loads: 0,
            low_stock_alerts: true,
            default_vat_bp: 2000,
            default_threshold: Some(2),
            panels,
            mode: Mode::View,
            product: None,
            product_loading: false,
            product_error: None,
            product_form: None,
            variants: Vec::new(),
            movements: Vec::new(),
            category: None,
            category_form: None,
            saving: false,
            save_error: None,
            reception: None,
            variant: None,
            _subscriptions: subscriptions,
        };
        page.load_settings(cx);
        page.refresh_all(cx);
        if let Some(id) = reopen {
            page.open_product(id, cx);
        }
        page
    }

    // ---- loading ----------------------------------------------------------------------------

    /// The shop's choices this screen follows: whether low stock is flagged, and what a new
    /// product starts with. Without an answer the usual values stand.
    fn load_settings(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { settings::list(&client).await },
            |this, result, cx| {
                let Ok(stored) = result else { return };
                for setting in stored {
                    match setting.key.as_str() {
                        "stock.alert_low_stock" => this.low_stock_alerts = setting.value.as_bool() != Some(false),
                        "vat.default_rate_bp" => {
                            let rate = setting.value.as_i64();
                            // A rate the lists cannot show would be saved without being seen.
                            this.default_vat_bp =
                                rate.filter(|rate| products::VAT_PRESETS.iter().any(|(_, bp)| bp == rate)).unwrap_or(2000);
                        }
                        "stock.low_threshold_default" => {
                            this.default_threshold = setting.value.as_f64().filter(|n| *n >= 0.).map(|n| n.round() as i64);
                        }
                        _ => {}
                    }
                }
                cx.notify();
            },
        );
    }

    fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.load_categories(cx);
        self.load_products(cx);
    }

    fn load_categories(&mut self, cx: &mut Context<Self>) {
        self.loading_categories = true;
        cx.notify();
        cx.api(
            |client| async move { products::list_categories(&client).await },
            |this, result, cx| {
                this.loading_categories = false;
                // Without the categories the list still works: the rail stays empty.
                this.categories = result.unwrap_or_default();
                cx.notify();
            },
        );
    }

    fn load_products(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.loads += 1;
        let this_load = self.loads;
        let query = ListQuery {
            q: self.search.text(cx),
            category_id: match &self.filter {
                Filter::Category(id) => Some(id.clone()),
                _ => None,
            },
            low_stock: self.filter == Filter::Low,
            ..Default::default()
        };
        cx.notify();
        cx.api(
            move |client| async move { products::list(&client, &query).await },
            move |this, result, cx| {
                if this.loads != this_load {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(found) => this.products = Rc::new(found),
                    Err(error) => {
                        this.products = Rc::new(Vec::new());
                        this.error = Some(humanize_error(&error, LOAD_ERROR).into());
                    }
                }
                cx.notify();
            },
        );
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        if self.filter != filter {
            self.filter = filter;
            self.load_products(cx);
        }
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        self.searches += 1;
        let this_search = self.searches;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.searches == this_search {
                    this.load_products(cx);
                }
            });
        })
        .detach();
    }

    // ---- the panel stack --------------------------------------------------------------------

    fn panels_changed(&mut self, cx: &mut Context<Self>) {
        let (open, serialized) = {
            let stack = self.panels.read(cx);
            (stack.is_open(), stack.serialize())
        };
        if !open {
            self.mode = Mode::View;
            self.product = None;
            self.product_loading = false;
            self.product_error = None;
            self.product_form = None;
            self.variants.clear();
            self.movements.clear();
            self.category = None;
            self.category_form = None;
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

    fn layer(&self, cx: &App) -> Option<SharedString> {
        self.panels.read(cx).top().map(|layer| layer.id.clone())
    }

    /// One panel at a time on this screen: a new one takes the place of the open one.
    fn show(&self, layer: PanelLayer, cx: &mut Context<Self>) {
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });
    }

    /// A field of the open form changed.
    pub(super) fn form_changed(&mut self, cx: &mut Context<Self>) {
        let dirty = if let Some(form) = self.product_form.as_mut() {
            form.recheck(cx);
            form.is_dirty(cx)
        } else if let Some(form) = self.category_form.as_mut() {
            form.recheck(cx);
            form.is_dirty(cx)
        } else {
            return;
        };
        if let Some(id) = self.layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, dirty));
        }
        cx.notify();
    }

    /// Enter in a field of the open form.
    pub(super) fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.product_form.is_some() {
            self.save_product(window, cx);
        } else if self.category_form.is_some() {
            self.save_category(window, cx);
        }
    }

    // ---- products ---------------------------------------------------------------------------

    fn open_product(&mut self, id: String, cx: &mut Context<Self>) {
        self.mode = Mode::View;
        self.product = None;
        self.product_form = None;
        self.product_error = None;
        self.product_loading = true;
        self.variants.clear();
        self.movements.clear();
        self.category = None;
        self.category_form = None;
        self.save_error = None;
        self.show(PanelLayer::new(PRODUCT, id.clone(), "Produit").icon(IconName::Package).width(PanelWidth::Lg), cx);

        let asked = id.clone();
        cx.api(
            move |client| async move {
                let product = products::get(&client, &id).await?;
                // The panel is worth showing without these two.
                let variants = products::list_variants(&client, &id).await.unwrap_or_default();
                let movements = stock::list_movements(&client, Some(&id)).await.unwrap_or_default();
                Ok::<_, ApiError>((product, variants, movements))
            },
            move |this, result, cx| {
                // The answer for a product that is no longer the open one.
                if this.panels.read(cx).key_of(PRODUCT).as_deref() != Some(asked.as_str()) {
                    return;
                }
                this.product_loading = false;
                match result {
                    Ok((product, variants, movements)) => {
                        if let Some(layer) = this.layer(cx) {
                            let (title, brand) = (product.name.clone(), product.brand.clone().map(SharedString::from));
                            this.update_panels(cx, |stack| stack.retitle(&layer, title, brand));
                        }
                        this.variants = variants.into_iter().filter(|variant| variant.id != product.id).collect();
                        this.movements = movements.into_iter().take(MOVEMENTS_SHOWN).collect();
                        this.product = Some(product);
                    }
                    Err(error) => {
                        this.product_error = Some(humanize_error(&error, "Impossible de charger le produit.").into());
                    }
                }
                cx.notify();
            },
        );
    }

    fn open_create_product(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.product = None;
        self.product_error = None;
        self.product_loading = false;
        self.category = None;
        self.category_form = None;
        self.mode = Mode::Create;
        self.save_error = None;
        let form = ProductForm::new(None, &self.categories, self.default_vat_bp, self.default_threshold, window, cx);
        form.focus(window, cx);
        self.product_form = Some(form);
        self.show(
            PanelLayer::new(PRODUCT, NEW, "Nouveau produit")
                .subtitle("Ajoutez un article au catalogue.")
                .icon(IconName::Package)
                .width(PanelWidth::Lg),
            cx,
        );
    }

    fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(product) = self.product.as_ref() else { return };
        let form =
            ProductForm::new(Some(product), &self.categories, self.default_vat_bp, self.default_threshold, window, cx);
        form.focus(window, cx);
        self.product_form = Some(form);
        self.mode = Mode::Edit;
        self.save_error = None;
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.mode = Mode::View;
        self.product_form = None;
        self.save_error = None;
        if let Some(id) = self.layer(cx) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, false));
        }
        cx.notify();
    }

    fn save_product(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(form) = self.product_form.as_mut() else { return };
        let Some(input) = form.validated(cx) else {
            form.focus(window, cx);
            return cx.notify();
        };
        let editing = (self.mode == Mode::Edit).then(|| self.product.as_ref().map(|p| p.id.clone())).flatten();
        self.saving = true;
        self.save_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                match editing {
                    Some(id) => products::update(&client, &id, &input).await,
                    None => products::create(&client, &input).await,
                }
            },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok(saved) => {
                        let created = this.mode == Mode::Create;
                        toast::success(if created { "Produit créé." } else { "Produit enregistré." }, window, cx);
                        this.refresh_all(cx);
                        // Read it again: a corrected stock has written a movement.
                        this.open_product(saved.id, cx);
                    }
                    Err(error) => {
                        this.save_error = Some(humanize_error(&error, "Impossible d'enregistrer le produit.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    fn ask_delete_product(&mut self, id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        Confirm::new("Supprimer ce produit ?", format!("Supprimer « {name} » ? Cette action est définitive."))
            .confirm_label("Supprimer")
            .danger()
            .open(window, cx, move |window, cx| {
                let id = id.clone();
                page.update(cx, |_, cx| {
                    cx.api_in(
                        window,
                        move |client| async move { products::delete(&client, &id).await },
                        |this, result, window, cx| match result {
                            Ok(()) => {
                                toast::success("Produit supprimé.", window, cx);
                                this.update_panels(cx, |stack| stack.close_all());
                                this.refresh_all(cx);
                            }
                            Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
                        },
                    )
                })
            });
    }

    // ---- categories -------------------------------------------------------------------------

    fn open_category(&mut self, category: Option<Category>, window: &mut Window, cx: &mut Context<Self>) {
        self.product = None;
        self.product_form = None;
        self.product_error = None;
        self.product_loading = false;
        self.save_error = None;
        let form = CategoryForm::new(category.as_ref(), window, cx);
        form.focus(window, cx);
        self.category_form = Some(form);
        let layer = match category.as_ref() {
            Some(category) => {
                let n = category.product_count;
                let s = if n > 1 { "s" } else { "" };
                PanelLayer::new(CATEGORY, category.id.clone(), category.name.clone())
                    .subtitle(format!("{n} produit{s} classé{s}"))
            }
            None => PanelLayer::new(CATEGORY, NEW, "Nouvelle catégorie"),
        };
        self.category = category;
        self.show(layer.icon(IconName::Tag).width(PanelWidth::Sm), cx);
    }

    fn save_category(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(form) = self.category_form.as_mut() else { return };
        let Some(input) = form.validated(cx) else { return cx.notify() };
        let editing = self.category.as_ref().map(|category| category.id.clone());
        self.saving = true;
        self.save_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                match editing {
                    Some(id) => products::update_category(&client, &id, &input).await.map(|_| false),
                    None => products::create_category(&client, &input).await.map(|_| true),
                }
            },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok(created) => {
                        toast::success(if created { "Catégorie créée." } else { "Catégorie enregistrée." }, window, cx);
                        this.update_panels(cx, |stack| stack.close_all());
                        // A renamed category shows in the products' own column too.
                        this.refresh_all(cx);
                    }
                    Err(error) => {
                        this.save_error = Some(humanize_error(&error, "Impossible d'enregistrer la catégorie.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    fn ask_delete_category(&mut self, category: Category, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        let consequence = if category.product_count > 0 {
            format!(
                "Les {} produit(s) associés conserveront leurs données mais ne seront plus classés dans une catégorie.",
                category.product_count
            )
        } else {
            "Aucun produit n'y est associé.".to_string()
        };
        Confirm::new("Supprimer cette catégorie ?", format!("Supprimer « {} » ? {consequence}", category.name))
            .confirm_label("Supprimer")
            .danger()
            .open(window, cx, move |window, cx| {
                let id = category.id.clone();
                page.update(cx, |_, cx| {
                    cx.api_in(
                        window,
                        {
                            let id = id.clone();
                            move |client| async move { products::delete_category(&client, &id).await }
                        },
                        move |this, result, window, cx| match result {
                            Ok(()) => {
                                toast::success("Catégorie supprimée.", window, cx);
                                if this.filter == Filter::Category(id.clone()) {
                                    this.filter = Filter::All;
                                }
                                this.update_panels(cx, |stack| stack.close_all());
                                this.refresh_all(cx);
                            }
                            Err(error) => toast::error(humanize_error(&error, "Suppression impossible."), window, cx),
                        },
                    )
                })
            });
    }

    // ---- a supplier's delivery --------------------------------------------------------------

    fn open_reception(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The suppliers first: the list of the dialog is built from them.
        cx.api_in(
            window,
            |client| async move { Ok::<_, ApiError>(stock::list_suppliers(&client).await.unwrap_or_default()) },
            |this, result, window, cx| this.show_reception(result.unwrap_or_default(), window, cx),
        );
    }

    fn show_reception(&mut self, suppliers: Vec<Supplier>, window: &mut Window, cx: &mut Context<Self>) {
        let product = ComboModel::new("Nom, SKU, marque…", window, cx);
        let (on_query, on_pick) = (cx.entity().downgrade(), cx.entity().downgrade());
        product.on_query(cx, move |query, _, cx| {
            let query = query.to_string();
            let _ = on_query.update(cx, |this, cx| this.search_reception_products(query, cx));
        });
        product.on_pick(cx, move |_, _, cx| {
            let _ = on_pick.update(cx, |_, cx| cx.notify());
        });
        let mut supplier_options = vec![SelectOption::new(NO_SUPPLIER, "Sans fournisseur / nouveau ci-dessous")];
        supplier_options.extend(suppliers.iter().map(|s| SelectOption::new(s.id.clone(), s.name.clone())));
        let enter_saves = |field: &TextModel, window: &mut Window, cx: &mut Context<Self>| {
            field.on_event(window, cx, |this, event, window, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.save_reception(window, cx);
                }
            })
        };
        let quantity = TextModel::builder().value("1").build(window, cx);
        let new_supplier = TextModel::builder().placeholder("Nom du fournisseur").build(window, cx);
        let cost = TextModel::builder().placeholder("0,00").build(window, cx);
        // These live as long as the page: a handful per dialog opened, dropped with the screen.
        for field in [&quantity, &new_supplier, &cost] {
            let subscription = enter_saves(field, window, cx);
            self._subscriptions.push(subscription);
        }
        self.reception = Some(Reception {
            product: product.clone(),
            searches: 0,
            quantity,
            supplier: SelectModel::new(supplier_options, Some(NO_SUPPLIER), window, cx),
            new_supplier,
            cost,
            busy: false,
        });
        self.search_reception_products(String::new(), cx);

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Réception fournisseur").keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let Some(reception) = for_body.read(cx).reception.as_ref() else { return element(div()) };
                element(
                    col()
                        .gap(m.space_3)
                        .child(FormField::new("Produit").required().child(Combobox::new(&reception.product)))
                        .child(FormField::new("Quantité").required().child(Input::new(&reception.quantity)))
                        .child(FormField::new("Fournisseur").child(Select::new(&reception.supplier)))
                        .child(
                            FormField::new("Nouveau fournisseur")
                                .hint("Ignoré si un fournisseur est déjà choisi.")
                                .child(Input::new(&reception.new_supplier)),
                        )
                        .child(FormField::new("Coût unitaire HT").child(Input::new(&reception.cost))),
                )
            },
            move |_, cx| {
                let (chosen, busy) = for_footer
                    .read(cx)
                    .reception
                    .as_ref()
                    .map(|reception| (reception.product.value(cx).is_some(), reception.busy))
                    .unwrap_or((false, false));
                let page = for_footer.clone();
                vec![
                    element(
                        Button::new("reception-cancel")
                            .ghost()
                            .label("Annuler")
                            .disabled(busy)
                            .on_click(|_, window, cx| close_modal(window, cx)),
                    ),
                    element(
                        Button::new("reception-save").primary().label("Enregistrer").disabled(!chosen).loading(busy).on_click(
                            move |_, window, cx| page.update(cx, |this, cx| this.save_reception(window, cx)),
                        ),
                    ),
                ]
            },
        );
        // After the dialog has opened (it takes the focus when it does): start in the product field.
        product.focus(window, cx);
    }

    fn search_reception_products(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(reception) = self.reception.as_mut() else { return };
        reception.searches += 1;
        let this_search = reception.searches;
        reception.product.set_loading(true, cx);
        let query = ListQuery { q: query, limit: Some(PRODUCTS_OFFERED), ..Default::default() };
        cx.api(
            move |client| async move { products::list(&client, &query).await },
            move |this, result, cx| {
                let Some(reception) = this.reception.as_ref() else { return };
                // An answer to an older keystroke must not replace a newer one.
                if reception.searches != this_search {
                    return;
                }
                let options = result
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|product| !product.is_service)
                    .map(|product| {
                        let detail = [product.sku.as_deref(), product.brand.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                        ComboOption::new(product.id.clone(), product.name.clone()).detail(detail)
                    })
                    .collect();
                reception.product.set_options(options, cx);
            },
        );
    }

    fn save_reception(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(reception) = self.reception.as_mut() else { return };
        if reception.busy {
            return;
        }
        let Some(product_id) = reception.product.value(cx) else { return };
        let Some(quantity) = to_quantity(&reception.quantity.text(cx)).filter(|quantity| *quantity != 0.) else {
            return toast::error("Indiquez une quantité.", window, cx);
        };
        let chosen_supplier = reception.supplier.value(cx).filter(|id| id != NO_SUPPLIER && !id.is_empty());
        let new_supplier = reception.new_supplier.trimmed(cx);
        let input = MovementInput {
            product_id,
            supplier_id: chosen_supplier,
            kind: MovementKind::Reception,
            quantity_milli: (quantity * 1000.).round() as i64,
            unit_cost_ht_cents: price_cents(&reception.cost.text(cx)),
            reason: None,
        };
        reception.busy = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                let mut input = input;
                // A supplier named but not chosen is created on the way.
                if input.supplier_id.is_none()
                    && let Some(name) = new_supplier
                {
                    let created = stock::create_supplier(&client, &SupplierInput { name, ..Default::default() }).await?;
                    input.supplier_id = Some(created.id);
                }
                stock::create_movement(&client, &input).await
            },
            |this, result, window, cx| {
                if let Some(reception) = this.reception.as_mut() {
                    reception.busy = false;
                }
                match result {
                    Ok(_) => {
                        this.reception = None;
                        close_modal(window, cx);
                        toast::success("Réception enregistrée.", window, cx);
                        this.load_products(cx);
                    }
                    Err(error) => {
                        toast::error(humanize_error(&error, "Impossible d'enregistrer la réception."), window, cx)
                    }
                }
                cx.notify();
            },
        );
    }

    // ---- variants ---------------------------------------------------------------------------

    fn open_variant(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(product) = self.product.as_ref() else { return };
        let parent = variant_root_name(&product.name, product.parent_id.as_deref(), product.variant_label.as_deref());
        let sku = product.sku.as_ref().map(|sku| format!("{sku}-")).unwrap_or_default();
        let price = cents_to_text(product.selling_price_ttc_cents);
        let form = VariantForm::new(parent, sku, price, window, cx);

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Nouvelle variante").width(px(520.)).keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| match for_body.read(cx).variant.as_ref() {
                Some(form) => element(form.render(&for_body, cx)),
                None => element(div()),
            },
            move |_, cx| {
                let busy = for_footer.read(cx).variant.as_ref().is_some_and(|form| form.busy);
                let page = for_footer.clone();
                vec![
                    element(
                        Button::new("variant-cancel")
                            .ghost()
                            .label("Annuler")
                            .disabled(busy)
                            .on_click(|_, window, cx| close_modal(window, cx)),
                    ),
                    element(Button::new("variant-save").primary().label("Créer la variante").loading(busy).on_click(
                        move |_, window, cx| page.update(cx, |this, cx| this.submit_variant(window, cx)),
                    )),
                ]
            },
        );
        // After the dialog has opened (it takes the focus when it does).
        form.focus(window, cx);
        self.variant = Some(form);
        cx.notify();
    }

    pub(super) fn variant_attrs_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.variant.as_mut() {
            form.attrs_changed(window, cx);
        }
        cx.notify();
    }

    pub(super) fn submit_variant(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent_id) = self.product.as_ref().map(|product| product.id.clone()) else { return };
        let Some(form) = self.variant.as_mut() else { return };
        if form.busy {
            return;
        }
        let Some(input) = form.validated(cx) else { return cx.notify() };
        form.busy = true;
        form.error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { products::create_variant(&client, &parent_id, &input).await },
            |this, result, window, cx| match result {
                Ok(created) => {
                    this.variant = None;
                    close_modal(window, cx);
                    toast::success("Variante créée.", window, cx);
                    this.load_products(cx);
                    // The new variant is a product of its own: show it.
                    this.open_product(created.id, cx);
                }
                Err(error) => {
                    if let Some(form) = this.variant.as_mut() {
                        form.busy = false;
                        form.error = Some(humanize_error(&error, "Impossible de créer la variante.").into());
                    }
                    cx.notify();
                }
            },
        );
    }

    // ---- drawing ----------------------------------------------------------------------------

    fn rail(&self, cx: &mut Context<Self>) -> FilterRail {
        let low = self.products.iter().filter(|p| is_low_stock(p.is_service, p.stock_quantity, p.low_stock_threshold)).count();
        let all = ListRow::new("stock-all", "Tous les produits")
            .icon(IconName::Boxes)
            .count(self.products.len().to_string(), None)
            .selected(self.filter == Filter::All)
            .on_click(cx.listener(|this, _, _, cx| this.set_filter(Filter::All, cx)));
        let low_row = ListRow::new("stock-low", "Stock bas")
            .icon(IconName::TriangleAlert)
            .selected(self.filter == Filter::Low)
            .on_click(cx.listener(|this, _, _, cx| this.set_filter(Filter::Low, cx)));
        let rail = FilterRail::new("stock-rail", "Catalogue")
            .action(
                IconButton::new("category-new", IconName::Plus)
                    .title("Nouvelle catégorie")
                    .on_click(cx.listener(|this, _, window, cx| this.open_category(None, window, cx))),
            )
            .child(all)
            .child(if low > 0 { low_row.count(low.to_string(), None) } else { low_row });

        if self.loading_categories && self.categories.is_empty() {
            return rail.child(div().h(px(120.)).child(StateView::loading().compact().title("Chargement…")));
        }
        if self.categories.is_empty() {
            return rail.child(
                div().h(px(220.)).child(
                    StateView::empty()
                        .compact()
                        .icon(IconName::Tag)
                        .title("Aucune catégorie")
                        .body("Classez vos produits pour retrouver l'essentiel plus vite.")
                        .action("Créer une catégorie", cx.listener(|this, _, window, cx| this.open_category(None, window, cx))),
                ),
            );
        }
        rail.children(self.categories.iter().enumerate().map(|(index, category)| {
            let (filter, to_edit) = (Filter::Category(category.id.clone()), category.clone());
            ListRow::new(("stock-category", index), category.name.clone())
                .icon(IconName::Tag)
                .count(category.product_count.to_string(), None)
                .selected(self.filter == filter)
                .trailing(IconButton::new(("category-edit", index), IconName::Pencil).title("Modifier la catégorie").on_click(
                    cx.listener(move |this, _, window, cx| {
                        // The click is for the button, not for the row under it.
                        cx.stop_propagation();
                        this.open_category(Some(to_edit.clone()), window, cx)
                    }),
                ))
                .on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter.clone(), cx)))
        }))
    }

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if let Some(error) = self.error.clone() {
            return StateView::error()
                .title(LOAD_ERROR)
                .body(error)
                .action("Réessayer", cx.listener(|this, _, _, cx| this.load_products(cx)))
                .into_any_element();
        }
        if self.loading && self.products.is_empty() {
            return StateView::loading().title("Chargement du catalogue…").into_any_element();
        }
        if self.products.is_empty() {
            let searching = self.search.trimmed(cx).is_some();
            let (icon, title, body, can_add) = match (&self.filter, searching) {
                (Filter::Low, false) => (
                    IconName::TriangleAlert,
                    "Aucun produit en stock bas",
                    "Tous vos produits sont au-dessus de leur seuil d'alerte. Définissez un seuil sur un produit pour être prévenu.",
                    false,
                ),
                (filter, true) => (
                    if *filter == Filter::Low { IconName::TriangleAlert } else { IconName::Package },
                    "Aucun résultat",
                    "Aucun produit ne correspond à votre recherche.",
                    false,
                ),
                (Filter::All, false) => (
                    IconName::Package,
                    "Aucun produit pour le moment",
                    "Commencez par ajouter votre premier produit pour constituer votre catalogue.",
                    true,
                ),
                (Filter::Category(_), false) => (
                    IconName::Package,
                    "Cette catégorie est vide",
                    "Ajoutez un produit et classez-le dans cette catégorie.",
                    true,
                ),
            };
            let state = StateView::empty().icon(icon).title(title).body(body);
            return if can_add {
                state.action("Ajouter un produit", cx.listener(|this, _, window, cx| this.open_create_product(window, cx)))
            } else {
                state
            }
            .into_any_element();
        }

        let (rows, for_click, for_muted) = (self.products.clone(), self.products.clone(), self.products.clone());
        let (page, page_click) = (cx.entity(), cx.entity());
        let alerts = self.low_stock_alerts;
        let open = self.panels.read(cx).key_of(PRODUCT);
        let selected = open.and_then(|id| rows.iter().position(|p| p.id.as_str() == id.as_ref()));
        let dash = move || div().text_color(c.text_subtle).child("—").into_any_element();

        DataTable::new(
            "stock-table",
            vec![
                Column::new("Produit").flex(1.),
                Column::new("SKU").fixed(px(170.)).mono(),
                Column::new("Catégorie").fixed(px(180.)),
                Column::new("Stock").fixed(px(140.)).right(),
                Column::new("Prix TTC").fixed(px(130.)).right(),
                Column::new("").fixed(px(88.)).right(),
            ],
            rows.len(),
            move |ix, column, _, _| {
                let product = &rows[ix];
                match column {
                    0 => col()
                        .min_w_0()
                        .gap(px(1.))
                        .child(div().one_line().font_weight(FontWeight::MEDIUM).child(product.name.clone()))
                        .when_some(list_detail(product), |this, detail| {
                            this.child(div().one_line().fs_tight(m.fs_xs).text_color(c.text_muted).child(detail))
                        })
                        .into_any_element(),
                    1 => match product.sku.clone().filter(|sku| !sku.is_empty()) {
                        Some(sku) => div().one_line().child(sku).into_any_element(),
                        None => dash(),
                    },
                    2 => match product.category_name.clone().filter(|name| !name.is_empty()) {
                        Some(name) => Tag::new(name).into_any_element(),
                        None => dash(),
                    },
                    3 if product.is_service => div().text_color(c.text_subtle).child("service").into_any_element(),
                    3 => Tag::new(quantity_label(product.stock_quantity, product.unit))
                        .status(stock_status(false, product.stock_quantity, product.low_stock_threshold, alerts))
                        .into_any_element(),
                    4 => match product.selling_price_ttc_cents {
                        Some(cents) => format::euros(cents).into_any_element(),
                        None => dash(),
                    },
                    _ => {
                        let (to_open, to_delete) = (page.clone(), page.clone());
                        let (id, id_to_delete, name) = (product.id.clone(), product.id.clone(), product.name.clone());
                        // These clicks are for the buttons, not for the row under them.
                        row()
                            .gap(m.space_1)
                            .child(IconButton::new(("product-edit", ix), IconName::Pencil).title("Modifier").on_click(
                                move |_, _, cx| {
                                    cx.stop_propagation();
                                    let id = id.clone();
                                    to_open.update(cx, |this, cx| this.open_product(id, cx))
                                },
                            ))
                            .child(IconButton::new(("product-delete", ix), IconName::Trash).title("Supprimer").danger().on_click(
                                move |_, window, cx| {
                                    cx.stop_propagation();
                                    let (id, name) = (id_to_delete.clone(), name.clone());
                                    to_delete.update(cx, |this, cx| this.ask_delete_product(id, name, window, cx))
                                },
                            ))
                            .into_any_element()
                    }
                }
            },
        )
        .row_height(px(48.))
        .selected(selected)
        .muted_rows(move |ix| for_muted.get(ix).is_some_and(|product| !product.is_active))
        .on_row_click(move |ix, _, cx| {
            let Some(id) = for_click.get(*ix).map(|product| product.id.clone()) else { return };
            page_click.update(cx, |this, cx| this.open_product(id, cx));
        })
        .into_any_element()
    }

    fn category_panel(&self, panel: Panel, cx: &mut Context<Self>) -> Panel {
        let Some(form) = self.category_form.as_ref() else { return panel };
        let stack = self.panels.clone();
        let panel = match self.category.clone() {
            Some(category) => panel.action(
                IconButton::new("category-delete", IconName::Trash)
                    .title("Supprimer la catégorie")
                    .size(IconButtonSize::Md)
                    .danger()
                    .on_click(cx.listener(move |this, _, window, cx| this.ask_delete_category(category.clone(), window, cx))),
            ),
            None => panel,
        };
        panel
            .child(PanelSection::new("Catégorie").icon(IconName::Tag).child(form.render(self.save_error.clone(), cx)))
            .footer(
                Button::new("category-cancel")
                    .ghost()
                    .label("Annuler")
                    .disabled(self.saving)
                    .on_click(move |_, window, cx| request_close(&stack, window, cx)),
            )
            .footer(
                Button::new("category-save")
                    .primary()
                    .loading(self.saving)
                    .label(if self.category.is_some() { "Enregistrer" } else { "Créer" })
                    .on_click(cx.listener(|this, _, window, cx| this.save_category(window, cx))),
            )
    }

    fn product_panel(&self, panel: Panel, cx: &mut Context<Self>) -> Panel {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let stack = self.panels.clone();

        if self.product_loading {
            return panel.child(div().h(px(240.)).child(StateView::loading().title("Chargement du produit…")));
        }
        if let Some(error) = self.product_error.clone() {
            return panel.child(div().h(px(240.)).child(StateView::error().title("Produit indisponible").body(error)));
        }

        if self.mode != Mode::View {
            let Some(form) = self.product_form.as_ref() else { return panel };
            let creating = self.mode == Mode::Create;
            let panel = if creating { panel } else { panel.badge("Édition", Status::Accent) };
            return panel
                .children(form.render(self.save_error.clone(), cx))
                .footer(Button::new("product-cancel").ghost().label("Annuler").disabled(self.saving).on_click(cx.listener(
                    move |this, _, window, cx| {
                        if creating { request_close(&stack, window, cx) } else { this.cancel_edit(cx) }
                    },
                )))
                .footer(
                    Button::new("product-save")
                        .primary()
                        .loading(self.saving)
                        .label(if creating { "Créer le produit" } else { "Enregistrer" })
                        .on_click(cx.listener(|this, _, window, cx| this.save_product(window, cx))),
                );
        }

        let Some(product) = self.product.as_ref() else { return panel };
        let mut panel = panel;
        if !product.is_active {
            panel = panel.badge("Inactif", Status::Muted);
        } else if product.is_service {
            panel = panel.badge("Service", Status::Muted);
        }
        let (id, name) = (product.id.clone(), product.name.clone());
        let mut panel = panel
            .action(
                IconButton::new("product-edit", IconName::Pencil)
                    .title("Modifier le produit")
                    .size(IconButtonSize::Md)
                    .on_click(cx.listener(|this, _, window, cx| this.start_edit(window, cx))),
            )
            .action(
                IconButton::new("product-delete", IconName::Trash)
                    .title("Supprimer le produit")
                    .size(IconButtonSize::Md)
                    .danger()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.ask_delete_product(id.clone(), name.clone(), window, cx)
                    })),
            );

        // The two figures people open a product for.
        if !product.is_service {
            let status = stock_status(false, product.stock_quantity, product.low_stock_threshold, self.low_stock_alerts);
            panel = panel.child(
                row()
                    .flex_none()
                    .gap(m.space_2)
                    .child(
                        Tag::new(format!("{} en stock", quantity_label(product.stock_quantity, product.unit)))
                            .status(status)
                            .icon(IconName::Boxes)
                            .medium(),
                    )
                    .when_some(product.selling_price_ttc_cents, |this, cents| {
                        this.child(Tag::new(format!("{} TTC", format::euros(cents))).status(Status::Accent).medium())
                    }),
            );
        }
        if let Some(parent) = product.parent_id.clone() {
            panel = panel.child(
                Alert::info().child(
                    row().gap(m.space_2).child("Cette fiche est une variante.").child(
                        Button::new("open-model").link().small().label("Ouvrir le modèle").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.load_products(cx);
                                this.open_product(parent.clone(), cx)
                            },
                        )),
                    ),
                ),
            );
        }

        let category = product.category_id.as_ref().and_then(|id| self.categories.iter().find(|c| &c.id == id)).map(|c| c.name.clone());
        let mut identity = vec![
            InfoItem::new("Référence", product.sku.clone()).mono().placeholder("Aucun SKU"),
            InfoItem::new("Marque", product.brand.clone()),
            InfoItem::new("Catégorie", category).placeholder("Non classé"),
            InfoItem::new("Unité", Some(product.unit.label())),
        ];
        if product.parent_id.is_some() {
            identity.push(InfoItem::new("Variante", product.variant_label.clone()));
        }
        identity.push(InfoItem::new("Description", product.description.clone()).block());
        panel = panel.child(PanelSection::new("Identité").icon(IconName::Package).child(InfoList::new(identity)));

        let vat = format!("{} %", format!("{:.2}", product.vat_rate_bp as f64 / 100.).replace('.', ","));
        panel = panel.child(PanelSection::new("Prix & TVA").icon(IconName::Banknote).child(InfoList::new(vec![
            InfoItem::new("Prix de vente TTC", product.selling_price_ttc_cents.map(format::euros)),
            InfoItem::new("Prix d'achat HT", product.purchase_price_ht_cents.map(format::euros)),
            InfoItem::new("TVA", Some(vat)),
        ])));

        if !product.is_service {
            panel = panel.child(PanelSection::new("Stock").icon(IconName::Boxes).child(InfoList::new(vec![
                InfoItem::new("Quantité", Some(quantity_label(product.stock_quantity, product.unit))),
                InfoItem::new("Seuil d'alerte", product.low_stock_threshold.map(|n| n.to_string())).placeholder("Aucun seuil"),
                InfoItem::new("Dernière modification", Some(format::date_time(&product.updated_at))),
            ])));
        }

        if product.has_vehicle_data() {
            let odometer = product.odometer_value.map(|value| {
                format!("{} {}", format::integer(value), product.odometer_unit.as_deref().unwrap_or_default()).trim().to_string()
            });
            panel = panel.child(PanelSection::new("Attributs véhicule").icon(IconName::Bike).child(InfoList::new(vec![
                InfoItem::new("Type", product.vehicle_type.as_deref().map(|kind| vehicle_type_label(kind).to_string())),
                InfoItem::new("Année", product.year.map(|year| year.to_string())),
                InfoItem::new("Cylindrée", product.cylinder_capacity.filter(|cc| *cc != 0).map(|cc| format!("{cc} cm³"))),
                InfoItem::new("Immatriculation", product.plate_number.clone()).mono(),
                InfoItem::new("VIN / série", product.vin_or_serial.clone()).mono(),
                InfoItem::new("Compteur", odometer),
            ])));
        }

        if let Some(notes) = product.notes.clone().filter(|notes| !notes.trim().is_empty()) {
            panel = panel.child(
                PanelSection::new("Notes internes")
                    .icon(IconName::StickyNote)
                    .child(div().text_color(c.text).child(notes)),
            );
        }

        let count = self.variants.len();
        let variants = PanelSection::new("Variantes")
            .subtitle(if count == 0 { "Aucune".to_string() } else { format!("{count} variante{}", if count > 1 { "s" } else { "" }) })
            .icon(IconName::Layers)
            .flush()
            .action(
                Button::new("variant-add")
                    .small()
                    .icon(IconName::Plus)
                    .label("Ajouter")
                    .on_click(cx.listener(|this, _, window, cx| this.open_variant(window, cx))),
            );
        panel = panel.child(if count == 0 {
            variants.child(
                div().h(px(200.)).child(
                    StateView::empty()
                        .compact()
                        .icon(IconName::Layers)
                        .title("Aucune variante")
                        .body("Créez une couleur, une taille ou toute autre déclinaison. Chaque variante a son propre stock.")
                        .action("Ajouter une variante", cx.listener(|this, _, window, cx| this.open_variant(window, cx))),
                ),
            )
        } else {
            variants.child(col().children(self.variants.iter().enumerate().map(|(index, variant)| {
                let id = variant.id.clone();
                ListRow::new(("variant", index), variant.name.clone())
                    .meta(variant_detail(variant))
                    .icon(IconName::Layers)
                    .on_click(cx.listener(move |this, _, _, cx| this.open_product(id.clone(), cx)))
            })))
        });

        if !self.movements.is_empty() {
            let last = self.movements.len() - 1;
            panel = panel.child(PanelSection::new("Mouvements").icon(IconName::Boxes).flush().child(col().children(
                self.movements.iter().enumerate().map(|(index, movement)| {
                    row()
                        .gap(m.space_3)
                        .px(m.space_4)
                        .py(m.space_2)
                        .fs(m.fs_sm)
                        .when(index != last, |this| this.hairline_bottom(c.border_subtle))
                        .child(div().flex_1().min_w_0().one_line().child(movement_kind_label(&movement.kind).to_string()))
                        .child(
                            div()
                                .flex_none()
                                .w(px(90.))
                                .text_right()
                                .font_weight(FontWeight::MEDIUM)
                                .child(format::number(movement.quantity_milli as f64 / 1000.)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .w(px(140.))
                                .text_right()
                                .text_color(c.text_muted)
                                .child(format::date_time(&movement.created_at)),
                        )
                }),
            )));
        }

        panel
            .footer(Button::new("product-close").ghost().label("Fermer").on_click(move |_, window, cx| request_close(&stack, window, cx)))
            .footer(
                Button::new("product-modify")
                    .primary()
                    .icon(IconName::Pencil)
                    .label("Modifier")
                    .on_click(cx.listener(|this, _, window, cx| this.start_edit(window, cx))),
            )
    }

    fn panel(&self, layer: &PanelLayer, cx: &mut Context<Self>) -> Panel {
        let panel = Panel::new(&self.panels, layer.id.clone());
        if layer.kind.as_ref() == CATEGORY { self.category_panel(panel, cx) } else { self.product_panel(panel, cx) }
    }
}

impl Render for StockPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let title = match &self.filter {
            Filter::All => "Tous les produits".to_string(),
            Filter::Low => "Stock bas".to_string(),
            Filter::Category(id) => {
                self.categories.iter().find(|c| &c.id == id).map(|c| c.name.clone()).unwrap_or_else(|| "Catégorie".to_string())
            }
        };
        let shown = self.products.len();
        let count = if self.loading {
            "Chargement…".to_string()
        } else {
            let searched = self.search.trimmed(cx).map(|term| format!(" · « {term} »")).unwrap_or_default();
            format!("{shown} produit{}{searched}", if shown > 1 { "s" } else { "" })
        };
        let layers: Vec<PanelLayer> = self.panels.read(cx).layers().to_vec();
        let panels: Vec<Panel> = layers.iter().map(|layer| self.panel(layer, cx)).collect();

        row()
            .relative()
            .size_full()
            .items_stretch()
            .bg(c.bg_surface)
            .on_key_down(PanelHost::key_handler(&self.panels))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key.as_str() == "f5" {
                    this.refresh_all(cx);
                }
            }))
            .child(self.rail(cx))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        PageToolbar::new(title)
                            .icon(IconName::Package)
                            .meta(count)
                            .trailing(SearchField::new(&self.search).width(px(280.)))
                            .trailing(Button::new("stock-reception").icon(IconName::Package).label("Réception").on_click(
                                cx.listener(|this, _, window, cx| this.open_reception(window, cx)),
                            ))
                            .trailing(Button::new("stock-new").primary().icon(IconName::Plus).label("Nouveau produit").on_click(
                                cx.listener(|this, _, window, cx| this.open_create_product(window, cx)),
                            )),
                    )
                    .child(div().flex_1().min_h_0().child(self.list(cx))),
            )
            .child(PanelHost::new(&self.panels).children(panels))
    }
}

// ---- menus ----------------------------------------------------------------------------------

impl StockPage {
    /// The menus of the title bar while this screen is shown.
    pub fn menus(&self, cx: &mut Context<Self>) -> Vec<MenuGroup> {
        let page = cx.entity();
        vec![
            MenuGroup::new(
                "Fichier",
                vec![
                    MenuEntry::new("Nouveau produit…", crate::menus::on(&page, |this, window, cx| this.open_create_product(window, cx)))
                        .icon(IconName::Plus)
                        .shortcut("Ctrl+N"),
                    MenuEntry::new("Réception fournisseur…", crate::menus::on(&page, |this, window, cx| this.open_reception(window, cx)))
                        .icon(IconName::Package)
                        .shortcut("Ctrl+Maj+R"),
                    MenuEntry::new("Nouvelle catégorie…", crate::menus::on(&page, |this, window, cx| this.open_category(None, window, cx)))
                        .icon(IconName::Tag)
                        .shortcut("Ctrl+Maj+T"),
                ],
            ),
            MenuGroup::new(
                "Affichage",
                vec![
                    MenuEntry::new("Tous les produits", crate::menus::on(&page, |this, _, cx| this.set_filter(Filter::All, cx))).icon(IconName::Boxes),
                    MenuEntry::new("Stock bas", crate::menus::on(&page, |this, _, cx| this.set_filter(Filter::Low, cx))).icon(IconName::TriangleAlert),
                    MenuEntry::separator(),
                    MenuEntry::new("Actualiser", crate::menus::on(&page, |this, _, cx| this.refresh_all(cx))).icon(IconName::RefreshCw).shortcut("F5"),
                ],
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::products::{ProductListItem, Unit};

    use super::{Status, list_detail, quantity_label, stock_status, variant_detail};

    fn item(json: serde_json::Value) -> ProductListItem {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_stock_figure_is_red_at_zero_and_amber_under_its_threshold() {
        assert_eq!(stock_status(false, 0., Some(5), true), Status::Danger);
        assert_eq!(stock_status(false, 0., None, false), Status::Danger, "nothing left is said even with alerts off");
        assert_eq!(stock_status(false, 3., Some(5), true), Status::Warning);
        assert_eq!(stock_status(false, 3., Some(5), false), Status::Neutral, "the shop switched the alerts off");
        assert_eq!(stock_status(false, 8., Some(5), true), Status::Neutral);
        assert_eq!(stock_status(true, 0., Some(5), true), Status::Neutral, "a service has no stock");
    }

    #[test]
    fn a_quantity_is_written_with_its_unit() {
        assert_eq!(quantity_label(12., Unit::Piece), "12 pièce");
        assert_eq!(quantity_label(2.5, Unit::Liter), "2,5 L");
    }

    #[test]
    fn the_line_under_a_product_says_its_brand_and_its_variants() {
        let model = item(serde_json::json!({ "id": "1", "name": "Casque", "brand": "Shark", "variant_count": 2 }));
        assert_eq!(list_detail(&model).as_deref(), Some("Shark · 2 variantes"));
        let variant = item(serde_json::json!({ "id": "2", "name": "Casque — taille M", "variant_label": "taille M" }));
        assert_eq!(list_detail(&variant).as_deref(), Some("taille M"));
        assert_eq!(list_detail(&item(serde_json::json!({ "id": "3", "name": "Huile" }))), None);
    }

    #[test]
    fn a_variant_is_summed_up_in_one_line() {
        let variant = item(serde_json::json!({
            "id": "2", "name": "Casque — taille M", "variant_label": "taille M", "sku": "CSQ-M",
            "stock_quantity": 3.0, "selling_price_ttc_cents": 19900
        }));
        assert_eq!(variant_detail(&variant), "taille M · CSQ-M · 3 pièce · 199,00\u{a0}€");
    }
}
