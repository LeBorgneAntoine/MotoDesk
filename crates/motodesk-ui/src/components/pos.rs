//! What the till is made of. Everything here is larger than in the office screens: a till
//! is used standing, in a hurry, sometimes with a finger.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, CursorStyle, ElementId, FontWeight, InteractiveElement as _, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::components::content::{Status, Tag};
use crate::components::controls::{IconButton, IconButtonSize};
use crate::components::icon::{Icon, IconName};
use crate::style::{StyleExt as _, col, row};
use crate::theme::{FONT_MONO, Themed};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

// ---- PosKey ---------------------------------------------------------------------------------

/// A large key: a way to pay, a banknote.
#[derive(IntoElement)]
pub struct PosKey {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    primary: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl PosKey {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: label.into(), icon: None, primary: false, disabled: false, on_click: None }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The key most often pressed.
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for PosKey {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let (bg, ink, edge, hover) = if self.primary {
            (c.accent, c.text_on_accent, c.accent, c.accent_hover)
        } else {
            (c.bg_surface, c.text_strong, c.border_strong, c.bg_hover)
        };
        col()
            .id(self.id)
            .flex_1()
            .min_w_0()
            .h(px(72.))
            .items_center()
            .justify_center()
            .gap(m.space_1)
            .bg(bg)
            .border_1()
            .border_color(edge)
            .text_color(ink)
            .fs_tight(m.fs_md)
            .font_weight(FontWeight::SEMIBOLD)
            .children(self.icon.map(|icon| Icon::new(icon).lg()))
            .child(self.label)
            .map(|this| {
                if self.disabled {
                    this.opacity(0.5)
                } else {
                    this.cursor(CursorStyle::PointingHand)
                        .hover(move |style| style.bg(hover))
                        .when_some(self.on_click, |this, handler| this.on_click(move |event, window, cx| handler(event, window, cx)))
                }
            })
    }
}

// ---- PosItem --------------------------------------------------------------------------------

/// One article the till offers, in the quick strip or in the search list: its name, what tells
/// it apart, and its price, which is the decision and sits large on the right.
#[derive(IntoElement)]
pub struct PosItem {
    id: ElementId,
    name: SharedString,
    icon: IconName,
    meta: Option<SharedString>,
    price: SharedString,
    badge: Option<(SharedString, Status)>,
    compact: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl PosItem {
    pub fn new(id: impl Into<ElementId>, name: impl Into<SharedString>, price: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            icon: IconName::Package,
            meta: None,
            price: price.into(),
            badge: None,
            compact: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = icon;
        self
    }

    /// A second line: brand, reference. Empty for none.
    pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
        let meta: SharedString = meta.into();
        self.meta = (!meta.trim().is_empty()).then_some(meta);
        self
    }

    /// A quiet chip: "Stock bas", "Rupture".
    pub fn badge(mut self, label: impl Into<SharedString>, status: Status) -> Self {
        self.badge = Some((label.into(), status));
        self
    }

    /// For a narrow strip: the price goes under the name.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Shown, and not sold: out of stock.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for PosItem {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let price = div()
            .flex_none()
            .font_family(FONT_MONO)
            .fs_tight(if self.compact { m.fs_sm } else { m.fs_lg })
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if self.compact { c.text_muted } else { c.text_strong })
            .child(self.price);
        // In a narrow strip the price goes under the name; otherwise at the end of the row.
        let (under, aside) = if self.compact { (Some(price), None) } else { (None, Some(price)) };
        let has_second_line = self.meta.is_some() || self.badge.is_some();
        let text =
            col()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .fs_tight(m.fs_md)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c.text_strong)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(self.name),
                )
                .when(has_second_line, |this| {
                    this.child(
                        row()
                            .gap(m.space_2)
                            .children(self.meta.map(|meta| {
                                div().min_w_0().fs_tight(m.fs_xs).text_color(c.text_muted).overflow_hidden().whitespace_nowrap().child(meta)
                            }))
                            .children(self.badge.map(|(label, status)| Tag::new(label).status(status))),
                    )
                })
                .children(under);
        let item = row()
            .id(self.id)
            .w_full()
            .min_h(px(56.))
            .gap(m.space_3)
            .px(m.space_3)
            .py(m.space_2)
            .bg(c.bg_surface)
            .border_b_1()
            .border_color(c.border_subtle)
            .child(div().flex_none().text_color(c.text_muted).child(Icon::new(self.icon).lg()))
            .child(text)
            .children(aside);
        item.map(|this| {
            if self.disabled {
                this.opacity(0.5)
            } else {
                this.cursor(CursorStyle::PointingHand)
                    .hover(|style| style.bg(c.bg_hover))
                    .when_some(self.on_click, |this, handler| this.on_click(move |event, window, cx| handler(event, window, cx)))
            }
        })
    }
}

