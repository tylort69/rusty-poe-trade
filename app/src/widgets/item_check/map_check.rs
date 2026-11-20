//! Port of upstream `map-check/*`: MapCheck.vue, MapStatButton.vue,
//! prepare-map-stats.ts and common.ts (decisions / profiles).

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Sense, Ui, vec2};
use rpt_m3::{TypeRole, icons, theme};
use rpt_parser::modifiers::{TotalMode, stat_sources_total, translate_stat_with_roll};
use rpt_parser::{ItemRarity, ModifierType, ParsedItem, StatCalculated};
use rpt_trade::filters::util::round_roll;

use crate::config::{MapCheckConfig, MapCheckStat};
use crate::i18n::I18n;
use crate::widgets::price_check::ui::{harmonized, modifier_job};

pub const MEMORY_MAPS: &[&str] = &["Courtyard of Wasting", "Chambers of Impurity", "Theatre of Lies"];

// ---------------------------------------------------------------------------
// common.ts

/// Three profiles stored as "---": `-` none, `d` deadly, `w` warning, `g` good, `s` seen.
pub fn decision_has_color(decision_set: &str, profile: u32) -> bool {
    let d = decision_at(decision_set, profile);
    d != '-' && d != 's'
}

pub fn decision_at(decision_set: &str, profile: u32) -> char {
    decision_set.chars().nth(profile.saturating_sub(1) as usize).unwrap_or('-')
}

pub fn next_decision(current: char) -> char {
    match current {
        'd' => 'w',
        'w' => 'g',
        'g' => 's',
        _ => 'd',
    }
}

pub fn decision_create(value: char, profile: u32, update_set: Option<&str>) -> String {
    let mut chars: Vec<char> = update_set.unwrap_or("---").chars().collect();
    let idx = profile.saturating_sub(1) as usize;
    while chars.len() <= idx {
        chars.push('-');
    }
    chars[idx] = value;
    chars.into_iter().collect()
}

pub fn is_outdated(profile: u32, stat: &MapCheckStat) -> bool {
    decision_has_color(&stat.decision, profile) && rpt_data::data().stat_by_match_str_v2(&stat.matcher).is_none()
}

/// Current decision of `matcher` in the active profile.
pub fn current_decision(cfg: &MapCheckConfig, matcher: &str) -> char {
    cfg.selected_stats.iter().find(|s| s.matcher == matcher).map(|s| decision_at(&s.decision, cfg.profile)).unwrap_or('-')
}

/// `decision` setter of `MapStatButton.vue`.
pub fn set_decision(cfg: &mut MapCheckConfig, matcher: &str, value: char) {
    let profile = cfg.profile;
    match cfg.selected_stats.iter_mut().find(|s| s.matcher == matcher) {
        Some(e) => e.decision = decision_create(value, profile, Some(&e.decision)),
        None => cfg.selected_stats.push(MapCheckStat { matcher: matcher.to_owned(), decision: decision_create(value, profile, None) }),
    }
}

// ---------------------------------------------------------------------------
// prepare-map-stats.ts

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedStat {
    pub matcher: String,
    pub roll: Option<f64>,
}

pub fn prepare_map_stat(calc: &StatCalculated) -> PreparedStat {
    let roll = stat_sources_total(&calc.sources, TotalMode::Sum);
    let translation = translate_stat_with_roll(calc, roll.as_ref());
    let mut prepared = PreparedStat { matcher: translation.string, roll: roll.map(|r| round_roll(r.value, translation.dp.unwrap_or(false))) };
    if translation.negate {
        prepared.roll = prepared.roll.map(|r| -r);
    }
    prepared
}

// ---------------------------------------------------------------------------
// MapCheck.vue

pub fn map_name(item: &ParsedItem) -> String {
    if item.info.area.as_ref().is_some_and(|a| a.blighted) {
        return item.info.name.clone();
    }
    item.map_area.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| item.info.name.clone())
}

