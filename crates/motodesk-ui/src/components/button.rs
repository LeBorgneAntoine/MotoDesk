//! The push button.

use std::rc::Rc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::spinner::Spinner as KitSpinner;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, CursorStyle, ElementId, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, transparent_black,
};

use crate::components::icon::{Icon, IconName, IconSize};
use crate::components::tooltip::tooltip;
use crate::style::StyleExt as _;
use crate::theme::Themed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// The one action a screen or a dialog is there for.
    Primary,
    /// Everything else.
    #[default]
    Secondary,
    /// No chrome until hovered: toolbars, rows, dismissals.
    Ghost,
    /// Deletes or cannot be undone.
    Danger,
    /// Reads as a link inside a sentence or an alert.
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Sm,
    /// Same height as a text field, so a form row lines up.
    #[default]
    Md,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    icon_after: Option<IconName>,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    loading: bool,
    full_width: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            icon_after: None,
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            disabled: false,
            loading: false,
            full_width: false,
            tooltip: None,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// An icon after the label (a chevron, an arrow).
    pub fn icon_after(mut self, icon: IconName) -> Self {
        self.icon_after = Some(icon);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn danger(self) -> Self {
        self.variant(ButtonVariant::Danger)
    }

    pub fn link(self) -> Self {
        self.variant(ButtonVariant::Link)
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.size(ButtonSize::Sm)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Work in progress: shows a spinner and ignores clicks.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let inert = self.disabled || self.loading;
        let clear = transparent_black();

        // (background, border, text) at rest, hovered, pressed.
        let (rest, hover, active) = match self.variant {
            ButtonVariant::Primary => (
                (c.accent, c.accent, c.text_on_accent),
                (c.accent_hover, c.accent_hover, c.text_on_accent),
                (c.accent_active, c.accent_active, c.text_on_accent),
            ),
            ButtonVariant::Secondary => (
                (c.bg_app, clear, c.text),
                (c.bg_hover, c.border, c.text),
                (c.bg_active, c.border_strong, c.text),
            ),
            ButtonVariant::Ghost => {
                ((clear, clear, c.text), (c.bg_hover, clear, c.text), (c.bg_active, clear, c.text))
            }
            ButtonVariant::Danger => (
                (c.danger, c.danger, c.text_on_accent),
                (c.danger.opacity(0.92), c.danger, c.text_on_accent),
                (c.danger.opacity(0.84), c.danger, c.text_on_accent),
            ),
            ButtonVariant::Link => {
                ((clear, clear, c.accent), (clear, clear, c.accent_hover), (clear, clear, c.accent_active))
            }
        };

        let is_link = self.variant == ButtonVariant::Link;
        let (height, font, pad) = match self.size {
            ButtonSize::Sm => (m.control_h_sm, m.fs_sm, m.control_px_sm),
            ButtonSize::Md => (m.control_h_lg, m.fs_lg, m.control_px_lg),
        };
        let icon_size = if self.size == ButtonSize::Sm { IconSize::Sm } else { IconSize::Md };

        let content = div()
            .flex()
            .items_center()
            .justify_center()
            .gap(m.space_2)
            .when(self.loading, |this| this.opacity(0.6))
            .when_some(self.icon, |this, icon| this.child(Icon::new(icon).size(icon_size)))
            .when_some(self.label.clone(), |this, label| this.child(label))
            .when_some(self.icon_after, |this, icon| this.child(Icon::new(icon).size(icon_size)));

        let on_click = self.on_click;
        BaseButton::new(self.id)
            .disabled(inert)
            .flex_none()
            .gap(m.space_2)
            .border_1()
            .bg(rest.0)
            .border_color(rest.1)
            .text_color(rest.2)
            .font_weight(FontWeight::MEDIUM)
            .fs_tight(font)
            .whitespace_nowrap()
            .cursor(if inert { CursorStyle::OperationNotAllowed } else { CursorStyle::PointingHand })
            .when(!is_link, |this| this.h(height).px(pad))
            .when(self.full_width, |this| this.w_full())
            .hover(|s| {
                let s = s.bg(hover.0).border_color(hover.1).text_color(hover.2);
                if is_link { s.underline() } else { s }
            })
            .active(|s| s.bg(active.0).border_color(active.1).text_color(active.2))
            .focus_visible(|s| s.border_color(if rest.0 == c.accent || rest.0 == c.danger { c.text_strong } else { c.focus_ring }))
            .styles(|styles| {
                styles.disabled(|s| {
                    // Ghost and link buttons stay chrome-free even when disabled.
                    if matches!(self.variant, ButtonVariant::Ghost | ButtonVariant::Link) {
                        s.text_color(c.text_disabled)
                    } else {
                        s.bg(c.bg_app).border_color(c.border_subtle).text_color(c.text_disabled)
                    }
                })
            })
            .when(self.loading, |this| {
                this.child(KitSpinner::new().small().color(if self.disabled { c.text_disabled } else { c.text_muted }))
            })
            .child(content)
            .when_some(self.tooltip, |this, text| this.tooltip(tooltip(text)))
            .when_some(on_click, |this, handler| this.on_click(move |event, window, cx| handler(event, window, cx)))
    }
}
