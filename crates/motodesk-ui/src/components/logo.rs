//! The Motodesk mark: two linked bars. The lower one is drawn in the logo's ink, the upper one in
//! the owner's accent, so the mark follows the theme. The installer's icon is the same drawing
//! (`desktop-gpui/assets/icon-source.svg`).

use gpui_kit::{App, IntoElement, ParentElement, Pixels, RenderOnce, Styled, Window, div, px, svg};

use crate::theme::Themed;

/// The two halves, each a mask the interface tints.
const INK: &str = "logo/mark-ink.svg";
const ACCENT: &str = "logo/mark-accent.svg";

/// Width over height of the files' view box.
const RATIO: f32 = 210. / 131.;

#[derive(IntoElement, Default)]
pub struct Logo {
    tiled: bool,
}

impl Logo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Larger, on a square tile of the surface colour: for the accent panel, where the accent half
    /// of the mark would vanish.
    pub fn tiled(mut self) -> Self {
        self.tiled = true;
        self
    }
}

impl RenderOnce for Logo {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = &cx.ui().colors;
        let height: Pixels = if self.tiled { px(20.) } else { px(14.) };
        let width = height * RATIO;
        let half = |path: &'static str| svg().path(path).absolute().top_0().left_0().w(width).h(height);
        let mark = div()
            .relative()
            .flex_none()
            .w(width)
            .h(height)
            .child(half(INK).text_color(c.logo_ink))
            .child(half(ACCENT).text_color(c.accent));
        if !self.tiled {
            return mark;
        }
        div().flex().flex_none().items_center().justify_center().size(px(36.)).bg(c.bg_surface).child(mark)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AssetSource;

    use super::*;
    use crate::Assets;

    #[test]
    fn both_halves_of_the_mark_ship_with_the_kit() {
        for path in [INK, ACCENT] {
            let file = Assets.load(path).unwrap().unwrap_or_else(|| panic!("{path} is missing"));
            assert!(String::from_utf8_lossy(&file).contains("viewBox=\"-4 -2 210 131\""), "{path}");
        }
    }
}
