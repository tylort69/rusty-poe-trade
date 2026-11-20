//! Stash valuation, net-worth history, dust calculator and bulk-sell list
//! (Wealthy Exile-style, entirely local: pathofexile.com for the stash, poe.ninja for prices).

mod bulk;
mod history;
mod pricing;

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use egui::{Align, Color32, Layout, RichText, Ui, vec2};
use parking_lot::Mutex;
use rpt_m3::{Button, CardKind, IconButton, TypeRole, icons, theme};
use rpt_stash::model::StashTab;
use rpt_stash::{StashError, StashTabInfo};
use rpt_trade::PriceIndex;
use serde::{Deserialize, Serialize};

use super::{WCtx, chrome};
use crate::account::{self, Status};
use crate::config::{Anchor, Config, WidgetKind};
use crate::host::HostCommand;
use history::{Snapshot, fmt_int};
use pricing::{RowKind, UniqueDb, Valuation};

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-wealth"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WealthWidget {
    pub anchor: Anchor,
    /// League id → stash tab ids included in the valuation.
    pub tabs: BTreeMap<String, Vec<String>>,
    /// Re-value automatically every N minutes while the widget is open (0 = off).
    pub auto_snapshot_minutes: u32,
    /// Bulk-sell asking price in % of poe.ninja.
    pub bulk_percent: f64,
    /// Price notes use `~b/o` instead of `~price`.
    pub note_buyout: bool,
}

