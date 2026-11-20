//! State and watchers of upstream `CheckedItem.vue` plus the data side of
//! `TradeListing.vue`, `TradeBulk.vue` and `PricePrediction.vue`.

use std::collections::{HashMap, HashSet};

use rpt_parser::ParsedItem;
use rpt_trade::check;
use rpt_trade::filters::{FilterOrGroup, ItemFilters, Presets};
use rpt_trade::poeprices::RareItemPrice;
use rpt_trade::trade::TradeApi;
use rpt_trade::trade::bulk::{BulkCurrency, BulkResults, MarketRatioMap, find_market_ratio};
use rpt_trade::trade::search::ListingSearch;
use rpt_trade::{TradeConfig, TradeError};

use super::ui::Slowdown;
use crate::services::{Progress, Services, Task};

/// Error shown in the trade error box; `json` = upstream `error.includes('JSON')`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchError {
    pub message: String,
    pub json: bool,
}

impl From<TradeError> for SearchError {
    fn from(e: TradeError) -> Self {
        let json = matches!(e, TradeError::Json(_)) || e.to_string().contains("JSON");
        Self { message: e.to_string(), json }
    }
}

pub struct ListingState {
    task: Option<Task<Result<ListingSearch, SearchError>>>,
    progress: Progress<ListingSearch>,
    pub result: Option<ListingSearch>,
    pub error: Option<SearchError>,
}

impl ListingState {
    pub fn start(sv: &Services, filters: &ItemFilters, stats: &[FilterOrGroup]) -> Self {
        let progress: Progress<ListingSearch> = Default::default();
        let (p, ctx, trade) = (progress.clone(), sv.ctx().clone(), sv.trade.clone());
        let (filters, stats) = (filters.clone(), stats.to_vec());
        let task = sv.spawn(async move {
            trade
                .search_listings(&filters, &stats, move |s| {
                    *p.lock() = Some(s.clone());
                    ctx.request_repaint();
                })
                .await
                .map_err(SearchError::from)
        });
        Self { task: Some(task), progress, result: None, error: None }
    }

    pub fn poll(&mut self) {
        if let Some(p) = self.progress.lock().take() {
            self.result = Some(p);
        }
        if let Some(t) = &mut self.task {
            if let Some(r) = t.poll_take() {
                match r {
                    Ok(s) => self.result = Some(s),
                    Err(e) => self.error = Some(e),
                }
                self.task = None;
            }
        }
    }

    pub fn search_id(&self) -> Option<&str> {
        self.result.as_ref().map(|r| r.search_result.id.as_str())
    }
}

pub struct BulkState {
    task: Option<Task<Result<BulkResults, SearchError>>>,
    lazy: Option<Task<Result<BulkResults, SearchError>>>,
    requested: HashSet<BulkCurrencyKey>,
    pub results: Option<BulkResults>,
    pub error: Option<SearchError>,
    pub selected: BulkCurrency,
    pub ratio: MarketRatioMap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BulkCurrencyKey(bool);

fn key(c: BulkCurrency) -> BulkCurrencyKey {
    BulkCurrencyKey(c == BulkCurrency::XchgStable)
}

impl BulkState {
    pub fn start(sv: &Services, item: &ParsedItem, filters: &ItemFilters) -> Self {
        let (trade, i, f) = (sv.trade.clone(), item.clone(), filters.clone());
        let task = sv.spawn(async move { trade.bulk_search(&i, &f).await.map_err(SearchError::from) });
        let selected_league = sv.leagues.selected().map(|l| l.id);
        let ratio = find_market_ratio(item, filters, &sv.prices, selected_league.as_deref());
        Self {
            task: Some(task),
            lazy: None,
            requested: HashSet::new(),
            results: None,
            error: None,
            selected: BulkCurrency::XchgChaos,
            ratio,
        }
    }

