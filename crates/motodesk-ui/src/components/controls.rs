//! Form controls other than text: icon button, checkbox, switch, select, tabs, and the label
//! that goes around a field.

use std::rc::Rc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::IndexPath;
use gpui_kit::component::checkbox::Checkbox as KitCheckbox;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{Select as KitSelect, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch as KitSwitch;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, CursorStyle, ElementId, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement, Pixels, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px, transparent_black,
};

use crate::components::icon::{Icon, IconName, IconSize};
use crate::components::text_field::{Input, TextModel};
use crate::components::tooltip::tooltip;
use crate::style::{StyleExt as _, col, row};
use crate::theme::Themed;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

// ---- IconButton -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconButtonSize {
    /// 18px: inside a field or a tag.
    Xs,
    /// 24px: rows and panel headers.
    #[default]
    Sm,
    /// 28px.
    Md,
    /// Same height as a text field.
    Lg,
}

/// A button that is only an icon. Always give it a `title`: it is its tooltip and its name
/// for a screen reader.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    title: Option<SharedString>,
    size: IconButtonSize,
    danger: bool,
    active: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            title: None,
            size: IconButtonSize::default(),
            danger: false,
            active: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Turns red under the pointer: delete, remove.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// A toggle that is on.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
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

impl RenderOnce for IconButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let (side, icon_size) = match self.size {
            IconButtonSize::Xs => (px(18.), IconSize::Sm),
            IconButtonSize::Sm => (px(24.), IconSize::Md),
            IconButtonSize::Md => (m.control_h_md, IconSize::Md),
            IconButtonSize::Lg => (m.control_h_lg, IconSize::Md),
        };
        let (hover_bg, hover_ink) = if self.danger { (c.danger_soft, c.danger) } else { (c.bg_hover, c.text) };
        let on_click = self.on_click;

        BaseButton::new(self.id)
            .disabled(self.disabled)
            .flex_none()
            .size(side)
            .border_1()
            .border_color(transparent_black())
            .text_color(if self.active { c.accent } else { c.text_muted })
            .when(self.active, |this| this.bg(c.bg_selected))
            .cursor(if self.disabled { CursorStyle::OperationNotAllowed } else { CursorStyle::PointingHand })
            .hover(|s| s.bg(hover_bg).text_color(hover_ink))
            .active(|s| s.bg(if self.danger { c.danger_soft } else { c.bg_active }))
            .focus_visible(|s| s.border_color(c.focus_ring))
            .styles(|styles| styles.disabled(|s| s.text_color(c.text_disabled)))
            .when_some(self.title.clone(), |this, title| this.accessibility_label(title))
            .child(Icon::new(self.icon).size(icon_size))
            .when_some(self.title, |this, title| this.tooltip(tooltip(title)))
            .when_some(on_click, |this, handler| this.on_click(move |event, window, cx| handler(event, window, cx)))
    }
}

// ---- FormField ------------------------------------------------------------------------------

/// A label above a control, with a hint or an error under it. The child is the control.
#[derive(IntoElement)]
pub struct FormField {
    label: SharedString,
    required: bool,
    hint: Option<SharedString>,
    error: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl FormField {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into(), required: false, hint: None, error: None, children: Vec::new() }
    }

    /// Marks the label with a red asterisk.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Replaces the hint while the value is refused.
    pub fn error(mut self, error: Option<impl Into<SharedString>>) -> Self {
        self.error = error.map(Into::into);
        self
    }
}

impl ParentElement for FormField {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for FormField {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        col()
            .w_full()
            .gap(m.space_2)
            .child(
                row()
                    .gap(px(2.))
                    .fs_tight(m.fs_sm)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c.text)
                    .child(self.label)
                    .when(self.required, |this| this.child(div().text_color(c.danger).child("*"))),
            )
            .children(self.children)
            .map(|this| match (self.error, self.hint) {
                (Some(error), _) => this.child(div().fs(m.fs_xs).text_color(c.danger).child(error)),
                (None, Some(hint)) => this.child(div().fs(m.fs_xs).text_color(c.text_subtle).child(hint)),
                (None, None) => this,
            })
    }
}

// ---- Checkbox and Switch --------------------------------------------------------------------

/// A box to tick. Stateless: the owner holds `checked` and changes it in `on_change`.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<ToggleHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self { id: id.into(), checked, label: None, disabled: false, on_change: None }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        use gpui_kit::component::Disableable as _;
        let on_change = self.on_change;
        KitCheckbox::new(self.id)
            .checked(self.checked)
            .disabled(self.disabled)
            .when_some(self.label, |this, label| this.label(label))
            .when_some(on_change, |this, handler| this.on_click(move |checked, window, cx| handler(*checked, window, cx)))
    }
}

/// On or off, applied at once. Stateless, like [`Checkbox`].
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<ToggleHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self { id: id.into(), checked, label: None, disabled: false, on_change: None }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        use gpui_kit::component::Disableable as _;
        let accent = cx.ui().colors.accent;
        let on_change = self.on_change;
        KitSwitch::new(self.id)
            .checked(self.checked)
            .color(accent)
            .disabled(self.disabled)
            .when_some(self.label, |this, label| this.label(label))
            .when_some(on_change, |this, handler| this.on_click(move |checked, window, cx| handler(*checked, window, cx)))
    }
}

// ---- Select ---------------------------------------------------------------------------------

/// One choice of a [`Select`]: the value the code reads, the label the user sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub value: SharedString,
    pub label: SharedString,
}

impl SelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self { value: value.into(), label: label.into() }
    }
}

