//! Mapping session stats fed by Client.txt.
//!
//! - [`parse`]: Client.txt line parsing (also used by the whisper widget);
//! - [`session`]: map-run / session tracking from log timestamps;
//! - [`history`]: append-only `data_dir()/mapping/history.jsonl`.

pub mod history;
pub mod parse;
pub mod session;

use std::time::Duration;

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, vec2};
use rpt_m3::{CardKind, IconButton, TypeRole, icons, theme};
use serde::{Deserialize, Serialize};

use self::history::{History, Record};
use self::parse::{AreaKind, LogSecs, now_secs, tier};
use self::session::{MapRun, Output, Tracker, stats};
use super::{WCtx, chrome};
use crate::config::{Anchor, WidgetKind};

// Material Icons Round codepoints not in rpt_m3::icons.
const ICON_DEATH: &str = "\u{eac2}"; // heart_broken
const ICON_PORTAL: &str = "\u{ea77}"; // login
const ICON_HOME: &str = "\u{e88a}"; // home
const ICON_TOWN: &str = "\u{e587}"; // cottage

/// Completed maps kept in memory for the history list / chart.
const HISTORY_CAP: usize = 500;

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-mapping"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MappingWidget {
    pub anchor: Anchor,
    /// Character whose deaths are counted. Empty → the whisper widget's
    /// character name; if that is empty too, every "has been slain" counts.
    pub character_name: String,
    /// A new session starts after this many idle minutes.
    pub idle_minutes: u32,
}

impl Default for MappingWidget {
    fn default() -> Self {
        Self { anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 }, character_name: String::new(), idle_minutes: 30 }
    }
}

/// Runtime (non-persisted) state.
pub struct MappingState {
    pub tracker: Tracker,
    /// All completed maps (persisted + this run), oldest first.
    pub history: Vec<MapRun>,
    store: Option<History>,
    loaded: bool,
    /// Received at least one Client.txt line.
    got_lines: bool,
    error: Option<String>,
}

impl Default for MappingState {
    fn default() -> Self {
        Self { tracker: Tracker::default(), history: Vec::new(), store: None, loaded: false, got_lines: false, error: None }
    }
}

impl MappingState {
    /// State backed by a specific history file (tests).
    #[cfg(test)]
    pub fn with_history(path: impl Into<std::path::PathBuf>) -> Self {
        let mut st = Self::default();
        st.store = Some(History::new(path));
        st
    }

    /// Load persisted history once and continue the last session if it is still fresh.
    fn ensure_loaded(&mut self, now: LogSecs) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let store = self.store.get_or_insert_with(|| History::new(History::default_path()));
        let loaded = store.load();
        if let Some(s) = loaded.last_session.filter(|s| now - s.last <= self.tracker.idle_secs && !self.tracker.started()) {
            let runs = loaded.maps.iter().filter(|m| m.session == s.id).cloned().collect();
            self.tracker.restore(s, runs);
        }
        self.history = loaded.maps;
        self.trim();
    }

    fn trim(&mut self) {
        let n = self.history.len();
        if n > HISTORY_CAP {
            self.history.drain(..n - HISTORY_CAP);
        }
    }

    fn persist(&mut self, out: Vec<Output>) {
        for o in out {
            let rec = match o {
                Output::RunFinished(r) => {
                    self.history.push(r.clone());
                    Record::Map(r)
                }
                Output::Session(s) => Record::Session(s),
            };
            if let Some(store) = &self.store {
                if let Err(e) = store.append(&rec) {
                    self.error = Some(format!("Cannot write {}: {e}", store.path.display()));
                }
            }
        }
        self.trim();
    }

    fn sync_settings(&mut self, w: &WCtx<'_>) {
        let (name, idle) = settings(w);
        self.tracker.char_name = name;
        self.tracker.idle_secs = idle;
    }
}

fn own(cfg: &crate::config::Config) -> Option<&MappingWidget> {
    cfg.widgets.iter().find_map(|w| match &w.kind {
        WidgetKind::Mapping(m) => Some(m),
        _ => None,
    })
}

