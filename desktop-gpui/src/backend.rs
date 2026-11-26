//! The bridge between the interface and the data service.
//!
//! The service (Axum, SQLite, sync) runs on Tokio threads inside this process; the interface
//! runs on GPUI's main thread. A screen never blocks on the service: it hands a piece of work
//! to [`ApiExt::api`], which runs it on the service's threads and brings the answer back to the
//! view, on the main thread.

use std::future::Future;
use std::path::PathBuf;

use motodesk_client::Client;
use motodesk_ui::gpui::{App, Context, Global, PathPromptOptions, Task, Window};

/// What the interface needs to reach the service. Set once the service has started.
pub struct Backend {
    pub runtime: tokio::runtime::Handle,
    pub client: Client,
}

impl Global for Backend {}

/// The client, or `None` while the service is still starting.
pub fn client(cx: &App) -> Option<Client> {
    cx.try_global::<Backend>().map(|backend| backend.client.clone())
}

/// Where the system's "save as" dialog starts: the user's documents, or wherever the app
/// runs when there is no such folder.
fn documents_dir() -> PathBuf {
    let documents = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|home| PathBuf::from(home).join("Documents"));
    documents.filter(|folder| folder.is_dir()).unwrap_or_else(|| PathBuf::from("."))
}

/// Development only: stand in for the system's file dialogs, so that a screen can be driven
/// without them. `MOTODESK_DEV_PICK_FILES` holds the files an "open" dialog answers (separated
/// the way `PATH` is); `MOTODESK_DEV_SAVE_DIR` is the folder a "save as" dialog answers in,
/// under the name suggested. Neither exists in a release build.
#[cfg(debug_assertions)]
mod dev {
    use std::path::PathBuf;

    pub fn picked() -> Option<Vec<PathBuf>> {
        std::env::var_os("MOTODESK_DEV_PICK_FILES").map(|paths| std::env::split_paths(&paths).collect())
    }

    pub fn save_path(suggested_name: &str) -> Option<PathBuf> {
        std::env::var_os("MOTODESK_DEV_SAVE_DIR").map(|folder| PathBuf::from(folder).join(suggested_name))
    }
}

#[cfg(not(debug_assertions))]
mod dev {
    use std::path::PathBuf;

    pub fn picked() -> Option<Vec<PathBuf>> {
        None
    }

    pub fn save_path(_: &str) -> Option<PathBuf> {
        None
    }
}

/// Ask where to save a file. `None` when the dialog was closed, or went away with the window.
fn ask_save_path<V: 'static>(cx: &mut Context<V>, suggested_name: &str) -> impl Future<Output = std::io::Result<Option<PathBuf>>> + use<V> {
    let stand_in = dev::save_path(suggested_name);
    let asked = stand_in.is_none().then(|| cx.prompt_for_new_path(&documents_dir(), Some(suggested_name)));
    async move {
        match (stand_in, asked) {
            (Some(path), _) => Ok(Some(path)),
            (None, Some(asked)) => match asked.await {
                Ok(Ok(path)) => Ok(path),
                Ok(Err(error)) => Err(std::io::Error::other(error.to_string())),
                Err(_) => Ok(None),
            },
            (None, None) => Ok(None),
        }
    }
}

/// Ask which files to open. Empty when the dialog was closed.
fn ask_paths<V: 'static>(cx: &mut Context<V>, prompt: &str, multiple: bool) -> impl Future<Output = std::io::Result<Vec<PathBuf>>> + use<V> {
    let stand_in = dev::picked();
    let asked = stand_in.is_none().then(|| {
        cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple, prompt: Some(prompt.to_string().into()) })
    });
    async move {
        match (stand_in, asked) {
            (Some(paths), _) => Ok(paths),
            (None, Some(asked)) => match asked.await {
                Ok(Ok(paths)) => Ok(paths.unwrap_or_default()),
                Ok(Err(error)) => Err(std::io::Error::other(error.to_string())),
                Err(_) => Ok(Vec::new()),
            },
            (None, None) => Ok(Vec::new()),
        }
    }
}

