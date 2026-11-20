//! Port of upstream `filters/create-item-filters.ts`.

use rpt_data::{BaseType, Namespace};
use rpt_parser::{ItemCategory, ItemRarity, ModifierType, ParsedItem};

use super::*;
use crate::calc_q20::get_prop_quality;
use crate::leagues::PERMANENT_SC;
use crate::trade::{category_to_trade_id, trade_tag};

pub const SPECIAL_SUPPORT_GEM: &[&str] = &["Empower Support", "Enlighten Support", "Enhance Support"];

#[derive(Debug, Clone, PartialEq)]
pub struct CreateOptions {
    pub league: String,
    pub merchant_only: bool,
    pub currency: Option<String>,
    pub collapse_listings: CollapseListings,
    pub activate_stock_filter: bool,
    pub exact: bool,
    pub use_en: bool,
}

fn is_jewellery(c: Option<ItemCategory>) -> bool {
    c.is_some_and(|c| c.is_jewellery())
}
fn is_consumable_craftable(c: Option<ItemCategory>) -> bool {
    c.is_some_and(|c| c.is_consumable_craftable())
}
fn is_magic_only_or_unique(c: Option<ItemCategory>) -> bool {
    c.is_some_and(|c| c.is_magic_only_or_unique())
}

fn t(opts: &CreateOptions, info: &BaseType) -> String {
    if opts.use_en { info.ref_name.clone() } else { info.name.clone() }
}

fn base_search(opts: &CreateOptions, info: &BaseType) -> SearchFilter {
    SearchFilter {
        base_type: Some(info.name.clone()),
        base_type_trade: Some(t(opts, info)),
        ..Default::default()
    }
}

/// `sub` built from `item.mapArea` (Scrying Orb, Blighted maps, Charts).
fn map_area_sub(item: &ParsedItem) -> Option<Box<SearchFilter>> {
    let area = item.map_area.as_ref()?;
    Some(Box::new(SearchFilter {
        base_type: Some(area.name.clone()),
        base_type_trade: area.trade_disc.clone(),
        discriminator_trade: item.info.trade_disc.clone(),
        disabled: false,
        ..Default::default()
    }))
}

