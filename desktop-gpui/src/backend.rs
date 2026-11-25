//! The bridge between the interface and the data service.
//!
//! The service (Axum, SQLite, sync) runs on Tokio threads inside this process; the interface
//! runs on GPUI's main thread. A screen never blocks on the service: it hands a piece of work
//! to [`ApiExt::api`], which runs it on the service's threads and brings the answer back to the
//! view, on the main thread.

use std::future::Future;

use motodesk_client::Client;
use motodesk_ui::gpui::{App, Context, Global, Window};

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
