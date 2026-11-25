//! The panel stack: detail and edit panels that slide in from the right and cascade, so
//! opening a vehicle from a customer keeps the customer in view behind it.
//!
//! Three parts:
//! - [`PanelStack`] is the model: which panels are open, in which order. A page owns one.
//! - [`Panel`] draws one layer. The page that opened it supplies its body.
//! - [`PanelHost`] is the page's overlay: the scrim, Escape, and the question asked before a
//!   panel with unsaved edits is closed. It holds the panels as children.

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::FocusTrapElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, CursorStyle, ElementId, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    px,
};

use crate::components::content::{Heading, HeadingSize, Status, Tag};
use crate::components::controls::{IconButton, IconButtonSize};
use crate::components::icon::{Icon, IconName};
use crate::components::overlay::Confirm;
use crate::components::tooltip::tooltip;
use crate::style::{StyleExt as _, col, row};
use crate::theme::Themed;

/// How many panels can be open at once. Opening one more drops the oldest.
pub const MAX_DEPTH: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelWidth {
    /// 420px: a short form.
    Sm,
    /// 600px: a detail page.
    #[default]
    Md,
    /// 840px: a document with lines.
    Lg,
}

/// One open panel.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelLayer {
    /// `kind:key`, unique in the stack.
    pub id: SharedString,
    /// What the panel shows: `client`, `client-edit`, `vehicle`...
    pub kind: SharedString,
    /// Which one: an id, or `new`.
    pub key: SharedString,
    pub title: SharedString,
    pub subtitle: Option<SharedString>,
    pub icon: Option<IconName>,
    pub width: PanelWidth,
    /// Holds edits that closing would lose.
    pub dirty: bool,
}

