//! Containers and read-only data: cards, the label/value list of a detail panel, the row of a
//! simple list, and the pagination bar.

use std::rc::Rc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, CursorStyle, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
    transparent_black,
};

use crate::components::controls::{IconButton, IconButtonSize};
use crate::components::icon::{Icon, IconName};
use crate::style::{StyleExt as _, col, row};
use crate::theme::{FONT_MONO, Themed, line};
use crate::tokens::line_height;

// ---- Card -----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardVariant {
    /// No background: groups content on the page.
    #[default]
    Plain,
    /// A surface, no border.
    Surface,
    /// A surface with a hairline border.
    Bordered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardPadding {
    None,
    Sm,
    #[default]
    Md,
    Lg,
}

/// A light container with an optional header. Children are the body.
#[derive(IntoElement)]
pub struct Card {
    title: Option<SharedString>,
    subtitle: Option<SharedString>,
    variant: CardVariant,
    padding: CardPadding,
    actions: Vec<AnyElement>,
    children: Vec<AnyElement>,
}

impl Card {
    pub fn new() -> Self {
        Self {
            title: None,
            subtitle: None,
            variant: CardVariant::default(),
            padding: CardPadding::default(),
            actions: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn variant(mut self, variant: CardVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn bordered(self) -> Self {
        self.variant(CardVariant::Bordered)
    }

    pub fn padding(mut self, padding: CardPadding) -> Self {
        self.padding = padding;
        self
    }

    /// A control at the end of the header.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let bordered = self.variant == CardVariant::Bordered;
        let has_header = self.title.is_some() || !self.actions.is_empty();
        let pad = match self.padding {
            CardPadding::None => px(0.),
            CardPadding::Sm => m.space_2,
            CardPadding::Md => m.space_3,
            CardPadding::Lg => m.space_5,
        };

        col()
            .when(self.variant != CardVariant::Plain, |this| this.bg(c.bg_surface))
            .when(bordered, |this| this.border_1().border_color(c.border))
            .when(has_header, |this| {
                this.child(
                    row()
                        .justify_between()
                        .gap(m.space_2)
                        .py(m.space_2)
                        .px(m.space_3)
                        .when(bordered, |this| this.hairline_bottom(c.border_subtle))
                        .child(
                            col()
                                .gap(px(2.))
                                .when_some(self.title, |this, title| {
                                    this.child(
                                        div()
                                            .fs_tight(m.fs_md)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(c.text_strong)
                                            .child(title),
                                    )
                                })
                                .when_some(self.subtitle, |this, subtitle| {
                                    this.child(div().fs_tight(m.fs_xs).text_color(c.text_muted).child(subtitle))
                                }),
                        )
                        .child(row().gap(m.space_1).children(self.actions)),
                )
            })
            .child(col().p(pad).children(self.children))
    }
}

// ---- SectionCard ----------------------------------------------------------------------------

/// A titled block of a settings page or a form: header with icon and actions, body, optional
/// footer for the buttons.
#[derive(IntoElement)]
pub struct SectionCard {
    title: Option<SharedString>,
    subtitle: Option<SharedString>,
    icon: Option<IconName>,
    flush: bool,
    seamless: bool,
    actions: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    children: Vec<AnyElement>,
}

impl SectionCard {
    pub fn new() -> Self {
        Self {
            title: None,
            subtitle: None,
            icon: None,
            flush: false,
            seamless: false,
            actions: Vec::new(),
            footer: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// No padding around the body: for a table or a list that brings its own.
    pub fn flush(mut self) -> Self {
        self.flush = true;
        self
    }

    /// No border: for a card already inside a bordered container.
    pub fn seamless(mut self) -> Self {
        self.seamless = true;
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// A control of the footer, right-aligned.
    pub fn footer(mut self, control: impl IntoElement) -> Self {
        self.footer.push(control.into_any_element());
        self
    }
}

impl Default for SectionCard {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for SectionCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SectionCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let has_head = self.title.is_some() || !self.actions.is_empty();

        col()
            .bg(c.bg_surface)
            .when(!self.seamless, |this| this.border_1().border_color(c.border))
            .when(has_head, |this| {
                this.child(
                    row()
                        .justify_between()
                        .gap(m.space_3)
                        .min_h(m.control_h_xl)
                        .py(m.space_2)
                        .px(m.space_4)
                        .hairline_bottom(c.border_subtle)
                        .child(
                            row()
                                .gap(m.space_2)
                                .when_some(self.icon, |this, icon| this.child(Icon::new(icon).color(c.text_muted)))
                                .child(
                                    col()
                                        .gap(px(1.))
                                        .when_some(self.title, |this, title| {
                                            this.child(
                                                div()
                                                    .fs_tight(m.fs_md)
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(c.text_strong)
                                                    .child(title),
                                            )
                                        })
                                        .when_some(self.subtitle, |this, subtitle| {
                                            this.child(div().fs_tight(m.fs_xs).text_color(c.text_muted).child(subtitle))
                                        }),
                                ),
                        )
                        .child(row().flex_none().gap(m.space_2).children(self.actions)),
                )
            })
            .child(col().when(!self.flush, |this| this.p(m.space_4)).children(self.children))
            .when(!self.footer.is_empty(), |this| {
                this.child(
                    row()
                        .justify_end()
                        .gap(m.space_2)
                        .py(m.space_2)
                        .px(m.space_4)
                        .hairline_top(c.border_subtle)
                        .children(self.footer),
                )
            })
    }
}

// ---- KpiTile --------------------------------------------------------------------------------

/// One headline figure: a label, the number, an icon in the colour of what it says.
#[derive(IntoElement)]
pub struct KpiTile {
    label: SharedString,
    value: SharedString,
    unit: Option<SharedString>,
    icon: Option<IconName>,
    hint: Option<SharedString>,
    trend: Option<(SharedString, Trend)>,
    loading: bool,
    status: crate::components::content::Status,
}

/// Which way a figure moved since the previous period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    Up,
    Down,
}

impl KpiTile {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            unit: None,
            icon: None,
            hint: None,
            trend: None,
            loading: false,
            status: crate::components::content::Status::Neutral,
        }
    }

    /// A small word after the number: "€", "km".
    pub fn unit(mut self, unit: impl Into<SharedString>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A line under the number.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// How the figure moved, under the number, in place of the hint: "+3", "-8,2 %".
    pub fn trend(mut self, label: impl Into<SharedString>, direction: Trend) -> Self {
        self.trend = Some((label.into(), direction));
        self
    }

    /// A wheel in place of the number while it is being fetched.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// The colour of the icon: accent, success, warning, danger.
    pub fn status(mut self, status: crate::components::content::Status) -> Self {
        self.status = status;
        self
    }
}

impl RenderOnce for KpiTile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        use crate::components::content::Status;
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let (icon_bg, icon_ink) = match self.status {
            Status::Neutral | Status::Muted => (c.bg_hover, c.text_muted),
            other => other.soft(cx),
        };
        col()
            .flex_1()
            .min_w_0()
            .min_h(m.space_9)
            .gap(m.space_3)
            .p(m.space_4)
            .bg(c.bg_surface)
            .border_1()
            .border_color(c.border)
            .child(
                row()
                    .justify_between()
                    .gap(m.space_2)
                    .child(
                        div()
                            .min_w_0()
                            .one_line()
                            .fs_tight(m.fs_xs)
                            .text_color(c.text_muted)
                            .child(SharedString::from(self.label.to_uppercase())),
                    )
                    .when_some(self.icon, |this, icon| {
                        this.child(
                            row().flex_none().justify_center().size(m.control_h_sm).bg(icon_bg).child(Icon::new(icon).sm().color(icon_ink)),
                        )
                    }),
            )
            .child(if self.loading {
                // As tall as the number it stands for, so the tile does not jump.
                row().h(m.fs_3xl * line_height::TIGHT).child(crate::components::content::Spinner::new())
            } else {
                row()
                    .items_baseline()
                    .gap(m.space_1)
                    .child(
                        div().fs_tight(m.fs_3xl).font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(self.value),
                    )
                    .when_some(self.unit, |this, unit| {
                        this.child(div().fs_tight(m.fs_sm).font_weight(FontWeight::MEDIUM).text_color(c.text_muted).child(unit))
                    })
            })
            .map(|this| match (self.trend, self.hint) {
                (Some((label, direction)), _) => {
                    let (icon, ink) = match direction {
                        Trend::Up => (IconName::TrendingUp, c.success),
                        Trend::Down => (IconName::TrendingDown, c.danger),
                    };
                    this.child(
                        row().gap(m.space_1).fs_tight(m.fs_xs).text_color(ink).child(Icon::new(icon).sm().color(ink)).child(label),
                    )
                }
                (None, Some(hint)) => this.child(div().fs_tight(m.fs_xs).text_color(c.text_muted).child(hint)),
                (None, None) => this,
            })
    }
}