impl Default for WealthWidget {
    fn default() -> Self {
        Self {
            anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 },
            tabs: BTreeMap::new(),
            auto_snapshot_minutes: 0,
            bulk_percent: 90.0,
            note_buyout: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    #[default]
    Valuation,
    History,
    Dust,
    Bulk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DustSort {
    Name,
    Chaos,
    Dust,
    #[default]
    DustPerChaos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrKind {
    SignIn,
    RateLimited,
    Other,
}

#[derive(Debug, Clone)]
struct JobError {
    kind: ErrKind,
    msg: String,
}

impl From<&StashError> for JobError {
    fn from(e: &StashError) -> Self {
        let kind = match e {
            StashError::Unauthorized | StashError::NotSignedIn | StashError::AccountNameUnknown => ErrKind::SignIn,
            StashError::RateLimited { .. } => ErrKind::RateLimited,
            _ => ErrKind::Other,
        };
        Self { kind, msg: e.to_string() }
    }
}

/// Background fetch + pricing, shared with the account runtime.
#[derive(Default)]
struct Job {
    league: String,
    total: usize,
    done: usize,
    current: String,
    pricing: bool,
    tabs: Vec<StashTab>,
    error: Option<JobError>,
    valuation: Option<Valuation>,
    finished: bool,
    /// Re-price of already fetched tabs (no new snapshot unless none was written yet).
    reprice: bool,
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct WealthState {
    view: View,
    job: Option<Arc<Mutex<Job>>>,
    cancel: Arc<AtomicBool>,
    league: String,
    tabs: Arc<Vec<StashTab>>,
    valuation: Option<Valuation>,
    error: Option<JobError>,
    partial: bool,
    snapshot_written: bool,
    last_run: Option<Instant>,
    tabs_requested: Option<String>,
    history: Option<(String, Vec<Snapshot>)>,
    history_divine: bool,
    history_error: Option<String>,
    dust_sort: DustSort,
    dust_asc: bool,
    bulk_selected: HashSet<String>,
    prices_queued: bool,
}

/// What the body asks the caller to do (keeps the body testable without `Services`).
#[derive(Default)]
struct Out {
    save: bool,
    open_url: Option<String>,
}

fn widget_cfg(cfg: &mut Config, wm_id: u32) -> Option<&mut WealthWidget> {
    cfg.widgets.iter_mut().find(|x| x.wm_id == wm_id).and_then(|x| match &mut x.kind {
        WidgetKind::Wealth(w) => Some(w),
        _ => None,
    })
}

fn unique_db() -> &'static UniqueDb {
    static DB: OnceLock<UniqueDb> = OnceLock::new();
    DB.get_or_init(|| UniqueDb::from_data(&rpt_data::data()))
}

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, st: &mut WealthState, wm_id: u32) {
    if !st.prices_queued {
        st.prices_queued = true;
        w.services.queue_prices_fetch();
    }
    let prices = w.services.prices.clone();
    let screen = w.screen;
    let mut out = Out::default();
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, w| {
        let width = (screen.width() - 48.0).clamp(360.0, 980.0);
        let height = (screen.height() - 96.0).clamp(320.0, 720.0);
        ui.set_width(width);
        ui.set_max_height(height);
        out = body(ui, w.cfg, &prices, st, wm_id);
    });
    chrome::apply(w, wm_id, &r);
    if out.save {
        w.save_requested = true;
    }
    if let Some(url) = out.open_url {
        w.host.push(HostCommand::OpenUrl(url));
    }
}

fn body(ui: &mut Ui, cfg: &mut Config, prices: &Arc<PriceIndex>, st: &mut WealthState, wm_id: u32) -> Out {
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut out = Out::default();
    let acct = account::get(ui.ctx());
    #[cfg(not(test))]
    acct.ensure_restored(cfg); // never touch the keychain from tests
    let acct_state = acct.state();
    let league = cfg.league_id.clone().unwrap_or_default();

    poll_job(st, prices);

    rpt_m3::top_app_bar(ui, "Wealth", |ui| {
        if !league.is_empty() {
            rpt_m3::tonal_badge(ui, &league, c.secondary_container, c.on_secondary_container);
        }
    });

    match &acct_state.status {
        Status::SignedIn(_) => {}
        Status::Working(msg) => {
            ui.horizontal(|ui| {
                rpt_m3::circular_progress(ui, 20.0);
                ui.label(msg.as_str());
            });
            return out;
        }
        other => {
            sign_in_prompt(ui, cfg, if let Status::Failed(e) = other { Some(e.as_str()) } else { None });
            return out;
        }
    }
    if league.is_empty() {
        rpt_m3::banner(ui, icons::INFO, "Pick a league in Settings → General first.", false);
        return out;
    }
    if st.league != league {
        // League switched: results belong to the old league.
        st.cancel.store(true, Ordering::SeqCst);
        *st = WealthState { view: st.view, prices_queued: st.prices_queued, league: league.clone(), ..Default::default() };
    }

    // Auto snapshot while open.
    let auto = widget_cfg(cfg, wm_id).map(|x| x.auto_snapshot_minutes).unwrap_or(0);
    if auto > 0 && st.job.is_none() && st.last_run.is_none_or(|t| t.elapsed() >= Duration::from_secs(auto as u64 * 60)) {
        let selected = selected_tabs(cfg, wm_id, &acct_state, &league);
        if !selected.is_empty() {
            start_fetch(st, acct, prices, &league, selected);
        }
    }
    if auto > 0 {
        ui.ctx().request_repaint_after(Duration::from_secs(30));
    }

    ui.horizontal(|ui| {
        let opts = [(View::Valuation, "Valuation"), (View::History, "History"), (View::Dust, "Dust"), (View::Bulk, "Bulk sell")];
        rpt_m3::segmented(ui, &mut st.view, &opts);
    });
    ui.add_space(8.0);

    if let Some(e) = &st.error {
        let msg = match e.kind {
            ErrKind::SignIn => format!("{} Sign in again under Settings → Account.", e.msg),
            ErrKind::RateLimited => format!("{} Already loaded tabs are kept; try again later.", e.msg),
            ErrKind::Other => e.msg.clone(),
        };
        rpt_m3::banner(ui, icons::ERROR, msg, true);
        if e.kind == ErrKind::SignIn && ui.add(Button::text("Open Account settings").icon(icons::ACCOUNT_CIRCLE)).clicked() {
            crate::widgets::settings::open_account(cfg);
            out.save = true;
        }
        ui.add_space(4.0);
    }
    if prices.xchg_rate().is_none() {
        rpt_m3::banner(ui, icons::SCHEDULE, "poe.ninja prices are still loading (or unavailable for this league). Values will be incomplete.", false);
        ui.add_space(4.0);
    }

    egui::ScrollArea::vertical().id_salt(("wealth-view", st.view as u8)).auto_shrink([false, true]).show(ui, |ui| {
        ui.set_width(ui.available_width() - 8.0);
        match st.view {
            View::Valuation => valuation_view(ui, cfg, prices, st, wm_id, &acct_state, &league, &mut out),
            View::History => history_view(ui, cfg, st, wm_id, &league, &mut out),
            View::Dust => dust_view(ui, st),
            View::Bulk => bulk_view(ui, cfg, prices, st, wm_id, &league, &mut out),
        }
    });
    out
}

fn sign_in_prompt(ui: &mut Ui, cfg: &mut Config, failed: Option<&str>) {
    let th = theme(ui.ctx());
    let c = th.colors;
    rpt_m3::card(ui, CardKind::Filled).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(rpt_m3::icon_text(icons::ACCOUNT_CIRCLE, 40.0).color(c.primary));
            ui.vertical(|ui| {
                ui.label(RichText::new("Sign in to value your stash").font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
                ui.add(
                    egui::Label::new(
                        RichText::new("Wealth reads your stash tabs from pathofexile.com and prices them with poe.ninja. Nothing is sent anywhere else.")
                            .color(c.on_surface_variant),
                    )
                    .wrap(),
                );
            });
        });
        if let Some(e) = failed {
            ui.add_space(4.0);
            rpt_m3::banner(ui, icons::ERROR, e, true);
        }
        ui.add_space(8.0);
        if ui.add(Button::filled("Open Account settings").icon(icons::ACCOUNT_CIRCLE)).clicked() {
            crate::widgets::settings::open_account(cfg);
        }
    });
}