fn own_mut(cfg: &mut crate::config::Config) -> Option<&mut MappingWidget> {
    cfg.widgets.iter_mut().find_map(|w| match &mut w.kind {
        WidgetKind::Mapping(m) => Some(m),
        _ => None,
    })
}

/// (character name, idle seconds) with the whisper widget's name as fallback.
fn settings(w: &WCtx<'_>) -> (String, i64) {
    let m = own(w.cfg).cloned().unwrap_or_default();
    let name = if m.character_name.trim().is_empty() {
        w.cfg
            .widgets
            .iter()
            .find_map(|x| match &x.kind {
                WidgetKind::Whispers(wh) => Some(wh.character_name.trim().to_owned()),
                _ => None,
            })
            .unwrap_or_default()
    } else {
        m.character_name.trim().to_owned()
    };
    (name, i64::from(m.idle_minutes.max(1)) * 60)
}

/// Client.txt lines (already split, newest last). Called for every batch.
pub fn on_game_log(w: &mut WCtx<'_>, st: &mut MappingState, lines: &[String]) {
    st.sync_settings(w);
    st.ensure_loaded(now_secs());
    st.got_lines |= !lines.is_empty();
    let mut out = Vec::new();
    for line in lines {
        out.extend(st.tracker.feed(line));
    }
    st.persist(out);
}

// ---------------------------------------------------------------------------
// Formatting

pub fn fmt_duration(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Short duration for stat tiles: `1h 05m`, `12m`.
fn fmt_short(secs: i64) -> String {
    let m = secs.max(0) / 60;
    if m >= 60 { format!("{}h {:02}m", m / 60, m % 60) } else { format!("{m}m") }
}

pub fn fmt_clock(t: LogSecs) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.naive_utc().format("%m-%d %H:%M").to_string()).unwrap_or_default()
}

fn level_label(level: u32) -> String {
    match tier(level) {
        Some(t) => format!("T{t} · lvl {level}"),
        None => format!("lvl {level}"),
    }
}

