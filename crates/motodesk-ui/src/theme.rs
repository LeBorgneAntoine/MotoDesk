//! Colours and the live theme.
//!
//! The values of `desktop/src/lib/styles/colors.css` and `elevation.css`. A component reads
//! them from the theme (`cx.ui().colors`) and never writes a colour of its own.
//!
//! The accent ramp is derived from one base colour, so the light theme, the dark theme and the
//! owner's own accent stay in step.

use gpui_kit::component::{Theme as KitTheme, ThemeMode as KitMode};
use gpui_kit::{App, BoxShadow, Global, Hsla, Pixels, Rgba, SharedString, point, px, rgb, rgba};

use crate::tokens::{Density, Metrics};

/// The family every text uses. Embedded in the binary (see [`crate::assets`]).
pub const FONT_UI: &str = "Inter";
/// For references, serial numbers and amounts that must line up.
pub const FONT_MONO: &str = "Consolas";

/// Office blue: the accent until the owner picks another one.
pub const DEFAULT_ACCENT: u32 = 0x1976d2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

/// `color-mix(in srgb, a <share>, b)`: `share` of `a`, the rest of `b`.
pub fn mix(a: Rgba, share: f32, b: Rgba) -> Rgba {
    let rest = 1. - share;
    Rgba {
        r: a.r * share + b.r * rest,
        g: a.g * share + b.g * rest,
        b: a.b * share + b.b * rest,
        a: a.a * share + b.a * rest,
    }
}

/// Read `#rgb` or `#rrggbb`; anything else is not a colour.
pub fn parse_hex(raw: &str) -> Option<u32> {
    let hex = raw.trim().strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => u32::from_str_radix(hex, 16).ok(),
        3 => {
            let doubled: String = hex.chars().flat_map(|c| [c, c]).collect();
            u32::from_str_radix(&doubled, 16).ok()
        }
        _ => None,
    }
}

/// Every colour a component may use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    // Surfaces.
    pub bg_app: Hsla,
    pub bg_surface: Hsla,
    pub bg_elevated: Hsla,
    pub bg_toolbar: Hsla,
    pub bg_sidebar: Hsla,
    pub bg_titlebar: Hsla,
    pub bg_hover: Hsla,
    pub bg_active: Hsla,
    pub bg_selected: Hsla,

    // Text.
    pub text: Hsla,
    pub text_strong: Hsla,
    pub text_muted: Hsla,
    pub text_subtle: Hsla,
    pub text_on_accent: Hsla,
    pub text_disabled: Hsla,

    // Borders.
    pub border_subtle: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub border_input: Hsla,
    pub border_chrome: Hsla,
    pub border_focus: Hsla,

    // Accent ramp.
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_active: Hsla,
    pub accent_muted: Hsla,
    pub accent_soft: Hsla,
    pub accent_subtle: Hsla,

    // Brand: identity surfaces (auth panel, logo lockup).
    pub brand: Hsla,
    pub brand_strong: Hsla,
    pub brand_fg: Hsla,
    pub brand_fg_muted: Hsla,
    pub brand_fg_subtle: Hsla,
    pub brand_line: Hsla,
    pub logo_ink: Hsla,

    // Status bar and connection states.
    pub statusbar: Hsla,
    pub statusbar_fg: Hsla,
    pub state_ok: Hsla,
    pub state_idle: Hsla,
    pub state_syncing: Hsla,
    pub state_error: Hsla,

    // Window controls.
    pub window_close: Hsla,
    pub window_close_fg: Hsla,

    // Semantic.
    pub danger: Hsla,
    pub danger_soft: Hsla,
    pub success: Hsla,
    pub success_soft: Hsla,
    pub warning: Hsla,
    pub warning_soft: Hsla,
    pub info: Hsla,
    pub info_soft: Hsla,

    // Overlays.
    pub focus_ring: Hsla,
    /// Dimming behind the panel stack.
    pub scrim: Hsla,
    /// Dimming behind a modal.
    pub scrim_strong: Hsla,

    // Scrollbar.
    pub scrollbar_thumb: Hsla,
    pub scrollbar_thumb_hover: Hsla,

    /// How dark a shadow is: light surfaces need little, dark ones a lot.
    shadow_strength: f32,
}