    pub fn poll(&mut self, sv: &Services, item: &ParsedItem, filters: &ItemFilters) {
        if let Some(t) = &mut self.task {
            if let Some(r) = t.poll_take() {
                match r {
                    Ok(res) => {
                        self.selected = res.default_currency();
                        for c in [BulkCurrency::XchgChaos, BulkCurrency::XchgStable] {
                            if res.get(c).is_some() {
                                self.requested.insert(key(c));
                            }
                        }
                        self.results = Some(res);
                    }
                    Err(e) => self.error = Some(e),
                }
                self.task = None;
            }
        }
        if let Some(t) = &mut self.lazy {
            if let Some(r) = t.poll_take() {
                match r {
                    Ok(res) => self.results = Some(res),
                    Err(e) => self.error = Some(e),
                }
                self.lazy = None;
            }
        }
        // `listedLazy`: load the selected tab on first access
        if let Some(res) = &self.results {
            let sel = self.selected;
            if res.get(sel).is_none() && !self.requested.contains(&key(sel)) && self.lazy.is_none() {
                self.requested.insert(key(sel));
                let (trade, i, f, mut r) = (sv.trade.clone(), item.clone(), filters.clone(), res.clone());
                self.lazy = Some(sv.spawn(async move {
                    trade.load_bulk_have(&i, &f, &mut r, sel).await.map_err(SearchError::from)?;
                    Ok(r)
                }));
            }
        }
    }
}

pub struct PredictionState {
    task: Option<Task<Result<RareItemPrice, String>>>,
    pub price: Option<RareItemPrice>,
    pub error: Option<String>,
    pub show_contrib: bool,
    pub feedback_sent: bool,
    pub feedback_text: String,
}

impl PredictionState {
    pub fn start(sv: &Services, item: &ParsedItem, league: &str) -> Self {
        let (trade, prices, raw, league) = (sv.trade.clone(), sv.prices.clone(), item.raw_text.clone(), league.to_owned());
        let task = sv.spawn(async move { trade.request_poeprices(&raw, &league, &prices).await.map_err(|e| e.to_string()) });
        Self { task: Some(task), price: None, error: None, show_contrib: false, feedback_sent: false, feedback_text: String::new() }
    }

    pub fn loading(&self) -> bool {
        self.task.is_some()
    }

    pub fn poll(&mut self) {
        if let Some(t) = &mut self.task {
            if let Some(r) = t.poll_take() {
                match r {
                    Ok(p) => self.price = Some(p),
                    Err(e) => self.error = Some(e),
                }
                self.task = None;
            }
        }
    }
}

/// UI toggles of `FiltersBlock.vue` (reset per item).
#[derive(Debug, Default, Clone)]
pub struct FiltersUi {
    /// `statsVisibility.disabled`
    pub stats_collapsed: bool,
    pub show_hidden: bool,
    pub show_sources: bool,
}

/// A price-check session for one item (upstream `CheckedItem` instance state).
pub struct Session {
    pub item: ParsedItem,
    /// opened with the "locked" hotkey (overlay focused)
    #[allow(dead_code)]
    pub advanced_check: bool,
    pub presets: Presets,
    pub do_search: bool,
    pub trade_api: TradeApi,
    pub listing: Option<ListingState>,
    pub bulk: Option<BulkState>,
    pub prediction: Option<PredictionState>,
    pub filters_ui: FiltersUi,
    pub slowdown: Slowdown,
    /// min inputs of numeric filter buttons (`''` while editing = `None`)
    pub numeric_inputs: HashMap<&'static str, Option<f64>>,
    /// numeric filters created with a `max` key (upstream `'max' in filter`)
    pub has_max: HashSet<&'static str>,
    /// pointer was over the filters block last frame (`handleSearchMouseenter`)
    pub pointer_in_filters: bool,
    snapshot: String,
    trade_snapshot: String,
    snapshot_do_search: bool,
}

/// Names of the numeric filter buttons, in display order.
pub const NUMERIC_FILTERS: &[&str] =
    &["linked_sockets", "map_tier", "area_level", "sentinel_charge", "item_level", "stack_size", "white_sockets", "gem_level", "quality"];

pub fn numeric_filter<'a>(f: &'a mut ItemFilters, name: &str) -> Option<&'a mut rpt_trade::filters::FilterNumeric> {
    match name {
        "linked_sockets" => f.linked_sockets.as_mut(),
        "map_tier" => f.map_tier.as_mut(),
        "area_level" => f.area_level.as_mut(),
        "sentinel_charge" => f.sentinel_charge.as_mut(),
        "item_level" => f.item_level.as_mut(),
        "stack_size" => f.stack_size.as_mut(),
        "white_sockets" => f.white_sockets.as_mut(),
        "gem_level" => f.gem_level.as_mut(),
        "quality" => f.quality.as_mut(),
        _ => None,
    }
}

/// Outcome of the filter watchers in `CheckedItem.vue` for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub do_search: bool,
    /// a new search must be started (doSearch became true)
    pub start: bool,
}

/// Watchers 3 and 4 of `CheckedItem.vue`: editing filters while results are
/// shown hides them (user must press Search); changing trade options
/// (online filter) searches again immediately.
pub fn filters_transition(stats_changed: bool, trade_changed: bool, do_search: bool, prev_do_search: bool) -> Transition {
    let mut d = do_search;
    if stats_changed && d && prev_do_search {
        d = false;
    }
    if trade_changed {
        return Transition { do_search: true, start: true };
    }
    Transition { do_search: d, start: false }
}