// ---------------------------------------------------------------------------
// Fetch / price job

fn selected_tabs(cfg: &mut Config, wm_id: u32, acct: &account::AccountState, league: &str) -> Vec<StashTabInfo> {
    let Some((l, tabs)) = &acct.tabs else { return Vec::new() };
    if l != league {
        return Vec::new();
    }
    let ids: HashSet<String> = widget_cfg(cfg, wm_id).and_then(|x| x.tabs.get(league)).map(|v| v.iter().cloned().collect()).unwrap_or_default();
    StashTabInfo::flatten(tabs).into_iter().filter(|t| ids.contains(&t.id)).cloned().collect()
}

fn start_fetch(st: &mut WealthState, acct: &account::Account, prices: &Arc<PriceIndex>, league: &str, tabs: Vec<StashTabInfo>) {
    st.cancel.store(true, Ordering::SeqCst);
    let cancel = Arc::new(AtomicBool::new(false));
    st.cancel = cancel.clone();
    st.last_run = Some(Instant::now());
    st.error = None;
    let job = Arc::new(Mutex::new(Job { league: league.to_owned(), total: tabs.len(), ..Default::default() }));
    st.job = Some(job.clone());
    let (client, prices, league) = (acct.client(), prices.clone(), league.to_owned());
    acct.spawn(async move {
        let mut fetched = Vec::new();
        for t in &tabs {
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            job.lock().current = t.name.clone();
            match client.get_tab(&league, t).await {
                Ok(tab) => {
                    fetched.push(tab);
                    job.lock().done += 1;
                }
                Err(e) => {
                    job.lock().error = Some(JobError::from(&e));
                    break;
                }
            }
        }
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        job.lock().pricing = true;
        if let Err(e) = prices.queue_prices_fetch().await {
            tracing::warn!("poe.ninja load failed: {e}");
        }
        let v = value(&fetched, &prices);
        let mut j = job.lock();
        j.tabs = fetched;
        j.valuation = Some(v);
        j.finished = true;
    });
}

fn start_reprice(st: &mut WealthState, acct: &account::Account, prices: &Arc<PriceIndex>) {
    let job = Arc::new(Mutex::new(Job { league: st.league.clone(), pricing: true, reprice: true, ..Default::default() }));
    st.job = Some(job.clone());
    let (tabs, prices) = (st.tabs.clone(), prices.clone());
    acct.spawn(async move {
        let _ = prices.queue_prices_fetch().await;
        let v = value(&tabs, &prices);
        let mut j = job.lock();
        j.tabs = tabs.to_vec();
        j.valuation = Some(v);
        j.finished = true;
    });
}

fn value(tabs: &[StashTab], prices: &PriceIndex) -> Valuation {
    pricing::value_tabs(tabs, unique_db(), prices.xchg_rate(), |q| {
        prices.find_price_by_query(q).map(|p| pricing::Found { chaos: p.chaos, url: Some(p.url) })
    })
}

