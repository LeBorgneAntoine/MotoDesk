//! The template editor (port of `routes/templates/TemplateEditor.svelte` and its panels): a
//! page on which texts, pictures, tables and frames are placed, moved and resized; the
//! variables to put in them on the left, the layers and the properties of the selection on
//! the right.
//!
//! The page is drawn by the same layout that prints it ([`motodesk_client::templates::render`]),
//! in its editing mode: variables show their name instead of a value.

mod geometry;
mod props;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use motodesk_client::templates::render::{self, Mode};
use motodesk_client::templates::schema::{Align, Element, Kind, Layout, Weight};
use motodesk_client::templates::{self, pdf, variables, Orientation, Template, TemplateInput, TemplateKind};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;
use serde_json::Value;

use self::geometry::{Frame, Handle, History, ZOOM_FIT};
use super::sheet::Shop;
use crate::backend::{ApiExt as _, SaveExt as _};
use crate::format::humanize_error;

/// Pixels per millimetre at a zoom of 100 %.
const BASE_SCALE: f32 = 4.0;
/// How close, in pixels, a dragged element must come to a line to be pulled onto it.
const SNAP_PIXELS: f32 = 5.0;
/// The side of a resize handle, in pixels.
const HANDLE: f32 = 8.0;
/// The page of the preview, in pixels per millimetre.
const PREVIEW_SCALE: f32 = 2.6;
/// The variable groups shown open when the editor starts; the list is long.
const OPEN_GROUPS: [&str; 2] = ["Document", "Client"];

pub enum EditorEvent {
    /// The user is done: back to the library.
    Back,
}

/// An element being moved (no handle) or resized by one of its handles.
struct Drag {
    id: String,
    handle: Option<Handle>,
    from: Point<Pixels>,
    original: Frame,
    /// Whether the mouse has moved since the button went down: a click alone changes nothing.
    moved: bool,
}

#[derive(Clone, Copy)]
enum Tool {
    Text,
    Image,
    Table,
    Rect,
}

