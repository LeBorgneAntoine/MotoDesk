//! A sheet of paper on screen: the page of a document template, drawn from plain shapes placed
//! in millimetres. The same shapes go to the printer, so the sheet shows what will be printed.
//!
//! Paper is white and its ink is whatever the template says, in the light theme and the dark
//! one alike: this is the one place where colours come from data rather than from the theme.

use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, Image, InteractiveElement as _, IntoElement, ObjectFit, ParentElement, RenderOnce, SharedString,
    Styled as _, StyledImage as _, Window, div, img, px, rgb,
};

use crate::theme::Themed;

/// The font documents are set in. Its letters are as wide as those of the Helvetica a PDF is
/// printed with, so lines end at the same place on screen and on paper.
const DOCUMENT_FONT: &str = "Arial";

/// One point, in millimetres.
const PT_TO_MM: f32 = 25.4 / 72.0;
/// The height of a line of text, as a multiple of its font size.
const LINE_HEIGHT: f32 = 1.25;

/// One thing drawn on a page. Positions and sizes are millimetres from the top left of the
/// page, font sizes points, colours `0xRRGGBB`.
#[derive(Clone)]
pub enum PageShape {
    Rect { x: f32, y: f32, w: f32, h: f32, fill: Option<u32>, stroke: Option<(u32, f32)>, dashed: bool },
    /// A straight line, across or down.
    Line { x1: f32, y1: f32, x2: f32, y2: f32, color: u32, width: f32 },
    /// One line of text; (`x`, `y`) is the top left of its line box.
    Text { x: f32, y: f32, text: SharedString, size: f32, bold: bool, color: u32 },
    /// A variable of a template, shown by its name.
    Badge { x: f32, y: f32, w: f32, h: f32, label: SharedString, size: f32, bold: bool },
    /// A picture, stretched to its frame.
    Image { x: f32, y: f32, w: f32, h: f32, image: Arc<Image> },
    /// Where a picture goes while there is none to show.
    Placeholder { x: f32, y: f32, w: f32, h: f32 },
}

/// A page at a given size on screen. Nothing on it reacts to the mouse; what must (the handles
/// of an editor) is laid over it with [`PageView::overlay`].
#[derive(IntoElement)]
pub struct PageView {
    id: ElementId,
    width: f32,
    height: f32,
    scale: f32,
    grid: Option<f32>,
    shapes: Vec<PageShape>,
    overlays: Vec<AnyElement>,
}

impl PageView {
    /// A page `width` by `height` millimetres, drawn at `scale` pixels per millimetre.
    pub fn new(id: impl Into<ElementId>, width: f32, height: f32, scale: f32) -> Self {
        Self { id: id.into(), width, height, scale: scale.max(0.05), grid: None, shapes: Vec::new(), overlays: Vec::new() }
    }

    pub fn shapes(mut self, shapes: Vec<PageShape>) -> Self {
        self.shapes = shapes;
        self
    }

    /// Faint lines every `step` millimetres, to place things by eye.
    pub fn grid(mut self, step: f32) -> Self {
        self.grid = Some(step);
        self
    }

    /// Something laid over the page, positioned by the caller in pixels from its top left.
    pub fn overlay(mut self, element: impl IntoElement) -> Self {
        self.overlays.push(element.into_any_element());
        self
    }
}

/// A line is never thinner than what a screen can show.
fn at_least_a_pixel(width: f32) -> f32 {
    width.max(1.0)
}

impl RenderOnce for PageView {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = cx.ui().colors;
        let s = self.scale;
        let paper = rgb(0xffffff);
        let mut page = div()
            .id(self.id)
            .relative()
            .flex_none()
            .w(px(self.width * s))
            .h(px(self.height * s))
            .bg(paper)
            .border_1()
            .border_color(c.border)
            .overflow_hidden()
            .font_family(DOCUMENT_FONT);

        if let Some(step) = self.grid.filter(|step| *step * s >= 4.0) {
            let faint = rgb(0xf2f2f2);
            let mut at = step;
            while at < self.width {
                page = page.child(div().absolute().left(px(at * s)).top_0().bottom_0().w(px(1.)).bg(faint));
                at += step;
            }
            let mut at = step;
            while at < self.height {
                page = page.child(div().absolute().top(px(at * s)).left_0().right_0().h(px(1.)).bg(faint));
                at += step;
            }
        }

        for shape in self.shapes {
            let drawn: AnyElement = match shape {
                PageShape::Rect { x, y, w, h, fill, stroke, dashed } => div()
                    .absolute()
                    .left(px(x * s))
                    .top(px(y * s))
                    // A rule drawn as a very flat frame still shows.
                    .w(px(at_least_a_pixel(w * s)))
                    .h(px(at_least_a_pixel(h * s)))
                    .when_some(fill, |this, fill| this.bg(rgb(fill)))
                    .when_some(stroke, |this, (color, width)| {
                        this.border(px(at_least_a_pixel(width * s))).border_color(rgb(color)).when(dashed, |this| this.border_dashed())
                    })
                    .into_any_element(),
                PageShape::Line { x1, y1, x2, y2, color, width } => {
                    let thick = at_least_a_pixel(width * s);
                    let (left, top) = (x1.min(x2) * s, y1.min(y2) * s);
                    let (long, tall) = ((x2 - x1).abs() * s, (y2 - y1).abs() * s);
                    div().absolute().left(px(left)).top(px(top)).w(px(long.max(thick))).h(px(tall.max(thick))).bg(rgb(color)).into_any_element()
                }
                PageShape::Text { x, y, text, size, bold, color } => {
                    let font = size * PT_TO_MM * s;
                    div()
                        .absolute()
                        .left(px(x * s))
                        .top(px(y * s))
                        .whitespace_nowrap()
                        .text_size(px(font))
                        .line_height(px(font * LINE_HEIGHT))
                        .when(bold, |this| this.font_weight(FontWeight::BOLD))
                        .text_color(rgb(color))
                        .child(text)
                        .into_any_element()
                }
                PageShape::Badge { x, y, w, h, label, size, bold } => {
                    let font = size * PT_TO_MM * s;
                    div()
                        .absolute()
                        .left(px(x * s))
                        .top(px(y * s))
                        .w(px(w * s))
                        .h(px(h * s))
                        .flex()
                        .items_center()
                        .justify_center()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .bg(rgb(0xe3effa))
                        .border_1()
                        .border_color(rgb(0x7fb2e5))
                        .text_size(px(font))
                        .line_height(px(font * LINE_HEIGHT))
                        .when(bold, |this| this.font_weight(FontWeight::BOLD))
                        .text_color(rgb(0x0d4d8c))
                        .child(label)
                        .into_any_element()
                }
                PageShape::Image { x, y, w, h, image } => {
                    img(image).absolute().left(px(x * s)).top(px(y * s)).w(px(w * s)).h(px(h * s)).object_fit(ObjectFit::Fill).into_any_element()
                }
                PageShape::Placeholder { x, y, w, h } => div()
                    .absolute()
                    .left(px(x * s))
                    .top(px(y * s))
                    .w(px(w * s))
                    .h(px(h * s))
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .bg(rgb(0xfafafa))
                    .border_1()
                    .border_dashed()
                    .border_color(rgb(0xc8c8c8))
                    .text_size(px((2.8 * s).max(6.0)))
                    .text_color(rgb(0x8a8a8a))
                    .when(h * s >= 14.0, |this| this.child("Image"))
                    .into_any_element(),
            };
            page = page.child(drawn);
        }
        page.children(self.overlays)
    }
}
