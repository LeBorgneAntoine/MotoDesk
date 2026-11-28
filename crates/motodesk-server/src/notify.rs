//! Telling the people at the PC about something the service did on its own.
//!
//! A request has someone waiting for its answer. Background work (an automatic backup, a change
//! from another PC) has nobody: this is its way to the screen. Call [`post`] from any thread;
//! the host shows it in its notification centre, which registers itself with [`set_host`]. With
//! no host (the tests, a tool) the call does nothing, so log what matters as well.
//!
//! A notice has a `key`. Posting the same key again replaces it, so a task that fails every
//! hour leaves one notice, and [`retract`] removes it once things work again.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    /// Something is blocked or at risk until it is dealt with.
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Stable name of what is reported, such as `backup.auto-failed`.
    pub key: String,
    pub level: Level,
    /// A few words, in French: this is read at the counter.
    pub title: String,
    /// What happened and what to do about it.
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Post(Notice),
    Retract(String),
}

type Host = Box<dyn Fn(Message) + Send + Sync>;

static HOST: OnceLock<Host> = OnceLock::new();

/// Called once by the host, before the service starts: where notices go.
pub fn set_host(show: impl Fn(Message) + Send + Sync + 'static) {
    let _ = HOST.set(Box::new(show));
}

fn send(message: Message) {
    if let Some(host) = HOST.get() {
        host(message);
    }
}

pub fn post(key: impl Into<String>, level: Level, title: impl Into<String>, body: impl Into<String>) {
    send(Message::Post(Notice { key: key.into(), level, title: title.into(), body: body.into() }));
}

/// What was reported under `key` no longer holds.
pub fn retract(key: impl Into<String>) {
    send(Message::Retract(key.into()));
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn what_a_task_posts_reaches_the_host() {
        // Without a host nothing happens, and nothing breaks.
        post("early", Level::Info, "Trop tôt", "");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        set_host(move |message| kept.lock().unwrap().push(message));

        post("backup.auto-failed", Level::Warning, "Sauvegarde automatique échouée", "Voir les journaux.");
        retract("backup.auto-failed");

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(matches!(&seen[0], Message::Post(notice) if notice.key == "backup.auto-failed" && notice.level == Level::Warning));
        assert_eq!(seen[1], Message::Retract("backup.auto-failed".into()));
    }
}