fn poll_job(st: &mut WealthState, prices: &PriceIndex) {
    let Some(job) = st.job.clone() else { return };
    let mut j = job.lock();
    if !j.finished {
        return;
    }
    st.job = None;
    if j.league != st.league {
        return;
    }
    let reprice = j.reprice;
    st.error = j.error.take();
    st.partial = st.error.is_some();
    st.tabs = Arc::new(std::mem::take(&mut j.tabs));
    st.valuation = j.valuation.take();
    if !reprice {
        st.snapshot_written = false;
    }
    drop(j);
    // One snapshot per complete fetch, once prices are known.
    let Some(v) = &st.valuation else { return };
    if st.partial || st.snapshot_written || v.divine_chaos.is_none() || prices.xchg_rate().is_none() || st.tabs.is_empty() {
        return;
    }
    let mut per_tab = BTreeMap::new();
    for t in &v.tabs {
        *per_tab.entry(t.name.clone()).or_insert(0.0) += t.chaos;
    }
    let snap = Snapshot {
        ts: chrono::Utc::now().timestamp(),
        total_chaos: v.total_chaos,
        divine_chaos: v.divine_chaos.unwrap_or_default(),
        per_tab,
    };
    match history::append(&crate::store::data_dir(), &st.league, &snap) {
        Ok(()) => {
            st.snapshot_written = true;
            if let Some((l, h)) = &mut st.history {
                if *l == st.league {
                    h.push(snap);
                }
            }
        }
        Err(e) => st.history_error = Some(format!("Couldn't save the snapshot: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Views

fn chaos_text(v: f64) -> String {
    if v.abs() >= 100.0 { fmt_int(v) } else { format!("{v:.1}") }
}

fn div_text(chaos: f64, rate: Option<f64>) -> String {
    match rate.filter(|r| *r > 0.0) {
        Some(r) => format!("{:.2}", chaos / r),
        None => "—".into(),
    }
}

fn currency_icon(ui: &mut Ui, name: &str) {
    ui.add(egui::Image::new(crate::widgets::price_check::ui::bundled(name)).fit_to_exact_size(vec2(20.0, 20.0)));
}

#[allow(clippy::too_many_arguments)]
fn valuation_view(
    ui: &mut Ui,
    cfg: &mut Config,
    prices: &Arc<PriceIndex>,
    st: &mut WealthState,
    wm_id: u32,
    acct_state: &account::AccountState,
    league: &str,
    out: &mut Out,
) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let acct = account::get(ui.ctx());

    // ---- tab picker ------------------------------------------------------
    let have_tabs = acct_state.tabs.as_ref().is_some_and(|(l, _)| l == league);
    if !have_tabs && !acct_state.loading_tabs && acct_state.tabs_error.is_none() && st.tabs_requested.as_deref() != Some(league) {
        st.tabs_requested = Some(league.to_owned());
        acct.load_tabs(league.to_owned());
    }
    rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Stash tabs").font(th.font(TypeRole::TitleSmall)).color(c.primary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(IconButton::new(icons::REFRESH).size(32.0).tooltip("Reload tab list").enabled(!acct_state.loading_tabs)).clicked() {
                    acct.load_tabs(league.to_owned());
                }
                if have_tabs {
                    let all_ids: Vec<String> = acct_state.tabs.as_ref().map(|(_, t)| StashTabInfo::flatten(t).iter().map(|t| t.id.clone()).collect()).unwrap_or_default();
                    if ui.add(Button::text("None").small()).clicked() {
                        if let Some(x) = widget_cfg(cfg, wm_id) {
                            x.tabs.insert(league.to_owned(), Vec::new());
                            out.save = true;
                        }
                    }
                    if ui.add(Button::text("All").small()).clicked() {
                        if let Some(x) = widget_cfg(cfg, wm_id) {
                            x.tabs.insert(league.to_owned(), all_ids);
                            out.save = true;
                        }
                    }
                }
            });
        });
        if acct_state.loading_tabs {
            rpt_m3::linear_progress(ui, None);
        }
        if let Some(e) = &acct_state.tabs_error {
            rpt_m3::banner(ui, icons::ERROR, e.as_str(), true);
        }
        if let Some((_, tabs)) = acct_state.tabs.as_ref().filter(|(l, _)| l == league) {
            let mut selected: HashSet<String> = widget_cfg(cfg, wm_id).and_then(|x| x.tabs.get(league)).map(|v| v.iter().cloned().collect()).unwrap_or_default();
            let mut changed = false;
            egui::ScrollArea::vertical().id_salt("wealth-tabs").max_height(140.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for t in StashTabInfo::flatten(tabs) {
                        let on = selected.contains(&t.id);
                        let mut chip = rpt_m3::Chip::filter(t.name.as_str(), on);
                        if let Some([r, g, b]) = t.colour {
                            chip = chip.colors(Color32::from_rgb(r, g, b).gamma_multiply(0.55), Color32::WHITE);
                        }
                        if ui.add(chip).on_hover_text(&t.kind).clicked() {
                            if on {
                                selected.remove(&t.id);
                            } else {
                                selected.insert(t.id.clone());
                            }
                            changed = true;
                        }
                    }
                });
            });
            if changed {
                if let Some(x) = widget_cfg(cfg, wm_id) {
                    // keep tab-bar order
                    let order: Vec<String> = StashTabInfo::flatten(tabs).iter().filter(|t| selected.contains(&t.id)).map(|t| t.id.clone()).collect();
                    x.tabs.insert(league.to_owned(), order);
                    out.save = true;
                }
                selected.clear();
            }
        }
    });
    ui.add_space(8.0);

    // ---- actions / progress ---------------------------------------------
    let picked = selected_tabs(cfg, wm_id, acct_state, league);
    ui.horizontal(|ui| {
        let busy = st.job.is_some();
        let label = format!("Value {} tab{}", picked.len(), if picked.len() == 1 { "" } else { "s" });
        if ui.add(Button::filled(label).icon(icons::PRICE_CHECK).enabled(!busy && !picked.is_empty())).clicked() {
            start_fetch(st, acct, prices, league, picked.clone());
        }
        if st.valuation.is_some() && ui.add(Button::tonal("Re-price").icon(icons::REFRESH).enabled(!busy)).on_hover_text("Price the loaded items again with the latest poe.ninja data (no new requests to pathofexile.com)").clicked() {
            start_reprice(st, acct, prices);
        }
        if busy && ui.add(Button::text("Cancel").icon(icons::CLOSE)).clicked() {
            st.cancel.store(true, Ordering::SeqCst);
            st.job = None;
        }
    });
    if let Some(job) = &st.job {
        let j = job.lock();
        let (frac, text) = if j.pricing {
            (None, "Pricing items…".to_owned())
        } else {
            (Some(j.done as f32 / j.total.max(1) as f32), format!("Fetching {} ({} / {}) — requests are rate limited", j.current, j.done + 1, j.total))
        };
        drop(j);
        ui.add_space(4.0);
        rpt_m3::linear_progress(ui, frac);
        ui.label(RichText::new(text).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
    ui.add_space(8.0);

    let Some(v) = &st.valuation else {
        if st.job.is_none() {
            ui.label(RichText::new("Pick the tabs to include, then value them.").color(c.on_surface_variant));
        }
        return;
    };

    // ---- totals ----------------------------------------------------------
    let rate = v.divine_chaos;
    rpt_m3::card(ui, CardKind::Filled).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Net worth").font(th.font(TypeRole::LabelLarge)).color(c.on_surface_variant));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(chaos_text(v.total_chaos)).font(th.font(TypeRole::HeadlineSmall)).color(c.on_surface));
                    currency_icon(ui, "chaos.png");
                    ui.add_space(12.0);
                    ui.label(RichText::new(div_text(v.total_chaos, rate)).font(th.font(TypeRole::HeadlineSmall)).color(c.on_surface));
                    currency_icon(ui, "divine.png");
                });
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if v.unpriced > 0 {
                    rpt_m3::tonal_badge(ui, &format!("{} unpriced", v.unpriced), c.tertiary_container, c.on_tertiary_container)
                        .on_hover_text("Rares, plain gear and items poe.ninja doesn't list count as 0.");
                }
                if let Some(r) = rate {
                    rpt_m3::tonal_badge(ui, &format!("1 div = {r:.0}c"), c.secondary_container, c.on_secondary_container);
                }
                if st.partial {
                    rpt_m3::tonal_badge(ui, "partial", c.error_container, c.on_error_container);
                }
            });
        });
    });
    ui.add_space(8.0);

    // ---- per tab ---------------------------------------------------------
    rpt_m3::section_header(ui, "Per tab");
    egui::Grid::new("wealth-per-tab").num_columns(4).striped(true).spacing([16.0, 4.0]).show(ui, |ui| {
        for h in ["Tab", "Chaos", "Divine", "Items (unpriced)"] {
            ui.label(RichText::new(h).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant));
        }
        ui.end_row();
        let mut tabs: Vec<_> = v.tabs.iter().collect();
        tabs.sort_by(|a, b| b.chaos.total_cmp(&a.chaos));
        for t in tabs {
            ui.label(&t.name);
            ui.label(chaos_text(t.chaos));
            ui.label(div_text(t.chaos, rate));
            ui.label(format!("{} ({})", t.items, t.unpriced));
            ui.end_row();
        }
    });

    // ---- top items -------------------------------------------------------
    rpt_m3::section_header(ui, "Top items");
    egui::Grid::new("wealth-top").num_columns(5).striped(true).spacing([16.0, 4.0]).show(ui, |ui| {
        for h in ["Item", "Qty", "Unit (c)", "Total (c)", "Total (div)"] {
            ui.label(RichText::new(h).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant));
        }
        ui.end_row();
        for row in v.rows.iter().take(100) {
            ui.horizontal(|ui| {
                let name = ui.add(egui::Label::new(RichText::new(&row.name).color(c.on_surface)).sense(egui::Sense::click()));
                let name = match &row.detail {
                    Some(d) => name.on_hover_text(d),
                    None => name,
                };
                if let Some(url) = &row.url {
                    if name.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        out.open_url = Some(url.clone());
                    }
                }
                let tag = match row.kind {
                    RowKind::Unique => Some("unique"),
                    RowKind::Gem => Some("gem"),
                    RowKind::Map => Some("map"),
                    _ => None,
                };
                if let Some(tag) = tag {
                    rpt_m3::tonal_badge(ui, tag, c.surface_container_highest, c.on_surface_variant);
                }
            });
            ui.label(row.qty.to_string());
            ui.label(chaos_text(row.unit_chaos));
            ui.label(chaos_text(row.total_chaos));
            ui.label(div_text(row.total_chaos, rate));
            ui.end_row();
        }
    });
    if v.rows.len() > 100 {
        ui.label(RichText::new(format!("…and {} more", v.rows.len() - 100)).color(c.on_surface_variant));
    }
}

