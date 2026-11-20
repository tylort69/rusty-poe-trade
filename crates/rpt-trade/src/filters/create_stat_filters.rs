//! Port of upstream `filters/create-stat-filters.ts`.

use rpt_data::StatBetter;
use rpt_parser::modifiers::{TotalMode, stat_sources_total, translate_stat_with_roll};
use rpt_parser::{
    ItemCategory, ItemRarity, ModGeneration, ModifierMechanic, ModifierType, ParsedItem, RollGeneration,
    StatCalculated,
};

use super::pseudo::{self, anointments, atzoatl_rules, flasks, heist, item_property, maps, reflection_rules};
use super::util::{RoundMethod, percent_roll, percent_roll_delta, round_roll};
use super::*;
use crate::calc_q20::{QUALITY_CHANGING_ENCHANT, get_prop_quality};

pub struct FiltersCreationContext<'a> {
    pub item: &'a ParsedItem,
    pub search_in_range: f64,
    pub filters: Vec<StatFilter>,
    pub groups: Vec<FilterGroup>,
    pub stats_by_type: Vec<StatCalculated>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatFiltersMode {
    #[default]
    Default,
    Props,
    Bulk,
}

pub fn create_exact_stat_filters(
    item: &ParsedItem,
    stats_by_type: &[StatCalculated],
    search_stat_range: f64,
    mode: StatFiltersMode,
) -> Vec<StatFilter> {
    if item.info.area.as_ref().is_some_and(|a| a.blighted) || item.category == Some(ItemCategory::Invitation) {
        return Vec::new();
    }

    let mut keep_by_type = vec![
        ModifierType::Pseudo,
        ModifierType::Fractured,
        ModifierType::Enchant,
        ModifierType::Necropolis,
        ModifierType::Imbued,
    ];

    if item.influences.is_empty()
        && !item.is_fractured
        && !matches!(item.category, Some(ItemCategory::Tincture | ItemCategory::Idol | ItemCategory::Chart))
    {
        keep_by_type.push(ModifierType::Implicit);
    }

    if item.rarity == Some(ItemRarity::Magic)
        && !matches!(
            item.category,
            Some(
                ItemCategory::ClusterJewel
                    | ItemCategory::Map
                    | ItemCategory::HeistContract
                    | ItemCategory::HeistBlueprint
                    | ItemCategory::Chart
                    | ItemCategory::Sentinel
            )
        )
    {
        keep_by_type.push(ModifierType::Explicit);
    } else if item.rarity == Some(ItemRarity::Rare) && item.category == Some(ItemCategory::Idol) {
        keep_by_type.push(ModifierType::Explicit);
    }

    if item.category == Some(ItemCategory::Flask) {
        keep_by_type.push(ModifierType::Crafted);
    }

    let mut ctx = FiltersCreationContext {
        item,
        search_in_range: if mode != StatFiltersMode::Props { search_stat_range.min(2.0) } else { search_stat_range },
        filters: Vec::new(),
        groups: Vec::new(),
        stats_by_type: stats_by_type
            .iter()
            .filter(|calc| {
                keep_by_type.contains(&calc.type_)
                    || calc
                        .sources
                        .iter()
                        .any(|s| s.modifier.info.mechanic == Some(ModifierMechanic::RareOnly))
            })
            .cloned()
            .collect(),
    };

    item_property::filter_base_percentile(&mut ctx);
    item_property::filter_memory_strands(&mut ctx, None);
    maps::map_props(mode == StatFiltersMode::Bulk, &mut ctx);
    maps::chart_props(mode == StatFiltersMode::Bulk, &mut ctx);
    maps::valdo_bad_mods(&mut ctx);

    let range = ctx.search_in_range;
    let new_filters: Vec<StatFilter> =
        ctx.stats_by_type.iter().map(|m| calculated_stat_to_filter(m, range, item)).collect();
    ctx.filters.extend(new_filters);

    if item.info.ref_name == "Chronicle of Atzoatl" {
        atzoatl_rules::apply_rules(&mut ctx.filters);
        return ctx.filters;
    }
    if item.info.ref_name == "Mirrored Tablet" {
        reflection_rules::apply_rules(&mut ctx.filters);
        return ctx.filters;
    }

    for filter in &mut ctx.filters {
        if filter.not {
            continue;
        }

        filter.hidden = None;

        if filter.tag == FilterTag::EXPLICIT {
            filter.disabled = !filter
                .sources
                .iter()
                .any(|source| source.modifier.info.tier.is_some_and(|t| t <= 2));
        } else if filter.tag != FilterTag::Property && filter.tag != FilterTag::PSEUDO {
            filter.disabled = false;
        }

        if filter.stat_ref == "# uses remaining" {
            if let Some(roll) = &mut filter.roll {
                roll.min = Some(roll.value);
                roll.default.min = roll.value;
                roll.default.max = roll.value;
            }
        }
    }

    if item.category == Some(ItemCategory::ClusterJewel) {
        apply_cluster_jewel_rules(&mut ctx.filters, true);
    }
    if matches!(item.category, Some(ItemCategory::HeistContract | ItemCategory::HeistBlueprint)) {
        heist::apply_heist_rules(&mut ctx);
    } else if item.category == Some(ItemCategory::Flask) {
        apply_flask_rules(&mut ctx.filters);
        flasks::apply_flask_hybrid_mod(&mut ctx);
    } else if matches!(
        item.category,
        Some(ItemCategory::MemoryLine | ItemCategory::SanctumRelic | ItemCategory::Charm)
    ) {
        enable_all_filters(&mut ctx.filters);
    } else if item.category == Some(ItemCategory::Idol) {
        enable_good_rolled_filters(&mut ctx.filters, 0.66);
    }

    ctx.filters
}

