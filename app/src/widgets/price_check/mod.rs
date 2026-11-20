//! Price check. Port of upstream `price-check/PriceCheckWindow.vue` and
//! everything below it (CheckedItem, filters, trade listing/bulk, trends,
//! related items, unidentified resolver, price prediction, …).
//!
//! The window is an exclusive full-height surface placed next to the game's
//! stash or inventory panel (whichever side the item was checked on). In
//! "hold" mode (`hotkeyHold` + `hotkey`) the platform tracks the area and
//! closes the widget when the hold key is released or the cursor leaves it.

mod checked;
mod extras;
mod filters;
pub mod position;
mod session;
mod trade;
pub mod ui;

use std::cell::RefCell;

use egui::{Align, Align2, Area, Context, CornerRadius, Frame, Id, Layout, Margin, Order, Pos2, RichText, Shadow, Stroke};
use rpt_m3::{IconButton, TypeRole, icons, theme, with_alpha};
use rpt_parser::{ItemCategory, ItemRarity, ParsedItem};

use super::WCtx;
use crate::config::{PriceCheckWidget, WidgetKind};
use crate::host::{HostCommand, HostEvent};
use crate::i18n::I18n;
use crate::services::Services;
use crate::wm;
use position::Side;
use session::Session;

/// Upstream window width is `28.75rem` (rem = config font size).
pub const WINDOW_REM: f32 = 28.75;

/// Drawing context shared by the price-check components.
pub struct Env<'a> {
    pub i18n: &'a I18n,
    pub sv: &'a Services,
    pub pc: &'a PriceCheckWidget,
    commands: RefCell<Vec<HostCommand>>,
}

impl Env<'_> {
    pub fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }
    pub fn open_url(&self, url: String) {
        self.commands.borrow_mut().push(HostCommand::OpenUrl(url));
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub name: String,
    pub message: String,
    pub raw_text: String,
}

#[derive(Default)]
pub struct PriceCheckState {
    item: Option<Result<ParsedItem, ParseError>>,
    item_gen: u64,
    session_gen: u64,
    session: Option<Session>,
    advanced_check: bool,
    /// overlay-local position of the checked item
    check_position: Pos2,
    side: Side,
    /// upstream module-level `_showSupportLinksCounter`
    support_counter: u32,
    show_support_links: bool,
    last_do_search: Option<bool>,
}

/// Parse the clipboard like `PriceCheckWindow.vue` (sentinels are unsupported).
pub fn parse_item(clipboard: &str) -> Result<ParsedItem, ParseError> {
    rpt_parser::parse_clipboard(clipboard)
        .and_then(|item| {
            if item.category == Some(ItemCategory::Sentinel) && item.rarity != Some(ItemRarity::Unique) {
                Err("item.unknown".to_owned())
            } else {
                Ok(item)
            }
        })
        .map_err(|e| ParseError { message: format!("{e}_help"), name: e, raw_text: clipboard.to_owned() })
}

fn price_check_mut(cfg: &mut crate::config::Config) -> Option<&mut PriceCheckWidget> {
    cfg.widgets.iter_mut().find_map(|w| match &mut w.kind {
        WidgetKind::PriceCheck(p) => Some(p.as_mut()),
        _ => None,
    })
}

/// `MAIN->CLIENT::item-text` handler.
pub fn on_item_text(w: &mut WCtx<'_>, st: &mut PriceCheckState, ev: &HostEvent) {
    let HostEvent::ItemText { target, clipboard, position, focus_overlay } = ev else { return };
    if target != "price-check" {
        return;
    }
    let Some(id) = wm::id_of(w.cfg, "price-check") else { return };
    let ctx = w.services.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let origin = ctx.input(|i| i.viewport().inner_rect.or(i.viewport().outer_rect)).map(|r| r.min).unwrap_or(Pos2::ZERO);
    let abs = Pos2::new(position.x / ppp, position.y / ppp);
    let local = abs - origin.to_vec2();

    if !*focus_overlay {
        let width = WINDOW_REM * w.cfg.font_size;
        let area = position::track_area(w.screen, local, width).translate(origin.to_vec2());
        w.host.push(HostCommand::TrackArea {
            hold_key: w.cfg.price_check().hotkey_hold.clone(),
            // platform expects physical pixels for the threshold
            close_threshold: 2.5 * w.cfg.font_size * ppp,
            from: abs,
            area,
            dpr: ppp,
        });
    }
    wm::show(w.cfg, id);
    st.check_position = local;
    st.side = position::click_side(local, w.screen);
    st.advanced_check = *focus_overlay;
    let parsed = parse_item(clipboard);
    if parsed.is_ok() {
        w.services.queue_prices_fetch();
    }
    st.item = Some(parsed);
    st.item_gen += 1;
}

