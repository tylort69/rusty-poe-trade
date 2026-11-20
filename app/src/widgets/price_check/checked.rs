//! View of upstream `CheckedItem.vue`.

use egui::{Align, Layout, RichText, Ui};
use rpt_m3::{Button, icons, theme};
use rpt_trade::check;
use rpt_trade::trade::TradeApi;

use super::Env;
use super::extras;
use super::filters;
use super::session::{PredictionState, Session};
use super::trade::{self, TradeAction};

pub fn checked_item(ui: &mut Ui, env: &Env<'_>, s: &mut Session, league: &str, show_support_links: bool) {
    if !check::show_checked_item(&s.item) {
        return;
    }
    let th = theme(ui.ctx());
    let c = th.colors;
    let tc = env.sv.trade.config();

    if let Some(p) = s.presets.active_preset_mut() {
        filters::filter_name(ui, env, &s.item, &mut p.filters);
    }
    ui.add_space(8.0);

    let popular = env.sv.leagues.selected().is_some_and(|l| l.is_popular);
    if check::show_predicted_price(&s.item, &s.presets, &tc, popular) {
        if s.prediction.is_none() {
            s.prediction = Some(PredictionState::start(env.sv, &s.item, league));
        }
        if let Some(p) = &mut s.prediction {
            extras::price_prediction(ui, env, &s.item, p, &s.slowdown, league);
        }
    } else if let Some(f) = s.active_filters() {
        extras::price_trend_view(ui, env, &s.item, f, &s.slowdown);
    }

    let fo = filters::filters_block(ui, env, s);
    if fo.submit {
        s.start_search(env.sv);
    }
    ui.add_space(8.0);

    let mut action = TradeAction::None;
    if s.do_search {
        let Session { item, presets, listing, bulk, slowdown, trade_api, .. } = s;
        if let Some(p) = presets.active_preset_mut() {
            match trade_api {
                TradeApi::Trade => {
                    if let Some(l) = listing {
                        action = trade::trade_listing(ui, env, item, &mut p.filters, &p.stats, l, slowdown);
                    }
                }
                TradeApi::Bulk => {
                    if let Some(b) = bulk {
                        action = trade::trade_bulk(ui, env, item, &mut p.filters, b, slowdown);
                    }
                }
            }
        }
    } else {
        ui.horizontal(|ui| {
            let r = ui.add(Button::filled(env.t("Search")).icon(icons::SEARCH));
            if r.clicked() {
                action = TradeAction::Retry;
            } else if r.hovered() && s.pointer_in_filters {
                // `handleSearchMouseenter`: moving from the filters onto Search starts it
                action = TradeAction::Retry;
                ui.ctx().memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
            if s.trade_api == TradeApi::Trade {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let link = s.presets.active_preset().and_then(|p| env.sv.trade.make_trade_link(&p.filters, &p.stats, None).ok());
                    trade::trade_links(ui, env, || link.clone());
                });
            }
        });
    }
    s.pointer_in_filters = fo.rect.is_some_and(|r| ui.rect_contains_pointer(r));
    match action {
        TradeAction::Retry => s.start_search(env.sv),
        TradeAction::Reset => s.reset_results(),
        TradeAction::None => {}
    }

    if check::show_complexity_hint(&s.item, s.do_search, &tc) {
        ui.add_space(8.0);
        rpt_m3::banner(ui, icons::INFO, env.t("item.complexity_hint"), false);
    }
    if let Some(f) = s.active_filters() {
        extras::stack_value(ui, env, &s.item, f);
    }
    extras::dust_value(ui, env, &s.item);

    if show_support_links {
        ui.add_space(12.0);
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, c.outline_variant))
            .corner_radius(egui::CornerRadius::same(rpt_m3::shape::MEDIUM))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(env.t("Support development on"));
                    if ui.add(Button::text("Patreon").icon(icons::FAVORITE).small()).clicked() {
                        env.open_url("https://patreon.com/awakened_poe_trade".into());
                    }
                });
                let tpl = env.t("app.thanks_3rd_party");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let mut rest = tpl.as_str();
                    for (ph, name, url) in [("{0}", "poeprices.info", "https://poeprices.info"), ("{1}", "poe.ninja", "https://poe.ninja/support")] {
                        if let Some((a, b)) = rest.split_once(ph) {
                            if !a.trim().is_empty() {
                                ui.label(RichText::new(a.trim()).color(c.on_surface_variant));
                            }
                            if ui.add(Button::tonal(name).small()).clicked() {
                                env.open_url(url.into());
                            }
                            rest = b;
                        }
                    }
                    if !rest.trim().is_empty() {
                        ui.label(RichText::new(rest.trim()).color(c.on_surface_variant));
                    }
                });
            });
    }
}
