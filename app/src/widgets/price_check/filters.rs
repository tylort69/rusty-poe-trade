//! Port of upstream `filters/*.vue`: FiltersBlock, FilterName, FilterBtnNumeric,
//! FilterBtnLogical, FilterGroup, FilterModifier (+Options/Tiers/Links/
//! Anointment), SourceInfo, UnknownModifier and the StatRollSlider.

use egui::{Align, Color32, CornerRadius, Frame, Layout, Margin, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use rpt_m3::{RangeEdge, TypeRole, icons, theme, with_alpha};
use rpt_parser::{ItemCategory, ItemRarity, ModifierType, ParsedItem, RollGeneration, StatSource};
use rpt_trade::filters::util::{RoundMethod, percent_roll, round_roll};
use rpt_trade::filters::{
    FilterGroup, FilterNumeric, FilterOrGroup, FilterTag, GroupKind, ItemFilters, ItemHasEmptyModifier, StatFilter, is_internal_trade_id,
};
use rpt_trade::trade::category_to_trade_id;

use super::Env;
use super::session::{NUMERIC_FILTERS, Session, numeric_filter};
use super::ui::{bundled, harmonized, modifier_job};

/// Outcome of drawing the filters block.
#[derive(Default)]
pub struct FiltersOut {
    /// Enter pressed in a stat input (upstream form submit) → search.
    pub submit: bool,
    pub rect: Option<Rect>,
}

// ---------------------------------------------------------------------------
// FilterName.vue

fn active_search(f: &ItemFilters) -> &rpt_trade::filters::SearchFilter {
    match &f.search_relaxed {
        Some(r) if !r.disabled => r,
        _ => &f.search_exact,
    }
}

pub fn filter_name_label(env: &Env<'_>, f: &ItemFilters) -> String {
    let s = active_search(f);
    if let Some(n) = &s.name {
        return n.clone();
    }
    if let Some(b) = &s.base_type {
        return b.clone();
    }
    if let Some(cat) = s.category {
        if let Some(id) = category_to_trade_id(cat) {
            return env.i18n.tf("item_category.prop", &[&env.i18n.t(&format!("item_category.{}", id.replacen('.', "_", 1)))]);
        }
    }
    "??? Report if you see this text".into()
}

/// Item header card: name (accuracy toggle) + corrupted toggle, tinted by rarity.
pub fn filter_name(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem, f: &mut ItemFilters) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let tint = harmonized(ui, super::ui::rarity_design_color(item));
    let label = filter_name_label(env, f);
    let show_as_active = {
        let s = active_search(f);
        if s.sub.as_ref().is_some_and(|sub| !sub.disabled) { false } else { f.search_relaxed.as_ref().is_some_and(|r| r.disabled) }
    };
    Frame::new()
        .fill(tint.container)
        .corner_radius(CornerRadius::same(rpt_m3::shape::MEDIUM))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let can_toggle = f.search_relaxed.is_some();
                let name = RichText::new(&label).font(th.font(TypeRole::TitleMedium)).color(tint.on_container);
                let corrupted_w = if f.corrupted.is_some() { 140.0 } else { 0.0 };
                let max_w = (ui.available_width() - corrupted_w).max(60.0);
                let r = ui
                    .scope(|ui| {
                        ui.set_max_width(max_w);
                        let frame_stroke = if show_as_active { Stroke::new(1.0, tint.accent) } else { Stroke::NONE };
                        Frame::new()
                            .stroke(frame_stroke)
                            .corner_radius(CornerRadius::same(rpt_m3::shape::SMALL))
                            .inner_margin(Margin::symmetric(6, 2))
                            .show(ui, |ui| ui.add(egui::Label::new(name).truncate().sense(if can_toggle { Sense::click() } else { Sense::hover() })))
                            .inner
                    })
                    .inner;
                if r.clicked() {
                    if let Some(rel) = &mut f.search_relaxed {
                        rel.disabled = !rel.disabled;
                    }
                }
                if can_toggle {
                    r.on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                if let Some(cor) = &mut f.corrupted {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (text, color) = if !cor.value {
                            (env.t("item.not_corrupted"), c.on_surface_variant)
                        } else if cor.twice {
                            (env.t("item.twice_corrupted"), c.error)
                        } else {
                            (env.t("item.corrupted"), c.error)
                        };
                        let resp = ui.add(egui::Label::new(RichText::new(text).font(th.font(TypeRole::LabelLarge)).color(color)).sense(Sense::click()));
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            cor.value = !cor.value;
                        }
                    });
                }
            });
        });
}

// ---------------------------------------------------------------------------
// FilterBtnLogical.vue / FilterBtnNumeric.vue

/// Filter-chip styled toggle with optional image and collapse chevron.
pub fn logical_chip(ui: &mut Ui, text: &str, img: Option<&str>, active: bool, collapse: Option<bool>) -> Response {
    let th = theme(ui.ctx());
    let c = th.colors;
    let fg = if active { c.on_secondary_container } else { c.on_surface_variant };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), th.font(TypeRole::LabelLarge), fg);
    let lead = if img.is_some() { 24.0 } else if active { 22.0 } else { 0.0 };
    let trail = if collapse.is_some() { 20.0 } else { 0.0 };
    let size = vec2(10.0 + lead + galley.size().x + trail + 12.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let r = CornerRadius::same(rpt_m3::shape::SMALL);
        let p = ui.painter();
        if active {
            p.rect_filled(rect, r, c.secondary_container);
        } else {
            p.rect_stroke(rect, r, Stroke::new(1.0, c.outline_variant), StrokeKind::Inside);
        }
        if resp.hovered() {
            p.rect_filled(rect, r, with_alpha(fg, 0.08));
        }
        let mut x = rect.left() + 10.0;
        if let Some(name) = img {
            let ir = Rect::from_min_size(Pos2::new(x, rect.center().y - 10.0), vec2(20.0, 20.0));
            egui::Image::new(bundled(name)).paint_at(ui, ir);
            x += lead;
        } else if active {
            p.text(Pos2::new(x, rect.center().y), egui::Align2::LEFT_CENTER, icons::CHECK, icon_font(16.0), fg);
            x += lead;
        }
        p.galley(Pos2::new(x, rect.center().y - galley.size().y / 2.0), galley.clone(), fg);
        x += galley.size().x + 4.0;
        if let Some(collapsed) = collapse {
            p.text(Pos2::new(x, rect.center().y), egui::Align2::LEFT_CENTER, if collapsed { icons::EXPAND_MORE } else { icons::EXPAND_LESS }, icon_font(18.0), fg);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn icon_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(rpt_m3::theme::ICONS.into()))
}

