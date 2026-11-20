//! Port of upstream `trade/*.vue`: TradeListing, TradeBulk, OnlineFilter(+Core),
//! TradeLinks and RateLimiterState.

use egui::{Align, Color32, Layout, RichText, Sense, Ui, vec2};
use rpt_m3::{Button, TypeRole, icons, theme};
use rpt_parser::{ItemCategory, ParsedItem};
use rpt_trade::filters::{FilterOrGroup, ItemFilters};
use rpt_trade::trade::bulk::{BulkCurrency, BulkRow, merge_with_market_ratio};
use rpt_trade::trade::search::{GroupedResult, SHOW_RESULTS};
use rpt_trade::trade::{AccountStatus, RateLimitState, TradeApi};

use super::Env;
use super::session::{BulkState, ListingState, SearchError};
use super::ui::{Slowdown, bundled, harmonized};
use crate::config::ShowSeller;

/// What the trade components ask the session to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TradeAction {
    #[default]
    None,
    Retry,
    /// upstream `emit('reset')` after opening the trade site from the error box
    Reset,
}

fn status_color(ui: &Ui, s: AccountStatus) -> Color32 {
    let c = theme(ui.ctx()).colors;
    match s {
        AccountStatus::Online => c.success,
        AccountStatus::Afk => harmonized(ui, Color32::from_rgb(0xF9, 0x73, 0x16)).accent,
        AccountStatus::Offline => c.error,
    }
}

fn show_seller(env: &Env<'_>) -> Option<&'static str> {
    match &env.pc.show_seller {
        ShowSeller::Mode(m) if m == "ign" => Some("ign"),
        ShowSeller::Mode(m) if m == "account" => Some("account"),
        _ => None,
    }
}

fn you_badge(ui: &mut Ui, env: &Env<'_>) {
    let c = theme(ui.ctx()).colors;
    rpt_m3::tonal_badge(ui, &env.t("You"), c.tertiary_container, c.on_tertiary_container);
}

fn listed_cell(ui: &mut Ui, env: &Env<'_>, status: AccountStatus, relative: &str, is_mine: bool) {
    let th = theme(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let col = status_color(ui, status);
        rpt_m3::status_dot(ui, col, 7.0);
        ui.label(RichText::new(relative).font(th.font(TypeRole::BodySmall)).color(th.colors.on_surface_variant));
        if show_seller(env).is_none() && is_mine {
            you_badge(ui, env);
        }
    });
}

fn seller_cell(ui: &mut Ui, env: &Env<'_>, is_mine: bool, ign: &str, account: &str) {
    let th = theme(ui.ctx());
    if is_mine {
        you_badge(ui, env);
    } else {
        let name = if show_seller(env) == Some("ign") { ign } else { account };
        ui.label(RichText::new(name).font(th.font(TypeRole::BodySmall)).color(th.colors.on_surface_variant));
    }
}

fn error_box(ui: &mut Ui, env: &Env<'_>, err: &SearchError, link: Option<String>) -> TradeAction {
    let c = theme(ui.ctx()).colors;
    let mut action = TradeAction::None;
    let msg = if err.json { env.t("app.leagues_failed_help") } else { format!("Error: {}", err.message) };
    rpt_m3::banner(ui, icons::ERROR, format!("{}\n{}", env.t("trade_result.error"), msg), true);
    ui.horizontal(|ui| {
        if ui.add(Button::tonal(env.t("Retry")).icon(icons::REFRESH).small()).clicked() {
            action = TradeAction::Retry;
        }
        if let Some(l) = link {
            if ui.add(Button::text(env.t("Browser")).icon(icons::OPEN_IN_NEW).small()).clicked() {
                env.open_url(l);
                action = TradeAction::Reset;
            }
        }
    });
    let _ = c;
    action
}