pub fn init_ui_mod_filters(item: &ParsedItem, search_stat_range: f64) -> Vec<FilterOrGroup> {
    let mut ctx = FiltersCreationContext {
        item,
        filters: Vec::new(),
        groups: Vec::new(),
        search_in_range: if item.rarity == Some(ItemRarity::Normal) { 100.0 } else { search_stat_range },
        stats_by_type: item
            .stats_by_type
            .iter()
            .map(|calc| {
                if calc.type_ == ModifierType::Fractured && calc.stat.trade.ids.contains_key("explicit") {
                    StatCalculated { type_: ModifierType::Explicit, ..calc.clone() }
                } else {
                    calc.clone()
                }
            })
            .collect(),
    };

    item_property::filter_equipment_props(&mut ctx);
    if item.rarity == Some(ItemRarity::Unique) {
        item_property::filter_base_percentile(&mut ctx);
    }
    item_property::filter_memory_strands(&mut ctx, Some("hide_memory_strands"));
    if item.info.ref_name != "Split Personality" {
        pseudo::filter_pseudo(&mut ctx);
    }
    if item.unique_base.as_ref().is_some_and(|b| b.ref_name == "Timeless Jewel") {
        pseudo::timeless_jewel::filter_timeless_jewel_keystones(&mut ctx);
    }

    if item.is_corrupted.is_none() && !item.is_mirrored {
        ctx.stats_by_type.retain(|m| m.type_ != ModifierType::Fractured);
        ctx.stats_by_type
            .extend(item.stats_by_type.iter().filter(|m| m.type_ == ModifierType::Fractured).cloned());
    }

    if item.is_veiled {
        ctx.stats_by_type.retain(|m| m.type_ != ModifierType::Veiled);
    }

    let range = ctx.search_in_range;
    let new_filters: Vec<StatFilter> =
        ctx.stats_by_type.iter().map(|m| calculated_stat_to_filter(m, range, item)).collect();
    ctx.filters.extend(new_filters);

    if item.is_veiled {
        for filter in &mut ctx.filters {
            filter.disabled = true;
        }
    }

    final_filter_tweaks(&mut ctx);

    let mut out: Vec<FilterOrGroup> = ctx.filters.into_iter().map(FilterOrGroup::Stat).collect();
    out.extend(ctx.groups.into_iter().map(FilterOrGroup::Group));
    out
}

