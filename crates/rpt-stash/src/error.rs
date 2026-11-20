#[derive(Debug, thiserror::Error)]
pub enum StashError {
    #[error("Not signed in")]
    NotSignedIn,
    /// Cookie expired / logged out on the website, or OAuth token rejected.
    #[error("Your Path of Exile sign-in was rejected or has expired. Sign in again.")]
    Unauthorized,
    /// Signed in, but this resource is not visible (wrong account name, private tab, PoE 2 league...).
    #[error("Path of Exile refused access: {0}")]
    Forbidden(String),
    #[error("Rate limited by pathofexile.com, retry in {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("Could not find your account name. Enter it in Settings → Account (it looks like Name#1234).")]
    AccountNameUnknown,
    #[error("OAuth sign-in is not available: {0}")]
    OAuthUnavailable(String),
    #[error("OAuth sign-in failed: {0}")]
    OAuth(String),
    #[error("Keychain error: {0}")]
    Keychain(String),
    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
