//! Small layout and text helpers shared by the components and the pages.

use gpui_kit::{Div, Hsla, Pixels, Styled, div, px};

use crate::tokens::line_height;

/// A horizontal flex box, children centred on the cross axis.
pub fn row() -> Div {
    div().flex().flex_row().items_center()
}

/// A vertical flex box.
pub fn col() -> Div {
    div().flex().flex_col()
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

    /// A hairline under the element.
    fn hairline_bottom(self, color: Hsla) -> Self {
        self.border_b_1().border_color(color)
    }

    /// A hairline above the element.
    fn hairline_top(self, color: Hsla) -> Self {
        self.border_t_1().border_color(color)
    }
}

impl<T: Styled> StyleExt for T {}
