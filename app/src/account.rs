//! App-side account service: owns the [`StashClient`], keeps the signed-in
//! state for the UI, and talks to the keychain off the UI thread.
//!
//! The rest of the app (stash valuation, bulk sell, ...) uses [`get`] and
//! [`Account::client`] and never needs to know which sign-in method is active.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use rpt_stash::oauth::OAuthConfig;
use rpt_stash::{AccountInfo, AuthMethod, Credentials, Secret, StashClient, StashError, StashTabInfo, secrets};

use crate::config::{AccountSettings, Config};

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Status {
    #[default]
    SignedOut,
    Working(String),
    SignedIn(AccountInfo),
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct AccountState {
    pub status: Status,
    /// `Some(false)` if the keychain was unavailable and the secret is only kept in memory.
    pub saved_to_keychain: Option<bool>,
    pub keychain_error: Option<String>,
    /// League + tabs from the last "load tabs".
    pub tabs: Option<(String, Vec<StashTabInfo>)>,
    pub tabs_error: Option<String>,
    pub loading_tabs: bool,
}

pub struct Account {
    rt: tokio::runtime::Runtime,
    client: Arc<StashClient>,
    state: Arc<Mutex<AccountState>>,
    ctx: egui::Context,
    restored: AtomicBool,
}

static ACCOUNT: OnceLock<Account> = OnceLock::new();

pub fn get(ctx: &egui::Context) -> &'static Account {
    ACCOUNT.get_or_init(|| Account::new(ctx.clone()))
}