pub fn calculated_stat_to_filter(calc: &StatCalculated, percent: f64, item: &ParsedItem) -> StatFilter {
    let mut percent = percent;
    let StatCalculated { stat, sources, type_ } = calc;
    let type_ = *type_;
    let trade_id = stat.trade.ids.get(type_.as_str()).cloned().unwrap_or_default();

    let mut option_filter: Option<StatFilter> = None;
    if stat.trade.option {
        let mut f = StatFilter::bare(
            trade_id.clone(),
            &stat.ref_,
            sources.first().map(|s| s.stat.translation.string.as_str()).unwrap_or(""),
            if type_ == ModifierType::Enchant { FilterTag::ENCHANT } else { FilterTag::Variant },
            false,
        );
        f.reminder_text = sources.first().and_then(|s| s.stat.stat.reminder_text.clone());
        f.sources = sources.clone();
        f.option = Some(FilterOption {
            value: sources.first().and_then(|s| s.contributes).map(|c| c.value).unwrap_or(0.0),
        });
        option_filter = Some(f);
    }

    let total_mode = if item.info.ref_name == "Mirrored Tablet" { TotalMode::Max } else { TotalMode::Sum };
    let roll = stat_sources_total(sources, total_mode);
    let translation = translate_stat_with_roll(calc, roll.as_ref());

    let mut filter = option_filter.unwrap_or_else(|| {
        let mut f = StatFilter::bare(trade_id, &stat.ref_, &translation.string, FilterTag::Mod(type_), true);
        f.oils = anointments::decode_oils(calc);
        f.reminder_text = sources.iter().find_map(|s| s.stat.stat.reminder_text.clone());
        f.sources = sources.clone();
        f
    });

    if stat.better == StatBetter::NotComparable {
        if type_ != ModifierType::Enchant {
            filter.tag = FilterTag::Variant;
        }
        if filter.oils.is_none() {
            filter.disabled = false;
        }
    }

    if type_ == ModifierType::Implicit || type_ == ModifierType::Explicit {
        if type_ == ModifierType::Explicit {
            if let Some(unique) = &item.info.unique {
                if let Some(fixed_stats) = &unique.fixed_stats {
                    if !fixed_stats.contains(&filter.stat_ref) {
                        filter.tag = FilterTag::Variant;
                    }
                } else if sources.iter().any(|s| s.modifier.info.generation.is_some()) {
                    // recognized unveiled mods are overridden below, this is kept as fallback
                    filter.tag = FilterTag::Variant;
                }
            }
        }

        if let Some(mechanic) = sources.iter().find_map(|s| s.modifier.info.mechanic) {
            filter.tag = FilterTag::Mechanic(mechanic);
        }
    }

    if let (Some(roll), None) = (roll, &filter.option) {
        if (item.rarity == Some(ItemRarity::Magic)
            && (item.is_unmodifiable || item.is_corrupted.is_some() || item.is_mirrored))
            || item.info.ref_name == "Viscera"
        {
            percent = 0.0;
        } else if item.rarity == Some(ItemRarity::Unique)
            || (item.rarity == Some(ItemRarity::Magic) && item.category == Some(ItemCategory::Jewel))
            || calc.sources.iter().any(|s| {
                s.modifier.info.tier == Some(1) && s.modifier.info.type_ == ModifierType::Fractured
            })
        {
            let perfect_roll = (stat.better == StatBetter::PositiveRoll && roll.value >= roll.max)
                || (stat.better == StatBetter::NegativeRoll && roll.value <= roll.min);
            if perfect_roll {
                percent = 0.0;
            }
        }

        let mut goodness = None;
        if stat.better != StatBetter::NotComparable {
            if roll.min == roll.max {
                goodness = Some(1.0);
            } else {
                let mut g = (roll.value - roll.min) / (roll.max - roll.min);
                if stat.better == StatBetter::NegativeRoll {
                    g = 1.0 - g;
                }
                goodness = Some(g);
            }
        }

        let dp = stat.dp
            || calc
                .sources
                .iter()
                .any(|s| s.stat.stat.ref_ == stat.ref_ && s.stat.roll.as_ref().is_some_and(|r| r.dp));

        let filter_bounds = MinMax {
            min: percent_roll(roll.min, -0.0, RoundMethod::Floor, dp),
            max: percent_roll(roll.max, 0.0, RoundMethod::Ceil, dp),
        };

        let mut filter_default = if stat.better == StatBetter::NotComparable {
            MinMax { min: roll.value, max: roll.value }
        } else if item.rarity == Some(ItemRarity::Unique) {
            MinMax {
                min: percent_roll_delta(roll.value, roll.max - roll.min, -percent, RoundMethod::Floor, dp),
                max: percent_roll_delta(roll.value, roll.max - roll.min, percent, RoundMethod::Ceil, dp),
            }
        } else {
            MinMax {
                min: percent_roll(roll.value, -percent, RoundMethod::Floor, dp),
                max: percent_roll(roll.value, percent, RoundMethod::Ceil, dp),
            }
        };
        filter_default.min = filter_default.min.max(filter_bounds.min);
        filter_default.max = filter_default.max.min(filter_bounds.max);

        let mut froll = FilterRoll {
            value: round_roll(roll.value, dp),
            min: None,
            max: None,
            default: filter_default,
            bounds: if roll.min != roll.max && stat.better != StatBetter::NotComparable {
                Some(filter_bounds)
            } else {
                None
            },
            dp,
            is_negated: false,
            trade_invert: stat.trade.inverted,
            goodness,
        };

        filter_fill_min_max(&mut froll, stat.better);

        if translation.negate {
            filter_adjustment_for_negate(&mut froll);
        }
        filter.roll = Some(froll);
    }

    filter
}

