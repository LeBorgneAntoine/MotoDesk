//! What a template draws (port of the layout half of `desktop/src/lib/templates/engine.ts` and
//! `print.ts`): a template and the data of a document become, page by page, a list of plain
//! shapes in millimetres.
//!
//! The screen and the PDF both draw these shapes and nothing else, so what is shown is what is
//! printed. Nothing here knows about pixels, windows or files.

use std::collections::HashMap;

use serde_json::Value;

use super::interpolate::{bare_variable, format_value, interpolate, resolve, tokenize, Hint, Segment};
use super::metrics::{line_height, text_width, wrap, PT_TO_MM};
use super::schema::{Align, Element, Fit, Format, ImageEl, Kind, Layout, RectEl, TableEl, TextEl, Weight};
use super::variables::human_label;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// As one number, `0xRRGGBB`.
    pub fn hex(self) -> u32 {
        u32::from(self.r) << 16 | u32::from(self.g) << 8 | u32::from(self.b)
    }
}

const INK: Rgb = Rgb::new(0x1a, 0x1a, 0x1a);
const HEADER_INK: Rgb = Rgb::new(0x0d, 0x0d, 0x0d);
const HEADER_FILL: Rgb = Rgb::new(0xf3, 0xf3, 0xf3);
const ZEBRA: Rgb = Rgb::new(0xfa, 0xfa, 0xfa);
const SEPARATOR: Rgb = Rgb::new(0xc8, 0xc8, 0xc8);
const GUIDE: Rgb = Rgb::new(0xdc, 0xdc, 0xdc);
const PAGE_LABEL: Rgb = Rgb::new(0x55, 0x55, 0x55);

/// The thinnest line drawn: a frame whose border is set to nothing still has this one.
const HAIRLINE: f64 = 0.1;
/// Room between a table cell's edge and its text.
const CELL_PAD: f64 = 1.0;
/// Room around a variable's name in the editor, and between two of them.
const BADGE_PAD: f64 = 1.0;
const BADGE_GAP: f64 = 0.25;

/// A colour as a template writes it: `#1a1a1a`, `#fff`. `transparent`, or anything that is not
/// a colour, is none.
pub fn parse_color(text: &str) -> Option<Rgb> {
    let hex = text.trim().strip_prefix('#')?;
    let digit = |at: usize| u8::from_str_radix(hex.get(at..at + 1)?, 16).ok();
    let pair = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    match hex.len() {
        3 | 4 => Some(Rgb::new(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)),
        6 | 8 => Some(Rgb::new(pair(0)?, pair(2)?, pair(4)?)),
        _ => None,
    }
}

/// One thing to draw. Positions and sizes are millimetres from the top left of the page; font
/// sizes are points.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Rect { x: f64, y: f64, w: f64, h: f64, fill: Option<Rgb>, stroke: Option<(Rgb, f64)>, dashed: bool },
    Line { x1: f64, y1: f64, x2: f64, y2: f64, color: Rgb, width: f64 },
    /// One line of text; (`x`, `y`) is the top left of its line box.
    Text { x: f64, y: f64, text: String, size: f64, bold: bool, color: Rgb },
    /// In the editor only: a variable, shown by its name rather than by a value.
    Badge { x: f64, y: f64, w: f64, h: f64, label: String, size: f64, bold: bool },
    /// A picture, already fitted: the frame given is the one it fills.
    Image { x: f64, y: f64, w: f64, h: f64, source: String },
    /// In the editor only: where a picture goes while there is none to show.
    Placeholder { x: f64, y: f64, w: f64, h: f64 },
    /// A word across the whole page, on top of everything: DUPLICATA on a copy.
    Watermark { text: String },
}

/// One page, ready to be drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct PageDrawing {
    pub width: f64,
    pub height: f64,
    pub shapes: Vec<Shape>,
}

/// A picture a template can show, decoded: its size and its pixels (red, green, blue, opacity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The pictures at hand, by the source a template names them with.
pub type Pictures = HashMap<String, Picture>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// As it goes to paper: variables replaced by their values.
    Print,
    /// As the editor shows it: variables shown by name, the margin drawn, room kept for pictures.
    Edit,
}

fn font_bold(text: &TextEl) -> bool {
    text.font_weight == Weight::Bold
}

