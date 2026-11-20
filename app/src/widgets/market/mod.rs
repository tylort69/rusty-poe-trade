//! Market browser: 7-day economy view, favorites / watchlist, saved trade
//! searches and price-history charts.
//!
//! Data sources:
//! - the poe.ninja dense overview already loaded by [`PriceIndex`] (prices and the
//!   relative 7-day sparkline of every line),
//! - poe.ninja exchange history (`/poe1/api/economy/exchange/current/details`),
//!   available for overviews with `source: "cx"` (currency, fragments, cards, oils …),
//! - local daily snapshots of favorites in `data_dir()/market/history.jsonl`, so
//!   history beyond poe.ninja's window accumulates over time.
//!
//! Favorites are stored per league (each entry carries its league); saved
//! searches are global (the trade URL already contains the league).

pub mod chart;
pub mod data;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Align, Layout, RichText, Sense, Ui, UiBuilder, vec2};
use rpt_m3::{Button, Chip, IconButton, TypeRole, icons, theme, with_alpha};
use rpt_trade::PriceIndex;
use rpt_trade::prices::{DbQuery, NinjaPrice};
use serde::{Deserialize, Serialize};

pub use chart::{ChartOpts, Series, line_chart};
pub use data::{Favorite, ItemKey, SavedSearch};
use data::{DayPoint, EconRow, HistoryStore, SortBy, Snapshot};

use super::{WCtx, chrome};
use crate::config::{Anchor, Config, WidgetKind};
use crate::host::HostCommand;
use crate::services::{Services, Task};

/// Material Icons Round codepoints not in `rpt_m3::icons`.
const STAR_BORDER: &str = "\u{e83a}";
const NOTIFICATIONS_ACTIVE: &str = "\u{e7f7}";

const PANEL_WIDTH: f32 = 780.0;
const LIST_HEIGHT: f32 = 430.0;
const ROW_H: f32 = 36.0;

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-market"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MarketWidget {
    pub anchor: Anchor,
    /// Watchlist; each entry is tied to the league it was starred in.
    pub favorites: Vec<Favorite>,
    /// Global, user-ordered.
    pub saved_searches: Vec<SavedSearch>,
}

impl Default for MarketWidget {
    fn default() -> Self {
        Self { anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 }, favorites: vec![], saved_searches: vec![] }
    }
}

/// The market widget's persisted settings inside `cfg` (created if missing).
pub fn market_widget_mut(cfg: &mut Config) -> Option<&mut MarketWidget> {
    if !cfg.widgets.iter().any(|w| matches!(w.kind, WidgetKind::Market(_))) {
        cfg.ensure_rpt_widgets();
    }
    cfg.widgets.iter_mut().find_map(|w| match &mut w.kind {
        WidgetKind::Market(m) => Some(m),
        _ => None,
    })
}

/// Save a trade-site search (for e.g. a "Save search" button in the price check).
/// Validates that the URL is a pathofexile.com trade link; an already saved URL
/// is renamed instead of duplicated. The caller should persist the config.
#[allow(dead_code)] // public API for the price-check window (not wired yet)
pub fn save_search(cfg: &mut Config, name: &str, url: &str) -> Result<(), String> {
    let mw = market_widget_mut(cfg).ok_or_else(|| "market widget missing".to_owned())?;
    data::upsert_search(&mut mw.saved_searches, name, url, "").map(|_| ()).map_err(str::to_owned)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Economy,
    Favorites,
    Saved,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Range {
    D7,
    D30,
    #[default]
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Unit {
    #[default]
    Chaos,
    Divine,
}

struct EconCache {
    type_: String,
    updated: Option<Instant>,
    url: &'static str,
    rows: Vec<EconRow>,
    view_key: Option<(String, SortBy, bool)>,
    view: Vec<usize>,
}

enum HistEntry {
    Loading(Task<Result<Vec<DayPoint>, String>>),
    Done(Result<Vec<DayPoint>, String>),
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct MarketState {
    pub tab: Tab,
    category: String,
    search: String,
    sort: SortBy,
    ascending: bool,
    econ: Option<EconCache>,
    /// item shown in the History tab
    pub selected: Option<ItemKey>,
    range: Range,
    unit: Unit,
    ninja_hist: HashMap<(String, ItemKey), HistEntry>,
    /// overview type → (`source`, url slug), invalidated with the price index
    type_meta: HashMap<String, (Option<String>, &'static str)>,
    quotes: HashMap<ItemKey, Option<NinjaPrice>>,
    stamp: Option<(Option<Instant>, Option<String>)>,
    pub history: HistoryStore,
    snapshot_key: Option<(i64, Option<Instant>, usize)>,
    last_interest: Option<Instant>,
    // saved-search form
    new_name: String,
    new_url: String,
    new_notes: String,
    form_error: Option<String>,
    editing: Option<usize>,
    edit_name: String,
    edit_notes: String,
    confirm_delete: Option<usize>,
}

/// What the body needs from the app (kept small so it can be rendered headless).
pub struct Env<'a> {
    pub prices: &'a PriceIndex,
    pub league: Option<&'a str>,
    /// For background poe.ninja history requests; `None` disables fetching.
    pub services: Option<&'a Services>,
}

#[derive(Default)]
pub struct Outcome {
    pub open_urls: Vec<String>,
    /// persisted settings changed
    pub changed: bool,
}

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, st: &mut MarketState, wm_id: u32) {
    if st.history.path.is_none() {
        st.history.path = Some(HistoryStore::default_path());
    }
    if !st.history.is_loaded() {
        st.history.load();
    }
    let services: &Services = w.services;
    if st.last_interest.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
        services.queue_prices_fetch();
        st.last_interest = Some(Instant::now());
    }
    let league = w.cfg.league_id.clone();
    if let (Some(league), Some(mw)) = (league.as_deref(), market_widget_mut(w.cfg)) {
        snapshot_favorites(&services.prices, league, &mw.favorites, st);
    }
    let mut out = Outcome::default();
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, w| {
        let env = Env { prices: &services.prices, league: league.as_deref(), services: Some(services) };
        if let Some(mw) = market_widget_mut(w.cfg) {
            out = body(ui, &env, mw, st);
        }
    });
    chrome::apply(w, wm_id, &r);
    for url in out.open_urls {
        w.host.push(HostCommand::OpenUrl(url));
    }
    if out.changed {
        w.save_requested = true;
    }
}