fn hide_not_variable_stat(filter: &mut StatFilter, item: &ParsedItem) {
    if item.rarity != Some(ItemRarity::Unique)
        || (filter.tag != FilterTag::EXPLICIT && filter.tag != FilterTag::Property)
    {
        return;
    }

    // always show stat increased by Catalyst
    if item.quality.is_some_and(|q| q != 0.0)
        && item.category.is_some_and(|c| c.is_jewellery())
        && filter.sources.iter().any(|s| {
            s.modifier.info.roll_incr.is_some_and(|v| v != 0.0) && s.stat.roll.as_ref().is_some_and(|r| !r.unscalable)
        })
    {
        return;
    }

    // always show scalable stat on corrupted item
    let has_roll_and_scalable = filter.sources.iter().any(|s| s.stat.roll.as_ref().is_some_and(|r| !r.unscalable));
    if item.is_corrupted.is_some() && has_roll_and_scalable {
        let is_vvo_corrupted = item.new_mods.iter().any(|m| {
            m.stats
                .iter()
                .any(|s| s.roll.as_ref().is_some_and(|r| r.generation == Some(RollGeneration::Volatile)))
        });
        if !is_vvo_corrupted {
            // some stat not being reduced compared to other VVO corrupted items can be important
            filter.disabled = false;
        }
        return;
    }

    let first_id = filter.first_trade_id().to_owned();
    let all_const_sources = filter.sources.iter().all(|s| {
        s.stat.roll.as_ref().map(|r| r.min) == s.stat.roll.as_ref().map(|r| r.max)
    });
    match &mut filter.roll {
        None => {
            filter.hidden = Some("filters.hide_const_roll".to_owned());
            filter.disabled = true;
        }
        Some(roll) if roll.bounds.is_none() => {
            roll.min = None;
            roll.max = None;
            filter.hidden = Some("filters.hide_const_roll".to_owned());
            filter.disabled = true;
        }
        Some(roll)
            if item_property::BASE_PCTL_AFFECTED_IDS.contains(&first_id.as_str())
                && all_const_sources
                && get_prop_quality(item) < 21.0 =>
        {
            roll.min = None;
            roll.max = None;
            filter.hidden = Some("hide_variable_by_base_percentile_only".to_owned());
            filter.disabled = true;
        }
        Some(_) => {}
    }

    if item.is_foulborn && filter.tag == FilterTag::EXPLICIT {
        // some mod not being replaced with foulborn one can be important
        filter.disabled = false;
    }
}

