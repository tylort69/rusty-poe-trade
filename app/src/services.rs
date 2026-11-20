//! Long-lived background services shared by widgets: async runtime, trade
//! client, poe.ninja price index and the league list (upstream
//! `background/Prices.ts`, `background/Leagues.ts` and the trade API globals).

use std::future::Future;
use std::sync::Arc;

use parking_lot::Mutex;
use rpt_trade::client::ShowSeller as TradeShowSeller;
use rpt_trade::filters::CollapseListings;
use rpt_trade::{Leagues, PriceCheckOptions, PriceIndex, Realm, TradeClient, TradeConfig};

use crate::config::{Config, PriceCheckWidget, ShowSeller};

pub struct Services {
    pub rt: tokio::runtime::Runtime,
    pub trade: Arc<TradeClient>,
    pub prices: Arc<PriceIndex>,
    pub leagues: Arc<Leagues>,
    ctx: egui::Context,
    /// Last config mirrored into the trade client (detects edits made by the settings UI).
    synced: Mutex<TradeConfig>,
}

impl Services {
    pub fn new(cfg: &Config, ctx: egui::Context) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("rpt-net").build().expect("tokio runtime");
        let tc = trade_config(cfg);
        let trade = Arc::new(TradeClient::new(tc.clone()).unwrap_or_else(|e| {
            tracing::error!("failed to build HTTP client: {e}");
            TradeClient::with_http(reqwest::Client::new(), tc.clone())
        }));
        let leagues = Arc::new(Leagues::new(trade.clone()));
        let prices = Arc::new(PriceIndex::new(trade.clone(), leagues.clone()));
        let this = Self { rt, trade, prices, leagues, ctx, synced: Mutex::new(tc) };
        {
            let _guard = this.rt.enter();
            this.prices.spawn_refresh_loop();
        }
        this.reload_leagues();
        this
    }

    /// (Re)load the league list, then the price index for the selected league
    /// (upstream `leagues.load()` + `watch(selectedId)` in `usePoeninja`).
    pub fn reload_leagues(&self) {
        let leagues = self.leagues.clone();
        let prices = self.prices.clone();
        let ctx = self.ctx.clone();
        self.rt.spawn(async move {
            ctx.request_repaint();
            if let Err(e) = leagues.load().await {
                tracing::warn!("failed to load leagues: {e}");
            } else if let Err(e) = prices.on_league_changed().await {
                tracing::warn!("poe.ninja load failed: {e}");
            }
            ctx.request_repaint();
        });
    }

    /// Upstream `queuePricesFetch`: mark interest in prices and refresh if stale.
    pub fn queue_prices_fetch(&self) {
        let prices = self.prices.clone();
        let ctx = self.ctx.clone();
        self.rt.spawn(async move {
            if let Err(e) = prices.queue_prices_fetch().await {
                tracing::warn!("poe.ninja load failed: {e}");
            }
            ctx.request_repaint();
        });
    }

    fn on_league_changed(&self) {
        let prices = self.prices.clone();
        let ctx = self.ctx.clone();
        self.rt.spawn(async move {
            if let Err(e) = prices.on_league_changed().await {
                tracing::warn!("poe.ninja load failed: {e}");
            }
            ctx.request_repaint();
        });
    }

    /// Two-way sync between the app config and the trade client, call once per
    /// frame. Config edits (settings UI) are pushed into the client; a league
    /// auto-selected by [`Leagues`] is written back into `cfg.league_id`.
    /// Returns `true` when `cfg` was modified (so the app can persist it).
    pub fn sync_into(&self, cfg: &mut Config) -> bool {
        let from_cfg = trade_config(cfg);
        let mut synced = self.synced.lock();
        match sync_decision(&synced, &from_cfg, &self.trade.config()) {
            Sync::None => false,
            Sync::Push { league_changed, endpoint_changed } => {
                self.trade.set_config(from_cfg.clone());
                *synced = from_cfg;
                drop(synced);
                if endpoint_changed {
                    self.reload_leagues();
                } else if league_changed {
                    self.on_league_changed();
                }
                false
            }
            Sync::Pull(league) => {
                cfg.league_id = league;
                *synced = trade_config(cfg);
                drop(synced);
                self.on_league_changed();
                true
            }
        }
    }

    /// Run a future on the runtime; the returned [`Task`] receives its output
    /// and repaints the UI when it completes. Dropping the task aborts it.
    pub fn spawn<T, F>(&self, fut: F) -> Task<T>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        Task::spawn(&self.rt, &self.ctx, fut)
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }
}

#[derive(Debug, PartialEq)]
enum Sync {
    None,
    Push { league_changed: bool, endpoint_changed: bool },
    Pull(Option<String>),
}

fn sync_decision(synced: &TradeConfig, from_cfg: &TradeConfig, client: &TradeConfig) -> Sync {
    if from_cfg != synced {
        return Sync::Push {
            league_changed: from_cfg.league_id != client.league_id,
            endpoint_changed: from_cfg.poe_web_api() != synced.poe_web_api() || from_cfg.realm != synced.realm,
        };
    }
    if client.league_id != from_cfg.league_id {
        return Sync::Pull(client.league_id.clone());
    }
    Sync::None
}