impl PanelLayer {
    pub fn new(kind: impl Into<SharedString>, key: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        let (kind, key) = (kind.into(), key.into());
        Self {
            id: layer_id(&kind, &key),
            kind,
            key,
            title: title.into(),
            subtitle: None,
            icon: None,
            width: PanelWidth::default(),
            dirty: false,
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn width(mut self, width: PanelWidth) -> Self {
        self.width = width;
        self
    }
}

fn layer_id(kind: &str, key: &str) -> SharedString {
    format!("{kind}:{key}").into()
}

/// The open panels of one page, oldest first: the last one is on top.
pub struct PanelStack {
    layers: Vec<PanelLayer>,
    expanded: bool,
    /// A panel with unsaved edits that the user asked to close.
    pending_close: Option<SharedString>,
    focus: Option<FocusHandle>,
    /// The panels took the keyboard for this opening (see `PanelHost`).
    took_keyboard: std::cell::Cell<bool>,
}

impl PanelStack {
    /// A stack outside any window, for the rules alone (tests).
    pub fn detached() -> Self {
        Self {
            layers: Vec::new(),
            expanded: false,
            pending_close: None,
            focus: None,
            took_keyboard: std::cell::Cell::new(false),
        }
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { focus: Some(cx.focus_handle()), ..Self::detached() }
    }

    pub fn layers(&self) -> &[PanelLayer] {
        &self.layers
    }

    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    pub fn is_open(&self) -> bool {
        !self.layers.is_empty()
    }

    pub fn top(&self) -> Option<&PanelLayer> {
        self.layers.last()
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn pending_close(&self) -> Option<&SharedString> {
        self.pending_close.as_ref()
    }

    pub fn find(&self, kind: &str, key: &str) -> Option<&PanelLayer> {
        let id = layer_id(kind, key);
        self.layers.iter().find(|l| l.id == id)
    }

    /// The key of the open panel of this kind, if there is one.
    pub fn key_of(&self, kind: &str) -> Option<SharedString> {
        self.layers.iter().rev().find(|l| l.kind.as_ref() == kind).map(|l| l.key.clone())
    }

    /// How far a layer is from the top: 0 for the top one. `None` when it is not open.
    pub fn offset_of(&self, id: &str) -> Option<usize> {
        let i = self.layers.iter().position(|l| l.id.as_ref() == id)?;
        Some(self.layers.len() - 1 - i)
    }

    /// Open a panel above the others. Opening one that is already open comes back to it,
    /// closing what was above.
    pub fn open(&mut self, layer: PanelLayer) -> SharedString {
        let id = layer.id.clone();
        if let Some(existing) = self.layers.iter().position(|l| l.id == id) {
            self.layers.truncate(existing + 1);
            return id;
        }
        self.layers.push(layer);
        if self.layers.len() > MAX_DEPTH {
            let extra = self.layers.len() - MAX_DEPTH;
            self.layers.drain(..extra);
        }
        id
    }

    /// Put a panel in the place of the top one (a form replacing the page it edits).
    pub fn replace_top(&mut self, layer: PanelLayer) -> SharedString {
        if self.layers.is_empty() {
            return self.open(layer);
        }
        let id = layer.id.clone();
        *self.layers.last_mut().expect("not empty") = layer;
        id
    }

    /// Change what a layer shows in its header (its title once the record is loaded).
    pub fn retitle(&mut self, id: &str, title: impl Into<SharedString>, subtitle: Option<SharedString>) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.id.as_ref() == id) {
            layer.title = title.into();
            layer.subtitle = subtitle;
        }
    }

    pub fn set_dirty(&mut self, id: &str, dirty: bool) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.id.as_ref() == id) {
            layer.dirty = dirty;
        }
    }

    /// Close the top panel, whatever it holds.
    pub fn close(&mut self) {
        self.layers.pop();
        if self.layers.is_empty() {
            self.expanded = false;
        }
    }

    /// Close everything above a layer (a click on its spine).
    pub fn close_above(&mut self, id: &str) {
        if let Some(i) = self.layers.iter().position(|l| l.id.as_ref() == id) {
            self.layers.truncate(i + 1);
        }
    }

    pub fn close_all(&mut self) {
        self.layers.clear();
        self.expanded = false;
        self.pending_close = None;
    }

    /// The user asks to close the top panel. With unsaved edits it stays open and the question
    /// is pending; otherwise it closes. Returns whether it closed.
    pub fn request_close(&mut self) -> bool {
        let Some(top) = self.layers.last() else { return false };
        if top.dirty {
            self.pending_close = Some(top.id.clone());
            return false;
        }
        self.close();
        true
    }

    /// "Abandonner": drop the edits and close.
    pub fn confirm_close(&mut self) {
        let Some(id) = self.pending_close.take() else { return };
        self.set_dirty(&id, false);
        if self.top().is_some_and(|l| l.id == id) {
            self.close();
        }
    }

    pub fn cancel_close(&mut self) {
        self.pending_close = None;
    }

    pub fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }

    /// `client:9f3a>vehicle:77b1`: the stack as one word, to find it again later.
    pub fn serialize(&self) -> String {
        self.layers.iter().map(|l| l.id.as_ref()).collect::<Vec<_>>().join(">")
    }

    /// Read [`Self::serialize`] back into `(kind, key)` pairs.
    pub fn parse(raw: &str) -> Vec<(String, String)> {
        raw.split('>')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .filter_map(|part| {
                let (kind, key) = part.split_once(':')?;
                (!kind.is_empty()).then(|| (kind.to_string(), key.to_string()))
            })
            .take(MAX_DEPTH)
            .collect()
    }
}

/// The user asks to close the top panel (cross, Escape, a click beside it). A panel with
/// unsaved edits asks first; the others close at once.
pub fn request_close(stack: &Entity<PanelStack>, window: &mut Window, cx: &mut App) {
    let dirty = stack.read(cx).top().is_some_and(|layer| layer.dirty);
    if !dirty {
        stack.update(cx, |stack, cx| {
            stack.close();
            cx.notify();
        });
        return;
    }
    let stack = stack.clone();
    Confirm::new(
        "Abandonner les modifications ?",
        "Ce panneau contient des modifications non enregistrées. Elles seront perdues.",
    )
    .confirm_label("Abandonner")
    .cancel_label("Continuer la saisie")
    .danger()
    .open(window, cx, move |_, cx| {
        stack.update(cx, |stack, cx| {
            stack.close();
            cx.notify();
        })
    });
}

// ---- Panel ----------------------------------------------------------------------------------

/// One layer of the stack. Children are the body, which scrolls.
#[derive(IntoElement)]
pub struct Panel {
    stack: Entity<PanelStack>,
    layer_id: SharedString,
    badge: Option<(SharedString, Status)>,
    actions: Vec<AnyElement>,
    tabs: Option<AnyElement>,
    footer: Vec<AnyElement>,
    children: Vec<AnyElement>,
}

