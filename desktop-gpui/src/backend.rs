//! The bridge between the interface and the data service.
//!
//! The service (Axum, SQLite, sync) runs on Tokio threads inside this process; the interface
//! runs on GPUI's main thread. A screen never blocks on the service: it hands a piece of work
//! to [`ApiExt::api`], which runs it on the service's threads and brings the answer back to the
//! view, on the main thread.

use std::future::Future;
use std::path::PathBuf;

use motodesk_client::Client;
use motodesk_ui::gpui::{App, Context, Global, PathPromptOptions, Window};

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
}

impl<V: 'static> SaveExt<V> for Context<'_, V> {
    fn save_file(
        &mut self,
        window: &Window,
        suggested_name: &str,
        bytes: Vec<u8>,
        done: impl FnOnce(&mut V, std::io::Result<Option<PathBuf>>, &mut Window, &mut Context<V>) + 'static,
    ) {
        let chosen = self.prompt_for_new_path(&documents_dir(), Some(suggested_name));
        self.spawn_in(window, async move |view, cx| {
            let outcome = match chosen.await {
                Ok(Ok(Some(path))) => {
                    // Off the interface's thread: a year of entries is a file of some size.
                    cx.background_executor().spawn(async move { std::fs::write(&path, bytes).map(|()| Some(path)) }).await
                }
                // The dialog was closed, or went away with the window.
                Ok(Ok(None)) | Err(_) => Ok(None),
                Ok(Err(error)) => Err(std::io::Error::other(error.to_string())),
            };
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
}

impl<V: 'static> PickExt<V> for Context<'_, V> {
    fn pick_file(
        &mut self,
        window: &Window,
        prompt: &str,
        max_bytes: usize,
        done: impl FnOnce(&mut V, std::io::Result<Option<PickedFile>>, &mut Window, &mut Context<V>) + 'static,
    ) {
        let options = PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(prompt.to_string().into()) };
        let chosen = self.prompt_for_paths(options);
        self.spawn_in(window, async move |view, cx| {
            let outcome = match chosen.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => cx.background_executor().spawn(async move { read_picked(&path, max_bytes).map(Some) }).await,
                    None => Ok(None),
                },
                // The dialog was closed, or went away with the window.
                Ok(Ok(None)) | Err(_) => Ok(None),
                Ok(Err(error)) => Err(std::io::Error::other(error.to_string())),
            };
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
}

impl<V: 'static> ApiExt<V> for Context<'_, V> {
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
