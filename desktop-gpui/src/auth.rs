//! Who is signed in.
//!
//! [`AuthStore`] is what the screens observe. The steps that talk to the service or the cloud
//! are the free functions of [`flows`]: they run on the service's threads and return what the
//! store then adopts.

use std::time::Duration;

use motodesk_client::local_account::{self, AccessMode, LocalState, LocalStatus};
use motodesk_client::sync::BootstrapResult;
use motodesk_client::{PublicUser, SessionEvent};
use motodesk_ui::gpui::{App, AppContext as _, Context, Entity, Global};

use crate::backend::{self, ApiExt as _};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    /// The stored session is being read: show neither the sign-in page nor the app.
    Initializing,
    Anonymous,
    Authenticated,
}

pub struct AuthStore {
    pub user: Option<PublicUser>,
    pub status: AuthStatus,
    /// A new recovery key waiting to be written down. Kept in memory only, never stored.
    pub recovery_key: Option<String>,
    /// What the last online sign-in imported from the cloud.
    pub last_bootstrap: Option<BootstrapResult>,
    /// Which side of the sign-in switch is on.
    pub access_mode: AccessMode,
    /// What this PC says about itself, once the service has answered.
    pub local_status: Option<LocalStatus>,
    access_chosen: bool,
}

struct GlobalAuth(Entity<AuthStore>);

impl Global for GlobalAuth {}

/// The access token lasts 15 minutes and only the interface can renew it (the refresh token
/// rotates, so the service must not do it too).
const KEEP_ALIVE: Duration = Duration::from_secs(10 * 60);
/// A local account can be disabled or changed from another PC while its session is open here.
const SESSION_CHECK: Duration = Duration::from_secs(15);

impl AuthStore {
    pub fn init(cx: &mut App) {
        let store = cx.new(|_| AuthStore {
            user: None,
            status: AuthStatus::Initializing,
            recovery_key: None,
            last_bootstrap: None,
            access_mode: AccessMode::Online,
            local_status: None,
            access_chosen: false,
        });
        cx.set_global(GlobalAuth(store));
    }

    pub fn global(cx: &App) -> Entity<AuthStore> {
        cx.global::<GlobalAuth>().0.clone()
    }

    pub fn is_authenticated(&self) -> bool {
        self.status == AuthStatus::Authenticated
    }

    /// A standalone installation: signed in with a local account, no cloud behind it.
    pub fn is_local(&self) -> bool {
        self.user.as_ref().is_some_and(PublicUser::is_local)
    }

    pub fn is_admin(&self) -> bool {
        self.user.as_ref().is_some_and(PublicUser::is_admin)
    }

    pub fn local_state(&self) -> LocalState {
        local_account::local_state(self.local_status)
    }

    pub fn local_offered(&self) -> bool {
        local_account::local_offered(self.local_status)
    }

    /// The user flips the sign-in switch.
    pub fn choose_access(&mut self, mode: AccessMode, cx: &mut Context<Self>) {
        self.access_chosen = true;
        self.access_mode = mode;
        cx.notify();
    }

