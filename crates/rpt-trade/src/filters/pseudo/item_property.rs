//! Port of upstream `filters/pseudo/item-property.ts`.

use std::collections::BTreeMap;

use rpt_data::{Stat, StatBetter, StatMatcher, StatTrade};
use rpt_parser::{
    ItemCategory, ModifierInfo, ModifierType, ParsedItem, ParsedModifier, ParsedRoll, ParsedStat, StatCalculated,
    StatRoll, StatSource,
};

use crate::calc_q20::{QualityStatRefs, calc_prop_bounds, prop_at_20_quality, quality_stats};
use crate::filters::create_stat_filters::{FiltersCreationContext, calculated_stat_to_filter};
use crate::filters::{FilterTag, StatFilter};

pub const BASE_PCTL_AFFECTED_IDS: &[&str] = &["item.armour", "item.evasion_rating", "item.energy_shield", "item.ward"];

pub fn filter_equipment_props(ctx: &mut FiltersCreationContext<'_>) {
    if ctx.item.category.is_some_and(|c| c.is_armour()) {
        armour_props(ctx);
    }
    if ctx.item.category.is_some_and(|c| c.is_weapon()) {
        weapon_props(ctx);
    }
}

pub fn filter_base_percentile(ctx: &mut FiltersCreationContext<'_>) {
    if let Some(pct) = ctx.item.base_percentile {
        let f = prop_to_filter(
            PropOpts {
                ref_: "Base Percentile: #%",
                trade_id: "item.base_percentile",
                roll: Some(StatRoll { min: 0.0, max: 100.0, value: pct }),
                sources: Vec::new(),
                dp: None,
                disabled: Some(pct < 50.0),
                hidden: None,
            },
            ctx.item,
            ctx.search_in_range,
        );
        ctx.filters.push(f);
    }
}

pub fn filter_memory_strands(ctx: &mut FiltersCreationContext<'_>, hidden: Option<&str>) {
    if let Some(strands) = ctx.item.memory_strands {
        let strands = strands as f64;
        let f = prop_to_filter(
            PropOpts {
                ref_: "Memory Strands: #",
                trade_id: "item.memory_strands",
                roll: Some(StatRoll { min: 0.0, max: 100.0, value: strands }),
                sources: Vec::new(),
                dp: None,
                disabled: Some(strands < 60.0 || hidden.is_some()),
                hidden: hidden.map(str::to_owned),
            },
            ctx.item,
            ctx.search_in_range,
        );
        ctx.filters.push(f);
    }
}

fn armour_stats() -> Vec<&'static str> {
    let q = [quality_stats::ARMOUR, quality_stats::EVASION, quality_stats::ENERGY_SHIELD, quality_stats::WARD];
    let mut out: Vec<&'static str> = Vec::new();
    for s in q.iter().flat_map(|s| s.flat.iter()).chain(q.iter().flat_map(|s| s.incr.iter())) {
        if !out.contains(s) {
            out.push(s);
        }
    }
    out.push("+#% Chance to Block");
    out
}

const WEAPON_STATS: &[&str] = &[
    "Adds # to # Physical Damage",
    "#% increased Physical Damage",
    "#% increased Attack Speed",
    "#% increased Critical Strike Chance",
    // "Adds # to # Chaos Damage",
    "Adds # to # Lightning Damage",
    "Adds # to # Cold Damage",
    "Adds # to # Fire Damage",
];

/// All refs used in this module (for data validation in tests).
pub fn used_stat_refs() -> Vec<&'static str> {
    let mut v = armour_stats();
    v.extend_from_slice(WEAPON_STATS);
    v
}