/// `TradeLinks.vue` (the built-in browser is not available: always external).
pub fn trade_links(ui: &mut Ui, env: &Env<'_>, link: impl Fn() -> Option<String>) {
    if ui.add(Button::tonal(env.t("trade_result.trade")).icon(icons::OPEN_IN_NEW).small()).clicked() {
        if let Some(l) = link() {
            env.open_url(l);
        }
    }
    // Material Icons Round "bookmark_add"
    const BOOKMARK_ADD: &str = "\u{e598}";
    let save = rpt_m3::IconButton::new(BOOKMARK_ADD).size(28.0).tooltip("Save search to Market");
    if ui.add(save).clicked() {
        if let Some(l) = link() {
            env.save_search(l);
        }
    }
}

// ---------------------------------------------------------------------------
// OnlineFilter.vue

/// Button + popover with `OnlineFilterCore` (mutates `filters.trade`).
fn online_filter(ui: &mut Ui, env: &Env<'_>, filters: &mut ItemFilters, api: TradeApi) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let t = &mut filters.trade;
    let label_id = if t.offline {
        "Offline"
    } else if api == TradeApi::Trade && t.merchant_only {
        "online_filter.merchant_toggle"
    } else {
        "Online"
    };
    let mut label = env.t(label_id);
    let selected = env.sv.leagues.selected_id();
    if selected.as_deref() != Some(t.league.as_str()) {
        label.push_str(&format!(", {}", t.league));
    }
    let warn = t.show_warning();
    let warn_col = harmonized(ui, Color32::from_rgb(0xF9, 0x73, 0x16)).accent;
    let fg = if warn { warn_col } else { c.on_surface_variant };
    let resp = ui.add(
        egui::Button::new(RichText::new(format!("{} {}", icons::HISTORY, label)).color(fg).font(th.font(TypeRole::LabelLarge)))
            .frame(false)
            .truncate(),
    );
    let leagues = env.sv.leagues.list();
    rpt_m3::popover(&resp, |ui| {
        ui.set_min_width(320.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let mut off = t.offline;
                    if rpt_m3::switch(ui, &mut off).changed() {
                        t.set_offline(off, api);
                    }
                    ui.label(env.t("online_filter.offline_toggle"));
                });
                if api == TradeApi::Trade {
                    for (v, key) in [
                        (None, "online_filter.listed_any_time"),
                        (Some("1day"), "online_filter.listed_1day"),
                        (Some("3days"), "online_filter.listed_3days"),
                        (Some("1week"), "online_filter.listed_1week"),
                        (Some("2weeks"), "online_filter.listed_2weeks"),
                        (Some("1month"), "online_filter.listed_1month"),
                        (Some("2months"), "online_filter.listed_2months"),
                    ] {
                        let mut cur = t.listed.clone();
                        if rpt_m3::radio(ui, &mut cur, v.map(str::to_owned), env.t(key)).clicked() {
                            t.listed = v.map(str::to_owned);
                        }
                    }
                }
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                if !t.offline {
                    ui.horizontal(|ui| {
                        if api == TradeApi::Bulk {
                            rpt_m3::switch(ui, &mut t.online_in_league);
                            ui.label(env.t("online_filter.in_league_toggle"));
                        } else {
                            rpt_m3::switch(ui, &mut t.merchant_only);
                            ui.label(env.t("online_filter.merchant_toggle"));
                        }
                    });
                } else {
                    ui.add_space(32.0);
                }
                for l in &leagues {
                    let mut cur = t.league.clone();
                    if rpt_m3::radio(ui, &mut cur, l.id.clone(), l.id.as_str()).clicked() {
                        t.league = l.id.clone();
                    }
                }
                if api == TradeApi::Trade {
                    ui.add_space(8.0);
                    for (v, key) in [
                        (None, "online_filter.currency_any"),
                        (Some("chaos"), "online_filter.currency_only_chaos"),
                        (Some("divine"), "online_filter.currency_only_div"),
                        (Some("chaos_divine"), "online_filter.currency_chaos_div"),
                    ] {
                        let mut cur = t.currency.clone();
                        if rpt_m3::radio(ui, &mut cur, v.map(str::to_owned), env.t(key)).clicked() {
                            t.currency = v.map(str::to_owned);
                        }
                    }
                }
            });
        });
    });
}