pub fn create_filters(item: &ParsedItem, opts: &CreateOptions) -> ItemFilters {
    let mut filters = ItemFilters::new(TradeOptions {
        offline: false,
        online_in_league: false,
        merchant_only: opts.merchant_only &&
            // these are Divination Cards, and some items at start of league
            // that are on Currency Exchange but was not added to Bulk section of site yet
            !(item.info.exchangeable && item.info.trade_tag.is_none()),
        in_person_only: false,
        listed: None,
        currency: opts.currency.clone(),
        league: opts.league.clone(),
        collapse_listings: opts.collapse_listings,
        collapse_merchant: false,
    });

    if (item.info.craftable.is_none() || is_consumable_craftable(item.category))
        && item.rarity != Some(ItemRarity::Unique)
    {
        if opts.currency.is_none() {
            filters.trade.currency = Some("chaos_divine".to_owned());
        }
        if item.info.ref_name != "Mercenary Warrant" && item.info.ref_name != "Viscera" {
            filters.trade.collapse_merchant = true;
        }
    }

    if item.category == Some(ItemCategory::Gem) {
        return create_gem_filters(item, filters, opts);
    }
    if item.category == Some(ItemCategory::CapturedBeast) {
        filters.search_exact = base_search(opts, &item.info);
        return filters;
    }
    if item.stack_size.is_some() || trade_tag(item).is_some() || item.info.exchangeable {
        let value = item.stack_size.map(|s| s.value).filter(|v| *v != 0.0).unwrap_or(1.0);
        filters.stack_size = Some(FilterNumeric::new(
            value,
            !(item.stack_size.is_some_and(|s| s.value > 1.0) && opts.activate_stock_filter),
        ));
    }
    if item.category == Some(ItemCategory::Invitation) {
        filters.search_exact = base_search(opts, &item.info);
        return filters;
    }
    if item.category == Some(ItemCategory::MetamorphSample) {
        filters.search_exact = base_search(opts, &item.info);
        filters.item_level = Some(FilterNumeric::new(item.item_level.unwrap_or(0) as f64, false));
        return filters;
    }
    if item.info.ref_name == "Scrying Orb" {
        filters.search_exact = SearchFilter { sub: map_area_sub(item), ..base_search(opts, &item.info) };
        return filters;
    }
    if item.category == Some(ItemCategory::DivinationCard)
        || item.category == Some(ItemCategory::Currency)
        || item.info.ref_name == "Charged Compass"
    {
        filters.search_exact = base_search(opts, &item.info);
        let area_level = item.area_level.unwrap_or(0) as f64;
        if item.info.ref_name == "Chronicle of Atzoatl" {
            filters.area_level = Some(FilterNumeric::new(
                floor_to_bracket(area_level, &[1.0, 68.0, 73.0, 75.0, 78.0, 80.0]),
                false,
            ));
        } else if item.info.ref_name == "Mirrored Tablet" {
            filters.area_level = Some(FilterNumeric::new(area_level, false));
        } else if item.info.ref_name == "Forbidden Tome" {
            let mut f = FilterNumeric::new(area_level, false);
            if area_level < 83.0 {
                f.max = Some(area_level);
            }
            filters.area_level = Some(f);
        } else if let Some(ilvl) = item.item_level.filter(|v| *v != 0) {
            // Incubators, Wombgifts
            filters.item_level = Some(FilterNumeric::new(ilvl as f64, false));
        }
        return filters;
    }
    if item.info.ref_name == "Viscera" {
        filters.search_exact = SearchFilter { category: item.category, ..Default::default() };
        return filters;
    }

    if item.category == Some(ItemCategory::Map) {
        if item.info.area.as_ref().is_some_and(|a| a.blighted) {
            let data = rpt_data::data();
            let map_base = data
                .item_by_ref(Namespace::Item, "Map")
                .and_then(|v| v.first().map(|b| t(opts, b)));
            filters.search_exact = SearchFilter {
                base_type: Some(item.info.name.clone()),
                base_type_trade: map_base,
                discriminator_trade: Some("map".to_owned()),
                sub: map_area_sub(item),
                ..Default::default()
            };
        } else if let (Some(ItemRarity::Unique), Some(unique_base)) = (item.rarity, &item.unique_base) {
            filters.search_exact = SearchFilter {
                name: Some(item.info.name.clone()),
                name_trade: Some(t(opts, &item.info)),
                base_type_trade: Some(t(opts, unique_base)),
                ..Default::default()
            };
        } else {
            filters.search_exact = base_search(opts, &item.info);
        }

        if item.info.ref_name == "Map" || item.unique_base.as_ref().is_some_and(|b| b.ref_name == "Map") {
            filters.search_exact.discriminator_trade = Some("map".to_owned());
        }

        filters.map_blighted = match item.info.ref_name.as_str() {
            "Blighted Map" => Some(MapBlightedFilter { value: MapBlighted::Blighted }),
            "Blight-ravaged Map" => Some(MapBlightedFilter { value: MapBlighted::BlightRavaged }),
            "Map" => Some(MapBlightedFilter { value: MapBlighted::No }),
            _ => None,
        };

        if let Some(reward) = &item.map_completion_reward {
            filters.map_completion_reward =
                Some(MapCompletionRewardFilter { name: reward.name.clone(), name_trade: t(opts, reward) });
        }

        if let Some(tier) = item.map_tier.filter(|v| *v != 0) {
            filters.map_tier = Some(FilterNumeric::new(tier as f64, false));
        }
    } else if item.info.ref_name == "Expedition Logbook" {
        filters.search_exact = base_search(opts, &item.info);
        filters.area_level = Some(FilterNumeric::new(
            floor_to_bracket(item.area_level.unwrap_or(0) as f64, &[1.0, 68.0, 73.0, 78.0, 81.0, 83.0]),
            false,
        ));
    } else if item.category == Some(ItemCategory::Chart) {
        filters.search_exact = base_search(opts, &item.info);
        filters.search_relaxed = Some(SearchFilter {
            category: item.category,
            disabled: false,
            sub: map_area_sub(item),
            ..Default::default()
        });
    } else if let (Some(ItemRarity::Unique), Some(unique_base)) = (item.rarity, &item.unique_base) {
        filters.search_exact = SearchFilter {
            name: Some(item.info.name.clone()),
            name_trade: Some(t(opts, &item.info)),
            base_type_trade: Some(t(opts, unique_base)),
            ..Default::default()
        };
    } else {
        filters.search_exact = base_search(opts, &item.info);
        if let Some(category) = item.category.filter(|c| category_to_trade_id(*c).is_some()) {
            let disabled = match category {
                ItemCategory::ClusterJewel
                | ItemCategory::Idol
                | ItemCategory::Graft
                | ItemCategory::HeistBlueprint => true,
                ItemCategory::SanctumRelic | ItemCategory::Charm | ItemCategory::HeistContract => false,
                _ => opts.exact,
            };
            filters.search_relaxed = Some(SearchFilter { category: Some(category), disabled, ..Default::default() });
        }
    }

    if let Some(charge) = item.sentinel_charge {
        filters.sentinel_charge = Some(FilterNumeric::new(charge as f64, false));
    }

    let quality = item.quality.filter(|q| *q != 0.0);

    if is_jewellery(item.category) && quality.is_none() {
        filters.not_catalysed = Some(Disabled { disabled: false });
    }

    if let Some(q) = quality.filter(|q| *q >= 20.0) {
        if !is_jewellery(item.category)
            && (item.category == Some(ItemCategory::Flask)
                || item.category == Some(ItemCategory::Tincture)
                || get_prop_quality(item) == 0.0
                || opts.exact)
        {
            // for Weapons & Armour
            filters.quality = Some(FilterNumeric::new(q, q <= 20.0));
        }
    }

    if let Some(linked) = item.sockets.and_then(|s| s.linked).filter(|v| *v != 0) {
        filters.linked_sockets = Some(FilterNumeric::new(linked as f64, false));
    }

    let for_adorned_jewel = item.rarity == Some(ItemRarity::Magic)
        // item.isCorrupted && -- let the buyer corrupt
        && matches!(item.category, Some(ItemCategory::Jewel | ItemCategory::AbyssJewel));

    if !item.is_unmodifiable && (item.info.craftable.is_some() || item.rarity == Some(ItemRarity::Unique)) {
        filters.corrupted = Some(CorruptedFilter {
            value: item.is_corrupted.is_some(),
            twice: item.is_corrupted.is_some_and(|c| c.twice),
            exact: Some(for_adorned_jewel),
        });
    }

    if for_adorned_jewel {
        filters.rarity = Some(ValueDisabled { value: "magic".to_owned(), disabled: false });
    } else if opts.exact
        && item.rarity == Some(ItemRarity::Magic)
        && !is_consumable_craftable(item.category)
        && !is_magic_only_or_unique(item.category)
    {
        filters.rarity = Some(ValueDisabled { value: "magic".to_owned(), disabled: true });
    } else if item.info.craftable.is_some()
        && matches!(item.rarity, Some(ItemRarity::Normal | ItemRarity::Magic | ItemRarity::Rare))
    {
        filters.rarity = Some(ValueDisabled { value: "nonunique".to_owned(), disabled: false });
    }

    if item.is_mirrored {
        filters.mirrored = Some(DisabledHidden { disabled: false, hidden: false });
    } else if item.info.craftable.is_some() && item.is_corrupted.is_none() {
        filters.mirrored = Some(DisabledHidden { disabled: true, hidden: true });
    }

    if item.is_split {
        filters.split = Some(DisabledHidden { disabled: false, hidden: false });
    } else if (!PERMANENT_SC.contains(&opts.league.as_str()) || opts.exact)
        && item.info.craftable.is_some()
        && item.is_corrupted.is_none()
        && !item.is_mirrored
        && !item.is_synthesised
        && !item.is_fractured
        && item.influences.is_empty()
    {
        filters.split = Some(DisabledHidden { disabled: true, hidden: true });
    }

    if !item.is_fractured && item.info.craftable.is_some() && item.is_corrupted.is_none() && !item.is_mirrored {
        filters.fractured = Some(BoolValue { value: false });
    }

    if item.is_foil {
        filters.foil = Some(Disabled { disabled: false });
    }

    if !item.influences.is_empty() && item.influences.len() <= 2 {
        filters.influences = Some(
            item.influences
                .iter()
                .map(|influence| InfluenceFilter { value: *influence, disabled: !opts.exact })
                .collect(),
        );
    }

    if let Some(ilvl) = item.item_level.filter(|v| *v != 0) {
        let ilvl = ilvl as f64;
        if item.rarity != Some(ItemRarity::Unique)
            && !matches!(
                item.category,
                Some(
                    ItemCategory::Map
                        | ItemCategory::Jewel // https://pathofexile.gamepedia.com/Jewel#Affixes
                        | ItemCategory::HeistBlueprint
                        | ItemCategory::HeistContract
                        | ItemCategory::Chart
                        | ItemCategory::MemoryLine
                        | ItemCategory::SanctumRelic
                        | ItemCategory::Charm
                        | ItemCategory::Idol
                )
            )
            && item.info.ref_name != "Expedition Logbook"
        {
            if item.category == Some(ItemCategory::ClusterJewel) {
                filters.item_level = Some(FilterNumeric {
                    value: floor_to_bracket(ilvl, &[1.0, 50.0, 68.0, 75.0, 84.0]),
                    max: Some(ceil_to_bracket(ilvl, &[100.0, 74.0, 67.0, 49.0])),
                    disabled: !opts.exact,
                });
            } else {
                // TODO limit level by item type
                filters.item_level = Some(FilterNumeric::new(
                    ilvl.min(86.0),
                    !opts.exact
                        || item.category == Some(ItemCategory::Flask)
                        || item.category == Some(ItemCategory::Tincture),
                ));
            }
        }

        if item.rarity == Some(ItemRarity::Unique) {
            if item.is_unidentified && item.info.ref_name == "Watcher's Eye" {
                filters.item_level = Some(FilterNumeric::new(ilvl, false));
            }

            if ilvl >= 75.0
                && ["Agnerod", "Agnerod East", "Agnerod North", "Agnerod South", "Agnerod West"]
                    .contains(&item.info.ref_name.as_str())
            {
                // https://pathofexile.gamepedia.com/The_Vinktar_Square
                let normalized = if ilvl >= 82.0 {
                    82.0
                } else if ilvl >= 80.0 {
                    80.0
                } else if ilvl >= 78.0 {
                    78.0
                } else {
                    75.0
                };
                filters.item_level = Some(FilterNumeric::new(normalized, false));
            }
        }
    }

    if item.is_unidentified {
        filters.unidentified = Some(ValueDisabled { value: true, disabled: item.rarity != Some(ItemRarity::Unique) });
    }

    if item.is_veiled {
        filters.veiled = Some(VeiledFilter {
            stat_refs: item
                .stats_by_type
                .iter()
                .filter(|calc| calc.type_ == ModifierType::Veiled)
                .map(|calc| calc.stat.ref_.clone())
                .collect(),
            disabled: false,
        });

        if item.rarity != Some(ItemRarity::Unique) {
            if let Some(il) = &mut filters.item_level {
                il.disabled = false;
            }
        }
    }

    if item.rarity == Some(ItemRarity::Unique) {
        filters.foulborn = Some(BoolValue { value: item.is_foulborn });
        filters.vestigial = Some(BoolValue { value: item.is_vestigial });
    }

    if matches!(
        item.category,
        Some(ItemCategory::HeistContract | ItemCategory::HeistBlueprint | ItemCategory::Chart)
    ) && item.rarity != Some(ItemRarity::Unique)
    {
        filters.area_level = Some(FilterNumeric::new(item.area_level.unwrap_or(0) as f64, false));
    }

    filters
}