// ---- TileButton -----------------------------------------------------------------------------

/// A large tile that leads somewhere: an icon, a name and one line saying what is there. For
/// launcher grids (the modules of the home screen, the tools of a page).
#[derive(IntoElement)]
pub struct TileButton {
    id: ElementId,
    label: SharedString,
    description: Option<SharedString>,
    icon: Option<IconName>,
    badge: Option<SharedString>,
    disabled: bool,
    divided: bool,
    on_click: Option<ClickHandler>,
}

impl TileButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            icon: None,
            badge: None,
            disabled: false,
            divided: false,
            on_click: None,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A short chip at the end: "Bientôt".
    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// A line on its left: for every tile of a row but the first.
    pub fn divided(mut self, divided: bool) -> Self {
        self.divided = divided;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for TileButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        use crate::components::content::{Status, Tag};
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let disabled = self.disabled;
        let (icon_bg, icon_ink) = if disabled { (c.bg_app, c.text_subtle) } else { (c.accent_soft, c.accent) };
        row()
            .id(self.id)
            .min_w_0()
            .gap(m.space_3)
            .p(m.space_3)
            .when(self.divided, |this| this.border_l_1().border_color(c.border_subtle))
            .when(!disabled, |this| this.cursor(CursorStyle::PointingHand).hover(|s| s.bg(c.bg_hover)))
            .when_some(self.icon, |this, icon| {
                this.child(row().flex_none().justify_center().size(px(30.)).bg(icon_bg).child(Icon::new(icon).color(icon_ink)))
            })
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .one_line()
                            .fs_tight(m.fs_md)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if disabled { c.text_muted } else { c.text_strong })
                            .child(self.label),
                    )
                    .when_some(self.description, |this, description| {
                        this.child(div().one_line().fs_tight(m.fs_xs).text_color(c.text_muted).child(description))
                    }),
            )
            .when_some(self.badge, |this, badge| this.child(div().flex_none().child(Tag::new(badge).status(Status::Muted))))
            .when_some(self.on_click.filter(|_| !disabled), |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

// ---- InfoList -------------------------------------------------------------------------------

/// One label/value pair of an [`InfoList`].
#[derive(Debug, Clone, PartialEq)]
pub struct InfoItem {
    pub label: SharedString,
    pub value: Option<SharedString>,
    pub icon: Option<IconName>,
    pub mono: bool,
    pub block: bool,
    /// What stands for a missing value, in place of the dash.
    pub placeholder: Option<SharedString>,
}

impl InfoItem {
    /// An empty or missing value shows as a dash.
    pub fn new(label: impl Into<SharedString>, value: Option<impl Into<SharedString>>) -> Self {
        let value = value.map(Into::into).filter(|v: &SharedString| !v.trim().is_empty());
        Self { label: label.into(), value, icon: None, mono: false, block: false, placeholder: None }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Fixed-width value: identifiers, serial numbers, plates.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// The words shown when there is no value: "Aucun seuil", "Non classé".
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// A paragraph (notes): wraps and reads at a looser line height.
    pub fn block(mut self) -> Self {
        self.block = true;
        self
    }
}

/// The label/value list of a detail panel.
#[derive(IntoElement)]
pub struct InfoList {
    items: Vec<InfoItem>,
    dense: bool,
}

impl InfoList {
    pub fn new(items: Vec<InfoItem>) -> Self {
        Self { items, dense: false }
    }

    pub fn dense(mut self) -> Self {
        self.dense = true;
        self
    }
}

impl RenderOnce for InfoList {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let last = self.items.len().saturating_sub(1);
        let pad = if self.dense { m.space_1 } else { m.space_2 };

        col().w_full().children(self.items.into_iter().enumerate().map(|(i, item)| {
            let empty = item.value.is_none();
            let value_size = if item.mono { m.fs_sm } else { m.fs_md };
            row()
                .items_start()
                .gap(m.space_3)
                .py(pad)
                .when(i != last, |this| this.hairline_bottom(c.border_subtle))
                .child(
                    row()
                        .flex_none()
                        .w(relative_label())
                        .min_w(px(110.))
                        .gap(m.space_1)
                        .text_color(c.text_muted)
                        .fs(m.fs_xs)
                        .when_some(item.icon, |this, icon| this.child(Icon::new(icon).sm()))
                        .child(SharedString::from(item.label.to_uppercase())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(value_size)
                        .line_height(line(
                            value_size,
                            if item.block { line_height::RELAXED } else { line_height::NORMAL },
                        ))
                        .text_color(if empty { c.text_subtle } else { c.text })
                        .when(item.mono, |this| this.font_family(FONT_MONO))
                        .child(item.value.or(item.placeholder).unwrap_or_else(|| "—".into())),
                )
        }))
    }
}

fn relative_label() -> gpui_kit::DefiniteLength {
    gpui_kit::relative(0.34)
}

// ---- ListRow --------------------------------------------------------------------------------

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type PageHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// A row of a simple list: an icon or initials, a title, a line of detail, a count, and
/// actions that appear at the end.
#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    title: SharedString,
    meta: Option<SharedString>,
    icon: Option<IconName>,
    avatar: Option<SharedString>,
    selected: bool,
    dense: bool,
    count: Option<SharedString>,
    count_icon: Option<IconName>,
    trailing: Vec<AnyElement>,
    on_click: Option<ClickHandler>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            meta: None,
            icon: None,
            avatar: None,
            selected: false,
            dense: false,
            count: None,
            count_icon: None,
            trailing: Vec::new(),
            on_click: None,
        }
    }

    /// The second line, in small muted type.
    pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Initials in a small square, for a person.
    pub fn avatar(mut self, initials: impl Into<SharedString>) -> Self {
        self.avatar = Some(initials.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// A tighter row for a navigation rail.
    pub fn dense(mut self) -> Self {
        self.dense = true;
        self
    }

    /// A number at the end ("3"), with an optional icon before it.
    pub fn count(mut self, count: impl Into<SharedString>, icon: Option<IconName>) -> Self {
        self.count = Some(count.into());
        self.count_icon = icon;
        self
    }

    /// Something at the end of the row: a tag, an amount, an icon button.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ListRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let on_click = self.on_click;
        let clickable = on_click.is_some();
        let selected = self.selected;

        BaseButton::new(self.id)
            .w_full()
            .justify_start()
            .text_color(c.text)
            .border_color(if selected { c.accent } else { transparent_black() })
            .when(selected, |this| this.bg(c.bg_selected))
            .selected(selected)
            .map(|this| {
                if self.dense {
                    this.gap(m.space_2).py(px(3.)).px(m.space_2).border_l_2()
                } else {
                    this.gap(m.space_3).py(m.space_2).px(m.space_3).border_l_3()
                }
            })
            .when(clickable, |this| this.cursor(CursorStyle::PointingHand))
            .hover(|s| if selected { s } else { s.bg(c.bg_hover) })
            .when_some(self.avatar, |this, initials| {
                this.child(
                    row()
                        .flex_none()
                        .justify_center()
                        .size(px(26.))
                        .bg(c.accent_soft)
                        .text_color(c.accent_active)
                        .text_size(m.fs_micro)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(initials.to_uppercase())),
                )
            })
            .when_some(self.icon, |this, icon| {
                this.child(Icon::new(icon).color(if selected { c.accent } else { c.text_muted }))
            })
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .items_start()
                    .gap(px(1.))
                    .child(
                        div()
                            .w_full()
                            .text_left()
                            .one_line()
                            .fs(if self.dense { m.fs_xs } else { m.fs_md })
                            .font_weight(if self.dense { FontWeight::NORMAL } else { FontWeight::MEDIUM })
                            .when(selected, |this| this.text_color(c.text_strong))
                            .child(self.title),
                    )
                    .when_some(self.meta, |this, meta| {
                        this.child(
                            div().w_full().text_left().one_line().fs(m.fs_xs).text_color(c.text_muted).child(meta),
                        )
                    }),
            )
            .when_some(self.count, |this, count| {
                this.child(
                    row()
                        .flex_none()
                        .gap(px(3.))
                        .fs(m.fs_xs)
                        .text_color(c.text_muted)
                        .when_some(self.count_icon, |this, icon| this.child(Icon::new(icon).sm()))
                        .child(count),
                )
            })
            // The buttons at the end sit in the row's own padding: a row with one is as tall as
            // a row without.
            .when(!self.trailing.is_empty(), |this| {
                this.child(row().flex_none().gap(m.space_1).my(-m.space_2).children(self.trailing))
            })
            .when_some(on_click, |this, handler| this.on_click(move |event, window, cx| handler(event, window, cx)))
    }
}

