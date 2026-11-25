//! What floats above a page: confirmations, modal dialogs, toasts.
//!
//! These are functions, not elements: a screen asks the window to show one, from an event
//! handler. The window keeps the keyboard inside the dialog and gives focus back when it closes.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::ButtonVariant as KitButtonVariant;
use gpui_kit::component::notification::Notification;
use gpui_kit::{AnyElement, App, IntoElement, ParentElement as _, Pixels, SharedString, Styled as _, Window, div, px};

use crate::style::{StyleExt as _, col};
use crate::theme::Themed;

// ---- Confirmation ---------------------------------------------------------------------------

/// A question with two answers. Build it, then [`Confirm::open`].
pub struct Confirm {
    title: SharedString,
    message: SharedString,
    confirm_label: SharedString,
    cancel_label: SharedString,
    danger: bool,
}

impl Confirm {
    pub fn new(title: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: "Confirmer".into(),
            cancel_label: "Annuler".into(),
            danger: false,
        }
    }

    pub fn confirm_label(mut self, label: impl Into<SharedString>) -> Self {
        self.confirm_label = label.into();
        self
    }

    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// The confirming button is red: the action deletes or cannot be undone.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// Ask. `on_confirm` runs when the user confirms; cancelling does nothing.
    pub fn open(self, window: &mut Window, cx: &mut App, on_confirm: impl Fn(&mut Window, &mut App) + 'static) {
        let Self { title, message, confirm_label, cancel_label, danger } = self;
        let on_confirm = Rc::new(on_confirm);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let on_confirm = on_confirm.clone();
            alert
                .confirm()
                .title(title.clone())
                .description(message.clone())
                .ok_text(confirm_label.clone())
                .cancel_text(cancel_label.clone())
                .ok_variant(if danger { KitButtonVariant::Danger } else { KitButtonVariant::Primary })
                .on_ok(move |_, window, cx| {
                    on_confirm(window, cx);
                    true
                })
        });
    }
}

// ---- Modal ----------------------------------------------------------------------------------

/// A dialog that holds a small form or a message. Build it, then [`Modal::open`]; close it
/// with [`close_modal`] (the cross and Escape do too).
pub struct Modal {
    title: SharedString,
    width: Pixels,
    close_on_overlay: bool,
    locked: bool,
}

impl Modal {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { title: title.into(), width: px(480.), close_on_overlay: true, locked: false }
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    /// A click outside does not close it: for a form that would lose what was typed.
    pub fn keep_on_overlay_click(mut self) -> Self {
        self.close_on_overlay = false;
        self
    }

    /// No cross, no Escape, no click outside: it closes only when the code closes it. For the
    /// rare step that must not be skipped.
    pub fn locked(mut self) -> Self {
        self.locked = true;
        self.close_on_overlay = false;
        self
    }

    /// Show it. `body` and `footer` are drawn each frame, so they can read live state; the
    /// footer holds the buttons, right-aligned.
    pub fn open(
        self,
        window: &mut Window,
        cx: &mut App,
        body: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        footer: impl Fn(&mut Window, &mut App) -> Vec<AnyElement> + 'static,
    ) {
        let Self { title, width, close_on_overlay, locked } = self;
        window.open_dialog(cx, move |dialog, window, cx| {
            let m = cx.ui().m;
            let buttons = footer(window, cx);
            dialog
                .title(title.clone())
                .width(width)
                .overlay_closable(close_on_overlay)
                .close_button(!locked)
                .keyboard(!locked)
                // Enter belongs to the form inside (its fields submit it): it must not close the
                // dialog before the answer is back. Escape and the cross still close it.
                .on_ok(|_, _, _| false)
                .child(col().gap(m.space_4).fs(m.fs_md).child(body(window, cx)))
                .footer(div().flex().justify_end().gap(m.space_2).children(buttons))
        });
    }
}

/// Close the dialog on top.
pub fn close_modal(window: &mut Window, cx: &mut App) {
    window.close_dialog(cx);
}

// ---- Toasts ---------------------------------------------------------------------------------

/// A short message that goes away on its own: the result of what the user just did.
pub mod toast {
    use super::*;

    pub fn info(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::info(message), cx);
    }

    pub fn success(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::success(message), cx);
    }

    /// Stays until dismissed: a failure must not vanish before it is read.
    pub fn error(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(message).autohide(false), cx);
    }
}

/// Keep `into_any_element` in scope for the callers of [`Modal::open`].
pub fn element(e: impl IntoElement) -> AnyElement {
    e.into_any_element()
}