fn change_badge(ui: &mut Ui, label: &str, change: Option<f64>, divine: bool, rate: f64) {
    let c = theme(ui.ctx()).colors;
    let (text, bg, fg) = match change {
        None => (format!("{label}: —"), c.surface_container_highest, c.on_surface_variant),
        Some(d) => {
            let v = if divine && rate > 0.0 { format!("{:+.2} div", d / rate) } else { format!("{}{} c", if d >= 0.0 { "+" } else { "-" }, chaos_text(d.abs())) };
            let (bg, fg) = if d >= 0.0 { (c.success_container, c.on_success_container) } else { (c.error_container, c.on_error_container) };
            (format!("{label}: {v}"), bg, fg)
        }
    };
    rpt_m3::tonal_badge(ui, &text, bg, fg);
}

fn history_view(ui: &mut Ui, cfg: &mut Config, st: &mut WealthState, wm_id: u32, league: &str, out: &mut Out) {
    let th = theme(ui.ctx());
    let c = th.colors;
    if st.history.as_ref().is_none_or(|(l, _)| l != league) {
        st.history = Some((league.to_owned(), history::load(&crate::store::data_dir(), league)));
    }
    let Some((_, h)) = &st.history else { return };
    if let Some(e) = &st.history_error {
        rpt_m3::banner(ui, icons::ERROR, e.as_str(), true);
    }
    ui.horizontal(|ui| {
        let mut unit = st.history_divine;
        if rpt_m3::segmented(ui, &mut unit, &[(false, "Chaos"), (true, "Divine")]) {
            st.history_divine = unit;
        }
        ui.add_space(8.0);
        let rate = h.last().map(|s| s.divine_chaos).unwrap_or(0.0);
        change_badge(ui, "Last", history::change_over(h, 0), st.history_divine, rate);
        change_badge(ui, "24h", history::change_over(h, 86_400), st.history_divine, rate);
        change_badge(ui, "7d", history::change_over(h, 7 * 86_400), st.history_divine, rate);
    });
    ui.add_space(8.0);
    history::chart(ui, h, st.history_divine, 240.0);
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!(
            "{} snapshot{} · a snapshot is saved after every complete valuation",
            h.len(),
            if h.len() == 1 { "" } else { "s" }
        ))
        .font(th.font(TypeRole::BodySmall))
        .color(c.on_surface_variant),
    );

    rpt_m3::section_header(ui, "Auto snapshot");
    if let Some(x) = widget_cfg(cfg, wm_id) {
        let mut on = x.auto_snapshot_minutes > 0;
        let label = "Re-value the selected tabs periodically while this window is open";
        if rpt_m3::setting_row(ui, Some(icons::SCHEDULE), label, None, |ui| rpt_m3::switch(ui, &mut on).changed()) {
            x.auto_snapshot_minutes = if on { 60 } else { 0 };
            out.save = true;
        }
        if x.auto_snapshot_minutes > 0 {
            let mut m = x.auto_snapshot_minutes as f64;
            ui.horizontal(|ui| {
                ui.label("Every");
                if rpt_m3::number_field(ui, &mut m, 10.0..=1440.0, 1.0, 0, " min").changed() {
                    x.auto_snapshot_minutes = m as u32;
                    out.save = true;
                }
            });
        }
    }
}

