//! HTTP client + settings that upstream read from the global `AppConfig()`.
//!
//! Upstream routed every request through an Electron proxy (`main/src/proxy.ts`)
//! that only allowed a fixed host list, forced `https://`, replaced the
//! User-Agent and used the Electron session cookie jar (Cloudflare cookies).
//! Here requests go directly through `reqwest` with an in-memory cookie store
//! and a generic app User-Agent. No account cookies (POESESSID) are sent.

use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::filters::CollapseListings;
use crate::trade::bulk::BulkSearchResponse;
use crate::trade::cache::Cache;
use crate::trade::rate_limiter::{RateLimitRules, RateLimitState, RateLimiter};
use crate::trade::search::{FetchResult, SearchResult};
use crate::{Result, TradeError};

pub const USER_AGENT: &str = concat!("RustyPoeTrade/", env!("CARGO_PKG_VERSION"));

/// Hosts upstream's proxy allowed (`PROXY_HOSTS`).
pub const ALLOWED_HOSTS: &[&str] = &[
    "www.pathofexile.com",
    "ru.pathofexile.com",
    "pathofexile.tw",
    "poe.kakaogames.com",
    "poe.ninja",
    "www.poeprices.info",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Realm {
    #[default]
    #[serde(rename = "pc-ggg")]
    PcGgg,
    #[serde(rename = "pc-garena")]
    PcGarena,
}

impl Realm {
    pub fn as_str(self) -> &'static str {
        match self {
            Realm::PcGgg => "pc-ggg",
            Realm::PcGarena => "pc-garena",
        }
    }
}

/// Upstream `showSeller: false | 'account' | 'ign'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShowSeller {
    #[default]
    No,
    Account,
    Ign,
}

impl Serialize for ShowSeller {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            ShowSeller::No => s.serialize_bool(false),
            ShowSeller::Account => s.serialize_str("account"),
            ShowSeller::Ign => s.serialize_str("ign"),
        }
    }
}

impl<'de> Deserialize<'de> for ShowSeller {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(match serde_json::Value::deserialize(d)? {
            serde_json::Value::String(s) if s == "account" => ShowSeller::Account,
            serde_json::Value::String(s) if s == "ign" => ShowSeller::Ign,
            _ => ShowSeller::No,
        })
    }
}

/// Upstream `PriceCheckWidget` options relevant to the core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PriceCheckOptions {
    pub show_seller: ShowSeller,
    pub search_stat_range: f64,
    pub show_rate_limit_state: bool,
    pub api_latency_seconds: f64,
    pub collapse_listings: CollapseListings,
    pub smart_initial_search: bool,
    pub locked_initial_search: bool,
    pub activate_stock_filter: bool,
    pub merchant_only: bool,
    pub default_currency: Option<String>,
    pub request_price_prediction: bool,
    pub builtin_browser: bool,
}

impl Default for PriceCheckOptions {
    /// Upstream `initInstance()` values for the price-check widget.
    fn default() -> Self {
        Self {
            show_seller: ShowSeller::No,
            search_stat_range: 10.0,
            show_rate_limit_state: false,
            api_latency_seconds: 2.0,
            collapse_listings: CollapseListings::Api,
            smart_initial_search: true,
            locked_initial_search: true,
            activate_stock_filter: false,
            merchant_only: true,
            default_currency: None,
            request_price_prediction: false,
            builtin_browser: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TradeConfig {
    pub league_id: Option<String>,
    pub realm: Realm,
    /// game data language: "en" | "ru" | "cmn-Hant" | "ko"
    pub language: String,
    pub use_intl_site: bool,
    pub account_name: String,
    pub price_check: PriceCheckOptions,
}

impl Default for TradeConfig {
    fn default() -> Self {
        Self {
            league_id: None,
            realm: Realm::PcGgg,
            language: "en".to_owned(),
            use_intl_site: false,
            account_name: String::new(),
            price_check: PriceCheckOptions::default(),
        }
    }
}

impl TradeConfig {
    /// Upstream `poeWebApi()`.
    pub fn poe_web_api(&self) -> &'static str {
        poe_web_api(&self.language, self.use_intl_site)
    }
}

/// Upstream `poeWebApi()`.
pub fn poe_web_api(language: &str, use_intl_site: bool) -> &'static str {
    if use_intl_site {
        return "www.pathofexile.com";
    }
    match language {
        "ru" => "ru.pathofexile.com",
        "cmn-Hant" => "pathofexile.tw",
        "ko" => "poe.kakaogames.com",
        _ => "www.pathofexile.com",
    }
}

/// Shared HTTP client for the trade site, poe.ninja and poeprices.info.
/// Holds the rate limiters and response caches (upstream module globals).
pub struct TradeClient {
    http: reqwest::Client,
    config: RwLock<TradeConfig>,
    pub(crate) search_limits: RateLimitRules,
    pub(crate) exchange_limits: RateLimitRules,
    pub(crate) fetch_limits: RateLimitRules,
    pub(crate) search_cache: Cache<SearchResult>,
    pub(crate) fetch_cache: Cache<Vec<FetchResult>>,
    pub(crate) bulk_cache: Cache<BulkSearchResponse>,
    pub(crate) poeprices_cache: Cache<crate::poeprices::PoepricesApiResponse>,
}

impl TradeClient {
    pub fn new(config: TradeConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .cookie_store(true)
            .build()?;
        Ok(Self::with_http(http, config))
    }