// ---------------------------------------------------------------------------
// UI

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, st: &mut MappingState, wm_id: u32) {
    let now = now_secs();
    st.sync_settings(w);
    st.ensure_loaded(now);
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, w| {
        body(ui, w, st, now);
    });
    chrome::apply(w, wm_id, &r);
    if st.tracker.in_map() || st.tracker.zone.is_some() {
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

fn body(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut MappingState, now: LogSecs) {
    let th = theme(ui.ctx());
    let c = th.colors;
    ui.set_width(340.0);

    // Header
    ui.horizontal(|ui| {
        ui.label(rpt_m3::icon_text(icons::MAP, 20.0).color(c.primary));
        ui.label(RichText::new("Mapping").font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let gear = ui.add(IconButton::new(icons::SETTINGS).size(32.0).tooltip("Mapping settings"));
            rpt_m3::popover(&gear, |ui| settings_popover(ui, w));
            if ui.add(IconButton::new(icons::RESTART_ALT).size(32.0).tooltip("Reset session")).clicked() {
                let out = st.tracker.reset_session(now);
                st.persist(out);
            }
        });
    });

    if let Some(e) = &st.error {
        rpt_m3::banner(ui, icons::ERROR, e.clone(), true);
    }
    client_log_hint(ui, w, st.got_lines);

    // Current map
    rpt_m3::card(ui, CardKind::Filled).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match &st.tracker.current {
            Some(run) => {
                ui.horizontal(|ui| {
                    let name = if run.name.is_empty() { run.area_id.as_str() } else { run.name.as_str() };
                    ui.label(RichText::new(name).font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
                    rpt_m3::tonal_badge(ui, &level_label(run.level), c.secondary_container, c.on_secondary_container);
                    if !st.tracker.in_map() {
                        rpt_m3::tonal_badge(ui, "paused", c.surface_container, c.on_surface_variant);
                    }
                });
                ui.horizontal(|ui| {
                    let elapsed = st.tracker.current_elapsed(now);
                    ui.label(RichText::new(fmt_duration(elapsed)).font(th.font(TypeRole::HeadlineSmall)).color(c.primary));
                    ui.add_space(8.0);
                    icon_count(ui, ICON_DEATH, run.deaths, "Deaths in this map", if run.deaths > 0 { c.error } else { c.on_surface_variant });
                    icon_count(ui, ICON_PORTAL, run.portals(), "Re-entries (portals used)", c.on_surface_variant);
                });
            }
            None => {
                ui.label(RichText::new("Not in a map").font(th.font(TypeRole::TitleSmall)).color(c.on_surface_variant));
            }
        }
        if let Some(z) = &st.tracker.zone {
            let icon = match z.kind {
                AreaKind::Map => icons::MAP,
                AreaKind::Hideout => ICON_HOME,
                AreaKind::Town => ICON_TOWN,
                AreaKind::Other => icons::PUBLIC,
            };
            let since = (st.tracker.effective_now(now) - z.since).max(0);
            ui.horizontal(|ui| {
                ui.label(rpt_m3::icon_text(icon, 14.0).color(c.on_surface_variant));
                ui.label(RichText::new(format!("{} · {}", z.name, fmt_duration(since))).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));
            });
        }
    });

    // Session stats
    let s = stats(&st.tracker, now);
    rpt_m3::section_header(ui, if st.tracker.started() { format!("Session · since {}", fmt_clock(st.tracker.session.id)) } else { "Session".into() });
    let tile_w = ((ui.available_width() - 12.0) / 3.0).max(60.0);
    let rows = [
        [("Maps", s.maps.to_string()), ("Maps / hour", format!("{:.1}", s.maps_per_hour)), ("Avg map", fmt_duration(s.avg_map_secs))],
        [("In maps", fmt_short(s.map_secs)), ("In hideout", fmt_short(s.hideout_secs + s.town_secs)), ("Deaths", s.deaths.to_string())],
    ];
    for row in rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (label, value) in row {
                stat_tile(ui, tile_w, label, &value);
            }
        });
    }
    ui.add_space(6.0);
    time_split_bar(ui, &[(s.map_secs, c.primary, "maps"), (s.hideout_secs + s.town_secs, c.tertiary, "hideout/town"), (s.other_secs, c.outline_variant, "other")]);

    // Chart
    rpt_m3::section_header(ui, "Last 20 maps");
    let last: Vec<&MapRun> = st.history.iter().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect();
    duration_chart(ui, &last);

    // History
    rpt_m3::section_header(ui, "History");
    if st.history.is_empty() {
        ui.label(RichText::new("Completed maps appear here.").font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));
    } else {
        egui::ScrollArea::vertical().id_salt("mapping-history").max_height(160.0).show(ui, |ui| {
            for run in st.history.iter().rev().take(100) {
                history_row(ui, run);
            }
        });
    }
}

fn settings_popover(ui: &mut Ui, w: &mut WCtx<'_>) {
    let th = theme(ui.ctx());
    ui.set_width(260.0);
    let Some(m) = own_mut(w.cfg) else { return };
    let mut changed = false;
    ui.label(RichText::new("Character name (for deaths)").font(th.font(TypeRole::LabelLarge)).color(th.colors.on_surface_variant));
    changed |= rpt_m3::text_field(ui, &mut m.character_name, "same as trade whispers").changed();
    ui.add_space(6.0);
    ui.label(RichText::new("New session after idle").font(th.font(TypeRole::LabelLarge)).color(th.colors.on_surface_variant));
    let mut idle = f64::from(m.idle_minutes);
    if rpt_m3::number_field(ui, &mut idle, 1.0..=600.0, 1.0, 0, " min").changed() {
        m.idle_minutes = idle.round() as u32;
        changed = true;
    }
    if changed {
        w.save_requested = true;
    }
}