// ---- Pagination -----------------------------------------------------------------------------

/// The bar under a paged list: "1–50 sur 312", previous, next. Pages count from 0.
#[derive(IntoElement)]
pub struct Pagination {
    page: usize,
    page_size: usize,
    total: usize,
    noun: SharedString,
    disabled: bool,
    on_page: Option<PageHandler>,
}

impl Pagination {
    pub fn new(page: usize, page_size: usize, total: usize) -> Self {
        Self { page, page_size: page_size.max(1), total, noun: "".into(), disabled: false, on_page: None }
    }

    /// What is being counted, plural: "clients". Shown after the total.
    pub fn noun(mut self, noun: impl Into<SharedString>) -> Self {
        self.noun = noun.into();
        self
    }

    /// While a page is loading.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_page(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_page = Some(Rc::new(handler));
        self
    }

    /// "1–50 sur 312 clients", "Aucun client" being the caller's business (an empty state).
    pub fn summary(page: usize, page_size: usize, total: usize, noun: &str) -> String {
        if total == 0 {
            return String::new();
        }
        let first = page * page_size + 1;
        let last = ((page + 1) * page_size).min(total);
        let noun = if noun.is_empty() { String::new() } else { format!(" {noun}") };
        format!("{first}–{last} sur {total}{noun}")
    }

    pub fn page_count(page_size: usize, total: usize) -> usize {
        total.div_ceil(page_size.max(1)).max(1)
    }
}

