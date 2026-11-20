//! Item check. Port of upstream `item-check/*` (WidgetItemCheck, ItemInfo,
//! hotkeyable actions), `map-check/*` and `client-log/client-log.ts`.

pub mod client_log;
pub mod map_check;

use egui::{Area, Context, CornerRadius, Frame, Id, Margin, Order, Pos2, RichText, vec2};
use rpt_m3::{Button, TypeRole, icons, theme, with_alpha};
use rpt_parser::{ItemCategory, ItemRarity, ParsedItem};
use rpt_trade::js::encode_uri_component;

use super::WCtx;
use super::price_check::position;
use crate::config::{ItemCheckWidget, WidgetKind};
use crate::host::{HostCommand, HostEvent};
use crate::wm;

#[derive(Default)]
pub struct ItemCheckState {
    item: Option<ParsedItem>,
    check_position: Pos2,
    /// Recent chat log entries (upstream parses but does not display them yet).
    pub log: Vec<client_log::LogEntry>,
}

// ---------------------------------------------------------------------------
// hotkeyable-actions.ts

pub fn poedb_lang(language: &str) -> &'static str {
    match language {
        "ru" => "ru",
        "cmn-Hant" => "tw",
        "ko" => "kr",
        _ => "us",
    }
}

pub fn wiki_url(item: &ParsedItem) -> String {
    format!("https://www.poewiki.net/wiki/{}", encode_uri_component(&item.info.ref_name))
}

pub fn poedb_url(item: &ParsedItem, language: &str) -> String {
    let slug = encode_uri_component(&item.info.ref_name.replace('\'', "").replace(' ', "_"));
    format!("https://poedb.tw/{}/{}", poedb_lang(language), slug)
}

pub fn coe_url(item: &ParsedItem) -> String {
    format!("https://craftofexile.com/?game=poe1&eimport={}", encode_uri_component(&item.raw_text))
}

/// Stash search text for "find similar" (`JSON.stringify(item.info.name)`).
pub fn similar_search(item: &ParsedItem) -> String {
    serde_json::to_string(&item.info.name).unwrap_or_default()
}

fn run_action(w: &mut WCtx<'_>, target: &str, item: &ParsedItem) {
    let cmd = match target {
        "open-wiki" => HostCommand::OpenUrl(wiki_url(item)),
        "open-craft-of-exile" => HostCommand::OpenUrl(coe_url(item)),
        "open-poedb" => HostCommand::OpenUrl(poedb_url(item, &w.cfg.language)),
        "search-similar" => HostCommand::StashSearch(similar_search(item)),
        _ => return,
    };
    w.host.push(cmd);
}

/// `isMapLike` in `WidgetItemCheck.vue`.
pub fn is_map_like(item: &ParsedItem) -> bool {
    (item.category == Some(ItemCategory::Map) && item.rarity != Some(ItemRarity::Unique))
        || matches!(item.category, Some(ItemCategory::HeistContract | ItemCategory::HeistBlueprint | ItemCategory::Invitation | ItemCategory::Chart))
        || item.info.ref_name == "Expedition Logbook"
}

/// Weapon DPS rows of `ItemInfo.vue`: (phys, elem, total).
pub fn weapon_dps(item: &ParsedItem) -> Option<(f64, f64, f64)> {
    let aps = item.weapon_as?;
    let p = (aps * item.weapon_physical.unwrap_or(0.0)).round();
    let e = (aps * item.weapon_elemental.unwrap_or(0.0)).round();
    Some((p, e, p + e))
}

pub fn on_item_text(w: &mut WCtx<'_>, st: &mut ItemCheckState, ev: &HostEvent) {
    let HostEvent::ItemText { target, clipboard, position, .. } = ev else { return };
    match target.as_str() {
        "item-check" => {
            let ctx = w.services.ctx().clone();
            let ppp = ctx.pixels_per_point();
            let origin = ctx.input(|i| i.viewport().inner_rect.or(i.viewport().outer_rect)).map(|r| r.min).unwrap_or(Pos2::ZERO);
            st.check_position = Pos2::new(position.x / ppp, position.y / ppp) - origin.to_vec2();
            st.item = rpt_parser::parse_clipboard(clipboard).ok();
            if st.item.is_some() {
                if let Some(id) = wm::id_of(w.cfg, "item-check") {
                    wm::show(w.cfg, id);
                }
            }
        }
        "open-wiki" | "open-craft-of-exile" | "open-poedb" | "search-similar" => {
            if let Ok(item) = rpt_parser::parse_clipboard(clipboard) {
                run_action(w, target, &item);
            }
        }
        _ => {}
    }
}

/// `MAIN->CLIENT::game-log` (upstream `handleLine` for each line).
pub fn on_game_log(w: &mut WCtx<'_>, st: &mut ItemCheckState, lines: &[String]) {
    for line in lines {
        if let Some(e) = client_log::handle_line(line, &w.cfg.language) {
            st.log.push(e);
        }
    }
    let n = st.log.len();
    if n > 100 {
        st.log.drain(..n - 100);
    }
}

fn item_check_mut(cfg: &mut crate::config::Config) -> Option<&mut ItemCheckWidget> {
    cfg.widgets.iter_mut().find_map(|w| match &mut w.kind {
        WidgetKind::ItemCheck(p) => Some(p),
        _ => None,
    })
}