impl Palette {
    pub fn new(mode: Mode, accent_base: u32) -> Self {
        let base = rgb(accent_base);
        let white = rgb(0xffffff);
        let black = rgb(0x000000);
        let h = |c: Rgba| -> Hsla { c.into() };

        match mode {
            Mode::Light => {
                let surface = rgb(0xffffff);
                let accent_soft = mix(base, 0.14, surface);
                Self {
                    bg_app: h(rgb(0xf3f3f3)),
                    bg_surface: h(surface),
                    bg_elevated: h(rgb(0xffffff)),
                    bg_toolbar: h(rgb(0xffffff)),
                    bg_sidebar: h(rgb(0xfafafa)),
                    bg_titlebar: h(rgb(0xf3f3f3)),
                    bg_hover: h(rgb(0xededed)),
                    bg_active: h(rgb(0xdcdcdc)),
                    bg_selected: h(accent_soft),

                    text: h(rgb(0x1a1a1a)),
                    text_strong: h(rgb(0x0d0d0d)),
                    text_muted: h(rgb(0x5a5a5a)),
                    text_subtle: h(rgb(0x8a8a8a)),
                    text_on_accent: h(white),
                    text_disabled: h(rgb(0xb0b0b0)),

                    border_subtle: h(rgb(0xececec)),
                    border: h(rgb(0xe0e0e0)),
                    border_strong: h(rgb(0xc8c8c8)),
                    border_input: h(rgb(0xd4d4d4)),
                    border_chrome: h(rgb(0xc4c4c4)),
                    border_focus: h(base),

                    accent: h(base),
                    accent_hover: h(mix(base, 0.86, black)),
                    accent_active: h(mix(base, 0.70, black)),
                    accent_muted: h(mix(base, 0.55, white)),
                    accent_soft: h(accent_soft),
                    accent_subtle: h(mix(base, 0.07, surface)),

                    brand: h(rgb(0x1976d2)),
                    brand_strong: h(rgb(0x0d47a1)),
                    brand_fg: h(white),
                    brand_fg_muted: h(rgba(0xffffffb8)),
                    brand_fg_subtle: h(rgba(0xffffff73)),
                    brand_line: h(rgba(0xffffff2e)),
                    logo_ink: h(black),

                    statusbar: h(base),
                    statusbar_fg: h(white),
                    state_ok: h(rgb(0x6ddf8e)),
                    state_idle: h(rgb(0xffd666)),
                    state_syncing: h(rgb(0x7ec8ff)),
                    state_error: h(rgb(0xff8a8a)),

                    window_close: h(rgb(0xe81123)),
                    window_close_fg: h(white),

                    danger: h(rgb(0xc62828)),
                    danger_soft: h(rgb(0xfdecea)),
                    success: h(rgb(0x2e7d32)),
                    success_soft: h(rgb(0xe6f4ea)),
                    warning: h(rgb(0xed6c02)),
                    warning_soft: h(rgb(0xfff4e5)),
                    info: h(rgb(0x0277bd)),
                    info_soft: h(rgb(0xe1f5fe)),

                    focus_ring: h(base),
                    scrim: h(rgba(0x0000002e)),
                    scrim_strong: h(rgba(0x0000004d)),

                    scrollbar_thumb: h(rgb(0xd4d4d4)),
                    scrollbar_thumb_hover: h(rgb(0xbdbdbd)),

                    shadow_strength: 1.,
                }
            }
            Mode::Dark => {
                // True-neutral greys (R=G=B).
                let app = rgb(0x0e0e0e);
                let surface = rgb(0x181818);
                let accent = mix(base, 0.72, white);
                let accent_soft = mix(base, 0.16, surface);
                Self {
                    bg_app: h(app),
                    bg_surface: h(surface),
                    bg_elevated: h(rgb(0x1f1f1f)),
                    bg_toolbar: h(rgb(0x1f1f1f)),
                    bg_sidebar: h(rgb(0x141414)),
                    bg_titlebar: h(rgb(0x0e0e0e)),
                    bg_hover: h(rgb(0x262626)),
                    bg_active: h(rgb(0x2e2e2e)),
                    bg_selected: h(accent_soft),

                    text: h(rgb(0xe8e8e8)),
                    text_strong: h(white),
                    text_muted: h(rgb(0xa3a3a3)),
                    text_subtle: h(rgb(0x7a7a7a)),
                    text_on_accent: h(white),
                    text_disabled: h(rgb(0x555555)),

                    border_subtle: h(rgb(0x222222)),
                    border: h(rgb(0x2e2e2e)),
                    border_strong: h(rgb(0x3c3c3c)),
                    border_input: h(rgb(0x333333)),
                    border_chrome: h(rgb(0x343434)),
                    border_focus: h(accent),

                    accent: h(accent),
                    accent_hover: h(mix(accent, 0.88, white)),
                    accent_active: h(mix(base, 0.82, black)),
                    accent_muted: h(mix(base, 0.42, white)),
                    accent_soft: h(accent_soft),
                    accent_subtle: h(mix(base, 0.08, app)),

                    brand: h(rgb(0x12508f)),
                    brand_strong: h(rgb(0x0a2f57)),
                    brand_fg: h(white),
                    brand_fg_muted: h(rgba(0xffffffb8)),
                    brand_fg_subtle: h(rgba(0xffffff73)),
                    brand_line: h(rgba(0xffffff2e)),
                    logo_ink: h(rgb(0xf2f2f2)),

                    statusbar: h(mix(base, 0.82, black)),
                    statusbar_fg: h(white),
                    state_ok: h(rgb(0x6ddf8e)),
                    state_idle: h(rgb(0xffd666)),
                    state_syncing: h(rgb(0x7ec8ff)),
                    state_error: h(rgb(0xff8a8a)),

                    window_close: h(rgb(0xe81123)),
                    window_close_fg: h(white),

                    danger: h(rgb(0xf05a5a)),
                    danger_soft: h(rgb(0x2a1616)),
                    success: h(rgb(0x5cb85f)),
                    success_soft: h(rgb(0x162016)),
                    warning: h(rgb(0xf0a23c)),
                    warning_soft: h(rgb(0x241c10)),
                    info: h(rgb(0x4aa8e0)),
                    info_soft: h(rgb(0x141c22)),

                    focus_ring: h(accent),
                    scrim: h(rgba(0x00000080)),
                    scrim_strong: h(rgba(0x000000a3)),

                    scrollbar_thumb: h(rgb(0x3d3d3d)),
                    scrollbar_thumb_hover: h(rgb(0x4a4a4a)),

                    shadow_strength: 3.6,
                }
            }
        }
    }

