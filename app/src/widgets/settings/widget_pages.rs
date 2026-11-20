//! Per-widget settings pages: price check, map check, item info, item search,
//! stopwatch and the stash-search editor.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use rpt_m3::{Button, CardKind, Chip, IconButton, TypeRole, icons, shape, theme, with_alpha};

use super::hotkey::{hotkey_input, hotkey_row};
use super::{SettingsState, field_label, leagues, switch_setting};
use crate::config::{MapCheckStat, ShowSeller, StashSearchEntry, WidgetKind};
use crate::widgets::WCtx;

// ---------------------------------------------------------------------------
// Price check (upstream price-check/settings-price-check.vue)

pub fn price_check(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let i18n = w.i18n;

    // --- League
    let lg = st.leagues.lock().clone();
    if lg.list.is_empty() && !lg.loading && lg.error.is_none() {
        leagues::load(&st.leagues, &w.services.rt, w.cfg.poe_web_api(), ui.ctx().clone());
    }
    if lg.just_loaded {
        st.leagues.lock().just_loaded = false;
        leagues::auto_select(&lg.list, &mut w.cfg.league_id);
    }
    ui.horizontal(|ui| {
        rpt_m3::section_header(ui, i18n.t("league"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(Button::text(i18n.t("Refresh")).icon(icons::REFRESH).small().enabled(!lg.loading)).clicked() {
                leagues::load(&st.leagues, &w.services.rt, w.cfg.poe_web_api(), ui.ctx().clone());
            }
        });
    });
    if let Some(err) = &lg.error {
        rpt_m3::card(ui, CardKind::Filled).fill(c.error_container).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(i18n.t("app.leagues_failed")).font(th.font(TypeRole::TitleSmall)).color(c.on_error_container));
            ui.label(RichText::new(i18n.t("app.leagues_failed_help_alt")).color(c.on_error_container));
            ui.label(RichText::new(err).small().color(c.on_error_container));
            if ui.add(Button::tonal(i18n.t("Retry")).icon(icons::REFRESH).small()).clicked() {
                leagues::load(&st.leagues, &w.services.rt, w.cfg.poe_web_api(), ui.ctx().clone());
            }
        });
    } else if lg.loading {
        ui.horizontal(|ui| {
            rpt_m3::circular_progress(ui, 20.0);
            ui.label(i18n.t("app.leagues_loading"));
        });
    } else if !lg.list.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for l in &lg.list {
                let sel = w.cfg.league_id.as_deref() == Some(l.as_str());
                if ui.add(Chip::filter(l.as_str(), sel)).clicked() {
                    w.cfg.league_id = Some(l.clone());
                }
            }
        });
        ui.add_space(8.0);
        field_label(ui, &i18n.t("settings.private_league"));
        let mut custom = w.cfg.league_id.clone().filter(|id| !lg.list.contains(id)).unwrap_or_default();
        if rpt_m3::text_field(ui, &mut custom, "My League (PL12345)").changed() {
            w.cfg.league_id = Some(custom);
        }
    }

    // --- Account
    rpt_m3::section_header(ui, i18n.t("settings.account_name"));
    rpt_m3::text_field(ui, &mut w.cfg.account_name, "");

    let Some(pc) = w.cfg.widgets.iter_mut().find_map(|x| match &mut x.kind {
        WidgetKind::PriceCheck(p) => Some(p.as_mut()),
        _ => None,
    }) else {
        return;
    };

    rpt_m3::section_header(ui, i18n.t("price_check.show_seller"));
    let mut seller = match &pc.show_seller {
        ShowSeller::Mode(m) => m.clone(),
        ShowSeller::Off(_) => String::new(),
    };
    let (no, acc, ign) = (i18n.t("No"), i18n.t("settings.account_name"), i18n.t("settings.last_char_name"));
    if rpt_m3::segmented(ui, &mut seller, &[(String::new(), no.as_str()), ("account".into(), acc.as_str()), ("ign".into(), ign.as_str())]) {
        pc.show_seller = if seller.is_empty() { ShowSeller::Off(false) } else { ShowSeller::Mode(seller) };
    }
    ui.label(RichText::new(i18n.t("price_check.highlight_hint")).italics().color(c.on_surface_variant));

    rpt_m3::setting_row(ui, None, &i18n.t("price_check.fill_rolls"), None, |ui| {
        let exact = pc.search_stat_range == 0.0;
        if ui.add(Chip::filter(i18n.t("price_check.fill_roll_exact"), exact)).clicked() && !exact {
            pc.search_stat_range = 0.0;
        }
        rpt_m3::number_field(ui, &mut pc.search_stat_range, 0.0..=50.0, 0.2, 0, " %");
        ui.label("±");
    });
    switch_setting(ui, &i18n.t("price_check.select_stock"), None, &mut pc.activate_stock_filter);
    switch_setting(ui, &i18n.t("price_check.show_prediction"), Some("www.poeprices.info"), &mut pc.request_price_prediction);
    switch_setting(ui, &i18n.t("price_check.cursor_pos"), None, &mut pc.show_cursor);

    // --- Default trade filters (upstream OnlineFilterCore in "settings" context)
    ui.add_space(8.0);
    rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(i18n.t("price_check.default_settings")).font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
        switch_setting(ui, &i18n.t("online_filter.merchant_toggle"), None, &mut pc.merchant_only);
        ui.horizontal_wrapped(|ui| {
            let opts: [(Option<&str>, &str); 4] = [
                (None, "online_filter.currency_any"),
                (Some("chaos"), "online_filter.currency_only_chaos"),
                (Some("divine"), "online_filter.currency_only_div"),
                (Some("chaos_divine"), "online_filter.currency_chaos_div"),
            ];
            for (v, key) in opts {
                let v = v.map(str::to_owned);
                let sel = pc.default_currency == v;
                if ui.add(Chip::filter(i18n.t(key), sel)).clicked() {
                    pc.default_currency = v;
                }
            }
        });
    });

    // --- Builtin browser
    ui.add_space(8.0);
    switch_setting(ui, &i18n.t("price_check.enable_browser"), None, &mut pc.builtin_browser);
    if pc.builtin_browser {
        rpt_m3::banner(ui, icons::WARNING, i18n.t("price_check.builtin_browser_warning"), true);
    }

    // --- Expensive options
    ui.add_space(8.0);
    rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
        ui.set_width(ui.available_width());
        rpt_m3::banner(ui, icons::WARNING_AMBER, i18n.t("price_check.warn_expensive"), false);
        let mut accurate = pc.collapse_listings == "app";
        if switch_setting(ui, &i18n.t("price_check.accurate_collapsed"), None, &mut accurate) {
            pc.collapse_listings = if accurate { "app" } else { "api" }.into();
        }
        field_label(ui, &i18n.t("price_check.auto_search"));
        ui.horizontal_wrapped(|ui| {
            if let Some(k) = &pc.hotkey {
                rpt_m3::switch(ui, &mut pc.smart_initial_search);
                rpt_m3::key_cap(ui, &format!("{} + {}", pc.hotkey_hold, k));
                ui.add_space(16.0);
            }
            if let Some(k) = pc.hotkey_locked.clone() {
                rpt_m3::switch(ui, &mut pc.locked_initial_search);
                rpt_m3::key_cap(ui, &k);
            }
        });
        let label = i18n.t("price_check.extra_delay");
        let secs = i18n.t("seconds");
        rpt_m3::setting_row(ui, None, &label, None, |ui| {
            rpt_m3::number_field(ui, &mut pc.api_latency_seconds, 0.5..=10.0, 0.05, 1, &format!(" {secs}"));
        });
    });
}