pub fn show(ctx: &Context, w: &mut WCtx<'_>, st: &mut ItemCheckState, wm_id: u32) {
    let Some(item) = st.item.clone() else { return };
    let th = theme(ctx);
    let c = th.colors;
    let screen = w.screen;
    let (pivot, x, y) = position::item_check_anchor(screen, st.check_position);
    let pos = Pos2::new(screen.left() + screen.width() * x / 100.0, screen.top() + screen.height() * y / 100.0);
    let max_w = (screen.width() - position::poe_panel_width(screen.height())).clamp(320.0, 480.0);
    let mut changed = false;
    let mut action: Option<&'static str> = None;
    Area::new(Id::unique(("rpt-item-check", wm_id)))
        .order(Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .constrain_to(screen)
        .interactable(w.st.active)
        .show(ctx, |ui| {
            Frame::new()
                .fill(c.surface_container_high)
                .corner_radius(CornerRadius::same(rpt_m3::shape::EXTRA_LARGE))
                .inner_margin(Margin::same(16))
                .shadow(egui::Shadow { offset: [0, 6], blur: 20, spread: 0, color: with_alpha(c.shadow, 0.45) })
                .show(ui, |ui| {
                    ui.set_max_width(max_w);
                    if is_map_like(&item) {
                        if let Some(ic) = item_check_mut(w.cfg) {
                            changed = map_check::map_check(ui, w.i18n, &item, &mut ic.maps);
                        }
                        return;
                    }
                    // ItemInfo.vue
                    ui.set_min_width(320.0);
                    let tint = super::price_check::ui::harmonized(ui, super::price_check::ui::rarity_design_color(&item));
                    Frame::new().fill(tint.container).corner_radius(CornerRadius::same(rpt_m3::shape::MEDIUM)).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(&item.info.name).font(th.font(TypeRole::TitleMedium)).color(tint.on_container));
                        });
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(Button::tonal("wiki").icon(icons::MENU_BOOK).small()).on_hover_text(w.i18n.t("item.open_on_wiki")).clicked() {
                            action = Some("open-wiki");
                        }
                        if ui.add(Button::tonal("poedb").icon(icons::TRAVEL_EXPLORE).small()).on_hover_text(w.i18n.t("item.open_on_poedb")).clicked() {
                            action = Some("open-poedb");
                        }
                        let show_coe = item.info.craftable.is_some() && item.is_corrupted.is_none() && !item.is_mirrored;
                        if show_coe && ui.add(Button::tonal("CoE").icon(icons::CONSTRUCTION).small()).on_hover_text(w.i18n.t("item.open_on_craftofexile")).clicked() {
                            action = Some("open-craft-of-exile");
                        }
                        ui.label(rpt_m3::icon_text(icons::MORE_VERT, 18.0).color(c.outline));
                        if ui.add(Button::filled(w.i18n.t("item.find_in_stash")).icon(icons::MANAGE_SEARCH).small()).clicked() {
                            action = Some("search-similar");
                        }
                    });
                    let dps = weapon_dps(&item);
                    if dps.is_some() || item.dust_equivalent.is_some() {
                        ui.add_space(8.0);
                        Frame::new().fill(c.surface_container).corner_radius(CornerRadius::same(rpt_m3::shape::MEDIUM)).inner_margin(Margin::symmetric(16, 8)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::Grid::new("rpt-item-info").num_columns(2).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                                if let Some((p, e, t)) = dps {
                                    for (k, v) in [("item.physical_dps", p), ("item.elemental_dps", e), ("item.total_dps", t)] {
                                        ui.label(RichText::new(w.i18n.t(k).replace(": {0}", "")).color(c.on_surface_variant));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(rpt_trade::js::num_to_string(v)));
                                        ui.end_row();
                                    }
                                }
                                if let Some(d) = item.dust_equivalent {
                                    ui.label(RichText::new(w.i18n.t("item.disenchanting")).color(c.on_surface_variant));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.add(egui::Image::new(super::price_check::ui::bundled("dust.png")).fit_to_exact_size(vec2(20.0, 20.0)));
                                        ui.label(format_thousands(d));
                                    });
                                    ui.end_row();
                                }
                            });
                        });
                    }
                });
        });
    if let Some(a) = action {
        run_action(w, a, &item);
    }
    if changed {
        w.save_requested = true;
    }
}

fn format_thousands(v: f64) -> String {
    let s = (v.round() as i64).abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if v < 0.0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: &str) -> ParsedItem {
        rpt_data::init("en").unwrap();
        rpt_parser::parse_clipboard(text).unwrap()
    }

    #[test]
    fn urls() {
        let it = item("Item Class: Rings\nRarity: Unique\nBerek's Grip\nTwo-Stone Ring\n--------\nItem Level: 80\n");
        assert_eq!(wiki_url(&it), "https://www.poewiki.net/wiki/Berek's%20Grip");
        assert_eq!(poedb_url(&it, "en"), "https://poedb.tw/us/Bereks_Grip");
        assert_eq!(poedb_url(&it, "ko"), "https://poedb.tw/kr/Bereks_Grip");
        assert!(coe_url(&it).starts_with("https://craftofexile.com/?game=poe1&eimport=Item%20Class%3A%20Rings%0A"));
        assert_eq!(similar_search(&it), "\"Berek's Grip\"");
        assert!(!is_map_like(&it));
        assert_eq!(format_thousands(1234.0), "1,234");
    }

    #[test]
    fn dps() {
        let mut it = item("Item Class: Rings\nRarity: Unique\nBerek's Grip\nTwo-Stone Ring\n--------\nItem Level: 80\n");
        assert_eq!(weapon_dps(&it), None);
        it.weapon_as = Some(1.5);
        it.weapon_physical = Some(100.0);
        it.weapon_elemental = Some(33.3);
        assert_eq!(weapon_dps(&it), Some((150.0, 50.0, 200.0)));
    }
}
