//! Item search. Port of upstream `item-search/WidgetItemSearch.vue`:
//! search skill gems / replica uniques by (translated) name, keep up to five
//! picks with their poe.ninja price, click a pick to price check it.
//!
//! Not ported: the OCR "heist gems" hotkey (`MAIN->CLIENT::ocr-text`) — the
//! platform layer has no OCR, so the timeout/auto-hide part is omitted too.

use egui::{Context, Pos2, RichText, Sense, vec2};
use rpt_data::{BaseType, Namespace};
use rpt_m3::{Button, TypeRole, icons, theme};
use rpt_parser::{ItemCategory, ItemRarity, ParsedItem};
use rpt_trade::prices::{CurrencyValue, DbQuery};

use super::price_check::ui::{QuickPrice, quick_price};
use super::{WCtx, chrome};

const MAX_RESULTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeFilter {
    #[default]
    Gem,
    Replica,
}

#[derive(Debug, Clone)]
pub struct SelectedItem {
    pub info: BaseType,
    pub discr: Option<String>,
    pub chaos: Option<f64>,
    pub price: Option<CurrencyValue>,
}

#[derive(Default)]
pub struct ItemSearchState {
    search: String,
    type_filter: TypeFilter,
    selected: Vec<SelectedItem>,
    /// item to open in the price check (consumed by `widgets::draw_widget`)
    pending_check: Option<(ParsedItem, Pos2)>,
}

impl ItemSearchState {
    pub fn take_price_check(&mut self) -> Option<(ParsedItem, Pos2)> {
        self.pending_check.take()
    }
}

/// Upstream `findItems`: `None` = too many results (or query shorter than 3).
pub fn find_names<'a>(search: &str, names: impl Iterator<Item = &'a str>, language: &str) -> Option<Vec<&'a str>> {
    let search = search.trim();
    if search.chars().count() < 3 {
        return None;
    }
    let lc = search.to_lowercase();
    let mut words: Vec<&str> = lc.split_whitespace().collect();
    words.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    let longest = words.first().copied().unwrap_or("");
    let mut out = Vec::new();
    for name in names {
        let lc_name = name.to_lowercase();
        if words.iter().all(|w| lc_name.contains(w)) && (language == "cmn-Hant" || lc_name.split_whitespace().any(|p| p.starts_with(longest))) {
            out.push(name);
            if out.len() > MAX_RESULTS {
                return None;
            }
        }
    }
    Some(out)
}

/// `useSelectedItems().addItem`. Returns false if it was already selected.
pub fn add_item(items: &mut Vec<SelectedItem>, new: SelectedItem) -> bool {
    if items.iter().any(|i| i.info.name == new.info.name && i.discr == new.discr) {
        return false;
    }
    if items.len() < 5 {
        items.push(new);
        items.sort_by(|a, b| b.chaos.unwrap_or(0.0).total_cmp(&a.chaos.unwrap_or(0.0)));
    } else {
        *items = vec![new];
    }
    true
}

fn virtual_item(info: &BaseType) -> ParsedItem {
    let mut it = ParsedItem::virtual_item(info.clone());
    if info.namespace == Namespace::Gem {
        it.category = Some(ItemCategory::Gem);
        it.gem_level = Some(1);
    } else {
        it.rarity = Some(ItemRarity::Unique);
    }
    it
}

fn names_for(filter: TypeFilter) -> Vec<String> {
    let data = rpt_data::data();
    let mut v: Vec<String> = match filter {
        TypeFilter::Gem => data.all_items().iter().filter(|i| i.namespace == Namespace::Gem).map(|i| i.name.clone()).collect(),
        TypeFilter::Replica => data.replica_unique_names().map(str::to_owned).collect(),
    };
    v.dedup();
    v
}

fn select_item(w: &mut WCtx<'_>, st: &mut ItemSearchState, item: &BaseType) {
    w.services.queue_prices_fetch();
    let query = match item.namespace {
        Namespace::Unique => Some(DbQuery { ns: "UNIQUE".into(), name: item.ref_name.clone(), variant: item.unique.as_ref().map(|u| u.base.clone()) }),
        _ => rpt_trade::trends::get_details_id(&virtual_item(item)).map(Into::into),
    };
    let price = query.and_then(|q| w.services.prices.find_price_by_query(&q));
    add_item(
        &mut st.selected,
        SelectedItem { info: item.clone(), discr: None, chaos: price.as_ref().map(|p| p.chaos), price: price.map(|p| w.services.prices.auto_currency(p.chaos)) },
    );
    st.search.clear();
}