impl Panel {
    pub fn new(stack: &Entity<PanelStack>, layer_id: impl Into<SharedString>) -> Self {
        Self {
            stack: stack.clone(),
            layer_id: layer_id.into(),
            badge: None,
            actions: Vec::new(),
            tabs: None,
            footer: Vec::new(),
            children: Vec::new(),
        }
    }

    /// A tag beside the title.
    pub fn badge(mut self, label: impl Into<SharedString>, status: Status) -> Self {
        self.badge = Some((label.into(), status));
        self
    }

    /// A control in the header, before the close button.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// A row of tabs under the header.
    pub fn tabs(mut self, tabs: impl IntoElement) -> Self {
        self.tabs = Some(tabs.into_any_element());
        self
    }

    /// A control of the footer (the form's buttons), right-aligned.
    pub fn footer(mut self, control: impl IntoElement) -> Self {
        self.footer.push(control.into_any_element());
        self
    }
}

impl ParentElement for Panel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Panel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let stack = self.stack.read(cx);
        let Some(layer) = stack.layers().iter().find(|l| l.id == self.layer_id).cloned() else {
            return div().into_any_element();
        };
        let offset = stack.offset_of(&layer.id).unwrap_or(0);
        let is_top = offset == 0;
        let expanded = stack.is_expanded() && is_top;

        let width = if expanded {
            (window.viewport_size().width - m.sidebar_w_collapsed - m.space_7).min(px(1280.))
        } else {
            match layer.width {
                PanelWidth::Sm => m.panel_w_sm,
                PanelWidth::Md => m.panel_w_md,
                PanelWidth::Lg => m.panel_w_lg,
            }
        };

        let (stack_close, stack_expand, stack_spine) = (self.stack.clone(), self.stack.clone(), self.stack.clone());
        let spine_id = layer.id.clone();

        let head = row()
            .flex_none()
            .items_start()
            .justify_between()
            .gap(m.space_3)
            .py(m.space_3)
            .pr(m.space_4)
            .pl(if is_top { m.space_5 } else { m.panel_stack_offset + m.space_4 })
            .bg(c.bg_surface)
            .hairline_bottom(c.border)
            .child(
                row()
                    .min_w_0()
                    .items_start()
                    .gap(m.space_3)
                    .when_some(layer.icon, |this, icon| {
                        this.child(div().mt(px(2.)).child(Icon::new(icon).lg().color(c.accent)))
                    })
                    .child(
                        col()
                            .min_w_0()
                            .gap(px(2.))
                            .child(
                                row()
                                    .gap(m.space_2)
                                    .child(Heading::new(layer.title.clone()).size(HeadingSize::Xl))
                                    .when_some(self.badge, |this, (label, status)| {
                                        this.child(Tag::new(label).status(status))
                                    })
                                    .when(layer.dirty, |this| {
                                        this.child(
                                            div()
                                                .id("panel-dirty")
                                                .flex_none()
                                                .size(px(6.))
                                                .bg(c.warning)
                                                .tooltip(tooltip("Modifications non enregistrées")),
                                        )
                                    }),
                            )
                            .when_some(layer.subtitle.clone(), |this, subtitle| {
                                this.child(div().fs_tight(m.fs_sm).text_color(c.text_muted).child(subtitle))
                            }),
                    ),
            )
            .child(
                row()
                    .flex_none()
                    .gap(m.space_1)
                    .children(self.actions)
                    .child(
                        IconButton::new("panel-expand", if expanded { IconName::Minimize2 } else { IconName::Maximize2 })
                            .title(if expanded { "Réduire" } else { "Agrandir" })
                            .size(IconButtonSize::Md)
                            .on_click(move |_, _, cx| {
                                stack_expand.update(cx, |stack, cx| {
                                    stack.toggle_expanded();
                                    cx.notify();
                                })
                            }),
                    )
                    .child(
                        IconButton::new("panel-close", IconName::X)
                            .title("Fermer (Échap)")
                            .size(IconButtonSize::Md)
                            .on_click(move |_, window, cx| request_close(&stack_close, window, cx)),
                    ),
            );

        let body = div()
            .id(ElementId::Name(format!("panel-body-{}", layer.id).into()))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(c.bg_app)
            .child(col().p(m.space_4).gap(m.space_4).children(self.children));

        div()
            .id(ElementId::Name(format!("panel-{}", layer.id).into()))
            .absolute()
            .top_0()
            .bottom_0()
            .right(m.panel_stack_offset * offset as f32)
            .w(width)
            .flex()
            .flex_col()
            .bg(c.bg_surface)
            .border_l_1()
            .border_color(c.border_strong)
            .shadow(c.shadow_panel(!is_top))
            .text_color(c.text)
            .occlude()
            // A layer under the top one is dimmed and only its spine answers.
            .child(col().size_full().when(!is_top, |this| this.opacity(0.42)).child(head).when_some(self.tabs, |this, tabs| {
                this.child(div().flex_none().px(m.space_4).bg(c.bg_surface).child(tabs))
            }).child(body).when(!self.footer.is_empty(), |this| {
                this.child(
                    row()
                        .flex_none()
                        .justify_end()
                        .gap(m.space_2)
                        .py(m.space_3)
                        .px(m.space_4)
                        .bg(c.bg_surface)
                        .hairline_top(c.border)
                        .children(self.footer),
                )
            }))
            .when(!is_top, |this| {
                this.child(
                    BaseButton::new("panel-spine")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(m.panel_stack_offset)
                        .items_start()
                        .pt(m.space_3)
                        .bg(c.bg_sidebar)
                        .border_r_1()
                        .border_color(c.border)
                        .text_color(c.text_muted)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(c.bg_hover).text_color(c.text))
                        .accessibility_label(layer.title.clone())
                        .child(Icon::new(layer.icon.unwrap_or(IconName::ChevronLeft)))
                        .tooltip(tooltip(format!("Revenir à : {}", layer.title)))
                        .on_click(move |_, _, cx| {
                            stack_spine.update(cx, |stack, cx| {
                                stack.close_above(&spine_id);
                                cx.notify();
                            })
                        }),
                )
            })
            .into_any_element()
    }
}