/// Price check an item picked in another widget (upstream item search
/// self-dispatches `item-text` with `focusOverlay: true`).
pub fn open_item(w: &mut WCtx<'_>, st: &mut PriceCheckState, item: ParsedItem, local_pos: Pos2) {
    let Some(id) = wm::id_of(w.cfg, "price-check") else { return };
    wm::show(w.cfg, id);
    st.check_position = local_pos;
    st.side = position::click_side(local_pos, w.screen);
    st.advanced_check = true;
    w.services.queue_prices_fetch();
    st.item = Some(Ok(item));
    st.item_gen += 1;
}

fn open_league_selection(w: &mut WCtx<'_>, wm_id: u32) {
    if let Some(settings) = wm::id_of(w.cfg, "settings") {
        wm::set_flag(w.cfg, settings, &format!("settings::widget={wm_id}"), true);
        wm::show(w.cfg, settings);
    }
}

fn close_price_check(w: &mut WCtx<'_>, wm_id: u32) {
    wm::hide(w.cfg, wm_id);
    if w.st.active {
        w.host.push(HostCommand::FocusGame);
    }
}

/// Keep sessions in sync with the current item and the selected league.
fn sync_session(w: &WCtx<'_>, st: &mut PriceCheckState) {
    let Some(Ok(item)) = &st.item else {
        st.session = None;
        return;
    };
    if st.session_gen == st.item_gen && st.session.is_some() {
        return;
    }
    let Some(league) = w.services.leagues.selected_id() else { return };
    let cfg = w.services.trade.config();
    let s = Session::new(item.clone(), st.advanced_check, st.session.as_ref(), &cfg, &league);
    st.session = Some(s);
    st.session_gen = st.item_gen;
    update_support_links(st, true);
}

/// `showSupportLinks` watcher on `[item, doSearch]`.
fn update_support_links(st: &mut PriceCheckState, item_changed: bool) {
    let Some(s) = &st.session else { return };
    if st.support_counter >= 13 && (!s.do_search || s.trade_api == rpt_trade::trade::TradeApi::Bulk) {
        st.show_support_links = true;
        st.support_counter = 0;
    } else {
        st.show_support_links = false;
        if item_changed {
            st.support_counter += 1;
        }
    }
    st.last_do_search = Some(s.do_search);
}

