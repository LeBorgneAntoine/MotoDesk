//! Static content: headings, tags, alerts, dividers, the spinner, and the page that says
//! "loading", "nothing here" or "it failed".

use std::rc::Rc;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::spinner::Spinner as KitSpinner;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, FontWeight, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, Styled as _,
    Window, div, px,
};

use crate::components::button::Button;
use crate::components::icon::{Icon, IconName};
use crate::style::{StyleExt as _, col, row};
use crate::theme::{Themed, line};
use crate::tokens::line_height;

// ---- Heading --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeadingSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
    /// Page titles.
    Xl2,
    /// Screen headlines.
    Xl3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Strong,
    Default,
    Muted,
}

/// A title. `caps()` is the small uppercase label above a group of fields.
#[derive(IntoElement)]
pub struct Heading {
    text: SharedString,
    size: HeadingSize,
    tone: Tone,
    caps: bool,
}

impl Heading {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into(), size: HeadingSize::default(), tone: Tone::default(), caps: false }
    }

    pub fn size(mut self, size: HeadingSize) -> Self {
        self.size = size;
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn caps(mut self) -> Self {
        self.caps = true;
        self
    }
}

impl RenderOnce for Heading {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        if self.caps {
            return div()
                .fs_tight(m.fs_xs)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c.text_muted)
                .child(SharedString::from(self.text.to_uppercase()));
        }
        let size = match self.size {
            HeadingSize::Xs => m.fs_xs,
            HeadingSize::Sm => m.fs_sm,
            HeadingSize::Md => m.fs_md,
            HeadingSize::Lg => m.fs_lg,
            HeadingSize::Xl => m.fs_xl,
            HeadingSize::Xl2 => m.fs_2xl,
            HeadingSize::Xl3 => m.fs_3xl,
        };
        let color = match self.tone {
            Tone::Strong => c.text_strong,
            Tone::Default => c.text,
            Tone::Muted => c.text_muted,
        };
        div().fs_tight(size).font_weight(FontWeight::SEMIBOLD).text_color(color).child(self.text)
    }
}

// ---- Tag ------------------------------------------------------------------------------------

/// What a tag, an alert or a toast is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Neutral,
    Muted,
    Accent,
    Info,
    Success,
    Warning,
    Danger,
}

impl Status {
    /// (background, ink) of the soft fill used by tags and alerts.
    pub(crate) fn soft(self, cx: &App) -> (Hsla, Hsla) {
        let c = cx.ui().colors;
        match self {
            Status::Neutral => (c.bg_app, c.text),
            Status::Muted => (c.bg_app, c.text_muted),
            Status::Accent => (c.accent_soft, c.accent),
            Status::Info => (c.info_soft, c.info),
            Status::Success => (c.success_soft, c.success),
            Status::Warning => (c.warning_soft, c.warning),
            Status::Danger => (c.danger_soft, c.danger),
        }
    }
}

/// A short status word: "Payée", "En attente", "3 véhicules".
#[derive(IntoElement)]
pub struct Tag {
    label: SharedString,
    status: Status,
    icon: Option<IconName>,
    small: bool,
}

impl Tag {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into(), status: Status::default(), icon: None, small: true }
    }

    pub fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The larger tag, for a panel header.
    pub fn medium(mut self) -> Self {
        self.small = false;
        self
    }
}

impl RenderOnce for Tag {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let m = cx.ui().m;
        let (bg, ink) = self.status.soft(cx);
        let (py, px_, font) = if self.small { (px(3.), px(8.), m.fs_xs) } else { (px(5.), px(10.), m.fs_sm) };
        row()
            .flex_none()
            .gap(px(4.))
            .py(py)
            .px(px_)
            .bg(bg)
            .text_color(ink)
            .text_size(font)
            .line_height(font)
            .font_weight(if self.status == Status::Muted { FontWeight::NORMAL } else { FontWeight::MEDIUM })
            .whitespace_nowrap()
            .when_some(self.icon, |this, icon| this.child(Icon::new(icon).exact(font)))
            .child(self.label)
    }
}

// ---- Alert ----------------------------------------------------------------------------------

/// A message that stays on the page until its cause is gone. Children are the body: text, and
/// the actions (link buttons) that resolve it.
#[derive(IntoElement)]
pub struct Alert {
    status: Status,
    title: Option<SharedString>,
    flush: bool,
    children: Vec<AnyElement>,
}

impl Alert {
    pub fn new(status: Status) -> Self {
        Self { status, title: None, flush: false, children: Vec::new() }
    }

    pub fn info() -> Self {
        Self::new(Status::Info)
    }

    pub fn success() -> Self {
        Self::new(Status::Success)
    }

    pub fn warning() -> Self {
        Self::new(Status::Warning)
    }

    pub fn danger() -> Self {
        Self::new(Status::Danger)
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Edge to edge, for the strip under the title bar.
    pub fn flush(mut self) -> Self {
        self.flush = true;
        self
    }
}

impl ParentElement for Alert {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Alert {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let (bg, ink) = match self.status {
            Status::Neutral | Status::Muted | Status::Accent => (c.bg_app, c.border),
            other => other.soft(cx),
        };
        col()
            .gap(px(2.))
            .bg(bg)
            .border_l_3()
            .border_color(ink)
            .text_color(c.text)
            .fs(m.fs_sm)
            .map(|this| if self.flush { this.py(m.space_3).px(m.space_4) } else { this.py(m.space_2).px(m.space_3) })
            .when_some(self.title, |this, title| {
                this.child(div().font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(title))
            })
            .children(self.children)
    }
}

// ---- Divider --------------------------------------------------------------------------------

/// A hairline between two groups.
#[derive(IntoElement)]
pub struct Divider {
    vertical: bool,
}

impl Divider {
    pub fn horizontal() -> Self {
        Self { vertical: false }
    }

