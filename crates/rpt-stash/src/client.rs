//! One client, two backends. Callers only see [`StashClient::verify`],
//! [`StashClient::list_tabs`] and [`StashClient::get_tab`].

use parking_lot::RwLock;
use reqwest::{Response, StatusCode};
use rpt_trade::trade::rate_limiter::{RateLimitRules, RateLimiter};
use serde::Deserialize;

use crate::auth::{AuthMethod, Credentials, OAuthTokens};
use crate::model::{ApiStashList, ApiStashOne, LegacyStashResponse, StashTab, StashTabInfo};
use crate::{Result, StashError, oauth};

const WEB: &str = "https://www.pathofexile.com";
const API: &str = "https://api.pathofexile.com";

#[derive(Debug, Clone, PartialEq)]
pub struct AccountInfo {
    pub name: String,
    pub method: AuthMethod,
}

pub struct StashClient {
    http: reqwest::Client,
    creds: RwLock<Option<Credentials>>,
    /// `pc` | `xbox` | `sony`
    realm: RwLock<String>,
    legacy_limits: RateLimitRules,
    api_limits: RateLimitRules,
}

impl Default for StashClient {
    fn default() -> Self {
        Self::new()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl StashClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("RustyPoeTrade/{} (contact: {})", env!("CARGO_PKG_VERSION"), crate::CONTACT))
            .gzip(true)
            .build()
            .expect("reqwest client");
        Self {
            http,
            creds: RwLock::new(None),
            realm: RwLock::new("pc".into()),
            // Conservative until the first response tells us the real rules.
            legacy_limits: RateLimitRules::new("stash-legacy", vec![RateLimiter::new(10, 60.0)]),
            api_limits: RateLimitRules::new("stash-request-limit", vec![RateLimiter::new(10, 60.0)]),
        }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn set_credentials(&self, c: Option<Credentials>) {
        *self.creds.write() = c;
    }

    pub fn credentials(&self) -> Option<Credentials> {
        self.creds.read().clone()
    }

    pub fn method(&self) -> Option<AuthMethod> {
        self.creds.read().as_ref().map(Credentials::method)
    }

    pub fn set_realm(&self, realm: &str) {
        *self.realm.write() = if realm.is_empty() { "pc".into() } else { realm.into() };
    }

    /// Checks the credentials work and returns the account name. For the
    /// session cookie this also fills in the account name if it was empty.
    pub async fn verify(&self) -> Result<AccountInfo> {
        match self.credentials().ok_or(StashError::NotSignedIn)? {
            Credentials::Session { poesessid, account_name } => {
                let typed = account_name.trim().to_owned();
                // `/my-account` validates the cookie (401 when it's invalid or
                // expired). The old `character-window/get-account-name`
                // endpoint is gone (404). The account name is read from the
                // page if possible, else the typed one is kept; it's only
                // needed for the legacy stash API, not for trade actions.
                let found = self.session_check().await?;
                let name = found.unwrap_or(typed);
                *self.creds.write() = Some(Credentials::Session { poesessid, account_name: name.clone() });
                Ok(AccountInfo { name, method: AuthMethod::SessionCookie })
            }
            Credentials::OAuth(_) => {
                let t = self.oauth_tokens().await?;
                let name = match t.username.clone() {
                    Some(n) => n,
                    None => self.oauth_profile_name().await?,
                };
                Ok(AccountInfo { name, method: AuthMethod::OAuth })
            }
        }
    }

    pub async fn list_tabs(&self, league: &str) -> Result<Vec<StashTabInfo>> {
        match self.credentials().ok_or(StashError::NotSignedIn)? {
            Credentials::Session { .. } => Ok(self.legacy_stash(league, 0, true).await?.tabs.into_iter().map(Into::into).collect()),
            Credentials::OAuth(_) => {
                let r: ApiStashList = self.api_get(&self.api_path(&format!("/stash/{}", enc(league)))).await?;
                Ok(r.stashes.into_iter().map(Into::into).collect())
            }
        }
    }

    pub async fn get_tab(&self, league: &str, tab: &StashTabInfo) -> Result<StashTab> {
        match self.credentials().ok_or(StashError::NotSignedIn)? {
            Credentials::Session { .. } => {
                let idx = tab.index.ok_or_else(|| StashError::Forbidden("this tab can't be read with a session cookie".into()))?;
                let r = self.legacy_stash(league, idx, false).await?;
                Ok(StashTab { info: tab.clone(), items: r.items })
            }
            Credentials::OAuth(_) => {
                let path = match &tab.parent {
                    Some(p) => format!("/stash/{}/{}/{}", enc(league), enc(p), enc(&tab.id)),
                    None => format!("/stash/{}/{}", enc(league), enc(&tab.id)),
                };
                let r: ApiStashOne = self.api_get(&self.api_path(&path)).await?;
                Ok(r.stash.into_tab())
            }
        }
    }

    // -- session cookie (legacy character-window) ---------------------------

    fn session(&self) -> Result<(String, String)> {
        match &*self.creds.read() {
            Some(Credentials::Session { poesessid, account_name }) => Ok((poesessid.expose().to_owned(), account_name.clone())),
            _ => Err(StashError::NotSignedIn),
        }
    }

    async fn legacy_get(&self, path_and_query: &str) -> Result<Response> {
        let (sid, _) = self.session()?;
        self.legacy_limits.wait_multi().await;
        let resp = self.http.get(format!("{WEB}{path_and_query}")).header("Cookie", format!("POESESSID={sid}")).send().await?;
        self.legacy_limits.adjust_from_headers(resp.headers(), 1.0);
        check(resp).await
    }

