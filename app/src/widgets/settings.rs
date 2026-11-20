//! Port of upstream `settings/SettingsWindow.vue` and all settings pages.
//!
//! An exclusive, dialog-like M3 window: top app bar with close, a navigation
//! rail of pages on the left, the selected page on the right, and a
//! Save / Cancel action row. Edits apply to `w.cfg` live (so theme changes
//! preview instantly); Cancel restores the snapshot taken when the window
//! opened, Save persists and pushes the host config.
//!
//! Opening with `settings::open_for(cfg, wm_id)` shows only that widget's
//! pages (upstream `settings::widget=<id>` flag).

mod account;
pub mod hotkey;
mod tools;
pub mod leagues;
mod pages;
mod widget_pages;

use egui::{Context, Margin, RichText, Ui, vec2};
use rpt_m3::{Button, IconButton, NavItem, TypeRole, icons, shape, theme};

use super::{WCtx, chrome};
use crate::config::Config;
use crate::host::HostCommand;
use crate::wm;

const WIDGET_FLAG: &str = "settings::widget=";
const ACCOUNT_FLAG: &str = "settings::page=account";
const TOOLS_FLAG: &str = "settings::page=tools";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Tools,
    Hotkeys,
    Chat,
    General,
    Appearance,
    Account,
    Widgets,
    PriceCheck,
    Maps,
    ItemCheck,
    ItemSearch,
    Debug,
    About,
    StashSearch,
    Stopwatch,
}

impl Page {
    fn icon(self) -> &'static str {
        match self {
            Self::Hotkeys => icons::KEYBOARD,
            Self::Chat => icons::CHAT,
            Self::General => icons::TUNE,
            Self::Appearance => icons::PALETTE,
            Self::Account => icons::ACCOUNT_CIRCLE,
            Self::Tools => icons::WIDGETS,
            Self::Widgets => icons::WIDGETS,
            Self::PriceCheck => icons::PRICE_CHECK,
            Self::Maps => icons::MAP,
            Self::ItemCheck => icons::INFO,
            Self::ItemSearch => icons::SEARCH,
            Self::Debug => icons::BUG_REPORT,
            Self::About => icons::HELP_OUTLINE,
            Self::StashSearch => icons::INVENTORY_2,
            Self::Stopwatch => icons::TIMER,
        }
    }

    fn title(self, w: &WCtx<'_>) -> String {
        let key = match self {
            Self::Hotkeys => "settings.hotkeys",
            Self::Chat => "settings.chat",
            Self::General => "settings.general",
            Self::Appearance => return "Appearance".into(),
            Self::Account => return "Account".into(),
            Self::Tools => return "Tools".into(),
            Self::Widgets => return "Widgets".into(),
            Self::PriceCheck => "price_check.name",
            Self::Maps => "map_check.name",
            Self::ItemCheck => "item.info",
            Self::ItemSearch => "item_search.name",
            Self::Debug => "settings.debug",
            Self::About => "settings.about",
            Self::StashSearch => "stash_search.name",
            Self::Stopwatch => "stopwatch.name",
        };
        w.i18n.t(key)
    }
}

/// Upstream `menuByType`: page groups (separated in the rail).
fn menu_by_type(t: Option<&str>) -> Vec<Vec<Page>> {
    use Page::*;
    match t {
        Some("stash-search") => vec![vec![StashSearch]],
        Some("timer") => vec![vec![Stopwatch]],
        Some("item-check") => vec![vec![ItemCheck, Maps]],
        Some("price-check") => vec![vec![PriceCheck]],
        Some("item-search") => vec![vec![ItemSearch]],
        _ => vec![
            vec![Tools],
            vec![Hotkeys, Chat],
            vec![General, Appearance, Account, Widgets],
            vec![PriceCheck, Maps, ItemCheck, ItemSearch],
            vec![Debug, About],
        ],
    }
}

pub struct SettingsState {
    pub(crate) page: Page,
    /// Per-widget mode (upstream `selectedWmId`).
    target: Option<u32>,
    snapshot: Option<Config>,
    last_pass: u64,
    host_key: Option<String>,
    pub(crate) cap: hotkey::Capture,
    leagues: leagues::Shared,
    maps: widget_pages::MapsState,
    patrons: Option<Vec<Vec<rpt_data::Patron>>>,
    account: account::AccountPageState,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            page: Page::Tools,
            target: None,
            snapshot: None,
            last_pass: 0,
            host_key: None,
            cap: hotkey::Capture::default(),
            leagues: Default::default(),
            maps: Default::default(),
            patrons: None,
            account: Default::default(),
        }
    }
}