/// Hint when Client.txt is not configured and nothing has arrived yet.
pub(crate) fn client_log_hint(ui: &mut Ui, w: &mut WCtx<'_>, got_lines: bool) {
    if got_lines || w.cfg.client_log.as_deref().is_some_and(|p| !p.trim().is_empty()) {
        return;
    }
    rpt_m3::banner(ui, icons::INFO, "Client.txt path is not set (auto-detect is used). If nothing shows up, set it in Settings → General.", false);
    if ui.add(rpt_m3::Button::text("Open Settings").icon(icons::SETTINGS).small()).clicked() {
        if let Some(id) = crate::wm::id_of(w.cfg, "settings") {
            crate::wm::show(w.cfg, id);
        }
    }
}

fn icon_count(ui: &mut Ui, icon: &str, n: u32, tip: &str, color: Color32) {
    let th = theme(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.label(rpt_m3::icon_text(icon, 16.0).color(color));
        ui.label(RichText::new(n.to_string()).font(th.font(TypeRole::LabelLarge)).color(color));
    })
    .response
    .on_hover_text(tip);
}

fn stat_tile(ui: &mut Ui, width: f32, label: &str, value: &str) {
    let th = theme(ui.ctx());
    let c = th.colors;
    rpt_m3::surface(ui, c.surface_container_high, rpt_m3::shape::SMALL)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .shadow(egui::Shadow::NONE)
        .show(ui, |ui| {
            ui.set_width(width - 16.0);
            ui.label(RichText::new(label).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
            ui.label(RichText::new(value).font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
        });
}

/// Stacked horizontal bar of where the session time went, with a legend.
fn time_split_bar(ui: &mut Ui, parts: &[(i64, Color32, &str)]) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let total: i64 = parts.iter().map(|p| p.0.max(0)).sum();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), c.surface_container_highest);
    if total > 0 {
        let mut x = rect.left();
        for (secs, color, _) in parts {
            let wdt = rect.width() * (*secs).max(0) as f32 / total as f32;
            if wdt > 0.0 {
                p.rect_filled(Rect::from_min_size(Pos2::new(x, rect.top()), vec2(wdt, rect.height())), CornerRadius::same(2), *color);
            }
            x += wdt;
        }
    }
    ui.horizontal_wrapped(|ui| {
        for (secs, color, label) in parts {
            rpt_m3::status_dot(ui, *color, 8.0);
            let pct = if total > 0 { *secs as f64 * 100.0 / total as f64 } else { 0.0 };
            ui.label(RichText::new(format!("{label} {pct:.0}%")).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
        }
    });
}

/// Bar chart of map durations (oldest left). Average drawn as a dashed line.
fn duration_chart(ui: &mut Ui, runs: &[&MapRun]) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 80.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(rpt_m3::shape::SMALL), c.surface_container_low);
    if runs.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, "No maps yet", th.font(TypeRole::BodySmall), c.on_surface_variant);
        return;
    }
    let max = runs.iter().map(|r| r.map_secs).max().unwrap_or(1).max(60) as f32;
    let avg = runs.iter().map(|r| r.map_secs).sum::<i64>() as f32 / runs.len() as f32;
    let inner = rect.shrink2(vec2(6.0, 8.0));
    let slot = inner.width() / 20.0;
    let bar_w = (slot * 0.7).max(2.0);
    let hover = resp.hover_pos();
    let mut tip: Option<String> = None;
    for (i, r) in runs.iter().enumerate() {
        let h = inner.height() * (r.map_secs as f32 / max).clamp(0.0, 1.0);
        let x = inner.left() + slot * i as f32 + (slot - bar_w) / 2.0;
        let bar = Rect::from_min_max(Pos2::new(x, inner.bottom() - h.max(2.0)), Pos2::new(x + bar_w, inner.bottom()));
        let hovered = hover.is_some_and(|h| h.x >= x - (slot - bar_w) / 2.0 && h.x < x + bar_w + (slot - bar_w) / 2.0);
        let color = if r.deaths > 0 { c.error } else if hovered { c.primary } else { rpt_m3::with_alpha(c.primary, 0.75) };
        p.rect_filled(bar, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, color);
        if hovered {
            let name = if r.name.is_empty() { &r.area_id } else { &r.name };
            tip = Some(format!("{name} ({}) · {} · {} deaths", level_label(r.level), fmt_duration(r.map_secs), r.deaths));
        }
    }
    let y = inner.bottom() - inner.height() * (avg / max).clamp(0.0, 1.0);
    let dashes = egui::Shape::dashed_line(&[Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)], Stroke::new(1.0, c.tertiary), 4.0, 3.0);
    p.extend(dashes);
    p.text(Pos2::new(inner.right(), y - 2.0), Align2::RIGHT_BOTTOM, format!("avg {}", fmt_duration(avg as i64)), FontId::proportional(10.0), c.tertiary);
    if let Some(t) = tip {
        resp.on_hover_text(t);
    }
}