fn aligned(align: Align, box_width: f64, content_width: f64) -> f64 {
    match align {
        Align::Left => 0.0,
        Align::Center => ((box_width - content_width) / 2.0).max(0.0),
        Align::Right => (box_width - content_width).max(0.0),
    }
}

fn text_shapes(element: &Element, text: &TextEl, data: &Value, mode: Mode, out: &mut Vec<Shape>) {
    let color = parse_color(&text.color).unwrap_or(INK);
    let (size, bold) = (text.font_size, font_bold(text));
    let line_h = line_height(size);
    if mode == Mode::Edit {
        // One row per line as written, never wrapped: a variable keeps its name in one piece.
        for (row, raw) in text.content.split('\n').enumerate() {
            let pieces: Vec<(Segment, f64)> = tokenize(raw)
                .into_iter()
                .map(|segment| {
                    let width = match &segment {
                        Segment::Text(plain) => text_width(plain, size, bold),
                        Segment::Var { path, .. } => text_width(&human_label(path), size, bold) + 2.0 * BADGE_PAD + BADGE_GAP,
                    };
                    (segment, width)
                })
                .collect();
            let total: f64 = pieces.iter().map(|(_, width)| width).sum();
            let mut x = element.x + aligned(text.align, element.w, total);
            let y = element.y + row as f64 * line_h;
            for (segment, width) in pieces {
                match segment {
                    Segment::Text(plain) => out.push(Shape::Text { x, y, text: plain, size, bold, color }),
                    Segment::Var { path, .. } => {
                        out.push(Shape::Badge { x, y, w: width - BADGE_GAP, h: line_h, label: human_label(&path), size, bold })
                    }
                }
                x += width;
            }
        }
        return;
    }
    // What does not fit the height of the element is not drawn; the first line always is.
    let fitting = ((element.h + 1e-6) / line_h).floor().max(1.0) as usize;
    for (row, line) in wrap(&interpolate(&text.content, data), element.w, size, bold).into_iter().take(fitting).enumerate() {
        let line = line.trim_end().to_string();
        if line.is_empty() {
            continue;
        }
        let x = element.x + aligned(text.align, element.w, text_width(&line, size, bold));
        out.push(Shape::Text { x, y: element.y + row as f64 * line_h, text: line, size, bold, color });
    }
}

fn rect_shapes(element: &Element, rect: &RectEl, out: &mut Vec<Shape>) {
    let stroke = parse_color(&rect.stroke).map(|color| (color, rect.stroke_width.max(HAIRLINE)));
    out.push(Shape::Rect { x: element.x, y: element.y, w: element.w, h: element.h, fill: parse_color(&rect.fill), stroke, dashed: false });
}

/// Where a picture of `size` pixels goes in a frame: whole and centred, or stretched.
fn fitted(element: &Element, fit: Fit, (width, height): (u32, u32)) -> (f64, f64, f64, f64) {
    if fit == Fit::Fill || width == 0 || height == 0 {
        return (element.x, element.y, element.w, element.h);
    }
    let ratio = (element.w / f64::from(width)).min(element.h / f64::from(height));
    let (w, h) = (f64::from(width) * ratio, f64::from(height) * ratio);
    (element.x + (element.w - w) / 2.0, element.y + (element.h - h) / 2.0, w, h)
}

/// The source a picture element names once its variables are replaced.
pub fn image_source(image: &ImageEl, data: &Value) -> String {
    match bare_variable(&image.src) {
        Some(path) => resolve(&path, data).and_then(Value::as_str).unwrap_or_default().to_string(),
        None => interpolate(&image.src, data),
    }
}

fn image_shapes(element: &Element, image: &ImageEl, data: &Value, pictures: &Pictures, mode: Mode, out: &mut Vec<Shape>) {
    let source = image_source(image, data);
    match pictures.get(&source) {
        Some(picture) => {
            let (x, y, w, h) = fitted(element, image.fit, (picture.width, picture.height));
            out.push(Shape::Image { x, y, w, h, source });
        }
        // On paper a missing picture leaves nothing; the editor keeps its place visible.
        None if mode == Mode::Edit => out.push(Shape::Placeholder { x: element.x, y: element.y, w: element.w, h: element.h }),
        None => {}
    }
}

/// The rows a table is bound to, in the data.
fn table_rows<'a>(table: &TableEl, data: &'a Value) -> &'a [Value] {
    resolve(&table.list_binding, data).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn hint_of(format: Option<Format>) -> Hint {
    match format {
        Some(Format::Price) => Hint::Price,
        Some(Format::Qty) => Hint::Qty,
        Some(Format::Percent) => Hint::Percent,
        _ => Hint::Text,
    }
}

