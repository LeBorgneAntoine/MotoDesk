//! The notification centre: the one place the application tells the shop that something wants
//! attention. It is the bell of the title bar; nothing is laid over the screens.
//!
//! Three ways in:
//!
//! - **A condition** that holds or not (a day left unclosed, a backup that is late): compute the
//!   whole list for your part of the application and [`publish`] it under your own source name.
//!   What is no longer in the list disappears.
//! - **An event** from a screen (an export that finished, something refused): [`post`] it, and
//!   [`retract`] it if it stops being true. The user can remove it with its cross.
//! - **Background work**, with no window at hand (the data service, a task on another thread):
//!   `motodesk_server::notify::post`, which ends up here.
//!
//! Every notification has a `key`; the same key replaces what was there.

use std::rc::Rc;

use motodesk_server::notify;
use motodesk_ui::gpui::{App, AppContext as _, Entity, Global, SharedString, Window};
use motodesk_ui::{Notice, NoticeAction, NotificationBell, Status};
use tokio::sync::mpsc::UnboundedReceiver;

type Run = Rc<dyn Fn(&mut Window, &mut App)>;

/// What a notification offers to do about itself.
#[derive(Clone)]
pub struct Action {
    label: SharedString,
    primary: bool,
    busy: bool,
    keeps_open: bool,
    run: Run,
}

impl Action {
    /// Running it closes the centre: it leads somewhere else.
    pub fn new(label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self { label: label.into(), primary: false, busy: false, keeps_open: false, run: Rc::new(run) }
    }

    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// For what happens in place (« Plus tard », a long operation to watch).
    pub fn keeps_open(mut self) -> Self {
        self.keeps_open = true;
        self
    }
}

#[derive(Clone)]
pub struct Notification {
    key: SharedString,
    status: Status,
    title: SharedString,
    body: SharedString,
    actions: Vec<Action>,
    dismissible: bool,
}

impl Notification {
    pub fn new(key: impl Into<SharedString>, status: Status, title: impl Into<SharedString>) -> Self {
        Self { key: key.into(), status, title: title.into(), body: SharedString::default(), actions: Vec::new(), dismissible: false }
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = body.into();
        self
    }

    pub fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    /// The user may remove it: for something that happened, not for something still wrong.
    pub fn dismissible(mut self) -> Self {
        self.dismissible = true;
        self
    }

    #[cfg(test)]
    fn key(&self) -> &str {
        &self.key
    }

    /// Whether the two look the same (what an action does is not compared).
    fn looks_like(&self, other: &Self) -> bool {
        let face = |action: &Action| (action.label.clone(), action.primary, action.busy);
        self.key == other.key
            && self.status == other.status
            && self.title == other.title
            && self.body == other.body
            && self.dismissible == other.dismissible
            && self.actions.iter().map(face).eq(other.actions.iter().map(face))
    }
}

/// Where events and background work go, as opposed to a published source.
const POSTED: &str = "posted";

/// The list itself, free of any window.
#[derive(Default)]
pub struct Inbox {
    entries: Vec<(&'static str, Notification)>,
}

impl Inbox {
    /// Replace everything `source` had published. Says whether anything visible changed.
    pub fn publish(&mut self, source: &'static str, notifications: Vec<Notification>) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(from, kept)| *from != source || notifications.iter().any(|n| n.key == kept.key));
        let mut changed = before != self.entries.len();
        for notification in notifications {
            changed |= self.put(source, notification);
        }
        changed
    }

    pub fn post(&mut self, notification: Notification) -> bool {
        self.put(POSTED, notification)
    }

    fn put(&mut self, source: &'static str, notification: Notification) -> bool {
        match self.entries.iter_mut().find(|(_, kept)| kept.key == notification.key) {
            // Kept in its place; the newer actions are taken even when it looks the same.
            Some(entry) => {
                let changed = !entry.1.looks_like(&notification);
                *entry = (source, notification);
                changed
            }
            None => {
                self.entries.push((source, notification));
                true
            }
        }
    }

    pub fn retract(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(_, kept)| kept.key.as_ref() != key);
        before != self.entries.len()
    }

    /// The most pressing first, otherwise in the order they came.
    pub fn shown(&self) -> Vec<&Notification> {
        let rank = |status| match status {
            Status::Danger => 0,
            Status::Warning => 1,
            _ => 2,
        };
        let mut shown: Vec<&Notification> = self.entries.iter().map(|(_, notification)| notification).collect();
        shown.sort_by_key(|notification| rank(notification.status));
        shown
    }
}

pub struct Center {
    inbox: Inbox,
    open: bool,
}

struct GlobalCenter(Entity<Center>);

impl Global for GlobalCenter {}