fn legacy_message(ui: &mut Ui, env: &Env<'_>) {
    rpt_m3::banner(ui, icons::CURRENCY_EXCHANGE, env.t("trade_result.legacy_bulk_xchg_msg"), false);
    ui.add_space(4.0);
}

fn header_cell(ui: &mut Ui, text: &str) {
    let th = theme(ui.ctx());
    ui.label(RichText::new(text).font(th.font(TypeRole::LabelMedium)).color(th.colors.on_surface_variant));
}

fn placeholder_row(ui: &mut Ui, cols: usize) {
    for _ in 0..cols {
        ui.label(RichText::new(" ").font(theme(ui.ctx()).font(TypeRole::BodyMedium)));
    }
    ui.end_row();
}

// ---------------------------------------------------------------------------
// TradeListing.vue

pub fn trade_listing(
    ui: &mut Ui,
    env: &Env<'_>,
    item: &ParsedItem,
    filters: &mut ItemFilters,
    stats: &[FilterOrGroup],
    st: &ListingState,
    slowdown: &Slowdown,
) -> TradeAction {
    let th = theme(ui.ctx());
    let c = th.colors;
    let link = |filters: &ItemFilters| env.sv.trade.make_trade_link(filters, stats, st.search_id()).ok();
    if let Some(err) = &st.error {
        let l = env.sv.trade.make_trade_link(filters, stats, None).ok();
        return error_box(ui, env, err, l);
    }
    if item.info.exchangeable {
        legacy_message(ui, env);
    }
    ui.horizontal(|ui| {
        let matched = match &st.result {
            None => env.i18n.tf("trade_result.matched", &["..."]),
            Some(r) => env.i18n.tf(
                "trade_result.matched",
                &[&format!("{}{}", r.search_result.total, if r.search_result.inexact == Some(true) { "+" } else { "" })],
            ),
        };
        ui.label(RichText::new(matched).color(c.on_surface_variant));
        if st.result.is_some() {
            online_filter(ui, env, filters, TradeApi::Trade);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let l = link(filters);
                trade_links(ui, env, || l.clone());
            });
        }
    });
    if st.result.is_none() {
        rpt_m3::linear_progress(ui, None);
    }
    if let Some((msg, err)) = &env.travel_msg {
        rpt_m3::banner(ui, if *err { icons::ERROR } else { icons::INFO }, msg.clone(), *err);
    }
    let signed_in = env.sv.trade.has_session();

    let ready = slowdown.is_ready(ui.ctx(), 900);
    let grouped: Vec<GroupedResult> = if ready { st.result.as_ref().map(|r| r.grouped()).unwrap_or_default() } else { Vec::new() };
    let seller = show_seller(env);
    let has_stack = item.stack_size.is_some();
    let has_ilvl = filters.item_level.is_some();
    let is_gem = item.category == Some(ItemCategory::Gem);
    let has_q = filters.quality.is_some() || is_gem;
    let cols = 4 + has_stack as usize + has_ilvl as usize + is_gem as usize + has_q as usize + seller.is_some() as usize;
    egui::Grid::new(ui.scope_id().with("listing")).striped(true).min_row_height(24.0).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
        header_cell(ui, &env.t("trade_result.price"));
        if has_stack {
            header_cell(ui, &env.t("trade_result.stock"));
        }
        if has_ilvl {
            header_cell(ui, &env.t("trade_result.item_level"));
        }
        if is_gem {
            header_cell(ui, &env.t("trade_result.gem_level"));
        }
        if has_q {
            header_cell(ui, &env.t("trade_result.quality"));
        }
        header_cell(ui, &env.t("trade_result.listed"));
        if seller.is_some() {
            header_cell(ui, &env.t("trade_result.seller"));
        }
        header_cell(ui, "Sale");
        header_cell(ui, "Action");
        ui.end_row();
        for r in &grouped {
            let res = &r.result;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let text = format!("{} {}", rpt_trade::js::num_to_string(res.price_amount), res.price_currency);
                let (bg, fg) = if res.price_currency == "divine" { (harmonized(ui, Color32::from_rgb(0xE4, 0xC2, 0x9A)).container, harmonized(ui, Color32::from_rgb(0xE4, 0xC2, 0x9A)).on_container) } else { (c.secondary_container, c.on_secondary_container) };
                let b = rpt_m3::tonal_badge(ui, &text, bg, fg);
                if res.price_currency == "exalted" {
                    ui.painter().hline(b.rect.x_range(), b.rect.center().y, egui::Stroke::new(1.0, fg));
                }
                if r.listed_times > 2 {
                    rpt_m3::tonal_badge(ui, &format!("× {}", r.listed_times), c.surface_container_highest, c.on_surface_variant);
                } else if !res.has_fee {
                    let resp = ui.add(egui::Image::new(bundled("stash.png")).fit_to_exact_size(vec2(20.0, 20.0)).tint(Color32::from_white_alpha(120)));
                    if !res.has_note {
                        ui.painter().text(resp.rect.center(), egui::Align2::CENTER_CENTER, icons::QUESTION_MARK, super::filters::icon_font(14.0), c.on_surface);
                    }
                }
            });
            if has_stack {
                ui.label(res.stack_size.map(rpt_trade::js::num_to_string).unwrap_or_default());
            }
            if has_ilvl {
                ui.label(res.item_level.clone().unwrap_or_default());
            }
            if is_gem {
                ui.label(res.level.clone().unwrap_or_default());
            }
            if has_q {
                ui.label(RichText::new(res.quality.clone().unwrap_or_default()).color(c.primary));
            }
            listed_cell(ui, env, res.account_status, &res.relative_date, res.is_mine);
            if seller.is_some() {
                seller_cell(ui, env, res.is_mine, &res.ign, &res.account_name);
            }
            sale_cells(ui, env, res, signed_in);
            ui.end_row();
        }
        for _ in grouped.len()..SHOW_RESULTS {
            placeholder_row(ui, cols);
        }
    });
    TradeAction::None
}

