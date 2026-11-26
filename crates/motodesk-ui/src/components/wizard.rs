//! What a step-by-step screen is made of: the line of steps at its top, and the area a file
//! is dropped on.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, CursorStyle, ElementId, ExternalPaths, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::components::content::Spinner;
use crate::components::icon::{Icon, IconName};
use crate::style::{StyleExt as _, col, row};
use crate::theme::Themed;

type StepHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;
type PathsHandler = Rc<dyn Fn(&[PathBuf], &mut Window, &mut App)>;
type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

// ---- Stepper --------------------------------------------------------------------------------

/// Where one is in a sequence of steps. A step already done can be gone back to when
/// `on_select` is set; the steps ahead cannot be jumped to.
#[derive(IntoElement)]
pub struct Stepper {
    id: ElementId,
    /// `(label, hint)` for each step.
    steps: Vec<(SharedString, SharedString)>,
    current: usize,
    on_select: Option<StepHandler>,
}

impl Stepper {
    /// `current` counts from zero.
    pub fn new(id: impl Into<ElementId>, current: usize) -> Self {
        Self { id: id.into(), steps: Vec::new(), current, on_select: None }
    }

    /// Add a step: its name, and a few words under it (empty for none).
    pub fn step(mut self, label: impl Into<SharedString>, hint: impl Into<SharedString>) -> Self {
        self.steps.push((label.into(), hint.into()));
        self
    }

    /// Called with the position of a finished step that was clicked.
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Stepper {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let last = self.steps.len().saturating_sub(1);
        let current = self.current;
        let on_select = self.on_select;
        let items = self.steps.into_iter().enumerate().map(|(index, (label, hint))| {
            let (done, active) = (index < current, index == current);
            let marker = div()
                .size(m.control_h_sm)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .fs_tight(m.fs_xs)
                .font_weight(FontWeight::SEMIBOLD)
                .map(|this| {
                    if done {
                        this.bg(c.accent).border_color(c.accent).text_color(c.text_on_accent).child(Icon::new(IconName::Check).sm())
                    } else if active {
                        this.border_color(c.accent).text_color(c.accent).child((index + 1).to_string())
                    } else {
                        this.border_color(c.border_strong).text_color(c.text_subtle).child((index + 1).to_string())
                    }
                });
            let text = col()
                .min_w_0()
                .child(
                    div()
                        .fs_tight(m.fs_sm)
                        .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                        .text_color(if done || active { c.text_strong } else { c.text_muted })
                        .child(label),
                )
                .when(!hint.is_empty(), |this| this.child(div().fs_tight(m.fs_xs).text_color(c.text_subtle).child(hint)));
            let step = row()
                .id(ElementId::NamedInteger("step".into(), index as u64))
                .flex_none()
                .gap(m.space_2)
                .child(marker)
                .child(text)
                .when_some(on_select.clone().filter(|_| done), |this, handler| {
                    this.cursor(CursorStyle::PointingHand).on_click(move |_, window, cx| handler(index, window, cx))
                });
            // The line to the next step: coloured once this one is behind.
            row().when(index < last, |this| this.flex_1()).min_w_0().gap(m.space_3).child(step).when(index < last, |this| {
                this.child(div().flex_1().min_w(m.space_4).h(px(1.)).bg(if done { c.accent } else { c.border }))
            })
        });
        row().id(self.id).w_full().gap(m.space_3).children(items)
    }
}

// ---- DropZone -------------------------------------------------------------------------------

/// A framed area a file is dropped on, or clicked to choose one. It tells of the paths
/// dropped; reading them is the caller's.
#[derive(IntoElement)]
pub struct DropZone {
    id: ElementId,
    icon: IconName,
    title: SharedString,
    lines: Vec<SharedString>,
    action: Option<AnyElement>,
    busy: bool,
    on_paths: Option<PathsHandler>,
    on_click: Option<ClickHandler>,
}

impl DropZone {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon: IconName::Upload,
            title: title.into(),
            lines: Vec::new(),
            action: None,
            busy: false,
            on_paths: None,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = icon;
        self
    }

    /// A line of smaller text under the title. Several calls stack.
    pub fn line(mut self, text: impl Into<SharedString>) -> Self {
        self.lines.push(text.into());
        self
    }

    /// Several lines at once (or none: an `Option` works).
    pub fn lines<T: Into<SharedString>>(mut self, texts: impl IntoIterator<Item = T>) -> Self {
        self.lines.extend(texts.into_iter().map(Into::into));
        self
    }

    /// The button under the text ("Choisir un fichier").
    pub fn action(mut self, button: impl IntoElement) -> Self {
        self.action = Some(button.into_any_element());
        self
    }

    /// A spinner in place of the icon, and nothing can be dropped or clicked.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// Called with the files dropped on the area.
    pub fn on_paths(mut self, handler: impl Fn(&[PathBuf], &mut Window, &mut App) + 'static) -> Self {
        self.on_paths = Some(Rc::new(handler));
        self
    }

    /// Called when the area itself is clicked.
    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for DropZone {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let busy = self.busy;
        col()
            .id(self.id)
            .w_full()
            .items_center()
            .gap(m.space_2)
            .px(m.space_5)
            .py(m.space_8)
            .bg(c.bg_app)
            .border_1()
            .border_dashed()
            .border_color(c.border_strong)
            .text_center()
            .child(if busy {
                Spinner::new().into_any_element()
            } else {
                Icon::new(self.icon).xl().color(c.text_subtle).into_any_element()
            })
            .child(div().fs(m.fs_lg).font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(self.title))
            .children(self.lines.into_iter().map(|line| div().fs(m.fs_sm).text_color(c.text_muted).child(line)))
            .children(self.action.map(|action| div().pt(m.space_2).child(action)))
            .when(!busy, |this| {
                this.drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(c.accent_subtle).border_color(c.accent))
                    .when_some(self.on_paths, |this, handler| {
                        this.on_drop(move |dropped: &ExternalPaths, window, cx| handler(dropped.paths(), window, cx))
                    })
                    .when_some(self.on_click, |this, handler| {
                        this.cursor(CursorStyle::PointingHand).on_click(move |event, window, cx| handler(event, window, cx))
                    })
            })
    }
}
