//! GGG OAuth 2.1 for a public (desktop) client: authorization code + PKCE
//! (S256) with a loopback redirect to `http://127.0.0.1:<port>/callback`.
//!
//! Ready to use as soon as GGG issues a client ID: enter it in Settings and
//! the login button lights up. No server of ours is involved: the browser
//! talks to pathofexile.com and redirects back to this machine only.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::auth::{OAuthTokens, Secret};
use crate::{Result, StashError};

pub const AUTHORIZE_URL: &str = "https://www.pathofexile.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://www.pathofexile.com/oauth/token";
pub const DEFAULT_PORT: u16 = 8080;
pub const SCOPES: &str = "account:profile account:stashes";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub port: u16,
}

impl OAuthConfig {
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }
}

/// GGG's required User-Agent shape for OAuth clients.
pub fn user_agent(client_id: &str) -> String {
    format!("OAuth {client_id}/{} (contact: {})", env!("CARGO_PKG_VERSION"), crate::CONTACT)
}

fn random_token(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| StashError::OAuth(format!("no randomness: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn authorize_url(cfg: &OAuthConfig, state: &str, challenge: &str) -> String {
    let mut u = reqwest::Url::parse(AUTHORIZE_URL).expect("static url");
    u.query_pairs_mut()
        .append_pair("client_id", &cfg.client_id)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPES)
        .append_pair("state", state)
        .append_pair("redirect_uri", &cfg.redirect_uri())
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    u.into()
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<i64>,
    #[serde(default)]
    scope: String,
    username: Option<String>,
    refresh_token: Option<String>,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

async fn token_request(http: &reqwest::Client, client_id: &str, form: &[(&str, &str)]) -> Result<OAuthTokens> {
    let resp = http.post(TOKEN_URL).header("User-Agent", user_agent(client_id)).form(form).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(StashError::OAuth(format!("token endpoint HTTP {}: {}", status.as_u16(), body.chars().take(200).collect::<String>())));
    }
    let t: TokenResponse = resp.json().await?;
    Ok(OAuthTokens {
        access_token: Secret::new(t.access_token),
        refresh_token: t.refresh_token.map(Secret::new),
        expires_at: now() + t.expires_in.unwrap_or(36_000),
        scope: t.scope,
        username: t.username,
        client_id: client_id.to_owned(),
    })
}

/// Full interactive login. `open_browser` is called with the authorize URL.
/// Waits up to 5 minutes for the redirect.
pub async fn login(http: &reqwest::Client, cfg: &OAuthConfig, open_browser: impl FnOnce(&str)) -> Result<OAuthTokens> {
    if cfg.client_id.trim().is_empty() {
        return Err(StashError::OAuthUnavailable("no client ID configured".into()));
    }
    let verifier = random_token(48)?;
    let state = random_token(16)?;
    let listener = TcpListener::bind(("127.0.0.1", cfg.port))
        .await
        .map_err(|e| StashError::OAuth(format!("can't listen on 127.0.0.1:{} ({e}); is another app using it?", cfg.port)))?;
    open_browser(&authorize_url(cfg, &state, &pkce_challenge(&verifier)));

    let code = tokio::time::timeout(LOGIN_TIMEOUT, wait_for_code(&listener, &state))
        .await
        .map_err(|_| StashError::OAuth("timed out waiting for the browser".into()))??;

    token_request(
        http,
        &cfg.client_id,
        &[
            ("client_id", cfg.client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", cfg.redirect_uri().as_str()),
            ("scope", SCOPES),
            ("code_verifier", verifier.as_str()),
        ],
    )
    .await
}

pub async fn refresh(http: &reqwest::Client, tokens: &OAuthTokens) -> Result<OAuthTokens> {
    let Some(rt) = &tokens.refresh_token else {
        return Err(StashError::Unauthorized);
    };
    let mut t = token_request(
        http,
        &tokens.client_id,
        &[("client_id", tokens.client_id.as_str()), ("grant_type", "refresh_token"), ("refresh_token", rt.expose())],
    )
    .await?;
    if t.username.is_none() {
        t.username = tokens.username.clone();
    }
    Ok(t)
}

const PAGE_OK: &str = "<!doctype html><meta charset=utf-8><title>Rusty POE Trade</title><body style=\"font-family:sans-serif;background:#1d1b16;color:#e8e2d9;display:grid;place-items:center;height:90vh\"><div><h2>Signed in</h2><p>You can close this tab and return to the game.</p></div>";
const PAGE_ERR: &str = "<!doctype html><meta charset=utf-8><title>Rusty POE Trade</title><body style=\"font-family:sans-serif\"><h2>Sign-in failed</h2><p>Return to Rusty POE Trade and try again.</p>";

/// Accept loopback connections until `/callback` arrives with our `state`.
async fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String> {
    loop {
        let (mut sock, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = match tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => continue,
        };
        let req = String::from_utf8_lossy(&buf[..n]);
        let Some(target) = req.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next()) else {
            continue;
        };
        let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else { continue };
        if url.path() != "/callback" {
            let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            continue;
        }
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        let result = match (q.get("state"), q.get("code"), q.get("error")) {
            (_, _, Some(err)) => Err(StashError::OAuth(q.get("error_description").cloned().unwrap_or_else(|| err.clone()))),
            (Some(s), Some(code), None) if s == expected_state => Ok(code.clone()),
            _ => Err(StashError::OAuth("unexpected callback (state mismatch)".into())),
        };
        let page = if result.is_ok() { PAGE_OK } else { PAGE_ERR };
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = sock.write_all(resp.as_bytes()).await;
        return result;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_rfc7636_vector() {
        // RFC 7636 Appendix B
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn authorize_url_has_pkce() {
        let cfg = OAuthConfig { client_id: "rustypoetrade".into(), port: 8080 };
        let u = authorize_url(&cfg, "st", "ch");
        assert!(u.starts_with(AUTHORIZE_URL));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Fcallback"));
    }

    #[tokio::test]
    async fn loopback_callback_roundtrip() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(b"GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        });
        assert_eq!(wait_for_code(&listener, "xyz").await.unwrap(), "abc");
        assert!(client.await.unwrap().contains("Signed in"));
    }
}