/// `FilterBtnNumeric`: name toggles the filter, min (and optional max) inputs.
fn numeric_chip(ui: &mut Ui, id: egui::Id, name: &str, f: &mut FilterNumeric, input: &mut Option<f64>, has_max: bool) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let active = !f.disabled;
    Frame::new()
        .fill(if active { c.secondary_container } else { Color32::TRANSPARENT })
        .stroke(if active { Stroke::NONE } else { Stroke::new(1.0, c.outline_variant) })
        .corner_radius(CornerRadius::same(rpt_m3::shape::SMALL))
        .inner_margin(Margin { left: 10, right: 4, top: 3, bottom: 3 })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.horizontal(|ui| {
                let fg = if active { c.on_secondary_container } else { c.on_surface_variant };
                let l = ui.add(egui::Label::new(RichText::new(name).font(th.font(TypeRole::LabelLarge)).color(fg)).sense(Sense::click()));
                if l.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    f.disabled = !f.disabled;
                }
                let w = 14.0 + 7.0 * input.map(rpt_m3::fmt_number).map(|s| s.len().max(2)).unwrap_or(2) as f32;
                let r = rpt_m3::optional_number_field(ui, id.with("min"), input, "", w);
                if r.gained_focus() {
                    f.disabled = false;
                }
                if r.changed() {
                    f.value = input.unwrap_or(0.0);
                }
                if r.lost_focus() && input.is_none() {
                    *input = Some(0.0);
                    f.value = 0.0;
                    f.disabled = true;
                }
                if has_max {
                    ui.label(RichText::new("–").color(fg));
                    let mut max = f.max;
                    let w = 14.0 + 7.0 * max.map(rpt_m3::fmt_number).map(|s| s.len().max(2)).unwrap_or(2) as f32;
                    let r = rpt_m3::optional_number_field(ui, id.with("max"), &mut max, "…", w);
                    if r.gained_focus() {
                        f.disabled = false;
                    }
                    if r.changed() {
                        f.max = max;
                    }
                }
            });
        });
}

// ---------------------------------------------------------------------------
// FiltersBlock.vue

fn search_sub(f: &mut ItemFilters) -> Option<&mut rpt_trade::filters::SearchFilter> {
    let relaxed_active = f.search_relaxed.as_ref().is_some_and(|r| !r.disabled);
    let s = if relaxed_active { f.search_relaxed.as_mut()? } else { &mut f.search_exact };
    s.sub.as_deref_mut()
}

fn is_hidden(s: &FilterOrGroup) -> bool {
    match s {
        FilterOrGroup::Group(g) => g.meta.hidden.is_some(),
        FilterOrGroup::Stat(s) => s.hidden.is_some(),
    }
}

