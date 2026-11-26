//! The properties of the selected element (port of `routes/templates/PropertiesPanel.svelte`):
//! where it is and how large, then what is its own (the text and its style, the source of a
//! picture, the list and the columns of a table, the border and fill of a frame).
//!
//! The fields are built from the element each time the selection changes, or the element is
//! changed from elsewhere (dragged, undone). What is typed goes to the element at once; a
//! value that cannot be read (a letter where a number goes) leaves the element as it was.

use std::time::Instant;

use motodesk_client::templates::render::parse_color;
use motodesk_client::templates::schema::{Align, Element, Fit, Format, Kind, TableColumn, Weight, MIN_HEIGHT_MM, MIN_WIDTH_MM};
use motodesk_client::templates::variables;
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::geometry::tenth;
use super::TemplateEditor;

/// The choice « a field of my own » in the list of a column's fields.
const CUSTOM: &str = "__custom__";
const TRANSPARENT: &str = "transparent";
/// Inks and fills offered at a click.
const SWATCHES: [u32; 6] = [0x1a1a1a, 0x5c6166, 0x1e3a5f, 0x1976d2, 0xd92d20, 0xffffff];

pub(super) struct ColumnFields {
    field: SelectModel,
    binding: TextModel,
    header: TextModel,
    width: TextModel,
    format: SelectModel,
    align: SelectModel,
}

pub(super) struct Fields {
    x: TextModel,
    y: TextModel,
    w: TextModel,
    h: TextModel,
    pub(super) content: TextModel,
    size: TextModel,
    weight: SelectModel,
    align: SelectModel,
    color: TextModel,
    src: TextModel,
    fit: SelectModel,
    list: SelectModel,
    row_height: TextModel,
    header_height: TextModel,
    table_size: TextModel,
    columns: Vec<ColumnFields>,
    stroke: TextModel,
    stroke_width: TextModel,
    fill: TextModel,
    _subscriptions: Vec<Subscription>,
}

