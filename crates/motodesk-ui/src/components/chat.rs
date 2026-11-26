//! Talking to the assistant: the field a message is typed in, and the text it answers with.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::input::Textarea as KitTextarea;
use gpui_kit::component::text::{TextView, TextViewStyle};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div, relative, rems};

use crate::components::content::Spinner;
use crate::components::controls::{IconButton, IconButtonSize};
use crate::components::icon::IconName;
use crate::components::text_field::TextModel;
use crate::style::with_focus_mark;
use crate::theme::Themed;
use crate::tokens::line_height;

type SendHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// The field a message is written in, with its send button. It starts one line tall and grows
/// with the text. Enter sends, Shift+Enter starts a new line.
///
/// Over a model built with [`crate::TextModelBuilder::composer`]. The owning view hears Enter
/// through [`TextModel::on_event`] (`TextEvent::Submitted`); `on_send` is the button.
#[derive(IntoElement)]
pub struct Composer {
    id: SharedString,
    model: TextModel,
    loading: bool,
    disabled: bool,
    on_send: Option<SendHandler>,
}

impl Composer {
    pub fn new(id: impl Into<SharedString>, model: &TextModel) -> Self {
        Self { id: id.into(), model: model.clone(), loading: false, disabled: false, on_send: None }
    }

    /// An answer is being written: the button turns into a spinner and nothing can be sent.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_send(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_send = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Composer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let focused = !self.disabled && self.model.is_focused(window, cx);
        let can_send = !self.disabled && !self.loading && !self.model.text(cx).trim().is_empty();
        let field = self.model.area().map(|state| KitTextarea::new(&state).appearance(false).disabled(self.disabled));
        let send = if self.loading {
            div().size(m.control_h_lg).flex().items_center().justify_center().child(Spinner::new()).into_any_element()
        } else {
            IconButton::new(ElementId::Name(format!("{}-send", self.id).into()), IconName::Send)
                .title("Envoyer")
                .size(IconButtonSize::Lg)
                .disabled(!can_send)
                .when_some(self.on_send, |this, handler| this.on_click(move |_, window, cx| handler(window, cx)))
                .into_any_element()
        };
        let frame = div()
            .id(ElementId::Name(self.id))
            .w_full()
            .flex()
            .items_end()
            .gap(m.space_1)
            .p(m.space_1)
            .bg(c.bg_surface)
            .border_1()
            .border_color(if focused { c.border_focus } else { c.border })
            .when(self.disabled, |this| this.opacity(0.6))
            .child(div().flex_1().min_w_0().text_size(m.fs_md).children(field))
            .child(send);
        with_focus_mark(frame, focused, c.border_focus)
    }
}

type LinkHandler = Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync>;

/// Text written in markdown, drawn as a document: headings, lists, tables, quotes, code. For
/// what the assistant answers and what it lays on its canvas. The text can be selected.
///
/// A link to the web opens in the browser. Any other link (a path of the app, a file, a
/// scheme of its own) goes to `on_link`, and does nothing without one: the text comes from a
/// model, and a click on it must not start a program.
#[derive(IntoElement)]
pub struct Prose {
    id: ElementId,
    source: SharedString,
    compact: bool,
    on_link: Option<LinkHandler>,
}

impl Prose {
    /// `id` must stay the same for the same text from one frame to the next.
    pub fn new(id: impl Into<ElementId>, source: impl Into<SharedString>) -> Self {
        Self { id: id.into(), source: source.into(), compact: false, on_link: None }
    }

    /// Tighter paragraphs, for a chat bubble.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Called with the target of a link that does not lead to the web.
    pub fn on_link(mut self, handler: impl Fn(&str, &mut Window, &mut App) + Send + Sync + 'static) -> Self {
        self.on_link = Some(Arc::new(handler));
        self
    }
}

/// Whether a link leads to a web page.
fn is_web_link(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("https://") || url.starts_with("http://")
}

impl RenderOnce for Prose {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let on_link = self.on_link;
        let gap = if self.compact { 0.5 } else { 0.75 };
        let text = TextView::markdown(self.id, self.source)
            .selectable(true)
            .style(TextViewStyle::default().paragraph_gap(rems(gap)))
            .on_link_click(move |url, _, window, cx| {
                if is_web_link(url) {
                    cx.open_url(url);
                } else if let Some(handler) = &on_link {
                    handler(url, window, cx);
                }
            });
        div().w_full().min_w_0().text_size(m.fs_md).line_height(relative(line_height::RELAXED)).text_color(c.text).child(text)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_web_links_open_in_the_browser() {
        assert!(super::is_web_link("https://openrouter.ai/keys"));
        assert!(super::is_web_link(" HTTP://exemple.fr "));
        assert!(!super::is_web_link("#/sales"));
        assert!(!super::is_web_link("file:///C:/Windows/System32/calc.exe"));
        assert!(!super::is_web_link("ms-settings:privacy"));
        assert!(!super::is_web_link("javascript:alert(1)"));
    }
}