impl RenderOnce for Pagination {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let pages = Self::page_count(self.page_size, self.total);
        let (page, on_page) = (self.page, self.on_page);
        let previous = on_page.clone();

        row()
            .flex_none()
            .justify_end()
            .gap(m.space_3)
            .min_h(m.control_h_lg)
            .px(m.space_4)
            .bg(c.bg_surface)
            .hairline_top(c.border)
            .text_color(c.text_muted)
            .fs(m.fs_sm)
            .child(SharedString::from(Self::summary(self.page, self.page_size, self.total, &self.noun)))
            .child(
                row()
                    .gap(m.space_1)
                    .child(
                        IconButton::new("page-previous", IconName::ChevronLeft)
                            .title("Page précédente")
                            .size(IconButtonSize::Sm)
                            .disabled(self.disabled || page == 0)
                            .on_click(move |_, window, cx| {
                                if let Some(handler) = &previous {
                                    handler(page.saturating_sub(1), window, cx)
                                }
                            }),
                    )
                    .child(SharedString::from(format!("{} / {}", page + 1, pages)))
                    .child(
                        IconButton::new("page-next", IconName::ChevronRight)
                            .title("Page suivante")
                            .size(IconButtonSize::Sm)
                            .disabled(self.disabled || page + 1 >= pages)
                            .on_click(move |_, window, cx| {
                                if let Some(handler) = &on_page {
                                    handler(page + 1, window, cx)
                                }
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_names_the_rows_on_screen() {
        assert_eq!(Pagination::summary(0, 50, 312, "clients"), "1–50 sur 312 clients");
        assert_eq!(Pagination::summary(6, 50, 312, "clients"), "301–312 sur 312 clients");
        assert_eq!(Pagination::summary(0, 50, 7, ""), "1–7 sur 7");
        assert_eq!(Pagination::summary(0, 50, 0, "clients"), "");
    }

    #[test]
    fn there_is_always_at_least_one_page() {
        assert_eq!(Pagination::page_count(50, 0), 1);
        assert_eq!(Pagination::page_count(50, 50), 1);
        assert_eq!(Pagination::page_count(50, 51), 2);
    }
}