/// Open the settings window on a widget's own pages (e.g. from a widget's ⚙ button).
pub fn open_for(cfg: &mut Config, widget_id: u32) {
    let Some(sid) = wm::id_of(cfg, "settings") else { return };
    wm::set_flag(cfg, sid, &format!("{WIDGET_FLAG}{widget_id}"), true);
    wm::show(cfg, sid);
}

/// Open the full settings window on the Account page (e.g. from a "Sign in" prompt).
pub fn open_account(cfg: &mut Config) {
    let Some(sid) = wm::id_of(cfg, "settings") else { return };
    wm::set_flag(cfg, sid, ACCOUNT_FLAG, true);
    wm::show(cfg, sid);
}

/// Open the full settings window on the Tools page (the settings hotkey).
pub fn open_tools(cfg: &mut Config) {
    let Some(sid) = wm::id_of(cfg, "settings") else { return };
    wm::set_flag(cfg, sid, TOOLS_FLAG, true);
    wm::show(cfg, sid);
}

/// Everything that requires `PushHostConfig` when it changes.
fn host_key(cfg: &Config) -> String {
    format!(
        "{:?}|{:?}|{:?}|{}|{}|{}|{}",
        cfg.shortcuts(),
        cfg.client_log,
        cfg.game_config,
        cfg.window_title,
        cfg.log_keys,
        cfg.stash_scroll,
        cfg.restore_clipboard
    )
}

fn close(w: &mut WCtx<'_>, st: &mut SettingsState, wm_id: u32, save: bool) {
    if save {
        account::submit_pending(w, &mut st.account);
    }
    if !save {
        if let Some(snap) = st.snapshot.take() {
            *w.cfg = snap;
        }
    }
    st.snapshot = None;
    st.cap.active = None;
    if st.target.take().is_some() {
        st.page = Page::Hotkeys;
    }
    wm::hide(w.cfg, wm_id);
    w.host.push(HostCommand::PushHostConfig);
    w.save_requested = true;
}

