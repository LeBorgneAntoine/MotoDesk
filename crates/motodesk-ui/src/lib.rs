//! Motodesk's UI kit: the single source of controls for the native interface.
//!
//! A screen imports from this crate only. If something is missing, add a generic component
//! here rather than hand-rolling it in a page: that is what keeps forty screens looking like
//! one application.
//!
//! - [`tokens`]: sizes (spacing, control heights, type ramp, chrome).
//! - [`theme`]: colours, light and dark, and the live theme every component reads (`cx.ui()`).
//! - [`components`]: the controls. One file each, a builder, no state of their own unless the
//!   doc says so.
//!
//! Two rules a page must follow (checked by `desktop-gpui/tests/ui_rules.rs`): no colour
//! written by hand, and no rounded corner. Every corner in Motodesk is square.
//!
//! Text fields, tables and menus are GPUI Kit's, themed here and wrapped so that a page never
//! names them directly.

pub mod assets;
pub mod components;
pub mod style;
pub mod theme;
pub mod tokens;

pub use assets::Assets;
pub use components::*;
pub use style::{StyleExt, col, row};
pub use theme::{DEFAULT_ACCENT, FONT_MONO, FONT_UI, Mode, Palette, Themed, UiTheme, mix, parse_hex};
pub use tokens::{Density, Metrics};

/// GPUI itself, for the pages: elements (`div`), contexts, entities, events.
pub use gpui_kit as gpui;

/// The application object, with the kit's fonts and icons.
pub fn application() -> gpui_kit::Application {
    gpui_kit::application().with_assets(Assets)
}

/// Call once, first thing in `Application::run`.
pub fn init(cx: &mut gpui_kit::App) {
    gpui_kit::init(cx);
    if let Err(e) = cx.text_system().add_fonts(assets::fonts()) {
        // Windows falls back to Segoe UI: readable, only not the intended face.
        eprintln!("motodesk-ui: the interface font could not be loaded: {e}");
    }
    UiTheme::new(Mode::Light, DEFAULT_ACCENT, Density::Regular).apply(cx);
}