// ---- PanelSection ---------------------------------------------------------------------------

/// A titled block inside a panel body.
#[derive(IntoElement)]
pub struct PanelSection {
    title: SharedString,
    subtitle: Option<SharedString>,
    icon: Option<IconName>,
    flush: bool,
    actions: Vec<AnyElement>,
    children: Vec<AnyElement>,
}

impl PanelSection {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { title: title.into(), subtitle: None, icon: None, flush: false, actions: Vec::new(), children: Vec::new() }
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// No padding around the body: for a list that brings its own.
    pub fn flush(mut self) -> Self {
        self.flush = true;
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl ParentElement for PanelSection {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for PanelSection {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        col()
            .flex_none()
            .bg(c.bg_surface)
            .border_1()
            .border_color(c.border)
            .child(
                row()
                    .justify_between()
                    .gap(m.space_2)
                    .px(m.space_3)
                    .min_h(m.control_h_xl)
                    .hairline_bottom(c.border)
                    .child(
                        row()
                            .gap(m.space_2)
                            .when_some(self.icon, |this, icon| this.child(Icon::new(icon).color(c.text_muted)))
                            .child(
                                row()
                                    .items_baseline()
                                    .gap(m.space_2)
                                    .child(Heading::new(self.title).size(HeadingSize::Md))
                                    .when_some(self.subtitle, |this, subtitle| {
                                        this.child(div().fs_tight(m.fs_xs).text_color(c.text_subtle).child(subtitle))
                                    }),
                            ),
                    )
                    .child(row().flex_none().gap(m.space_1).children(self.actions)),
            )
            .child(col().when(!self.flush, |this| this.p(m.space_4)).children(self.children))
    }
}

// ---- PanelHost ------------------------------------------------------------------------------

/// The overlay of a page that has panels. Put it last in the page's root (which must be
/// `relative()`), with one [`Panel`] child per open layer.
#[derive(IntoElement)]
pub struct PanelHost {
    stack: Entity<PanelStack>,
    children: Vec<AnyElement>,
}

impl PanelHost {
    pub fn new(stack: &Entity<PanelStack>) -> Self {
        Self { stack: stack.clone(), children: Vec::new() }
    }

    /// Escape and Alt+Left close the top panel. The host listens for them itself; a page whose
    /// own fields can hold the focus while a panel is open (a search box) puts this on its root
    /// too: `.on_key_down(PanelHost::key_handler(&self.panels))`.
    pub fn key_handler(
        stack: &Entity<PanelStack>,
    ) -> impl Fn(&gpui_kit::KeyDownEvent, &mut Window, &mut App) + 'static + use<> {
        let stack = stack.clone();
        move |event, window, cx| {
            if !stack.read(cx).is_open() {
                return;
            }
            let key = event.keystroke.key.as_str();
            if key == "escape" || (key == "left" && event.keystroke.modifiers.alt) {
                cx.stop_propagation();
                request_close(&stack, window, cx);
            }
        }
    }
}

impl ParentElement for PanelHost {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for PanelHost {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scrim = cx.ui().colors.scrim;
        let stack = self.stack.read(cx);
        if !stack.is_open() {
            stack.took_keyboard.set(false);
            return div().into_any_element();
        }
        let focus = stack.focus.clone();
        let opening = !stack.took_keyboard.replace(true);
        let on_scrim = self.stack.clone();
        let host = div()
            .id("panel-host")
            .absolute()
            .inset_0()
            .occlude()
            .on_key_down(Self::key_handler(&self.stack))
            .child(div().id("panel-scrim").absolute().inset_0().bg(scrim).on_mouse_down(
                MouseButton::Left,
                move |_, window, cx| request_close(&on_scrim, window, cx),
            ))
            .children(self.children);

