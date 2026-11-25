//! Text entry: the single-line field and the multi-line one.
//!
//! A field has two halves. The [`TextModel`] holds the text, the caret and the selection; the
//! view that owns the form creates it once and keeps it. [`Input`] draws it, each frame.
//!
//! ```ignore
//! let name = TextModel::builder().placeholder("Nom").build(window, cx);   // in the view's `new`
//! Input::new(&self.name).invalid(self.name_error.is_some())              // in `render`
//! self.name.text(cx)                                                     // on submit
//! ```

use gpui_kit::component::input::{
    Input as KitInput, InputEvent, InputState, Textarea as KitTextarea, TextareaState,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Subscription, Window, div,
};

use crate::components::icon::{Icon, IconName};
use crate::theme::{FONT_MONO, Themed};

/// What happened in a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEvent {
    /// The text changed (typing, paste, undo).
    Changed,
    /// Enter was pressed. `with_modifier` for Ctrl+Enter.
    Submitted { with_modifier: bool },
    Focused,
    Blurred,
}

impl TextEvent {
    fn from_kit(event: &InputEvent) -> Self {
        match event {
            InputEvent::Change => TextEvent::Changed,
            InputEvent::PressEnter { secondary, .. } => TextEvent::Submitted { with_modifier: *secondary },
            InputEvent::Focus => TextEvent::Focused,
            InputEvent::Blur => TextEvent::Blurred,
        }
    }
}

#[derive(Clone)]
enum Kind {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

/// The text of one field. Cheap to clone; clones share the same text.
#[derive(Clone)]
pub struct TextModel {
    kind: Kind,
}

/// Options of a field, set before it exists. See [`TextModel::builder`].
#[derive(Default)]
pub struct TextModelBuilder {
    placeholder: SharedString,
    value: SharedString,
    password: bool,
    rows: Option<usize>,
}

impl TextModelBuilder {
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// The text the field starts with.
    pub fn value(mut self, text: impl Into<SharedString>) -> Self {
        self.value = text.into();
        self
    }

    /// Dots instead of characters.
    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    /// Several lines, `rows` of them visible. Drawn by [`Textarea`].
    pub fn multiline(mut self, rows: usize) -> Self {
        self.rows = Some(rows.max(2));
        self
    }

    pub fn build(self, window: &mut Window, cx: &mut App) -> TextModel {
        let Self { placeholder, value, password, rows } = self;
        let kind = match rows {
            Some(rows) => Kind::Area(cx.new(|cx| {
                TextareaState::new(window, cx).rows(rows).placeholder(placeholder).default_value(value)
            })),
            None => Kind::Line(cx.new(|cx| {
                InputState::new(window, cx).masked(password).placeholder(placeholder).default_value(value)
            })),
        };
        TextModel { kind }
    }
}

impl TextModel {
    pub fn builder() -> TextModelBuilder {
        TextModelBuilder::default()
    }

    /// An empty single-line field.
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self::builder().build(window, cx)
    }

    pub fn text(&self, cx: &App) -> String {
        match &self.kind {
            Kind::Line(state) => state.read(cx).value().to_string(),
            Kind::Area(state) => state.read(cx).value().to_string(),
        }
    }

    /// The text without the spaces around it, or `None` when nothing is left: what a form
    /// sends for an optional field.
    pub fn trimmed(&self, cx: &App) -> Option<String> {
        let text = self.text(cx);
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    pub fn set_text(&self, text: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        let text: SharedString = text.into();
        match &self.kind {
            Kind::Line(state) => state.update(cx, |state, cx| state.set_value(text, window, cx)),
            Kind::Area(state) => state.update(cx, |state, cx| state.set_value(text, window, cx)),
        }
    }

    pub fn clear(&self, window: &mut Window, cx: &mut App) {
        self.set_text("", window, cx);
    }

    /// Put the caret in the field.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        match &self.kind {
            Kind::Line(state) => state.update(cx, |state, cx| state.focus(window, cx)),
            Kind::Area(state) => state.update(cx, |state, cx| state.focus(window, cx)),
        }
    }

    /// Call `handler` on the owning view for everything that happens in the field. Keep the
    /// returned subscription for as long as the view lives.
    pub fn on_event<V: 'static>(
        &self,
        window: &mut Window,
        cx: &mut Context<V>,
        handler: impl Fn(&mut V, TextEvent, &mut Window, &mut Context<V>) + 'static,
    ) -> Subscription {
        match &self.kind {
            Kind::Line(state) => cx.subscribe_in(state, window, move |view, _, event: &InputEvent, window, cx| {
                handler(view, TextEvent::from_kit(event), window, cx)
            }),
            Kind::Area(state) => cx.subscribe_in(state, window, move |view, _, event: &InputEvent, window, cx| {
                handler(view, TextEvent::from_kit(event), window, cx)
            }),
        }
    }
}

/// Draws a [`TextModel`]. One line, or several when the model is multi-line.
#[derive(IntoElement)]
pub struct Input {
    model: TextModel,
    icon: Option<IconName>,
    invalid: bool,
    disabled: bool,
    readonly: bool,
    mono: bool,
    clearable: bool,
    reveal: bool,
    tall: bool,
}

impl Input {
    pub fn new(model: &TextModel) -> Self {
        Self {
            model: model.clone(),
            icon: None,
            invalid: false,
            disabled: false,
            readonly: false,
            mono: false,
            clearable: false,
            reveal: false,
            tall: false,
        }
    }

    /// An icon at the start of the field.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The value was refused: red border.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    /// Fixed-width digits and letters: references, serial numbers.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// A cross at the end that empties the field.
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// For a password: an eye at the end that shows the characters.
    pub fn reveal_toggle(mut self) -> Self {
        self.reveal = true;
        self
    }

    /// The taller field of the sign-in card and the till.
    pub fn tall(mut self) -> Self {
        self.tall = true;
        self
    }
}

impl RenderOnce for Input {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let height = if self.tall { m.control_h_xl } else { m.control_h_lg };

        let field = match &self.model.kind {
            Kind::Line(state) => exact_height(KitInput::new(state), height)
                .disabled(self.disabled)
                .readonly(self.readonly)
                .cleanable(self.clearable)
                .when(self.reveal, |this| this.mask_toggle())
                .when_some(self.icon, |this, icon| this.prefix(Icon::new(icon).color(c.text_subtle)))
                .when(self.invalid, |this| this.border_color(c.danger))
                .when(self.mono, |this| this.font_family(FONT_MONO))
                .into_any_element(),
            Kind::Area(state) => KitTextarea::new(state)
                .disabled(self.disabled)
                .readonly(self.readonly)
                .when(self.invalid, |this| this.border_color(c.danger))
                .when(self.mono, |this| this.font_family(FONT_MONO))
                .into_any_element(),
        };
        div().w_full().text_size(m.fs_md).child(field)
    }
}

/// The Kit sizes a field from its own scale; Motodesk's control heights are set here instead.
fn exact_height(mut input: KitInput, height: gpui_kit::Pixels) -> KitInput {
    use gpui_kit::Styled as _;
    input.style().size.height = Some(height.into());
    input
}

/// The multi-line field: [`Input`] over a model built with [`TextModelBuilder::multiline`].
pub struct Textarea;

impl Textarea {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(model: &TextModel) -> Input {
        Input::new(model)
    }
}