fn history_row(ui: &mut Ui, run: &MapRun) {
    let th = theme(ui.ctx());
    let c = th.colors;
    ui.horizontal(|ui| {
        let name = if run.name.is_empty() { run.area_id.as_str() } else { run.name.as_str() };
        ui.label(RichText::new(name).font(th.font(TypeRole::BodyMedium)).color(c.on_surface));
        let t = tier(run.level).map(|t| format!("T{t}")).unwrap_or_else(|| format!("L{}", run.level));
        rpt_m3::tonal_badge(ui, &t, c.secondary_container, c.on_secondary_container);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(fmt_clock(run.started)).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
            if run.deaths > 0 {
                icon_count(ui, ICON_DEATH, run.deaths, "Deaths", c.error);
            }
            ui.label(RichText::new(fmt_duration(run.map_secs)).font(th.font(TypeRole::LabelLarge)).color(c.on_surface));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::HostQueue;

    fn line(hms: &str, msg: &str) -> String {
        format!("2024/01/01 {hms} 123456789 cffb0719 [INFO Client 1234] {msg}")
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_duration(65), "1:05");
        assert_eq!(fmt_duration(3725), "1:02:05");
        assert_eq!(fmt_short(3900), "1h 05m");
        assert_eq!(level_label(83), "T16 · lvl 83");
    }

    /// Feed lines through `on_game_log`, persist to a temp file, render headless.
    #[test]
    fn renders_headless_and_persists() {
        rpt_data::init("en").unwrap();
        let ctx = egui::Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let data = rpt_data::data();
        let i18n = crate::i18n::I18n::new(&data);
        let mut cfg = crate::config::Config::default();
        let services = crate::services::Services::new(&cfg, ctx.clone());
        let mut host = HostQueue::default();
        let dir = std::env::temp_dir().join(format!("rpt-mapping-render-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut st = MappingState::with_history(dir.join("history.jsonl"));
        let id = crate::wm::id_of(&cfg, "rpt-mapping").unwrap();
        crate::wm::show(&mut cfg, id);
        let screen = egui::Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0));
        let lines = vec![
            line("12:00:00", "Generating level 83 area \"MapWorldsStrand\" with seed 1"),
            line("12:00:01", ": You have entered Strand."),
            line("12:03:00", ": Me has been slain."),
            line("12:05:00", ": You have entered Felled Hideout."),
            line("12:06:00", "Generating level 81 area \"MapWorldsBeach\" with seed 2"),
            line("12:06:01", ": You have entered Beach."),
        ];
        for active in [true, false, true] {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let mut w = crate::widgets::WCtx {
                    cfg: &mut cfg,
                    st: crate::wm::OverlayState { active, ..Default::default() },
                    i18n: &i18n,
                    host: &mut host,
                    services: &services,
                    screen,
                    save_requested: false,
                };
                if active && st.history.is_empty() {
                    on_game_log(&mut w, &mut st, &lines);
                }
                show(ui.ctx(), &mut w, &mut st, id);
            });
            out.textures_delta.clear();
        }
        assert_eq!(st.history.len(), 1);
        assert_eq!(st.history[0].map_secs, 299);
        assert_eq!(st.tracker.current.as_ref().unwrap().name, "Beach");
        let loaded = History::new(dir.join("history.jsonl")).load();
        assert_eq!(loaded.maps.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