pub fn filters_block(ui: &mut Ui, env: &Env<'_>, s: &mut Session) -> FiltersOut {
    let mut out = FiltersOut::default();
    let th = theme(ui.ctx());
    let c = th.colors;
    let Session { item, presets, filters_ui, numeric_inputs, has_max, .. } = s;
    let preset_ids: Vec<(String, bool)> = presets.presets.iter().map(|p| (p.id.clone(), p.id == presets.active)).collect();
    let mut select_preset: Option<String> = None;
    let Some(preset) = presets.active_preset_mut() else { return out };
    let (filters, stats) = (&mut preset.filters, &mut preset.stats);

    let show_unknown_mods =
        !item.unknown_modifiers.is_empty() && item.category != Some(ItemCategory::Sentinel) && item.category != Some(ItemCategory::Map);
    let has_stats = !stats.is_empty() || (show_unknown_mods && item.rarity == Some(ItemRarity::Unique)) || preset_ids.len() > 1;
    let total_selected = stats.iter().filter(|s| s.is_enabled()).count();

    let top = ui.vertical(|ui| {
        // chips row
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if let Some(sub) = search_sub(filters) {
                let text = sub.name.clone().or_else(|| sub.base_type.clone()).unwrap_or_default();
                if logical_chip(ui, &env.t(&text), None, !sub.disabled, None).clicked() {
                    sub.disabled = !sub.disabled;
                }
            }
            let names: [(&str, &str); 9] = [
                ("linked_sockets", "item.linked_sockets"),
                ("map_tier", "item.map_tier"),
                ("area_level", "item.area_level"),
                ("sentinel_charge", "item.sentinel_charge"),
                ("item_level", "item.item_level"),
                ("stack_size", "item.stock"),
                ("white_sockets", "item.white_sockets"),
                ("gem_level", "item.gem_level"),
                ("quality", "item.quality"),
            ];
            debug_assert_eq!(names.len(), NUMERIC_FILTERS.len());
            for (name, key) in names {
                if name == "area_level" {
                    if let Some(r) = &filters.map_completion_reward {
                        logical_chip(ui, &env.i18n.tf("item.map_foil_reward", &[&r.name]), None, true, None);
                    }
                }
                if let Some(f) = numeric_filter(filters, name) {
                    let input = numeric_inputs.entry(NUMERIC_FILTERS.iter().copied().find(|n| *n == name).unwrap_or("x")).or_insert(Some(f.value));
                    let label = env.i18n.t(key).replace(": {0}", "").replace("{0}", "");
                    numeric_chip(ui, ui.scope_id().with(("num", name)), &label, f, input, has_max.contains(name));
                }
            }
            if let Some(nc) = &mut filters.not_catalysed {
                if logical_chip(ui, &env.t("item.not_catalysed"), None, !nc.disabled, None).clicked() {
                    nc.disabled = !nc.disabled;
                }
            }
            if let Some(infl) = &mut filters.influences {
                for i in infl.iter_mut() {
                    let img = format!("influence-{}.png", i.value.as_str());
                    if logical_chip(ui, &env.t(i.value.as_str()), Some(&img), !i.disabled, None).clicked() {
                        i.disabled = !i.disabled;
                    }
                }
            }
            if let Some(r) = &mut filters.rarity {
                if r.value == "magic" && logical_chip(ui, &env.t("Magic"), None, !r.disabled, None).clicked() {
                    r.disabled = !r.disabled;
                }
            }
            if let Some(u) = &mut filters.unidentified {
                if logical_chip(ui, &env.t("item.unidentified"), None, !u.disabled, None).clicked() {
                    u.disabled = !u.disabled;
                }
            }
            if let Some(v) = &mut filters.veiled {
                if logical_chip(ui, &env.t("item.veiled"), None, !v.disabled, None).clicked() {
                    v.disabled = !v.disabled;
                }
            }
            if let Some(f) = &mut filters.foil {
                if logical_chip(ui, &env.t("item.foil_unique"), None, !f.disabled, None).clicked() {
                    f.disabled = !f.disabled;
                }
            }
            if let Some(m) = &mut filters.mirrored {
                if !m.hidden {
                    let t = env.t(if m.disabled { "item.not_mirrored" } else { "item.mirrored" });
                    if logical_chip(ui, &t, None, true, None).clicked() {
                        m.disabled = !m.disabled;
                    }
                }
            }
            if let Some(m) = &mut filters.split {
                if !m.hidden {
                    let t = env.t(if m.disabled { "item.not_split" } else { "item.split" });
                    if logical_chip(ui, &t, None, true, None).clicked() {
                        m.disabled = !m.disabled;
                    }
                }
            }
            if has_stats {
                let text = if total_selected > 0 {
                    env.i18n.tf("filters.selected_some", &[&total_selected.to_string(), &stats.len().to_string()])
                } else {
                    env.t("filters.selected_none")
                };
                if logical_chip(ui, &text, None, total_selected > 0, Some(filters_ui.stats_collapsed)).clicked() {
                    filters_ui.stats_collapsed = !filters_ui.stats_collapsed;
                }
            }
        });

        if !filters_ui.stats_collapsed && has_stats {
            ui.add_space(8.0);
            if preset_ids.len() > 1 {
                let labels: Vec<String> = preset_ids.iter().map(|(id, _)| env.t(id)).collect();
                let opts: Vec<(String, &str)> = preset_ids.iter().zip(&labels).map(|((id, _), l)| (id.clone(), l.as_str())).collect();
                let mut cur = preset_ids.iter().find(|(_, a)| *a).map(|(id, _)| id.clone()).unwrap_or_default();
                if rpt_m3::segmented(ui, &mut cur, &opts) {
                    select_preset = Some(cur);
                }
                ui.add_space(4.0);
            }
            let show_hidden = filters_ui.show_hidden;
            let mut shown = 0;
            for (idx, st) in stats.iter_mut().enumerate() {
                if is_hidden(st) != show_hidden {
                    continue;
                }
                shown += 1;
                match st {
                    FilterOrGroup::Group(g) => {
                        if filter_group(ui, env, item, g, idx, filters_ui.show_sources) {
                            out.submit = true;
                        }
                    }
                    FilterOrGroup::Stat(f) => {
                        if filter_modifier(ui, env, item, f, ModOpts { idx, grouped: false, show_sources: filters_ui.show_sources, group_expanded: None, links: None }).submit {
                            out.submit = true;
                        }
                    }
                }
            }
            if shown == 0 && !show_unknown_mods {
                ui.add_space(4.0);
                ui.label(RichText::new(env.t("filters.empty")).color(c.on_surface_variant));
                rpt_m3::divider(ui);
            }
            if show_unknown_mods {
                for m in &item.unknown_modifiers {
                    unknown_modifier(ui, env, &m.text, m.type_);
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.add(rpt_m3::Button::text(env.t("filters.collapse")).icon(icons::EXPAND_LESS).small()).clicked() {
                    filters_ui.stats_collapsed = true;
                }
                let filtered = stats.iter().filter(|s| !is_hidden(s)).count();
                if filtered != stats.len() {
                    ui.add_space(8.0);
                    rpt_m3::switch(ui, &mut filters_ui.show_hidden);
                    ui.label(RichText::new(env.t("filters.hidden_toggle")).color(c.on_surface_variant));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(env.t("filters.mods_toggle")).color(c.on_surface_variant));
                    rpt_m3::switch(ui, &mut filters_ui.show_sources);
                });
            });
        }
    });
    out.rect = Some(top.response.rect);
    if let Some(id) = select_preset {
        presets.active = id;
    }
    out
}

// ---------------------------------------------------------------------------
// FilterGroup.vue

/// Returns true on submit (Enter in an input).
fn filter_group(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem, g: &mut FilterGroup, idx: usize, show_sources: bool) -> bool {
    let mut expanded = g.expanded;
    let links = (g.group == GroupKind::Mercenary).then(|| mercenary_links_state(g));
    let r = filter_modifier(ui, env, item, &mut g.meta, ModOpts { idx, grouped: false, show_sources, group_expanded: Some(&mut expanded), links });
    g.expanded = expanded;
    let mut submit = r.submit;
    if g.expanded {
        for (i, f) in g.stats.iter_mut().enumerate() {
            if filter_modifier(ui, env, item, f, ModOpts { idx: idx * 1000 + i + 1, grouped: true, show_sources, group_expanded: None, links: None }).submit {
                submit = true;
            }
        }
    }
    submit
}

/// `FilterModifierLinks.vue` state: (is six-link, checked) for the skill
/// gem, then (tier, checked) for each support gem.
#[derive(Debug, Clone, PartialEq)]
pub struct LinksState {
    pub skill_sixlink: bool,
    pub skill_checked: bool,
    pub supports: Vec<(Option<u32>, bool)>,
}