/// Record today's price of every favorite of `league` (once per day and item).
fn snapshot_favorites(prices: &PriceIndex, league: &str, favs: &[Favorite], st: &mut MarketState) {
    let updated = prices.last_update();
    let key = (data::today(), updated, favs.len());
    if updated.is_none() || st.snapshot_key == Some(key) {
        return;
    }
    st.snapshot_key = Some(key);
    let day = data::today();
    let snaps: Vec<Snapshot> = favs
        .iter()
        .filter(|f| f.league == league)
        .filter_map(|f| quote(prices, &f.key).map(|q| Snapshot { day, league: league.to_owned(), key: f.key.clone(), chaos: q.chaos }))
        .collect();
    if let Err(e) = st.history.record(snaps) {
        tracing::warn!("market history write failed: {e}");
    }
}

fn quote(prices: &PriceIndex, key: &ItemKey) -> Option<NinjaPrice> {
    prices.find_price_by_query(&DbQuery { ns: key.ns().into(), name: key.name.clone(), variant: key.variant.clone() })
}

impl MarketState {
    /// Drop caches derived from the price index when it reloads or the league changes.
    fn sync_stamp(&mut self, env: &Env<'_>) {
        let stamp = (env.prices.last_update(), env.league.map(str::to_owned));
        if self.stamp.as_ref() != Some(&stamp) {
            self.stamp = Some(stamp);
            self.quotes.clear();
            self.type_meta.clear();
        }
    }

    fn quote(&mut self, prices: &PriceIndex, key: &ItemKey) -> Option<NinjaPrice> {
        self.quotes.entry(key.clone()).or_insert_with(|| quote(prices, key)).clone()
    }

    fn type_meta(&mut self, prices: &PriceIndex, type_: &str) -> (Option<String>, &'static str) {
        if let Some(m) = self.type_meta.get(type_) {
            return m.clone();
        }
        let m = prices.overview(type_).map(|o| (o.source, o.url)).unwrap_or((None, ""));
        self.type_meta.insert(type_.to_owned(), m.clone());
        m
    }

    fn open_history(&mut self, key: ItemKey) {
        self.selected = Some(key);
        self.tab = Tab::History;
    }
}

/// The widget's content (tabs). Rendered inside the chrome by [`show`].
pub fn body(ui: &mut Ui, env: &Env<'_>, mw: &mut MarketWidget, st: &mut MarketState) -> Outcome {
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut out = Outcome::default();
    st.sync_stamp(env);
    ui.set_width(PANEL_WIDTH);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{}  Market", icons::STOREFRONT)).font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let league = env.league.unwrap_or("no league");
            ui.label(RichText::new(league).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant));
            if let Some(rate) = env.prices.xchg_rate() {
                rpt_m3::tonal_badge(ui, &format!("1 div = {} c", data::fmt_chaos(rate)), c.surface_container_highest, c.on_surface_variant);
            }
        });
    });
    ui.add_space(4.0);
    ui.push_id("market-tabs", |ui| {
        rpt_m3::segmented(
            ui,
            &mut st.tab,
            &[(Tab::Economy, "Economy"), (Tab::Favorites, "Favorites"), (Tab::Saved, "Saved searches"), (Tab::History, "History")],
        );
    });
    ui.add_space(6.0);
    if env.prices.initial_loading() {
        rpt_m3::linear_progress(ui, None);
        ui.add_space(4.0);
    }
    match st.tab {
        Tab::Economy => economy_tab(ui, env, mw, st, &mut out),
        Tab::Favorites => favorites_tab(ui, env, mw, st, &mut out),
        Tab::Saved => saved_tab(ui, mw, st, &mut out),
        Tab::History => history_tab(ui, env, mw, st, &mut out),
    }
    out
}

