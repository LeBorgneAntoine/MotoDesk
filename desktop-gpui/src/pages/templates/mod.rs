//! Modèles (port of `desktop/src/routes/templates/`): the library of document templates, and
//! the editor a template is laid out in.
//!
//! Each card shows its template's page as it prints, drawn from the template itself with a
//! sample document: there is no picture of the page to keep up to date.

mod editor;
mod sheet;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use motodesk_client::templates::render::Mode;
use motodesk_client::templates::schema::Layout;
use motodesk_client::templates::{self, starters, ListQuery, Orientation, Template, TemplateKind, TemplateSummary};
use motodesk_client::ApiResult;
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use self::editor::{EditorEvent, TemplateEditor};
use self::sheet::Shop;
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};

const TEMPLATE: &str = "template";
/// The filter that shows every kind.
const ALL_KINDS: &str = "all";

/// A card's page, in pixels per millimetre: an A4 sheet 190 pixels wide.
const CARD_SCALE: f32 = 190.0 / 210.0;
/// The page in the side panel.
const PANEL_SCALE: f32 = 1.6;

/// What one load of the library brings back.
struct Loaded {
    items: Vec<TemplateSummary>,
    layouts: HashMap<String, Layout>,
    /// How many starter templates were installed on the way (an empty library gets them).
    seeded: usize,
}

pub struct TemplatesPage {
    editor: Option<Entity<TemplateEditor>>,
    items: Vec<TemplateSummary>,
    /// The page of each template, laid out with a sample document.
    thumbs: HashMap<String, (f32, f32, Vec<PageShape>)>,
    shop: Rc<RefCell<Shop>>,
    shop_loaded: bool,
    loading: bool,
    load_error: Option<String>,
    loads: u64,
    include_archived: bool,
    busy: bool,
    installing: bool,

    kind: SelectModel,
    search: TextModel,
    new_name: TextModel,
    new_kind: SelectModel,
    new_name_missing: bool,
    panels: Entity<PanelStack>,
    _subscriptions: Vec<Subscription>,
    _editor_subscription: Option<Subscription>,
}

fn plural(count: usize) -> String {
    if count == 1 { "1 modèle de base installé.".to_string() } else { format!("{count} modèles de base installés.") }
}

/// The line under the title: how many templates are shown, and through which filters.
fn count_label(shown: usize, kind: Option<TemplateKind>, include_archived: bool) -> String {
    let mut label = format!("{shown} modèle{}", if shown > 1 { "s" } else { "" });
    if let Some(kind) = kind {
        label.push_str(&format!(" · {}", kind.label()));
    }
    if include_archived {
        label.push_str(" · archivés inclus");
    }
    label
}

