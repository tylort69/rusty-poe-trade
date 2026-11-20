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
