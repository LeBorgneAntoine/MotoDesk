//! Sizes: spacing, control heights, the type ramp, the chrome.
//!
//! A component reads these numbers from the theme (`cx.ui().m`) and never writes a size of its
//! own, so the three densities stay coherent.
//!
//! There is no radius here: every corner in Motodesk is square.

use gpui_kit::{Pixels, px};

/// How tightly the interface is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    #[default]
    Regular,
    /// Preferences › Affichage compact: more rows on a small screen.
    Compact,
    /// The till: large hit targets for a touch screen. Wins over compact.
    Pos,
}

/// Every size a component may use, resolved for one density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    // Spacing scale (4px base).
    pub space_1: Pixels,
    pub space_2: Pixels,
    pub space_3: Pixels,
    pub space_4: Pixels,
    pub space_5: Pixels,
    pub space_6: Pixels,
    pub space_7: Pixels,
    pub space_8: Pixels,
    pub space_9: Pixels,

    // Control heights.
    pub control_h_xs: Pixels,
    pub control_h_sm: Pixels,
    pub control_h_md: Pixels,
    pub control_h_lg: Pixels,
    pub control_h_xl: Pixels,

    // Control horizontal padding.
    pub control_px_sm: Pixels,
    pub control_px_md: Pixels,
    pub control_px_lg: Pixels,

    // Chrome.
    pub titlebar_h: Pixels,
    pub toolbar_h: Pixels,
    pub statusbar_h: Pixels,
    pub sidebar_w: Pixels,
    pub sidebar_w_collapsed: Pixels,
    pub filter_rail_w: Pixels,

    // Panel stack (cascading right panels).
    pub panel_w_sm: Pixels,
    pub panel_w_md: Pixels,
    pub panel_w_lg: Pixels,
    pub panel_stack_offset: Pixels,
    pub panel_spine_w: Pixels,

    // Borders.
    pub border_w: Pixels,
    pub border_w_strong: Pixels,

    // Icons.
    pub icon_sm: Pixels,
    pub icon_md: Pixels,
    pub icon_lg: Pixels,
    pub icon_xl: Pixels,

    // Type ramp.
    /// Dot labels, table units, keyboard hints (sparingly, uppercase).
    pub fs_micro: Pixels,
    /// Captions, meta lines, tags.
    pub fs_xs: Pixels,
    /// Secondary body, table cells.
    pub fs_sm: Pixels,
    /// Default body.
    pub fs_md: Pixels,
    /// Emphasised body, section titles.
    pub fs_lg: Pixels,
    /// Panel titles.
    pub fs_xl: Pixels,
    /// Page titles.
    pub fs_2xl: Pixels,
    /// Screen headlines.
    pub fs_3xl: Pixels,
    /// Hero numbers (KPI values).
    pub fs_4xl: Pixels,
    /// Identity lockup only (auth panel).
    pub fs_brand: Pixels,
}

impl Metrics {
    pub fn for_density(density: Density) -> Self {
        let mut m = Self {
            space_1: px(4.),
            space_2: px(8.),
            space_3: px(12.),
            space_4: px(16.),
            space_5: px(20.),
            space_6: px(24.),
            space_7: px(32.),
            space_8: px(40.),
            space_9: px(48.),

            control_h_xs: px(20.),
            control_h_sm: px(24.),
            control_h_md: px(28.),
            control_h_lg: px(32.),
            control_h_xl: px(38.),

            control_px_sm: px(10.),
            control_px_md: px(12.),
            control_px_lg: px(16.),

            titlebar_h: px(40.),
            toolbar_h: px(48.),
            statusbar_h: px(28.),
            sidebar_w: px(200.),
            sidebar_w_collapsed: px(52.),
            filter_rail_w: px(240.),

            panel_w_sm: px(420.),
            panel_w_md: px(600.),
            panel_w_lg: px(840.),
            panel_stack_offset: px(32.),
            panel_spine_w: px(40.),

            border_w: px(1.),
            border_w_strong: px(2.),

            icon_sm: px(14.),
            icon_md: px(16.),
            icon_lg: px(20.),
            icon_xl: px(28.),

            fs_micro: px(10.),
            fs_xs: px(11.),
            fs_sm: px(12.),
            fs_md: px(13.),
            fs_lg: px(14.),
            fs_xl: px(16.),
            fs_2xl: px(18.),
            fs_3xl: px(22.),
            fs_4xl: px(28.),
            fs_brand: px(26.),
        };
        match density {
            Density::Regular => {}
            Density::Compact => {
                m.space_2 = px(6.);
                m.space_3 = px(8.);
                m.space_4 = px(12.);
                m.control_h_lg = px(28.);
                m.control_h_xl = px(32.);
                m.titlebar_h = px(34.);
                m.toolbar_h = px(40.);
                m.statusbar_h = px(24.);
                m.panel_stack_offset = px(24.);
            }
            Density::Pos => {
                // The till: every large control is a 52px touch target.
                m.control_h_lg = px(52.);
                m.control_h_xl = px(52.);
                m.toolbar_h = px(52.);
                m.filter_rail_w = px(220.);
                m.fs_xs = px(12.);
                m.fs_sm = px(13.);
                m.fs_md = px(15.);
                m.fs_lg = px(16.);
                m.fs_xl = px(18.);
                m.fs_2xl = px(22.);
                m.fs_3xl = px(26.);
                m.fs_4xl = px(32.);
            }
        }
        m
    }
}

/// Line heights, as multiples of the font size.
pub mod line_height {
    pub const TIGHT: f32 = 1.20;
    pub const NORMAL: f32 = 1.40;
    pub const RELAXED: f32 = 1.55;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_tightens_the_chrome_and_the_till_enlarges_the_targets() {
        let regular = Metrics::for_density(Density::Regular);
        let compact = Metrics::for_density(Density::Compact);
        let pos = Metrics::for_density(Density::Pos);
        assert!(compact.toolbar_h < regular.toolbar_h && compact.space_3 < regular.space_3);
        assert_eq!(pos.control_h_lg, px(52.));
        assert!(pos.fs_md > regular.fs_md);
        // Small controls are the same everywhere: only the large ones grow or shrink.
        assert_eq!(compact.control_h_md, regular.control_h_md);
    }
}