impl TemplatesPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kinds = |with_all: bool| {
            let all = with_all.then(|| SelectOption::new(ALL_KINDS, "Tous les types"));
            all.into_iter().chain(TemplateKind::ALL.into_iter().map(|kind| SelectOption::new(kind.id(), kind.label()))).collect::<Vec<_>>()
        };
        let kind = SelectModel::new(kinds(true), Some(ALL_KINDS), window, cx);
        let new_kind = SelectModel::new(kinds(false), Some(TemplateKind::Facture.id()), window, cx);
        let search = TextModel::builder().placeholder("Rechercher un modèle…").build(window, cx);
        let new_name = TextModel::builder().placeholder("Facture standard").build(window, cx);
        let panels = cx.new(PanelStack::new);
        let subscriptions = vec![
            kind.on_change(window, cx, |this: &mut Self, _, window, cx| this.refresh(window, cx)),
            search.on_event(window, cx, |_, event, _, cx| {
                if event == TextEvent::Changed {
                    cx.notify();
                }
            }),
            new_name.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                TextEvent::Submitted { .. } => this.start_draft(window, cx),
                TextEvent::Changed => {
                    this.new_name_missing = false;
                    cx.notify();
                }
                _ => {}
            }),
            cx.observe(&panels, |_, _, cx| cx.notify()),
        ];
        let mut page = Self {
            editor: None,
            items: Vec::new(),
            thumbs: HashMap::new(),
            shop: Rc::new(RefCell::new(Shop::default())),
            shop_loaded: false,
            loading: true,
            load_error: None,
            loads: 0,
            include_archived: false,
            busy: false,
            installing: false,
            kind,
            search,
            new_name,
            new_kind,
            new_name_missing: false,
            panels,
            _subscriptions: subscriptions,
            _editor_subscription: None,
        };
        page.refresh(window, cx);
        page
    }

    fn kind_filter(&self, cx: &App) -> Option<TemplateKind> {
        self.kind.value(cx).filter(|id| id != ALL_KINDS).map(|id| TemplateKind::from_id(&id))
    }

    // ---- loading -------------------------------------------------------------------------------

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loads += 1;
        let asked = self.loads;
        self.loading = true;
        self.load_error = None;
        let query = ListQuery { kind: self.kind_filter(cx), include_archived: self.include_archived };
        let with_shop = !self.shop_loaded;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                let mut items = templates::list(&client, &query).await?;
                let mut seeded = 0;
                // A library with nothing in it gets the starter templates, once.
                if items.is_empty() && query == ListQuery::default() {
                    seeded = starters::install_missing(&client).await?;
                    if seeded > 0 {
                        items = templates::list(&client, &query).await?;
                    }
                }
                let mut layouts = HashMap::new();
                for item in &items {
                    // A template that cannot be read keeps its card, without a page on it.
                    if let Ok(template) = templates::get(&client, &item.id).await {
                        layouts.insert(item.id.clone(), template.schema);
                    }
                }
                let shop = if with_shop { Some(Shop::load(&client).await) } else { None };
                Ok((Loaded { items, layouts, seeded }, shop))
            },
            move |this, answer: ApiResult<(Loaded, Option<Shop>)>, window, cx| {
                if this.loads != asked {
                    return;
                }
                this.loading = false;
                match answer {
                    Ok((loaded, shop)) => {
                        if let Some(shop) = shop {
                            *this.shop.borrow_mut() = shop;
                            this.shop_loaded = true;
                        }
                        if loaded.seeded > 0 {
                            toast::success(plural(loaded.seeded), window, cx);
                        }
                        this.show(loaded);
                    }
                    Err(error) => {
                        this.load_error = Some(humanize_error(&error, "Les modèles n'ont pas pu être chargés."));
                        this.items.clear();
                    }
                }
                cx.notify();
            },
        );
    }

    fn show(&mut self, loaded: Loaded) {
        let mut shop = self.shop.borrow_mut();
        let mut samples: HashMap<TemplateKind, serde_json::Value> = HashMap::new();
        self.thumbs = loaded
            .items
            .iter()
            .filter_map(|item| {
                let layout = loaded.layouts.get(&item.id)?;
                let sample = samples.entry(item.kind).or_insert_with(|| shop.sample(item.kind)).clone();
                shop.read_inline_pictures(layout, &sample);
                Some((item.id.clone(), (layout.page.width as f32, layout.page.height as f32, shop.page(layout, &sample, Mode::Print, 0))))
            })
            .collect();
        self.items = loaded.items;
    }

    fn shown(&self, cx: &App) -> Vec<TemplateSummary> {
        let query = self.search.text(cx).trim().to_lowercase();
        self.items.iter().filter(|item| query.is_empty() || item.name.to_lowercase().contains(&query)).cloned().collect()
    }

    fn item(&self, id: &str) -> Option<&TemplateSummary> {
        self.items.iter().find(|item| item.id == id)
    }

    // ---- the side panel ------------------------------------------------------------------------

    fn open_panel(&mut self, item: &TemplateSummary, cx: &mut Context<Self>) {
        let layer = PanelLayer::new(TEMPLATE, item.id.clone(), item.name.clone()).icon(IconName::LayoutTemplate).width(PanelWidth::Md);
        self.panels.update(cx, |stack, cx| {
            stack.close_all();
            stack.open(layer);
            cx.notify();
        });
    }

    fn close_panel(&mut self, cx: &mut Context<Self>) {
        self.panels.update(cx, |stack, cx| {
            stack.close_all();
            cx.notify();
        });
    }

    fn selected(&self, cx: &App) -> Option<String> {
        self.panels.read(cx).key_of(TEMPLATE).map(|key| key.to_string())
    }

    // ---- actions on a template -----------------------------------------------------------------

    /// Run one change on a template, say how it went, and read the library again.
    fn act<T: Send + 'static, Fut>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(motodesk_client::Client) -> Fut + Send + 'static,
        done: &'static str,
        failed: &'static str,
    ) where
        Fut: std::future::Future<Output = ApiResult<T>> + Send + 'static,
    {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        cx.api_in(window, work, move |this, answer, window, cx| {
            this.busy = false;
            match answer {
                Ok(_) => {
                    toast::success(done, window, cx);
                    this.refresh(window, cx);
                }
                Err(error) => toast::error(humanize_error(&error, failed), window, cx),
            }
            cx.notify();
        });
    }

    fn duplicate(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, move |client| async move { templates::duplicate(&client, &id).await }, "Modèle dupliqué.", "Duplication impossible.");
    }

    fn make_default(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.act(
            window,
            cx,
            move |client| async move { templates::set_default(&client, &id).await },
            "Modèle défini par défaut.",
            "Impossible de définir le modèle par défaut.",
        );
    }

    fn toggle_archive(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let archived = self.item(&id).is_some_and(|item| item.is_archived);
        self.act(
            window,
            cx,
            move |client| async move {
                let full = templates::get(&client, &id).await?;
                let mut input = full.as_input();
                // An archived template is nobody's default.
                input.is_default = Some(full.is_archived && full.is_default);
                input.is_archived = Some(!full.is_archived);
                templates::update(&client, &id, &input).await
            },
            if archived { "Modèle désarchivé." } else { "Modèle archivé." },
            "Modification impossible.",
        );
    }

    fn ask_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.item(&id).map(|item| item.name.clone()) else { return };
        let page = cx.entity();
        Confirm::new("Supprimer ce modèle ?", format!("Supprimer « {name} » ? Cette action est définitive."))
            .confirm_label("Supprimer")
            .danger()
            .open(window, cx, move |window, cx| {
                let id = id.clone();
                page.update(cx, |this, cx| {
                    if this.selected(cx).as_deref() == Some(id.as_str()) {
                        this.close_panel(cx);
                    }
                    this.act(window, cx, move |client| async move { templates::delete(&client, &id).await }, "Modèle supprimé.", "Suppression impossible.");
                });
            });
    }

    fn install_starters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.installing {
            return;
        }
        self.installing = true;
        cx.notify();
        cx.api_in(
            window,
            |client| async move { starters::install_missing(&client).await },
            |this, answer, window, cx| {
                this.installing = false;
                match answer {
                    Ok(0) => toast::success("Les modèles de base sont déjà installés.", window, cx),
                    Ok(count) => toast::success(plural(count), window, cx),
                    Err(error) => toast::error(humanize_error(&error, "Impossible d'installer les modèles de base."), window, cx),
                }
                this.refresh(window, cx);
            },
        );
    }

    // ---- the editor ----------------------------------------------------------------------------

    fn edit(&mut self, template: Template, window: &mut Window, cx: &mut Context<Self>) {
        self.close_panel(cx);
        let shop = self.shop.clone();
        let editor = cx.new(|cx| TemplateEditor::new(template, shop, window, cx));
        self._editor_subscription = Some(cx.subscribe_in(&editor, window, |this, _, event: &EditorEvent, window, cx| match event {
            EditorEvent::Back => {
                this.editor = None;
                this._editor_subscription = None;
                this.refresh(window, cx);
            }
        }));
        self.editor = Some(editor);
        cx.notify();
    }

    fn open_editor(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            move |client| async move { templates::get(&client, &id).await },
            |this, answer, window, cx| match answer {
                Ok(template) => this.edit(template, window, cx),
                Err(error) => toast::error(humanize_error(&error, "Impossible d'ouvrir le modèle."), window, cx),
            },
        );
    }

    fn ask_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_panel(cx);
        self.new_name.clear(window, cx);
        self.new_name_missing = false;
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Nouveau modèle").width(px(420.)).open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let this = for_body.read(cx);
                element(
                    col()
                        .gap(m.space_4)
                        .child(FormField::new("Nom du modèle").required().child(Input::new(&this.new_name).invalid(this.new_name_missing)))
                        .child(FormField::new("Type de document").required().child(Select::new(&this.new_kind))),
                )
            },
            move |_, _| {
                let page = for_footer.clone();
                vec![
                    element(Button::new("template-new-cancel").ghost().label("Annuler").on_click(|_, window, cx| close_modal(window, cx))),
                    element(
                        Button::new("template-new-create")
                            .primary()
                            .label("Créer et ouvrir")
                            .on_click(move |_, window, cx| page.update(cx, |this, cx| this.start_draft(window, cx))),
                    ),
                ]
            },
        );
        self.new_name.focus(window, cx);
    }

    /// A new template starts from the starter layout of its kind. It exists once it is saved.
    fn start_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.new_name.trimmed(cx) else {
            self.new_name_missing = true;
            cx.notify();
            return;
        };
        let kind = TemplateKind::from_id(&self.new_kind.value(cx).unwrap_or_default());
        close_modal(window, cx);
        let draft = Template {
            id: String::new(),
            name,
            kind,
            page_size: "A4".to_string(),
            orientation: Orientation::Portrait,
            schema: starters::starter_layout(kind),
            is_default: false,
            is_archived: false,
            created_at: String::new(),
            updated_at: String::new(),
        };
        self.edit(draft, window, cx);
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn page_of(&self, id: &str, scale: f32, key: &'static str) -> AnyElement {
        match self.thumbs.get(id) {
            Some((width, height, shapes)) => {
                PageView::new(ElementId::Name(format!("{key}-{id}").into()), *width, *height, scale).shapes(shapes.clone()).into_any_element()
            }
            None => div().w(px(210. * scale)).h(px(297. * scale)).child(StateView::empty().compact().icon(IconName::LayoutTemplate).title("Aucun aperçu")).into_any_element(),
        }
    }

    fn card(&self, index: usize, item: &TemplateSummary, selected: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let id = item.id.clone();
        let action = |name: &'static str, icon: IconName, title: &'static str, run: fn(&mut Self, String, &mut Window, &mut Context<Self>)| {
            let id = id.clone();
            IconButton::new((name, index), icon).title(title).size(IconButtonSize::Sm).on_click(cx.listener(move |this, _, window, cx| {
                // The card under the button would open its panel.
                cx.stop_propagation();
                run(this, id.clone(), window, cx)
            }))
        };
        let badge = if item.is_archived {
            Some(Tag::new("Archivé").status(Status::Muted).icon(IconName::Archive))
        } else if item.is_default {
            Some(Tag::new("Par défaut").status(Status::Success).icon(IconName::Check))
        } else {
            None
        };
        let opened = item.clone();
        col()
            .id(("template-card", index))
            .flex_none()
            .bg(c.bg_surface)
            .border_1()
            .border_color(if selected { c.accent } else { c.border })
            .cursor_pointer()
            .hover(|this| this.border_color(c.border_strong))
            .when(item.is_archived, |this| this.opacity(0.7))
            .on_click(cx.listener(move |this, _, _, cx| this.open_panel(&opened, cx)))
            .child(
                div()
                    .relative()
                    .p(m.space_2)
                    .bg(c.bg_app)
                    .child(self.page_of(&item.id, CARD_SCALE, "template-thumb"))
                    .when_some(badge, |this, badge| this.child(div().absolute().top(m.space_3).left(m.space_3).child(badge))),
            )
            .child(
                col()
                    .gap(m.space_1)
                    .px(m.space_3)
                    .py(m.space_2)
                    .border_t_1()
                    .border_color(c.border_subtle)
                    .child(div().w(px(190.)).one_line().child(Heading::new(item.name.clone()).size(HeadingSize::Md)))
                    .child(
                        row()
                            .gap(m.space_2)
                            .items_center()
                            .child(Tag::new(item.kind.label()).status(Status::Accent))
                            .child(div().fs(m.fs_xs).text_color(c.text_muted).child(format!("Modifié le {}", format::date(&item.updated_at)))),
                    ),
            )
            .child(
                row()
                    .gap(m.space_1)
                    .px(m.space_2)
                    .py(m.space_1)
                    .border_t_1()
                    .border_color(c.border_subtle)
                    .child(action("template-edit", IconName::Pencil, "Ouvrir dans l'éditeur", Self::open_editor))
                    .child(action("template-copy", IconName::Copy, "Dupliquer", Self::duplicate))
                    .when(!item.is_default && !item.is_archived, |this| {
                        this.child(action("template-default", IconName::Check, "Définir par défaut", Self::make_default))
                    })
                    .child(action("template-archive", IconName::Archive, if item.is_archived { "Désarchiver" } else { "Archiver" }, Self::toggle_archive))
                    .child(div().flex_1())
                    .child(action("template-delete", IconName::Trash, "Supprimer", Self::ask_delete).danger()),
            )
    }

    fn panel(&self, layer: &PanelLayer, cx: &mut Context<Self>) -> Panel {
        let m = cx.ui().m;
        let panel = Panel::new(&self.panels, layer.id.clone());
        let Some(item) = self.item(layer.key.as_ref()).cloned() else { return panel };
        let stack = self.panels.clone();
        let busy = self.busy;
        let act = |name: &'static str, icon: IconName, title: &'static str, run: fn(&mut Self, String, &mut Window, &mut Context<Self>)| {
            let id = item.id.clone();
            IconButton::new(name, icon).title(title).disabled(busy).on_click(cx.listener(move |this, _, window, cx| run(this, id.clone(), window, cx)))
        };
        let usage = if item.is_default {
            Tag::new(format!("Modèle par défaut pour {}", item.kind.label())).status(Status::Success).icon(IconName::Check)
        } else {
            Tag::new("Non utilisé par défaut").status(Status::Muted)
        };
        let orientation = item.orientation.label();
        let panel = match (item.is_archived, item.is_default) {
            (true, _) => panel.badge("Archivé", Status::Muted),
            (false, true) => panel.badge("Par défaut", Status::Success),
            _ => panel,
        };
        let id = item.id.clone();
        let for_default = item.id.clone();
        panel
            .action(act("template-panel-copy", IconName::Copy, "Dupliquer", Self::duplicate))
            .action(act("template-panel-archive", IconName::Archive, if item.is_archived { "Désarchiver" } else { "Archiver" }, Self::toggle_archive))
            .action(act("template-panel-delete", IconName::Trash, "Supprimer", Self::ask_delete).danger())
            .child(PanelSection::new("Aperçu").icon(IconName::Image).child(row().justify_center().child(self.page_of(&item.id, PANEL_SCALE, "template-preview"))))
            .child(PanelSection::new("Propriétés").icon(IconName::Info).child(InfoList::new(vec![
                InfoItem::new("Type", Some(item.kind.label())),
                InfoItem::new("Format", Some(format!("{} · {orientation}", if item.page_size.is_empty() { "A4" } else { item.page_size.as_str() }))),
                InfoItem::new("Par défaut", Some(if item.is_default { "Oui" } else { "Non" })),
                InfoItem::new("Statut", Some(if item.is_archived { "Archivé" } else { "Actif" })),
                InfoItem::new("Dernière modification", Some(format::date_time(&item.updated_at))),
            ])))
            .child(PanelSection::new("Utilisation").icon(IconName::FileText).child(
                row().gap(m.space_3).items_center().child(usage).when(!item.is_default && !item.is_archived, |this| {
                    this.child(
                        Button::new("template-panel-default")
                            .ghost()
                            .icon(IconName::Check)
                            .label("Définir par défaut")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| this.make_default(for_default.clone(), window, cx))),
                    )
                }),
            ))
            .footer(Button::new("template-panel-close").ghost().label("Fermer").on_click(move |_, window, cx| request_close(&stack, window, cx)))
            .footer(
                Button::new("template-panel-edit")
                    .primary()
                    .icon(IconName::Pencil)
                    .label("Ouvrir dans l'éditeur")
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| this.open_editor(id.clone(), window, cx))),
            )
    }

    fn body(&self, shown: &[TemplateSummary], cx: &mut Context<Self>) -> AnyElement {
        let m = cx.ui().m;
        if let Some(error) = self.load_error.clone() {
            return StateView::error()
                .title("Les modèles n'ont pas pu être chargés")
                .body(error)
                .action("Réessayer", cx.listener(|this, _, window, cx| this.refresh(window, cx)))
                .into_any_element();
        }
        if self.loading && self.items.is_empty() {
            return StateView::loading().title("Chargement des modèles…").into_any_element();
        }
        if shown.is_empty() {
            let searching = self.search.trimmed(cx).is_some();
            let filtered = self.kind_filter(cx).is_some();
            let empty = StateView::empty().icon(IconName::LayoutTemplate);
            return if searching {
                empty.title("Aucun résultat").body("Aucun modèle ne correspond à votre recherche.")
            } else if filtered {
                empty
                    .title("Aucun modèle de ce type")
                    .body("Créez un modèle de ce type, ou installez les modèles de base.")
                    .action("Créer un modèle", cx.listener(|this, _, window, cx| this.ask_new(window, cx)))
            } else {
                empty
                    .title("Aucun modèle pour le moment")
                    .body("Installez les modèles de base (facture, devis, bon de livraison, fiche client, fiche produit) pour imprimer tout de suite, ou partez d'une page déjà mise en page.")
                    .action("Installer les modèles de base", cx.listener(|this, _, window, cx| this.install_starters(window, cx)))
                    .secondary("Nouveau modèle", cx.listener(|this, _, window, cx| this.ask_new(window, cx)))
            }
            .into_any_element();
        }
        let selected = self.selected(cx);
        let cards: Vec<Stateful<Div>> =
            shown.iter().enumerate().map(|(index, item)| self.card(index, item, selected.as_deref() == Some(item.id.as_str()), cx)).collect();
        div()
            .id("templates-grid")
            .size_full()
            .overflow_y_scroll()
            .child(row().flex_wrap().items_start().gap(m.space_4).p(m.space_4).children(cards))
            .into_any_element()
    }
}