/// Map the app config (upstream `AppConfig()` + price-check widget) to [`TradeConfig`].
pub fn trade_config(cfg: &Config) -> TradeConfig {
    TradeConfig {
        league_id: cfg.league_id.clone(),
        realm: if cfg.realm == "pc-garena" { Realm::PcGarena } else { Realm::PcGgg },
        language: cfg.language.clone(),
        use_intl_site: cfg.use_intl_site,
        account_name: cfg.account_name.clone(),
        price_check: price_check_options(cfg.price_check()),
    }
}

pub fn price_check_options(pc: &PriceCheckWidget) -> PriceCheckOptions {
    PriceCheckOptions {
        show_seller: match &pc.show_seller {
            ShowSeller::Mode(m) if m == "account" => TradeShowSeller::Account,
            ShowSeller::Mode(m) if m == "ign" => TradeShowSeller::Ign,
            _ => TradeShowSeller::No,
        },
        search_stat_range: pc.search_stat_range,
        show_rate_limit_state: pc.show_rate_limit_state,
        api_latency_seconds: pc.api_latency_seconds,
        collapse_listings: if pc.collapse_listings == "app" { CollapseListings::App } else { CollapseListings::Api },
        smart_initial_search: pc.smart_initial_search,
        locked_initial_search: pc.locked_initial_search,
        activate_stock_filter: pc.activate_stock_filter,
        merchant_only: pc.merchant_only,
        default_currency: pc.default_currency.clone(),
        request_price_prediction: pc.request_price_prediction,
        builtin_browser: pc.builtin_browser,
        smart_initial_always: pc.smart_initial_always,
        locked_initial_always: pc.locked_initial_always,
        default_offline: pc.default_offline,
        default_listed: pc.default_listed.clone(),
        fill_roll_max: pc.fill_roll_max,
    }
}

// ---------------------------------------------------------------------------

/// Result slot of a background future (a tiny `poll_promise`).
pub struct Task<T> {
    slot: Arc<Mutex<Option<T>>>,
    handle: tokio::task::JoinHandle<()>,
    taken: bool,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn<F>(rt: &tokio::runtime::Runtime, ctx: &egui::Context, fut: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        let slot = Arc::new(Mutex::new(None));
        let out = slot.clone();
        let ctx = ctx.clone();
        let handle = rt.spawn(async move {
            let v = fut.await;
            *out.lock() = Some(v);
            ctx.request_repaint();
        });
        Self { slot, handle, taken: false }
    }

    /// Take the result once it is ready (returns `Some` exactly once).
    pub fn poll_take(&mut self) -> Option<T> {
        let v = self.slot.lock().take();
        if v.is_some() {
            self.taken = true;
        }
        v
    }

    /// Still running (no result produced or taken yet).
    #[allow(dead_code)]
    pub fn is_pending(&self) -> bool {
        !self.taken && self.slot.lock().is_none()
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Shared slot for streaming partial results from a background task.
pub type Progress<T> = Arc<Mutex<Option<T>>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_widget_options() {
        let mut cfg = Config::default();
        cfg.realm = "pc-garena".into();
        if let Some(crate::config::WidgetKind::PriceCheck(pc)) = cfg.widgets.iter_mut().map(|w| &mut w.kind).find(|k| matches!(k, crate::config::WidgetKind::PriceCheck(_))) {
            pc.show_seller = ShowSeller::Mode("ign".into());
            pc.collapse_listings = "app".into();
        }
        let tc = trade_config(&cfg);
        assert_eq!(tc.realm, Realm::PcGarena);
        assert_eq!(tc.price_check.show_seller, TradeShowSeller::Ign);
        assert_eq!(tc.price_check.collapse_listings, CollapseListings::App);
        assert_eq!(tc.price_check.search_stat_range, 10.0);
    }

    #[test]
    fn sync_directions() {
        let base = TradeConfig { league_id: Some("A".into()), ..Default::default() };
        assert_eq!(sync_decision(&base, &base, &base), Sync::None);
        // leagues auto-selected another league in the client
        let client = TradeConfig { league_id: Some("B".into()), ..base.clone() };
        assert_eq!(sync_decision(&base, &base, &client), Sync::Pull(Some("B".into())));
        // settings changed the league
        let edited = TradeConfig { league_id: Some("C".into()), ..base.clone() };
        assert_eq!(sync_decision(&base, &edited, &base), Sync::Push { league_changed: true, endpoint_changed: false });
        let lang = TradeConfig { language: "ru".into(), ..base.clone() };
        assert_eq!(sync_decision(&base, &lang, &base), Sync::Push { league_changed: false, endpoint_changed: true });
    }

    #[test]
    fn task_delivers_once() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let ctx = egui::Context::default();
        let mut t = Task::spawn(&rt, &ctx, async { 7 });
        rt.block_on(async { tokio::task::yield_now().await });
        for _ in 0..100 {
            if !t.is_pending() {
                break;
            }
            rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(1)).await });
        }
        assert_eq!(t.poll_take(), Some(7));
        assert_eq!(t.poll_take(), None);
        assert!(!t.is_pending());
    }
}