fn empty_state(ui: &mut Ui, icon: &str, title: &str, body: &str) {
    let th = theme(ui.ctx());
    let c = th.colors;
    ui.vertical_centered(|ui| {
        ui.add_space(36.0);
        ui.label(rpt_m3::icon_text(icon, 40.0).color(c.on_surface_variant));
        ui.add_space(8.0);
        ui.label(RichText::new(title).font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
        ui.label(RichText::new(body).font(th.font(TypeRole::BodyMedium)).color(c.on_surface_variant));
        ui.add_space(36.0);
    });
}

// ---------------------------------------------------------------------------
// Row pieces

/// A full-width clickable row; children added in `add` sit on top of it (so
/// their buttons win clicks). Returns the row response.
fn clickable_row(ui: &mut Ui, highlight: Option<egui::Color32>, add: impl FnOnce(&mut Ui)) -> egui::Response {
    let c = theme(ui.ctx()).colors;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    if let Some(bg) = highlight {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(rpt_m3::shape::SMALL), bg);
    }
    if resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(rpt_m3::shape::SMALL), with_alpha(c.on_surface, 0.06));
    }
    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(4.0, 0.0))).layout(Layout::left_to_right(Align::Center)), add);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn cell<R>(ui: &mut Ui, width: f32, right: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = if right { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
    ui.allocate_ui_with_layout(vec2(width, ROW_H), layout, |ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        add(ui)
    })
    .inner
}

fn name_cell(ui: &mut Ui, width: f32, name: &str, variant: Option<&str>) {
    let th = theme(ui.ctx());
    cell(ui, width, false, |ui| {
        let mut job = egui::text::LayoutJob::default();
        RichText::new(name).font(th.font(TypeRole::BodyMedium)).color(th.colors.on_surface).append_to(&mut job, ui.style(), egui::FontSelection::Default, Align::Center);
        if let Some(v) = variant {
            RichText::new(format!("  {v}")).font(th.font(TypeRole::BodySmall)).color(th.colors.on_surface_variant).append_to(&mut job, ui.style(), egui::FontSelection::Default, Align::Center);
        }
        ui.add(egui::Label::new(job).truncate().selectable(false));
    });
}

fn price_cell(ui: &mut Ui, width: f32, chaos: Option<f64>, xchg: Option<f64>) {
    let th = theme(ui.ctx());
    cell(ui, width, true, |ui| {
        let Some(chaos) = chaos else {
            ui.label(RichText::new("—").color(th.colors.on_surface_variant));
            return;
        };
        if let Some(d) = data::divine_text(chaos, xchg) {
            ui.label(RichText::new(format!("{d} div")).font(th.font(TypeRole::LabelSmall)).color(th.colors.tertiary));
        }
        ui.label(RichText::new(format!("{} c", data::fmt_chaos(chaos))).font(th.font(TypeRole::LabelLarge)).color(th.colors.on_surface));
    });
}

fn change_badge(ui: &mut Ui, change: Option<f64>) {
    let c = theme(ui.ctx()).colors;
    match change {
        Some(v) if v >= 0.5 => rpt_m3::tonal_badge(ui, &format!("{} {}", icons::TRENDING_UP, data::fmt_pct(v)), c.success_container, c.on_success_container),
        Some(v) if v <= -0.5 => rpt_m3::tonal_badge(ui, &format!("{} {}", icons::TRENDING_DOWN, data::fmt_pct(v)), c.error_container, c.on_error_container),
        Some(v) => rpt_m3::tonal_badge(ui, &data::fmt_pct(v), c.surface_container_highest, c.on_surface_variant),
        None => rpt_m3::tonal_badge(ui, "n/a", c.surface_container_highest, c.on_surface_variant),
    };
}

fn spark(ui: &mut Ui, graph: &[Option<f64>], change: Option<f64>) {
    let c = theme(ui.ctx()).colors;
    let pts: Vec<f64> = graph.iter().flatten().copied().collect();
    let (min, max) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    let line = match change {
        Some(v) if v >= 0.5 => c.success,
        Some(v) if v <= -0.5 => c.error,
        _ => c.outline,
    };
    rpt_m3::sparkline(ui, &pts, min, max, vec2(76.0, 22.0), line, with_alpha(line, 0.15));
}

fn star_button(ui: &mut Ui, starred: bool, enabled: bool) -> bool {
    let tip = if starred { "Remove from favorites" } else { "Add to favorites" };
    ui.add(IconButton::new(if starred { icons::STAR } else { STAR_BORDER }).selected(starred).size(28.0).enabled(enabled).tooltip(tip)).clicked()
}

// ---------------------------------------------------------------------------
// Economy

fn ensure_econ(st: &mut MarketState, prices: &PriceIndex) {
    let updated = prices.last_update();
    let fresh = st.econ.as_ref().is_some_and(|e| e.type_ == st.category && e.updated == updated);
    if fresh {
        return;
    }
    let o = prices.overview(&st.category);
    let (url, source, rows) = match o {
        Some(o) => (o.url, o.source, o.lines.into_iter().map(EconRow::new).collect()),
        None => ("", None, vec![]),
    };
    st.type_meta.insert(st.category.clone(), (source, url));
    st.econ = Some(EconCache { type_: st.category.clone(), updated, url, rows, view_key: None, view: vec![] });
}