fn create_gem_filters(item: &ParsedItem, mut filters: ItemFilters, opts: &CreateOptions) -> ItemFilters {
    let gem = item.info.gem.as_ref();
    let transfigured = gem.is_some_and(|g| g.transfigured);
    let max_level = gem.map(|g| g.max_level).unwrap_or(20);

    if !transfigured {
        filters.search_exact = base_search(opts, &item.info);
    } else {
        let data = rpt_data::data();
        let normal = gem
            .and_then(|g| g.normal_variant.as_deref())
            .and_then(|name| data.item_by_ref(Namespace::Gem, name))
            .and_then(|v| v.first().map(|b| t(opts, b)));
        filters.search_exact = SearchFilter {
            base_type: Some(item.info.name.clone()),
            base_type_trade: normal,
            discriminator_trade: item.info.trade_disc.clone(),
            ..Default::default()
        };
    }

    if let Some(vaal) = &item.vaal_gem {
        filters.search_exact.sub = Some(Box::new(SearchFilter {
            base_type: Some(vaal.name.clone()),
            base_type_trade: Some(t(opts, vaal)),
            discriminator_trade: item.info.trade_disc.clone(),
            disabled: false,
            ..Default::default()
        }));
    }

    filters.corrupted = Some(CorruptedFilter {
        value: item.is_corrupted.is_some(),
        twice: item.is_corrupted.is_some_and(|c| c.twice),
        exact: None,
    });

    let gem_level = item.gem_level.unwrap_or(0);
    if !item.imbued_gem && item.is_corrupted.is_some() && gem_level >= 20 {
        filters.imbued_gem = Some(Disabled { disabled: true });
    }

    filters.gem_level = Some(FilterNumeric::new(gem_level as f64, gem_level < max_level));

    if let Some(q) = item.quality.filter(|q| *q != 0.0) {
        let disabled = if max_level == 1 {
            false
        } else if max_level == 20 && !transfigured {
            q < 16.0
        } else {
            q < 20.0
        };
        filters.quality = Some(FilterNumeric::new(q, disabled));
    }

    filters
}

pub fn floor_to_bracket(value: f64, brackets: &[f64]) -> f64 {
    let mut prev = brackets[0];
    for &num in brackets {
        if num > value {
            return prev;
        }
        prev = num;
    }
    prev
}

fn ceil_to_bracket(value: f64, brackets: &[f64]) -> f64 {
    let mut prev = brackets[0];
    for &num in brackets {
        if num < value {
            return prev;
        }
        prev = num;
    }
    prev
}