pub fn map_image(item: &ParsedItem) -> Option<String> {
    if (item.rarity == Some(ItemRarity::Unique) && item.is_unidentified) || item.info.area.as_ref().is_some_and(|a| a.blighted) {
        return None;
    }
    item.info
        .area
        .as_ref()
        .and_then(|a| a.screenshot.clone())
        .or_else(|| item.map_area.as_ref().and_then(|m| m.area.as_ref()).and_then(|a| a.screenshot.clone()))
}

pub fn map_check(ui: &mut Ui, i18n: &I18n, item: &ParsedItem, cfg: &mut MapCheckConfig) -> bool {
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut changed = false;
    ui.set_min_width(320.0);
    ui.set_max_width(480.0);
    // header: name + profile selector
    ui.horizontal(|ui| {
        ui.label(RichText::new(map_name(item)).font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut p = cfg.profile;
            if rpt_m3::segmented(ui, &mut p, &[(1, "I"), (2, "II"), (3, "III")]) {
                cfg.profile = p;
                changed = true;
            }
            ui.label(RichText::new(i18n.t("map_check.profile")).color(c.on_surface_variant));
        });
    });
    if let Some(url) = map_image(item) {
        let w = ui.available_width();
        ui.add(egui::Image::new(url).fit_to_exact_size(vec2(w, w * 9.0 / 21.0)).corner_radius(CornerRadius::same(rpt_m3::shape::MEDIUM)));
    }
    if item.map_area.as_ref().is_some_and(|a| MEMORY_MAPS.contains(&a.ref_name.as_str())) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(crate::widgets::price_check::ui::bundled("maven-witness.png")).fit_to_exact_size(vec2(32.0, 32.0)));
            ui.label(RichText::new(i18n.t("map_check.maven_reminder")).italics().color(c.on_surface_variant));
        });
    }
    let stats: Vec<PreparedStat> = item.stats_by_type.iter().filter(|s| s.type_ == ModifierType::Explicit).map(prepare_map_stat).collect();
    if stats.is_empty() {
        ui.label(i18n.t("map_check.no_mods"));
    } else {
        ui.spacing_mut().item_spacing.y = 4.0;
        for s in &stats {
            if map_stat_button(ui, s, cfg) {
                changed = true;
            }
        }
        for m in item.unknown_modifiers.iter().filter(|m| m.type_ == ModifierType::Explicit) {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} —", i18n.t("Not recognized modifier"))).color(harmonized(ui, Color32::from_rgb(0xF6, 0xAD, 0x55)).accent));
                ui.label(&m.text);
            });
        }
    }
    let profile = cfg.profile;
    if cfg.selected_stats.iter().any(|e| is_outdated(profile, e)) {
        rpt_m3::banner(ui, icons::INFO, i18n.t("map_check.has_outdated"), false);
    }
    changed
}

