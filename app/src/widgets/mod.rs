//! Overlay widgets. Each upstream `Widget*.vue` maps to a module here.
//!
//! A widget is drawn by [`draw_widget`], which dispatches on `WidgetKind` and
//! hands the widget a [`WCtx`] (config, overlay state, i18n, host command queue,
//! long-lived services) plus its own per-instance runtime state.

pub mod chrome;
pub mod delve_grid;
pub mod image_strip;
pub mod item_check;
pub mod item_search;
pub mod menu;
pub mod price_check;
pub mod settings;
pub mod stash_search;
pub mod timer;
pub mod wealth;
pub mod whispers;
pub mod mapping;
pub mod market;
pub mod pob;

use std::collections::HashMap;

use crate::config::{Config, WidgetKind};
use crate::host::{HostEvent, HostQueue};
use crate::i18n::I18n;
use crate::services::Services;
use crate::wm::OverlayState;

/// Tools that can be opened from the Settings → Tools page or their own hotkey.
/// (widget type, title, Material icon, one-line description)
pub const TOOLS: &[(&str, &str, &str, &str)] = &[
    ("rpt-wealth", "Wealth", rpt_m3::icons::ATTACH_MONEY, "Stash value, net-worth history, dust and bulk-sell lists"),
    ("rpt-market", "Market", rpt_m3::icons::STOREFRONT, "7-day movers, favorites, saved searches and price history"),
    ("rpt-whispers", "Trade whispers", rpt_m3::icons::FORUM, "Incoming and outgoing trades with invite / trade / kick"),
    ("rpt-mapping", "Mapping", rpt_m3::icons::MAP, "Map timer, maps per hour, deaths and history"),
    ("rpt-pob", "Path of Building", rpt_m3::icons::QUERY_STATS, "Your PoB builds: stats, skills, items, tree"),
    ("item-search", "Item search", rpt_m3::icons::SEARCH, "Look up any item's price by name"),
];

/// Open a tool (or Settings with `"settings"`) and make the overlay
/// interactive. If it's already open in an active overlay, close it and return
/// to the game instead, so the same hotkey toggles.
pub fn open_tool(w: &mut WCtx<'_>, tool: &str) {
    let Some(id) = crate::wm::id_of(w.cfg, tool) else { return };
    let shown = w.cfg.widgets.iter().any(|x| x.wm_id == id && x.wants_show());
    if shown && w.st.active {
        crate::wm::hide(w.cfg, id);
        w.host.push(crate::host::HostCommand::FocusGame);
    } else {
        if tool == "settings" {
            settings::open_tools(w.cfg);
        } else {
            crate::wm::show(w.cfg, id);
        }
        if !w.st.active {
            w.host.push(crate::host::HostCommand::FocusOverlay);
        }
    }
    w.save_requested = true;
}

/// Everything a widget may touch while drawing.
pub struct WCtx<'a> {
    pub cfg: &'a mut Config,
    pub st: OverlayState,
    pub i18n: &'a I18n,
    pub host: &'a mut HostQueue,
    pub services: &'a Services,
    /// Overlay area in egui points (the game client area).
    pub screen: egui::Rect,
    /// Set by a widget to ask `App` to persist the config.
    pub save_requested: bool,
}

/// Per-instance runtime state (not persisted).
#[derive(Default)]
pub struct Runtime {
    pub timers: HashMap<u32, timer::TimerState>,
    pub price_check: price_check::PriceCheckState,
    pub item_check: item_check::ItemCheckState,
    pub item_search: item_search::ItemSearchState,
    pub settings: settings::SettingsState,
    pub menu: menu::MenuState,
    pub image_strip: HashMap<u32, image_strip::ImageStripState>,
    pub wealth: wealth::WealthState,
    pub whispers: whispers::WhispersState,
    pub mapping: mapping::MappingState,
    pub market: market::MarketState,
    pub pob: pob::PobState,
}