    pub fn vertical() -> Self {
        Self { vertical: true }
    }
}

impl RenderOnce for Divider {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = cx.ui().colors.border;
        if self.vertical {
            div().flex_none().w(px(1.)).h_full().min_h(px(12.)).bg(color)
        } else {
            div().flex_none().h(px(1.)).w_full().bg(color)
        }
    }
}

// ---- Spinner --------------------------------------------------------------------------------

/// Work in progress, with or without a word beside it.
#[derive(IntoElement)]
pub struct Spinner {
    label: Option<SharedString>,
    large: bool,
    color: Option<Hsla>,
}

impl Spinner {
    pub fn new() -> Self {
        Self { label: None, large: false, color: None }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn large(mut self) -> Self {
        self.large = true;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let color = self.color.unwrap_or(c.text_muted);
        let wheel = KitSpinner::new().color(color);
        row()
            .gap(m.space_2)
            .text_color(color)
            .fs(m.fs_sm)
            .child(if self.large { wheel.large() } else { wheel.small() })
            .when_some(self.label, |this, label| this.child(label))
    }
}

// ---- Preformatted ---------------------------------------------------------------------------

/// Text shown as it is, line for line, in fixed-width type: the first lines of a file.
/// A line too long for the block is cut at its edge, never wrapped.
#[derive(IntoElement)]
pub struct Preformatted {
    lines: Vec<SharedString>,
}

impl Preformatted {
    pub fn new(text: impl AsRef<str>) -> Self {
        // An empty line still takes its height.
        let lines = text.as_ref().lines().map(|line| if line.is_empty() { "\u{a0}".into() } else { line.to_string().into() });
        Self { lines: lines.collect() }
    }
}

impl RenderOnce for Preformatted {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        col()
            .w_full()
            .overflow_hidden()
            .p(m.space_3)
            .bg(c.bg_app)
            .border_1()
            .border_color(c.border)
            .font_family(crate::theme::FONT_MONO)
            .fs(m.fs_xs)
            .text_color(c.text)
            .children(self.lines.into_iter().map(|line| div().whitespace_nowrap().child(line)))
    }
}

// ---- StateView ------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    Loading,
    Empty,
    Error,
}

type Action = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// What a list, a panel or a whole page shows instead of its content: loading, nothing to
/// show, or a failure, with up to two actions.
#[derive(IntoElement)]
pub struct StateView {
    kind: StateKind,
    icon: Option<IconName>,
    title: Option<SharedString>,
    body: Option<SharedString>,
    action: Option<(SharedString, Action)>,
    secondary: Option<(SharedString, Action)>,
    compact: bool,
}

impl StateView {
    pub fn new(kind: StateKind) -> Self {
        Self { kind, icon: None, title: None, body: None, action: None, secondary: None, compact: false }
    }

    pub fn loading() -> Self {
        Self::new(StateKind::Loading)
    }

    pub fn empty() -> Self {
        Self::new(StateKind::Empty)
    }

    pub fn error() -> Self {
        Self::new(StateKind::Error)
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// The main way out ("Réessayer", "Nouveau client").
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }

    pub fn secondary(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.secondary = Some((label.into(), Rc::new(handler)));
        self
    }

    /// For a panel section or a small card.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

impl RenderOnce for StateView {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let compact = self.compact;

        let mark: AnyElement = match self.kind {
            StateKind::Loading => Spinner::new().large().into_any_element(),
            StateKind::Empty => {
                Icon::new(self.icon.unwrap_or(IconName::Inbox)).xl().color(c.text_subtle).into_any_element()
            }
            StateKind::Error => {
                Icon::new(self.icon.unwrap_or(IconName::CircleAlert)).xl().color(c.danger).into_any_element()
            }
        };
        let has_actions = self.action.is_some() || self.secondary.is_some();

        col()
            .size_full()
            .items_center()
            .justify_center()
            .text_center()
            .text_color(c.text_muted)
            .map(|this| {
                if compact {
                    this.py(m.space_5).px(m.space_3).gap(m.space_1)
                } else {
                    this.py(m.space_8).px(m.space_5).gap(m.space_2)
                }
            })
            .child(div().mb(m.space_1).child(mark))
            .when_some(self.title, |this, title| {
                this.child(
                    div()
                        .fs_tight(if compact { m.fs_md } else { m.fs_lg })
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c.text_strong)
                        .child(title),
                )
            })
            .when_some(self.body, |this, body| {
                let size = if compact { m.fs_xs } else { m.fs_sm };
                this.child(
                    div()
                        .max_w(if compact { px(240.) } else { px(320.) })
                        .text_size(size)
                        .line_height(line(size, line_height::RELAXED))
                        .child(body),
                )
            })
            .when(has_actions, |this| {
                this.child(
                    row()
                        .gap(m.space_2)
                        .mt(m.space_3)
                        .when_some(self.action, |this, (label, handler)| {
                            this.child(
                                Button::new("state-action")
                                    .primary()
                                    .label(label)
                                    .on_click(move |e, window, cx| handler(e, window, cx)),
                            )
                        })
                        .when_some(self.secondary, |this, (label, handler)| {
                            this.child(
                                Button::new("state-secondary")
                                    .label(label)
                                    .on_click(move |e, window, cx| handler(e, window, cx)),
                            )
                        }),
                )
            })
    }
}
