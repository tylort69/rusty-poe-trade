//! Port of upstream `parser/calc-q20.ts`.

use rpt_parser::modifiers::{TotalMode, stat_sources_total};
use rpt_parser::{ParsedItem, StatRoll, StatSource};

/// Stat refs that contribute flat / increased values to an item property.
#[derive(Debug, Clone, Copy)]
pub struct QualityStatRefs {
    pub flat: &'static [&'static str],
    pub incr: &'static [&'static str],
}

pub mod quality_stats {
    use super::QualityStatRefs;

    pub const ARMOUR: QualityStatRefs = QualityStatRefs {
        flat: &["+# to Armour"],
        incr: &[
            "#% increased Armour",
            "#% increased Armour and Energy Shield",
            "#% increased Armour and Evasion",
            "#% increased Armour, Evasion and Energy Shield",
        ],
    };
    pub const EVASION: QualityStatRefs = QualityStatRefs {
        flat: &["+# to Evasion Rating"],
        incr: &[
            "#% increased Evasion Rating",
            "#% increased Armour and Evasion",
            "#% increased Evasion and Energy Shield",
            "#% increased Armour, Evasion and Energy Shield",
        ],
    };
    pub const ENERGY_SHIELD: QualityStatRefs = QualityStatRefs {
        flat: &["+# to maximum Energy Shield"],
        incr: &[
            "#% increased Energy Shield",
            "#% increased Armour and Energy Shield",
            "#% increased Evasion and Energy Shield",
            "#% increased Armour, Evasion and Energy Shield",
        ],
    };
    pub const WARD: QualityStatRefs = QualityStatRefs {
        flat: &["+# to Ward"],
        incr: &["#% increased Ward"],
    };
    pub const PHYSICAL_DAMAGE: QualityStatRefs = QualityStatRefs {
        flat: &["Adds # to # Physical Damage"],
        incr: &["#% increased Physical Damage"],
    };
}

pub const QUALITY_CHANGING_ENCHANT: &[&str] = &[
    "Quality does not increase Defences",
    "Quality does not increase Physical Damage",
];

fn has_quality_changing_enchant(item: &ParsedItem) -> bool {
    item.stats_by_type
        .iter()
        .any(|calc| QUALITY_CHANGING_ENCHANT.contains(&calc.stat.ref_.as_str()))
}

pub fn get_trade_max_quality(item: &ParsedItem) -> f64 {
    if has_quality_changing_enchant(item) {
        return 0.0;
    }
    // currently Trade Site applies 20% to everything else,
    // including Mirrored items
    item.quality.unwrap_or(0.0).max(20.0)
}

pub fn get_prop_quality(item: &ParsedItem) -> f64 {
    if has_quality_changing_enchant(item) {
        return 0.0;
    }
    item.quality.unwrap_or(0.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropResult {
    pub roll: StatRoll,
    pub sources: Vec<StatSource>,
}

pub fn prop_at_20_quality(
    total: f64,
    stat_refs: QualityStatRefs,
    bounds: Option<[f64; 2]>,
    item: &ParsedItem,
) -> PropResult {
    let PropBase { incr, flat, sources } = calc_prop_base(stat_refs, item);
    let base = calc_flat(total, incr.value, get_prop_quality(item)) - flat.value;
    let [base_min, base_max] = bounds.unwrap_or([base, base]);
    let quality = get_trade_max_quality(item);
    PropResult {
        roll: if !base.is_nan() {
            StatRoll {
                value: calc_increased(base + flat.value, incr.value, quality),
                min: calc_increased(base_min + flat.min, incr.min, quality),
                max: calc_increased(base_max + flat.max, incr.max, quality),
            }
        } else {
            StatRoll { value: 0.0, min: 0.0, max: 0.0 }
        },
        sources: sources.into_iter().map(|s| StatSource { contributes: None, ..s }).collect(),
    }
}

pub fn calc_prop_bounds(total: f64, stat_refs: QualityStatRefs, item: &ParsedItem) -> PropResult {
    let PropBase { incr, flat, sources } = calc_prop_base(stat_refs, item);
    let base = calc_flat(total, incr.value, 0.0) - flat.value;
    let roll = StatRoll {
        value: calc_increased(base + flat.value, incr.value, 0.0),
        min: calc_increased(base + flat.min, incr.min, 0.0),
        max: calc_increased(base + flat.max, incr.max, 0.0),
    };
    let sources = if !stat_refs.incr.is_empty() && !stat_refs.flat.is_empty() {
        sources.into_iter().map(|s| StatSource { contributes: None, ..s }).collect()
    } else if !stat_refs.incr.is_empty() {
        sources
            .into_iter()
            .map(|s| {
                let c = s.contributes.unwrap_or(StatRoll { value: 0.0, min: 0.0, max: 0.0 });
                StatSource {
                    contributes: Some(StatRoll {
                        value: base * c.value / 100.0,
                        min: base * c.min / 100.0,
                        max: base * c.max / 100.0,
                    }),
                    ..s
                }
            })
            .collect()
    } else {
        sources
    };
    PropResult { roll, sources }
}

pub fn calc_prop_percentile(
    total: f64,
    bounds: [f64; 2],
    stat_refs: QualityStatRefs,
    item: &ParsedItem,
) -> f64 {
    let PropBase { incr, flat, .. } = calc_prop_base(stat_refs, item);
    let roll = calc_flat(total, incr.value, get_prop_quality(item)) - flat.value;
    let [min, max] = bounds;
    let result = crate::js::math_round(((roll - min) / (max - min)) * 100.0);
    result.clamp(0.0, 100.0)
}

struct PropBase {
    incr: StatRoll,
    flat: StatRoll,
    sources: Vec<StatSource>,
}

fn calc_prop_base(stat_refs: QualityStatRefs, item: &ParsedItem) -> PropBase {
    let mut incr = StatRoll { value: 0.0, min: 0.0, max: 0.0 };
    let mut flat = StatRoll { value: 0.0, min: 0.0, max: 0.0 };
    let mut sources = Vec::new();

    for calc in &item.stats_by_type {
        let r = calc.stat.ref_.as_str();
        let total = if stat_refs.flat.contains(&r) {
            &mut flat
        } else if stat_refs.incr.contains(&r) {
            &mut incr
        } else {
            continue;
        };
        if let Some(roll) = stat_sources_total(&calc.sources, TotalMode::Sum) {
            total.value += roll.value;
            total.min += roll.min;
            total.max += roll.max;
        }
        sources.extend(calc.sources.iter().cloned());
    }

    PropBase { incr, flat, sources }
}

fn calc_flat(total: f64, incr_pct: f64, more_pct: f64) -> f64 {
    // We cannot know original roll if the `morePct` or `incrPct` is equal to -100,
    // in that case a NaN will be returned.
    total / (1.0 + more_pct / 100.0) / (1.0 + incr_pct / 100.0)
}

fn calc_increased(flat: f64, incr_pct: f64, more_pct: f64) -> f64 {
    flat * (1.0 + incr_pct / 100.0) * (1.0 + more_pct / 100.0)
}
