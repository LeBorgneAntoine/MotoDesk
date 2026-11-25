//! The chrome of the window and of a page: title bar, navigation rail, status bar, page
//! toolbar, filter rail.

use std::rc::Rc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::TitleBar as KitTitleBar;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, CursorStyle, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::components::content::{Heading, HeadingSize};
use crate::components::controls::{IconButton, IconButtonSize};
use crate::components::icon::{Icon, IconName};
use crate::components::tooltip::tooltip;
use crate::style::{StyleExt as _, col, row};
use crate::theme::Themed;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type PickHandler = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;

// ---- TitleBar -------------------------------------------------------------------------------

/// The top of the window: it is the handle to move the window by, and carries the window
/// buttons at its end. Children are laid out from the start; [`TitleBar::trailing`] adds to the
/// end, before the window buttons.
#[derive(IntoElement)]
pub struct TitleBar {
    children: Vec<AnyElement>,
    trailing: Vec<AnyElement>,
    transparent: bool,
}

impl TitleBar {
    pub fn new() -> Self {
        Self { children: Vec::new(), trailing: Vec::new(), transparent: false }
    }

    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    /// No background or hairline: over the sign-in page.
    pub fn transparent(mut self) -> Self {
        self.transparent = true;
        self
    }

    /// The window options a window with this title bar must be opened with.
    pub fn window_options() -> gpui_kit::WindowOptions {
        KitTitleBar::window_options()
    }
}

impl Default for TitleBar {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for TitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for TitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let clear = c.bg_titlebar.opacity(0.);
        KitTitleBar::new()
            .h(m.titlebar_h)
            .pl(m.space_3)
            .bg(if self.transparent { clear } else { c.bg_titlebar })
            .border_color(if self.transparent { clear } else { c.border_chrome })
            .text_color(c.text)
            .fs(m.fs_md)
            .child(row().h_full().gap(m.space_2).children(self.children))
            .child(row().h_full().gap(m.space_2).pr(m.space_2).children(self.trailing))
    }
}

// ---- Sidebar --------------------------------------------------------------------------------

/// One destination of the navigation rail.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarItem {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: IconName,
    pub disabled: bool,
    pub badge: Option<SharedString>,
}

impl SidebarItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>, icon: IconName) -> Self {
        Self { id: id.into(), label: label.into(), icon, disabled: false, badge: None }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// A small count on the corner of the icon.
    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }
}

/// The navigation rail: one square per module, its name in a tooltip. `footer` items sit at
/// the bottom (settings, account).
#[derive(IntoElement)]
pub struct Sidebar {
    items: Vec<SidebarItem>,
    footer: Vec<SidebarItem>,
    active: SharedString,
    on_select: Option<PickHandler>,
}

impl Sidebar {
    pub fn new(items: Vec<SidebarItem>, active: impl Into<SharedString>) -> Self {
        Self { items, footer: Vec::new(), active: active.into(), on_select: None }
    }

