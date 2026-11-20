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

use std::collections::HashMap;

use crate::config::{Config, WidgetKind};
use crate::host::{HostEvent, HostQueue};
use crate::i18n::I18n;
use crate::services::Services;
use crate::wm::OverlayState;

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
}

pub fn draw_widget(ctx: &egui::Context, w: &mut WCtx<'_>, rt: &mut Runtime, wm_id: u32) {
    let Some(kind) = w.cfg.widgets.iter().find(|x| x.wm_id == wm_id).map(|x| x.kind.type_name()) else { return };
    match kind {
        "menu" => menu::show(ctx, w, &mut rt.menu, wm_id),
        "settings" => settings::show(ctx, w, &mut rt.settings, wm_id),
        "item-search" => item_search::show(ctx, w, &mut rt.item_search, wm_id),
        "timer" => timer::show(ctx, w, rt.timers.entry(wm_id).or_default(), wm_id),
        "stash-search" => stash_search::show(ctx, w, wm_id),
        "price-check" => price_check::show(ctx, w, &mut rt.price_check, wm_id),
        "item-check" => item_check::show(ctx, w, &mut rt.item_check, wm_id),
        "image-strip" => image_strip::show(ctx, w, rt.image_strip.entry(wm_id).or_default(), wm_id),
        "delve-grid" => delve_grid::show(ctx, w, wm_id),
        _ => {}
    }
}

/// Route a host event to widgets (upstream `MainProcess.onEvent` listeners).
pub fn handle_event(w: &mut WCtx<'_>, rt: &mut Runtime, ev: &HostEvent) {
    match ev {
        HostEvent::WidgetAction { target } => {
            if let Some(id) = target.strip_prefix("stopwatch-start-stop:").and_then(|s| s.parse().ok()) {
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
    };
    i18n.t(key)
}
