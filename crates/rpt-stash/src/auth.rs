use serde::{Deserialize, Serialize};

/// Which sign-in method the user picked. Persisted in config as a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMethod {
    #[default]
    SessionCookie,
    OAuth,
}

impl AuthMethod {
    pub fn label(self) -> &'static str {
        match self {
            AuthMethod::SessionCookie => "Session cookie (POESESSID)",
            AuthMethod::OAuth => "Official login (OAuth)",
        }
    }
}

/// A string that never shows up in logs or `Debug` output.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Tokens from `POST /oauth/token`. Stored in the keychain as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    /// Unix seconds.
    pub expires_at: i64,
    pub scope: String,
    /// Account name (`Name#1234`) as returned by the token endpoint.
    pub username: Option<String>,
    /// The client ID these tokens belong to (needed to refresh).
    pub client_id: String,
}

impl OAuthTokens {
    pub fn expired(&self, now_unix: i64) -> bool {
        // 60 s of slack so we refresh before the server starts rejecting.
        now_unix + 60 >= self.expires_at
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Credentials {
    /// `POESESSID` cookie + account name (`Name#1234`, may be empty → auto-detect).
    Session { poesessid: Secret, account_name: String },
    OAuth(OAuthTokens),
}

impl Credentials {
    pub fn method(&self) -> AuthMethod {
        match self {
            Credentials::Session { .. } => AuthMethod::SessionCookie,
            Credentials::OAuth(_) => AuthMethod::OAuth,
        }
    }
}

/// Light sanity check so a pasted cookie line / whitespace still works.
/// Accepts `POESESSID=abc...;` or the bare 32-hex value.
pub fn normalize_poesessid(input: &str) -> Option<String> {
    let s = input.trim();
    let s = s.strip_prefix("POESESSID=").unwrap_or(s);
    let s = s.split(';').next().unwrap_or("").trim();
    (s.len() >= 16 && s.chars().all(|c| c.is_ascii_alphanumeric())).then(|| s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poesessid_normalization() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(normalize_poesessid(id).as_deref(), Some(id));
        assert_eq!(normalize_poesessid(&format!("  POESESSID={id}; Path=/ ")).as_deref(), Some(id));
        assert_eq!(normalize_poesessid("nope"), None);
        assert_eq!(normalize_poesessid("abc def ghi jkl mno"), None);
    }

    #[test]
    fn secret_debug_is_redacted() {
        assert_eq!(format!("{:?}", Secret::new("hunter2")), "Secret(***)");
    }
}