pub fn mercenary_links_state(g: &FilterGroup) -> LinksState {
    let supports: Vec<&StatFilter> = g.stats.iter().filter(|s| s.mercenary.is_some()).collect();
    let meta_checked = !g.meta.disabled;
    let sixlink_checked = g
        .stats
        .iter()
        .any(|s| !s.disabled && ((s.not && supports.len() == 5) || s.first_trade_id() == "item.mercenary_6link"));
    LinksState {
        skill_sixlink: supports.len() == 5,
        skill_checked: meta_checked && sixlink_checked,
        supports: supports.iter().map(|s| (s.mercenary.as_ref().and_then(|m| m.tier), meta_checked && !s.disabled)).collect(),
    }
}

fn draw_links(ui: &mut Ui, st: &LinksState) {
    let c = theme(ui.ctx()).colors;
    let gold = harmonized(ui, Color32::from_rgb(0xEA, 0xB3, 0x08));
    let gilded = harmonized(ui, Color32::from_rgb(0xA1, 0x62, 0x07));
    let n = 1 + st.supports.len();
    let (rect, _) = ui.allocate_exact_size(vec2(n as f32 * 10.0 + (n as f32 - 1.0) * 10.0, 16.0), Sense::hover());
    let p = ui.painter();
    p.hline(rect.left() + 4.0..=rect.right() - 4.0, rect.center().y, Stroke::new(2.0, c.outline_variant));
    let mut x = rect.left() + 5.0;
    let center = |x: f32| Pos2::new(x, rect.center().y);
    // skill gem: diamond
    let col = if st.skill_sixlink { gold.accent } else { c.outline };
    let pts = vec![center(x) + vec2(0.0, -6.0), center(x) + vec2(6.0, 0.0), center(x) + vec2(0.0, 6.0), center(x) + vec2(-6.0, 0.0)];
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    if st.skill_sixlink {
        p.text(center(x), egui::Align2::CENTER_CENTER, "6", egui::FontId::proportional(8.0), Color32::BLACK);
    }
    if st.skill_checked {
        p.circle_stroke(center(x), 8.5, Stroke::new(1.0, c.on_surface));
    }
    for (tier, checked) in &st.supports {
        x += 20.0;
        let col = match tier {
            Some(3) => gold.accent,
            Some(4) => gilded.accent,
            _ => c.surface_container_highest,
        };
        p.circle_filled(center(x), 5.0, col);
        if *checked {
            p.circle_stroke(center(x), 7.5, Stroke::new(1.0, c.on_surface));
        }
    }
}

// ---------------------------------------------------------------------------
// FilterModifier.vue

struct ModOpts<'a> {
    idx: usize,
    grouped: bool,
    show_sources: bool,
    group_expanded: Option<&'a mut bool>,
    /// mercenary group: links drawn in the inputs slot
    links: Option<LinksState>,
}

struct ModOut {
    submit: bool,
}

/// Roll option buttons (`FilterModifierOptions.vue`).
pub fn roll_options(env: &Env<'_>, f: &StatFilter) -> Option<Vec<(String, f64)>> {
    if f.tag == FilterTag::MercenarySupport && f.option.is_some() {
        use rpt_trade::filters::pseudo::mercenary::SearchMode;
        return Some(vec![
            (env.t("filters.option_merc_required"), SearchMode::Required.value()),
            (env.t("filters.option_merc_optional"), SearchMode::Optional.value()),
        ]);
    }
    if f.first_trade_id() == "item.has_empty_modifier" {
        return Some(vec![
            (env.t("filters.option_empty_affix"), ItemHasEmptyModifier::Any as u8 as f64),
            (env.t("filters.option_empty_prefix"), ItemHasEmptyModifier::Prefix as u8 as f64),
            (env.t("filters.option_empty_suffix"), ItemHasEmptyModifier::Suffix as u8 as f64),
        ]);
    }
    None
}

/// `miniFilter` computed.
pub fn is_mini_filter(item: &ParsedItem, f: &StatFilter) -> bool {
    f.hidden.is_none()
        && (f.tag == FilterTag::MercenarySupport
            || (f.tag == FilterTag::Property && item.info.ref_name == "Mercenary Warrant")
            || item.info.ref_name == "Mirrored Tablet"
            || item.info.ref_name == "Filled Coffin"
            || item.category == Some(ItemCategory::Gem))
}

/// `showTag` computed.
pub fn show_tag(item: &ParsedItem, f: &StatFilter, grouped: bool) -> bool {
    f.tag != FilterTag::Property
        && f.tag != FilterTag::MercenarySupport
        && f.first_trade_id() != "item.has_empty_modifier"
        && !["Chronicle of Atzoatl", "Mirrored Tablet", "Filled Coffin", "Viscera"].contains(&item.info.ref_name.as_str())
        && item.category != Some(ItemCategory::Gem)
        && !(item.rarity == Some(ItemRarity::Unique)
            && f.tag == FilterTag::EXPLICIT
            && (f.roll.as_ref().is_some_and(|r| r.bounds.is_some()) || f.hidden.is_some())
            && !grouped)
}

/// `showSourceInfo` computed.
pub fn show_source_info(f: &StatFilter, show_sources: bool) -> bool {
    show_sources
        && !f.sources.is_empty()
        && f.option.is_none()
        && (f.tag == FilterTag::PSEUDO
            || f.sources.len() >= 2
            || f.sources[0].modifier.info.name.is_some()
            || f.sources[0].modifier.info.tier.is_some()
            || f.sources[0].modifier.info.rank.is_some())
}

/// `FilterModifierTiers.vue`: (style, tier) pairs sorted by tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierStyle {
    Tier1,
    Tier2,
    NotTier1,
    MercGilded,
}