// ---------------------------------------------------------------------------
// Map check (upstream map-check/settings-maps.vue + SettingsMatcherEntry.vue)

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tag {
    None,
    UberMap,
    Heist,
    Outdated,
}

#[derive(Default)]
pub struct MapsState {
    search: String,
    only_selected: bool,
    db: Option<Vec<(String, Tag)>>,
}

fn all_area_mods() -> Vec<(String, Tag)> {
    let data = rpt_data::data();
    let mut out = Vec::new();
    for stat in data.stats_iter("AreaMods", &[]) {
        let Some(from) = stat.from_area_mods.as_deref() else { continue };
        let tag = match from {
            "heist_exclusive" => Tag::Heist,
            "ubermap_exclusive" => Tag::UberMap,
            _ => Tag::None,
        };
        for m in &stat.matchers {
            out.push((m.string.clone(), tag));
        }
    }
    let order = |t: Tag| match t {
        Tag::UberMap => 1,
        Tag::Heist => 2,
        _ => 0,
    };
    out.sort_by(|a, b| order(a.1).cmp(&order(b.1)).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    out.dedup();
    out
}

/// Three profiles stored as e.g. "d--"; '-' none, 's' seen.
fn has_color(set: &str, profile: u32) -> bool {
    let ch = set.chars().nth(profile.saturating_sub(1) as usize).unwrap_or('-');
    ch != '-' && ch != 's'
}

fn decision_create(value: char, profile: u32, update: &str) -> String {
    let mut v: Vec<char> = update.chars().collect();
    while v.len() < 3 {
        v.push('-');
    }
    let idx = profile.saturating_sub(1) as usize;
    if idx < v.len() {
        v[idx] = value;
    }
    v.into_iter().collect()
}

pub fn maps(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let i18n = w.i18n;
    let ms = &mut st.maps;
    let db = ms.db.get_or_insert_with(all_area_mods);
    let Some(ic) = w.cfg.widgets.iter_mut().find_map(|x| match &mut x.kind {
        WidgetKind::ItemCheck(p) => Some(p),
        _ => None,
    }) else {
        return;
    };
    let maps = &mut ic.maps;
    let profile = maps.profile.max(1);

    // Toolbar
    ui.horizontal(|ui| {
        ui.scope(|ui| {
            ui.set_width(240.0);
            rpt_m3::text_field(ui, &mut ms.search, &format!("{}…", i18n.t("Search")));
        });
        ui.add_space(8.0);
        if ui.add(Chip::filter(i18n.t("map_check.search_selected"), ms.only_selected)).clicked() {
            ms.only_selected = !ms.only_selected;
        }
        if ui.add(Chip::filter(i18n.t("map_check.new_mods_icon"), maps.show_new_stats)).clicked() {
            maps.show_new_stats = !maps.show_new_stats;
        }
    });

    let data = rpt_data::data();
    let selected: std::collections::HashSet<&str> =
        maps.selected_stats.iter().filter(|e| has_color(&e.decision, profile)).map(|e| e.matcher.as_str()).collect();
    let outdated: Vec<(String, Tag)> = maps
        .selected_stats
        .iter()
        .filter(|e| has_color(&e.decision, profile) && data.stat_by_match_str_v2(&e.matcher).is_none())
        .map(|e| (e.matcher.clone(), Tag::Outdated))
        .collect();
    let q: Vec<String> = ms.search.to_lowercase().split(' ').filter(|s| !s.is_empty()).map(str::to_owned).collect();
    let mut rows: Vec<(String, Tag)> = outdated;
    rows.extend(
        db.iter()
            .filter(|(s, _)| {
                let l = s.to_lowercase();
                q.iter().all(|p| l.contains(p.as_str())) && (!ms.only_selected || selected.contains(s.as_str()))
            })
            .cloned(),
    );

    // Header
    let decisions: [(char, &str, Color32, Color32); 3] = [
        ('w', icons::WARNING, c.tertiary_container, c.on_tertiary_container),
        ('d', icons::DANGEROUS, c.error_container, c.on_error_container),
        ('g', icons::CHECK_CIRCLE, c.primary_container, c.on_primary_container),
    ];
    const COL: f32 = 40.0;
    const TRAIL: f32 = 40.0;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let n = rows.len().to_string();
        ui.label(RichText::new(i18n.tf("map_check.search_stat_col", &[&n])).font(th.font(TypeRole::LabelLarge)).color(c.on_surface_variant));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(TRAIL + 12.0);
            for (_, icon, bg, fg) in decisions.iter().rev() {
                let (r, _) = ui.allocate_exact_size(vec2(COL, 28.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(shape::SMALL), *bg);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, *icon, FontId::new(18.0, egui::FontFamily::Name(rpt_m3::theme::ICONS.into())), *fg);
            }
        });
    });
    rpt_m3::divider(ui);

    let row_h = 36.0;
    let mut set: Option<(String, char)> = None;
    egui::ScrollArea::vertical().id_salt("map-stats").auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (matcher, tag) in &rows[range] {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
            let entry = maps.selected_stats.iter().find(|e| &e.matcher == matcher);
            let cur = entry.and_then(|e| e.decision.chars().nth(profile as usize - 1)).unwrap_or('-');
            let removable = entry.is_some_and(|e| has_color(&e.decision, profile));
            let p = ui.painter().clone();
            if resp.hovered() {
                p.rect_filled(rect, CornerRadius::same(shape::SMALL), with_alpha(c.on_surface, 0.06));
            }
            let mut x = rect.left() + 8.0;
            let tag_info = match tag {
                Tag::Outdated => Some((i18n.t("map.mods.outdated"), c.error, c.on_error)),
                Tag::Heist => Some((i18n.t("map.mods.heist"), c.error_container, c.on_error_container)),
                Tag::UberMap => Some((i18n.t("map.mods.uber"), c.inverse_surface, c.inverse_on_surface)),
                Tag::None => None,
            };
            if let Some((label, bg, fg)) = tag_info {
                let g = p.layout_no_wrap(label, th.font(TypeRole::LabelSmall), fg);
                let tr = Rect::from_min_size(Pos2::new(x, rect.center().y - 10.0), vec2(g.size().x + 10.0, 20.0));
                p.rect_filled(tr, CornerRadius::same(shape::EXTRA_SMALL), bg);
                p.galley(tr.center() - g.size() / 2.0, g, fg);
                x = tr.right() + 6.0;
            }
            let controls_w = 3.0 * (COL + 4.0) + TRAIL + 12.0;
            let text_clip = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(rect.right() - controls_w, rect.bottom()));
            p.with_clip_rect(text_clip).text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, matcher, th.font(TypeRole::BodyMedium), c.on_surface);

            // Decision radios (hidden until hover unless set, like upstream).
            let show_controls = removable || resp.hovered();
            let mut cx = rect.right() - controls_w + 4.0;
            for (ch, icon, bg, fg) in decisions {
                let r = Rect::from_min_size(Pos2::new(cx, rect.center().y - 14.0), vec2(COL, 28.0));
                let id = ui.make_persistent_id(("map-dec", matcher.as_str(), ch));
                let br = ui.interact(r, id, Sense::click());
                let on = cur == ch;
                if on {
                    p.rect_filled(r, CornerRadius::same(shape::SMALL), bg);
                    p.text(r.center(), Align2::CENTER_CENTER, icon, FontId::new(18.0, egui::FontFamily::Name(rpt_m3::theme::ICONS.into())), fg);
                } else if show_controls || br.hovered() {
                    p.rect_stroke(r, CornerRadius::same(shape::SMALL), Stroke::new(1.0, c.outline_variant), StrokeKind::Inside);
                    if br.hovered() {
                        p.rect_filled(r, CornerRadius::same(shape::SMALL), with_alpha(c.on_surface, 0.08));
                    }
                }
                if br.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
                    set = Some((matcher.clone(), ch));
                }
                cx += COL + 4.0;
            }
            if removable {
                let r = Rect::from_min_size(Pos2::new(cx + 4.0, rect.center().y - 16.0), vec2(32.0, 32.0));
                let icon = if *tag == Tag::Outdated { icons::DELETE } else { icons::CLOSE };
                if ui.put(r, IconButton::new(icon).size(32.0).tooltip(i18n.t("Remove"))).clicked() {
                    set = Some((matcher.clone(), '-'));
                }
            }
        }
    });

    if let Some((matcher, ch)) = set {
        match maps.selected_stats.iter_mut().find(|e| e.matcher == matcher) {
            Some(e) => e.decision = decision_create(ch, profile, &e.decision),
            None => maps.selected_stats.push(MapCheckStat { decision: decision_create(ch, profile, "---"), matcher }),
        }
    }
}