fn filter_fill_min_max(roll: &mut FilterRoll, better: StatBetter) {
    match better {
        StatBetter::PositiveRoll => roll.min = Some(roll.default.min),
        StatBetter::NegativeRoll => roll.max = Some(roll.default.max),
        StatBetter::NotComparable => {
            roll.min = Some(roll.default.min);
            roll.max = Some(roll.default.max);
        }
    }
}

fn filter_adjustment_for_negate(roll: &mut FilterRoll) {
    roll.trade_invert = !roll.trade_invert;
    roll.is_negated = true;
    let swap = roll.clone();

    if let (Some(sb), Some(b)) = (swap.bounds, &mut roll.bounds) {
        b.min = -sb.max;
        b.max = -sb.min;
    }

    roll.default.min = -swap.default.max;
    roll.default.max = -swap.default.min;

    roll.value = -swap.value;
    roll.min = swap.max.map(|v| -v);
    roll.max = swap.min.map(|v| -v);
}

fn final_filter_tweaks(ctx: &mut FiltersCreationContext<'_>) {
    let item = ctx.item;

    if item.category == Some(ItemCategory::ClusterJewel) && item.rarity != Some(ItemRarity::Unique) {
        apply_cluster_jewel_rules(&mut ctx.filters, false);
    } else if item.category == Some(ItemCategory::Flask) {
        apply_flask_rules(&mut ctx.filters);
        flasks::apply_flask_hybrid_mod(ctx);
    }

    if let Some(has_empty_modifier) = show_has_empty_modifier(ctx) {
        let mut f = StatFilter::bare(
            vec!["item.has_empty_modifier".to_owned()],
            "1 Empty or Crafted Modifier",
            "1 Empty or Crafted Modifier",
            FilterTag::PSEUDO,
            true,
        );
        f.hidden = Some("filters.hide_empty_mod".to_owned());
        f.option = Some(FilterOption { value: has_empty_modifier as u8 as f64 });
        ctx.filters.push(f);
    }

    if matches!(item.category, Some(ItemCategory::Amulet | ItemCategory::Ring)) {
        anointments::apply_anointment_rules(&mut ctx.filters, item);
    }

    for filter in &mut ctx.filters {
        hide_not_variable_stat(filter, item);

        if filter.tag == FilterTag::ENCHANT {
            if QUALITY_CHANGING_ENCHANT.contains(&filter.stat_ref.as_str()) {
                filter.hidden = Some("hide_enchant_meta_stat".to_owned());
            }
        } else if filter.tag == FilterTag::FRACTURED {
            let has_explicit = item
                .stats_by_type
                .iter()
                .find(|m| m.stat.ref_ == filter.stat_ref)
                .is_some_and(|m| m.stat.trade.ids.contains_key("explicit"));
            if has_explicit {
                // hide only if fractured mod has corresponding explicit variant
                filter.hidden = Some("filters.hide_for_crafting".to_owned());
            }
        } else if filter.sources.first().is_some_and(|s| s.stat.stat.jewellery_quality.is_some()) {
            filter.hidden = Some("hide_jewellery_quality".to_owned());
        } else if filter.tag == FilterTag::IMPLICIT {
            if item.rarity == Some(ItemRarity::Unique)
                && item.is_corrupted.is_none()
                && item.category != Some(ItemCategory::Jewel)
            {
                // hide not Corrupted, Vestigial implicits etc., that were not consumed by pseudo stats
                filter.hidden = Some("hide_unique_base_implicit".to_owned());
                filter.disabled = true;
            }
        } else if filter.tag == FilterTag::FOULBORN
            || filter.tag == FilterTag::VESTIGIAL
            || filter.tag == FilterTag::Variant
        {
            filter.disabled = false;
        }
    }

    if item.rarity == Some(ItemRarity::Unique) {
        let has_visible_armour_prop = ctx.filters.iter().any(|f| {
            item_property::BASE_PCTL_AFFECTED_IDS.contains(&f.first_trade_id()) && f.hidden.is_none()
        });
        if let Some(base_percentile) =
            ctx.filters.iter_mut().find(|f| f.first_trade_id() == "item.base_percentile")
        {
            if has_visible_armour_prop {
                base_percentile.hidden = Some("hide_redundant".to_owned());
                base_percentile.disabled = true;
            }
        }

        let count_visible = ctx.filters.iter().filter(|f| f.hidden.is_none()).count();
        if count_visible <= 3 {
            enable_all_filters(&mut ctx.filters);
        }
    }
}

