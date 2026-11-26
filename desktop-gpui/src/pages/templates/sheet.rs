//! What the screens of the templates share: who the shop is (its names, its settings, its
//! logo), and how a laid-out page becomes what the kit draws.

use std::collections::HashMap;
use std::sync::Arc;

use motodesk_client::company::{self, Company};
use motodesk_client::settings::{self, AppSetting};
use motodesk_client::templates::render::{self, Mode, PageDrawing, Picture, Pictures, Shape};
use motodesk_client::templates::sample::{self, LOGO_SOURCE};
use motodesk_client::templates::schema::{Kind, Layout};
use motodesk_client::templates::{pdf, TemplateKind};
use motodesk_client::Client;
use motodesk_ui::gpui::{Image, SharedString};
use motodesk_ui::{PageShape, picture_from_bytes};
use serde_json::Value;

/// The shop as documents show it. Read once when the screen opens.
#[derive(Default)]
pub struct Shop {
    company: Option<Company>,
    settings: Vec<AppSetting>,
    /// The pictures at hand, decoded (their size is what fits them in a frame).
    pub pictures: Pictures,
    /// The same pictures, as the screen draws them.
    images: HashMap<String, Arc<Image>>,
}

impl Shop {
    /// Read the shop's identity. Whatever cannot be read is left empty: a template is still
    /// shown, with the sample garage in place of what is missing.
    pub async fn load(client: &Client) -> Shop {
        let mut shop = Shop { company: company::get(client).await.ok(), settings: settings::list(client).await.unwrap_or_default(), ..Shop::default() };
        if shop.company.as_ref().is_some_and(|company| company.has_logo)
            && let Ok(Some(bytes)) = company::logo(client).await
        {
            shop.keep(LOGO_SOURCE, bytes);
        }
        shop
    }

    fn keep(&mut self, source: &str, bytes: Vec<u8>) {
        let (Some(picture), Some(image)) = (Picture::decode(&bytes), picture_from_bytes(bytes)) else { return };
        self.pictures.insert(source.to_string(), picture);
        self.images.insert(source.to_string(), image);
    }

    /// A sample document of this kind, issued by this shop.
    pub fn sample(&self, kind: TemplateKind) -> Value {
        sample::sample_for(kind, self.company.as_ref(), &self.settings)
    }

    /// Lay one page of a template out and hand it to the kit.
    pub fn page(&self, layout: &Layout, data: &Value, mode: Mode, page: usize) -> Vec<PageShape> {
        self.shapes(&render::draw(layout, data, &self.pictures, mode, page))
    }

    pub fn shapes(&self, drawing: &PageDrawing) -> Vec<PageShape> {
        drawing.shapes.iter().filter_map(|shape| self.shape(shape)).collect()
    }

    fn shape(&self, shape: &Shape) -> Option<PageShape> {
        let text = |text: &String| SharedString::from(text.clone());
        Some(match shape {
            Shape::Rect { x, y, w, h, fill, stroke, dashed } => PageShape::Rect {
                x: *x as f32,
                y: *y as f32,
                w: *w as f32,
                h: *h as f32,
                fill: fill.map(|fill| fill.hex()),
                stroke: stroke.map(|(color, width)| (color.hex(), width as f32)),
                dashed: *dashed,
            },
            Shape::Line { x1, y1, x2, y2, color, width } => {
                PageShape::Line { x1: *x1 as f32, y1: *y1 as f32, x2: *x2 as f32, y2: *y2 as f32, color: color.hex(), width: *width as f32 }
            }
            Shape::Text { x, y, text: line, size, bold, color } => {
                PageShape::Text { x: *x as f32, y: *y as f32, text: text(line), size: *size as f32, bold: *bold, color: color.hex() }
            }
            Shape::Badge { x, y, w, h, label, size, bold } => {
                PageShape::Badge { x: *x as f32, y: *y as f32, w: *w as f32, h: *h as f32, label: text(label), size: *size as f32, bold: *bold }
            }
            Shape::Image { x, y, w, h, source } => {
                PageShape::Image { x: *x as f32, y: *y as f32, w: *w as f32, h: *h as f32, image: self.images.get(source)?.clone() }
            }
            Shape::Placeholder { x, y, w, h } => PageShape::Placeholder { x: *x as f32, y: *y as f32, w: *w as f32, h: *h as f32 },
            // The mark across a copy is for paper: the screens here show originals and samples.
            Shape::Watermark { .. } => return None,
        })
    }

    /// Pictures a template carries in its own text (`data:image/png;base64,…`) are decoded the
    /// first time they are met. Pictures at a web address are not fetched.
    pub fn read_inline_pictures(&mut self, layout: &Layout, data: &Value) {
        for element in &layout.elements {
            let Kind::Image(image) = &element.kind else { continue };
            let source = render::image_source(image, data);
            if !source.starts_with("data:") || self.pictures.contains_key(&source) {
                continue;
            }
            if let Some(bytes) = pdf::data_url_bytes(&source) {
                self.keep(&source, bytes);
            }
        }
    }
}