fn armour_props(ctx: &mut FiltersCreationContext<'_>) {
    let item = ctx.item;
    let base_info = item.unique_base.as_ref().unwrap_or(&item.info);
    let base_armour = base_info.armour.clone().unwrap_or_default();
    let single = is_single_attr_armour(item);
    let range = ctx.search_in_range;

    let mut push_q20 = |value: Option<f64>, refs: QualityStatRefs, bounds: Option<[f64; 2]>, ref_: &'static str, id: &'static str| {
        if let Some(v) = value.filter(|v| *v != 0.0) {
            let q20 = prop_at_20_quality(v, refs, bounds, item);
            ctx.filters.push(prop_to_filter(
                PropOpts {
                    ref_,
                    trade_id: id,
                    roll: Some(q20.roll),
                    sources: q20.sources,
                    dp: None,
                    disabled: Some(!single),
                    hidden: None,
                },
                item,
                range,
            ));
        }
    };

    push_q20(item.armour_ar, quality_stats::ARMOUR, base_armour.ar, "Armour: #", "item.armour");
    push_q20(item.armour_ev, quality_stats::EVASION, base_armour.ev, "Evasion Rating: #", "item.evasion_rating");
    push_q20(item.armour_es, quality_stats::ENERGY_SHIELD, base_armour.es, "Energy Shield: #", "item.energy_shield");
    push_q20(item.armour_ward, quality_stats::WARD, base_armour.ward, "Ward: #", "item.ward");

    if let Some(block) = item.armour_block.filter(|v| *v != 0.0) {
        let b = calc_prop_bounds(block, QualityStatRefs { flat: &["+#% Chance to Block"], incr: &[] }, item);
        ctx.filters.push(prop_to_filter(
            PropOpts {
                ref_: "Block: #%",
                trade_id: "item.block",
                roll: Some(b.roll),
                sources: b.sources,
                dp: None,
                disabled: Some(true),
                hidden: None,
            },
            item,
            range,
        ));
    }

    let nz = |v: Option<f64>| v.is_some_and(|v| v != 0.0);
    if nz(item.armour_ar) || nz(item.armour_ev) || nz(item.armour_es) || nz(item.armour_ward) || nz(item.armour_block) {
        let stats = armour_stats();
        remove_used_stats(ctx, &stats);
    }
}

fn weapon_props(ctx: &mut FiltersCreationContext<'_>) {
    let item = ctx.item;
    let range = ctx.search_in_range;

    let attack_speed = calc_prop_bounds(
        item.weapon_as.unwrap_or(0.0),
        QualityStatRefs { incr: &["#% increased Attack Speed"], flat: &[] },
        item,
    );
    let phys_q20 = prop_at_20_quality(item.weapon_physical.unwrap_or(0.0), quality_stats::PHYSICAL_DAMAGE, None, item);
    let pdps_q20 = StatRoll {
        value: phys_q20.roll.value * attack_speed.roll.value,
        min: phys_q20.roll.min * attack_speed.roll.min,
        max: phys_q20.roll.max * attack_speed.roll.max,
    };

    let ele_dmg = calc_prop_bounds(
        item.weapon_elemental.unwrap_or(0.0),
        QualityStatRefs {
            flat: &["Adds # to # Lightning Damage", "Adds # to # Cold Damage", "Adds # to # Fire Damage"],
            incr: &[],
        },
        item,
    );

    let edps = StatRoll {
        value: ele_dmg.roll.value * attack_speed.roll.value,
        min: ele_dmg.roll.min * attack_speed.roll.min,
        max: ele_dmg.roll.max * attack_speed.roll.max,
    };
    let dps = StatRoll {
        value: pdps_q20.value + edps.value,
        min: pdps_q20.min + edps.min,
        max: pdps_q20.max + edps.max,
    };

    let nz = |v: Option<f64>| v.is_some_and(|v| v != 0.0);

    if nz(item.weapon_elemental) {
        if nz(item.weapon_physical) {
            ctx.filters.push(prop_to_filter(
                PropOpts {
                    ref_: "Total DPS: #",
                    trade_id: "item.total_dps",
                    roll: Some(dps),
                    sources: Vec::new(),
                    dp: None,
                    disabled: Some(false),
                    hidden: None,
                },
                item,
                range,
            ));
        }

        let low = edps.value / dps.value < 0.67;
        ctx.filters.push(prop_to_filter(
            PropOpts {
                ref_: "Elemental DPS: #",
                trade_id: "item.elemental_dps",
                roll: Some(edps),
                sources: Vec::new(),
                dp: None,
                disabled: Some(low),
                hidden: low.then(|| "filters.hide_ele_dps".to_owned()),
            },
            item,
            range,
        ));
    }

    if nz(item.weapon_physical) {
        let low = pdps_q20.value / dps.value < 0.67;
        ctx.filters.push(prop_to_filter(
            PropOpts {
                ref_: "Physical DPS: #",
                trade_id: "item.physical_dps",
                roll: Some(pdps_q20),
                sources: Vec::new(),
                dp: None,
                disabled: Some(!is_pdps_important(item) || low),
                hidden: low.then(|| "filters.hide_phys_dps".to_owned()),
            },
            item,
            range,
        ));
    }

    ctx.filters.push(prop_to_filter(
        PropOpts {
            ref_: "Attacks per Second: #",
            trade_id: "item.aps",
            roll: Some(attack_speed.roll),
            sources: attack_speed.sources,
            dp: Some(true),
            disabled: Some(true),
            hidden: None,
        },
        item,
        range,
    ));

    if let Some(crit) = item.weapon_crit.filter(|v| *v != 0.0) {
        let c = calc_prop_bounds(
            crit,
            QualityStatRefs { incr: &["#% increased Critical Strike Chance"], flat: &[] },
            item,
        );
        ctx.filters.push(prop_to_filter(
            PropOpts {
                ref_: "Critical Strike Chance: #%",
                trade_id: "item.crit",
                roll: Some(c.roll),
                sources: c.sources,
                dp: Some(true),
                disabled: Some(true),
                hidden: None,
            },
            item,
            range,
        ));
    }

    if nz(item.weapon_as) || nz(item.weapon_crit) || nz(item.weapon_elemental) || nz(item.weapon_physical) {
        remove_used_stats(ctx, WEAPON_STATS);
    }
}

