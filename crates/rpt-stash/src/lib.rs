//! Account + stash access for Rusty POE Trade.
//!
//! Two interchangeable sign-in methods sit behind one [`StashClient`]:
//!
//! - [`AuthMethod::SessionCookie`]: the `POESESSID` cookie from a logged-in
//!   pathofexile.com browser session, used with the legacy
//!   `character-window` endpoints. Works today.
//! - [`AuthMethod::OAuth`]: GGG's official OAuth 2.1 (public client + PKCE,
//!   loopback redirect) and `api.pathofexile.com/stash`. Needs a client ID
//!   issued by GGG; everything else is implemented.
//!
//! Both return the same normalized [`StashTabInfo`] / [`StashTab`] / [`Item`]
//! types, so the UI and valuation code never care which one is active.
//! Switching is a config change plus signing in again.
//!
//! Secrets (the cookie, OAuth tokens) are stored only in the OS keychain
//! ([`secrets`]) and are only ever sent to pathofexile.com.

pub mod auth;
pub mod client;
pub mod error;
pub mod model;
pub mod oauth;
pub mod secrets;

pub use auth::{AuthMethod, Credentials, OAuthTokens, Secret};
pub use client::{AccountInfo, StashClient};
pub use error::StashError;
pub use model::{Item, StashTab, StashTabInfo};

pub type Result<T, E = StashError> = std::result::Result<T, E>;

/// Required by GGG's developer terms for anything using their API.
pub const GGG_DISCLAIMER: &str = "This product isn't affiliated with or endorsed by Grinding Gear Games in any way.";

/// Generic contact for the User-Agent GGG asks API clients to send.
/// Deliberately the project page, never a personal address.
pub const CONTACT: &str = "https://github.com/OWNER/rusty-poe-trade";