/// Widgets drawn in their own OS windows (outside the game overlay). Call
/// once per frame after the overlay widgets.
pub fn draw_popouts(ctx: &egui::Context, w: &mut WCtx<'_>, rt: &mut Runtime) {
    whispers::show_popout(ctx, w, &mut rt.whispers);
}

pub fn draw_widget(ctx: &egui::Context, w: &mut WCtx<'_>, rt: &mut Runtime, wm_id: u32) {
    let Some(kind) = w.cfg.widgets.iter().find(|x| x.wm_id == wm_id).map(|x| x.kind.type_name()) else { return };
    match kind {
        "menu" => menu::show(ctx, w, &mut rt.menu, wm_id),
        "settings" => settings::show(ctx, w, &mut rt.settings, wm_id),
        "item-search" => {
            item_search::show(ctx, w, &mut rt.item_search, wm_id);
            if let Some((item, pos)) = rt.item_search.take_price_check() {
                price_check::open_item(w, &mut rt.price_check, item, pos);
            }
        }
        "timer" => timer::show(ctx, w, rt.timers.entry(wm_id).or_default(), wm_id),
        "stash-search" => stash_search::show(ctx, w, wm_id),
        "price-check" => price_check::show(ctx, w, &mut rt.price_check, wm_id),
        "item-check" => item_check::show(ctx, w, &mut rt.item_check, wm_id),
        "image-strip" => image_strip::show(ctx, w, rt.image_strip.entry(wm_id).or_default(), wm_id),
        "delve-grid" => delve_grid::show(ctx, w, wm_id),
        "rpt-wealth" => wealth::show(ctx, w, &mut rt.wealth, wm_id),
        "rpt-whispers" => whispers::show(ctx, w, &mut rt.whispers, wm_id),
        "rpt-mapping" => mapping::show(ctx, w, &mut rt.mapping, wm_id),
        "rpt-market" => market::show(ctx, w, &mut rt.market, wm_id),
        "rpt-pob" => pob::show(ctx, w, &mut rt.pob, wm_id),
        _ => {}
    }
}

/// Route a host event to widgets (upstream `MainProcess.onEvent` listeners).
pub fn handle_event(w: &mut WCtx<'_>, rt: &mut Runtime, ev: &HostEvent) {
    pob::on_item_text(&mut rt.pob, ev);
    match ev {
        HostEvent::WidgetAction { target } => {
            if let Some(tool) = target.strip_prefix("open-tool:") {
                open_tool(w, tool);
            } else if let Some(qa) = target.strip_prefix("whisper-quick:").and_then(whispers::QuickAction::from_key) {
                whispers::quick(w, &mut rt.whispers, qa);
            } else if let Some(id) = target.strip_prefix("stopwatch-start-stop:").and_then(|s| s.parse().ok()) {
                rt.timers.entry(id).or_default().toggle();
            } else if let Some(id) = target.strip_prefix("stopwatch-reset:").and_then(|s| s.parse().ok()) {
                rt.timers.entry(id).or_default().reset();
            } else if target == "delve-grid" {
                if let Some(id) = crate::wm::id_of(w.cfg, "delve-grid") {
                    crate::wm::toggle(w.cfg, id);
                }
            }
        }
        HostEvent::ItemText { target, .. } => match target.as_str() {
            "price-check" => price_check::on_item_text(w, &mut rt.price_check, ev),
            "item-check" | "open-wiki" | "open-craft-of-exile" | "open-poedb" | "search-similar" => {
                item_check::on_item_text(w, &mut rt.item_check, ev)
            }
            _ => {}
        },
        HostEvent::GameLog(lines) => {
            item_check::on_game_log(w, &mut rt.item_check, lines);
            whispers::on_game_log(w, &mut rt.whispers, lines);
            mapping::on_game_log(w, &mut rt.mapping, lines);
        }
        _ => {}
    }
}