    /// Use a custom `reqwest::Client` (it should have a cookie store enabled).
    pub fn with_http(http: reqwest::Client, config: TradeConfig) -> Self {
        Self {
            http,
            config: RwLock::new(config),
            search_limits: RateLimitRules::new("trade-search-request-limit", vec![RateLimiter::new(1, 5.0)]),
            exchange_limits: RateLimitRules::new("trade-exchange-request-limit", vec![RateLimiter::new(1, 5.0)]),
            fetch_limits: RateLimitRules::new("trade-fetch-request-limit", vec![RateLimiter::new(1, 5.0)]),
            search_cache: Cache::new(),
            fetch_cache: Cache::new(),
            bulk_cache: Cache::new(),
            poeprices_cache: Cache::new(),
        }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn config(&self) -> TradeConfig {
        self.config.read().clone()
    }

    pub fn set_config(&self, config: TradeConfig) {
        *self.config.write() = config;
    }

    pub fn update_config(&self, f: impl FnOnce(&mut TradeConfig)) {
        f(&mut self.config.write());
    }

    /// Upstream `getTradeEndpoint()` (host only, e.g. `www.pathofexile.com`).
    pub fn trade_endpoint(&self) -> &'static str {
        self.config.read().poe_web_api()
    }

    /// Snapshot for the UI rate-limit indicator (upstream `RateLimiterState.vue`).
    pub fn rate_limit_state(&self) -> RateLimitState {
        RateLimitState {
            limits: vec![self.search_limits.state(), self.exchange_limits.state(), self.fetch_limits.state()],
        }
    }

    /// Sends a trade-site API request, re-syncs the rate limiters with the
    /// response headers and decodes `TradeResponse<T>`.
    pub(crate) async fn trade_api<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
        limits: &RateLimitRules,
    ) -> Result<T> {
        let desync = self.config.read().price_check.api_latency_seconds;
        let resp = req.send().await?;
        limits.adjust_from_headers(resp.headers(), desync);
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let text = resp.text().await?;
        decode_trade_response(status.as_u16(), retry_after, &text)
    }
}

pub(crate) fn decode_trade_response<T: DeserializeOwned>(
    status: u16,
    retry_after: Option<u64>,
    text: &str,
) -> Result<T> {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            if !(200..300).contains(&status) {
                return Err(TradeError::Status { status, body: text.chars().take(300).collect() });
            }
            return Err(e.into());
        }
    };
    if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("").to_owned();
        if status == 429 || code == 3 {
            return Err(TradeError::RateLimited { retry_after_secs: retry_after.unwrap_or(60) });
        }
        return Err(TradeError::Api { code, message });
    }
    if !(200..300).contains(&status) {
        return Err(TradeError::Status { status, body: text.chars().take(300).collect() });
    }
    // decode from text (not `value`) to keep object key order
    Ok(serde_json::from_str(text)?)
}