fn dust_view(ui: &mut Ui, st: &mut WealthState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    rpt_m3::banner(ui, icons::INFO, "Dust assumes ilvl 84 and 20% quality for every unique (like Wealthy Exile); influences and corruption implicits add more in game.", false);
    ui.add_space(8.0);
    let Some(v) = &st.valuation else {
        ui.label(RichText::new("Value some tabs first.").color(c.on_surface_variant));
        return;
    };
    if v.uniques.is_empty() {
        ui.label(RichText::new("No disenchantable uniques in the selected tabs.").color(c.on_surface_variant));
        return;
    }
    let mut rows: Vec<&pricing::UniqueRow> = v.uniques.iter().collect();
    let key = |r: &pricing::UniqueRow| match st.dust_sort {
        DustSort::Name => 0.0,
        DustSort::Chaos => r.chaos.unwrap_or(-1.0),
        DustSort::Dust => r.dust,
        DustSort::DustPerChaos => r.dust_per_chaos().unwrap_or(f64::MAX),
    };
    rows.sort_by(|a, b| {
        let o = if st.dust_sort == DustSort::Name { a.name.cmp(&b.name) } else { key(a).total_cmp(&key(b)) };
        if st.dust_asc { o } else { o.reverse() }
    });
    egui::Grid::new("wealth-dust").num_columns(5).striped(true).spacing([16.0, 4.0]).show(ui, |ui| {
        for (label, sort) in [("Unique", Some(DustSort::Name)), ("Tab", None), ("Chaos", Some(DustSort::Chaos)), ("Dust", Some(DustSort::Dust)), ("Dust / chaos", Some(DustSort::DustPerChaos))] {
            let active = sort == Some(st.dust_sort);
            let arrow = if active { if st.dust_asc { " ▲" } else { " ▼" } } else { "" };
            let text = RichText::new(format!("{label}{arrow}")).font(th.font(TypeRole::LabelMedium)).color(if active { c.primary } else { c.on_surface_variant });
            match sort {
                Some(s) => {
                    if ui.add(egui::Label::new(text).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        if active {
                            st.dust_asc = !st.dust_asc;
                        } else {
                            st.dust_sort = s;
                            st.dust_asc = s == DustSort::Name;
                        }
                    }
                }
                None => {
                    ui.label(text);
                }
            }
        }
        ui.end_row();
        for r in rows {
            ui.label(&r.name).on_hover_text(&r.base);
            ui.label(RichText::new(&r.tab).color(c.on_surface_variant));
            ui.label(r.chaos.map(chaos_text).unwrap_or_else(|| "—".into()));
            ui.label(fmt_int(r.dust));
            ui.label(r.dust_per_chaos().map(|d| fmt_int(d)).unwrap_or_else(|| "—".into()));
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new("High dust per chaos → disenchant; low → sell.").font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant),
    );
}

fn bulk_view(ui: &mut Ui, cfg: &mut Config, prices: &Arc<PriceIndex>, st: &mut WealthState, wm_id: u32, league: &str, out: &mut Out) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let Some(v) = &st.valuation else {
        ui.label(RichText::new("Value some tabs first.").color(c.on_surface_variant));
        return;
    };
    let rate = v.divine_chaos.or_else(|| prices.xchg_rate());
    let Some(x) = widget_cfg(cfg, wm_id) else { return };

    ui.horizontal(|ui| {
        ui.label("Asking price");
        if rpt_m3::slider(ui, &mut x.bulk_percent, 50.0..=120.0, 1.0, 220.0).changed() {
            out.save = true;
        }
        ui.label(RichText::new(format!("{:.0}% of poe.ninja", x.bulk_percent)).font(th.font(TypeRole::LabelLarge)));
    });
    let pct = x.bulk_percent / 100.0;
    let candidates: Vec<&pricing::Row> = v.rows.iter().filter(|r| matches!(r.kind, RowKind::Stackable | RowKind::Unique)).collect();
    if candidates.is_empty() {
        ui.label(RichText::new("No priced stackables or uniques.").color(c.on_surface_variant));
        return;
    }

    rpt_m3::section_header(ui, "Items to sell");
    ui.horizontal(|ui| {
        if ui.add(Button::text("Select all").small()).clicked() {
            st.bulk_selected = candidates.iter().map(|r| r.key.clone()).collect();
        }
        if ui.add(Button::text("Clear").small()).clicked() {
            st.bulk_selected.clear();
        }
    });
    egui::ScrollArea::vertical().id_salt("wealth-bulk-pick").max_height(220.0).show(ui, |ui| {
        egui::Grid::new("wealth-bulk").num_columns(4).striped(true).spacing([16.0, 0.0]).show(ui, |ui| {
            for h in ["Item", "Qty", "poe.ninja", "Asking"] {
                ui.label(RichText::new(h).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant));
            }
            ui.end_row();
            for r in &candidates {
                let mut on = st.bulk_selected.contains(&r.key);
                if rpt_m3::checkbox(ui, &mut on, r.name.as_str()).changed() {
                    if on {
                        st.bulk_selected.insert(r.key.clone());
                    } else {
                        st.bulk_selected.remove(&r.key);
                    }
                }
                ui.label(r.qty.to_string());
                ui.label(format!("{}c", chaos_text(r.unit_chaos)));
                ui.label(bulk::chat_price(r.unit_chaos * pct, rate));
                ui.end_row();
            }
        });
    });

    let picked: Vec<&&pricing::Row> = candidates.iter().filter(|r| st.bulk_selected.contains(&r.key)).collect();
    if picked.is_empty() {
        return;
    }
    let entries: Vec<bulk::BulkEntry> = picked.iter().map(|r| bulk::BulkEntry { name: r.name.clone(), qty: r.qty, unit_chaos: r.unit_chaos * pct }).collect();

    rpt_m3::section_header(ui, "Trade chat");
    for (i, line) in bulk::wts_lines(league, &entries, rate, bulk::CHAT_LINE_MAX).into_iter().enumerate() {
        copy_row(ui, ("wts", i), &line);
    }

    rpt_m3::section_header(ui, "Price notes");
    rpt_m3::banner(ui, icons::INFO, "Set these notes on the items in game yourself (right-click the item in a premium tab). Stack notes are per unit.", false);
    if rpt_m3::switch_row(ui, "Use ~b/o instead of ~price", &mut x.note_buyout).changed() {
        out.save = true;
    }
    for e in &entries {
        let note = bulk::price_note(e.unit_chaos, rate, x.note_buyout);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&e.name).color(c.on_surface_variant));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(IconButton::new(icons::CONTENT_COPY).size(32.0).tooltip("Copy note")).clicked() {
                    ui.ctx().copy_text(note.clone());
                }
                ui.label(RichText::new(&note).font(th.font(TypeRole::LabelLarge)).color(c.on_surface));
            });
        });
    }
}