/// Title shown for a widget in menus (upstream `wmTitle` or translated type name).
pub fn display_title(i18n: &I18n, kind: &WidgetKind, title: &str) -> String {
    if !title.is_empty() && !title.starts_with("{icon=") {
        return title.to_owned();
    }
    let key = match kind {
        WidgetKind::Timer(_) => "stopwatch.name",
        WidgetKind::StashSearch(_) => "stash_search.name",
        WidgetKind::ImageStrip(_) => "image_strip.name",
        WidgetKind::ItemSearch(_) => "item_search.name",
        WidgetKind::Settings(_) => "settings.title",
        WidgetKind::DelveGrid(_) => "delve_grid.name",
        WidgetKind::PriceCheck(_) => "price_check.name",
        WidgetKind::ItemCheck(_) => "item_check.name",
        WidgetKind::Menu(_) => "menu",
        WidgetKind::Wealth(_) => return "Wealth".into(),
        WidgetKind::Whispers(_) => return "Trade whispers".into(),
        WidgetKind::Mapping(_) => return "Mapping".into(),
        WidgetKind::Market(_) => return "Market".into(),
        WidgetKind::Pob(_) => return "Path of Building".into(),
    };
    i18n.t(key)
}

#[cfg(test)]
mod overlap_tests {
    use super::*;
    use crate::host::HostQueue;