fn economy_tab(ui: &mut Ui, env: &Env<'_>, mw: &mut MarketWidget, st: &mut MarketState, out: &mut Outcome) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let types = env.prices.overview_types();
    if types.is_empty() {
        empty_state(ui, icons::QUERY_STATS, "No poe.ninja data yet", "Prices load for the selected league; this fills in once they arrive.");
        return;
    }
    if !types.iter().any(|(t, _)| *t == st.category) {
        st.category = types.iter().find(|(t, _)| *t == "Currency").unwrap_or(&types[0]).0.to_owned();
    }
    egui::ScrollArea::horizontal().id_salt("market-cats").max_height(40.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            for (t, _) in &types {
                if ui.add(Chip::filter(data::category_label(t), st.category == *t)).clicked() {
                    st.category = (*t).to_owned();
                }
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.scope(|ui| {
            ui.set_width(260.0);
            rpt_m3::text_field(ui, &mut st.search, "Filter by name");
        });
        ui.push_id("market-sort", |ui| {
            rpt_m3::segmented(ui, &mut st.sort, &[(SortBy::Price, "Price"), (SortBy::Change, "7d change"), (SortBy::Movers, "Movers")]);
        });
        let (icon, tip) = if st.ascending { (icons::ARROW_UPWARD, "Ascending") } else { (icons::ARROW_DOWNWARD, "Descending") };
        if ui.add(IconButton::new(icon).size(32.0).tooltip(tip)).clicked() {
            st.ascending = !st.ascending;
        }
    });
    ensure_econ(st, env.prices);
    let Some(econ) = st.econ.as_mut() else { return };
    let key = (st.search.clone(), st.sort, !st.ascending);
    if econ.view_key.as_ref() != Some(&key) {
        econ.view = data::filter_sort(&econ.rows, &key.0, key.1, key.2);
        econ.view_key = Some(key);
    }
    ui.label(RichText::new(format!("{} of {} items · click a row for its price history", econ.view.len(), econ.rows.len())).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
    let xchg = env.prices.xchg_rate();
    let league = env.league;
    let mut action: Option<(ItemKey, RowAct)> = None;
    egui::ScrollArea::vertical().id_salt("market-econ").max_height(LIST_HEIGHT).auto_shrink([false, true]).show_rows(ui, ROW_H, econ.view.len(), |ui, range| {
        for &i in &econ.view[range] {
            let row = &econ.rows[i];
            let key = ItemKey::new(&econ.type_, &row.info);
            let starred = league.is_some_and(|l| data::is_favorite(&mw.favorites, l, &key));
            let mut act = RowAct::None;
            let resp = clickable_row(ui, None, |ui| {
                if star_button(ui, starred, league.is_some()) {
                    act = RowAct::Star;
                }
                name_cell(ui, 300.0, &row.info.name, row.info.variant.as_deref());
                price_cell(ui, 130.0, Some(row.info.chaos), xchg);
                ui.add_space(12.0);
                cell(ui, 92.0, false, |ui| change_badge(ui, row.change));
                spark(ui, &row.info.graph, row.change);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(IconButton::new(icons::OPEN_IN_NEW).size(28.0).tooltip("Open on poe.ninja")).clicked() {
                        act = RowAct::Ninja;
                    }
                });
            });
            if matches!(act, RowAct::None) && resp.clicked() {
                act = RowAct::History;
            }
            if !matches!(act, RowAct::None) {
                action = Some((key, act));
            }
        }
    });
    let url = econ.url;
    let chaos = action.as_ref().and_then(|(k, _)| {
        econ.rows.iter().find(|r| r.info.name == k.name && r.info.variant.as_deref().filter(|v| !v.is_empty()) == k.variant.as_deref()).map(|r| r.info.chaos)
    });
    if let Some((key, act)) = action {
        match act {
            RowAct::Star => {
                if let Some(l) = league {
                    let chaos = chaos.unwrap_or(0.0);
                    data::toggle_favorite(&mut mw.favorites, l, &key, chaos, data::now_secs());
                    st.snapshot_key = None;
                    out.changed = true;
                }
            }
            RowAct::Ninja => out.open_urls.push(key.ninja_url(league.unwrap_or("Standard"), url)),
            RowAct::History => st.open_history(key),
            RowAct::None | RowAct::Changed => {}
        }
    }
}

enum RowAct {
    None,
    Star,
    Ninja,
    History,
    /// a persisted field was edited inline
    Changed,
}

// ---------------------------------------------------------------------------
// Favorites