/// A number as a field shows it: a comma, and no more decimals than it has.
fn shown(value: f64) -> String {
    let text = format!("{:.2}", value);
    text.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

/// A number as it is typed: with a comma or a point. `None` for anything else.
fn typed(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse::<f64>().ok().filter(|value| value.is_finite())
}

fn align_options() -> Vec<SelectOption> {
    Align::ALL.into_iter().map(|align| SelectOption::new(align.id(), align.label())).collect()
}

fn column_align(column: &TableColumn) -> Align {
    column.align.unwrap_or_default()
}

/// The columns a table gets from its list: one per field, sharing the width, numbers to the right.
fn automatic_columns(list: &str) -> Vec<TableColumn> {
    let Some(list) = variables::list(list) else { return Vec::new() };
    let share = tenth(100.0 / list.fields.len().max(1) as f64);
    list.fields
        .iter()
        .map(|field| TableColumn {
            header: field.label.to_string(),
            binding: field.binding.to_string(),
            width_pct: share,
            format: Some(field.format),
            align: Some(if field.format == Format::Text { Align::Left } else { Align::Right }),
        })
        .collect()
}

impl Fields {
    pub(super) fn build(element: Option<&Element>, window: &mut Window, cx: &mut Context<TemplateEditor>) -> Fields {
        // The fields' own subscriptions are kept apart from the lists': both are being added to
        // while the columns are built.
        let (mut subscriptions, mut typing) = (Vec::new(), Vec::new());
        // A field that tells the editor, at each keystroke, that the element changed.
        let mut text = |value: String, placeholder: &'static str, about: &'static str, lines: Option<usize>, window: &mut Window, cx: &mut Context<TemplateEditor>| {
            let builder = TextModel::builder().placeholder(placeholder).value(value);
            let model = match lines {
                Some(lines) => builder.multiline(lines),
                None => builder,
            }
            .build(window, cx);
            typing.push(model.on_event(window, cx, move |this: &mut TemplateEditor, event, window, cx| {
                if event == TextEvent::Changed {
                    this.props_changed(about, window, cx);
                }
            }));
            model
        };
        let number = |value: Option<f64>| value.map(shown).unwrap_or_default();
        let (text_el, image_el, table_el, rect_el) = match element.map(|element| &element.kind) {
            Some(Kind::Text(text)) => (Some(text), None, None, None),
            Some(Kind::Image(image)) => (None, Some(image), None, None),
            Some(Kind::Table(table)) => (None, None, Some(table), None),
            Some(Kind::Rect(rect)) => (None, None, None, Some(rect)),
            None => (None, None, None, None),
        };

        let x = text(number(element.map(|e| e.x)), "0", "x", None, window, cx);
        let y = text(number(element.map(|e| e.y)), "0", "y", None, window, cx);
        let w = text(number(element.map(|e| e.w)), "0", "w", None, window, cx);
        let h = text(number(element.map(|e| e.h)), "0", "h", None, window, cx);
        let content = text(text_el.map(|t| t.content.clone()).unwrap_or_default(), "Texte ou variable", "content", Some(4), window, cx);
        let size = text(number(text_el.map(|t| t.font_size)), "11", "size", None, window, cx);
        let color = text(text_el.map(|t| t.color.clone()).unwrap_or_default(), "#1a1a1a", "color", None, window, cx);
        let src = text(image_el.map(|i| i.src.clone()).unwrap_or_default(), "{{shop.logo_url}}", "src", None, window, cx);
        let row_height = text(number(table_el.map(|t| t.row_height)), "7", "row", None, window, cx);
        let header_height = text(number(table_el.map(|t| t.header_height)), "8", "header", None, window, cx);
        let table_size = text(number(table_el.map(|t| t.font_size)), "10", "tablesize", None, window, cx);
        let stroke = text(rect_el.map(|r| r.stroke.clone()).unwrap_or_default(), "#1a1a1a", "stroke", None, window, cx);
        let stroke_width = text(number(rect_el.map(|r| r.stroke_width)), "0,3", "strokewidth", None, window, cx);
        let fill = text(rect_el.map(|r| r.fill.clone()).unwrap_or_default(), TRANSPARENT, "fill", None, window, cx);

        let mut columns = Vec::new();
        let list_path = table_el.map(|t| t.list_binding.clone()).unwrap_or_default();
        let known = variables::list(&list_path).map(|list| list.fields).unwrap_or_default();
        for (index, column) in table_el.map(|t| t.columns.as_slice()).unwrap_or_default().iter().enumerate() {
            let is_known = known.iter().any(|field| field.binding == column.binding);
            let options = known.iter().map(|field| SelectOption::new(field.binding, field.label)).chain([SelectOption::new(CUSTOM, "Champ libre…")]).collect();
            let field = SelectModel::new(options, Some(if is_known { column.binding.as_str() } else { CUSTOM }), window, cx);
            subscriptions.push(field.on_change(window, cx, move |this: &mut TemplateEditor, chosen, window, cx| this.column_field_chosen(index, chosen, window, cx)));
            let format = SelectModel::new(
                Format::CHOICES.into_iter().map(|format| SelectOption::new(format.id(), format.label())).collect(),
                Some(column.format.unwrap_or_default().id()),
                window,
                cx,
            );
            let align = SelectModel::new(align_options(), Some(column_align(column).id()), window, cx);
            for select in [&format, &align] {
                subscriptions.push(select.on_change(window, cx, |this: &mut TemplateEditor, _, window, cx| this.props_changed("column", window, cx)));
            }
            columns.push(ColumnFields {
                field,
                binding: text(column.binding.clone(), "champ", "column", None, window, cx),
                header: text(column.header.clone(), "En-tête", "column", None, window, cx),
                width: text(shown(column.width_pct), "20", "column", None, window, cx),
                format,
                align,
            });
        }

        let weight = SelectModel::new(
            vec![SelectOption::new("normal", "Normal"), SelectOption::new("bold", "Gras")],
            Some(if text_el.is_some_and(|t| t.font_weight == Weight::Bold) { "bold" } else { "normal" }),
            window,
            cx,
        );
        let align = SelectModel::new(align_options(), Some(text_el.map(|t| t.align).unwrap_or_default().id()), window, cx);
        let fit = SelectModel::new(
            vec![SelectOption::new("contain", "Contenir (sans déformer)"), SelectOption::new("fill", "Étirer")],
            Some(if image_el.is_some_and(|i| i.fit == Fit::Fill) { "fill" } else { "contain" }),
            window,
            cx,
        );
        for (select, about) in [(&weight, "weight"), (&align, "align"), (&fit, "fit")] {
            subscriptions.push(select.on_change(window, cx, move |this: &mut TemplateEditor, _, window, cx| this.props_changed(about, window, cx)));
        }
        let lists = variables::LISTS.iter().map(|list| SelectOption::new(list.path, list.label)).collect();
        let list = SelectModel::new(lists, Some(list_path.as_str()).filter(|path| variables::list(path).is_some()), window, cx);
        subscriptions.push(list.on_change(window, cx, |this: &mut TemplateEditor, _, window, cx| {
            this.props_changed("list", window, cx);
            // Another list has other fields to offer in each column.
            this.rebuild_props(window, cx);
        }));

        subscriptions.extend(typing);
        Fields {
            x,
            y,
            w,
            h,
            content,
            size,
            weight,
            align,
            color,
            src,
            fit,
            list,
            row_height,
            header_height,
            table_size,
            columns,
            stroke,
            stroke_width,
            fill,
            _subscriptions: subscriptions,
        }
    }

    /// `current`, with what the fields say. A field that cannot be read keeps its old value.
    fn read(&self, current: &Element, cx: &App) -> Element {
        let number = |model: &TextModel| typed(&model.text(cx));
        let colour = |model: &TextModel, old: &str| {
            let text = model.text(cx).trim().to_string();
            if parse_color(&text).is_some() { text } else { old.to_string() }
        };
        let mut element = current.clone();
        element.x = number(&self.x).map_or(current.x, |x| tenth(x.max(0.0)));
        element.y = number(&self.y).map_or(current.y, |y| tenth(y.max(0.0)));
        element.w = number(&self.w).map_or(current.w, |w| tenth(w.max(MIN_WIDTH_MM)));
        element.h = number(&self.h).map_or(current.h, |h| tenth(h.max(MIN_HEIGHT_MM)));
        match &mut element.kind {
            Kind::Text(text) => {
                text.content = self.content.text(cx);
                text.font_size = number(&self.size).map_or(text.font_size, |size| size.clamp(4.0, 200.0));
                text.font_weight = if self.weight.value(cx).as_deref() == Some("bold") { Weight::Bold } else { Weight::Normal };
                text.align = Align::from_id(&self.align.value(cx).unwrap_or_default());
                text.color = colour(&self.color, &text.color);
            }
            Kind::Image(image) => {
                image.src = self.src.text(cx).trim().to_string();
                image.fit = if self.fit.value(cx).as_deref() == Some("fill") { Fit::Fill } else { Fit::Contain };
            }
            Kind::Table(table) => {
                if let Some(list) = self.list.value(cx) {
                    table.list_binding = list;
                }
                table.row_height = number(&self.row_height).map_or(table.row_height, |height| height.clamp(2.0, 100.0));
                table.header_height = number(&self.header_height).map_or(table.header_height, |height| height.clamp(2.0, 100.0));
                table.font_size = number(&self.table_size).map_or(table.font_size, |size| size.clamp(4.0, 72.0));
                for (column, fields) in table.columns.iter_mut().zip(&self.columns) {
                    let chosen = fields.field.value(cx).unwrap_or_default();
                    column.binding = if chosen == CUSTOM { fields.binding.text(cx).trim().to_string() } else { chosen };
                    column.header = fields.header.text(cx);
                    column.width_pct = number(&fields.width).map_or(column.width_pct, |width| width.clamp(0.0, 100.0));
                    column.format = Some(Format::from_id(&fields.format.value(cx).unwrap_or_default()));
                    column.align = Some(Align::from_id(&fields.align.value(cx).unwrap_or_default()));
                }
            }
            Kind::Rect(rect) => {
                rect.stroke = colour(&self.stroke, &rect.stroke);
                rect.stroke_width = number(&self.stroke_width).map_or(rect.stroke_width, |width| width.clamp(0.0, 20.0));
                let fill = self.fill.text(cx).trim().to_lowercase();
                rect.fill = if fill.is_empty() || fill == TRANSPARENT {
                    TRANSPARENT.to_string()
                } else if parse_color(&fill).is_some() {
                    fill
                } else {
                    rect.fill.clone()
                };
            }
        }
        element
    }
}

impl TemplateEditor {
    /// A property field changed: the element takes what the fields say. `about` names the
    /// field, so that typing in it is one step back and not one per keystroke.
    pub(super) fn props_changed(&mut self, about: &'static str, _: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.selected_element().cloned() else { return };
        let updated = self.props.read(&current, cx);
        if updated == current {
            return;
        }
        self.history.record(&self.layout, Some(&format!("{about}:{}", current.id)), Instant::now());
        if let Some(slot) = self.layout.element_mut(&current.id) {
            *slot = updated;
        }
        cx.notify();
    }

    /// Change the columns of the selected table, then show them again.
    fn edit_columns(&mut self, window: &mut Window, cx: &mut Context<Self>, change: impl FnOnce(&mut Vec<TableColumn>, &str)) {
        self.edit_selected("columns", window, cx, |element, _| {
            if let Kind::Table(table) = &mut element.kind {
                let list = table.list_binding.clone();
                change(&mut table.columns, &list);
            }
        });
        self.rebuild_props(window, cx);
    }

    /// A column is given a field of the list: it takes its name and its way of writing values.
    fn column_field_chosen(&mut self, index: usize, chosen: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chosen) = chosen else { return };
        self.edit_columns(window, cx, |columns, list| {
            let Some(column) = columns.get_mut(index) else { return };
            match variables::list(list).and_then(|list| list.fields.iter().find(|field| field.binding == chosen)) {
                Some(field) => {
                    column.binding = field.binding.to_string();
                    column.header = field.label.to_string();
                    column.format = Some(field.format);
                    column.align = Some(if field.format == Format::Text { Align::Left } else { Align::Right });
                }
                // « A field of my own »: the binding is cleared, to be typed.
                None => column.binding = String::new(),
            }
        });
    }

    fn number_pair(&self, first: (&'static str, &TextModel), second: (&'static str, &TextModel), cx: &App) -> Div {
        let m = cx.ui().m;
        let field = |(label, model): (&'static str, &TextModel)| div().flex_1().min_w_0().child(FormField::new(label).child(Input::new(model)));
        row().gap(m.space_2).child(field(first)).child(field(second))
    }

    pub(super) fn properties(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let Some(element) = self.selected_element() else {
            return div()
                .h(px(160.))
                .child(StateView::empty().compact().icon(IconName::MousePointer2).title("Aucune sélection").body("Cliquez un élément de la page ou un calque."))
                .into_any_element();
        };
        let fields = &self.props;
        let editor = cx.entity();
        let picked = move |about: &'static str| {
            let editor = editor.clone();
            move |_: &str, window: &mut Window, cx: &mut App| editor.update(cx, |this, cx| this.props_changed(about, window, cx))
        };
        let section = |title: &'static str| div().pt(m.space_2).child(Heading::new(title).size(HeadingSize::Xs).caps());

        let mut panel = col()
            .gap(m.space_3)
            .child(Heading::new(element.kind.label()).size(HeadingSize::Md))
            .child(section("Position et taille (mm)"))
            .child(self.number_pair(("X", &fields.x), ("Y", &fields.y), cx))
            .child(self.number_pair(("L", &fields.w), ("H", &fields.h), cx));

        match &element.kind {
            Kind::Text(_) => {
                panel = panel
                    .child(section("Texte"))
                    .child(FormField::new("Contenu").hint("Cliquez une variable à gauche pour l'insérer au curseur.").child(Textarea::new(&fields.content)))
                    .child(
                        row()
                            .gap(m.space_2)
                            .child(div().flex_1().min_w_0().child(FormField::new("Taille (pt)").child(Input::new(&fields.size))))
                            .child(div().flex_1().min_w_0().child(FormField::new("Graisse").child(Select::new(&fields.weight)))),
                    )
                    .child(FormField::new("Alignement").child(Select::new(&fields.align)))
                    .child(FormField::new("Couleur").child(ColorField::new("template-text-colour", &fields.color).presets(&SWATCHES).on_pick(picked("color"))));
            }
            Kind::Image(_) => {
                panel = panel
                    .child(section("Image"))
                    .child(
                        FormField::new("Source")
                            .hint("Une variable (le logo : {{shop.logo_url}}) ou une image écrite dans son adresse (data:…). Une adresse web n'est pas chargée.")
                            .child(Input::new(&fields.src).mono()),
                    )
                    .child(FormField::new("Ajustement").child(Select::new(&fields.fit)));
            }
            Kind::Table(table) => {
                let auto = cx.listener(|this, _, window, cx| this.edit_columns(window, cx, |columns, list| *columns = automatic_columns(list)));
                let add = cx.listener(|this, _, window, cx| {
                    this.edit_columns(window, cx, |columns, _| {
                        columns.push(TableColumn { header: "Colonne".to_string(), binding: String::new(), width_pct: 15.0, format: Some(Format::Text), align: Some(Align::Left) })
                    })
                });
                panel = panel
                    .child(section("Tableau"))
                    .child(FormField::new("Source de la liste").hint("Les lignes du tableau viennent de cette liste.").child(Select::new(&fields.list)))
                    .child(self.number_pair(("Hauteur ligne (mm)", &fields.row_height), ("Hauteur en-tête (mm)", &fields.header_height), cx))
                    .child(FormField::new("Taille texte (pt)").child(Input::new(&fields.table_size)))
                    .child(
                        row()
                            .gap(m.space_2)
                            .items_center()
                            .pt(m.space_2)
                            .child(div().flex_1().child(Heading::new(format!("Colonnes ({})", table.columns.len())).size(HeadingSize::Xs).caps()))
                            .child(Button::new("template-columns-auto").ghost().small().icon(IconName::Wand).label("Auto").tooltip("Une colonne par champ de la liste").on_click(auto))
                            .child(Button::new("template-columns-add").ghost().small().icon(IconName::Plus).label("Ajouter").on_click(add)),
                    );
                let last = fields.columns.len().saturating_sub(1);
                for (index, column) in fields.columns.iter().enumerate() {
                    let shift = |by: isize| {
                        cx.listener(move |this: &mut Self, _: &ClickEvent, window: &mut Window, cx| {
                            this.edit_columns(window, cx, |columns, _| {
                                let to = index as isize + by;
                                if to >= 0 && (to as usize) < columns.len() {
                                    columns.swap(index, to as usize);
                                }
                            })
                        })
                    };
                    let remove = cx.listener(move |this, _, window, cx| {
                        this.edit_columns(window, cx, |columns, _| {
                            if index < columns.len() {
                                columns.remove(index);
                            }
                        })
                    });
                    let custom = column.field.value(cx).as_deref() == Some(CUSTOM);
                    panel = panel.child(
                        col()
                            .gap(m.space_2)
                            .p(m.space_2)
                            .border_1()
                            .border_color(c.border_subtle)
                            .bg(c.bg_app)
                            .child(
                                row()
                                    .gap(m.space_1)
                                    .items_center()
                                    .child(div().flex_1().min_w_0().child(Select::new(&column.field)))
                                    .child(IconButton::new(("template-column-up", index), IconName::ChevronUp).title("Monter").size(IconButtonSize::Sm).disabled(index == 0).on_click(shift(-1)))
                                    .child(IconButton::new(("template-column-down", index), IconName::ChevronDown).title("Descendre").size(IconButtonSize::Sm).disabled(index == last).on_click(shift(1)))
                                    .child(IconButton::new(("template-column-remove", index), IconName::Trash).title("Supprimer la colonne").size(IconButtonSize::Sm).danger().on_click(remove)),
                            )
                            .when(custom, |this| this.child(FormField::new("Champ").child(Input::new(&column.binding).mono())))
                            .child(
                                row()
                                    .gap(m.space_2)
                                    .child(div().flex_1().min_w_0().child(FormField::new("En-tête").child(Input::new(&column.header))))
                                    .child(div().w(px(72.)).child(FormField::new("Largeur %").child(Input::new(&column.width)))),
                            )
                            .child(
                                row()
                                    .gap(m.space_2)
                                    .child(div().flex_1().min_w_0().child(Select::new(&column.format)))
                                    .child(div().flex_1().min_w_0().child(Select::new(&column.align))),
                            ),
                    );
                }
            }
            Kind::Rect(_) => {
                panel = panel
                    .child(section("Cadre"))
                    .child(FormField::new("Bordure").child(ColorField::new("template-rect-stroke", &fields.stroke).presets(&SWATCHES).on_pick(picked("stroke"))))
                    .child(FormField::new("Épaisseur (mm)").child(Input::new(&fields.stroke_width)))
                    .child(FormField::new("Remplissage").hint("Une couleur (#eef2f6), ou « transparent ».").child(Input::new(&fields.fill).mono()));
            }
        }
        panel
            .child(
                div().pt(m.space_2).child(
                    Button::new("template-props-delete")
                        .danger()
                        .full_width()
                        .icon(IconName::Trash)
                        .label("Supprimer l'élément")
                        .on_click(cx.listener(|this, _, window, cx| this.delete_selected(window, cx))),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::templates::schema::{Align, Format};

    use super::{automatic_columns, shown, typed};

    #[test]
    fn numbers_are_shown_and_read_the_french_way() {
        assert_eq!(shown(12.0), "12");
        assert_eq!(shown(12.5), "12,5");
        assert_eq!(shown(0.25), "0,25");
        assert_eq!(shown(100.0), "100");
        assert_eq!(typed(" 12,5 "), Some(12.5));
        assert_eq!(typed("12.5"), Some(12.5));
        assert_eq!(typed("-3"), Some(-3.0));
        assert_eq!(typed(""), None);
        assert_eq!(typed("douze"), None);
    }

    #[test]
    fn a_table_can_take_one_column_per_field_of_its_list() {
        let columns = automatic_columns("items");
        assert_eq!(columns.len(), 7);
        assert_eq!((columns[0].header.as_str(), columns[0].binding.as_str(), columns[0].align), ("Désignation", "label", Some(Align::Left)));
        assert_eq!((columns[1].format, columns[1].align), (Some(Format::Qty), Some(Align::Right)));
        assert!((columns.iter().map(|c| c.width_pct).sum::<f64>() - 100.0).abs() < 1.0);
        assert!(automatic_columns("inconnue").is_empty());
    }
}