pub fn tier_tags(item: &ParsedItem, f: &StatFilter) -> Vec<(TierStyle, u32)> {
    let mut out = Vec::new();
    if let Some(m) = &f.mercenary {
        if let Some(t) = m.tier.filter(|t| *t != 0) {
            let style = match t {
                4 => TierStyle::MercGilded,
                3 => TierStyle::Tier1,
                _ if m.max_tier == Some(true) => TierStyle::Tier2,
                _ => TierStyle::NotTier1,
            };
            out.push((style, t));
        }
    }
    for s in &f.sources {
        let Some(tier) = s.modifier.info.tier.filter(|t| *t != 0) else { continue };
        let normal_tag = matches!(f.tag, FilterTag::EXPLICIT | FilterTag::PSEUDO | FilterTag::Property);
        let jewel_like = matches!(item.category, Some(ItemCategory::Jewel | ItemCategory::ClusterJewel | ItemCategory::MemoryLine | ItemCategory::Idol));
        if normal_tag && !jewel_like {
            if tier == 1 {
                out.push((TierStyle::Tier1, tier));
            } else if tier == 2 {
                out.push((TierStyle::Tier2, tier));
            }
        } else if tier >= 2 {
            out.push((TierStyle::NotTier1, tier));
        }
    }
    out.sort_by_key(|(_, t)| *t);
    out
}

fn draw_tiers(ui: &mut Ui, env: &Env<'_>, tags: &[(TierStyle, u32)]) {
    let c = theme(ui.ctx()).colors;
    let gold = harmonized(ui, Color32::from_rgb(0xEA, 0xB3, 0x08));
    let gilded = harmonized(ui, Color32::from_rgb(0xA1, 0x62, 0x07));
    for (style, tier) in tags {
        let text = if *style == TierStyle::MercGilded { env.t("filters.tier_gilded") } else { env.i18n.tf("filters.tier", &[&tier.to_string()]) };
        match style {
            TierStyle::Tier1 => rpt_m3::tonal_badge(ui, &text, gold.accent, gold.container),
            TierStyle::Tier2 => outlined_badge(ui, &text, gold.accent),
            TierStyle::NotTier1 => rpt_m3::tonal_badge(ui, &text, c.surface_container_highest, c.on_surface_variant),
            TierStyle::MercGilded => rpt_m3::tonal_badge(ui, &text, gilded.container, gilded.on_container),
        };
    }
}

fn outlined_badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let th = theme(ui.ctx());
    let g = ui.painter().layout_no_wrap(text.to_owned(), th.font(TypeRole::LabelMedium), color);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 12.0, (g.size().y + 4.0).max(18.0)), Sense::hover());
    ui.painter().rect_stroke(rect, CornerRadius::same(rpt_m3::shape::SMALL), Stroke::new(1.0, color), StrokeKind::Inside);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, color);
    resp
}

/// Tag chip colors (upstream `.tag-*` classes), harmonized with the theme.
fn tag_style(ui: &Ui, tag: &str) -> TagStyle {
    let c = theme(ui.ctx()).colors;
    let tone = |rgb: u32| {
        let h = harmonized(ui, Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8));
        TagStyle::Filled(h.container, h.on_container)
    };
    match tag {
        "variant" => tone(0xB4_83_09),
        "eldritch" => TagStyle::Gradient(harmonized(ui, Color32::from_rgb(0xB9, 0x1C, 0x1C)).container, harmonized(ui, Color32::from_rgb(0x1D, 0x4E, 0xD8)).container, c.on_surface),
        "corrupted" | "brick" => tone(0xB9_1C_1C),
        "fractured" => tone(0xFD_BA_74),
        "crafted" | "synthesised" => tone(0x25_63_EB),
        "scourge" => tone(0xEA_58_0C),
        "foulborn" => tone(0xBE_18_5D),
        "vestigial" => tone(0x93_33_EA),
        "enchant" => tone(0xC7_D2_FE),
        "pseudo" | "not" => TagStyle::Filled(c.surface_container_highest, c.on_surface_variant),
        t if t.starts_with("explicit-") => TagStyle::Influence(match t {
            "explicit-shaper" => "influence-Shaper.png",
            "explicit-elder" => "influence-Elder.png",
            "explicit-crusader" => "influence-Crusader.png",
            "explicit-hunter" => "influence-Hunter.png",
            "explicit-redeemer" => "influence-Redeemer.png",
            "explicit-warlord" => "influence-Warlord.png",
            "explicit-delve" => "delve.png",
            "explicit-veiled" => "veiled.png",
            "explicit-incursion" => "incursion.png",
            "explicit-infamous" => "mercenary.png",
            "explicit-essence" => "essence.png",
            _ => "rareonly.png",
        }),
        _ => TagStyle::Text,
    }
}

enum TagStyle {
    Filled(Color32, Color32),
    Gradient(Color32, Color32, Color32),
    Influence(&'static str),
    Text,
}

fn draw_tag(ui: &mut Ui, tag: &str, text: &str) {
    let th = theme(ui.ctx());
    let c = th.colors;
    match tag_style(ui, tag) {
        TagStyle::Filled(bg, fg) => {
            rpt_m3::tonal_badge(ui, text, bg, fg);
        }
        TagStyle::Gradient(a, b, fg) => {
            let g = ui.painter().layout_no_wrap(text.to_owned(), th.font(TypeRole::LabelMedium), fg);
            let (rect, _) = ui.allocate_exact_size(vec2(g.size().x + 12.0, 18.0), Sense::hover());
            let (l, r) = rect.split_left_right_at_fraction(0.5);
            ui.painter().rect_filled(l, CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, a);
            ui.painter().rect_filled(r, CornerRadius { nw: 0, sw: 0, ne: 8, se: 8 }, b);
            ui.painter().galley(rect.center() - g.size() / 2.0, g, fg);
        }
        TagStyle::Influence(img) => {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add(egui::Image::new(bundled(img)).fit_to_exact_size(vec2(18.0, 18.0)));
            ui.label(RichText::new(text).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant));
        }
        TagStyle::Text => {
            ui.label(RichText::new(text).font(th.font(TypeRole::LabelMedium)).color(c.outline));
        }
    }
}

/// Q20 notice (`showQ20Notice`).
fn shows_q20(f: &StatFilter) -> bool {
    ["item.armour", "item.evasion_rating", "item.energy_shield", "item.ward", "item.total_dps", "item.physical_dps"].contains(&f.first_trade_id())
}

