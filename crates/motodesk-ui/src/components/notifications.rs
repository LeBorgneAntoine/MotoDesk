//! The notification centre of the title bar: a bell with how many notices wait, and under it
//! the list of them.
//!
//! It holds no notices of its own: the application hands it what to show and whether the panel
//! is open, and is told when the user opens or closes it.

use std::rc::Rc;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button as KitButton, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Anchor, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};

use crate::components::button::Button;
use crate::components::content::Status;
use crate::components::controls::IconButton;
use crate::components::icon::{Icon, IconName};
use crate::style::{StyleExt as _, col, row};
use crate::theme::Themed as _;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;
type OpenHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// Something a notice offers to do about itself.
#[derive(Clone)]
pub struct NoticeAction {
    label: SharedString,
    primary: bool,
    busy: bool,
    handler: Handler,
}

impl NoticeAction {
    pub fn new(label: impl Into<SharedString>, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self { label: label.into(), primary: false, busy: false, handler: Rc::new(handler) }
    }

    /// The thing to do; the others are ways out.
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    /// Being done: shown as working, and not pressed again.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }
}

/// One entry of the list.
#[derive(Clone)]
pub struct Notice {
    id: SharedString,
    status: Status,
    title: SharedString,
    body: SharedString,
    actions: Vec<NoticeAction>,
    dismiss: Option<Handler>,
}

impl Notice {
    pub fn new(id: impl Into<SharedString>, status: Status, title: impl Into<SharedString>) -> Self {
        Self { id: id.into(), status, title: title.into(), body: SharedString::default(), actions: Vec::new(), dismiss: None }
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = body.into();
        self
    }

    pub fn action(mut self, action: NoticeAction) -> Self {
        self.actions.push(action);
        self
    }

    /// A cross to remove it, for a notice that tells of something that happened rather than of
    /// something still wrong.
    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.dismiss = Some(Rc::new(handler));
        self
    }
}

/// The most pressing of the statuses shown: the colour of the count.
fn gravest(notices: &[Notice]) -> Status {
    for status in [Status::Danger, Status::Warning] {
        if notices.iter().any(|notice| notice.status == status) {
            return status;
        }
    }
    Status::Info
}

#[derive(IntoElement)]
pub struct NotificationBell {
    notices: Vec<Notice>,
    open: bool,
    on_open_change: Option<OpenHandler>,
}

impl NotificationBell {
    pub fn new(notices: Vec<Notice>) -> Self {
        Self { notices, open: false, on_open_change: None }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn on_open_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for NotificationBell {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let count = self.notices.len();
        let ink = gravest(&self.notices).soft(cx).1;
        let label: SharedString = if count > 99 { "99+".into() } else { count.to_string().into() };

        let trigger = KitButton::new("notification-bell")
            .ghost()
            .small()
            .tooltip(match count {
                0 => "Notifications : rien à signaler".to_string(),
                1 => "1 notification".to_string(),
                many => format!("{many} notifications"),
            })
            .child(
                row()
                    .gap(m.space_1)
                    .child(Icon::new(IconName::Bell))
                    .when(count > 0, |this| {
                        this.child(
                            div()
                                .px(m.space_1)
                                .min_w(px(18.))
                                .flex()
                                .justify_center()
                                .bg(ink)
                                .text_color(c.text_on_accent)
                                .font_weight(FontWeight::SEMIBOLD)
                                .fs(m.fs_xs)
                                .child(label),
                        )
                    }),
            );

        let notices = self.notices;
        let on_open_change = self.on_open_change;
        Popover::new("notification-center")
            .anchor(Anchor::TopRight)
            .open(self.open)
            .when_some(on_open_change, |this, handler| this.on_open_change(move |open, window, cx| handler(*open, window, cx)))
            .trigger(trigger)
            .content(move |_, _, cx| list(&notices, cx))
    }
}

/// The panel's content.
fn list(notices: &[Notice], cx: &App) -> impl IntoElement + use<> {
    let theme = cx.ui();
    let (c, m) = (theme.colors, theme.m);
    let header = div().font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child("Notifications");
    let body = if notices.is_empty() {
        col()
            .items_center()
            .gap(m.space_2)
            .py(m.space_5)
            .text_color(c.text_muted)
            .child(Icon::new(IconName::CircleCheck))
            .child("Rien à signaler")
            .into_any_element()
    } else {
        col()
            .id("notification-list")
            .max_h(px(440.))
            .overflow_y_scroll()
            .gap(m.space_2)
            .children(notices.iter().map(|notice| entry(notice, cx)))
            .into_any_element()
    };
    col().w(px(380.)).gap(m.space_3).text_color(c.text).fs(m.fs_sm).child(header).child(body)
}

fn entry(notice: &Notice, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.ui();
    let (c, m) = (theme.colors, theme.m);
    let (bg, ink) = match notice.status {
        Status::Neutral | Status::Muted | Status::Accent => (c.bg_app, c.border),
        other => other.soft(cx),
    };
    let title = row()
        .items_start()
        .justify_between()
        .gap(m.space_2)
        .child(div().flex_1().min_w_0().font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(notice.title.clone()))
        .when_some(notice.dismiss.clone(), |this, dismiss| {
            this.child(
                IconButton::new(SharedString::from(format!("notice-dismiss-{}", notice.id)), IconName::X)
                    .title("Retirer")
                    .on_click(move |_, window, cx| dismiss(window, cx)),
            )
        });
    let actions = notice.actions.iter().enumerate().map(|(index, action)| {
        let handler = action.handler.clone();
        let button = Button::new(SharedString::from(format!("notice-{}-{index}", notice.id)))
            .label(action.label.clone())
            .small()
            .loading(action.busy)
            .on_click(move |_, window, cx| handler(window, cx));
        if action.primary { button.primary() } else { button.link() }
    });
    col()
        .flex_none()
        .gap(m.space_1)
        .px(m.space_3)
        .py(m.space_2)
        .bg(bg)
        .border_l_3()
        .border_color(ink)
        .child(title)
        .when(!notice.body.is_empty(), |this| this.child(div().text_color(c.text_muted).child(notice.body.clone())))
        .when(!notice.actions.is_empty(), |this| this.child(row().flex_wrap().gap(m.space_2).mt(m.space_1).children(actions)))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would take the place of Rust's.
    use super::{Notice, gravest};
    use crate::components::content::Status;

    #[test]
    fn the_count_takes_the_colour_of_the_most_pressing_notice() {
        let notice = |status| Notice::new("a", status, "A");
        assert!(gravest(&[]) == Status::Info);
        assert!(gravest(&[notice(Status::Info), notice(Status::Success)]) == Status::Info);
        assert!(gravest(&[notice(Status::Info), notice(Status::Warning)]) == Status::Warning);
        assert!(gravest(&[notice(Status::Warning), notice(Status::Danger)]) == Status::Danger);
    }
}