fn copy_row(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, text: &str) {
    let th = theme(ui.ctx());
    let c = th.colors;
    ui.push_id(id, |ui| {
        rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(IconButton::new(icons::CONTENT_COPY).size(32.0).tooltip("Copy")).clicked() {
                        ui.ctx().copy_text(text.to_owned());
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(text).color(c.on_surface)).wrap());
                    });
                });
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Headless render of the widget body while signed out (no network, no keychain).
    #[test]
    fn renders_signed_out_headless() {
        rpt_data::init("en").unwrap();
        let ctx = egui::Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let mut cfg = Config::default();
        cfg.league_id = Some("Standard".into());
        let wm_id = crate::wm::id_of(&cfg, "rpt-wealth").unwrap();
        let client = Arc::new(rpt_trade::TradeClient::new(rpt_trade::TradeConfig::default()).unwrap());
        let leagues = Arc::new(rpt_trade::Leagues::new(client.clone()));
        let prices = Arc::new(PriceIndex::new(client, leagues));
        let mut st = WealthState::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0));
        let mut saw_prompt = false;
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let o = body(ui, &mut cfg, &prices, &mut st, wm_id);
                assert!(!o.save);
            });
            saw_prompt |= out.shapes.iter().any(|s| format!("{:?}", s.shape).contains("Sign in to value your stash"));
            out.textures_delta.clear();
        }
        assert!(saw_prompt, "signed-out prompt is drawn");
        assert!(st.job.is_none());

        // The prompt's button opens Settings on the Account page.
        crate::widgets::settings::open_account(&mut cfg);
        let sid = crate::wm::id_of(&cfg, "settings").unwrap();
        assert!(cfg.widgets.iter().find(|w| w.wm_id == sid).unwrap().wants_show());
    }

    #[test]
    fn widget_config_round_trips() {
        let mut w = WealthWidget::default();
        w.tabs.insert("Standard".into(), vec!["a1".into()]);
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("\"bulkPercent\":90"));
        assert_eq!(serde_json::from_str::<WealthWidget>(&json).unwrap(), w);
        // older configs without the new fields still load
        let old: WealthWidget = serde_json::from_str(r#"{"anchor":{"pos":"cc","x":50,"y":50}}"#).unwrap();
        assert_eq!(old.bulk_percent, 90.0);
    }
}