/// Set up the centre and listen to background work. `from_service` is the other end of what
/// `main` registered with `motodesk_server::notify::set_host`.
pub fn init(mut from_service: UnboundedReceiver<notify::Message>, cx: &mut App) {
    let center = cx.new(|_| Center { inbox: Inbox::default(), open: false });
    cx.set_global(GlobalCenter(center));
    cx.spawn(async move |cx| {
        while let Some(message) = from_service.recv().await {
            cx.update(|cx| match message {
                notify::Message::Post(notice) => {
                    let status = match notice.level {
                        notify::Level::Info => Status::Info,
                        notify::Level::Warning => Status::Warning,
                        notify::Level::Danger => Status::Danger,
                    };
                    post(Notification::new(notice.key, status, notice.title).body(notice.body).dismissible(), cx);
                }
                notify::Message::Retract(key) => retract(&key, cx),
            });
        }
    })
    .detach();
}

pub fn center(cx: &App) -> Entity<Center> {
    cx.global::<GlobalCenter>().0.clone()
}

fn change(cx: &mut App, edit: impl FnOnce(&mut Center) -> bool) {
    center(cx).update(cx, |center, cx| {
        if edit(center) {
            cx.notify();
        }
    });
}

pub fn publish(source: &'static str, notifications: Vec<Notification>, cx: &mut App) {
    change(cx, |center| center.inbox.publish(source, notifications));
}

pub fn post(notification: Notification, cx: &mut App) {
    change(cx, |center| center.inbox.post(notification));
}

pub fn retract(key: &str, cx: &mut App) {
    change(cx, |center| center.inbox.retract(key));
}

fn set_open(open: bool, cx: &mut App) {
    change(cx, |center| std::mem::replace(&mut center.open, open) != open);
}

/// The bell of the title bar, with what the centre holds now.
pub fn bell(cx: &App) -> NotificationBell {
    let center = center(cx);
    let center = center.read(cx);
    let notices = center
        .inbox
        .shown()
        .into_iter()
        .map(|notification| {
            let mut notice = Notice::new(notification.key.clone(), notification.status, notification.title.clone()).body(notification.body.clone());
            for action in &notification.actions {
                let (run, keeps_open) = (action.run.clone(), action.keeps_open);
                let mut shown = NoticeAction::new(action.label.clone(), move |window, cx| {
                    if !keeps_open {
                        set_open(false, cx);
                    }
                    run(window, cx);
                })
                .busy(action.busy);
                if action.primary {
                    shown = shown.primary();
                }
                notice = notice.action(shown);
            }
            if notification.dismissible {
                let key = notification.key.clone();
                notice = notice.on_dismiss(move |_, cx| retract(&key, cx));
            }
            notice
        })
        .collect();
    NotificationBell::new(notices).open(center.open).on_open_change(|open, _, cx| set_open(open, cx))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would take the place of Rust's.
    use motodesk_ui::Status;

    use super::{Inbox, Notification};

    fn note(key: &'static str, status: Status) -> Notification {
        Notification::new(key, status, key)
    }

    fn keys(inbox: &Inbox) -> Vec<&str> {
        inbox.shown().into_iter().map(Notification::key).collect()
    }

    #[test]
    fn a_source_replaces_what_it_published_and_leaves_the_others() {
        let mut inbox = Inbox::default();
        assert!(inbox.publish("shell", vec![note("closures", Status::Warning), note("till", Status::Info)]));
        assert!(inbox.post(note("export", Status::Success)));
        // The same again changes nothing, so nothing is redrawn.
        assert!(!inbox.publish("shell", vec![note("closures", Status::Warning), note("till", Status::Info)]));
        // The day got closed.
        assert!(inbox.publish("shell", vec![note("till", Status::Info)]));
        assert_eq!(keys(&inbox), ["till", "export"]);
        assert!(inbox.publish("shell", Vec::new()));
        assert_eq!(keys(&inbox), ["export"]);
    }

    #[test]
    fn a_key_is_shown_once_and_its_changes_are_seen() {
        let mut inbox = Inbox::default();
        inbox.post(note("backup", Status::Warning));
        assert!(!inbox.post(note("backup", Status::Warning)));
        assert!(inbox.post(note("backup", Status::Warning).body("Disque débranché.")));
        assert_eq!(keys(&inbox), ["backup"]);
        assert!(inbox.retract("backup"));
        assert!(!inbox.retract("backup"));
        assert!(keys(&inbox).is_empty());
    }

    #[test]
    fn the_most_pressing_come_first() {
        let mut inbox = Inbox::default();
        for (key, status) in [("till", Status::Info), ("closures", Status::Warning), ("gap", Status::Danger), ("export", Status::Success), ("backup", Status::Warning)] {
            inbox.post(note(key, status));
        }
        assert_eq!(keys(&inbox), ["gap", "closures", "backup", "till", "export"]);
    }
}