    fn shadow(&self, x: f32, y: f32, blur: f32, alpha: f32) -> Vec<BoxShadow> {
        let alpha = (alpha * self.shadow_strength).min(0.64);
        vec![BoxShadow {
            color: Hsla { h: 0., s: 0., l: 0., a: alpha },
            offset: point(px(x), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.),
            inset: false,
        }]
    }

    // Flat UI: a shadow exists only to separate a floating layer from the surface underneath.

    /// Menus and dropdowns.
    pub fn shadow_menu(&self) -> Vec<BoxShadow> {
        self.shadow(0., 6., 18., 0.16)
    }

    /// A modal dialog.
    pub fn shadow_modal(&self) -> Vec<BoxShadow> {
        self.shadow(0., 16., 40., 0.18)
    }

    /// A panel of the right-hand stack; `deep` for the ones stacked above the first.
    pub fn shadow_panel(&self, deep: bool) -> Vec<BoxShadow> {
        if deep { self.shadow(-12., 0., 32., 0.16) } else { self.shadow(-8., 0., 24., 0.12) }
    }

    /// A toast.
    pub fn shadow_toast(&self) -> Vec<BoxShadow> {
        self.shadow(0., 4., 14., 0.16)
    }

    /// A card under the pointer.
    pub fn shadow_card_hover(&self) -> Vec<BoxShadow> {
        self.shadow(0., 2., 8., 0.10)
    }
}

/// The live theme: what every component reads.
#[derive(Debug, Clone, PartialEq)]
pub struct UiTheme {
    pub mode: Mode,
    /// The owner's accent, as `0xrrggbb`.
    pub accent: u32,
    pub density: Density,
    pub colors: Palette,
    pub m: Metrics,
}

impl Global for UiTheme {}

