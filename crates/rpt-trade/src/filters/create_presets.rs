//! Port of upstream `filters/create-presets.ts`.

use rpt_parser::modifiers::sum_stats_by_mod_type;
use rpt_parser::{ItemCategory, ItemRarity, ModifierType, ParsedItem};

use super::create_item_filters::{CreateOptions, create_filters};
use super::create_stat_filters::{StatFiltersMode, create_exact_stat_filters, init_ui_mod_filters};
use super::pseudo::mercenary::create_mercenary_filters;
use super::{CollapseListings, FilterOrGroup, FilterPreset, Presets, StatFilter};

const ROMAN_NUMERALS: &[&str] = &["I", "II", "III", "IV", "V"];

pub const PRESET_EXACT: &str = "filters.preset_exact";
pub const PRESET_BULK: &str = "filters.preset_bulk";
pub const PRESET_PSEUDO: &str = "filters.preset_pseudo";
pub const PRESET_BASE_ITEM: &str = "filters.preset_base_item";

/// Upstream `CreatePresetsOptions` (= `CreateOptions` without `exact` + `searchStatRange`).
#[derive(Debug, Clone, PartialEq)]
pub struct CreatePresetsOptions {
    pub league: String,
    pub merchant_only: bool,
    pub currency: Option<String>,
    pub collapse_listings: CollapseListings,
    pub activate_stock_filter: bool,
    pub use_en: bool,
    pub search_stat_range: f64,
}

impl CreatePresetsOptions {
    fn with_exact(&self, exact: bool) -> CreateOptions {
        CreateOptions {
            league: self.league.clone(),
            merchant_only: self.merchant_only,
            currency: self.currency.clone(),
            collapse_listings: self.collapse_listings,
            activate_stock_filter: self.activate_stock_filter,
            exact,
            use_en: self.use_en,
        }
    }
}

fn stats(filters: Vec<StatFilter>) -> Vec<FilterOrGroup> {
    filters.into_iter().map(FilterOrGroup::Stat).collect()
}

pub fn create_presets(item: &ParsedItem, opts: &CreatePresetsOptions) -> Presets {
    let range = opts.search_stat_range;

    if item.info.ref_name == "Expedition Logbook" {
        let areas = item.logbook_area_mods.clone().unwrap_or_default();
        return Presets {
            active: ROMAN_NUMERALS[0].to_owned(),
            presets: areas
                .iter()
                .enumerate()
                .map(|(idx, area)| FilterPreset {
                    id: ROMAN_NUMERALS.get(idx).copied().unwrap_or("?").to_owned(),
                    filters: create_filters(item, &opts.with_exact(true)),
                    stats: stats(create_exact_stat_filters(
                        item,
                        &sum_stats_by_mod_type(area),
                        range,
                        StatFiltersMode::Default,
                    )),
                })
                .collect(),
        };
    } else if item.info.ref_name == "Mercenary Warrant" {
        return Presets {
            active: PRESET_EXACT.to_owned(),
            presets: vec![FilterPreset {
                id: PRESET_EXACT.to_owned(),
                filters: create_filters(item, &opts.with_exact(true)),
                stats: create_mercenary_filters(item),
            }],
        };
    }

    let not_unique = item.rarity != Some(ItemRarity::Unique);
    let cat = item.category;
    if (item.info.craftable.is_none() && not_unique)
        || item.is_unidentified
        || item.rarity == Some(ItemRarity::Normal)
        || (cat == Some(ItemCategory::Flask) && not_unique)
        || (cat == Some(ItemCategory::Tincture) && not_unique)
        || (cat == Some(ItemCategory::SanctumRelic) && not_unique)
        || (cat == Some(ItemCategory::Idol) && not_unique)
        || matches!(
            cat,
            Some(
                ItemCategory::Charm
                    | ItemCategory::Map
                    | ItemCategory::MemoryLine
                    | ItemCategory::Invitation
                    | ItemCategory::HeistContract
                    | ItemCategory::HeistBlueprint
                    | ItemCategory::Chart
                    | ItemCategory::Sentinel
            )
        )
    {
        if not_unique
            && ((cat == Some(ItemCategory::Map) && item.map_completion_reward.is_none())
                || cat == Some(ItemCategory::Chart))
        {
            let bulk_preset = FilterPreset {
                id: PRESET_BULK.to_owned(),
                filters: create_filters(item, &opts.with_exact(true)),
                stats: stats(create_exact_stat_filters(item, &item.stats_by_type, range, StatFiltersMode::Bulk)),
            };
            if item.rarity == Some(ItemRarity::Rare)
                && !item.is_unidentified
                && !item.info.area.as_ref().is_some_and(|a| a.blighted)
            {
                let props_preset = FilterPreset {
                    id: PRESET_PSEUDO.to_owned(),
                    filters: create_filters(item, &opts.with_exact(true)),
                    stats: stats(create_exact_stat_filters(item, &item.stats_by_type, range, StatFiltersMode::Props)),
                };
                let special_chart = cat == Some(ItemCategory::Chart)
                    && item.map_area.as_ref().and_then(|a| a.area.as_ref()).is_some_and(|a| a.special);
                return Presets {
                    active: if special_chart { bulk_preset.id.clone() } else { props_preset.id.clone() },
                    presets: vec![props_preset, bulk_preset],
                };
            } else {
                return Presets { active: bulk_preset.id.clone(), presets: vec![bulk_preset] };
            }
        }

        return Presets {
            active: PRESET_EXACT.to_owned(),
            presets: vec![FilterPreset {
                id: PRESET_EXACT.to_owned(),
                filters: create_filters(item, &opts.with_exact(true)),
                stats: stats(create_exact_stat_filters(item, &item.stats_by_type, range, StatFiltersMode::Default)),
            }],
        };
    }

    let pseudo_preset = FilterPreset {
        id: PRESET_PSEUDO.to_owned(),
        filters: create_filters(item, &opts.with_exact(false)),
        stats: init_ui_mod_filters(item, range),
    };

    let likely_finished_item = item.rarity == Some(ItemRarity::Unique)
        || item.stats_by_type.iter().any(|c| c.type_ == ModifierType::Crafted)
        // quality > 20 can be used for selling bases, quality < 20 drops sometimes
        || (item.quality == Some(20.0) && item.memory_strands.is_none_or(|s| s == 0))
        || item.is_corrupted.is_some()
        || item.is_mirrored;

    let has_crafting_value = item.is_synthesised
        || item.is_fractured
        || !item.influences.is_empty()
        || cat == Some(ItemCategory::ClusterJewel)
        || (cat == Some(ItemCategory::Jewel) && item.rarity == Some(ItemRarity::Magic))
        || (cat != Some(ItemCategory::Jewel)
            && cat != Some(ItemCategory::AbyssJewel)
            && item.item_level.unwrap_or(0) >= 82);

    if likely_finished_item || !has_crafting_value {
        return Presets { active: pseudo_preset.id.clone(), presets: vec![pseudo_preset] };
    }

    let base_item_preset = FilterPreset {
        id: PRESET_BASE_ITEM.to_owned(),
        filters: create_filters(item, &opts.with_exact(true)),
        stats: stats(create_exact_stat_filters(item, &item.stats_by_type, range, StatFiltersMode::Default)),
    };

    Presets { active: pseudo_preset.id.clone(), presets: vec![pseudo_preset, base_item_preset] }
}
