//! Small layout and text helpers shared by the components and the pages.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{Div, Hsla, IntoElement, ParentElement as _, Pixels, Styled, div, px};

use crate::tokens::line_height;

/// A horizontal flex box, children centred on the cross axis.
pub fn row() -> Div {
    div().flex().flex_row().items_center()
}

/// A vertical flex box.
pub fn col() -> Div {
    div().flex().flex_col()
}

/// A field with Motodesk's focus mark: while it has the keyboard, its border takes the accent
/// (the Kit does that) and its bottom edge is drawn twice as thick (this does). No halo around
/// the field: it stays inside its own box and cannot be clipped.
pub(crate) fn with_focus_mark(field: impl IntoElement, focused: bool, color: Hsla) -> Div {
    div().relative().w_full().child(field).when(focused, |this| {
        this.child(div().absolute().left_0().right_0().bottom_0().h(px(2.)).bg(color))
    })
}

pub trait StyleExt: Styled + Sized {
    /// A font size with the line height that goes with body text.
    fn fs(self, size: Pixels) -> Self {
        self.text_size(size).line_height(px((f32::from(size) * line_height::NORMAL).round()))
    }

    /// A font size for a single line that must not add height of its own (labels in controls).
    fn fs_tight(self, size: Pixels) -> Self {
        self.text_size(size).line_height(px((f32::from(size) * line_height::TIGHT).round()))
    }

    /// One line, cut with an ellipsis.
    fn one_line(self) -> Self {
        self.whitespace_nowrap().overflow_hidden().text_ellipsis()
    }

    /// A hairline under the element. It sets the element's border colour, and an element has
    /// only one: do not combine it with a border of another colour on the same element.
    fn hairline_bottom(self, color: Hsla) -> Self {
        self.border_b_1().border_color(color)
    }

    /// A hairline above the element.
    fn hairline_top(self, color: Hsla) -> Self {
        self.border_t_1().border_color(color)
    }
}

impl<T: Styled> StyleExt for T {}