// ---------------------------------------------------------------------------
// Item info / item search / stopwatch (HotkeysGeneric-based pages)

pub fn item_check(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let i18n = w.i18n;
    let english = w.cfg.language == "en";
    for wd in w.cfg.widgets.iter_mut() {
        if let WidgetKind::ItemCheck(ic) = &mut wd.kind {
            hotkey_row(ui, &mut st.cap, i18n, &i18n.t("item.open_on_wiki"), &mut ic.wiki_key, false);
            hotkey_row(ui, &mut st.cap, i18n, &i18n.t("item.open_on_poedb"), &mut ic.poedb_key, false);
            if english {
                hotkey_row(ui, &mut st.cap, i18n, &i18n.t("item.open_on_craftofexile"), &mut ic.craft_of_exile_key, false);
            }
            hotkey_row(ui, &mut st.cap, i18n, &i18n.t("item.find_in_stash"), &mut ic.stash_search_key, false);
        }
    }
}

pub fn item_search(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let i18n = w.i18n;
    for wd in w.cfg.widgets.iter_mut() {
        if let WidgetKind::ItemSearch(s) = &mut wd.kind {
            hotkey_row(ui, &mut st.cap, i18n, &i18n.t("item_search.ocr_gems_key"), &mut s.ocr_gems_key, false);
        }
    }
}