fn favorites_tab(ui: &mut Ui, env: &Env<'_>, mw: &mut MarketWidget, st: &mut MarketState, out: &mut Outcome) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let Some(league) = env.league else {
        rpt_m3::banner(ui, icons::INFO, "Select a league in Settings to see its favorites.", false);
        return;
    };
    let idx: Vec<usize> = (0..mw.favorites.len()).filter(|&i| mw.favorites[i].league == league).collect();
    if idx.is_empty() {
        empty_state(ui, STAR_BORDER, "No favorites yet", "Star items in the Economy tab to watch their prices here.");
        return;
    }
    ui.label(
        RichText::new("Price now · change since starred · 7-day change. Set an alert % to highlight big moves; prices are snapshotted daily for the History chart.")
            .font(th.font(TypeRole::LabelSmall))
            .color(c.on_surface_variant),
    );
    let xchg = env.prices.xchg_rate();
    let mut remove: Option<usize> = None;
    let mut history: Option<ItemKey> = None;
    let mut ninja: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("market-favs").max_height(LIST_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
        for &i in &idx {
            let key = mw.favorites[i].key.clone();
            let q = st.quote(env.prices, &key);
            let now = q.as_ref().map(|q| q.chaos);
            let fav = &mut mw.favorites[i];
            let since = now.and_then(|n| fav.change_since_added(n));
            let alert = now.is_some_and(|n| fav.alert(n));
            let change7 = q.as_ref().and_then(|q| data::change_7d(&q.graph));
            let mut act = RowAct::None;
            let resp = clickable_row(ui, alert.then(|| with_alpha(c.tertiary_container, 0.7)), |ui| {
                if star_button(ui, true, true) {
                    act = RowAct::Star;
                }
                name_cell(ui, 230.0, &key.name, key.variant.as_deref());
                price_cell(ui, 120.0, now, xchg);
                ui.add_space(8.0);
                cell(ui, 112.0, false, |ui| {
                    let tip = format!("Starred at {} c", data::fmt_chaos(fav.added_chaos));
                    match since {
                        Some(v) if alert => rpt_m3::tonal_badge(ui, &format!("{} {}", NOTIFICATIONS_ACTIVE, data::fmt_pct(v)), c.tertiary, c.on_tertiary),
                        Some(v) => rpt_m3::tonal_badge(ui, &format!("{} {}", icons::HISTORY, data::fmt_pct(v)), c.surface_container_highest, c.on_surface_variant),
                        None => rpt_m3::tonal_badge(ui, "—", c.surface_container_highest, c.on_surface_variant),
                    }
                    .on_hover_text(tip);
                });
                cell(ui, 92.0, false, |ui| change_badge(ui, change7));
                spark(ui, q.as_ref().map(|q| q.graph.as_slice()).unwrap_or(&[]), change7);
                ui.add_space(6.0);
                let r = rpt_m3::optional_number_field(ui, ui.make_persistent_id(("market-alert", i)), &mut fav.alert_pct, "alert %", 44.0)
                    .on_hover_text("Highlight when the price moved more than this many percent since starred");
                if r.changed() {
                    act = RowAct::Changed;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(IconButton::new(icons::OPEN_IN_NEW).size(28.0).tooltip("Open on poe.ninja").enabled(q.is_some())).clicked() {
                        act = RowAct::Ninja;
                    }
                });
            });
            if matches!(act, RowAct::None) && resp.clicked() {
                act = RowAct::History;
            }
            match act {
                RowAct::Star => remove = Some(i),
                RowAct::History => history = Some(key),
                RowAct::Ninja => ninja = q.map(|q| q.url),
                RowAct::Changed => out.changed = true,
                RowAct::None => {}
            }
        }
    });
    if let Some(i) = remove {
        mw.favorites.remove(i);
        out.changed = true;
    }
    if let Some(k) = history {
        st.open_history(k);
    }
    if let Some(u) = ninja {
        out.open_urls.push(u);
    }
}

// ---------------------------------------------------------------------------
// Saved searches