impl Render for TemplatesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        if let Some(editor) = self.editor.clone() {
            return div().size_full().child(editor).into_any_element();
        }
        let shown = self.shown(cx);
        let meta = if self.loading { "Chargement…".to_string() } else { count_label(shown.len(), self.kind_filter(cx), self.include_archived) };
        let layers: Vec<PanelLayer> = self.panels.read(cx).layers().to_vec();
        let panels: Vec<Panel> = layers.iter().map(|layer| self.panel(layer, cx)).collect();
        let archived = cx.entity();
        row()
            .relative()
            .size_full()
            .items_stretch()
            .bg(c.bg_app)
            .on_key_down(PanelHost::key_handler(&self.panels))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        PageToolbar::new("Modèles de documents")
                            .icon(IconName::LayoutTemplate)
                            .meta(meta)
                            .below(
                                row()
                                    .gap(cx.ui().m.space_4)
                                    .items_center()
                                    .child(div().w(px(200.)).child(Select::new(&self.kind)))
                                    .child(Switch::new("templates-archived", self.include_archived).label("Archivés").on_change(move |on, window, cx| {
                                        archived.update(cx, |this, cx| {
                                            this.include_archived = on;
                                            this.refresh(window, cx);
                                        })
                                    })),
                            )
                            .trailing(SearchField::new(&self.search))
                            .trailing(
                                Button::new("templates-starters")
                                    .ghost()
                                    .label("Modèles de base")
                                    .loading(self.installing)
                                    .on_click(cx.listener(|this, _, window, cx| this.install_starters(window, cx))),
                            )
                            .trailing(
                                Button::new("templates-new")
                                    .primary()
                                    .icon(IconName::Plus)
                                    .label("Nouveau modèle")
                                    .on_click(cx.listener(|this, _, window, cx| this.ask_new(window, cx))),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(self.body(&shown, cx))),
            )
            .child(PanelHost::new(&self.panels).children(panels))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::templates::TemplateKind;

    use super::{count_label, plural};

    #[test]
    fn the_toolbar_says_how_many_templates_and_through_which_filters() {
        assert_eq!(count_label(0, None, false), "0 modèle");
        assert_eq!(count_label(1, None, false), "1 modèle");
        assert_eq!(count_label(5, Some(TemplateKind::Devis), false), "5 modèles · Devis");
        assert_eq!(count_label(2, Some(TemplateKind::BonLivraison), true), "2 modèles · Bon de livraison · archivés inclus");
    }

    #[test]
    fn installed_starters_are_counted_in_words() {
        assert_eq!(plural(1), "1 modèle de base installé.");
        assert_eq!(plural(5), "5 modèles de base installés.");
    }
}