/// "Sale" (instant buyout vs in person) and "Action" (Travel / Whisper) cells.
fn sale_cells(ui: &mut Ui, env: &Env<'_>, res: &rpt_trade::trade::search::PricingResult, signed_in: bool) {
    let c = theme(ui.ctx()).colors;
    const HOME: &str = "\u{e88a}"; // Material Icons Round "home"
    let sign_in = "Sign in under Settings → Account so listings include the seller's whisper and hideout details.";
    if res.instant_buyout {
        rpt_m3::tonal_badge(ui, "Instant buyout", c.primary_container, c.on_primary_container)
            .on_hover_text("Listed in a merchant tab: buy it in the seller's hideout without trading in person.");
        let tip = match (&res.hideout_token, &res.stash) {
            (Some(_), Some(s)) => format!("Travel to the seller's hideout. The item is highlighted in tab \"{}\" (left {}, top {}).", s.name, s.x + 1, s.y + 1),
            (Some(_), None) => "Travel to the seller's hideout. The item is highlighted in their stash.".to_owned(),
            (None, _) if !signed_in => sign_in.to_owned(),
            (None, _) => "pathofexile.com didn't send a hideout link for this listing.".to_owned(),
        };
        let enabled = !res.is_mine && (res.hideout_token.is_some() || !signed_in);
        let label = if signed_in { "Travel" } else { "Sign in" };
        if ui.add(Button::tonal(label).icon(HOME).small().enabled(enabled)).on_hover_text(tip).on_disabled_hover_text("No hideout link for this listing.").clicked() {
            match &res.hideout_token {
                Some(t) => env.travel(t.clone()),
                None => env.open_account(),
            }
        }
    } else {
        rpt_m3::tonal_badge(ui, "In person", c.surface_container_highest, c.on_surface_variant)
            .on_hover_text("Whisper the seller and trade in person.");
        let enabled = !res.is_mine && (res.whisper.is_some() || !signed_in);
        let tip = res.whisper.clone().unwrap_or_else(|| if signed_in { "The seller's character name is unknown for this listing.".into() } else { sign_in.into() });
        let label = if signed_in || res.whisper.is_some() { "Whisper" } else { "Sign in" };
        if ui.add(Button::tonal(label).icon(icons::CHAT).small().enabled(enabled)).on_hover_text(&tip).on_disabled_hover_text(tip.clone()).clicked() {
            match &res.whisper {
                Some(w) => env.whisper(w.clone()),
                None => env.open_account(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TradeBulk.vue

pub fn trade_bulk(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem, filters: &mut ItemFilters, st: &mut BulkState, slowdown: &Slowdown) -> TradeAction {
    let th = theme(ui.ctx());
    let c = th.colors;
    let have_for = |sel: BulkCurrency| vec![if sel == BulkCurrency::XchgStable { "divine" } else { "chaos" }.to_owned()];
    if let Some(err) = &st.error {
        let l = env.sv.trade.make_bulk_trade_link(filters, item, &["mirror".to_owned()]).ok();
        return error_box(ui, env, err, l);
    }
    legacy_message(ui, env);
    ui.horizontal(|ui| {
        ui.label(RichText::new(env.i18n.tf("trade_result.matched", &[""]).trim_end()).color(c.on_surface_variant));
        match &st.results {
            None => {
                ui.label(RichText::new("...").color(c.outline));
            }
            Some(res) => {
                let total = |cur: BulkCurrency| res.get(cur).map(|r| r.total.to_string()).unwrap_or_else(|| "?".into());
                for (cur, img) in [(BulkCurrency::XchgChaos, "chaos.png"), (BulkCurrency::XchgStable, "divine.png")] {
                    let sel = st.selected == cur;
                    let resp = ui
                        .scope(|ui| {
                            egui::Frame::new()
                                .fill(if sel { c.secondary_container } else { Color32::TRANSPARENT })
                                .stroke(egui::Stroke::new(1.0, if sel { Color32::TRANSPARENT } else { c.outline_variant }))
                                .corner_radius(egui::CornerRadius::same(rpt_m3::shape::SMALL))
                                .inner_margin(egui::Margin::symmetric(6, 2))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    ui.horizontal(|ui| {
                                        ui.add(egui::Image::new(bundled(img)).fit_to_exact_size(vec2(22.0, 22.0)));
                                        ui.label(RichText::new(total(cur)).color(if sel { c.on_secondary_container } else { c.on_surface_variant }));
                                    });
                                })
                                .response
                        })
                        .inner
                        .interact(Sense::click());
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        st.selected = cur;
                    }
                }
                online_filter(ui, env, filters, TradeApi::Bulk);
            }
        }
        if st.results.is_some() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let l = env.sv.trade.make_bulk_trade_link(filters, item, &have_for(st.selected)).ok();
                trade_links(ui, env, || l.clone());
            });
        }
    });
    if st.results.is_none() {
        rpt_m3::linear_progress(ui, None);
    }

    let ready = slowdown.is_ready(ui.ctx(), 900);
    let rows: Vec<BulkRow> = match (&st.results, ready) {
        (Some(res), true) => {
            let listed = res.get(st.selected).map(|r| r.listed.clone()).unwrap_or_default();
            let ratio = match st.selected {
                BulkCurrency::XchgChaos => st.ratio.xchg_chaos,
                BulkCurrency::XchgStable => st.ratio.xchg_stable,
            };
            if res.get(st.selected).is_none() && ratio.is_none() { Vec::new() } else { merge_with_market_ratio(&listed, ratio) }
        }
        _ => Vec::new(),
    };
    let seller = show_seller(env);
    let cols = 5 + seller.is_some() as usize;
    let fmt = |v: f64, d: u32| rpt_trade::js::num_to_string(rpt_trade::js::to_fixed(v, d).parse().unwrap_or(v));
    egui::Grid::new(ui.scope_id().with("bulk")).striped(true).min_row_height(24.0).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
        header_cell(ui, &env.t("trade_result.price"));
        header_cell(ui, &format!("{} / {}", if st.selected == BulkCurrency::XchgChaos { "chaos" } else { "div" }, env.t("trade_result.bulk")));
        header_cell(ui, &env.t("trade_result.stock"));
        header_cell(ui, &env.t("trade_result.fulfill"));
        header_cell(ui, &env.t("trade_result.listed"));
        if seller.is_some() {
            header_cell(ui, &env.t("trade_result.seller"));
        }
        ui.end_row();
        for row in rows.iter().take(20) {
            match row {
                BulkRow::MarketRatio(m) => {
                    let (bg, fg) = (c.tertiary_container, c.on_tertiary_container);
                    rpt_m3::tonal_badge(ui, &fmt(m.exchange_amount / m.item_amount, 4), bg, fg);
                    ui.label(RichText::new(format!("{}\u{2009}/\u{2009}{}", fmt(m.exchange_amount, 1), fmt(m.item_amount, 1))).color(fg));
                    rpt_m3::tonal_badge(ui, &env.t("trade_result.market_ratio"), bg, fg);
                    ui.label("");
                    ui.label("");
                    if seller.is_some() {
                        ui.label("");
                    }
                }
                BulkRow::Listing(r) => {
                    rpt_m3::tonal_badge(ui, &fmt(r.exchange_amount / r.item_amount, 4), c.secondary_container, c.on_secondary_container);
                    ui.label(format!("{}\u{2009}/\u{2009}{}", fmt(r.exchange_amount, 1), fmt(r.item_amount, 1)));
                    ui.label(rpt_trade::js::num_to_string(r.stock));
                    ui.horizontal(|ui| {
                        if r.stock < r.item_amount {
                            ui.label(rpt_m3::icon_text(icons::WARNING, 14.0).color(c.outline));
                        }
                        ui.label(rpt_trade::js::num_to_string((r.stock / r.item_amount).floor()));
                    });
                    listed_cell(ui, env, r.account_status, &r.relative_date, r.is_mine);
                    if seller.is_some() {
                        seller_cell(ui, env, r.is_mine, &r.ign, &r.account_name);
                    }
                }
            }
            ui.end_row();
        }
        for _ in rows.len().min(20)..20 {
            placeholder_row(ui, cols);
        }
    });
    TradeAction::None
}

// ---------------------------------------------------------------------------
// RateLimiterState.vue

/// Toggle button + details card. Returns the new `showRateLimitState`.
pub fn rate_limiter(ui: &mut Ui, state: &RateLimitState, open: bool) -> bool {
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut open = open;
    let limited = state.is_limited();
    if open {
        egui::Frame::new()
            .fill(c.surface_container_high)
            .corner_radius(egui::CornerRadius::same(rpt_m3::shape::LARGE))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                for l in &state.limits {
                    ui.label(RichText::new(format!("Policy: {}", l.policy)).color(if l.has_queue { c.error } else { c.on_surface }));
                    for r in &l.rules {
                        let mut s = format!("{} / {} over {}s", r.active, r.max, rpt_trade::js::num_to_string(r.window));
                        if r.queue > 0 {
                            s.push_str(&format!(" (queue: {})", r.queue));
                        }
                        ui.label(RichText::new(s).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));
                    }
                }
            });
    }
    let btn = if limited { Button::new(rpt_m3::ButtonKind::Danger, "Rate limiting") } else { Button::tonal("Rate limiting") };
    if ui.add(btn.icon(icons::SPEED).small()).clicked() {
        open = !open;
    }
    if limited {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
    open
}