    pub fn footer(mut self, items: Vec<SidebarItem>) -> Self {
        self.footer = items;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Sidebar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let side = m.sidebar_w_collapsed;
        let (active, on_select) = (self.active, self.on_select);

        let square = move |item: SidebarItem| {
            let is_active = item.id == active;
            let on_select = on_select.clone();
            let id = item.id.clone();
            BaseButton::new(ElementId::Name(format!("nav-{}", item.id).into()))
                .disabled(item.disabled)
                .flex_none()
                .relative()
                .size(side)
                .selected(is_active)
                .accessibility_label(item.label.clone())
                .cursor(CursorStyle::PointingHand)
                .when(is_active, |this| this.bg(c.accent))
                .hover(|s| s.bg(if is_active { c.accent_hover } else { c.bg_hover }))
                .child(Icon::new(item.icon).lg().color(if item.disabled {
                    c.text_disabled
                } else if is_active {
                    c.text_on_accent
                } else {
                    c.text_muted
                }))
                .when_some(item.badge, |this, badge| {
                    this.child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .right(px(4.))
                            .min_w(px(12.))
                            .px(px(5.))
                            .py(px(1.))
                            .bg(if is_active { c.text_on_accent } else { c.accent })
                            .text_color(if is_active { c.accent } else { c.text_on_accent })
                            .text_size(m.fs_micro)
                            .line_height(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(badge),
                    )
                })
                .tooltip(tooltip(item.label))
                .when_some(on_select, |this, handler| this.on_click(move |_, window, cx| handler(&id, window, cx)))
        };

        col()
            .flex_none()
            .w(side)
            .h_full()
            .bg(c.bg_titlebar)
            .border_r_1()
            .border_color(c.border_chrome)
            .child(
                div().id("nav-scroll").flex_1().min_h_0().overflow_y_scroll().child(
                    col().children(self.items.into_iter().map(square.clone())),
                ),
            )
            .child(col().flex_none().children(self.footer.into_iter().map(square)))
    }
}

// ---- StatusBar ------------------------------------------------------------------------------

/// The coloured bar at the bottom of the window: state on the left, tools on the right.
#[derive(IntoElement)]
pub struct StatusBar {
    left: Vec<AnyElement>,
    right: Vec<AnyElement>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self { left: Vec::new(), right: Vec::new() }
    }

    pub fn left(mut self, element: impl IntoElement) -> Self {
        self.left.push(element.into_any_element());
        self
    }

    pub fn right(mut self, element: impl IntoElement) -> Self {
        self.right.push(element.into_any_element());
        self
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for StatusBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        row()
            .flex_none()
            .w_full()
            .h(m.statusbar_h)
            .px(m.space_4)
            .justify_between()
            .bg(c.statusbar)
            .text_color(c.statusbar_fg)
            .fs_tight(m.fs_sm)
            .child(row().gap(m.space_3).children(self.left))
            .child(row().gap(m.space_3).children(self.right))
    }
}

/// A word or a button of the status bar. Without `on_click` it is a plain label.
#[derive(IntoElement)]
pub struct StatusBarButton {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    /// A dot in a state colour before the label.
    dot: Option<gpui_kit::Hsla>,
    title: Option<SharedString>,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl StatusBarButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: label.into(), icon: None, dot: None, title: None, disabled: false, on_click: None }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn dot(mut self, color: gpui_kit::Hsla) -> Self {
        self.dot = Some(color);
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
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

impl RenderOnce for StatusBarButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let on_click = self.on_click;
        let clickable = on_click.is_some() && !self.disabled;
        BaseButton::new(self.id)
            .disabled(self.disabled)
            .focusable(clickable)
            .flex_none()
            .h(px(22.))
            .px(m.space_2)
            .gap(m.space_1)
            .text_color(c.statusbar_fg)
            .fs_tight(m.fs_sm)
            .when(clickable, |this| this.cursor(CursorStyle::PointingHand).hover(|s| s.bg(c.brand_line)))
            .when_some(self.dot, |this, color| this.child(div().flex_none().size(px(7.)).bg(color)))
            .when_some(self.icon, |this, icon| this.child(Icon::new(icon).sm()))
            .child(self.label)
            .when_some(self.title, |this, title| this.tooltip(tooltip(title)))
            .when_some(on_click, |this, handler| this.on_click(move |event, window, cx| handler(event, window, cx)))
    }
}

// ---- PageToolbar ----------------------------------------------------------------------------

/// The header of a page: icon, title, a count, then search, filters and actions at the end.
#[derive(IntoElement)]
pub struct PageToolbar {
    title: SharedString,
    subtitle: Option<SharedString>,
    meta: Option<SharedString>,
    icon: Option<IconName>,
    on_back: Option<ClickHandler>,
    trailing: Vec<AnyElement>,
    below: Vec<AnyElement>,
}