    /// GET `/my-account` with the cookie: `Ok(Some(name))` / `Ok(None)` when
    /// the cookie is valid, `Err(Unauthorized)` when it isn't.
    async fn session_check(&self) -> Result<Option<String>> {
        let html = self.legacy_get("/my-account").await?.text().await?;
        Ok(account_name_from_html(&html))
    }

    async fn legacy_stash(&self, league: &str, tab_index: u32, with_tabs: bool) -> Result<LegacyStashResponse> {
        let (_, account) = self.session()?;
        if account.is_empty() {
            return Err(StashError::AccountNameUnknown);
        }
        let realm = self.realm.read().clone();
        let q = format!(
            "/character-window/get-stash-items?accountName={}&realm={}&league={}&tabs={}&tabIndex={}",
            enc(&account),
            enc(&realm),
            enc(league),
            u8::from(with_tabs),
            tab_index
        );
        Ok(self.legacy_get(&q).await?.json().await?)
    }

    // -- OAuth ---------------------------------------------------------------

    fn api_path(&self, path: &str) -> String {
        let realm = self.realm.read().clone();
        // PC realm is implicit; console realms are a path segment after the resource.
        match (realm.as_str(), path.strip_prefix("/stash/")) {
            ("pc", _) | (_, None) => format!("{API}{path}"),
            (r, Some(rest)) => format!("{API}/stash/{r}/{rest}"),
        }
    }

    /// Current tokens, refreshed (and re-saved to the keychain) if expired.
    async fn oauth_tokens(&self) -> Result<OAuthTokens> {
        let t = match &*self.creds.read() {
            Some(Credentials::OAuth(t)) => t.clone(),
            _ => return Err(StashError::NotSignedIn),
        };
        if !t.expired(now()) {
            return Ok(t);
        }
        let fresh = oauth::refresh(&self.http, &t).await?;
        *self.creds.write() = Some(Credentials::OAuth(fresh.clone()));
        let save = fresh.clone();
        let _ = tokio::task::spawn_blocking(move || crate::secrets::save_oauth(&save)).await;
        Ok(fresh)
    }

    async fn api_get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let t = self.oauth_tokens().await?;
        self.api_limits.wait_multi().await;
        let resp = self
            .http
            .get(url)
            .header("User-Agent", oauth::user_agent(&t.client_id))
            .bearer_auth(t.access_token.expose())
            .send()
            .await?;
        self.api_limits.adjust_from_headers(resp.headers(), 1.0);
        Ok(check(resp).await?.json().await?)
    }

    async fn oauth_profile_name(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct P {
            name: String,
        }
        let p: P = self.api_get(&format!("{API}/profile")).await?;
        Ok(p.name)
    }

    /// Run the interactive OAuth login and switch this client to it.
    pub async fn oauth_login(&self, cfg: &oauth::OAuthConfig, open_browser: impl FnOnce(&str)) -> Result<OAuthTokens> {
        let t = oauth::login(&self.http, cfg, open_browser).await?;
        *self.creds.write() = Some(Credentials::OAuth(t.clone()));
        Ok(t)
    }
}

/// Find the account name in a pathofexile.com page via its profile link
/// (`/account/view-profile/Name-1234` → `Name#1234`).
fn account_name_from_html(html: &str) -> Option<String> {
    const MARK: &str = "/account/view-profile/";
    let start = html.find(MARK)? + MARK.len();
    let raw: String = html[start..].chars().take_while(|c| !matches!(c, '"' | '\'' | '?' | '/' | '<' | '>' | '#') && !c.is_whitespace()).collect();
    let decoded = percent_decode(&raw);
    if decoded.is_empty() {
        return None;
    }
    // New-style names carry a 4-digit discriminator, written `-1234` in URLs.
    Some(match decoded.rsplit_once('-') {
        Some((n, d)) if d.len() == 4 && d.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => format!("{n}#{d}"),
        _ => decoded,
    })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn check(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let retry = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    let body = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| body.chars().take(200).collect());
    Err(match status {
        StatusCode::UNAUTHORIZED => StashError::Unauthorized,
        StatusCode::FORBIDDEN => StashError::Forbidden(msg),
        StatusCode::TOO_MANY_REQUESTS => StashError::RateLimited { retry_after_secs: retry.unwrap_or(60) },
        _ => StashError::Status { status: status.as_u16(), body: msg },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_name_from_profile_link() {
        let html = r#"<a href="/account/view-profile/Some%20Name-1234/characters">x</a>"#;
        assert_eq!(account_name_from_html(html).as_deref(), Some("Some Name#1234"));
        assert_eq!(account_name_from_html(r#"<a href="/account/view-profile/oldname">"#).as_deref(), Some("oldname"));
        assert_eq!(account_name_from_html("<html>nothing</html>"), None);
    }

    #[test]
    fn encodes_discriminator() {
        assert_eq!(enc("Some Name#1234"), "Some%20Name%231234");
    }

    #[test]
    fn console_realm_path() {
        let c = StashClient::new();
        assert_eq!(c.api_path("/stash/Standard"), "https://api.pathofexile.com/stash/Standard");
        c.set_realm("xbox");
        assert_eq!(c.api_path("/stash/Standard/abc"), "https://api.pathofexile.com/stash/xbox/Standard/abc");
        assert_eq!(c.api_path("/profile"), "https://api.pathofexile.com/profile");
    }
}
