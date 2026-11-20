//! Port of upstream `filters/pseudo/maps.ts`.

use rpt_parser::{ItemCategory, ItemRarity, ModifierType, StatRoll};

use super::item_property::{NoSourceOpts, PropOpts, no_source_pseudo_to_filter, prop_to_filter};
use super::utils::{find_and_resolve_by_ref, stat_to_not_filter};
use crate::filters::create_stat_filters::FiltersCreationContext;

const MORE_THREADS: &str = "More Scarabs: #%";
const MORE_MAPS: &str = "More Maps: #%";
const MORE_DIVINATION_CARDS: &str = "More Divination Cards: #%";
const MORE_CURRENCY: &str = "More Currency: #%";
const EXPLICIT_MODIFIERS: &str = "# Modifiers";

const VALDO_LETHAL_STATS: &[&str] = &["Players who Die in area are sent to the Void"];

pub fn used_stat_refs() -> Vec<&'static str> {
    let mut v = vec![MORE_THREADS, MORE_MAPS, MORE_DIVINATION_CARDS, MORE_CURRENCY, EXPLICIT_MODIFIERS];
    v.extend_from_slice(VALDO_LETHAL_STATS);
    v
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn unbounded(value: f64) -> Option<StatRoll> {
    Some(StatRoll { min: 0.0, max: MAX_SAFE_INTEGER, value })
}

#[derive(Default)]
struct AreaDisabled {
    item_quantity: bool,
    item_rarity: bool,
    pack_size: bool,
}

fn area_props(disabled: AreaDisabled, ctx: &mut FiltersCreationContext<'_>) {
    let item = ctx.item;
    let range = ctx.search_in_range;
    let mut push = |value: Option<f64>, ref_: &'static str, id: &'static str, disabled: bool| {
        if let Some(v) = value.filter(|v| *v != 0.0) {
            ctx.filters.push(prop_to_filter(
                PropOpts {
                    ref_,
                    trade_id: id,
                    roll: unbounded(v),
                    sources: Vec::new(),
                    dp: None,
                    disabled: Some(disabled),
                    hidden: None,
                },
                item,
                range,
            ));
        }
    };
    push(item.area_item_quantity, "Item Quantity: +#%", "item.map_item_quantity", disabled.item_quantity);
    // reuse quant
    push(item.area_atlas_drops, "Atlas Drops: +#%", "item.map_atlas_drops", disabled.item_quantity);
    push(item.area_item_rarity, "Item Rarity: +#%", "item.map_item_rarity", disabled.item_rarity);
    push(item.area_pack_size, "Monster Pack Size: +#%", "item.map_pack_size", disabled.pack_size);
}

fn push_pseudo(ctx: &mut FiltersCreationContext<'_>, ref_: &str, roll: Option<StatRoll>, disabled: bool, range: f64) {
    let data = rpt_data::data();
    let Some(pseudo) = data.pseudo_stat_by_ref(ref_) else {
        tracing::warn!("pseudo stat not found: {ref_}");
        return;
    };
    let f = no_source_pseudo_to_filter(
        NoSourceOpts { pseudo: pseudo.clone(), roll, dp: None, disabled: Some(disabled), hidden: None },
        ctx.item,
        range,
    );
    ctx.filters.push(f);
}

pub fn map_props(bulk: bool, ctx: &mut FiltersCreationContext<'_>) {
    let item = ctx.item;
    if item.category != Some(ItemCategory::Map)
        || item.map_completion_reward.is_some()
        || item.rarity == Some(ItemRarity::Unique)
    {
        return;
    }
    let nz = |v: Option<f64>| v.filter(|v| *v != 0.0);
    let range = ctx.search_in_range;

    if !bulk {
        let has_valuable_drops =
            nz(item.map_more_threads).is_some() || nz(item.map_more_currency).is_some() || nz(item.map_more_div_cards).is_some();

        area_props(
            AreaDisabled { item_quantity: false, item_rarity: has_valuable_drops, pack_size: false },
            ctx,
        );

        if let Some(v) = nz(item.map_more_maps) {
            push_pseudo(ctx, MORE_MAPS, unbounded(v), true, range);
        }
        if let Some(v) = nz(item.map_more_threads) {
            push_pseudo(ctx, MORE_THREADS, unbounded(v), false, range);
        }
        if let Some(v) = nz(item.map_more_currency) {
            push_pseudo(ctx, MORE_CURRENCY, unbounded(v), false, range);
        }
        if let Some(v) = nz(item.map_more_div_cards) {
            push_pseudo(ctx, MORE_DIVINATION_CARDS, unbounded(v), false, range);
        }
    }

    let explicit_mods = item
        .new_mods
        .iter()
        .filter(|m| m.info.type_ == ModifierType::Explicit || m.info.type_ == ModifierType::Fractured)
        .count();
    if explicit_mods >= 8 {
        push_pseudo(
            ctx,
            EXPLICIT_MODIFIERS,
            Some(StatRoll { min: 0.0, max: 9.0, value: explicit_mods as f64 }),
            false,
            0.0,
        );
    }
}

pub fn chart_props(bulk: bool, ctx: &mut FiltersCreationContext<'_>) {
    let item = ctx.item;
    if bulk || item.category != Some(ItemCategory::Chart) {
        return;
    }

    area_props(AreaDisabled::default(), ctx);

    if let Some(v) = item.chart_sulphur.filter(|v| *v != 0.0) {
        let f = prop_to_filter(
            PropOpts {
                ref_: "Dead Man's Sulphur: +#%",
                trade_id: "item.chart_sulphur",
                roll: unbounded(v),
                sources: Vec::new(),
                dp: None,
                disabled: Some(false),
                hidden: None,
            },
            item,
            ctx.search_in_range,
        );
        ctx.filters.push(f);
    }
}

pub fn valdo_bad_mods(ctx: &mut FiltersCreationContext<'_>) {
    if ctx.item.map_completion_reward.is_none() {
        return;
    }

    for lethal_ref in VALDO_LETHAL_STATS {
        if ctx.item.stats_by_type.iter().any(|calc| calc.stat.ref_ == *lethal_ref) {
            continue;
        }
        let Some(lethal) = find_and_resolve_by_ref(lethal_ref, ModifierType::Explicit, ctx.item.category, None)
        else {
            continue;
        };
        ctx.filters.push(stat_to_not_filter(&lethal, ModifierType::Explicit, false, false));
    }
}