impl SearchableListItem for SelectOption {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

/// The choice of one [`Select`], owned by the view that reads it. Clones share the choice.
#[derive(Clone)]
pub struct SelectModel {
    state: Entity<SelectState<Vec<SelectOption>>>,
}

impl SelectModel {
    /// `selected` is the value to start on, when it is one of the options.
    pub fn new(options: Vec<SelectOption>, selected: Option<&str>, window: &mut Window, cx: &mut App) -> Self {
        let index = selected.and_then(|value| options.iter().position(|o| o.value.as_ref() == value)).map(IndexPath::new);
        Self { state: cx.new(|cx| SelectState::new(options, index, window, cx)) }
    }

    /// The chosen value, or `None` when nothing is chosen yet.
    pub fn value(&self, cx: &App) -> Option<String> {
        self.state.read(cx).selected_value().map(|v| v.to_string())
    }

    pub fn set_value(&self, value: &str, window: &mut Window, cx: &mut App) {
        let value = SharedString::from(value.to_string());
        self.state.update(cx, |state, cx| state.set_selected_value(&value, window, cx));
    }

    /// Call `handler` on the owning view when the user picks another option.
    pub fn on_change<V: 'static>(
        &self,
        window: &mut Window,
        cx: &mut Context<V>,
        handler: impl Fn(&mut V, Option<String>, &mut Window, &mut Context<V>) + 'static,
    ) -> Subscription {
        cx.subscribe_in(&self.state, window, move |view, _, event: &SelectEvent<Vec<SelectOption>>, window, cx| {
            let SelectEvent::Confirm(value) = event;
            handler(view, value.as_ref().map(|v| v.to_string()), window, cx)
        })
    }
}

/// A dropdown over a fixed list of options.
#[derive(IntoElement)]
pub struct Select {
    model: SelectModel,
    placeholder: Option<SharedString>,
    disabled: bool,
}

impl Select {
    pub fn new(model: &SelectModel) -> Self {
        Self { model: model.clone(), placeholder: None, disabled: false }
    }

    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Select {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let m = cx.ui().m;
        div().w_full().text_size(m.fs_md).child(
            KitSelect::new(&self.model.state)
                .h(m.control_h_lg)
                .disabled(self.disabled)
                .when_some(self.placeholder, |this, text| this.placeholder(text)),
        )
    }
}

// ---- SearchField ----------------------------------------------------------------------------

/// The search box of a list: a magnifier, the text, a cross to empty it.
#[derive(IntoElement)]
pub struct SearchField {
    model: TextModel,
    width: Option<Pixels>,
    tall: bool,
}

impl SearchField {
    pub fn new(model: &TextModel) -> Self {
        Self { model: model.clone(), width: None, tall: false }
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// The till's search box.
    pub fn tall(mut self) -> Self {
        self.tall = true;
        self
    }
}

impl RenderOnce for SearchField {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let input = Input::new(&self.model).icon(IconName::Search).clearable();
        div()
            .flex_none()
            .map(|this| match self.width {
                Some(width) => this.w(width),
                None => this.w(px(280.)),
            })
            .child(if self.tall { input.tall() } else { input })
    }
}

// ---- Tabs -----------------------------------------------------------------------------------

/// One tab: a stable id, a label, and optionally an icon and a count.
#[derive(Debug, Clone, PartialEq)]
pub struct TabItem {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<IconName>,
    pub badge: Option<SharedString>,
}

impl TabItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: label.into(), icon: None, badge: None }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }
}

/// A row of tabs over the content they switch. Stateless: the owner holds the active id.
#[derive(IntoElement)]
pub struct Tabs {
    id: ElementId,
    items: Vec<TabItem>,
    active: SharedString,
    small: bool,
    on_change: Option<Rc<dyn Fn(&SharedString, &mut Window, &mut App)>>,
}

impl Tabs {
    pub fn new(id: impl Into<ElementId>, items: Vec<TabItem>, active: impl Into<SharedString>) -> Self {
        Self { id: id.into(), items, active: active.into(), small: false, on_change: None }
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Tabs {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let (height, pad, font) =
            if self.small { (px(30.), m.space_2, m.fs_sm) } else { (px(36.), m.space_3, m.fs_md) };
        let on_change = self.on_change;

        row().id(self.id).w_full().gap(m.space_1).hairline_bottom(c.border).children(self.items.into_iter().map(
            |item| {
                let active = item.id == self.active;
                let on_change = on_change.clone();
                let id = item.id.clone();
                BaseButton::new(ElementId::Name(item.id.clone()))
                    .flex_none()
                    .h(height)
                    .px(pad)
                    .gap(m.space_2)
                    .border_b_2()
                    .border_color(if active { c.accent } else { transparent_black() })
                    .text_color(if active { c.accent } else { c.text_muted })
                    .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                    .fs_tight(font)
                    .cursor(CursorStyle::PointingHand)
                    .selected(active)
                    .hover(|s| if active { s } else { s.bg(c.bg_hover).text_color(c.text) })
                    .when_some(item.icon, |this, icon| this.child(Icon::new(icon)))
                    .child(item.label)
                    .when_some(item.badge, |this, badge| {
                        this.child(
                            row()
                                .justify_center()
                                .min_w(px(18.))
                                .h(px(18.))
                                .px(px(5.))
                                .bg(if active { c.accent_soft } else { c.bg_app })
                                .text_color(if active { c.accent } else { c.text_muted })
                                .text_size(m.fs_xs)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(badge),
                        )
                    })
                    .when_some(on_change, |this, handler| this.on_click(move |_, window, cx| handler(&id, window, cx)))
            },
        ))
    }
}
