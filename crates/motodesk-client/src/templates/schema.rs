//! What a template is made of (port of `desktop/src/lib/templates/schema.ts`): a page and the
//! elements placed on it.
//!
//! Positions and sizes are in **millimetres**, font sizes in points, so a template reads the
//! same on a screen at any zoom and on paper. This is the JSON the Svelte editor wrote: a
//! template made there opens here unchanged, and the other way round.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// The smallest an element can be made.
pub const MIN_WIDTH_MM: f64 = 5.0;
pub const MIN_HEIGHT_MM: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub width: f64,
    pub height: f64,
    /// Shown as a guide in the editor; nothing stops an element from crossing it.
    #[serde(default = "default_margin")]
    pub margin: f64,
}

fn default_margin() -> f64 {
    15.0
}

impl Page {
    pub const A4_PORTRAIT: Page = Page { width: 210.0, height: 297.0, margin: 15.0 };
    pub const A4_LANDSCAPE: Page = Page { width: 297.0, height: 210.0, margin: 15.0 };

    pub fn is_landscape(&self) -> bool {
        self.width > self.height
    }
}

impl Default for Page {
    fn default() -> Self {
        Page::A4_PORTRAIT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub const ALL: [Align; 3] = [Align::Left, Align::Center, Align::Right];

    pub fn id(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }

    pub fn from_id(id: &str) -> Self {
        Self::ALL.into_iter().find(|align| align.id() == id).unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Align::Left => "Gauche",
            Align::Center => "Centre",
            Align::Right => "Droite",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weight {
    #[default]
    Normal,
    Bold,
}

/// How a picture fills its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Whole and undistorted, centred in the frame.
    #[default]
    Contain,
    /// Stretched to the frame.
    Fill,
}

/// How the values of a table column are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Text,
    Qty,
    Price,
    Percent,
    #[serde(other)]
    Other,
}

impl Format {
    pub const CHOICES: [Format; 4] = [Format::Text, Format::Qty, Format::Price, Format::Percent];

    pub fn id(self) -> &'static str {
        match self {
            Format::Text | Format::Other => "text",
            Format::Qty => "qty",
            Format::Price => "price",
            Format::Percent => "percent",
        }
    }