fn roll_tags(f: &StatFilter) -> Vec<&'static str> {
    let mut out = Vec::new();
    for s in &f.sources {
        match s.stat.roll.as_ref().and_then(|r| r.generation) {
            Some(RollGeneration::Volatile) => {
                out.push("VolatileVaalOrb.png");
                break;
            }
            Some(RollGeneration::Reflecting) => {
                out.push("ReflectingMist.png");
                break;
            }
            _ => {}
        }
    }
    if f.sources.iter().any(|s| s.modifier.info.roll_incr.is_some() && s.stat.roll.as_ref().is_some_and(|r| !r.unscalable)) {
        out.push("increased.png");
    }
    out
}

fn filter_modifier(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem, f: &mut StatFilter, opts: ModOpts<'_>) -> ModOut {
    let links = opts.links.clone();
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut out = ModOut { submit: false };
    let mini = is_mini_filter(item, f);
    let show_inputs = f.roll.is_some() && f.oils.is_none();
    let options = roll_options(env, f);
    let show_bounds = item.rarity == Some(ItemRarity::Unique);
    let text = if is_internal_trade_id(f.first_trade_id()) { env.i18n.tf(f.first_trade_id(), &["#", "#"]) } else { f.text.clone() };
    let roll_value = f.roll.as_ref().map(|r| r.value);
    let mut group_expanded = opts.group_expanded;
    let base_id = ui.scope_id().with(("stat", opts.idx, &f.text));

    let row = ui.scope(|ui| {
        ui.horizontal(|ui| {
            if opts.grouped {
                ui.add_space(20.0);
            }
            ui.spacing_mut().item_spacing.x = 4.0;
            // checkbox
            let unchecked_hint = f.disabled && group_expanded.as_deref().copied() == Some(true);
            let col = if !f.disabled {
                c.primary
            } else if unchecked_hint {
                let t = (ui.input(|i| i.time) * 2.0).sin().abs() as f32;
                ui.ctx().request_repaint();
                rpt_m3::state_layer(c.outline_variant, c.on_surface_variant, t)
            } else {
                c.on_surface_variant
            };
            let cb = ui.add(egui::Label::new(rpt_m3::icon_text(if f.disabled { icons::CHECK_BOX_OUTLINE_BLANK } else { icons::CHECK_BOX }, 20.0).color(col)).sense(Sense::click()));
            if cb.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                f.disabled = !f.disabled;
            }
            // inputs on the right, label takes the rest
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if f.oils.is_some() {
                    anointment(ui, env, f);
                }
                if let Some(l) = &links {
                    draw_links(ui, l);
                } else if show_inputs {
                    let id = base_id.with("roll");
                    if let Some(roll) = &mut f.roll {
                        let mut max = roll.max;
                        let r = rpt_m3::optional_number_field(ui, id.with("max"), &mut max, &env.t("max"), 36.0);
                        if r.gained_focus() {
                            if max.is_none() {
                                max = Some(roll.default.max);
                            }
                            f.disabled = false;
                        }
                        roll.max = max;
                        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            out.submit = true;
                        }
                        let mut min = roll.min;
                        let r = rpt_m3::optional_number_field(ui, id.with("min"), &mut min, &env.t("min"), 36.0);
                        if r.gained_focus() {
                            if min.is_none() {
                                min = Some(roll.default.min);
                            }
                            f.disabled = false;
                        }
                        roll.min = min;
                        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            out.submit = true;
                        }
                    }
                    for img in roll_tags(f) {
                        ui.add(egui::Image::new(bundled(img)).fit_to_exact_size(vec2(20.0, 20.0)));
                    }
                    if shows_q20(f) {
                        let q = rpt_trade::calc_q20::get_trade_max_quality(item);
                        ui.label(RichText::new(env.i18n.tf("item.prop_quality", &[&rpt_m3::fmt_number(q)])).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant));
                    }
                } else if mini {
                    if let Some(opts) = &options {
                        option_buttons(ui, f, opts, false);
                    }
                }
                if mini && !show_inputs {
                    draw_tiers(ui, env, &tier_tags(item, f));
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if let Some(icon) = f.mercenary.as_ref().and_then(|m| m.icon.clone()) {
                        super::ui::icon(ui, &icon, vec2(16.0, 16.0));
                    }
                    if f.not && mini {
                        draw_tag(ui, "not", &env.t("filters.tag_not"));
                    }
                    let expand_w = if group_expanded.is_some() { 24.0 } else { 0.0 };
                    let job = modifier_job(ui, &text, roll_value, false, None);
                    let lab = ui
                        .scope(|ui| {
                            ui.set_max_width((ui.available_width() - expand_w).max(40.0));
                            ui.add(egui::Label::new(job).truncate().sense(Sense::click()))
                        })
                        .inner;
                    let lab = lab.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let full_text = text.clone();
                    let lab = if lab.hovered() {
                        lab.on_hover_ui(|ui| {
                            ui.set_max_width(360.0);
                            ui.label(modifier_job(ui, &full_text, roll_value, true, None));
                        })
                    } else {
                        lab
                    };
                    if lab.clicked() {
                        // smartToggle
                        match group_expanded.as_deref_mut() {
                            Some(exp) if !f.disabled && *exp => *exp = false,
                            exp => {
                                f.disabled = !f.disabled;
                                if let Some(e) = exp {
                                    if !f.disabled && !*e {
                                        *e = true;
                                    }
                                }
                            }
                        }
                    }
                    if let Some(exp) = group_expanded.as_deref_mut() {
                        let b = ui.add(egui::Label::new(rpt_m3::icon_text(if *exp { icons::EXPAND_MORE } else { icons::CHEVRON_RIGHT }, 20.0).color(c.on_surface_variant)).sense(Sense::click()));
                        if b.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            *exp = !*exp;
                        }
                    }
                });
            });
        });

        if !mini {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(if opts.grouped { 44.0 } else { 24.0 });
                if let Some(reason) = &f.hidden {
                    let r = ui.add(egui::Label::new(rpt_m3::icon_text(icons::VISIBILITY_OFF, 14.0).color(c.outline)).sense(Sense::click()));
                    rpt_m3::rich_tooltip(r, None, &env.t(reason));
                }
                let right_w = if f.roll.as_ref().is_some_and(|r| r.bounds.is_some()) && show_bounds { 176.0 } else { 0.0 };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if right_w > 0.0 {
                        ui.add_space(4.0);
                        if let Some(roll) = &mut f.roll {
                            if let Some(b) = roll.bounds {
                                let dp = roll.dp;
                                let res = rpt_m3::range_slider(ui, base_id.with("slider"), (b.min, b.max), roll.value, roll.min, roll.max, 160.0, |v, edge| match edge {
                                    RangeEdge::Min => percent_roll(v, -0.0, RoundMethod::Floor, dp),
                                    RangeEdge::Max => percent_roll(v, 0.0, RoundMethod::Ceil, dp),
                                });
                                if let Some((edge, v)) = res {
                                    match edge {
                                        RangeEdge::Min => roll.min = Some(v),
                                        RangeEdge::Max => roll.max = Some(v),
                                    }
                                    f.disabled = false;
                                }
                            }
                        }
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if f.not {
                            draw_tag(ui, "not", &env.t("filters.tag_not"));
                        }
                        if show_tag(item, f, opts.grouped) {
                            let tag = f.tag.as_str();
                            let mut label = env.t(&format!("filters.tag_{}", tag.replace('-', "_")));
                            if f.sources.len() > 1 {
                                label.push_str(&format!(" x {}", f.sources.len()));
                            }
                            draw_tag(ui, tag, &label);
                        }
                        if !show_bounds {
                            draw_tiers(ui, env, &tier_tags(item, f));
                        }
                        if let Some(opts) = &options {
                            option_buttons(ui, f, opts, true);
                        }
                    });
                });
            });
            if let Some(rt) = &f.reminder_text {
                ui.horizontal(|ui| {
                    ui.add_space(if opts.grouped { 44.0 } else { 24.0 });
                    ui.add(egui::Label::new(RichText::new(rt).italics().font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant)).wrap());
                });
            }
        }
    });
    let rect = row.response.rect;
    rpt_m3::divider(ui);
    // SourceInfo popup under the hovered row
    if show_source_info(f, opts.show_sources) && ui.rect_contains_pointer(rect) {
        egui::Area::new(base_id.with("sources-area"))
            .order(egui::Order::Tooltip)
            .fixed_pos(Pos2::new(rect.left() - 8.0, rect.bottom() + 1.0))
            .interactable(false)
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(c.surface_container_highest)
                    .corner_radius(CornerRadius::same(rpt_m3::shape::SMALL))
                    .inner_margin(Margin::symmetric(12, 8))
                    .shadow(egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: with_alpha(c.shadow, 0.35) })
                    .show(ui, |ui| {
                        ui.set_width(rect.width());
                        for s in &f.sources {
                            source_info(ui, env, s, f);
                        }
                    });
            });
    }
    out
}