impl Account {
    fn new(ctx: egui::Context) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("rpt-account")
            .enable_all()
            .build()
            .expect("account runtime");
        Self { rt, client: Arc::new(StashClient::new()), state: Default::default(), ctx, restored: AtomicBool::new(false) }
    }

    #[allow(dead_code)] // used by the stash valuation / bulk-sell features
    pub fn client(&self) -> Arc<StashClient> {
        self.client.clone()
    }

    pub fn state(&self) -> AccountState {
        self.state.lock().clone()
    }

    #[allow(dead_code)]
    pub fn signed_in(&self) -> Option<AccountInfo> {
        match &self.state.lock().status {
            Status::SignedIn(i) => Some(i.clone()),
            _ => None,
        }
    }

    fn set_status(state: &Mutex<AccountState>, ctx: &egui::Context, s: Status) {
        state.lock().status = s;
        ctx.request_repaint();
    }

    /// Load saved credentials for the configured method from the keychain and
    /// verify them. Runs once per process; cheap to call every frame.
    pub fn ensure_restored(&self, cfg: &Config) {
        if self.restored.swap(true, Ordering::SeqCst) {
            return;
        }
        // Both APT realms (pc-ggg, pc-garena) are PC; console stashes aren't supported by the overlay.
        self.client.set_realm("pc");
        let (client, state, ctx) = (self.client.clone(), self.state.clone(), self.ctx.clone());
        let method = cfg.rpt.account.method;
        let account_name = cfg.account_name.clone();
        let remember = cfg.rpt.account.remember;
        self.rt.spawn(async move {
            let loaded = tokio::task::spawn_blocking(move || -> rpt_stash::Result<Option<Credentials>> {
                Ok(match method {
                    AuthMethod::SessionCookie if remember => {
                        secrets::load_session()?.map(|poesessid| Credentials::Session { poesessid, account_name })
                    }
                    AuthMethod::SessionCookie => None,
                    AuthMethod::OAuth => secrets::load_oauth()?.map(Credentials::OAuth),
                })
            })
            .await
            .unwrap_or(Ok(None));
            match loaded {
                Ok(Some(creds)) => {
                    client.set_credentials(Some(creds));
                    Self::set_status(&state, &ctx, Status::Working("Checking saved sign-in…".into()));
                    Self::finish_verify(&client, &state, &ctx).await;
                }
                Ok(None) => {}
                Err(e) => {
                    state.lock().keychain_error = Some(e.to_string());
                    ctx.request_repaint();
                }
            }
        });
    }

    async fn finish_verify(client: &StashClient, state: &Mutex<AccountState>, ctx: &egui::Context) -> bool {
        match client.verify().await {
            Ok(info) => {
                Self::set_status(state, ctx, Status::SignedIn(info));
                true
            }
            Err(e) => {
                if matches!(e, StashError::Unauthorized | StashError::Forbidden(_)) {
                    client.set_credentials(None);
                }
                Self::set_status(state, ctx, Status::Failed(e.to_string()));
                false
            }
        }
    }

    /// Sign in with a `POESESSID` (already normalized by the caller).
    pub fn sign_in_session(&self, poesessid: String, settings: &AccountSettings, account_name: &str) {
        self.restored.store(true, Ordering::SeqCst);
        let (client, state, ctx) = (self.client.clone(), self.state.clone(), self.ctx.clone());
        let remember = settings.remember;
        let secret = Secret::new(poesessid);
        client.set_credentials(Some(Credentials::Session { poesessid: secret.clone(), account_name: account_name.to_owned() }));
        Self::set_status(&state, &ctx, Status::Working("Signing in…".into()));
        self.rt.spawn(async move {
            if !Self::finish_verify(&client, &state, &ctx).await {
                return;
            }
            let res = tokio::task::spawn_blocking(move || {
                // OAuth tokens from an earlier method are now stale.
                let _ = secrets::delete_oauth();
                if remember { secrets::save_session(&secret) } else { secrets::delete_session() }
            })
            .await
            .unwrap_or_else(|e| Err(StashError::Keychain(e.to_string())));
            let mut s = state.lock();
            match res {
                Ok(()) => (s.saved_to_keychain, s.keychain_error) = (Some(remember), None),
                Err(e) => (s.saved_to_keychain, s.keychain_error) = (Some(false), Some(e.to_string())),
            }
            ctx.request_repaint();
        });
    }

    /// Official OAuth login: opens the browser and waits for the loopback redirect.
    pub fn sign_in_oauth(&self, settings: &AccountSettings) {
        self.restored.store(true, Ordering::SeqCst);
        let (client, state, ctx) = (self.client.clone(), self.state.clone(), self.ctx.clone());
        let cfg = OAuthConfig { client_id: settings.oauth_client_id.trim().to_owned(), port: settings.oauth_port };
        Self::set_status(&state, &ctx, Status::Working("Waiting for pathofexile.com in your browser…".into()));
        self.rt.spawn(async move {
            let open_ctx = ctx.clone();
            let login = client.oauth_login(&cfg, move |url| open_ctx.open_url(egui::OpenUrl::new_tab(url))).await;
            let tokens = match login {
                Ok(t) => t,
                Err(e) => return Self::set_status(&state, &ctx, Status::Failed(e.to_string())),
            };
            if !Self::finish_verify(&client, &state, &ctx).await {
                return;
            }
            let res = tokio::task::spawn_blocking(move || {
                let _ = secrets::delete_session();
                secrets::save_oauth(&tokens)
            })
            .await
            .unwrap_or_else(|e| Err(StashError::Keychain(e.to_string())));
            let mut s = state.lock();
            match res {
                Ok(()) => (s.saved_to_keychain, s.keychain_error) = (Some(true), None),
                Err(e) => (s.saved_to_keychain, s.keychain_error) = (Some(false), Some(e.to_string())),
            }
            ctx.request_repaint();
        });
    }

    /// Forget everything: in-memory credentials and both keychain entries.
    pub fn sign_out(&self) {
        self.client.set_credentials(None);
        *self.state.lock() = AccountState::default();
        let state = self.state.clone();
        let ctx = self.ctx.clone();
        self.rt.spawn_blocking(move || {
            let r = secrets::delete_session().and_then(|_| secrets::delete_oauth());
            if let Err(e) = r {
                state.lock().keychain_error = Some(e.to_string());
            }
            ctx.request_repaint();
        });
    }

    pub fn load_tabs(&self, league: String) {
        let (client, state, ctx) = (self.client.clone(), self.state.clone(), self.ctx.clone());
        {
            let mut s = state.lock();
            s.loading_tabs = true;
            s.tabs_error = None;
        }
        self.rt.spawn(async move {
            let r = client.list_tabs(&league).await;
            let mut s = state.lock();
            s.loading_tabs = false;
            match r {
                Ok(tabs) => s.tabs = Some((league, tabs)),
                Err(e) => {
                    if matches!(e, StashError::Unauthorized) {
                        s.status = Status::Failed(e.to_string());
                    }
                    s.tabs_error = Some(e.to_string());
                }
            }
            ctx.request_repaint();
        });
    }

    /// Run arbitrary stash work on the account runtime (for later features).
    #[allow(dead_code)]
    pub fn spawn<F>(&self, f: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.rt.spawn(f);
    }
}