pub struct TemplateEditor {
    shop: Rc<RefCell<Shop>>,
    /// Empty until the template is saved for the first time.
    id: String,
    name: String,
    kind: TemplateKind,
    page_size: String,
    is_default: bool,
    is_archived: bool,
    layout: Layout,
    /// The layout as it was last saved (or opened): what tells unsaved work apart.
    saved_layout: Layout,
    sample: Value,
    selected: Option<String>,
    history: History,
    clipboard: Option<Element>,
    zoom: f32,
    drag: Option<Drag>,
    /// The lines the dragged element is aligned with, across and down, in millimetres.
    guides: (Option<f64>, Option<f64>),
    saving: bool,
    save_error: Option<String>,
    saved_at: Option<String>,
    folded: HashSet<&'static str>,
    /// Tells this run's new identifiers from those of another day.
    seed: u64,
    focus: FocusHandle,
    orientation: SelectModel,
    meta_name: TextModel,
    meta_kind: SelectModel,
    meta_name_missing: bool,
    props: props::Fields,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EditorEvent> for TemplateEditor {}

impl TemplateEditor {
    pub fn new(template: Template, shop: Rc<RefCell<Shop>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let landscape = template.schema.page.is_landscape();
        let orientation = SelectModel::new(
            Orientation::ALL.into_iter().map(|choice| SelectOption::new(choice.id(), choice.label())).collect(),
            Some(if landscape { Orientation::Landscape.id() } else { Orientation::Portrait.id() }),
            window,
            cx,
        );
        let meta_name = TextModel::builder().placeholder("Facture standard").build(window, cx);
        let meta_kind =
            SelectModel::new(TemplateKind::ALL.into_iter().map(|kind| SelectOption::new(kind.id(), kind.label())).collect(), Some(template.kind.id()), window, cx);
        let subscriptions = vec![
            orientation.on_change(window, cx, |this: &mut Self, chosen, window, cx| {
                let landscape = chosen.as_deref() == Some(Orientation::Landscape.id());
                if this.layout.page.is_landscape() != landscape {
                    this.record(None);
                    this.layout.set_landscape(landscape);
                    this.rebuild_props(window, cx);
                }
            }),
            meta_name.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                TextEvent::Submitted { .. } => this.submit_meta(window, cx),
                TextEvent::Changed => {
                    this.meta_name_missing = false;
                    cx.notify();
                }
                _ => {}
            }),
        ];
        let sample = {
            let mut shop = shop.borrow_mut();
            let sample = shop.sample(template.kind);
            shop.read_inline_pictures(&template.schema, &sample);
            sample
        };
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_millis() as u64).unwrap_or_default();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let props = props::Fields::build(None, window, cx);
        Self {
            shop,
            id: template.id,
            name: template.name,
            kind: template.kind,
            page_size: template.page_size,
            is_default: template.is_default,
            is_archived: template.is_archived,
            saved_layout: template.schema.clone(),
            layout: template.schema,
            sample,
            selected: None,
            history: History::default(),
            clipboard: None,
            zoom: ZOOM_FIT,
            drag: None,
            guides: (None, None),
            saving: false,
            save_error: None,
            saved_at: None,
            folded: variables::groups_for(TemplateKind::Facture).into_iter().map(|(group, _)| group).filter(|group| !OPEN_GROUPS.contains(group)).collect(),
            seed,
            focus,
            orientation,
            meta_name,
            meta_kind,
            meta_name_missing: false,
            props,
            _subscriptions: subscriptions,
        }
    }

    fn is_draft(&self) -> bool {
        self.id.is_empty()
    }

    fn scale(&self) -> f32 {
        BASE_SCALE * self.zoom
    }

    fn selected_element(&self) -> Option<&Element> {
        self.layout.element(self.selected.as_deref()?)
    }

    /// Keep the layout as it is now, to come back to. `about` names what is about to change.
    fn record(&mut self, about: Option<&str>) {
        self.history.record(&self.layout, about, Instant::now());
    }

    /// The layout changed from outside the property fields: they show the selection again.
    fn rebuild_props(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_ref().is_some_and(|id| self.layout.element(id).is_none()) {
            self.selected = None;
        }
        self.props = props::Fields::build(self.selected_element(), window, cx);
        cx.notify();
    }

    fn select(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected != id {
            self.selected = id;
            self.rebuild_props(window, cx);
        }
    }

    // ---- changing the layout -------------------------------------------------------------------

    fn add(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        let page = self.layout.page;
        // New elements land near the top left, each a little lower than the last one.
        let offset = (self.layout.elements.len() % 8) as f64 * 3.0;
        let mut element = match tool {
            Tool::Text => Element::new_text(0.0, 0.0, self.seed),
            Tool::Image => Element::new_image(0.0, 0.0, self.seed),
            Tool::Table => Element::new_table(0.0, 0.0, self.seed),
            Tool::Rect => Element::new_rect(0.0, 0.0, self.seed),
        };
        element.w = element.w.min(page.width - 2.0 * page.margin).max(5.0);
        let top = if matches!(tool, Tool::Table) { page.height / 2.0 } else { page.margin + offset };
        element.move_to(page.margin, top, &page);
        self.record(None);
        self.selected = Some(element.id.clone());
        self.layout.elements.push(element);
        self.rebuild_props(window, cx);
    }

    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.take() else { return };
        self.record(None);
        self.layout.elements.retain(|element| element.id != id);
        self.rebuild_props(window, cx);
    }

    fn paste(&mut self, source: Option<Element>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = source else { return };
        let copy = source.copy_moved(4.0, 4.0, &self.layout.page, self.seed);
        self.record(None);
        self.selected = Some(copy.id.clone());
        self.layout.elements.push(copy);
        self.rebuild_props(window, cx);
    }

    fn duplicate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.paste(self.selected_element().cloned(), window, cx);
    }

    fn restack(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        let before = self.layout.clone();
        if geometry::restack(&mut self.layout, &id, by) {
            self.history.record(&before, None, Instant::now());
            self.rebuild_props(window, cx);
        }
    }

    /// Change the selected element with `change`; `about` names what changes, so that repeated
    /// changes of the same thing (arrow keys held down) are one step back.
    fn edit_selected(&mut self, about: &str, window: &mut Window, cx: &mut Context<Self>, change: impl FnOnce(&mut Element, &Layout)) {
        let Some(id) = self.selected.clone() else { return };
        let before = self.layout.clone();
        let Some(element) = self.layout.element_mut(&id) else { return };
        change(element, &before);
        if self.layout != before {
            self.history.record(&before, Some(&format!("{about}:{id}")), Instant::now());
            self.rebuild_props(window, cx);
        }
    }

    fn nudge(&mut self, dx: f64, dy: f64, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_selected("move", window, cx, |element, before| element.move_to(element.x + dx, element.y + dy, &before.page));
    }

    fn set_align(&mut self, align: Align, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_selected("align", window, cx, |element, _| {
            if let Kind::Text(text) = &mut element.kind {
                text.align = align;
            }
        });
    }

    fn toggle_bold(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_selected("weight", window, cx, |element, _| {
            if let Kind::Text(text) = &mut element.kind {
                text.font_weight = if text.font_weight == Weight::Bold { Weight::Normal } else { Weight::Bold };
            }
        });
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(before) = self.history.undo(&self.layout) {
            self.layout = before;
            self.after_time_travel(window, cx);
        }
    }

    fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = self.history.redo(&self.layout, Instant::now()) {
            self.layout = next;
            self.after_time_travel(window, cx);
        }
    }

    fn after_time_travel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The orientation is part of the layout: the list above the page follows it.
        let orientation = if self.layout.page.is_landscape() { Orientation::Landscape } else { Orientation::Portrait };
        self.orientation.set_value(orientation.id(), window, cx);
        self.rebuild_props(window, cx);
    }

    fn zoom_by(&mut self, direction: i32, cx: &mut Context<Self>) {
        self.zoom = geometry::zoom_step(self.zoom, direction);
        cx.notify();
    }

    /// Put a variable in the selected text: where the caret is when its text is being typed,
    /// at its end otherwise. With no text selected the variable goes to the clipboard.
    fn insert_variable(&mut self, token: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.selected_element().map(|element| &element.kind), Some(Kind::Text(_))) {
            cx.write_to_clipboard(ClipboardItem::new_string(token.to_string()));
            toast::success("Variable copiée. Sélectionnez un texte pour l'y insérer directement.", window, cx);
            return;
        }
        if self.props.content.is_focused(window, cx) {
            self.props.content.insert(token, window, cx);
            self.props_changed("content", window, cx);
            return;
        }
        self.edit_selected("content", window, cx, |element, _| {
            if let Kind::Text(text) = &mut element.kind {
                if !text.content.is_empty() && !text.content.ends_with([' ', '\n']) {
                    text.content.push(' ');
                }
                text.content.push_str(token);
            }
        });
    }

    // ---- the mouse on the page -----------------------------------------------------------------

    fn grab(&mut self, id: String, handle: Option<Handle>, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        self.select(Some(id.clone()), window, cx);
        let Some(element) = self.layout.element(&id) else { return };
        // A double click on a text goes to its text, ready to be typed over.
        if event.click_count >= 2 && matches!(element.kind, Kind::Text(_)) {
            self.props.content.focus_selected(window, cx);
            return;
        }
        self.drag = Some(Drag { id, handle, from: event.position, original: Frame::of(element), moved: false });
    }

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let scale = f64::from(self.scale());
        let Some(drag) = self.drag.as_mut() else { return };
        let (dx, dy) = (f64::from(f32::from(position.x - drag.from.x)) / scale, f64::from(f32::from(position.y - drag.from.y)) / scale);
        if !drag.moved {
            // A hand is never perfectly still: a pixel or two is still a click.
            if dx.abs() * scale < 2.0 && dy.abs() * scale < 2.0 {
                return;
            }
            drag.moved = true;
            self.history.record(&self.layout, None, Instant::now());
        }
        let (id, handle, original) = (drag.id.clone(), drag.handle, drag.original);
        let page = self.layout.page;
        let frame = match handle {
            Some(handle) => {
                self.guides = (None, None);
                geometry::resized(original, handle, dx, dy, &page)
            }
            None => {
                let others: Vec<Frame> = self.layout.elements.iter().filter(|element| element.id != id).map(Frame::of).collect();
                let (frame, across, down) = geometry::snapped(geometry::moved(original, dx, dy, &page), &others, &page, f64::from(SNAP_PIXELS) / scale);
                self.guides = (across, down);
                // Pulled onto a line near the edge, it still does not leave the page.
                geometry::moved(frame, 0.0, 0.0, &page)
            }
        };
        if let Some(element) = self.layout.element_mut(&id) {
            frame.put_on(element);
        }
        cx.notify();
    }

    fn release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        self.guides = (None, None);
        if drag.moved {
            self.rebuild_props(window, cx);
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let (command, shift) = (event.keystroke.modifiers.secondary(cx), event.keystroke.modifiers.shift);
        // Saving and the preview work from anywhere, a field included.
        match (command, key) {
            (true, "s") => {
                cx.stop_propagation();
                return self.save(window, cx);
            }
            (true, "p") => {
                cx.stop_propagation();
                return self.preview(window, cx);
            }
            _ => {}
        }
        // Everything else is for the page: in a field, the keys belong to the text being typed.
        if !self.focus.is_focused(window) {
            return;
        }
        let step = if shift { 10.0 } else { 1.0 };
        match (command, key) {
            (true, "z") if shift => self.redo(window, cx),
            (true, "z") => self.undo(window, cx),
            (true, "y") => self.redo(window, cx),
            (true, "c") => self.clipboard = self.selected_element().cloned(),
            (true, "v") => self.paste(self.clipboard.clone(), window, cx),
            (true, "d") => self.duplicate_selected(window, cx),
            (true, "]") => self.restack(1, window, cx),
            (true, "[") => self.restack(-1, window, cx),
            (false, "delete" | "backspace") => self.delete_selected(window, cx),
            (false, "escape") => self.select(None, window, cx),
            (false, "left") => self.nudge(-step, 0.0, window, cx),
            (false, "right") => self.nudge(step, 0.0, window, cx),
            (false, "up") => self.nudge(0.0, -step, window, cx),
            (false, "down") => self.nudge(0.0, step, window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    // ---- saving --------------------------------------------------------------------------------

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        if self.name.trim().is_empty() {
            return self.ask_meta(window, cx);
        }
        self.saving = true;
        self.save_error = None;
        cx.notify();
        let orientation = if self.layout.page.is_landscape() { Orientation::Landscape } else { Orientation::Portrait };
        let draft = self.is_draft();
        let input = TemplateInput {
            name: self.name.trim().to_string(),
            kind: self.kind,
            page_size: Some(if self.page_size.is_empty() { "A4".to_string() } else { self.page_size.clone() }),
            orientation: Some(orientation),
            schema: self.layout.clone(),
            // A new template is nobody's default yet.
            is_default: (!draft).then_some(self.is_default),
            is_archived: (!draft).then_some(self.is_archived),
        };
        let (id, kept) = (self.id.clone(), self.layout.clone());
        cx.api_in(
            window,
            move |client| async move { if id.is_empty() { templates::create(&client, &input).await } else { templates::update(&client, &id, &input).await } },
            move |this, answer, _, cx| {
                this.saving = false;
                match answer {
                    Ok(saved) => {
                        this.id = saved.id;
                        this.saved_layout = kept;
                        this.saved_at = Some(chrono::Local::now().format("%H:%M").to_string());
                    }
                    Err(error) => this.save_error = Some(humanize_error(&error, "Enregistrement impossible.")),
                }
                cx.notify();
            },
        );
    }

    /// The name and the kind of the template, in a dialog. A new template is named before it
    /// is first saved.
    fn ask_meta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.meta_name.set_text(self.name.clone(), window, cx);
        self.meta_kind.set_value(self.kind.id(), window, cx);
        self.meta_name_missing = false;
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        let draft = self.is_draft();
        Modal::new(if draft { "Enregistrer le modèle" } else { "Propriétés du modèle" }).width(px(440.)).open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let this = for_body.read(cx);
                element(
                    col()
                        .gap(m.space_4)
                        .child(FormField::new("Nom du modèle").required().child(Input::new(&this.meta_name).invalid(this.meta_name_missing)))
                        .child(FormField::new("Type de document").required().child(Select::new(&this.meta_kind))),
                )
            },
            move |_, _| {
                let editor = for_footer.clone();
                vec![
                    element(Button::new("template-meta-cancel").ghost().label("Annuler").on_click(|_, window, cx| close_modal(window, cx))),
                    element(
                        Button::new("template-meta-save")
                            .primary()
                            .label("Enregistrer")
                            .on_click(move |_, window, cx| editor.update(cx, |this, cx| this.submit_meta(window, cx))),
                    ),
                ]
            },
        );
        self.meta_name.focus_selected(window, cx);
    }

    fn submit_meta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.meta_name.trimmed(cx) else {
            self.meta_name_missing = true;
            cx.notify();
            return;
        };
        let kind = TemplateKind::from_id(&self.meta_kind.value(cx).unwrap_or_default());
        close_modal(window, cx);
        if kind != self.kind {
            // Another kind of document has other sample content, and other variables.
            self.sample = self.shop.borrow().sample(kind);
        }
        (self.name, self.kind) = (name, kind);
        self.save(window, cx);
    }

    fn unsaved(&self) -> bool {
        self.is_draft() || self.layout != self.saved_layout
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.unsaved() {
            return cx.emit(EditorEvent::Back);
        }
        let editor = cx.entity();
        Confirm::new("Quitter sans enregistrer ?", "Les modifications de ce modèle n'ont pas été enregistrées. Elles seront perdues.")
            .confirm_label("Quitter")
            .danger()
            .open(window, cx, move |_, cx| editor.update(cx, |_, cx| cx.emit(EditorEvent::Back)));
    }

    // ---- the preview and the PDF ---------------------------------------------------------------

    /// The template filled with the sample document, as a PDF file.
    fn pdf(&self) -> Vec<u8> {
        let shop = self.shop.borrow();
        pdf::write(&render::document(&self.layout, &self.sample, &shop.pictures, None), &shop.pictures, self.title())
    }

    fn title(&self) -> &str {
        if self.name.trim().is_empty() { "Modèle" } else { self.name.trim() }
    }

    fn save_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = format!("{}.pdf", self.title());
        cx.save_file(window, &name, self.pdf(), |_, written, window, cx| match written {
            Ok(Some(path)) => toast::success(format!("PDF enregistré : {}", path.display()), window, cx),
            Ok(None) => {}
            Err(error) => toast::error(format!("Le PDF n'a pas pu être enregistré : {error}"), window, cx),
        });
    }

    /// Hand the PDF to the program this PC opens PDF files with: that is where it is printed from.
    fn open_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folder = std::env::temp_dir().join("motodesk");
        let path = folder.join(format!("{}.pdf", self.title().replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "-")));
        match std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&path, self.pdf())) {
            Ok(()) => cx.open_with_system(&path),
            Err(error) => toast::error(format!("Le PDF n'a pas pu être préparé : {error}"), window, cx),
        }
    }

    fn preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Aperçu").width(px(210. * PREVIEW_SCALE + 80.)).open(
            window,
            cx,
            move |_, cx| {
                let m = cx.ui().m;
                let this = for_body.read(cx);
                let shop = this.shop.borrow();
                let pages = render::document(&this.layout, &this.sample, &shop.pictures, None);
                let sheets = pages.iter().enumerate().map(|(index, page)| {
                    PageView::new(("template-preview-page", index), page.width as f32, page.height as f32, PREVIEW_SCALE).shapes(shop.shapes(page))
                });
                element(
                    div()
                        .id("template-preview")
                        .max_h(px(560.))
                        .overflow_y_scroll()
                        .child(col().items_center().gap(m.space_4).children(sheets.collect::<Vec<_>>())),
                )
            },
            move |_, _| {
                let (save, open) = (for_footer.clone(), for_footer.clone());
                vec![
                    element(Button::new("template-preview-close").ghost().label("Fermer").on_click(|_, window, cx| close_modal(window, cx))),
                    element(
                        Button::new("template-preview-save")
                            .icon(IconName::FileDown)
                            .label("Enregistrer en PDF")
                            .on_click(move |_, window, cx| save.update(cx, |this, cx| this.save_pdf(window, cx))),
                    ),
                    element(
                        Button::new("template-preview-print")
                            .primary()
                            .icon(IconName::Printer)
                            .label("Ouvrir pour imprimer")
                            .on_click(move |_, window, cx| open.update(cx, |this, cx| this.open_pdf(window, cx))),
                    ),
                ]
            },
        );
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn top_bar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let tool = |id: &'static str, icon: IconName, title: &'static str, tool: Tool| {
            IconButton::new(id, icon).title(title).on_click(cx.listener(move |this, _, window, cx| this.add(tool, window, cx)))
        };
        let separator = || div().w(px(1.)).h(m.control_h_sm).bg(c.border);
        let label = format!(
            "{} · {}{}",
            if self.name.trim().is_empty() { "Nouveau modèle" } else { self.name.trim() },
            self.kind.label(),
            if self.is_draft() { " · brouillon" } else { "" }
        );
        let status = match (&self.save_error, &self.saved_at) {
            (Some(error), _) => div().fs(m.fs_sm).text_color(c.danger).child(error.clone()),
            (None, Some(at)) if !self.unsaved() => div().fs(m.fs_sm).text_color(c.text_muted).child(format!("Enregistré à {at}")),
            (None, _) if self.unsaved() => div().fs(m.fs_sm).text_color(c.text_muted).child("Modifications non enregistrées"),
            _ => div(),
        };
        row()
            .flex_none()
            .h(m.toolbar_h)
            .px(m.space_3)
            .gap(m.space_2)
            .items_center()
            .bg(c.bg_toolbar)
            .border_b_1()
            .border_color(c.border)
            .child(IconButton::new("template-back", IconName::ArrowLeft).title("Retour à la liste").on_click(cx.listener(|this, _, window, cx| this.back(window, cx))))
            .child(separator())
            .child(tool("template-add-text", IconName::Type, "Ajouter un texte", Tool::Text))
            .child(tool("template-add-image", IconName::Image, "Ajouter une image", Tool::Image))
            .child(tool("template-add-table", IconName::Table, "Ajouter un tableau", Tool::Table))
            .child(tool("template-add-rect", IconName::Square, "Ajouter un cadre", Tool::Rect))
            .child(separator())
            .child(
                Button::new("template-meta")
                    .ghost()
                    .icon(IconName::Pencil)
                    .label(label)
                    .tooltip("Modifier le nom et le type du modèle")
                    .on_click(cx.listener(|this, _, window, cx| this.ask_meta(window, cx))),
            )
            .child(div().flex_1())
            .child(
                IconButton::new("template-undo", IconName::Undo)
                    .title("Annuler (Ctrl+Z)")
                    .disabled(!self.history.can_undo())
                    .on_click(cx.listener(|this, _, window, cx| this.undo(window, cx))),
            )
            .child(
                IconButton::new("template-redo", IconName::Redo)
                    .title("Rétablir (Ctrl+Maj+Z)")
                    .disabled(!self.history.can_redo())
                    .on_click(cx.listener(|this, _, window, cx| this.redo(window, cx))),
            )
            .child(status)
            .child(Button::new("template-preview-open").icon(IconName::Eye).label("Aperçu").on_click(cx.listener(|this, _, window, cx| this.preview(window, cx))))
            .child(
                Button::new("template-save")
                    .primary()
                    .icon(IconName::Save)
                    .label("Enregistrer")
                    .loading(self.saving)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
    }

    /// The variables, by group; a click puts one in the selected text.
    fn variables(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let can_insert = matches!(self.selected_element().map(|element| &element.kind), Some(Kind::Text(_)));
        let mut list = col();
        for (group_index, (group, members)) in variables::groups_for(self.kind).into_iter().enumerate() {
            let folded = self.folded.contains(group);
            list = list.child(
                row()
                    .id(("template-variable-group", group_index))
                    .gap(m.space_2)
                    .items_center()
                    .px(m.space_3)
                    .py(m.space_1)
                    .bg(c.bg_toolbar)
                    .border_b_1()
                    .border_color(c.border_subtle)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.folded.remove(group) {
                            this.folded.insert(group);
                        }
                        cx.notify();
                    }))
                    .child(Icon::new(if folded { IconName::ChevronRight } else { IconName::ChevronDown }).color(c.text_muted))
                    .child(div().flex_1().child(Heading::new(group).size(HeadingSize::Sm)))
                    .child(div().fs(m.fs_xs).text_color(c.text_subtle).child(members.len().to_string())),
            );
            if folded {
                continue;
            }
            for (index, variable) in members.into_iter().enumerate() {
                let token = variable.token;
                list = list.child(
                    ListRow::new(ElementId::Name(format!("template-variable-{group_index}-{index}").into()), variable.label)
                        .dense()
                        .icon(IconName::Braces)
                        .on_click(cx.listener(move |this, _, window, cx| this.insert_variable(token, window, cx))),
                );
            }
        }
        let lists = variables::lists_for(self.kind).into_iter().map(|list| {
            let fields: Vec<&str> = list.fields.iter().map(|field| field.label).collect();
            col()
                .gap(m.space_1)
                .px(m.space_3)
                .py(m.space_2)
                .border_b_1()
                .border_color(c.border_subtle)
                .child(div().fs(m.fs_sm).font_weight(FontWeight::SEMIBOLD).child(list.label))
                .child(div().fs(m.fs_xs).font_family(FONT_MONO).text_color(c.text_muted).child(list.path))
                .child(div().fs(m.fs_xs).text_color(c.text_muted).child(fields.join(" · ")))
        });
        col()
            .id("template-variables")
            .flex_none()
            .w(px(216.))
            .h_full()
            .overflow_y_scroll()
            .bg(c.bg_surface)
            .border_r_1()
            .border_color(c.border)
            .child(
                col()
                    .gap(m.space_1)
                    .p(m.space_3)
                    .child(Heading::new("Variables").size(HeadingSize::Xs).caps())
                    .child(div().fs(m.fs_xs).text_color(c.text_muted).child(if can_insert {
                        "Cliquez pour insérer dans le texte sélectionné."
                    } else {
                        "Sélectionnez un texte pour insérer, ou cliquez pour copier."
                    })),
            )
            .child(list)
            .child(div().px(m.space_3).py(m.space_2).bg(c.bg_toolbar).border_b_1().border_color(c.border_subtle).child(Heading::new("Listes").size(HeadingSize::Sm)))
            .children(lists)
            .child(div().p(m.space_3).fs(m.fs_xs).text_color(c.text_muted).child("Une liste remplit un tableau : ajoutez-en un, puis choisissez sa source à droite."))
    }

    /// What is above the page: its size and orientation, the actions on the selection, the zoom.
    fn canvas_bar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let page = self.layout.page;
        let mut bar = row()
            .flex_none()
            .h(m.toolbar_h)
            .px(m.space_3)
            .gap(m.space_2)
            .items_center()
            .bg(c.bg_surface)
            .border_b_1()
            .border_color(c.border)
            .child(div().fs(m.fs_sm).text_color(c.text_muted).child(format!("A4 · {} × {} mm", page.width, page.height)))
            .child(div().w(px(120.)).child(Select::new(&self.orientation)))
            .child(div().flex_1());
        if let Some(element) = self.selected_element() {
            let separator = || div().w(px(1.)).h(m.control_h_sm).bg(c.border);
            if let Kind::Text(text) = &element.kind {
                let align = |id: &'static str, icon: IconName, title: &'static str, align: Align| {
                    IconButton::new(id, icon).title(title).active(text.align == align).on_click(cx.listener(move |this, _, window, cx| this.set_align(align, window, cx)))
                };
                bar = bar
                    .child(align("template-align-left", IconName::TextAlignStart, "Aligner à gauche", Align::Left))
                    .child(align("template-align-center", IconName::TextAlignCenter, "Centrer", Align::Center))
                    .child(align("template-align-right", IconName::TextAlignEnd, "Aligner à droite", Align::Right))
                    .child(
                        IconButton::new("template-bold", IconName::Bold)
                            .title("Gras")
                            .active(text.font_weight == Weight::Bold)
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_bold(window, cx))),
                    )
                    .child(separator());
            }
            let stack = |id: &'static str, icon: IconName, title: &'static str, by: isize| {
                IconButton::new(id, icon).title(title).on_click(cx.listener(move |this, _, window, cx| this.restack(by, window, cx)))
            };
            bar = bar
                .child(stack("template-to-front", IconName::BringToFront, "Mettre au premier plan", isize::MAX))
                .child(stack("template-forward", IconName::ArrowUp, "Avancer (Ctrl+])", 1))
                .child(stack("template-backward", IconName::ArrowDown, "Reculer (Ctrl+[)", -1))
                .child(stack("template-to-back", IconName::SendToBack, "Mettre à l'arrière-plan", isize::MIN + 1))
                .child(separator())
                .child(
                    IconButton::new("template-duplicate", IconName::Copy)
                        .title("Dupliquer (Ctrl+D)")
                        .on_click(cx.listener(|this, _, window, cx| this.duplicate_selected(window, cx))),
                )
                .child(
                    IconButton::new("template-delete", IconName::Trash)
                        .title("Supprimer")
                        .danger()
                        .on_click(cx.listener(|this, _, window, cx| this.delete_selected(window, cx))),
                )
                .child(separator());
        }
        bar.child(IconButton::new("template-zoom-out", IconName::Minus).title("Zoom arrière").on_click(cx.listener(|this, _, _, cx| this.zoom_by(-1, cx))))
            .child(
                Button::new("template-zoom-fit")
                    .ghost()
                    .small()
                    .label(format!("{} %", (self.zoom * 100.0).round()))
                    .tooltip("Revenir au zoom de départ")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = ZOOM_FIT;
                        cx.notify();
                    })),
            )
            .child(IconButton::new("template-zoom-in", IconName::Plus).title("Zoom avant").on_click(cx.listener(|this, _, _, cx| this.zoom_by(1, cx))))
    }

    /// The page, with what makes it editable laid over it: a hit area on each element, the
    /// frame and handles of the selection, and the lines a dragged element aligns with.
    fn stage(&self, cx: &mut Context<Self>) -> PageView {
        let c = cx.ui().colors;
        let scale = self.scale();
        let page = self.layout.page;
        let shapes = self.shop.borrow().page(&self.layout, &self.sample, Mode::Edit, 0);
        let at = |mm: f64| px(mm as f32 * scale);
        let mut view = PageView::new("template-stage", page.width as f32, page.height as f32, scale).grid(5.0).shapes(shapes).overlay(
            // A click on bare paper selects nothing.
            div().absolute().top_0().left_0().size_full().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.focus.focus(window, cx);
                    this.select(None, window, cx);
                }),
            ),
        );
        for (index, element) in self.layout.elements.iter().enumerate() {
            let id = element.id.clone();
            view = view.overlay(
                div()
                    .id(("template-element", index))
                    .absolute()
                    .left(at(element.x))
                    .top(at(element.y))
                    .w(at(element.w))
                    .h(at(element.h))
                    .cursor_move()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            // The paper underneath would take the selection away again.
                            cx.stop_propagation();
                            this.grab(id.clone(), None, event, window, cx);
                        }),
                    ),
            );
        }
        if let Some(element) = self.selected_element() {
            // The selection is grabbed first, whatever lies over it: an element picked in the
            // list of layers can be moved even from under a larger one.
            let id = element.id.clone();
            view = view.overlay(
                div()
                    .id("template-selection")
                    .absolute()
                    .left(at(element.x))
                    .top(at(element.y))
                    .w(at(element.w))
                    .h(at(element.h))
                    .border_1()
                    .border_color(c.accent)
                    .cursor_move()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.grab(id.clone(), None, event, window, cx);
                        }),
                    ),
            );
            for (index, handle) in Handle::ALL.into_iter().enumerate() {
                let (across, down) = handle.anchor();
                let id = element.id.clone();
                view = view.overlay(
                    div()
                        .id(("template-handle", index))
                        .absolute()
                        .left(at(element.x + element.w * across) - px(HANDLE / 2.0))
                        .top(at(element.y + element.h * down) - px(HANDLE / 2.0))
                        .size(px(HANDLE))
                        .bg(c.bg_surface)
                        .border_1()
                        .border_color(c.accent)
                        .map(|this| match (handle.horizontal != 0, handle.vertical != 0) {
                            (true, false) => this.cursor_ew_resize(),
                            (false, true) => this.cursor_ns_resize(),
                            _ if handle.horizontal == handle.vertical => this.cursor_nwse_resize(),
                            _ => this.cursor_nesw_resize(),
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.grab(id.clone(), Some(handle), event, window, cx);
                            }),
                        ),
                );
            }
        }
        if let Some(across) = self.guides.0 {
            view = view.overlay(div().absolute().left(at(across)).top_0().bottom_0().w(px(1.)).bg(c.accent));
        }
        if let Some(down) = self.guides.1 {
            view = view.overlay(div().absolute().top(at(down)).left_0().right_0().h(px(1.)).bg(c.accent));
        }
        view
    }

    /// The elements, the one in front first.
    fn layers(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let rows = self.layout.elements.iter().enumerate().rev().map(|(index, element)| {
            let icon = match element.kind {
                Kind::Text(_) => IconName::Type,
                Kind::Image(_) => IconName::Image,
                Kind::Table(_) => IconName::Table,
                Kind::Rect(_) => IconName::Square,
            };
            let id = element.id.clone();
            ListRow::new(("template-layer", index), element.title())
                .dense()
                .icon(icon)
                .selected(self.selected.as_deref() == Some(element.id.as_str()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus.focus(window, cx);
                    this.select(Some(id.clone()), window, cx);
                }))
        });
        col()
            .flex_none()
            .child(
                row()
                    .gap(m.space_2)
                    .items_center()
                    .px(m.space_3)
                    .py(m.space_2)
                    .bg(c.bg_toolbar)
                    .border_b_1()
                    .border_color(c.border_subtle)
                    .child(Icon::new(IconName::Layers).color(c.text_muted))
                    .child(div().flex_1().child(Heading::new("Calques").size(HeadingSize::Sm)))
                    .child(div().fs(m.fs_xs).text_color(c.text_subtle).child(self.layout.elements.len().to_string())),
            )
            .child(if self.layout.elements.is_empty() {
                div().h(px(120.)).child(StateView::empty().compact().icon(IconName::Layers).title("Page vide").body("Ajoutez un texte, une image, un tableau ou un cadre.")).into_any_element()
            } else {
                div().id("template-layers").max_h(px(220.)).overflow_y_scroll().child(col().children(rows.collect::<Vec<_>>())).into_any_element()
            })
    }
}