pub fn show(ctx: &Context, w: &mut WCtx<'_>, st: &mut ItemSearchState, wm_id: u32) {
    let th = theme(ctx);
    let c = th.colors;
    let mut pick: Option<BaseType> = None;
    let mut clear = false;
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, w| {
        ui.set_min_width(384.0);
        // centered rows fill the available width; cap it so the window doesn't stretch
        ui.set_max_width(400.0);
        if !st.selected.is_empty() {
            egui::Frame::new().fill(c.surface_container_high).corner_radius(egui::CornerRadius::same(rpt_m3::shape::MEDIUM)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for it in &st.selected {
                        let resp = ui
                            .scope(|ui| {
                                egui::Frame::new().corner_radius(egui::CornerRadius::same(rpt_m3::shape::SMALL)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        ui.set_max_width(112.0);
                                        quick_price(ui, it.price.as_ref(), &QuickPrice { item_img: Some(&it.info.icon), currency_text: true, ..Default::default() });
                                        ui.add(egui::Label::new(RichText::new(&it.info.name).font(th.font(TypeRole::BodySmall))).truncate());
                                        if let Some(d) = &it.discr {
                                            ui.label(RichText::new(w.i18n.t(d)).font(th.font(TypeRole::BodySmall)));
                                        }
                                    });
                                })
                            })
                            .response
                            .interact(Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(resp.rect, egui::CornerRadius::same(rpt_m3::shape::SMALL), rpt_m3::with_alpha(c.on_surface, 0.08));
                        }
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            let pos = ui.input(|i| i.pointer.interact_pos()).unwrap_or_default();
                            st.pending_check = Some((virtual_item(&it.info), pos));
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }
        egui::Frame::new().fill(c.surface_container_high).corner_radius(egui::CornerRadius::same(rpt_m3::shape::MEDIUM)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                // fixed width: filling the available width stretched the window to the screen edge
                let w_field = 384.0 - 130.0;
                ui.scope(|ui| {
                    ui.set_width(w_field);
                    rpt_m3::text_field(ui, &mut st.search, &w.i18n.t("item_search.input"));
                });
                if ui.add(Button::tonal(w.i18n.t("item_search.reset")).icon(icons::CLOSE).small()).clicked() {
                    clear = true;
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(w.i18n.t("item_search.heist_target"));
                let (gem, rep) = (w.i18n.t("item_search.target_gem"), w.i18n.t("item_search.target_replica"));
                rpt_m3::segmented(ui, &mut st.type_filter, &[(TypeFilter::Gem, gem.as_str()), (TypeFilter::Replica, rep.as_str())]);
            });
            ui.add_space(4.0);
            let names = names_for(st.type_filter);
            let ns = if st.type_filter == TypeFilter::Gem { Namespace::Gem } else { Namespace::Unique };
            let lang = w.cfg.language.clone();
            match find_names(&st.search, names.iter().map(String::as_str), &lang) {
                None => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(16.0);
                        ui.label(RichText::new(format!("{}  {}", icons::SEARCH, w.i18n.t("item_search.too_many"))).color(c.on_surface_variant));
                        ui.add_space(16.0);
                    });
                }
                Some(found) if found.is_empty() => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(16.0);
                        ui.label(RichText::new(format!("{}  {}", icons::WARNING, w.i18n.t("item_search.not_found"))).color(c.on_surface_variant));
                        ui.add_space(16.0);
                    });
                }
                Some(found) => {
                    let data = rpt_data::data();
                    for name in found {
                        for item in data.item_by_translated(ns, name).unwrap_or_default() {
                            ui.horizontal(|ui| {
                                super::price_check::ui::icon(ui, &item.icon, vec2(48.0, 48.0));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&item.name).font(th.font(TypeRole::BodyLarge)));
                                    if ui.add(Button::text(w.i18n.t("Select")).small()).clicked() {
                                        pick = Some(item.clone());
                                    }
                                });
                            });
                        }
                    }
                }
            }
        });
    });
    chrome::apply(w, wm_id, &r);
    if let Some(item) = pick {
        select_item(w, st, &item);
    }
    if clear {
        st.selected.clear();
        if let Some(wd) = w.cfg.widgets.iter_mut().find(|x| x.wm_id == wm_id) {
            wd.wm_flags = vec!["invisible-on-blur".into()];
            w.save_requested = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        let names = ["Vaal Grace", "Grace", "Purity of Fire", "Vaal Righteous Fire", "Righteous Fire", "Fire Trap", "Fireball", "Firestorm"];
        assert_eq!(find_names("gr", names.iter().copied(), "en"), None);
        assert_eq!(find_names("grace", names.iter().copied(), "en"), Some(vec!["Vaal Grace", "Grace"]));
        assert_eq!(find_names("fire righ", names.iter().copied(), "en"), Some(vec!["Vaal Righteous Fire", "Righteous Fire"]));
        // "fire" matches 6 names → too many
        assert_eq!(find_names("fire", names.iter().copied(), "en"), None);
        assert_eq!(find_names("zzz", names.iter().copied(), "en"), Some(vec![]));
        // word-start rule ("ace" is inside "Grace" but no word starts with it)
        assert_eq!(find_names("ace", names.iter().copied(), "en"), Some(vec![]));
        assert_eq!(find_names("ace", names.iter().copied(), "cmn-Hant"), Some(vec!["Vaal Grace", "Grace"]));
    }

    #[test]
    fn selection() {
        rpt_data::init("en").unwrap();
        let data = rpt_data::data();
        let gems: Vec<BaseType> = data.all_items().iter().filter(|i| i.namespace == Namespace::Gem).take(7).cloned().collect();
        let mut sel = Vec::new();
        for (i, g) in gems.iter().take(5).enumerate() {
            assert!(add_item(&mut sel, SelectedItem { info: g.clone(), discr: None, chaos: Some(i as f64), price: None }));
        }
        assert_eq!(sel[0].chaos, Some(4.0));
        assert!(!add_item(&mut sel, SelectedItem { info: gems[0].clone(), discr: None, chaos: None, price: None }));
        assert!(add_item(&mut sel, SelectedItem { info: gems[6].clone(), discr: None, chaos: None, price: None }));
        assert_eq!(sel.len(), 1);
        let v = virtual_item(&gems[0]);
        assert_eq!(v.category, Some(ItemCategory::Gem));
    }
}