/// `MapStatButton.vue`: click cycles danger → warning → good → seen → danger.
fn map_stat_button(ui: &mut Ui, stat: &PreparedStat, cfg: &mut MapCheckConfig) -> bool {
    let th = theme(ui.ctx());
    let c = th.colors;
    let decision = current_decision(cfg, &stat.matcher);
    let (tint, icon) = match decision {
        'd' => (Some(harmonized(ui, Color32::from_rgb(0xC5, 0x30, 0x30))), Some(icons::DANGEROUS)),
        'w' => (Some(harmonized(ui, Color32::from_rgb(0xDD, 0x6B, 0x20))), Some(icons::WARNING)),
        'g' => (Some(harmonized(ui, Color32::from_rgb(0x2F, 0x85, 0x5A))), Some(icons::CHECK)),
        _ => (None, None),
    };
    let mut changed = false;
    ui.horizontal(|ui| {
        let w = ui.available_width() - 36.0;
        let resp = Frame::new()
            .fill(tint.map(|t| t.container).unwrap_or(Color32::TRANSPARENT))
            .corner_radius(CornerRadius::same(rpt_m3::shape::SMALL))
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(w - 16.0);
                ui.horizontal(|ui| {
                    let fg = tint.map(|t| t.on_container).unwrap_or(c.on_surface);
                    ui.add_sized(vec2(18.0, 18.0), egui::Label::new(rpt_m3::icon_text(icon.unwrap_or(" "), 18.0).color(fg)));
                    ui.add(egui::Label::new(modifier_job(ui, &stat.matcher, stat.roll, false, Some(fg))).truncate().selectable(false));
                });
            })
            .response
            .interact(Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(resp.rect, CornerRadius::same(rpt_m3::shape::SMALL), rpt_m3::with_alpha(c.on_surface, 0.08));
        }
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            set_decision(cfg, &stat.matcher, next_decision(decision));
            changed = true;
        }
        let seen_visible = cfg.show_new_stats && decision == '-';
        let r = ui.add_sized(vec2(28.0, 28.0), egui::Label::new(rpt_m3::icon_text(if seen_visible { icons::VISIBILITY_OFF } else { " " }, 16.0).color(c.outline)).sense(Sense::click()));
        if r.clicked() && cfg.show_new_stats {
            set_decision(cfg, &stat.matcher, if decision == '-' { 's' } else { '-' });
            changed = true;
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions() {
        assert!(decision_has_color("d--", 1));
        assert!(!decision_has_color("d--", 2));
        assert!(!decision_has_color("s--", 1));
        assert_eq!(next_decision('-'), 'd');
        assert_eq!(next_decision('d'), 'w');
        assert_eq!(next_decision('w'), 'g');
        assert_eq!(next_decision('g'), 's');
        assert_eq!(next_decision('s'), 'd');
        assert_eq!(decision_create('w', 2, None), "-w-");
        assert_eq!(decision_create('g', 3, Some("d--")), "d-g");
    }

    #[test]
    fn profile_updates() {
        let mut cfg = MapCheckConfig { profile: 2, show_new_stats: true, selected_stats: vec![MapCheckStat { matcher: "A".into(), decision: "d--".into() }] };
        assert_eq!(current_decision(&cfg, "A"), '-');
        set_decision(&mut cfg, "A", 'g');
        assert_eq!(cfg.selected_stats[0].decision, "dg-");
        set_decision(&mut cfg, "B", 'd');
        assert_eq!(cfg.selected_stats[1].decision, "-d-");
        assert_eq!(current_decision(&cfg, "B"), 'd');
        cfg.profile = 1;
        assert_eq!(current_decision(&cfg, "A"), 'd');
    }

    #[test]
    fn outdated_and_prepared() {
        rpt_data::init("en").unwrap();
        let ok = MapCheckStat { matcher: "# to maximum Life".into(), decision: "d--".into() };
        assert!(rpt_data::data().stat_by_match_str_v2("# to maximum Life").is_some());
        assert!(!is_outdated(1, &ok));
        let old = MapCheckStat { matcher: "This stat no longer exists #".into(), decision: "d--".into() };
        assert!(is_outdated(1, &old));
        assert!(!is_outdated(2, &old));

        let item = rpt_parser::parse_clipboard(
            "Item Class: Maps\nRarity: Magic\nAbyssal Strand Map of Reflection\n--------\nMap Tier: 16\nItem Quantity: +10% (augmented)\n--------\nItem Level: 83\n--------\nMonster Level: 83\n--------\n{ Suffix Modifier \"of Toughness\" }\nMonsters cannot be Stunned\n",
        );
        if let Ok(item) = item {
            let stats: Vec<PreparedStat> = item.stats_by_type.iter().filter(|s| s.type_ == ModifierType::Explicit).map(prepare_map_stat).collect();
            assert!(stats.iter().any(|s| s.matcher == "Monsters cannot be Stunned"), "{stats:?}");
        }
    }
}