pub trait SaveExt<V: 'static> {
    /// Ask the user where to save `bytes` (the system's "save as" dialog, offering
    /// `suggested_name`), write them there, then call `done` on the view with the path
    /// written: `None` when the user closed the dialog.
    fn save_file(
        &mut self,
        window: &Window,
        suggested_name: &str,
        bytes: Vec<u8>,
        done: impl FnOnce(&mut V, std::io::Result<Option<PathBuf>>, &mut Window, &mut Context<V>) + 'static,
    );

    /// Only ask where: for a file that is fetched and written afterwards, a piece at a time.
    fn choose_save_path(
        &mut self,
        window: &Window,
        suggested_name: &str,
        done: impl FnOnce(&mut V, std::io::Result<Option<PathBuf>>, &mut Window, &mut Context<V>) + 'static,
    );
}

impl<V: 'static> SaveExt<V> for Context<'_, V> {
    fn save_file(
        &mut self,
        window: &Window,
        suggested_name: &str,
        bytes: Vec<u8>,
        done: impl FnOnce(&mut V, std::io::Result<Option<PathBuf>>, &mut Window, &mut Context<V>) + 'static,
    ) {
        let chosen = ask_save_path(self, suggested_name);
        self.spawn_in(window, async move |view, cx| {
            let outcome = match chosen.await {
                // Off the interface's thread: a year of entries is a file of some size.
                Ok(Some(path)) => cx.background_executor().spawn(async move { std::fs::write(&path, bytes).map(|()| Some(path)) }).await,
                other => other,
            };
            let _ = view.update_in(cx, |view, window, cx| done(view, outcome, window, cx));
        })
        .detach();
    }

    fn choose_save_path(
        &mut self,
        window: &Window,
        suggested_name: &str,
        done: impl FnOnce(&mut V, std::io::Result<Option<PathBuf>>, &mut Window, &mut Context<V>) + 'static,
    ) {
        let chosen = ask_save_path(self, suggested_name);
        self.spawn_in(window, async move |view, cx| {
            let outcome = chosen.await;
            let _ = view.update_in(cx, |view, window, cx| done(view, outcome, window, cx));
        })
        .detach();
    }
}

/// A file the user chose.
pub struct PickedFile {
    /// Its name, without the folders.
    pub name: String,
    /// Its size on disk.
    pub size: usize,
    /// What it holds; empty when it is larger than the caller said it would read.
    pub bytes: Vec<u8>,
}

pub trait PickExt<V: 'static> {
    /// Ask the user for one file (the system's "open" dialog), read it when it is no larger
    /// than `max_bytes`, then call `done` on the view: `None` when the dialog was closed.
    fn pick_file(
        &mut self,
        window: &Window,
        prompt: &str,
        max_bytes: usize,
        done: impl FnOnce(&mut V, std::io::Result<Option<PickedFile>>, &mut Window, &mut Context<V>) + 'static,
    );

    /// Ask the user for any number of files and hand back where they are, unread: for files
    /// that are sent a piece at a time. Empty when the dialog was closed.
    fn pick_files(
        &mut self,
        window: &Window,
        prompt: &str,
        done: impl FnOnce(&mut V, std::io::Result<Vec<PathBuf>>, &mut Window, &mut Context<V>) + 'static,
    );
}

impl<V: 'static> PickExt<V> for Context<'_, V> {
    fn pick_file(
        &mut self,
        window: &Window,
        prompt: &str,
        max_bytes: usize,
        done: impl FnOnce(&mut V, std::io::Result<Option<PickedFile>>, &mut Window, &mut Context<V>) + 'static,
    ) {
        let chosen = ask_paths(self, prompt, false);
        self.spawn_in(window, async move |view, cx| {
            let outcome = match chosen.await.map(|paths| paths.into_iter().next()) {
                Ok(Some(path)) => cx.background_executor().spawn(async move { read_picked(&path, max_bytes).map(Some) }).await,
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            let _ = view.update_in(cx, |view, window, cx| done(view, outcome, window, cx));
        })
        .detach();
    }

    fn pick_files(
        &mut self,
        window: &Window,
        prompt: &str,
        done: impl FnOnce(&mut V, std::io::Result<Vec<PathBuf>>, &mut Window, &mut Context<V>) + 'static,
    ) {
        let chosen = ask_paths(self, prompt, true);
        self.spawn_in(window, async move |view, cx| {
            let outcome = chosen.await;
            let _ = view.update_in(cx, |view, window, cx| done(view, outcome, window, cx));
        })
        .detach();
    }
}

/// Read a chosen file, unless it is larger than the caller is ready to take.
fn read_picked(path: &std::path::Path, max_bytes: usize) -> std::io::Result<PickedFile> {
    let size = usize::try_from(std::fs::metadata(path)?.len()).unwrap_or(usize::MAX);
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = if size <= max_bytes { std::fs::read(path)? } else { Vec::new() };
    Ok(PickedFile { name, size, bytes })
}

pub trait ApiExt<V: 'static> {
    /// Run `work` against the service, then `done` on the view with its result.
    ///
    /// Nothing happens if the view is gone when the answer arrives.
    fn api<T, Fut>(
        &mut self,
        work: impl FnOnce(Client) -> Fut + Send + 'static,
        done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
    ) where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static;