pub fn show(ctx: &Context, w: &mut WCtx<'_>, st: &mut PriceCheckState, wm_id: u32) {
    sync_session(w, st);
    if let Some(s) = &mut st.session {
        s.poll(w.services);
    }
    if st.session.as_ref().map(|s| Some(s.do_search) != st.last_do_search).unwrap_or(false) {
        update_support_links(st, false);
    }

    let th = theme(ctx);
    let c = th.colors;
    let screen = w.screen;
    let pc = w.cfg.price_check().clone();
    let width = (WINDOW_REM * w.cfg.font_size).min(screen.width());
    let win = position::window_rect(screen, st.side, width);
    let env = Env { i18n: w.i18n, sv: w.services, pc: &pc, commands: RefCell::new(Vec::new()) };

    // CheckPositionCircle
    if w.st.active && pc.show_cursor {
        let p = ctx.layer_painter(egui::LayerId::new(Order::Middle, Id::unique("rpt-price-check-pos")));
        let center = st.check_position;
        p.circle(center, 40.0, with_alpha(c.inverse_surface, 0.35), Stroke::new(1.0, with_alpha(c.shadow, 0.8)));
        p.circle_filled(center, 10.0, with_alpha(c.inverse_surface, 0.5));
    }

    let mut close = false;
    let mut open_settings = false;
    let mut identified: Option<ParsedItem> = None;
    let league_title = w.services.leagues.selected_id();

    Area::new(Id::unique(("rpt-price-check", wm_id)))
        .order(Order::Foreground)
        .fixed_pos(win.min)
        .constrain(false)
        .interactable(w.st.active)
        .show(ctx, |ui| {
            Frame::new()
                .fill(c.surface_container_high)
                .corner_radius(CornerRadius::same(rpt_m3::shape::EXTRA_LARGE))
                .shadow(Shadow { offset: [0, 8], blur: 24, spread: 0, color: with_alpha(c.shadow, 0.45) })
                .show(ui, |ui| {
                    ui.set_width(win.width());
                    ui.set_height(win.height());
                    // --- title bar (AppTitleBar)
                    Frame::new().inner_margin(Margin { left: 20, right: 8, top: 8, bottom: 4 }).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_min_height(40.0);
                            let title = league_title.clone().unwrap_or_else(|| "Rusty POE Trade".into());
                            let t = ui.add(egui::Label::new(RichText::new(title).font(th.font(TypeRole::TitleMedium)).color(c.on_surface)).sense(egui::Sense::click()));
                            if t.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(env.t("league")).clicked() {
                                open_settings = true;
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(IconButton::new(icons::CLOSE).size(36.0)).clicked() {
                                    close = true;
                                }
                                match w.services.prices.xchg_rate() {
                                    Some(rate) => {
                                        let cost = rate.round();
                                        let r = ui.add(rpt_m3::Button::text(rpt_m3::fmt_number(cost)).icon(icons::SWAP_HORIZ).small());
                                        let r = r.on_hover_text("Divine Orb");
                                        rpt_m3::popover(&r, |ui| {
                                            ui::quick_price(
                                                ui,
                                                Some(&rpt_trade::prices::CurrencyValue { min: cost, max: cost, currency: rpt_trade::prices::PriceCurrency::Chaos }),
                                                &ui::QuickPrice { item_bundled: Some("divine.png"), ..Default::default() },
                                            );
                                            for i in 1..=9 {
                                                ui.label(format!("{} div ⇒ {} c", rpt_m3::fmt_number(i as f64 / 10.0), (cost * i as f64 / 10.0).round()));
                                            }
                                        });
                                    }
                                    None if w.services.prices.initial_loading() => {
                                        rpt_m3::circular_progress(ui, 20.0);
                                    }
                                    None => {}
                                }
                            });
                        });
                    });
                    // --- body
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        Frame::new().inner_margin(Margin { left: 16, right: 16, top: 4, bottom: 16 }).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            extras::background_info(ui, &env);
                            match &st.item {
                                Some(Err(e)) => {
                                    rpt_m3::banner(ui, icons::ERROR, format!("{}\n{}", env.t(&e.name), env.t(&e.message)), true);
                                    ui.add_space(8.0);
                                    Frame::new().fill(c.surface_container_lowest).corner_radius(CornerRadius::same(rpt_m3::shape::SMALL)).inner_margin(Margin::same(8)).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.add(egui::Label::new(RichText::new(&e.raw_text).monospace()).wrap());
                                    });
                                }
                                Some(Ok(item)) => {
                                    identified = extras::unidentified_resolver(ui, &env, item);
                                    if let (Some(s), Some(league)) = (&mut st.session, league_title.as_deref()) {
                                        checked::checked_item(ui, &env, s, league, st.show_support_links);
                                    }
                                }
                                None => {}
                            }
                        });
                    });
                });
        });

    // related items + rate limiter in the free column next to the window
    let col = position::side_column(screen, win, st.side);
    if col.width() > 48.0 {
        let (pivot, x) = match st.side {
            Side::Stash => (Align2::LEFT_TOP, col.left()),
            Side::Inventory => (Align2::RIGHT_TOP, col.right()),
        };
        if let Some(Ok(item)) = &st.item {
            Area::new(Id::unique(("rpt-price-check-related", wm_id)))
                .order(Order::Foreground)
                .pivot(pivot)
                .fixed_pos(Pos2::new(x, col.top() + 24.0))
                .interactable(w.st.active)
                .show(ctx, |ui| extras::related_items(ui, &env, item));
        }
        let (pivot, x) = match st.side {
            Side::Stash => (Align2::LEFT_BOTTOM, col.left() + 24.0),
            Side::Inventory => (Align2::RIGHT_BOTTOM, col.right() - 24.0),
        };
        let mut open = pc.show_rate_limit_state;
        Area::new(Id::unique(("rpt-price-check-ratelimit", wm_id)))
            .order(Order::Foreground)
            .pivot(pivot)
            .fixed_pos(Pos2::new(x, col.bottom() - 8.0))
            .interactable(w.st.active)
            .show(ctx, |ui| {
                ui.with_layout(Layout::top_down(if st.side == Side::Stash { Align::Min } else { Align::Max }), |ui| {
                    open = trade::rate_limiter(ui, &w.services.trade.rate_limit_state(), open);
                });
            });
        if open != pc.show_rate_limit_state {
            if let Some(p) = price_check_mut(w.cfg) {
                p.show_rate_limit_state = open;
                w.save_requested = true;
            }
        }
    }

    for cmd in env.commands.into_inner() {
        w.host.push(cmd);
    }
    if let Some(s) = &mut st.session {
        s.after_frame(w.services);
    }
    if let Some(item) = identified {
        st.item = Some(Ok(item));
        st.item_gen += 1;
    }
    if open_settings {
        open_league_selection(w, wm_id);
    }
    if close {
        close_price_check(w, wm_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_errors() {
        rpt_data::init("en").unwrap();
        let e = parse_item("garbage").unwrap_err();
        assert!(e.message.ends_with("_help"));
        assert_eq!(e.raw_text, "garbage");
        let ok = parse_item("Item Class: Stackable Currency\nRarity: Currency\nDivine Orb\n--------\nStack Size: 1/20\n");
        assert!(ok.is_ok());
    }
}