/// How the text of a table cell is set.
#[derive(Clone, Copy)]
struct CellStyle {
    align: Align,
    size: f64,
    bold: bool,
    color: Rgb,
}

/// One cell: a single line, cut to the column, set in the middle of the row.
fn cell(text: &str, (x, y, width, height): (f64, f64, f64, f64), style: CellStyle, out: &mut Vec<Shape>) {
    let CellStyle { align, size, bold, color } = style;
    let inner = (width - 2.0 * CELL_PAD).max(0.0);
    let line = wrap(text, inner, size, bold).into_iter().next().unwrap_or_default();
    if line.trim().is_empty() {
        return;
    }
    let x = x + CELL_PAD + aligned(align, inner, text_width(&line, size, bold));
    out.push(Shape::Text { x, y: y + (height - line_height(size)) / 2.0, text: line, size, bold, color });
}

fn table_shapes(element: &Element, table: &TableEl, data: &Value, mode: Mode, page: usize, out: &mut Vec<Shape>) {
    let all = table_rows(table, data);
    let per_page = if table.row_height > 0.0 { ((element.h - table.header_height) / table.row_height).floor().max(0.0) as usize } else { 0 };
    // A table that ended on an earlier page is not repeated empty.
    if page > 0 && all.len() <= page * per_page {
        return;
    }
    let rows: Vec<&Value> = all.iter().skip(page * per_page).take(per_page).collect();
    // With no row to show, the editor draws three empty ones so the table has a shape.
    let drawn = if rows.is_empty() && mode == Mode::Edit && page == 0 { 3.min(per_page) } else { rows.len() };

    let total: f64 = table.columns.iter().map(|column| column.width_pct.max(0.0)).sum();
    let total = if total > 0.0 { total } else { 100.0 };
    let widths: Vec<f64> = table.columns.iter().map(|column| element.w * column.width_pct.max(0.0) / total).collect();
    let lefts: Vec<f64> = widths.iter().scan(element.x, |x, width| Some(std::mem::replace(x, *x + width))).collect();
    let (top, header_h, row_h, size) = (element.y, table.header_height, table.row_height, table.font_size);

    out.push(Shape::Rect { x: element.x, y: top, w: element.w, h: header_h, fill: Some(HEADER_FILL), stroke: None, dashed: false });
    for (index, column) in table.columns.iter().enumerate() {
        let header = interpolate(&column.header, data);
        let style = CellStyle { align: column.align.unwrap_or_default(), size, bold: true, color: HEADER_INK };
        cell(&header, (lefts[index], top, widths[index], header_h), style, out);
    }
    for row in 0..drawn {
        let y = top + header_h + row as f64 * row_h;
        if row % 2 == 1 {
            out.push(Shape::Rect { x: element.x, y, w: element.w, h: row_h, fill: Some(ZEBRA), stroke: None, dashed: false });
        }
        let Some(values) = rows.get(row) else { continue };
        for (index, column) in table.columns.iter().enumerate() {
            let value = format_value(resolve(&column.binding, values), hint_of(column.format));
            let style = CellStyle { align: column.align.unwrap_or_default(), size, bold: false, color: INK };
            cell(&value, (lefts[index], y, widths[index], row_h), style, out);
        }
    }
    let bottom = (header_h + drawn as f64 * row_h).min(element.h.max(header_h));
    for left in lefts.iter().skip(1) {
        out.push(Shape::Line { x1: *left, y1: top, x2: *left, y2: top + bottom, color: SEPARATOR, width: HAIRLINE });
    }
    out.push(Shape::Line { x1: element.x, y1: top + header_h, x2: element.x + element.w, y2: top + header_h, color: INK, width: HAIRLINE });
    out.push(Shape::Rect { x: element.x, y: top, w: element.w, h: bottom, fill: None, stroke: Some((INK, HAIRLINE)), dashed: false });
}

/// How many pages a document takes: the longest table decides. A list that does not fit its
/// table continues on the next page instead of being cut short above totals that count it all.
pub fn page_count(layout: &Layout, data: &Value) -> usize {
    layout
        .elements
        .iter()
        .filter_map(|element| match &element.kind {
            Kind::Table(table) => Some(table_rows(table, data).len().div_ceil(table.capacity(element.h))),
            _ => None,
        })
        .max()
        .unwrap_or(1)
        .max(1)
}

