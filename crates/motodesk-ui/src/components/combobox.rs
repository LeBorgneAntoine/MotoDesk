//! A field that searches as the user types and offers the matches under it: pick a customer,
//! a product, a supplier among thousands.
//!
//! The owner does the searching. It is told what was typed ([`ComboModel::on_query`]), asks
//! its source, and hands back the matches ([`ComboModel::set_options`]). A choice is made when
//! the user picks a row; editing the text afterwards unmakes it.
//!
//! ```ignore
//! let buyer = ComboModel::new("Nom, téléphone…", window, cx);          // in the view's `new`
//! buyer.on_query(cx, move |query, _, cx| page.update(cx, |p, cx| p.search_buyers(query, cx)));
//! buyer.on_pick(cx, move |_, _, cx| page.update(cx, |_, cx| cx.notify()));
//! Combobox::new(&self.buyer)                                           // in `render`
//! self.buyer.value(cx)                                                 // the picked id
//! ```

use std::rc::Rc;

use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{Input as KitInput, InputState};
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Task, WeakEntity, Window, div,
};

use crate::components::icon::{Icon, IconName};
use crate::style::{StyleExt as _, col, with_focus_mark};
use crate::theme::Themed;

/// One match: the value the code reads, the label the user sees, a line of detail under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComboOption {
    pub value: SharedString,
    pub label: SharedString,
    pub detail: Option<SharedString>,
}

impl ComboOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self { value: value.into(), label: label.into(), detail: None }
    }

    /// A second line that tells two matches apart: a phone number, a reference.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        let detail: SharedString = detail.into();
        self.detail = (!detail.trim().is_empty()).then_some(detail);
        self
    }
}

type QueryHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type PickHandler = Rc<dyn Fn(Option<&ComboOption>, &mut Window, &mut App)>;

struct Matches {
    options: Vec<ComboOption>,
    highlighted: Option<IndexPath>,
    picked: Option<ComboOption>,
    loading: bool,
    input: WeakEntity<InputState>,
    on_query: Option<QueryHandler>,
    on_pick: Option<PickHandler>,
}

impl ListDelegate for Matches {
    type Item = ListItem;

    fn perform_search(&mut self, query: &str, window: &mut Window, cx: &mut Context<ListState<Self>>) -> Task<()> {
        // The text of a choice just made is not a new search.
        if self.picked.as_ref().is_some_and(|picked| picked.label.as_ref() == query) {
            return Task::ready(());
        }
        // Typing after a choice unmakes it.
        let unpicked = self.picked.take().is_some();
        let (on_query, on_pick, query) = (self.on_query.clone(), self.on_pick.clone(), query.to_string());
        // The owner will answer by updating this list: let the current update finish first.
        window.defer(cx, move |window, cx| {
            if let (true, Some(on_pick)) = (unpicked, on_pick) {
                on_pick(None, window, cx);
            }
            if let Some(on_query) = on_query {
                on_query(&query, window, cx);
            }
        });
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.options.len()
    }

    fn render_item(&mut self, ix: IndexPath, _: &mut Window, cx: &mut Context<ListState<Self>>) -> Option<Self::Item> {
        let option = self.options.get(ix.row)?;
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        Some(
            ListItem::new(ix).selected(self.highlighted == Some(ix)).child(
                col()
                    .py(m.space_1)
                    .gap(gpui_kit::px(1.))
                    .child(div().fs(m.fs_md).text_color(c.text).child(option.label.clone()))
                    .when_some(option.detail.clone(), |this, detail| {
                        this.child(div().fs(m.fs_xs).text_color(c.text_muted).child(detail))
                    }),
            ),
        )
    }