impl UiTheme {
    pub fn new(mode: Mode, accent: u32, density: Density) -> Self {
        Self { mode, accent, density, colors: Palette::new(mode, accent), m: Metrics::for_density(density) }
    }

    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }

    /// Make this the theme of the whole application, and redraw every window.
    pub fn apply(self, cx: &mut App) {
        // The controls borrowed from GPUI Kit (text fields, tables, menus) read its own theme:
        // give it the same colours, and square corners.
        let kit_mode = if self.is_dark() { KitMode::Dark } else { KitMode::Light };
        if KitTheme::global(cx).mode != kit_mode {
            KitTheme::change(kit_mode, None, cx);
        }
        let (c, m) = (self.colors, self.m);
        KitTheme::update(cx, |kit| {
            kit.radius = px(0.);
            kit.radius_lg = px(0.);
            kit.window_radius = px(0.);
            kit.shadow = false;
            kit.font_family = SharedString::new_static(FONT_UI);
            // GPUI Kit sizes its controls in rems and writes their text at 0.875rem: this base
            // makes that text exactly the body size.
            kit.font_size = px(f32::from(m.fs_md) / 0.875);
            kit.mono_font_family = SharedString::new_static(FONT_MONO);
            kit.mono_font_size = m.fs_sm;

            let k = &mut kit.colors;
            k.background = c.bg_surface;
            k.foreground = c.text;
            k.border = c.border;
            k.input = c.border_input;
            k.ring = c.focus_ring;
            k.caret = c.text;
            k.selection = c.accent_muted.opacity(0.45);
            k.muted = c.bg_hover;
            k.muted_foreground = c.text_muted;
            k.accent = c.bg_hover;
            k.accent_foreground = c.text;
            k.primary = c.accent;
            k.primary_hover = c.accent_hover;
            k.primary_active = c.accent_active;
            k.primary_foreground = c.text_on_accent;
            k.secondary = c.bg_surface;
            k.secondary_hover = c.bg_hover;
            k.secondary_active = c.bg_active;
            k.secondary_foreground = c.text;
            k.danger = c.danger;
            k.danger_foreground = c.text_on_accent;
            k.success = c.success;
            k.warning = c.warning;
            k.info = c.info;
            k.link = c.accent;
            k.link_hover = c.accent_hover;
            k.link_active = c.accent_active;
            k.popover = c.bg_elevated;
            k.popover_foreground = c.text;
            k.list = c.bg_surface;
            k.list_hover = c.bg_hover;
            k.list_active = c.bg_selected;
            k.list_active_border = c.accent;
            k.list_even = c.bg_surface;
            k.list_head = c.bg_surface;
            k.table = c.bg_surface;
            k.table_hover = c.bg_hover;
            k.table_active = c.bg_selected;
            k.table_active_border = c.accent;
            k.table_even = c.bg_surface;
            k.table_head = c.bg_surface;
            k.table_head_foreground = c.text_muted;
            k.table_row_border = c.border_subtle;
            k.title_bar = c.bg_titlebar;
            k.title_bar_border = c.border_chrome;
            k.sidebar = c.bg_sidebar;
            k.sidebar_border = c.border;
            k.sidebar_foreground = c.text;
            k.scrollbar = c.bg_surface.opacity(0.);
            k.scrollbar_thumb = c.scrollbar_thumb;
            k.scrollbar_thumb_hover = c.scrollbar_thumb_hover;
            k.switch = c.border_strong;
            k.switch_thumb = c.bg_surface;
            k.overlay = c.scrim_strong;
            k.window_border = c.border_chrome;
        });
        cx.set_global(self);
        cx.refresh_windows();
    }

    /// Change one aspect (mode, accent, density) and apply the result.
    pub fn update(cx: &mut App, change: impl FnOnce(&mut UiTheme)) {
        let mut next = cx.ui().clone();
        change(&mut next);
        UiTheme::new(next.mode, next.accent, next.density).apply(cx);
    }
}

/// `cx.ui()`: the theme, from anywhere a component is drawn.
pub trait Themed {
    fn ui(&self) -> &UiTheme;
}

impl Themed for App {
    fn ui(&self) -> &UiTheme {
        self.global::<UiTheme>()
    }
}

/// A font size with the line height that goes with it.
pub fn line(size: Pixels, ratio: f32) -> Pixels {
    px((f32::from(size) * ratio).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_accent_is_read_from_the_two_hex_forms_and_nothing_else() {
        assert_eq!(parse_hex("#1976d2"), Some(0x1976d2));
        assert_eq!(parse_hex(" #FA0 "), Some(0xffaa00));
        assert_eq!(parse_hex("1976d2"), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#zzzzzz"), None);
    }

    #[test]
    fn mixing_is_linear_in_srgb() {
        let grey = mix(rgb(0xffffff), 0.5, rgb(0x000000));
        assert!((grey.r - 0.5).abs() < 1e-6 && (grey.a - 1.).abs() < 1e-6);
        assert_eq!(mix(rgb(0x1976d2), 1., rgb(0x000000)), rgb(0x1976d2));
    }

    #[test]
    fn the_accent_ramp_follows_the_owners_colour_in_both_themes() {
        let light = Palette::new(Mode::Light, 0xd32f2f);
        let dark = Palette::new(Mode::Dark, 0xd32f2f);
        let red: Hsla = rgb(0xd32f2f).into();
        assert_eq!(light.accent, red);
        assert_eq!(light.statusbar, red);
        // Dark lifts the accent toward white so it holds its contrast on a dark surface.
        assert!(dark.accent.l > red.l);
        assert_ne!(light.accent_soft, dark.accent_soft);
    }
}
