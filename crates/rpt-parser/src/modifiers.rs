//! Port of upstream `renderer/src/parser/modifiers.ts` (functions) and the
//! roll-scaling helpers from `advanced-mod-desc.ts` that it depends on.

use rpt_data::StatMatcher;

use crate::types::*;

/// stat values internally stored as ints, this is the most common formatter
const DIV_BY_100: i32 = 2;

/// Upstream `incrRoll`.
pub fn incr_roll(value: f64, p: f64, dp: i32) -> f64 {
    let res = value + (value * p / 100.0);
    let rounding = 10f64.powi(dp);
    ((res + f64::EPSILON) * rounding).trunc() / rounding
}

/// Upstream `applyIncr`.
pub fn apply_incr(info: &ModifierInfo, parsed: &ParsedStat) -> Option<ParsedStat> {
    let incr = info.roll_incr.filter(|v| *v != 0.0)?;
    let roll = parsed.roll.as_ref()?;
    if roll.unscalable {
        return None;
    }
    let dp = if roll.dp { DIV_BY_100 } else { 0 };
    Some(ParsedStat {
        stat: parsed.stat.clone(),
        translation: parsed.translation.clone(),
        roll: Some(ParsedRoll {
            unscalable: roll.unscalable,
            generation: None,
            dp: roll.dp,
            value: incr_roll(roll.value, incr, dp),
            min: incr_roll(roll.min, incr, dp),
            max: incr_roll(roll.max, incr, dp),
        }),
    })
}

/// Upstream `sumStatsByModType`.
pub fn sum_stats_by_mod_type(mods: &[ParsedModifier]) -> Vec<StatCalculated> {
    let mut out: Vec<StatCalculated> = Vec::new();
    for mod_a in mods {
        for stat_a in &mod_a.stats {
            if out.iter().any(|m| m.stat.ref_ == stat_a.stat.ref_ && m.type_ == mod_a.info.type_) {
                continue;
            }
            let mut sources = Vec::new();
            for mod_b in mods.iter().filter(|m| m.info.type_ == mod_a.info.type_) {
                if let Some(target) = mod_b.stats.iter().find(|s| s.stat.ref_ == stat_a.stat.ref_) {
                    let incr = apply_incr(&mod_b.info, target);
                    let roll = incr.as_ref().unwrap_or(target).roll.as_ref();
                    sources.push(StatSource {
                        modifier: mod_b.clone(),
                        stat: target.clone(),
                        contributes: roll.map(|r| StatRoll { value: r.value, min: r.min, max: r.max }),
                    });
                }
            }
            out.push(StatCalculated { stat: stat_a.stat.clone(), type_: mod_a.info.type_, sources });
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotalMode {
    Sum,
    Max,
}

/// Upstream `statSourcesTotal`.
pub fn stat_sources_total(sources: &[StatSource], mode: TotalMode) -> Option<StatRoll> {
    if sources.iter().all(|s| s.contributes.is_none()) {
        return None;
    }
    if sources.len() == 1 {
        return sources[0].contributes;
    }
    let f = |a: f64, b: f64| match mode {
        TotalMode::Sum => a + b,
        TotalMode::Max => a.max(b),
    };
    let mut sum = StatRoll { value: 0.0, min: 0.0, max: 0.0 };
    for s in sources {
        // NOTE: this also sums up stats that have "flag" semantics
        let c = s.contributes.unwrap_or(StatRoll { value: 1.0, min: 1.0, max: 1.0 });
        sum.value = f(sum.value, c.value);
        sum.min = f(sum.min, c.min);
        sum.max = f(sum.max, c.max);
    }
    Some(sum)
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranslatedStat {
    pub string: String,
    pub negate: bool,
    /// `None` when no roll was supplied.
    pub dp: Option<bool>,
}

/// Upstream `translateStatWithRoll`.
pub fn translate_stat_with_roll(calc: &StatCalculated, roll: Option<&StatRoll>) -> TranslatedStat {
    let matchers = &calc.stat.matchers;
    let no_value = |m: &&StatMatcher| m.value.is_none();
    let translation: &StatMatcher = match roll {
        None => matchers.iter().find(no_value).unwrap_or(&matchers[0]),
        Some(roll) => matchers
            .iter()
            .find(|m| m.value == Some(roll.value))
            .or_else(|| {
                let same_sign = roll.min.signum() == roll.max.signum();
                if same_sign {
                    matchers.iter().find(|m| m.value.is_none() && m.negate == (roll.value < 0.0))
                } else {
                    matchers.iter().find(|m| m.value.is_none() && !m.negate)
                }
            })
            .or_else(|| matchers.iter().find(no_value))
            .unwrap_or(&matchers[0]),
    };
    let dp = roll.map(|_| {
        calc.stat.dp
            || calc
                .sources
                .iter()
                .any(|s| s.stat.stat.ref_ == calc.stat.ref_ && s.stat.roll.as_ref().is_some_and(|r| r.dp))
    });
    TranslatedStat { string: translation.string.clone(), negate: translation.negate, dp }
}