    fn set_selected_index(&mut self, ix: Option<IndexPath>, _: &mut Window, cx: &mut Context<ListState<Self>>) {
        self.highlighted = ix;
        cx.notify();
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(option) = self.highlighted.and_then(|ix| self.options.get(ix.row)).cloned() else { return };
        self.picked = Some(option.clone());
        // The choice is made: nothing left to offer.
        self.options.clear();
        self.highlighted = None;
        let (input, on_pick) = (self.input.clone(), self.on_pick.clone());
        window.defer(cx, move |window, cx| {
            let _ = input.update(cx, |input, cx| input.set_value(option.label.clone(), window, cx));
            if let Some(on_pick) = on_pick {
                on_pick(Some(&option), window, cx);
            }
        });
        cx.notify();
    }

    fn loading(&self, _: &App) -> bool {
        self.loading
    }
}

/// The text, the matches and the choice of one [`Combobox`]. Clones share them.
#[derive(Clone)]
pub struct ComboModel {
    input: Entity<InputState>,
    list: Entity<ListState<Matches>>,
}

impl ComboModel {
    pub fn new(placeholder: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> Self {
        let placeholder: SharedString = placeholder.into();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let matches = Matches {
            options: Vec::new(),
            highlighted: None,
            picked: None,
            loading: false,
            input: input.downgrade(),
            on_query: None,
            on_pick: None,
        };
        let list = cx.new(|cx| ListState::new(matches, window, cx));
        Self { input, list }
    }

    fn update(&self, cx: &mut App, change: impl FnOnce(&mut Matches)) {
        self.list.update(cx, |list, cx| {
            change(list.delegate_mut());
            cx.notify();
        });
    }

    /// What to do when the text changes: search, then call [`Self::set_options`].
    pub fn on_query(&self, cx: &mut App, handler: impl Fn(&str, &mut Window, &mut App) + 'static) {
        self.update(cx, |matches| matches.on_query = Some(Rc::new(handler)));
    }

    /// Called with the option picked, or `None` when the choice is unmade.
    pub fn on_pick(&self, cx: &mut App, handler: impl Fn(Option<&ComboOption>, &mut Window, &mut App) + 'static) {
        self.update(cx, |matches| matches.on_pick = Some(Rc::new(handler)));
    }

    /// The matches for the current text. Ends the loading state.
    pub fn set_options(&self, options: Vec<ComboOption>, cx: &mut App) {
        self.update(cx, |matches| {
            // A late answer must not reopen the list under a choice already made.
            if matches.picked.is_none() {
                matches.highlighted = None;
                matches.options = options;
            }
            matches.loading = false;
        });
    }

    /// A search is running.
    pub fn set_loading(&self, loading: bool, cx: &mut App) {
        self.update(cx, |matches| matches.loading = loading && matches.picked.is_none());
    }

    /// The value of the option picked, or `None` while nothing is chosen.
    pub fn value(&self, cx: &App) -> Option<String> {
        self.list.read(cx).delegate().picked.as_ref().map(|option| option.value.to_string())
    }

    /// Empty the field and forget the choice.
    pub fn clear(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |matches| {
            matches.picked = None;
            matches.options.clear();
            matches.highlighted = None;
            matches.loading = false;
        });
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }
}

/// Draws a [`ComboModel`].
#[derive(IntoElement)]
pub struct Combobox {
    model: ComboModel,
    invalid: bool,
    disabled: bool,
}

impl Combobox {
    pub fn new(model: &ComboModel) -> Self {
        Self { model: model.clone(), invalid: false, disabled: false }
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Combobox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let chosen = self.model.value(cx).is_some();
        let mut field = KitInput::new(&self.model.input).list(&self.model.list).disabled(self.disabled).prefix(
            Icon::new(if chosen { IconName::Check } else { IconName::Search })
                .color(if chosen { c.success } else { c.text_subtle }),
        );
        field.style().size.height = Some(m.control_h_lg.into());
        let focused = !self.disabled && self.model.input.read(cx).focus_handle(cx).is_focused(window);
        let mark = if self.invalid { c.danger } else { c.border_focus };
        div().w_full().text_size(m.fs_md).child(with_focus_mark(
            field.when(self.invalid, |this| this.border_color(c.danger)),
            focused,
            mark,
        ))
    }
}
