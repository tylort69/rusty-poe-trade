//! OS keychain storage: Secret Service (gnome-keyring / KWallet) on Linux,
//! Credential Manager on Windows. Blocking calls: run off the UI thread.

use crate::auth::{OAuthTokens, Secret};
use crate::{Result, StashError};

const SERVICE: &str = "rusty-poe-trade";
const SESSION_KEY: &str = "poesessid";
const OAUTH_KEY: &str = "oauth-tokens";

fn entry(user: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, user).map_err(|e| StashError::Keychain(e.to_string()))
}

fn get(user: &str) -> Result<Option<String>> {
    match entry(user)?.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(StashError::Keychain(e.to_string())),
    }
}

fn set(user: &str, value: &str) -> Result<()> {
    entry(user)?.set_password(value).map_err(|e| StashError::Keychain(e.to_string()))
}

fn delete(user: &str) -> Result<()> {
    match entry(user)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(StashError::Keychain(e.to_string())),
    }
}

pub fn load_session() -> Result<Option<Secret>> {
    Ok(get(SESSION_KEY)?.map(Secret::new))
}

pub fn save_session(poesessid: &Secret) -> Result<()> {
    set(SESSION_KEY, poesessid.expose())
}

pub fn delete_session() -> Result<()> {
    delete(SESSION_KEY)
}

pub fn load_oauth() -> Result<Option<OAuthTokens>> {
    match get(OAUTH_KEY)? {
        Some(s) => Ok(serde_json::from_str(&s).ok()),
        None => Ok(None),
    }
}

pub fn save_oauth(tokens: &OAuthTokens) -> Result<()> {
    set(OAUTH_KEY, &serde_json::to_string(tokens)?)
}

pub fn delete_oauth() -> Result<()> {
    delete(OAUTH_KEY)
}