    /// Text drawn inside a rect (to name widgets in the report).
    fn texts_in(shapes: &[egui::epaint::ClippedShape], r: egui::Rect) -> String {
        let mut out = Vec::new();
        fn walk(s: &egui::Shape, r: egui::Rect, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => {
                    let tr = egui::Rect::from_min_size(t.pos, t.galley.size());
                    if r.expand(1.0).intersects(tr) {
                        let txt = t.galley.text().chars().filter(|c| !('\u{e000}'..='\u{f8ff}').contains(c)).collect::<String>();
                        if !txt.trim().is_empty() {
                            out.push(txt.trim().chars().take(30).collect());
                        }
                    }
                }
                egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, r, out)),
                _ => {}
            }
        }
        for c in shapes {
            walk(&c.shape, r, &mut out);
        }
        out.dedup();
        out.join(" | ")
    }

    /// Interactive widgets (or labels) that partly cover each other in one window.
    fn overlaps(ctx: &egui::Context) -> Vec<(egui::Rect, egui::Rect, String)> {
        let mut out = Vec::new();
        ctx.viewport(|vp| {
            for (layer, rects) in vp.prev_pass.widgets.layers() {
                // skip containers (scroll areas, panels): only widget-sized rects
                let rs: Vec<_> = rects.iter().filter(|r| r.interact_rect.is_positive() && r.interact_rect.area() > 4.0 && r.interact_rect.height() < 120.0).collect();
                for (i, a) in rs.iter().enumerate() {
                    for b in &rs[i + 1..] {
                        // visible (clipped) parts: rows scrolled out of view don't count
                        let (ra, rb) = (a.interact_rect, b.interact_rect);
                        let inter = ra.intersect(rb);
                        if !inter.is_positive() {
                            continue;
                        }
                        // nesting (a scroll area and its contents, a card and its button) is fine
                        if ra.expand(0.5).contains_rect(rb) || rb.expand(0.5).contains_rect(ra) {
                            continue;
                        }
                        let small = ra.area().min(rb.area());
                        if inter.area() / small > 0.15 && inter.width() > 3.0 && inter.height() > 3.0 {
                            let kind = |s: egui::Sense| if s.senses_click() { "click" } else if s.senses_drag() { "drag" } else { "hover" };
                            out.push((ra, rb, format!("{:?} {}↔{}", layer.order, kind(a.sense), kind(b.sense))));
                        }
                    }
                }
            }
        });
        out
    }

    /// Render every tool window and report widgets drawn on top of each other.
    #[test]
    fn no_overlapping_widgets_in_tool_windows() {
        rpt_data::init("en").unwrap();
        let ctx = egui::Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let data = rpt_data::data();
        let i18n = crate::i18n::I18n::new(&data);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2560.0, 1440.0));
        use settings::Page;
        const HELMET: &str = "Item Class: Helmets\nRarity: Rare\nDoom Visor\nHubris Circlet\n--------\nEnergy Shield: 220 (augmented)\n--------\nRequirements:\nLevel: 69\nInt: 154\n--------\nSockets: B-B-B B\n--------\nItem Level: 84\n--------\n{ Prefix Modifier \"Rotund\" (Tier: 3) — Life }\n+71(70-79) to maximum Life\n{ Suffix Modifier \"of the Furnace\" (Tier: 3) — Elemental, Fire, Resistance }\n+42(42-45)% to Fire Resistance\n";
        // (wmType, settings page) — every tool, every settings page, and a price check
        let mut cases: Vec<(&str, Option<Page>)> =
            ["menu", "rpt-wealth", "rpt-whispers", "rpt-mapping", "rpt-market", "rpt-pob", "item-search", "stash-search", "timer", "price-check"].iter().map(|t| (*t, None)).collect();
        for p in [
            Page::Tools, Page::Hotkeys, Page::Chat, Page::General, Page::Appearance, Page::Widgets, Page::PriceCheck, Page::Maps,
            Page::ItemCheck, Page::ItemSearch, Page::Debug, Page::About, Page::Account, Page::Stopwatch,
        ] {
            cases.push(("settings", Some(p)));
        }
        let mut report = Vec::new();
        for (t, page) in cases {
            let mut cfg = crate::config::Config::default();
            let services = crate::services::Services::new(&cfg, ctx.clone());
            let mut host = HostQueue::default();
            let mut rt = Runtime::default();
            let Some(id) = crate::wm::id_of(&cfg, t) else { continue };
            crate::wm::show(&mut cfg, id);
            if let Some(p) = page {
                rt.settings.page = p;
            }
            let t = match page {
                Some(p) => format!("settings/{p:?}"),
                None => t.to_owned(),
            };
            let t = t.as_str();
            if t == "price-check" {
                let item = price_check::parse_item(HELMET).expect("helmet parses");
                let mut w = WCtx { cfg: &mut cfg, st: crate::wm::OverlayState { active: true, ..Default::default() }, i18n: &i18n, host: &mut host, services: &services, screen, save_requested: false };
                price_check::open_item(&mut w, &mut rt.price_check, item, egui::pos2(2200.0, 900.0));
            }
            let mut shapes = Vec::new();
            for _ in 0..4 {
                let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut out = ctx.run_ui(input, |ui| {
                    let mut w = WCtx { cfg: &mut cfg, st: crate::wm::OverlayState { active: true, ..Default::default() }, i18n: &i18n, host: &mut host, services: &services, screen, save_requested: false };
                    draw_widget(ui.ctx(), &mut w, &mut rt, id);
                });
                out.textures_delta.clear();
                shapes = out.shapes;
            }
            if std::env::var("RPT_DUMP").ok().as_deref() == Some(t) {
                ctx.viewport(|vp| {
                    for (layer, rects) in vp.prev_pass.widgets.layers() {
                        for r in rects {
                            eprintln!("DUMP {:?} {:?} {:?} [{}]", layer.order, r.sense, r.rect, texts_in(&shapes, r.rect));
                        }
                    }
                });
            }
            let n = ctx.viewport(|vp| vp.prev_pass.widgets.layers().map(|(_, r)| r.len()).sum::<usize>());
            eprintln!("case {t}: {n} widgets");
            assert!(n > 5, "{t} drew nothing");
            for (a, b, k) in overlaps(&ctx) {
                report.push(format!("{t}: {k} [{}] × [{}]  {a:?} × {b:?}", texts_in(&shapes, a), texts_in(&shapes, b)));
            }
        }
        for r in &report {
            eprintln!("{r}");
        }
        assert!(report.is_empty(), "{} overlapping widget pairs", report.len());
    }
}