fn saved_tab(ui: &mut Ui, mw: &mut MarketWidget, st: &mut MarketState, out: &mut Outcome) {
    let th = theme(ui.ctx());
    let c = th.colors;
    rpt_m3::card(ui, rpt_m3::CardKind::Filled).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Save a trade search").font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.scope(|ui| {
                ui.set_width(200.0);
                rpt_m3::text_field(ui, &mut st.new_name, "Name (optional)");
            });
            ui.scope(|ui| {
                ui.set_width(ui.available_width() - 90.0);
                rpt_m3::text_field(ui, &mut st.new_url, "https://www.pathofexile.com/trade/search/<league>/<id>");
            });
            if ui.add(Button::filled("Save").icon(icons::ADD).small()).clicked() {
                match data::upsert_search(&mut mw.saved_searches, &st.new_name, &st.new_url, st.new_notes.trim()) {
                    Ok(_) => {
                        st.new_name.clear();
                        st.new_url.clear();
                        st.new_notes.clear();
                        st.form_error = None;
                        out.changed = true;
                    }
                    Err(e) => st.form_error = Some(e.to_owned()),
                }
            }
        });
        ui.add_space(4.0);
        rpt_m3::text_field(ui, &mut st.new_notes, "Notes (optional)");
        if let Some(e) = &st.form_error {
            ui.label(RichText::new(format!("{}  {e}", icons::ERROR)).font(th.font(TypeRole::BodySmall)).color(c.error));
        }
    });
    ui.add_space(8.0);
    if mw.saved_searches.is_empty() {
        empty_state(ui, icons::MANAGE_SEARCH, "No saved searches", "Paste a pathofexile.com trade link above to keep it here.");
        return;
    }
    enum Act {
        Open(String),
        Move(usize, i32),
        Edit(usize),
        SaveEdit(usize),
        CancelEdit,
        AskDelete(usize),
        Delete(usize),
    }
    let mut act: Option<Act> = None;
    let n = mw.saved_searches.len();
    egui::ScrollArea::vertical().id_salt("market-saved").max_height(LIST_HEIGHT - 110.0).auto_shrink([false, true]).show(ui, |ui| {
        for (i, s) in mw.saved_searches.iter().enumerate() {
            ui.push_id(("saved", i), |ui| {
                rpt_m3::card(ui, rpt_m3::CardKind::Outlined).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if st.editing == Some(i) {
                        rpt_m3::text_field(ui, &mut st.edit_name, "Name");
                        ui.add_space(4.0);
                        rpt_m3::text_field(ui, &mut st.edit_notes, "Notes");
                        ui.horizontal(|ui| {
                            if ui.add(Button::filled("Save").icon(icons::CHECK).small()).clicked() {
                                act = Some(Act::SaveEdit(i));
                            }
                            if ui.add(Button::text("Cancel").small()).clicked() {
                                act = Some(Act::CancelEdit);
                            }
                        });
                        return;
                    }
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_max_width(ui.available_width() - 200.0);
                            ui.label(RichText::new(&s.name).font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
                            ui.add(egui::Label::new(RichText::new(&s.url).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant)).truncate());
                            if !s.notes.is_empty() {
                                ui.label(RichText::new(&s.notes).font(th.font(TypeRole::BodySmall)).color(c.on_surface));
                            }
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if st.confirm_delete == Some(i) {
                                if ui.add(Button::filled("Delete").small()).clicked() {
                                    act = Some(Act::Delete(i));
                                }
                                if ui.add(Button::text("Keep").small()).clicked() {
                                    act = Some(Act::CancelEdit);
                                }
                                return;
                            }
                            if ui.add(IconButton::new(icons::DELETE).size(30.0).tooltip("Delete")).clicked() {
                                act = Some(Act::AskDelete(i));
                            }
                            if ui.add(IconButton::new(icons::EDIT).size(30.0).tooltip("Rename / edit notes")).clicked() {
                                act = Some(Act::Edit(i));
                            }
                            if ui.add(IconButton::new(icons::ARROW_DOWNWARD).size(30.0).enabled(i + 1 < n).tooltip("Move down")).clicked() {
                                act = Some(Act::Move(i, 1));
                            }
                            if ui.add(IconButton::new(icons::ARROW_UPWARD).size(30.0).enabled(i > 0).tooltip("Move up")).clicked() {
                                act = Some(Act::Move(i, -1));
                            }
                            if ui.add(IconButton::new(icons::OPEN_IN_NEW).kind(rpt_m3::IconButtonKind::Tonal).size(30.0).tooltip("Open in browser")).clicked() {
                                act = Some(Act::Open(s.url.clone()));
                            }
                        });
                    });
                });
            });
            ui.add_space(4.0);
        }
    });
    match act {
        Some(Act::Open(u)) => out.open_urls.push(u),
        Some(Act::Move(i, d)) => {
            data::move_item(&mut mw.saved_searches, i, d);
            st.confirm_delete = None;
            out.changed = true;
        }
        Some(Act::Edit(i)) => {
            st.editing = Some(i);
            st.edit_name = mw.saved_searches[i].name.clone();
            st.edit_notes = mw.saved_searches[i].notes.clone();
        }
        Some(Act::SaveEdit(i)) => {
            let s = &mut mw.saved_searches[i];
            if !st.edit_name.trim().is_empty() {
                s.name = st.edit_name.trim().to_owned();
            }
            s.notes = st.edit_notes.trim().to_owned();
            st.editing = None;
            out.changed = true;
        }
        Some(Act::CancelEdit) => {
            st.editing = None;
            st.confirm_delete = None;
        }
        Some(Act::AskDelete(i)) => st.confirm_delete = Some(i),
        Some(Act::Delete(i)) => {
            mw.saved_searches.remove(i);
            st.confirm_delete = None;
            st.editing = None;
            out.changed = true;
        }
        None => {}
    }
}

// ---------------------------------------------------------------------------
// History

/// Merged daily chaos series for `key`: poe.ninja exchange history, local
/// snapshots, then the 7-day sparkline (earlier sources win on the same day).
fn history_series(env: &Env<'_>, st: &mut MarketState, league: &str, key: &ItemKey, q: Option<&NinjaPrice>) -> (Vec<DayPoint>, Vec<String>, Option<String>) {
    let (source, _) = st.type_meta(env.prices, &key.type_);
    let hk = (league.to_owned(), key.clone());
    let mut status = None;
    if data::has_exchange_history(source.as_deref()) {
        if let (Some(sv), false) = (env.services, st.ninja_hist.contains_key(&hk)) {
            let fut = data::fetch_exchange_history(sv.trade.http().clone(), league.to_owned(), key.type_.clone(), key.details_id());
            st.ninja_hist.insert(hk.clone(), HistEntry::Loading(sv.spawn(fut)));
        }
        if let Some(HistEntry::Loading(task)) = st.ninja_hist.get_mut(&hk)
            && let Some(r) = task.poll_take()
        {
            st.ninja_hist.insert(hk.clone(), HistEntry::Done(r));
        }
    }
    let ninja: Vec<DayPoint> = match st.ninja_hist.get(&hk) {
        Some(HistEntry::Done(Ok(v))) => v.clone(),
        Some(HistEntry::Done(Err(e))) => {
            status = Some(format!("poe.ninja history unavailable: {e}"));
            vec![]
        }
        Some(HistEntry::Loading(_)) => {
            status = Some("Loading poe.ninja history…".into());
            vec![]
        }
        None => vec![],
    };
    let local = st.history.series(league, key);
    let spark = q.map(|q| data::prices_from_graph(q.chaos, &q.graph, data::today())).unwrap_or_default();
    let mut sources = Vec::new();
    if !ninja.is_empty() {
        sources.push(format!("poe.ninja daily history ({} days)", ninja.len()));
    }
    if !local.is_empty() {
        sources.push(format!("local snapshots ({} days)", local.len()));
    }
    if !spark.is_empty() {
        sources.push("poe.ninja 7-day trend".into());
    }
    (data::merge_daily(&[&ninja, &local, &spark]), sources, status)
}