impl Focusable for TemplateEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TemplateEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let page = self.layout.page;
        let padding = 16.0;
        let (wide, tall) = (page.width as f32 * self.scale() + 2.0 * padding, page.height as f32 * self.scale() + 2.0 * padding);
        col()
            .id("template-editor")
            .size_full()
            .bg(c.bg_app)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            // The mouse is followed over the whole editor: a drag does not stop at the page's edge.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if this.drag.is_none() {
                    return;
                }
                if event.pressed_button == Some(MouseButton::Left) { this.drag_to(event.position, cx) } else { this.release(window, cx) }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, window, cx| this.release(window, cx)))
            .child(self.top_bar(cx))
            .child(
                row()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.variables(cx))
                    .child(
                        col().flex_1().min_w_0().child(self.canvas_bar(cx)).child(
                            div()
                                .id("template-canvas")
                                .flex_1()
                                .min_h_0()
                                .overflow_scroll()
                                .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                                    if !event.modifiers.secondary(cx) {
                                        return;
                                    }
                                    let turned = match event.delta {
                                        ScrollDelta::Pixels(by) => f32::from(by.y),
                                        ScrollDelta::Lines(by) => by.y,
                                    };
                                    if turned != 0.0 {
                                        // The page zooms; it must not scroll as well.
                                        cx.stop_propagation();
                                        this.zoom_by(if turned > 0.0 { 1 } else { -1 }, cx);
                                    }
                                }))
                                .child(div().flex().items_start().justify_center().min_w(px(wide)).min_h(px(tall)).p(px(padding)).child(self.stage(cx))),
                        ),
                    )
                    .child(
                        col()
                            .flex_none()
                            .w(px(288.))
                            .bg(c.bg_surface)
                            .border_l_1()
                            .border_color(c.border)
                            .child(self.layers(cx))
                            // One scroll position per selection: a long table does not leave the next
                            // element scrolled half way down.
                            .child(div().id(ElementId::Name(format!("template-properties-{}", self.selected.as_deref().unwrap_or_default()).into())).flex_1().min_h_0().overflow_y_scroll().border_t_1().border_color(c.border).p(m.space_3).child(self.properties(cx))),
                    ),
            )
    }
}
