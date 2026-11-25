//! An icon from the Lucide set.

use gpui_kit::{App, Hsla, IntoElement, Pixels, RenderOnce, Styled, Window, svg};

use crate::theme::Themed;

/// Every Lucide icon, by name: `IconName::Users`, `IconName::Trash2`, ...
pub use gpui_kit::assets::IconName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconSize {
    /// 14px: inside tags, table cells, small buttons.
    Sm,
    /// 16px: the default, beside body text.
    #[default]
    Md,
    /// 20px: toolbars and the sidebar.
    Lg,
    /// 28px: empty states.
    Xl,
}

/// Draws in the colour of the surrounding text unless told otherwise.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: IconSize,
    exact: Option<Pixels>,
    color: Option<Hsla>,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self { name, size: IconSize::default(), exact: None, color: None }
    }

    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    pub fn sm(self) -> Self {
        self.size(IconSize::Sm)
    }

    pub fn lg(self) -> Self {
        self.size(IconSize::Lg)
    }

    pub fn xl(self) -> Self {
        self.size(IconSize::Xl)
    }

    /// A size outside the scale, for the few places that draw their own geometry.
    pub fn exact(mut self, size: Pixels) -> Self {
        self.exact = Some(size);
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let m = &cx.ui().m;
        let size = self.exact.unwrap_or(match self.size {
            IconSize::Sm => m.icon_sm,
            IconSize::Md => m.icon_md,
            IconSize::Lg => m.icon_lg,
            IconSize::Xl => m.icon_xl,
        });
        let color = self.color.unwrap_or(window.text_style().color);
        svg().path(self.name.path()).flex_none().size(size).text_color(color)
    }
}