pub fn show(ctx: &Context, w: &mut WCtx<'_>, st: &mut SettingsState, wm_id: u32) {
    // A gap in drawn passes means the window was hidden in between → new session.
    let pass = ctx.cumulative_pass_nr();
    if pass > st.last_pass + 1 {
        st.snapshot = None;
        st.cap.active = None;
        st.patrons = None;
    }
    st.last_pass = pass;

    // Per-widget request (upstream watches `wmFlags`).
    let flag = w.cfg.widgets.iter().find(|x| x.wm_id == wm_id).and_then(|x| x.wm_flags.iter().find(|f| f.starts_with(WIDGET_FLAG)).cloned());
    if let Some(flag) = flag {
        wm::set_flag(w.cfg, wm_id, &flag, false);
        if let Some(id) = flag[WIDGET_FLAG.len()..].parse::<u32>().ok().filter(|id| w.cfg.widgets.iter().any(|x| x.wm_id == *id)) {
            let t = w.cfg.widgets.iter().find(|x| x.wm_id == id).map(|x| x.type_name());
            st.target = Some(id);
            st.page = menu_by_type(t)[0][0];
            st.snapshot = None;
        }
    }
    if wm::set_flag(w.cfg, wm_id, ACCOUNT_FLAG, false) {
        (st.target, st.page) = (None, Page::Account);
    }
    if wm::set_flag(w.cfg, wm_id, TOOLS_FLAG, false) {
        (st.target, st.page) = (None, Page::Tools);
    }
    if st.target.is_some_and(|id| !w.cfg.widgets.iter().any(|x| x.wm_id == id)) {
        st.target = None;
        st.page = Page::Hotkeys;
    }
    if st.snapshot.is_none() {
        st.snapshot = Some(w.cfg.clone());
    }
    if st.host_key.is_none() {
        st.host_key = Some(host_key(w.cfg));
    }

    let target_type = st.target.and_then(|id| w.cfg.widgets.iter().find(|x| x.wm_id == id)).map(|x| x.type_name());
    let groups = menu_by_type(target_type);
    if !groups.iter().flatten().any(|p| *p == st.page) {
        st.page = groups[0][0];
    }

    let th = theme(ctx);
    let c = th.colors;
    let screen = w.screen;
    let size = vec2((screen.width() - 32.0).min(880.0), (screen.height() - 32.0).min(720.0));
    let mut title = w.i18n.t("settings.title").replace("Awakened PoE Trade", "Rusty POE Trade");
    if let Some(wd) = st.target.and_then(|id| w.cfg.widgets.iter().find(|x| x.wm_id == id)) {
        title = format!("{title} — {}", super::display_title(w.i18n, &wd.kind, &wd.wm_title));
    }

    let mut action: Option<bool> = None; // Some(true) save, Some(false) cancel
    chrome::exclusive_window(ctx, "settings", size, None, |ui| {
        ui.set_min_height(size.y);
        rpt_m3::top_app_bar(ui, &title, |ui| {
            if ui.add(IconButton::new(icons::CLOSE).tooltip(w.i18n.t("Cancel"))).clicked() {
                action = Some(false);
            }
        });
        let body_h = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // Navigation rail.
            ui.allocate_ui_with_layout(vec2(80.0, body_h), egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.set_min_height(body_h);
                let titles: Vec<Vec<String>> = groups.iter().map(|g| g.iter().map(|p| p.title(w)).collect()).collect();
                egui::ScrollArea::vertical().id_salt("settings-rail").max_height(body_h - 48.0).show(ui, |ui| {
                    for (gi, group) in groups.iter().enumerate() {
                        if gi > 0 {
                            ui.add_space(4.0);
                            let (r, _) = ui.allocate_exact_size(vec2(80.0, 1.0), egui::Sense::hover());
                            ui.painter().hline((r.left() + 16.0)..=(r.right() - 16.0), r.center().y, egui::Stroke::new(1.0, c.outline_variant));
                            ui.add_space(8.0);
                        }
                        let items: Vec<NavItem<'_>> = group.iter().zip(&titles[gi]).map(|(p, t)| NavItem { icon: p.icon(), label: t.as_str() }).collect();
                        let sel = group.iter().position(|p| *p == st.page).unwrap_or(usize::MAX);
                        if let Some(i) = rpt_m3::navigation_rail(ui, &items, sel) {
                            st.page = group[i];
                            st.cap.active = None;
                        }
                    }
                });
                if groups.iter().map(Vec::len).sum::<usize>() >= 4 {
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        if ui.add(IconButton::new(icons::LOGOUT).tooltip(w.i18n.t("app.quit"))).clicked() {
                            w.host.push(HostCommand::Quit);
                        }
                    });
                }
            });

            // Page + actions.
            let page_w = ui.available_width();
            ui.allocate_ui_with_layout(vec2(page_w, body_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                let page_h = body_h - 56.0;
                egui::Frame::new()
                    .fill(c.surface_container_low)
                    .corner_radius(egui::CornerRadius::same(shape::LARGE))
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(page_w - 32.0);
                        ui.set_height(page_h - 24.0);
                        if st.page == Page::Maps {
                            // The map page manages its own (virtual) scrolling.
                            widget_pages::maps(ui, w, st);
                        } else {
                            egui::ScrollArea::vertical().id_salt(("settings-page", st.page as u8)).auto_shrink([false, false]).show(ui, |ui| {
                                ui.set_width(ui.available_width() - 8.0);
                                draw_page(ui, w, st);
                                ui.add_space(16.0);
                            });
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(Button::filled(w.i18n.t("Save")).icon(icons::SAVE)).clicked() {
                            action = Some(true);
                        }
                        if ui.add(Button::text(w.i18n.t("Cancel"))).clicked() {
                            action = Some(false);
                        }
                        if st.snapshot.as_ref().is_some_and(|s| s != &*w.cfg) {
                            ui.label(RichText::new("•").font(th.font(TypeRole::TitleLarge)).color(c.primary))
                                .on_hover_text("Unsaved changes");
                        }
                    });
                });
            });
        });
    });

    if let Some(save) = action {
        close(w, st, wm_id, save);
        st.host_key = None;
        return;
    }
    let key = host_key(w.cfg);
    if st.host_key.as_ref() != Some(&key) {
        w.host.push(HostCommand::PushHostConfig);
        st.host_key = Some(key);
    }
}