fn apply_cluster_jewel_rules(filters: &mut [StatFilter], exact: bool) {
    for filter in filters {
        if filter.stat_ref == "# Added Passive Skills are Jewel Sockets" {
            filter.hidden = Some("filters.hide_const_roll".to_owned());
            filter.disabled = true;
        }

        // https://www.poewiki.net/wiki/Cluster_Jewel#Optimal_passive_skill_amounts
        if filter.stat_ref == "Adds # Passive Skills" {
            filter.disabled = false;
            let Some(roll) = &mut filter.roll else { continue };

            let v = roll.value;
            // 4 is [_, 5]
            if v == 4.0 {
                roll.max = Some(5.0);
            // 5 is [5, 5] (and [_, 5] for Rare jewel)
            } else if v == 5.0 && exact {
                roll.min = Some(roll.default.min);
            // 3, 6, 10, 11, 12 are [n, _]
            } else if [3.0, 6.0, 10.0, 11.0, 12.0].contains(&v) {
                roll.min = Some(roll.default.min);
                roll.max = None;
            }
            // else 2, 5(Rare), 8, 9 are [_ , n]
        }
    }
}

fn apply_flask_rules(filters: &mut [StatFilter]) {
    let used_enkindling = filters.iter().any(|f| f.stat_ref == "Gains no Charges during Flask Effect");
    for filter in filters {
        if filter.tag == FilterTag::ENCHANT && !used_enkindling {
            filter.hidden = Some("hide_harvest_and_instilling".to_owned());
            filter.disabled = true;
        }
    }
}

// TODO
// +1 Prefix Modifier allowed
// -1 Suffix Modifier allowed
fn show_has_empty_modifier(ctx: &FiltersCreationContext<'_>) -> Option<ItemHasEmptyModifier> {
    let item = ctx.item;

    if item.rarity != Some(ItemRarity::Rare) || item.is_corrupted.is_some() || item.is_mirrored {
        return None;
    }

    let random_mods: Vec<_> = item
        .new_mods
        .iter()
        .filter(|m| {
            matches!(
                m.info.type_,
                ModifierType::Explicit | ModifierType::Fractured | ModifierType::Veiled | ModifierType::Crafted
            )
        })
        .collect();

    let crafted_mod = random_mods.iter().find(|m| m.info.type_ == ModifierType::Crafted);

    if (random_mods.len() == 5 && crafted_mod.is_none()) || (random_mods.len() == 6 && crafted_mod.is_some()) {
        let mut prefixes =
            random_mods.iter().filter(|m| m.info.generation == Some(ModGeneration::Prefix)).count() as i32;
        let mut suffixes =
            random_mods.iter().filter(|m| m.info.generation == Some(ModGeneration::Suffix)).count() as i32;

        if let Some(crafted) = crafted_mod {
            if crafted.info.generation == Some(ModGeneration::Prefix) {
                prefixes -= 1;
            } else {
                suffixes -= 1;
            }
        }

        if prefixes == 2 {
            return Some(ItemHasEmptyModifier::Prefix);
        }
        if suffixes == 2 {
            return Some(ItemHasEmptyModifier::Suffix);
        }
    }

    None
}

pub(crate) fn enable_all_filters(filters: &mut [StatFilter]) {
    for filter in filters {
        if filter.hidden.is_none() {
            filter.disabled = false;
        }
    }
}

fn enable_good_rolled_filters(filters: &mut [StatFilter], above_pct: f64) {
    for filter in filters {
        if filter.hidden.is_some() {
            continue;
        }
        match filter.roll.as_ref().and_then(|r| r.goodness) {
            None => filter.disabled = false,
            Some(g) if g >= above_pct => filter.disabled = false,
            Some(_) => {}
        }
    }
}