impl Session {
    pub fn new(item: ParsedItem, advanced_check: bool, prev: Option<&Session>, cfg: &TradeConfig, league: &str) -> Self {
        let prev_arg = prev.map(|p| (&p.item, p.active_filters().and_then(|f| f.trade.currency.clone())));
        let presets = check::presets_for_item(&item, prev_arg, cfg, league);
        let do_search = check::initial_do_search(&item, &presets, advanced_check, cfg);
        let trade_api = check::trade_api_for(&item, &presets);
        let mut has_max = HashSet::new();
        let mut s = Self {
            item,
            advanced_check,
            presets,
            do_search,
            trade_api,
            listing: None,
            bulk: None,
            prediction: None,
            filters_ui: FiltersUi::default(),
            slowdown: Slowdown::default(),
            numeric_inputs: HashMap::new(),
            has_max: HashSet::new(),
            pointer_in_filters: false,
            snapshot: String::new(),
            trade_snapshot: String::new(),
            snapshot_do_search: do_search,
        };
        for p in &mut s.presets.presets {
            for name in NUMERIC_FILTERS {
                if numeric_filter(&mut p.filters, name).is_some_and(|f| f.max.is_some()) {
                    has_max.insert(*name);
                }
            }
        }
        s.has_max = has_max;
        s.take_snapshots();
        s
    }

    pub fn active_filters(&self) -> Option<&ItemFilters> {
        self.presets.active_preset().map(|p| &p.filters)
    }

    fn snapshots(&self) -> (String, String) {
        let p = self.presets.active_preset();
        let full = serde_json::to_string(&(&self.presets.active, p)).unwrap_or_default();
        let trade = p.map(|p| serde_json::to_string(&p.filters.trade).unwrap_or_default()).unwrap_or_default();
        (full, trade)
    }

    fn take_snapshots(&mut self) {
        let (a, b) = self.snapshots();
        self.snapshot = a;
        self.trade_snapshot = b;
        self.snapshot_do_search = self.do_search;
    }

    /// Start the search for the active preset (`execSearch` of the trade component).
    pub fn start_search(&mut self, sv: &Services) {
        self.do_search = true;
        self.trade_api = check::trade_api_for(&self.item, &self.presets);
        self.listing = None;
        self.bulk = None;
        let Some(p) = self.presets.active_preset() else { return };
        match self.trade_api {
            TradeApi::Trade => self.listing = Some(ListingState::start(sv, &p.filters, &p.stats)),
            TradeApi::Bulk => self.bulk = Some(BulkState::start(sv, &self.item, &p.filters)),
        }
    }

    pub fn reset_results(&mut self) {
        self.do_search = false;
        self.listing = None;
        self.bulk = None;
    }

    /// Run the filter watchers after the UI mutated filters this frame.
    pub fn after_frame(&mut self, sv: &Services) {
        let (full, trade) = self.snapshots();
        let t = filters_transition(full != self.snapshot, trade != self.trade_snapshot, self.do_search, self.snapshot_do_search);
        if t.start {
            self.start_search(sv);
        } else if !t.do_search && self.do_search {
            self.reset_results();
        }
        self.take_snapshots();
    }

    pub fn poll(&mut self, sv: &Services) {
        if let Some(l) = &mut self.listing {
            l.poll();
        }
        if let (Some(b), Some(p)) = (&mut self.bulk, self.presets.active_preset()) {
            b.poll(sv, &self.item, &p.filters);
        }
        if let Some(p) = &mut self.prediction {
            p.poll();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchers() {
        // editing stats while results are shown hides them
        assert_eq!(filters_transition(true, false, true, true), Transition { do_search: false, start: false });
        // editing before the first search does nothing
        assert_eq!(filters_transition(true, false, false, false), Transition { do_search: false, start: false });
        // the search was just requested in this frame: keep it
        assert_eq!(filters_transition(true, false, true, false), Transition { do_search: true, start: false });
        // online filter / league / currency changes re-search
        assert_eq!(filters_transition(true, true, true, true), Transition { do_search: true, start: true });
        assert_eq!(filters_transition(false, true, false, false), Transition { do_search: true, start: true });
        assert_eq!(filters_transition(false, false, true, true), Transition { do_search: true, start: false });
    }

    #[test]
    fn session_from_clipboard() {
        rpt_data::init("en").unwrap();
        let item = rpt_parser::parse_clipboard(
            "Item Class: Stackable Currency\nRarity: Currency\nChaos Orb\n--------\nStack Size: 12/20\n--------\nReforges a rare item with new random modifiers\n",
        )
        .unwrap();
        let cfg = TradeConfig { league_id: Some("Standard".into()), ..Default::default() };
        let s = Session::new(item, false, None, &cfg, "Standard");
        assert!(s.do_search);
        assert_eq!(s.trade_api, TradeApi::Bulk);
        assert!(s.active_filters().unwrap().stack_size.is_some());
    }
}