pub fn stopwatch(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let i18n = w.i18n;
    let Some(id) = st.target else { return };
    if let Some(WidgetKind::Timer(t)) = w.cfg.widgets.iter_mut().find(|x| x.wm_id == id).map(|x| &mut x.kind) {
        hotkey_row(ui, &mut st.cap, i18n, &i18n.t("stopwatch.toggle_key"), &mut t.toggle_key, false);
        hotkey_row(ui, &mut st.cap, i18n, &i18n.t("stopwatch.reset_key"), &mut t.reset_key, false);
    }
}

// ---------------------------------------------------------------------------
// Stash search editor (upstream stash-search/stash-search-editor.vue)

pub fn stash_search(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let i18n = w.i18n;
    let Some(id) = st.target else { return };
    let Some(wd) = w.cfg.widgets.iter_mut().find(|x| x.wm_id == id) else { return };
    field_label(ui, &i18n.t("widget.title"));
    ui.scope(|ui| {
        ui.set_width(280.0);
        rpt_m3::text_field(ui, &mut wd.wm_title, &i18n.t("widget.title"));
    });
    ui.add_space(12.0);
    let WidgetKind::StashSearch(ss) = &mut wd.kind else { return };
    let n = ss.entries.len();
    let mut remove = None;
    let mut swap = None;
    for (i, e) in ss.entries.iter_mut().enumerate() {
        rpt_m3::card(ui, CardKind::Outlined).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if ui.add(IconButton::new(icons::EXPAND_LESS).size(28.0).enabled(i > 0)).clicked() {
                        swap = Some((i, i - 1));
                    }
                    if ui.add(IconButton::new(icons::EXPAND_MORE).size(28.0).enabled(i + 1 < n)).clicked() {
                        swap = Some((i, i + 1));
                    }
                });
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 48.0);
                    let too_long = e.text.chars().count() > 250;
                    rpt_m3::text_field(ui, &mut e.text, &i18n.t("stash_search.search_text"));
                    if too_long {
                        ui.label(RichText::new(format!("{} / 250", e.text.chars().count())).small().color(c.error));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.scope(|ui| {
                            ui.set_width((ui.available_width() - 216.0).max(120.0));
                            rpt_m3::text_field(ui, &mut e.name, &i18n.t("stash_search.friendly_name"));
                        });
                        hotkey_input(ui, &mut st.cap, i18n, ("stash", id, e.id), &mut e.hotkey, false, false, 200.0);
                    });
                });
                if ui.add(IconButton::new(icons::DELETE).tooltip(i18n.t("Remove"))).clicked() {
                    remove = Some(i);
                }
            });
        });
        ui.add_space(8.0);
    }
    if let Some((a, b)) = swap {
        ss.entries.swap(a, b);
    }
    if let Some(i) = remove {
        ss.entries.remove(i);
        st.cap.active = None;
    }
    if ui.add(Button::tonal(i18n.t("Add")).icon(icons::ADD)).clicked() {
        let next = ss.entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        ss.entries.push(StashSearchEntry { id: next, name: String::new(), text: String::new(), hotkey: None });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions() {
        assert_eq!(decision_create('d', 2, "w--"), "wd-");
        assert!(has_color("d--", 1));
        assert!(!has_color("s--", 1));
        assert!(!has_color("d--", 2));
    }
}
