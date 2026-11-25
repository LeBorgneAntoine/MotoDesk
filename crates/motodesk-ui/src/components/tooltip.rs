//! The small label that appears when the pointer rests on a control.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{AnyView, App, SharedString, Window};

/// For `.tooltip(...)` on any interactive element: `div().id("x").tooltip(tooltip("Supprimer"))`.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |window, cx| Tooltip::new(text.clone()).build(window, cx)
}