// ---- PosCartLine ----------------------------------------------------------------------------

/// One line of the cart. Its quantity, discount and removal are always on screen: a till has
/// no action that only appears under the pointer.
#[derive(IntoElement)]
pub struct PosCartLine {
    id: ElementId,
    label: SharedString,
    icon: IconName,
    meta: SharedString,
    quantity: SharedString,
    amount: SharedString,
    on_minus: Option<ClickHandler>,
    on_plus: Option<ClickHandler>,
    on_discount: Option<ClickHandler>,
    on_remove: Option<ClickHandler>,
}

impl PosCartLine {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, quantity: impl Into<SharedString>, amount: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: IconName::Package,
            meta: SharedString::default(),
            quantity: quantity.into(),
            amount: amount.into(),
            on_minus: None,
            on_plus: None,
            on_discount: None,
            on_remove: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = icon;
        self
    }

    /// Under the name: `2 × 9,99 € TTC`.
    pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
        self.meta = meta.into();
        self
    }

    pub fn on_minus(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_minus = Some(Rc::new(handler));
        self
    }

    pub fn on_plus(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_plus = Some(Rc::new(handler));
        self
    }

    pub fn on_discount(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_discount = Some(Rc::new(handler));
        self
    }

    pub fn on_remove(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for PosCartLine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let key = |part: &'static str, icon: IconName, title: &'static str, handler: Option<ClickHandler>| {
            let button = IconButton::new(part, icon).title(title).size(IconButtonSize::Lg);
            match handler {
                Some(handler) => button.on_click(move |event, window, cx| handler(event, window, cx)),
                None => button.disabled(true),
            }
        };
        col()
            .id(self.id)
            .w_full()
            .gap(m.space_2)
            .px(m.space_3)
            .py(m.space_2)
            .border_b_1()
            .border_color(c.border_subtle)
            .child(
                row()
                    .items_start()
                    .gap(m.space_2)
                    .child(div().flex_none().pt(px(2.)).text_color(c.text_muted).child(Icon::new(self.icon)))
                    .child(
                        col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(div().fs_tight(m.fs_md).font_weight(FontWeight::MEDIUM).text_color(c.text_strong).child(self.label))
                            .when(!self.meta.is_empty(), |this| this.child(div().fs_tight(m.fs_xs).text_color(c.text_muted).child(self.meta))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(FONT_MONO)
                            .fs_tight(m.fs_md)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c.text_strong)
                            .child(self.amount),
                    ),
            )
            .child(
                row()
                    .justify_between()
                    .child(
                        row()
                            .gap(m.space_1)
                            .child(key("minus", IconName::Minus, "Moins", self.on_minus))
                            .child(
                                div()
                                    .w(px(44.))
                                    .text_center()
                                    .font_family(FONT_MONO)
                                    .fs_tight(m.fs_md)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.quantity),
                            )
                            .child(key("plus", IconName::Plus, "Plus", self.on_plus)),
                    )
                    .child(
                        row()
                            .gap(m.space_1)
                            .child(key("discount", IconName::Tag, "Remise", self.on_discount))
                            .child(key("remove", IconName::Trash, "Retirer", self.on_remove).danger()),
                    ),
            )
    }
}
