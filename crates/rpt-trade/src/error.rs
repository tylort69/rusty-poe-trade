#[derive(Debug, thiserror::Error)]
pub enum TradeError {
    /// Client-side rate limiter would have to queue the request for too long
    /// (upstream `preventQueueCreation`), or the server answered with
    /// "rate limit exceeded".
    #[error("Retry after {retry_after_secs} seconds")]
    RateLimited { retry_after_secs: u64 },
    /// Error object returned by the trade API (`{ error: { code, message } }`).
    #[error("{message}")]
    Api { code: i64, message: String },
    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Invalid category: {0}")]
    InvalidCategory(String),
    /// poeprices.info specific failures (message mirrors upstream text).
    #[error("{0}")]
    Poeprices(String),
    /// Missing league selection / item data required by the call.
    #[error("{0}")]
    Precondition(String),
}

impl TradeError {
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, TradeError::RateLimited { .. })
    }
}