impl PageToolbar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            meta: None,
            icon: None,
            on_back: None,
            trailing: Vec::new(),
            below: Vec::new(),
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// A small figure beside the title ("312").
    pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A back arrow before the title, for a page inside a page.
    pub fn on_back(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_back = Some(Rc::new(handler));
        self
    }

    /// Search, filters, actions: in the order given, at the end of the row.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    /// A second row under the title: tabs.
    pub fn below(mut self, element: impl IntoElement) -> Self {
        self.below.push(element.into_any_element());
        self
    }
}

impl RenderOnce for PageToolbar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        col()
            .flex_none()
            .w_full()
            .bg(c.bg_surface)
            .hairline_bottom(c.border)
            .child(
                row()
                    .justify_between()
                    .gap(m.space_4)
                    .min_h(m.toolbar_h)
                    .py(m.space_2)
                    .px(m.space_5)
                    .child(
                        row()
                            .min_w_0()
                            .gap(m.space_3)
                            .when_some(self.on_back, |this, handler| {
                                this.child(
                                    IconButton::new("page-back", IconName::ArrowLeft)
                                        .title("Retour")
                                        .size(IconButtonSize::Md)
                                        .on_click(move |event, window, cx| handler(event, window, cx)),
                                )
                            })
                            .when_some(self.icon, |this, icon| this.child(Icon::new(icon).lg().color(c.accent)))
                            .child(
                                col()
                                    .min_w_0()
                                    .gap(px(2.))
                                    .child(
                                        row()
                                            .items_baseline()
                                            .gap(m.space_2)
                                            .child(Heading::new(self.title).size(HeadingSize::Xl))
                                            .when_some(self.meta, |this, meta| {
                                                this.child(div().fs_tight(m.fs_xs).text_color(c.text_muted).child(meta))
                                            }),
                                    )
                                    .when_some(self.subtitle, |this, subtitle| {
                                        this.child(div().fs_tight(m.fs_sm).text_color(c.text_muted).child(subtitle))
                                    }),
                            ),
                    )
                    .child(row().flex_none().gap(m.space_3).children(self.trailing)),
            )
            .when(!self.below.is_empty(), |this| {
                this.child(row().gap(m.space_3).px(m.space_5).hairline_top(c.border_subtle).children(self.below))
            })
    }
}

// ---- FilterRail -----------------------------------------------------------------------------

/// The column at the left of a page that narrows its list: categories, statuses. Children are
/// the choices (dense [`crate::ListRow`]s).
#[derive(IntoElement)]
pub struct FilterRail {
    id: ElementId,
    title: SharedString,
    actions: Vec<AnyElement>,
    footer: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl FilterRail {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self { id: id.into(), title: title.into(), actions: Vec::new(), footer: None, children: Vec::new() }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    pub fn footer(mut self, text: impl Into<SharedString>) -> Self {
        self.footer = Some(text.into());
        self
    }
}

impl ParentElement for FilterRail {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for FilterRail {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        col()
            .flex_none()
            .w(m.filter_rail_w)
            .h_full()
            .min_h_0()
            .bg(c.bg_sidebar)
            .border_r_1()
            .border_color(c.border)
            .child(
                row()
                    .flex_none()
                    .justify_between()
                    .gap(m.space_2)
                    .px(m.space_3)
                    .min_h(m.toolbar_h)
                    .hairline_bottom(c.border)
                    .child(Heading::new(self.title).caps())
                    .child(row().flex_none().gap(m.space_1).children(self.actions)),
            )
            .child(div().id(self.id).flex_1().min_h_0().overflow_y_scroll().child(col().children(self.children)))
            .when_some(self.footer, |this, text| {
                this.child(
                    div()
                        .flex_none()
                        .py(m.space_2)
                        .px(m.space_3)
                        .hairline_top(c.border)
                        .fs(m.fs_xs)
                        .text_color(c.text_muted)
                        .child(text),
                )
            })
    }
}
