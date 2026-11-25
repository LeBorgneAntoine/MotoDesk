//! The client: the local API, the cloud, and the session that ties them
//! (port of the token half of `desktop/src/lib/api/client.ts`).

use std::sync::{Arc, Mutex};

use axum::http::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::cloud::Cloud;
use crate::error::{ApiError, ApiResult};
use crate::token_store::TokenStore;
use crate::transport::Local;
use crate::types::TokenBundle;

/// What the session tells the interface without being asked.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// The cloud renewed the session: here is the new one.
    Refreshed(Box<TokenBundle>),
    /// The cloud rejected the session: the user has to sign in again.
    Unauthorized,
}

type Listener = Box<dyn Fn(SessionEvent) + Send + Sync>;

struct Inner {
    local: Local,
    cloud: Cloud,
    store: TokenStore,
    tokens: Mutex<Option<TokenBundle>>,
    /// The refresh token rotates: two overlapping renewals would present the same one and the
    /// second would be refused. One at a time.
    refreshing: tokio::sync::Mutex<()>,
    listener: Mutex<Option<Listener>>,
}

/// Cheap to clone; every screen holds one.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

/// Read a JSON answer; an empty one reads as `null` (so `()` and `Option<T>` both work).
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> ApiResult<T> {
    let source: &[u8] = if bytes.is_empty() { b"null" } else { bytes };
    serde_json::from_slice(source)
        .map_err(|e| ApiError::new(0, "bad_response", format!("Réponse inattendue du service de données : {e}")))
}

/// One request being prepared. Built by [`Client::call`].
pub struct Call<'a> {
    client: &'a Client,
    method: Method,
    path: String,
    body: Option<Vec<u8>>,
    auth: bool,
}

impl Call<'_> {
    /// A call that needs no session (sign-in, status): a 401 is its own answer, not a reason
    /// to renew the session.
    pub fn public(mut self) -> Self {
        self.auth = false;
        self
    }

    pub fn json<B: Serialize + ?Sized>(mut self, body: &B) -> Self {
        self.body = serde_json::to_vec(body).ok();
        self
    }

    /// Send, and read the answer as `T`.
    pub async fn send<T: DeserializeOwned>(self) -> ApiResult<T> {
        decode(&self.bytes().await?)
    }

    /// Send, and ignore what comes back.
    pub async fn done(self) -> ApiResult<()> {
        self.bytes().await.map(|_| ())
    }

    async fn bytes(self) -> ApiResult<Vec<u8>> {
        let Call { client, method, path, body, auth } = self;
        let local = &client.inner.local;
        match local.send(method.clone(), &path, body.clone()).await {
            Err(e) if e.is_unauthorized() && auth && client.has_refresh_token() => {
                if client.refresh_session().await.is_some() {
                    return local.send(method, &path, body).await;
                }
                // Only a rejected session signs out; an unreachable cloud leaves it alone.
                if client.tokens().is_none() {
                    client.emit(SessionEvent::Unauthorized);
                }
                Err(e)
            }
            other => other,
        }
    }
}

impl Client {
    pub fn new(local: Local, cloud: Cloud, store: TokenStore) -> Self {
        Self {
            inner: Arc::new(Inner {
                local,
                cloud,
                store,
                tokens: Mutex::new(None),
                refreshing: tokio::sync::Mutex::new(()),
                listener: Mutex::new(None),
            }),
        }
    }

    pub fn cloud(&self) -> &Cloud {
        &self.inner.cloud
    }

    /// Who hears about renewals and rejections (the interface's auth store).
    pub fn on_session_event(&self, listener: impl Fn(SessionEvent) + Send + Sync + 'static) {
        *self.inner.listener.lock().unwrap() = Some(Box::new(listener));
    }

    fn emit(&self, event: SessionEvent) {
        if let Some(listener) = self.inner.listener.lock().unwrap().as_ref() {
            listener(event);
        }
    }

    // ---- requests ---------------------------------------------------------------------------

    pub fn call(&self, method: Method, path: impl Into<String>) -> Call<'_> {
        Call { client: self, method, path: path.into(), body: None, auth: true }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: impl Into<String>) -> ApiResult<T> {
        self.call(Method::GET, path).send().await
    }

    pub async fn post<T: DeserializeOwned, B: Serialize + ?Sized>(&self, path: impl Into<String>, body: &B) -> ApiResult<T> {
        self.call(Method::POST, path).json(body).send().await
    }

    pub async fn put<T: DeserializeOwned, B: Serialize + ?Sized>(&self, path: impl Into<String>, body: &B) -> ApiResult<T> {
        self.call(Method::PUT, path).json(body).send().await
    }

    pub async fn patch<T: DeserializeOwned, B: Serialize + ?Sized>(&self, path: impl Into<String>, body: &B) -> ApiResult<T> {
        self.call(Method::PATCH, path).json(body).send().await
    }

    pub async fn delete(&self, path: impl Into<String>) -> ApiResult<()> {
        self.call(Method::DELETE, path).done().await
    }

    // ---- session ----------------------------------------------------------------------------

    /// The session in memory.
    pub fn tokens(&self) -> Option<TokenBundle> {
        self.inner.tokens.lock().unwrap().clone()
    }

    fn has_refresh_token(&self) -> bool {
        self.inner.tokens.lock().unwrap().as_ref().is_some_and(|t| !t.refresh_token.is_empty())
    }

    /// Read the session kept from the previous run.
    pub fn hydrate(&self) -> Option<TokenBundle> {
        let stored = self.inner.store.load();
        *self.inner.tokens.lock().unwrap() = stored.clone();
        stored
    }

    /// Replace the session, in memory and on disk. `None` forgets it.
    pub fn set_tokens(&self, tokens: Option<TokenBundle>) {
        match &tokens {
            Some(bundle) => self.inner.store.save(bundle),
            None => self.inner.store.clear(),
        }
        *self.inner.tokens.lock().unwrap() = tokens;
    }

    /// Renew the cloud session with the refresh token.
    ///
    /// The session is only dropped when the cloud *rejects* the refresh token. Being offline, or
    /// the cloud being down, says nothing about the token, and wiping it would sign people out
    /// of an app that is supposed to work without a network.
    pub async fn refresh_session(&self) -> Option<TokenBundle> {
        let presented = self.tokens().map(|t| t.refresh_token).filter(|t| !t.is_empty())?;
        let _one_at_a_time = self.inner.refreshing.lock().await;
        // Someone else renewed while this call waited: use theirs.
        match self.tokens() {
            Some(current) if current.refresh_token != presented => return Some(current),
            None => return None,
            _ => {}
        }
        match self.inner.cloud.refresh(&presented).await {
            Ok(next) => {
                self.set_tokens(Some(next.clone()));
                self.emit(SessionEvent::Refreshed(Box::new(next.clone())));
                Some(next)
            }
            Err(e) => {
                if e.status == 401 || e.status == 403 {
                    self.set_tokens(None);
                }
                None
            }
        }
    }
}