fn option_buttons(ui: &mut Ui, f: &mut StatFilter, opts: &[(String, f64)], always_checked: bool) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let Some(current) = f.option.map(|o| o.value) else { return };
    ui.spacing_mut().item_spacing.x = 2.0;
    for (text, value) in opts.iter().rev() {
        let checked = *value == current;
        let solid = checked && !f.disabled;
        let dashed = checked && always_checked && f.disabled;
        let g = ui.painter().layout_no_wrap(text.clone(), th.font(TypeRole::LabelMedium), if solid { c.on_secondary_container } else { c.on_surface_variant });
        let (rect, resp) = ui.allocate_exact_size(vec2((g.size().x + 12.0).max(40.0), 22.0), Sense::click());
        let r = CornerRadius::same(rpt_m3::shape::SMALL);
        let p = ui.painter();
        p.rect_filled(rect, r, if solid { c.secondary_container } else { c.surface_container_highest });
        if solid {
            p.rect_stroke(rect, r, Stroke::new(1.0, c.outline), StrokeKind::Inside);
        } else if dashed {
            let pts = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
            for w in pts.windows(2) {
                p.add(egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.0, c.outline), 3.0, 2.0));
            }
        }
        p.galley(rect.center() - g.size() / 2.0, g, c.on_surface);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            if checked {
                f.disabled = !f.disabled;
            } else if let Some(o) = &mut f.option {
                o.value = *value;
                f.disabled = false;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FilterModifierAnointment.vue

fn anointment(ui: &mut Ui, env: &Env<'_>, f: &StatFilter) {
    let Some(oils) = &f.oils else { return };
    let data = rpt_data::data();
    let mut items = Vec::new();
    for name in oils {
        match data.item_by_ref(rpt_data::Namespace::Item, name).and_then(|v| v.first().map(|b| (*b).clone())) {
            Some(b) => items.push(b),
            None => return,
        }
    }
    let mut total = Some(0.0);
    for o in &items {
        let q = rpt_trade::prices::DbQuery { ns: "ITEM".into(), name: o.ref_name.clone(), variant: None };
        match (total, env.sv.prices.find_price_by_query(&q)) {
            (Some(t), Some(p)) => total = Some(t + p.chaos),
            _ => {
                total = None;
                break;
            }
        }
    }
    let price = total.map(|t| env.sv.prices.auto_currency(t));
    // right-to-left layout: draw price first, then icons
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for o in &items {
            super::ui::icon(ui, &o.icon, vec2(28.0, 28.0));
        }
        ui.add_space(4.0);
        super::ui::quick_price(ui, price.as_ref(), &super::ui::QuickPrice { currency_text: true, item_label: Some(String::new()), ..Default::default() });
    });
}

// ---------------------------------------------------------------------------
// SourceInfo.vue