        match focus {
            Some(handle) => {
                // With nothing focused the keyboard would go nowhere: give it to the panels, so
                // Escape closes the top one.
                if window.focused(cx).is_none() {
                    window.focus(&handle, cx);
                } else if opening {
                    // The keyboard may still be where the click came from: the navigation rail,
                    // a row. The panels take it, unless a field of the new panel already has it
                    // (a form focuses its first field). Which is only known once this frame is
                    // drawn, hence the wait. Done once per opening: a dialog opened later over
                    // the panels keeps the keyboard.
                    let panels = handle.clone();
                    window.on_next_frame(move |window, cx| {
                        if !panels.contains_focused(window, cx) {
                            window.focus(&panels, cx);
                        }
                    });
                }
                host.track_focus(&handle).focus_trap("panel-trap", &handle).into_any_element()
            }
            None => host.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(kind: &str, key: &str) -> PanelLayer {
        PanelLayer::new(kind.to_string(), key.to_string(), "Titre")
    }

    #[test]
    fn panels_cascade_and_reopening_one_comes_back_to_it() {
        let mut stack = PanelStack::detached();
        stack.open(layer("client", "a"));
        stack.open(layer("vehicle", "v1"));
        assert_eq!(stack.serialize(), "client:a>vehicle:v1");
        assert_eq!(stack.offset_of("client:a"), Some(1));
        assert_eq!(stack.offset_of("vehicle:v1"), Some(0));

        stack.open(layer("client", "a"));
        assert_eq!(stack.serialize(), "client:a", "what was above is closed");
    }

    #[test]
    fn a_fourth_panel_drops_the_oldest() {
        let mut stack = PanelStack::detached();
        for key in ["1", "2", "3", "4"] {
            stack.open(layer("client", key));
        }
        assert_eq!(stack.depth(), MAX_DEPTH);
        assert_eq!(stack.serialize(), "client:2>client:3>client:4");
    }

    #[test]
    fn a_panel_with_unsaved_edits_asks_before_closing() {
        let mut stack = PanelStack::detached();
        let id = stack.open(layer("client-edit", "a"));
        stack.set_dirty(&id, true);
        assert!(!stack.request_close(), "it stays open");
        assert_eq!(stack.pending_close().map(|s| s.as_ref()), Some("client-edit:a"));

        stack.cancel_close();
        assert!(stack.is_open() && stack.pending_close().is_none());

        stack.request_close();
        stack.confirm_close();
        assert!(!stack.is_open());
    }

    #[test]
    fn the_stack_is_read_back_from_its_serialized_form() {
        assert_eq!(
            PanelStack::parse("client:9f3a > vehicle:77b1"),
            vec![("client".to_string(), "9f3a".to_string()), ("vehicle".to_string(), "77b1".to_string())]
        );
        assert!(PanelStack::parse("").is_empty());
        assert!(PanelStack::parse("nonsense>:x").is_empty());
        assert_eq!(PanelStack::parse("a:1>b:2>c:3>d:4").len(), MAX_DEPTH);
    }

    #[test]
    fn replacing_the_top_keeps_what_is_under_it() {
        let mut stack = PanelStack::detached();
        stack.open(layer("client", "a"));
        stack.open(layer("vehicle", "v"));
        stack.replace_top(layer("vehicle-edit", "v"));
        assert_eq!(stack.serialize(), "client:a>vehicle-edit:v");
        assert_eq!(stack.key_of("client").as_deref(), Some("a"));
    }
}