    /// Ask the service what this PC is, and settle the switch accordingly.
    pub fn refresh_access(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { local_account::status(&client).await },
            |this, status, cx| {
                // No answer: the service is still starting. Leave the switch as it is.
                let Ok(status) = status else { return };
                this.local_status = Some(status);
                if !this.access_chosen {
                    this.access_mode = local_account::initial_access_mode(this.local_status);
                }
                if !local_account::local_offered(this.local_status) {
                    this.access_mode = AccessMode::Online;
                }
                cx.notify();
            },
        );
    }

    /// The service is up: read the stored session and start the background checks.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        let Some(client) = backend::client(cx) else { return };

        // Renewals and rejections happen on the service's threads: bring them here.
        let (events, mut inbox) = tokio::sync::mpsc::unbounded_channel();
        client.on_session_event(move |event| {
            let _ = events.send(event);
        });
        cx.spawn(async move |this, cx| {
            while let Some(event) = inbox.recv().await {
                let alive = this.update(cx, |this, cx| match event {
                    SessionEvent::Refreshed(bundle) => {
                        let bundle = *bundle;
                        this.user = Some(bundle.user.clone());
                        cx.api(|client| async move { motodesk_client::auth::sync_session_to_local(&client, &bundle).await }, |_, _, _| {});
                        cx.notify();
                    }
                    SessionEvent::Unauthorized => this.signed_out(cx),
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let mut since_keep_alive = Duration::ZERO;
            loop {
                cx.background_executor().timer(SESSION_CHECK).await;
                since_keep_alive += SESSION_CHECK;
                let keep_alive = since_keep_alive >= KEEP_ALIVE;
                if keep_alive {
                    since_keep_alive = Duration::ZERO;
                }
                let alive = this.update(cx, |this, cx| {
                    if !this.is_authenticated() {
                        return;
                    }
                    if this.is_local() {
                        this.check_local_session(cx);
                    } else if keep_alive {
                        cx.api(|client| async move { client.refresh_session().await }, |_, _, _| {});
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.api(flows::restore, |this, restored, cx| {
            match restored {
                Some(user) => this.signed_in(user, cx),
                None => this.signed_out(cx),
            }
            this.refresh_access(cx);
        });
    }

    fn check_local_session(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move {
                let answer = motodesk_client::auth::local_me(&client).await;
                // Only a refusal ends the session: a service that does not answer says nothing.
                if matches!(&answer, Err(e) if e.is_unauthorized()) {
                    client.set_tokens(None);
                }
                answer
            },
            |this, answer, cx| match answer {
                Ok(me) => {
                    if this.user.as_ref().is_some_and(|u| u.role != me.role || u.full_name != me.full_name) {
                        this.user = Some(me);
                        cx.notify();
                    }
                }
                Err(e) if e.is_unauthorized() => this.signed_out(cx),
                Err(_) => {}
            },
        );
    }

    fn signed_in(&mut self, user: PublicUser, cx: &mut Context<Self>) {
        self.user = Some(user);
        self.status = AuthStatus::Authenticated;
        cx.notify();
    }

    fn signed_out(&mut self, cx: &mut Context<Self>) {
        self.user = None;
        self.recovery_key = None;
        self.status = AuthStatus::Anonymous;
        cx.notify();
    }

    /// A sign-in screen got a session: the app opens.
    pub fn adopt(&mut self, session: flows::SignedIn, cx: &mut Context<Self>) {
        self.recovery_key = session.recovery_key;
        self.last_bootstrap = session.bootstrap;
        self.signed_in(session.bundle.user, cx);
    }

    /// A new recovery key was issued: show it, once, the way the first one was.
    pub fn show_recovery_key(&mut self, key: String, cx: &mut Context<Self>) {
        self.recovery_key = Some(key);
        cx.notify();
    }

    /// The recovery key has been written down.
    pub fn recovery_key_noted(&mut self, cx: &mut Context<Self>) {
        self.recovery_key = None;
        cx.notify();
    }

    pub fn logout(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { motodesk_client::auth::logout(&client).await },
            |this, _, cx| {
                this.signed_out(cx);
                this.refresh_access(cx);
            },
        );
    }
}

/// The steps of signing in, run on the service's threads.
pub mod flows {
    use motodesk_client::auth::{self, RegisterPayload};
    use motodesk_client::local_account::{self, PromotePayload, SetupPayload};
    use motodesk_client::recovery::{self, RecoverPayload};
    use motodesk_client::sync::BootstrapResult;
    use motodesk_client::{ApiResult, Client, PublicUser, TokenBundle};


    /// A session a screen just obtained.
    pub struct SignedIn {
        pub bundle: TokenBundle,
        pub recovery_key: Option<String>,
        pub bootstrap: Option<BootstrapResult>,
    }

    impl SignedIn {
        fn plain(bundle: TokenBundle) -> Self {
            Self { bundle, recovery_key: None, bootstrap: None }
        }
    }

    /// Find the session of the previous run, if it is still good.
    pub async fn restore(client: Client) -> Option<PublicUser> {
        let cached = client.hydrate()?;
        if cached.user.is_local() {
            // A local session is the service's own record: there is no cloud to ask.
            return match auth::local_me(&client).await {
                Ok(me) if me.is_local() => Some(me),
                _ => {
                    client.set_tokens(None);
                    None
                }
            };
        }
        if cached.access_token.is_empty() {
            return None;
        }
        validate(&client, cached).await
    }

    /// Confirm a stored cloud session. An expired access token is renewed, not signed out, as
    /// long as the refresh token is still accepted; being offline keeps the session as it is.
    async fn validate(client: &Client, cached: TokenBundle) -> Option<PublicUser> {
        match client.cloud().me(&cached.access_token).await {
            Ok(fresh) => {
                let mut bundle = TokenBundle { user: fresh.clone(), ..cached.clone() };
                // Promoted to admin since the token was issued: a new token carries the role.
                if cached.user.role != fresh.role
                    && fresh.role == "admin"
                    && !cached.refresh_token.is_empty()
                    && let Ok(renewed) = client.cloud().refresh(&cached.refresh_token).await
                {
                    bundle = renewed;
                }
                client.set_tokens(Some(bundle.clone()));
                auth::sync_session_to_local(client, &bundle).await;
                Some(bundle.user)
            }
            Err(e) if e.is_unauthorized() => match client.refresh_session().await {
                Some(bundle) => {
                    auth::sync_session_to_local(client, &bundle).await;
                    Some(bundle.user)
                }
                None if client.tokens().is_none() => None,
                // The cloud could not be reached to renew: stay signed in.
                None => Some(cached.user),
            },
            // Offline: the cached session stands.
            Err(_) => Some(cached.user),
        }
    }

    pub async fn sign_in_local(client: Client, username: String, password: String) -> ApiResult<SignedIn> {
        let bundle = local_account::login(&client, &username, &password).await?;
        client.set_tokens(Some(bundle.clone()));
        Ok(SignedIn::plain(bundle))
    }

    /// First run of a standalone installation: create the first administrator here.
    pub async fn set_up_local(client: Client, payload: SetupPayload) -> ApiResult<SignedIn> {
        let reply = local_account::setup(&client, &payload).await?;
        // The key is shown, not saved with the session.
        client.set_tokens(Some(reply.bundle.clone()));
        Ok(SignedIn { bundle: reply.bundle, recovery_key: reply.recovery_key, bootstrap: None })
    }

    /// « Mot de passe perdu »: the recovery key gives one account a new password and the
    /// administrator's role. The key that replaces the one just used is shown, not saved.
    pub async fn recover_local(client: Client, payload: RecoverPayload) -> ApiResult<SignedIn> {
        let reply = recovery::recover_with_key(&client, &payload).await?;
        client.set_tokens(Some(reply.bundle.clone()));
        Ok(SignedIn { bundle: reply.bundle, recovery_key: reply.recovery_key, bootstrap: None })
    }

    /// Put a local network online: the cloud creates the company, and from here on the
    /// session is the cloud's.
    pub async fn go_online(client: Client, payload: PromotePayload) -> ApiResult<SignedIn> {
        let bundle = local_account::promote_to_online(&client, &payload).await?;
        client.set_tokens(Some(bundle.clone()));
        Ok(SignedIn::plain(bundle))
    }

    pub async fn sign_in_online(client: Client, identifier: String, password: String) -> ApiResult<SignedIn> {
        let bundle = auth::login(&client, &identifier, &password).await?;
        let bootstrap = auth::bootstrap_from_cloud(&client).await;
        Ok(SignedIn { bundle, recovery_key: None, bootstrap })
    }

    pub async fn register_online(client: Client, payload: RegisterPayload) -> ApiResult<SignedIn> {
        let bundle = auth::register(&client, &payload).await?;
        let bootstrap = auth::bootstrap_from_cloud(&client).await;
        Ok(SignedIn { bundle, recovery_key: None, bootstrap })
    }
}