fn source_info(ui: &mut Ui, env: &Env<'_>, source: &StatSource, f: &StatFilter) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let info = &source.modifier.info;
    let mut text = env.t(&format!("item.mod_{}", info.type_.as_str()));
    if let Some(n) = &info.name {
        text.push_str(&format!(" \"{n}\""));
    }
    if let Some(t) = info.tier {
        text.push_str(&format!(" ({})", env.i18n.tf("item.mod_tier", &[&t.to_string()])));
    }
    if let Some(r) = info.rank {
        text.push_str(&format!(" ({})", env.i18n.tf("item.mod_rank", &[&r.to_string()])));
    }
    ui.label(RichText::new(text).italics().color(c.on_surface_variant));
    let contribution = source.contributes.map(|cv| {
        let roll = f.roll.as_ref();
        let neg = roll.is_some_and(|r| r.is_negated);
        round_roll(cv.value * if neg { -1.0 } else { 1.0 }, roll.is_some_and(|r| r.dp))
    });
    for parsed in &source.modifier.stats {
        ui.horizontal(|ui| {
            if parsed.stat.ref_ != source.stat.stat.ref_ {
                let job = modifier_job(ui, &parsed.translation.string, None, true, Some(c.outline));
                ui.label(job).on_hover_text("—");
                return;
            }
            let p = rpt_parser::modifiers::apply_incr(info, parsed).unwrap_or_else(|| parsed.clone());
            let roll = p.roll.as_ref().map(|r| round_roll(r.value * if p.translation.negate { -1.0 } else { 1.0 }, r.dp));
            ui.label(modifier_job(ui, &p.translation.string, roll, true, None));
            if let Some(cv) = contribution.filter(|v| *v != 0.0) {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    rpt_m3::tonal_badge(ui, &rpt_m3::fmt_number(cv), c.surface_container, c.on_surface);
                });
            }
        });
    }
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// UnknownModifier.vue

fn unknown_modifier(ui: &mut Ui, env: &Env<'_>, text: &str, type_: ModifierType) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let warn = harmonized(ui, Color32::from_rgb(0xF6, 0xAD, 0x55)).accent;
    ui.horizontal(|ui| {
        ui.label(rpt_m3::icon_text(icons::WARNING, 18.0).color(warn));
        let job = modifier_job(ui, text, None, false, None);
        let r = ui.add(egui::Label::new(job).truncate());
        r.on_hover_text(text);
    });
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.label(RichText::new(format!("{} — ", env.t(type_.as_str()))).font(th.font(TypeRole::LabelSmall)).color(c.outline));
        ui.label(RichText::new(env.t("Not recognized modifier")).font(th.font(TypeRole::LabelSmall)).color(warn));
    });
    rpt_m3::divider(ui);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rpt_trade::filters::{FilterRoll, MercenaryFilterInfo, MinMax};

    fn stat(tag: FilterTag) -> StatFilter {
        StatFilter::bare(vec!["explicit.stat_1".into()], "x", "+# to Life", tag, false)
    }

    fn item(text: &str) -> ParsedItem {
        rpt_data::init("en").unwrap();
        rpt_parser::parse_clipboard(text).unwrap()
    }

    const UNIQUE: &str = "Item Class: Rings\nRarity: Unique\nBerek's Grip\nTwo-Stone Ring\n--------\nRequirements:\nLevel: 20\n--------\nItem Level: 80\n--------\n{ Implicit Modifier — Elemental, Fire, Cold, Resistance }\n+15(12-16)% to Fire and Cold Resistances (implicit)\n--------\n{ Unique Modifier — Damage, Physical, Elemental, Cold, Attack }\nAdds 1 to 50(50-70) Cold Damage to Spells and Attacks\n";

    #[test]
    fn tags_and_tiers() {
        let rare = item(
            "Item Class: Rings\nRarity: Rare\nDoom Loop\nRuby Ring\n--------\nItem Level: 80\n--------\n{ Prefix Modifier \"Rotund\" (Tier: 1) — Life }\n+71(70-79) to maximum Life\n",
        );
        let mut f = stat(FilterTag::EXPLICIT);
        assert!(show_tag(&rare, &f, false));
        assert!(!is_mini_filter(&rare, &f));
        f.mercenary = Some(MercenaryFilterInfo { icon: None, tier: Some(4), max_tier: None });
        assert_eq!(tier_tags(&rare, &f), vec![(TierStyle::MercGilded, 4)]);
        f.mercenary = Some(MercenaryFilterInfo { icon: None, tier: Some(2), max_tier: Some(true) });
        assert_eq!(tier_tags(&rare, &f), vec![(TierStyle::Tier2, 2)]);

        let uniq = item(UNIQUE);
        let mut u = stat(FilterTag::EXPLICIT);
        u.roll = Some(FilterRoll {
            value: 50.0,
            min: Some(50.0),
            max: None,
            default: MinMax { min: 50.0, max: 50.0 },
            bounds: Some(MinMax { min: 50.0, max: 70.0 }),
            trade_invert: false,
            dp: false,
            is_negated: false,
            goodness: None,
        });
        // unique explicit with bounds: no tag (slider is shown instead), but grouped shows it
        assert!(!show_tag(&uniq, &u, false));
        assert!(show_tag(&uniq, &u, true));
        assert!(!show_tag(&rare, &stat(FilterTag::Property), false));
    }

    #[test]
    fn merc_links_and_options() {
        let support = |tier, disabled| {
            let mut s = StatFilter::bare(vec!["x".into()], "x", "x", FilterTag::MercenarySupport, disabled);
            s.mercenary = Some(MercenaryFilterInfo { icon: None, tier: Some(tier), max_tier: None });
            s
        };
        let mut six = StatFilter::bare(vec!["item.mercenary_6link".into()], "x", "x", FilterTag::Property, false);
        six.disabled = false;
        let g = FilterGroup {
            group: GroupKind::Mercenary,
            expanded: false,
            meta: StatFilter::bare(vec!["item.not_group".into()], "x", "x", FilterTag::FilterGroup, false),
            stats: vec![six, support(3, false), support(4, true), support(1, false), support(2, false), support(3, false)],
        };
        let st = mercenary_links_state(&g);
        assert!(st.skill_sixlink && st.skill_checked);
        assert_eq!(st.supports.len(), 5);
        assert_eq!(st.supports[1], (Some(4), false));
        assert!(show_source_info(&{
            let mut f = stat(FilterTag::PSEUDO);
            f.sources = Vec::new();
            f
        }, true) == false);
    }
}