fn remove_used_stats(ctx: &mut FiltersCreationContext<'_>, stats: &[&str]) {
    ctx.stats_by_type.retain(|m| !stats.contains(&m.stat.ref_.as_str()));
}

fn is_single_attr_armour(item: &ParsedItem) -> bool {
    [item.armour_ar, item.armour_ev, item.armour_es, item.armour_ward].iter().filter(|v| v.is_some()).count() == 1
}

fn is_pdps_important(item: &ParsedItem) -> bool {
    matches!(
        item.category,
        Some(
            ItemCategory::OneHandedAxe
                | ItemCategory::TwoHandedAxe
                | ItemCategory::OneHandedSword
                | ItemCategory::TwoHandedSword
                | ItemCategory::Bow
                | ItemCategory::Warstaff
        )
    )
}

pub struct PropOpts {
    pub ref_: &'static str,
    pub trade_id: &'static str,
    pub roll: Option<StatRoll>,
    pub sources: Vec<StatSource>,
    pub dp: Option<bool>,
    pub disabled: Option<bool>,
    pub hidden: Option<String>,
}

/// Upstream `propToFilter`. `search_in_range` replaces upstream's `ctx` argument
/// (only `item` and `searchInRange` were read from it).
pub fn prop_to_filter(opts: PropOpts, item: &ParsedItem, search_in_range: f64) -> StatFilter {
    let mut ids = BTreeMap::new();
    ids.insert("pseudo".to_owned(), vec![opts.trade_id.to_owned()]);
    let stat = Stat {
        ref_: opts.ref_.to_owned(),
        dp: false,
        matchers: vec![StatMatcher { string: opts.ref_.to_owned(), advanced: None, negate: false, value: None }],
        better: StatBetter::PositiveRoll,
        mod_family: None,
        reminder_text: None,
        from_area_mods: None,
        anointments: None,
        mercenary: None,
        jewellery_quality: None,
        trade: StatTrade { inverted: false, option: false, ids },
    };
    let mut filter = no_source_pseudo_to_filter(
        NoSourceOpts { pseudo: stat, roll: opts.roll, dp: opts.dp, disabled: opts.disabled, hidden: opts.hidden },
        item,
        search_in_range,
    );
    filter.tag = FilterTag::Property;
    filter.sources = opts.sources;
    filter
}

pub struct NoSourceOpts {
    pub pseudo: Stat,
    pub roll: Option<StatRoll>,
    pub dp: Option<bool>,
    pub disabled: Option<bool>,
    pub hidden: Option<String>,
}

/// Upstream `noSourcePseudoToFilter`.
pub fn no_source_pseudo_to_filter(opts: NoSourceOpts, item: &ParsedItem, search_in_range: f64) -> StatFilter {
    let translation = opts.pseudo.matchers.first().cloned().unwrap_or(StatMatcher {
        string: opts.pseudo.ref_.clone(),
        advanced: None,
        negate: false,
        value: None,
    });
    let source = StatSource {
        modifier: ParsedModifier { info: ModifierInfo::new(ModifierType::Pseudo), stats: Vec::new() },
        stat: ParsedStat {
            stat: opts.pseudo.clone(),
            translation,
            roll: opts.roll.map(|r| ParsedRoll {
                unscalable: false,
                generation: None,
                dp: opts.dp.unwrap_or(false),
                value: r.value,
                min: r.min,
                max: r.max,
            }),
        },
        contributes: opts.roll,
    };
    let mut filter = calculated_stat_to_filter(
        &StatCalculated { stat: opts.pseudo, type_: ModifierType::Pseudo, sources: vec![source] },
        search_in_range,
        item,
    );

    filter.sources = Vec::new();
    if let Some(d) = opts.disabled {
        filter.disabled = d;
    }
    if let Some(h) = opts.hidden {
        filter.hidden = Some(h);
    }
    filter
}