/// Draw one page of a template. `page` counts from 0: a table shows the rows of that page,
/// everything else is repeated.
pub fn draw(layout: &Layout, data: &Value, pictures: &Pictures, mode: Mode, page: usize) -> PageDrawing {
    let mut shapes = Vec::new();
    if mode == Mode::Edit {
        let margin = layout.page.margin;
        shapes.push(Shape::Rect {
            x: margin,
            y: margin,
            w: (layout.page.width - 2.0 * margin).max(0.0),
            h: (layout.page.height - 2.0 * margin).max(0.0),
            fill: None,
            stroke: Some((GUIDE, HAIRLINE)),
            dashed: true,
        });
    }
    for element in &layout.elements {
        match &element.kind {
            Kind::Text(text) => text_shapes(element, text, data, mode, &mut shapes),
            Kind::Rect(rect) => rect_shapes(element, rect, &mut shapes),
            Kind::Image(image) => image_shapes(element, image, data, pictures, mode, &mut shapes),
            Kind::Table(table) => table_shapes(element, table, data, mode, page, &mut shapes),
        }
    }
    PageDrawing { width: layout.page.width, height: layout.page.height, shapes }
}

/// The whole document as it goes to paper: every page, numbered when there are several, with
/// `watermark` across each when there is one.
pub fn document(layout: &Layout, data: &Value, pictures: &Pictures, watermark: Option<&str>) -> Vec<PageDrawing> {
    let count = page_count(layout, data);
    (0..count)
        .map(|page| {
            let mut drawing = draw(layout, data, pictures, Mode::Print, page);
            if count > 1 {
                let (text, size) = (format!("Page {} / {count}", page + 1), 3.0 / PT_TO_MM);
                let x = layout.page.width - 10.0 - text_width(&text, size, false);
                drawing.shapes.push(Shape::Text { x, y: layout.page.height - 8.0, text, size, bold: false, color: PAGE_LABEL });
            }
            if let Some(word) = watermark.map(str::trim).filter(|word| !word.is_empty()) {
                drawing.shapes.push(Shape::Watermark { text: word.to_uppercase() });
            }
            drawing
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::sample::{demo_shop, sample_data, LOGO_SOURCE};
    use super::super::starters::starter_layout;
    use super::super::TemplateKind;
    use super::*;

    fn texts(drawing: &PageDrawing) -> Vec<&str> {
        drawing.shapes.iter().filter_map(|shape| if let Shape::Text { text, .. } = shape { Some(text.as_str()) } else { None }).collect()
    }

    fn one_text(content: &str, w: f64, h: f64, align: Align) -> Layout {
        let mut element = Element::new_text(10.0, 20.0, 1);
        (element.w, element.h) = (w, h);
        element.kind = Kind::Text(TextEl { content: content.into(), font_size: 10.0, align, font_weight: Weight::Normal, color: "#1e3a5f".into() });
        Layout { elements: vec![element], ..Layout::default() }
    }

    #[test]
    fn colours_read_as_templates_write_them() {
        assert_eq!(parse_color("#1e3a5f"), Some(Rgb::new(0x1e, 0x3a, 0x5f)));
        assert_eq!(parse_color(" #FFF "), Some(Rgb::new(255, 255, 255)));
        assert_eq!(parse_color("#1e3a5f80"), Some(Rgb::new(0x1e, 0x3a, 0x5f)));
        assert_eq!(parse_color("transparent"), None);
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color("#ggg"), None);
        assert_eq!(Rgb::new(0x1e, 0x3a, 0x5f).hex(), 0x1e3a5f);
    }

    #[test]
    fn on_paper_a_variable_is_its_value_aligned_in_its_box() {
        let data = json!({ "doc": { "total_ttc": 12345 } });
        let layout = one_text("{{doc.total_ttc | price}}", 60.0, 10.0, Align::Right);
        let drawing = draw(&layout, &data, &Pictures::new(), Mode::Print, 0);
        let [Shape::Text { x, y, text, size, color, .. }] = drawing.shapes.as_slice() else { panic!("one line: {:?}", drawing.shapes) };
        assert_eq!((text.as_str(), *size, *color), ("123,45\u{a0}€", 10.0, Rgb::new(0x1e, 0x3a, 0x5f)));
        assert!((x + text_width(text, 10.0, false) - 70.0).abs() < 1e-9, "its right edge is the box's");
        assert_eq!(*y, 20.0);

        let centred = draw(&one_text("abc", 60.0, 10.0, Align::Center), &data, &Pictures::new(), Mode::Print, 0);
        let [Shape::Text { x, .. }] = centred.shapes.as_slice() else { panic!("one line") };
        assert!((x - (10.0 + (60.0 - text_width("abc", 10.0, false)) / 2.0)).abs() < 1e-9);
        // Nothing to show draws nothing.
        assert!(draw(&one_text("{{doc.number}}", 60.0, 10.0, Align::Left), &data, &Pictures::new(), Mode::Print, 0).shapes.is_empty());
    }

    #[test]
    fn a_text_is_cut_to_the_lines_its_box_holds() {
        let long = "Révision des 24 000 km. RAS transmission. Prochain contrôle : plaquettes arrière vers 28 000 km.";
        let one_line = line_height(10.0);
        let lines = |h: f64| texts(&draw(&one_text(long, 50.0, h, Align::Left), &json!({}), &Pictures::new(), Mode::Print, 0)).len();
        assert_eq!(lines(one_line * 2.0 + 0.01), 2);
        assert_eq!(lines(one_line * 0.5), 1, "the first line is always drawn");
        assert!(lines(100.0) >= 4);
    }

    #[test]
    fn in_the_editor_a_variable_shows_its_name() {
        let layout = one_text("Bonjour {{client.first_name}} !\n{{doc.unknown_thing}}", 120.0, 10.0, Align::Left);
        let drawing = draw(&layout, &json!({ "client": { "first_name": "Léa" } }), &Pictures::new(), Mode::Edit, 0);
        let badges: Vec<&str> = drawing.shapes.iter().filter_map(|s| if let Shape::Badge { label, .. } = s { Some(label.as_str()) } else { None }).collect();
        assert_eq!(badges, ["Prénom (Client)", "Unknown Thing"]);
        assert_eq!(texts(&drawing), ["Bonjour ", " !"]);
        assert!(matches!(drawing.shapes[0], Shape::Rect { dashed: true, .. }), "the margin is shown");
        // The second line sits one line lower.
        let ys: Vec<f64> = drawing.shapes.iter().filter_map(|s| if let Shape::Badge { y, .. } = s { Some(*y) } else { None }).collect();
        assert!((ys[1] - ys[0] - line_height(10.0)).abs() < 1e-9);
    }

    #[test]
    fn a_frame_keeps_a_hairline_and_no_fill_when_transparent() {
        let mut element = Element::new_rect(5.0, 5.0, 1);
        element.kind = Kind::Rect(RectEl { stroke: "#d0d5db".into(), stroke_width: 0.0, fill: "transparent".into() });
        let drawing = draw(&Layout { elements: vec![element], ..Layout::default() }, &json!({}), &Pictures::new(), Mode::Print, 0);
        assert_eq!(drawing.shapes, [Shape::Rect { x: 5.0, y: 5.0, w: 40.0, h: 20.0, fill: None, stroke: Some((Rgb::new(0xd0, 0xd5, 0xdb), HAIRLINE)), dashed: false }]);
    }

    #[test]
    fn a_picture_is_fitted_whole_or_left_out() {
        let mut element = Element::new_image(10.0, 10.0, 1);
        (element.w, element.h) = (40.0, 20.0);
        element.kind = Kind::Image(ImageEl { src: "{{shop.logo_url}}".into(), fit: Fit::Contain });
        let layout = Layout { elements: vec![element], ..Layout::default() };
        let data = json!({ "shop": { "logo_url": LOGO_SOURCE } });
        let mut pictures = Pictures::new();
        pictures.insert(LOGO_SOURCE.into(), Picture { width: 100, height: 100, rgba: Vec::new() });

        // A square picture in a wide frame: as tall as the frame, centred.
        let drawing = draw(&layout, &data, &pictures, Mode::Print, 0);
        assert_eq!(drawing.shapes, [Shape::Image { x: 20.0, y: 10.0, w: 20.0, h: 20.0, source: LOGO_SOURCE.into() }]);

        assert!(draw(&layout, &data, &Pictures::new(), Mode::Print, 0).shapes.is_empty(), "nothing on paper without the picture");
        let edited = draw(&layout, &json!({}), &Pictures::new(), Mode::Edit, 0);
        assert!(edited.shapes.contains(&Shape::Placeholder { x: 10.0, y: 10.0, w: 40.0, h: 20.0 }));
    }

    #[test]
    fn a_long_list_continues_on_the_next_page() {
        let layout = starter_layout(TemplateKind::Facture);
        let mut data = sample_data(TemplateKind::Facture, &demo_shop(), 30);
        assert_eq!(page_count(&layout, &data), 1);
        // The invoice table holds 12 rows: 30 lines take three pages.
        data["items"] = Value::Array((1..=30).map(|n| json!({ "label": format!("Ligne {n}"), "qty": 1, "total_ttc": 100 })).collect());
        assert_eq!(page_count(&layout, &data), 3);

        let pages = document(&layout, &data, &Pictures::new(), Some("duplicata"));
        assert_eq!(pages.len(), 3);
        let on = |page: usize, text: &str| texts(&pages[page]).contains(&text);
        assert!(on(0, "Ligne 1") && on(0, "Ligne 12") && !on(0, "Ligne 13"));
        assert!(on(1, "Ligne 13") && on(1, "Ligne 24"));
        assert!(on(2, "Ligne 25") && on(2, "Ligne 30") && !on(2, "Ligne 24"));
        // Every page is numbered, carries the shop's name again, and the mark on top of it all.
        assert!(on(0, "Page 1 / 3") && on(2, "Page 3 / 3") && on(2, "Garage Martin Moto"));
        for page in &pages {
            assert_eq!(page.shapes.last(), Some(&Shape::Watermark { text: "DUPLICATA".into() }));
        }

        // A second, shorter table has nothing left to show on page 2: it is not drawn empty.
        let mut two = layout.clone();
        let mut small = Element::new_table(12.0, 240.0, 1);
        small.h = 20.0;
        small.kind = Kind::Table(TableEl { list_binding: "payments".into(), columns: Vec::new(), row_height: 7.0, header_height: 8.0, font_size: 8.0 });
        two.elements.push(small);
        let before = draw(&layout, &data, &Pictures::new(), Mode::Print, 1).shapes.len();
        assert_eq!(draw(&two, &data, &Pictures::new(), Mode::Print, 1).shapes.len(), before);
        assert!(draw(&two, &data, &Pictures::new(), Mode::Print, 0).shapes.len() > draw(&layout, &data, &Pictures::new(), Mode::Print, 0).shapes.len());
    }

    #[test]
    fn a_table_writes_each_value_the_way_its_column_says() {
        let layout = starter_layout(TemplateKind::Facture);
        let data = sample_data(TemplateKind::Facture, &demo_shop(), 30);
        let drawing = draw(&layout, &data, &Pictures::new(), Mode::Print, 0);
        let all = texts(&drawing);
        for expected in ["Désignation", "Total TTC", "Huile moteur Yamalube 10W-40", "3,2", "14,50\u{a0}€", "20\u{a0}%", "55,68\u{a0}€", "FACTURE", "FA-2026-C1-00042", "29/05/2026", "Léa Martin"] {
            assert!(all.contains(&expected), "{expected:?} is missing from {all:?}");
        }
        // One page, no page number, no mark.
        let pages = document(&layout, &data, &Pictures::new(), None);
        assert_eq!(pages.len(), 1);
        assert!(!texts(&pages[0]).iter().any(|text| text.starts_with("Page ")));
        assert!(!pages[0].shapes.iter().any(|shape| matches!(shape, Shape::Watermark { .. })));
    }

    #[test]
    fn an_empty_table_has_a_shape_in_the_editor_only() {
        let table = Element::new_table(10.0, 10.0, 1);
        let layout = Layout { elements: vec![table], ..Layout::default() };
        let zebra = |mode| draw(&layout, &json!({}), &Pictures::new(), mode, 0).shapes.iter().filter(|s| matches!(s, Shape::Rect { fill: Some(ZEBRA), .. })).count();
        assert_eq!(zebra(Mode::Edit), 1, "three empty rows, the second one tinted");
        assert_eq!(zebra(Mode::Print), 0);
        // The frame of an empty printed table stops under its header.
        let printed = draw(&layout, &json!({}), &Pictures::new(), Mode::Print, 0);
        assert!(matches!(printed.shapes.last(), Some(Shape::Rect { h, fill: None, .. }) if *h == 8.0));
    }
}