fn draw_page(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    match st.page {
        Page::Hotkeys => pages::hotkeys(ui, w, st),
        Page::Chat => pages::chat(ui, w, st),
        Page::General => pages::general(ui, w, st),
        Page::Appearance => pages::appearance(ui, w, st),
        Page::Account => account::page(ui, w, st),
        Page::Tools => tools::page(ui, w, st),
        Page::Widgets => pages::widgets(ui, w, st),
        Page::Debug => pages::debug(ui, w, st),
        Page::About => pages::about(ui, w, st),
        Page::PriceCheck => widget_pages::price_check(ui, w, st),
        Page::Maps => {}
        Page::ItemCheck => widget_pages::item_check(ui, w, st),
        Page::ItemSearch => widget_pages::item_search(ui, w, st),
        Page::StashSearch => widget_pages::stash_search(ui, w, st),
        Page::Stopwatch => widget_pages::stopwatch(ui, w, st),
    }
}

// ---------------------------------------------------------------------------
// Small shared helpers for the pages.

/// Text field bound to an optional string (empty → None). Returns true if changed.
fn opt_text_field(ui: &mut Ui, value: &mut Option<String>, hint: &str) -> bool {
    let mut s = value.clone().unwrap_or_default();
    if rpt_m3::text_field(ui, &mut s, hint).changed() {
        let t = s.trim();
        *value = if t.is_empty() { None } else { Some(t.to_owned()) };
        true
    } else {
        false
    }
}

/// Label above a full-width control (M3 "supporting" field label).
fn field_label(ui: &mut Ui, s: &str) {
    let th = theme(ui.ctx());
    ui.label(RichText::new(s).font(th.font(TypeRole::LabelLarge)).color(th.colors.on_surface_variant));
}

fn switch_setting(ui: &mut Ui, label: &str, supporting: Option<&str>, value: &mut bool) -> bool {
    rpt_m3::setting_row(ui, None, label, supporting, |ui| rpt_m3::switch(ui, value).changed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::HostQueue;

    /// Headless render of every settings page and the other widgets in this module's scope.
    #[test]
    fn renders_all_pages_headless() {
        rpt_data::init("en").unwrap();
        let ctx = Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let data = rpt_data::data();
        let i18n = crate::i18n::I18n::new(&data);
        let mut cfg = Config::default();
        let services = crate::services::Services::new(&cfg, ctx.clone());
        let mut host = HostQueue::default();
        let mut st = SettingsState::default();
        // Don't hit the network from tests.
        st.leagues.lock().error = Some("offline".into());
        let sid = wm::id_of(&cfg, "settings").unwrap();
        wm::show(&mut cfg, sid);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0));
        let stash_id = wm::id_of(&cfg, "stash-search").unwrap();
        let strip_id = wm::id_of(&cfg, "image-strip").unwrap();
        let delve_id = wm::id_of(&cfg, "delve-grid").unwrap();
        let pages = [
            Page::Tools, Page::Hotkeys, Page::Chat, Page::General, Page::Appearance, Page::Widgets, Page::PriceCheck, Page::Maps,
            Page::ItemCheck, Page::ItemSearch, Page::Debug, Page::About,
        ];
        let mut rt_strip = crate::widgets::image_strip::ImageStripState::default();
        for (i, page) in pages.iter().chain([Page::StashSearch].iter()).enumerate() {
            if *page == Page::StashSearch {
                open_for(&mut cfg, stash_id);
            } else {
                st.page = *page;
            }
            for _ in 0..2 {
                let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut out = ctx.run_ui(input, |ui| {
                    let mut w = WCtx { cfg: &mut cfg, st: crate::wm::OverlayState { active: true, ..Default::default() }, i18n: &i18n, host: &mut host, services: &services, screen, save_requested: false };
                    show(ui.ctx(), &mut w, &mut st, sid);
                    if i == 0 {
                        crate::widgets::stash_search::show(ui.ctx(), &mut w, stash_id);
                        crate::widgets::image_strip::show(ui.ctx(), &mut w, &mut rt_strip, strip_id);
                        crate::widgets::delve_grid::show(ui.ctx(), &mut w, delve_id);
                    }
                });
                out.textures_delta.clear();
            }
            assert_eq!(st.page, *page, "page {page:?}");
        }
        assert!(st.target == Some(stash_id));
    }
}