    /// Like [`ApiExt::api`], for a `done` that needs the window (to fill a field, show a toast).
    fn api_in<T, Fut>(
        &mut self,
        window: &Window,
        work: impl FnOnce(Client) -> Fut + Send + 'static,
        done: impl FnOnce(&mut V, T, &mut Window, &mut Context<V>) + 'static,
    ) where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static;

    /// Run `work`, which reports as it goes (an answer being written): `each` hears every
    /// report on the view, in order, then `done` the result.
    ///
    /// Dropping the returned task stops listening and gives the work up.
    fn api_stream<E, T, Fut>(
        &mut self,
        work: impl FnOnce(Client, Reporter<E>) -> Fut + Send + 'static,
        each: impl Fn(&mut V, E, &mut Context<V>) + 'static,
        done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
    ) -> Task<()>
    where
        E: Send + 'static,
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static;
}

/// What a piece of work on the service's threads calls to report to the screen.
pub type Reporter<E> = Box<dyn Fn(E) + Send + Sync>;

/// Work on the service's threads that ends when nobody waits for it any more.
struct GivenUpOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for GivenUpOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl<V: 'static> ApiExt<V> for Context<'_, V> {
    fn api_stream<E, T, Fut>(
        &mut self,
        work: impl FnOnce(Client, Reporter<E>) -> Fut + Send + 'static,
        each: impl Fn(&mut V, E, &mut Context<V>) + 'static,
        done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
    ) -> Task<()>
    where
        E: Send + 'static,
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let Some(backend) = self.try_global::<Backend>() else { return Task::ready(()) };
        let (reports, mut heard) = tokio::sync::mpsc::unbounded_channel::<E>();
        let report: Reporter<E> = Box::new(move |event| {
            // Nobody listens any more: the report is dropped, and the work is being given up.
            let _ = reports.send(event);
        });
        let mut answer = GivenUpOnDrop(backend.runtime.spawn(work(backend.client.clone(), report)));
        self.spawn(async move |view, cx| {
            while let Some(event) = heard.recv().await {
                if view.update(cx, |view, cx| each(view, event, cx)).is_err() {
                    return;
                }
            }
            if let Ok(result) = (&mut answer.0).await {
                let _ = view.update(cx, |view, cx| done(view, result, cx));
            }
        })
    }

    fn api<T, Fut>(
        &mut self,
        work: impl FnOnce(Client) -> Fut + Send + 'static,
        done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
    ) where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let Some(backend) = self.try_global::<Backend>() else { return };
        let answer = backend.runtime.spawn(work(backend.client.clone()));
        self.spawn(async move |view, cx| {
            // A task that panicked has no answer to bring back.
            if let Ok(result) = answer.await {
                let _ = view.update(cx, |view, cx| done(view, result, cx));
            }
        })
        .detach();
    }

    fn api_in<T, Fut>(
        &mut self,
        window: &Window,
        work: impl FnOnce(Client) -> Fut + Send + 'static,
        done: impl FnOnce(&mut V, T, &mut Window, &mut Context<V>) + 'static,
    ) where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let Some(backend) = self.try_global::<Backend>() else { return };
        let answer = backend.runtime.spawn(work(backend.client.clone()));
        self.spawn_in(window, async move |view, cx| {
            if let Ok(result) = answer.await {
                let _ = view.update_in(cx, |view, window, cx| done(view, result, window, cx));
            }
        })
        .detach();
    }
}