fn history_tab(ui: &mut Ui, env: &Env<'_>, mw: &mut MarketWidget, st: &mut MarketState, out: &mut Outcome) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let favs: Vec<ItemKey> = env.league.map(|l| mw.favorites.iter().filter(|f| f.league == l).map(|f| f.key.clone()).collect()).unwrap_or_default();
    if !favs.is_empty() {
        egui::ScrollArea::horizontal().id_salt("market-hist-favs").max_height(40.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                for k in &favs {
                    if ui.add(Chip::filter(k.label(), st.selected.as_ref() == Some(k)).leading(icons::STAR)).clicked() {
                        st.selected = Some(k.clone());
                    }
                }
            });
        });
        ui.add_space(4.0);
    }
    let Some(key) = st.selected.clone() else {
        empty_state(ui, icons::SHOW_CHART, "No item selected", "Click an item in Economy or Favorites to chart its price.");
        return;
    };
    let Some(league) = env.league else {
        rpt_m3::banner(ui, icons::INFO, "Select a league in Settings to load price history.", false);
        return;
    };
    let q = st.quote(env.prices, &key);
    let xchg = env.prices.xchg_rate();
    let starred = data::is_favorite(&mw.favorites, league, &key);
    ui.horizontal(|ui| {
        if star_button(ui, starred, true) {
            data::toggle_favorite(&mut mw.favorites, league, &key, q.as_ref().map(|q| q.chaos).unwrap_or(0.0), data::now_secs());
            st.snapshot_key = None;
            out.changed = true;
        }
        ui.vertical(|ui| {
            ui.label(RichText::new(&key.name).font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
            let mut sub = data::category_label(&key.type_);
            if let Some(v) = &key.variant {
                sub = format!("{sub} · {v}");
            }
            ui.label(RichText::new(sub).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(q) = &q {
                if ui.add(IconButton::new(icons::OPEN_IN_NEW).size(32.0).tooltip("Open on poe.ninja")).clicked() {
                    out.open_urls.push(q.url.clone());
                }
                change_badge(ui, data::change_7d(&q.graph));
                price_cell(ui, 130.0, Some(q.chaos), xchg);
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.push_id("market-range", |ui| {
            rpt_m3::segmented(ui, &mut st.range, &[(Range::D7, "7 days"), (Range::D30, "30 days"), (Range::All, "All")]);
        });
        ui.add_space(12.0);
        if xchg.is_some() {
            ui.push_id("market-unit", |ui| {
                rpt_m3::segmented(ui, &mut st.unit, &[(Unit::Chaos, "Chaos"), (Unit::Divine, "Divine")]);
            });
        }
    });
    ui.add_space(6.0);
    let (series, sources, status) = history_series(env, st, league, &key, q.as_ref());
    let from = match st.range {
        Range::D7 => data::today() - 7,
        Range::D30 => data::today() - 30,
        Range::All => i64::MIN,
    };
    let divine = st.unit == Unit::Divine && xchg.is_some();
    let rate = xchg.unwrap_or(1.0);
    let points: Vec<[f64; 2]> = series.iter().filter(|p| p.0 >= from).map(|&(d, v)| [d as f64, if divine { v / rate } else { v }]).collect();
    let opts = ChartOpts {
        height: 280.0,
        y_suffix: if divine { " div".into() } else { "c".into() },
        empty_text: if status.as_deref().is_some_and(|s| s.starts_with("Loading")) { "Loading…".into() } else { "No price history for this item yet".into() },
        ..Default::default()
    };
    line_chart(ui, &[Series { name: key.label(), points: points.clone(), color: None }], &opts);
    ui.add_space(4.0);
    if points.len() >= 2 {
        let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        let first = points[0][1];
        let last = points[points.len() - 1][1];
        let unit = if divine { " div" } else { " c" };
        let f = |v: f64| if divine { rpt_trade::prices::display_rounding(v, false) } else { data::fmt_chaos(v) };
        ui.horizontal(|ui| {
            for (label, value) in [("Low", f(lo)), ("High", f(hi))] {
                rpt_m3::tonal_badge(ui, &format!("{label} {value}{unit}"), c.surface_container_highest, c.on_surface_variant);
            }
            if first > 0.0 {
                change_badge(ui, Some((last / first - 1.0) * 100.0));
            }
        });
    }
    let mut note = if sources.is_empty() { String::new() } else { format!("Sources: {}", sources.join(" · ")) };
    if divine {
        note.push_str(" · divine values use today's rate");
    }
    if !note.is_empty() {
        ui.label(RichText::new(note).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
    }
    if let Some(s) = status {
        ui.label(RichText::new(s).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
    }
    if !starred {
        ui.label(RichText::new("Star this item to record daily local snapshots beyond poe.ninja's window.").font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use rpt_trade::{Leagues, TradeClient, TradeConfig};

    fn offline_prices(blob: Option<&str>) -> PriceIndex {
        let client = Arc::new(TradeClient::new(TradeConfig::default()).unwrap());
        let leagues = Arc::new(Leagues::new(client.clone()));
        let idx = PriceIndex::new(client, leagues);
        if let Some(b) = blob {
            idx.load_from_blob(b);
        }
        idx
    }

    fn render(env: &Env<'_>, mw: &mut MarketWidget, st: &mut MarketState, frames: usize) -> Outcome {
        let ctx = egui::Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 900.0));
        let mut last = Outcome::default();
        for _ in 0..frames {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                last = body(ui, env, mw, st);
            });
            out.textures_delta.clear();
        }
        last
    }

    const TABS: [Tab; 4] = [Tab::Economy, Tab::Favorites, Tab::Saved, Tab::History];

    /// Every tab renders headless with no data, no league and no services.
    #[test]
    fn renders_tabs_without_data() {
        let prices = offline_prices(None);
        let env = Env { prices: &prices, league: None, services: None };
        let mut mw = MarketWidget::default();
        let mut st = MarketState::default();
        for tab in TABS {
            st.tab = tab;
            let out = render(&env, &mut mw, &mut st, 2);
            assert!(!out.changed && out.open_urls.is_empty());
            assert_eq!(st.tab, tab);
        }
        // History with a selection but nothing loaded
        st.selected = Some(ItemKey { type_: "Currency".into(), name: "Divine Orb".into(), variant: None });
        st.tab = Tab::History;
        render(&env, &mut mw, &mut st, 2);
        let env = Env { prices: &prices, league: Some("Covens"), services: None };
        render(&env, &mut mw, &mut st, 2);
    }

    /// With the fixture blob, favorites and saved searches, every tab renders.
    #[test]
    fn renders_tabs_with_fixture_data() {
        let prices = offline_prices(Some(include_str!("fixtures/dense_overviews.json")));
        let env = Env { prices: &prices, league: Some("Covens"), services: None };
        let mut mw = MarketWidget::default();
        let divine = ItemKey { type_: "Currency".into(), name: "Divine Orb".into(), variant: None };
        data::toggle_favorite(&mut mw.favorites, "Covens", &divine, 100.0, 0);
        mw.favorites[0].alert_pct = Some(50.0);
        data::upsert_search(&mut mw.saved_searches, "Mirror", "https://www.pathofexile.com/trade/search/Covens/abc", "notes").unwrap();
        let mut st = MarketState { history: HistoryStore::default(), ..Default::default() };
        st.history.records.push(Snapshot { day: data::today() - 20, league: "Covens".into(), key: divine.clone(), chaos: 50.0 });
        for tab in TABS {
            st.tab = tab;
            render(&env, &mut mw, &mut st, 2);
        }
        // economy built its cache for Currency
        let econ = st.econ.as_ref().unwrap();
        assert_eq!(econ.type_, "Currency");
        assert_eq!(econ.rows.len(), 5);
        assert_eq!(econ.rows[econ.view[0]].info.name, "Mirror of Kalandra");
        st.sort = SortBy::Movers;
        st.tab = Tab::Economy;
        render(&env, &mut mw, &mut st, 1);
        // History: merged local snapshot (20 days ago) + 7-day trend
        st.selected = Some(divine.clone());
        let q = st.quote(&prices, &divine);
        let (series, sources, status) = history_series(&env, &mut st, "Covens", &divine, q.as_ref());
        assert_eq!(series.len(), 8);
        assert_eq!(series[0], (data::today() - 20, 50.0));
        let last = series.last().unwrap();
        assert_eq!(last.0, data::today());
        assert!((last.1 - 215.4).abs() < 1e-9);
        assert_eq!(sources.len(), 2);
        assert!(status.is_none(), "no services → no fetch");
        for unit in [Unit::Chaos, Unit::Divine] {
            st.unit = unit;
            st.tab = Tab::History;
            render(&env, &mut mw, &mut st, 2);
        }
        // the history file was never written (no path)
        assert!(st.history.path.is_none());
    }

    #[test]
    fn save_search_api() {
        let mut cfg = Config::default();
        assert!(save_search(&mut cfg, "x", "https://example.com/trade/search/a/b").is_err());
        save_search(&mut cfg, "", "https://www.pathofexile.com/trade/exchange/Covens/q1").unwrap();
        save_search(&mut cfg, "Bulk divines", "https://www.pathofexile.com/trade/exchange/Covens/q1").unwrap();
        let mw = market_widget_mut(&mut cfg).unwrap();
        assert_eq!(mw.saved_searches.len(), 1);
        assert_eq!(mw.saved_searches[0].name, "Bulk divines");
    }
}