    pub fn from_id(id: &str) -> Self {
        Self::CHOICES.into_iter().find(|format| format.id() == id).unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Text | Format::Other => "Texte",
            Format::Qty => "Quantité",
            Format::Price => "Prix",
            Format::Percent => "Pourcentage",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextEl {
    /// May hold variables: `Bonjour {{client.first_name}}`.
    #[serde(default)]
    pub content: String,
    /// In points.
    #[serde(rename = "fontSize", default = "default_font_size")]
    pub font_size: f64,
    #[serde(default)]
    pub align: Align,
    #[serde(rename = "fontWeight", default)]
    pub font_weight: Weight,
    /// A colour as CSS writes it (`#1a1a1a`).
    #[serde(default = "default_ink")]
    pub color: String,
}

fn default_font_size() -> f64 {
    11.0
}

fn default_ink() -> String {
    "#1a1a1a".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageEl {
    /// An address, or a variable that holds one (`{{shop.logo_url}}`).
    #[serde(default)]
    pub src: String,
    #[serde(default)]
    pub fit: Fit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableColumn {
    #[serde(default)]
    pub header: String,
    /// The field of each row this column shows (`label`, for `items[].label`).
    #[serde(default)]
    pub binding: String,
    /// Share of the table's width. The shares need not add up to 100.
    #[serde(rename = "widthPct", default)]
    pub width_pct: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableEl {
    /// The list the rows come from (`items`).
    #[serde(rename = "listBinding", default = "default_list")]
    pub list_binding: String,
    #[serde(default)]
    pub columns: Vec<TableColumn>,
    #[serde(rename = "rowHeight", default = "default_row_height")]
    pub row_height: f64,
    #[serde(rename = "headerHeight", default = "default_header_height")]
    pub header_height: f64,
    #[serde(rename = "fontSize", default = "default_table_font")]
    pub font_size: f64,
}

fn default_list() -> String {
    "items".to_string()
}

fn default_row_height() -> f64 {
    7.0
}

fn default_header_height() -> f64 {
    8.0
}

fn default_table_font() -> f64 {
    10.0
}

impl TableEl {
    /// How many rows fit under the header of a table `height` mm tall. At least one.
    pub fn capacity(&self, height: f64) -> usize {
        if self.row_height <= 0.0 {
            return 1;
        }
        (((height - self.header_height) / self.row_height).floor() as i64).max(1) as usize
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RectEl {
    #[serde(default = "default_ink")]
    pub stroke: String,
    /// In millimetres.
    #[serde(rename = "strokeWidth", default)]
    pub stroke_width: f64,
    /// A colour, or `transparent`.
    #[serde(default = "default_fill")]
    pub fill: String,
}

fn default_fill() -> String {
    "transparent".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Kind {
    Text(TextEl),
    Image(ImageEl),
    Table(TableEl),
    Rect(RectEl),
}

impl Kind {
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Text(_) => "Texte",
            Kind::Image(_) => "Image",
            Kind::Table(_) => "Tableau",
            Kind::Rect(_) => "Cadre",
        }
    }

    fn id_prefix(&self) -> &'static str {
        match self {
            Kind::Text(_) => "txt",
            Kind::Image(_) => "img",
            Kind::Table(_) => "tbl",
            Kind::Rect(_) => "rct",
        }
    }
}

/// One thing on the page: where it is, how large, and what it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(flatten)]
    pub kind: Kind,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// An identifier no other element of this run has. `seed` tells two runs apart (the time).
pub fn new_id(prefix: &str, seed: u64) -> String {
    let count = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{}_{}", radix36(seed), radix36(count))
}

fn radix36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

impl Element {
    fn new(kind: Kind, x: f64, y: f64, w: f64, h: f64, seed: u64) -> Self {
        Element { id: new_id(kind.id_prefix(), seed), x, y, w, h, kind }
    }

    pub fn new_text(x: f64, y: f64, seed: u64) -> Self {
        let text = TextEl { content: "Texte".to_string(), font_size: 11.0, align: Align::Left, font_weight: Weight::Normal, color: default_ink() };
        Self::new(Kind::Text(text), x, y, 80.0, 10.0, seed)
    }

    pub fn new_image(x: f64, y: f64, seed: u64) -> Self {
        Self::new(Kind::Image(ImageEl { src: String::new(), fit: Fit::Contain }), x, y, 40.0, 40.0, seed)
    }

    pub fn new_table(x: f64, y: f64, seed: u64) -> Self {
        let column = |header: &str, binding: &str, width_pct: f64, format: Format, align: Align| TableColumn {
            header: header.to_string(),
            binding: binding.to_string(),
            width_pct,
            format: Some(format),
            align: Some(align),
        };
        let table = TableEl {
            list_binding: default_list(),
            columns: vec![
                column("Désignation", "label", 50.0, Format::Text, Align::Left),
                column("Qté", "qty", 12.0, Format::Qty, Align::Right),
                column("PU HT", "price_ht", 18.0, Format::Price, Align::Right),
                column("Total TTC", "total_ttc", 20.0, Format::Price, Align::Right),
            ],
            row_height: 7.0,
            header_height: 8.0,
            font_size: 10.0,
        };
        Self::new(Kind::Table(table), x, y, 170.0, 80.0, seed)
    }

    pub fn new_rect(x: f64, y: f64, seed: u64) -> Self {
        Self::new(Kind::Rect(RectEl { stroke: default_ink(), stroke_width: 0.3, fill: default_fill() }), x, y, 40.0, 20.0, seed)
    }

    /// A copy under a new identifier, moved by (`dx`, `dy`) and kept on `page`.
    pub fn copy_moved(&self, dx: f64, dy: f64, page: &Page, seed: u64) -> Element {
        let mut copy = self.clone();
        copy.id = new_id(self.kind.id_prefix(), seed);
        copy.move_to(self.x + dx, self.y + dy, page);
        copy
    }

    /// Put the element at (`x`, `y`), or as near as the page allows.
    pub fn move_to(&mut self, x: f64, y: f64, page: &Page) {
        self.x = x.min(page.width - self.w).max(0.0);
        self.y = y.min(page.height - self.h).max(0.0);
    }

    /// What the list of layers calls this element: its text, the list of its table, or its kind.
    pub fn title(&self) -> String {
        let named = match &self.kind {
            Kind::Text(text) => text.content.lines().next().unwrap_or_default().trim().to_string(),
            Kind::Table(table) => format!("Tableau · {}", table.list_binding),
            Kind::Image(image) => image.src.trim().to_string(),
            Kind::Rect(_) => String::new(),
        };
        if named.is_empty() { self.kind.label().to_string() } else { named }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default = "version_one")]
    pub version: u32,
    #[serde(default)]
    pub page: Page,
    /// From the back of the page to the front.
    #[serde(default, deserialize_with = "known_elements")]
    pub elements: Vec<Element>,
}

fn version_one() -> u32 {
    1
}

impl Default for Layout {
    fn default() -> Self {
        Layout { version: 1, page: Page::default(), elements: Vec::new() }
    }
}

impl Layout {
    pub fn element(&self, id: &str) -> Option<&Element> {
        self.elements.iter().find(|element| element.id == id)
    }

    pub fn element_mut(&mut self, id: &str) -> Option<&mut Element> {
        self.elements.iter_mut().find(|element| element.id == id)
    }

    /// Turn the page: its margin is kept, its elements stay where they are.
    pub fn set_landscape(&mut self, landscape: bool) {
        let wanted = if landscape { Page::A4_LANDSCAPE } else { Page::A4_PORTRAIT };
        self.page = Page { margin: self.page.margin, ..wanted };
    }
}

/// An element of a kind this version does not know is left out rather than making the whole
/// template unreadable.
fn known_elements<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Element>, D::Error> {
    let raw = Vec::<Value>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|value| serde_json::from_value(value).ok()).collect())
}

/// A layout that is not one (a string, `null`) reads as an empty page.
pub(crate) fn lenient<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Layout, D::Error> {
    let raw = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_goes_out_as_it_came_in() {
        let written = serde_json::json!({
            "version": 1,
            "page": { "width": 210.0, "height": 297.0, "margin": 12.0 },
            "elements": [
                { "id": "t", "type": "text", "x": 12.0, "y": 5.0, "w": 118.0, "h": 12.0, "content": "{{shop.name}}", "fontSize": 16.0, "align": "right", "fontWeight": "bold", "color": "#ffffff" },
                { "id": "i", "type": "image", "x": 1.0, "y": 2.0, "w": 40.0, "h": 40.0, "src": "{{shop.logo_url}}", "fit": "contain" },
                { "id": "b", "type": "table", "x": 12.0, "y": 100.0, "w": 186.0, "h": 92.0, "listBinding": "items", "columns": [
                    { "header": "Désignation", "binding": "label", "widthPct": 42.0, "format": "text", "align": "left" },
                    { "header": "Libre", "binding": "note", "widthPct": 10.0 }
                ], "rowHeight": 7.0, "headerHeight": 8.0, "fontSize": 8.0 },
                { "id": "r", "type": "rect", "x": 0.0, "y": 0.0, "w": 210.0, "h": 22.0, "stroke": "#1e3a5f", "strokeWidth": 0.0, "fill": "#1e3a5f" }
            ]
        });
        let layout: Layout = serde_json::from_value(written.clone()).unwrap();
        assert_eq!(layout.elements.len(), 4);
        assert_eq!(serde_json::to_value(&layout).unwrap(), written);
    }

    #[test]
    fn an_element_of_an_unknown_kind_is_left_out() {
        let layout: Layout = serde_json::from_value(serde_json::json!({
            "version": 1, "page": { "width": 210, "height": 297 },
            "elements": [
                { "id": "q", "type": "qrcode", "x": 1, "y": 1, "w": 20, "h": 20, "value": "x" },
                { "id": "t", "type": "text", "x": 1, "y": 1, "w": 20, "h": 5 }
            ]
        }))
        .unwrap();
        assert_eq!(layout.elements.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["t"]);
        // What a text did not say takes the editor's defaults.
        assert_eq!(layout.elements[0].kind, Kind::Text(TextEl { content: String::new(), font_size: 11.0, align: Align::Left, font_weight: Weight::Normal, color: "#1a1a1a".into() }));
        assert_eq!(layout.page.margin, 15.0);
    }

    #[test]
    fn a_copy_has_its_own_identifier_and_stays_on_the_page() {
        let page = Page::A4_PORTRAIT;
        let text = Element::new_text(180.0, 290.0, 7);
        let copy = text.copy_moved(4.0, 4.0, &page, 7);
        assert_ne!(copy.id, text.id);
        assert!(copy.id.starts_with("txt_"));
        assert_eq!((copy.x, copy.y), (130.0, 287.0), "pushed back inside the page");
        assert_eq!(copy.kind, text.kind);
    }

    #[test]
    fn a_table_holds_at_least_one_row() {
        let Kind::Table(table) = Element::new_table(0.0, 0.0, 1).kind else { panic!("a table") };
        assert_eq!(table.capacity(80.0), 10);
        assert_eq!(table.capacity(92.0), 12);
        assert_eq!(table.capacity(5.0), 1);
    }

    #[test]
    fn turning_the_page_keeps_its_margin() {
        let mut layout = Layout { page: Page { margin: 12.0, ..Page::A4_PORTRAIT }, ..Layout::default() };
        layout.set_landscape(true);
        assert_eq!(layout.page, Page { width: 297.0, height: 210.0, margin: 12.0 });
        assert!(layout.page.is_landscape());
    }

    #[test]
    fn a_layer_is_named_after_what_it_shows() {
        assert_eq!(Element::new_text(0.0, 0.0, 1).title(), "Texte");
        assert_eq!(Element::new_table(0.0, 0.0, 1).title(), "Tableau · items");
        assert_eq!(Element::new_rect(0.0, 0.0, 1).title(), "Cadre");
        let mut text = Element::new_text(0.0, 0.0, 1);
        text.kind = Kind::Text(TextEl { content: "  {{shop.name}}\nadresse".into(), font_size: 9.0, align: Align::Left, font_weight: Weight::Normal, color: "#000".into() });
        assert_eq!(text.title(), "{{shop.name}}");
    }
}
