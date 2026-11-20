//! Port of upstream `renderer/src/parser/stat-translations.ts`.
//!
//! This file is a little messy and scary,
//! but that's how stats translations are parsed :-D

use std::sync::LazyLock;

use fancy_regex::Regex as FancyRegex;
use regex::Regex;
use rpt_data::{GameData, Stat, StatGroup, StatGroupResolver, StatMatcher, StatOrGroup};

use crate::js;
use crate::types::*;

static LOCALIZED_PAREN_LEFT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:\(|（)").unwrap());
static LOCALIZED_PAREN_RIGHT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:\)|）)\s*$").unwrap());

/// Upstream `StatString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatString {
    pub string: String,
    pub unscalable: bool,
}

/// Upstream `linesToStatStrings`.
///
/// Upstream is a generator that yields candidate stat strings and receives
/// `true` back via `next(isParsed)` when the candidate was recognized. Here the
/// consumer is the `try_parse` callback: it receives each candidate and returns
/// whether it was parsed. The return value is the generator's return value
/// (the lines that were not parsed, without empty ones).
pub fn lines_to_stat_strings(lines: &[String], try_parse: impl FnMut(StatString) -> bool) -> Vec<String> {
    let unscalable_value = rpt_data::data().client_strings.UNSCALABLE_VALUE.clone();
    lines_to_stat_strings_with(lines, &unscalable_value, try_parse)
}

pub(crate) fn lines_to_stat_strings_with(
    lines: &[String],
    unscalable_value: &str,
    mut try_parse: impl FnMut(StatString) -> bool,
) -> Vec<String> {
    let mut not_parsed_lines: Vec<String> = Vec::new();
    let mut reminder_string = false;

    let mut start = 0;
    'outer: while start < lines.len() {
        let cur = start;
        start += 1;

        if LOCALIZED_PAREN_LEFT.is_match(&lines[cur]) {
            reminder_string = true;
        }
        if reminder_string && LOCALIZED_PAREN_RIGHT.is_match(&lines[cur]) {
            reminder_string = false;
            continue;
        }
        if reminder_string {
            continue;
        }

        for end in cur..lines.len() {
            let mut s = lines[cur..=end].join("\n");
            let unscalable = s.ends_with(unscalable_value);
            if unscalable {
                s.truncate(s.len() - unscalable_value.len());
            }
            if try_parse(StatString { string: s, unscalable }) {
                // `start += (end - start)` then the loop's `start += 1`
                start = end + 1;
                continue 'outer;
            }
        }
        not_parsed_lines.push(lines[cur].clone());
    }
    not_parsed_lines.into_iter().filter(|l| !l.is_empty()).collect()
}

const PLACEHOLDER_MAP: &[&[&[usize]]] = &[
    // 0 # -> max 0 #
    &[&[]],
    // 1 # -> max 1 #
    &[&[0], &[]],
    // 2 # -> max 2 #
    &[&[0, 1], &[0], &[1], &[]],
    // 3 # -> max 2 #
    &[&[0, 1, 2], &[1, 2], &[0, 2], &[0, 1], &[2], &[1], &[0]],
    // 4 # -> max 2 #
    &[
        &[0, 1, 2, 3],
        &[1, 2, 3],
        &[0, 2, 3],
        &[0, 1, 3],
        &[0, 1, 2],
        &[2, 3],
        &[1, 3],
        &[1, 2],
        &[0, 3],
        &[0, 2],
        &[0, 1],
    ],
];

static VALUE_RE: LazyLock<FancyRegex> = LazyLock::new(|| {
    FancyRegex::new(r"(?<value>(?<![0-9]|\))[+-]?[0-9]+(?:\.[0-9]+)?)(?:\((?<min>.[^)-]*)(?:-(?<max>[^)]+))?\))?")
        .unwrap()
});

#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds {
    min: f64,
    max: f64,
}

#[derive(Debug, Clone)]
struct CapturedValue {
    roll: f64,
    roll_str: String,
    decimal: bool,
    bounds: Option<Bounds>,
}

struct Combination {
    stat: String,
    values: Vec<CapturedValue>,
}

/// Upstream `_statPlaceholderGenerator`, eagerly collected (all candidates are
/// cheap strings; consumers stop at the first hit just like upstream).
fn stat_placeholder_combinations(stat: &str) -> Vec<Combination> {
    let mut matches: Vec<CapturedValue> = Vec::new();
    // "()" when GGG didn't provide advanced desc, like in "Passives in Radius of Wicked Ward() can be Allocated"
    // "(...)" remove stat value ranges which are described textually
    let cleaned = stat.replace("()", "").replace("(...)", "");

    let mut with_placeholders = String::with_capacity(cleaned.len());
    let mut last = 0;
    for caps in VALUE_RE.captures_iter(&cleaned) {
        let Ok(caps) = caps else { break };
        let whole = caps.get(0).unwrap();
        with_placeholders.push_str(&cleaned[last..whole.start()]);
        last = whole.end();

        let roll = caps.name("value").unwrap().as_str();
        let min = caps.name("min").map(|m| m.as_str());
        let mut max = caps.name("max").map(|m| m.as_str());
        if min.is_some() && max.is_none() {
            // example: Sextant "# uses remaining", legacy rolls
            max = min;
        }
        let decimal = roll.contains('.')
            || min.is_some_and(|m| m.contains('.'))
            || max.is_some_and(|m| m.contains('.'));
        // Number(undefined) is NaN
        let bmin = min.map(js::number).unwrap_or(f64::NAN);
        let bmax = max.map(js::number).unwrap_or(f64::NAN);
        let mut captured = CapturedValue {
            roll: js::number(roll),
            roll_str: roll.to_owned(),
            decimal,
            bounds: Some(Bounds { min: bmin, max: bmax }),
        };
        if bmin.is_nan() || bmax.is_nan() {
            captured.bounds = None;
            match min {
                Some(min) => {
                    with_placeholders.push_str(&format!("#({}-{})", min, max.unwrap_or("undefined")));
                }
                None => with_placeholders.push('#'),
            }
        } else {
            with_placeholders.push('#');
        }
        matches.push(captured);
    }
    with_placeholders.push_str(&cleaned[last..]);

    let mut out = Vec::new();
    if matches.len() < PLACEHOLDER_MAP.len() {
        for replacements in PLACEHOLDER_MAP[matches.len()] {
            let mut idx: usize = 0;
            let mut replaced = String::with_capacity(with_placeholders.len());
            for ch in with_placeholders.chars() {
                if ch == '#' {
                    if replacements.contains(&idx) {
                        replaced.push_str(&matches[idx].roll_str);
                    } else {
                        replaced.push('#');
                    }
                    idx += 1;
                } else {
                    replaced.push(ch);
                }
            }
            let values = matches
                .iter()
                .enumerate()
                .filter(|(i, _)| !replacements.contains(i))
                .map(|(_, v)| v.clone())
                .collect();
            out.push(Combination { stat: replaced, values });
        }
    }

    // fallback to exact stat text, without any placeholders
    // N # -> max 0 #
    out.push(Combination { stat: stat.to_owned(), values: Vec::new() });
    out
}

/// Upstream `tryParseTranslation`.
pub fn try_parse_translation(
    stat: &StatString,
    mod_type: ModifierType,
    item_category: Option<ItemCategory>,
) -> Option<ParsedStat> {
    let data = rpt_data::data();
    try_parse_translation_with(&data, stat, mod_type, item_category)
}

pub(crate) fn try_parse_translation_with(
    data: &GameData,
    stat: &StatString,
    mod_type: ModifierType,
    item_category: Option<ItemCategory>,
) -> Option<ParsedStat> {
    for mut combination in stat_placeholder_combinations(&stat.string) {
        let found = find_and_resolve_translation(
            data,
            &FindResolveParams {
                match_str: combination.stat.clone(),
                mod_type,
                item_category,
                roll: if combination.values.len() == 1 { Some(combination.values[0].roll) } else { None },
            },
        );
        let Some((found_stat, matcher)) = found else { continue };
        if !found_stat.trade.ids.contains_key(mod_type.as_str()) {
            continue;
        }

        // Modifiers must be upgraded to the new values with a Divine Orb
        let mut legacy_stat_rolls = false;

        if matcher.negate {
            for v in &mut combination.values {
                v.roll *= -1.0;
                if let Some(b) = &mut v.bounds {
                    b.min *= -1.0;
                    b.max *= -1.0;
                }
            }
        }

        if found_stat.ref_ == "# uses remaining"
            && let Some(uses) = combination.values.first_mut() {
                uses.bounds = Some(Bounds { min: 1.0, max: uses.bounds.map(|b| b.max).unwrap_or(uses.roll) });
            }

        for v in &mut combination.values {
            let Some(b) = &mut v.bounds else { continue };

            if b.min > b.max {
                // some stats granted by legacy Modifiers (not legacy rolls)
                // can have same stat translations as granted by new Modifiers
                // but swapped translation strings for positive and negative rolls
                *b = Bounds { max: b.min, min: b.max };
                // don't consider them as a legacy rolls
            }

            if v.roll > b.max {
                b.max = v.roll;
                legacy_stat_rolls = true;
            }
            if v.roll < b.min {
                b.min = v.roll;
                legacy_stat_rolls = true;
            }
        }

        if combination.values.is_empty()
            && let Some(value) = matcher.value.filter(|v| *v != 0.0 && !v.is_nan()) {
                combination.values = vec![CapturedValue {
                    roll: value,
                    roll_str: String::new(),
                    decimal: false,
                    bounds: Some(Bounds { min: value, max: value }),
                }];
            }

        let roll = if combination.values.is_empty() {
            None
        } else {
            let vals = &combination.values;
            Some(ParsedRoll {
                unscalable: stat.unscalable,
                generation: if legacy_stat_rolls { Some(RollGeneration::Legacy) } else { None },
                dp: found_stat.dp || vals.iter().any(|v| v.decimal),
                value: get_roll_or_minmax_avg(&vals.iter().map(|v| v.roll).collect::<Vec<_>>()),
                min: get_roll_or_minmax_avg(
                    &vals.iter().map(|v| v.bounds.map(|b| b.min).unwrap_or(v.roll)).collect::<Vec<_>>(),
                ),
                max: get_roll_or_minmax_avg(
                    &vals.iter().map(|v| v.bounds.map(|b| b.max).unwrap_or(v.roll)).collect::<Vec<_>>(),
                ),
            })
        };

        return Some(ParsedStat { stat: found_stat, translation: matcher, roll });
    }
    None
}

/// Upstream `getRollOrMinmaxAvg`. Returns `NaN` for an empty slice (JS `undefined` arithmetic).
pub fn get_roll_or_minmax_avg(values: &[f64]) -> f64 {
    if values.len() == 2 {
        (values[0] + values[1]) / 2.0
    } else {
        values.first().copied().unwrap_or(f64::NAN)
    }
}

/// Upstream `FindResolveParams`.
#[derive(Debug, Clone, PartialEq)]
pub struct FindResolveParams {
    pub match_str: String,
    pub mod_type: ModifierType,
    pub item_category: Option<ItemCategory>,
    pub roll: Option<f64>,
}

fn matcher_hits(m: &StatMatcher, match_str: &str) -> bool {
    m.string == match_str || m.advanced.as_deref() == Some(match_str)
}

/// Upstream `findAndResolveTranslation`.
fn find_and_resolve_translation(data: &GameData, params: &FindResolveParams) -> Option<(Stat, StatMatcher)> {
    let stat_or_group = data.stat_by_match_str_v2(&params.match_str)?;
    let stat = match stat_or_group {
        StatOrGroup::Stat(s) => s.clone(),
        StatOrGroup::Group(g) => resolve_translation(g, params)?,
    };
    let matcher = stat.matchers.iter().find(|m| matcher_hits(m, &params.match_str))?.clone();
    Some((stat, matcher))
}

/// Upstream `_resolveTranslation`.
///
/// Upstream mutated the freshly `JSON.parse`d stats when merging trade ids;
/// here the returned `Stat` is an owned (possibly merged) clone, which is
/// equivalent because upstream never reused the parsed objects.
pub fn resolve_translation(stat_group: &StatGroup, params: &FindResolveParams) -> Option<Stat> {
    let StatGroup { resolve, stats } = stat_group;
    let match_str = params.match_str.as_str();

    if let StatGroupResolver::Select { test } = resolve {
        // give priority to exact match
        let mut idx = test
            .iter()
            .position(|expected| expected.as_deref().is_some_and(|e| test_item_category(params.item_category, e)));
        // fallback to any match (if it exists at all)
        if idx.is_none() {
            idx = test.iter().position(Option::is_none);
        }
        return idx.and_then(|i| stats.get(i)).cloned();
    }

    let on_trade_stats: Vec<&Stat> =
        stats.iter().filter(|s| s.trade.ids.contains_key(params.mod_type.as_str())).collect();
    if on_trade_stats.len() == 1 {
        return Some(on_trade_stats[0].clone());
    }

    match resolve {
        StatGroupResolver::TrivialMerge => {
            let with_match_str: Vec<&Stat> = if !match_str.is_empty() {
                on_trade_stats
                    .into_iter()
                    .filter(|s| s.matchers.iter().any(|m| matcher_hits(m, match_str)))
                    .collect()
            } else {
                on_trade_stats
            };
            let (first, rest) = with_match_str.split_first()?;
            let mut merged = (*first).clone();
            for s in rest {
                merge_trade_ids_into(&mut merged, s, None);
            }
            Some(merged)
        }
        StatGroupResolver::PercentMerge { kind } => {
            let pct_stat = stats.get(kind.iter().position(|k| k == "percent")?)?;
            let matcher = pct_stat.matchers.iter().find(|m| matcher_hits(m, match_str));
            let roll100 = matcher.is_some_and(|m| m.value == Some(100.0));
            if roll100 {
                let other_stat = stats.get(kind.iter().position(|k| k == "value")?)?;
                let flag = other_stat.matchers.len() == 1 && !other_stat.matchers[0].string.contains('#');
                let mut merged = pct_stat.clone();
                merge_trade_ids_into(
                    &mut merged,
                    other_stat,
                    Some(if flag { "{empty_if_100}" } else { "{div_by_100}" }),
                );
                return Some(merged);
            }
            stats.iter().find(|s| s.matchers.iter().any(|m| matcher_hits(m, match_str))).cloned()
        }
        StatGroupResolver::FlagMerge { kind } => {
            let roll = params.roll?;
            let val_stat = stats.get(kind.iter().position(|k| k == "value")?)?;
            let flag_stat = stats.get(kind.iter().position(|k| k == "flag")?)?;
            let flag_roll = flag_stat.matchers.first().and_then(|m| m.value);
            let mut merged = val_stat.clone();
            if Some(roll) == flag_roll {
                merge_trade_ids_into(&mut merged, flag_stat, Some("{empty}"));
            }
            Some(merged)
        }
        StatGroupResolver::Select { .. } => None,
    }
}

fn merge_trade_ids_into(dest: &mut Stat, source: &Stat, prefix: Option<&str>) {
    for (mod_type, ids) in &source.trade.ids {
        let Some(first) = ids.first() else { continue };
        let trade_id = match prefix {
            Some(p) => format!("{p}{first}"),
            None => first.clone(),
        };
        match dest.trade.ids.get_mut(mod_type) {
            Some(dest_ids) => {
                if !dest_ids.contains(&trade_id) {
                    dest_ids.push(trade_id);
                }
            }
            None => {
                dest.trade.ids.insert(mod_type.clone(), vec![trade_id]);
            }
        }
    }
}

fn test_item_category(actual: Option<ItemCategory>, expected: &str) -> bool {
    let Some(actual) = actual else { return false };
    match expected {
        "WEAPON" => actual.is_weapon(),
        "ARMOUR" => actual.is_armour(),
        "HEIST_EQUIPMENT" => actual.is_heist_equipment(),
        _ => expected == actual.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        let c = stat_placeholder_combinations("Adds 5(3-6) to 10(9-12) Fire Damage");
        assert_eq!(c[0].stat, "Adds 5 to 10 Fire Damage");
        assert_eq!(c[3].stat, "Adds # to # Fire Damage");
        assert_eq!(c[3].values.len(), 2);
        assert_eq!(c[3].values[0].bounds, Some(Bounds { min: 3.0, max: 6.0 }));
        assert_eq!(c.last().unwrap().values.len(), 0);

        let c = stat_placeholder_combinations("-10(-15--5)% to Fire Resistance");
        assert_eq!(c[1].stat, "#% to Fire Resistance");
        assert_eq!(c[1].values[0].bounds, Some(Bounds { min: -15.0, max: -5.0 }));
    }

    #[test]
    fn stat_strings_generator() {
        let lines: Vec<String> = ["a", "b", "c", "(reminder", "text)", "d"].iter().map(|s| s.to_string()).collect();
        let mut seen = Vec::new();
        let rest = lines_to_stat_strings_with(&lines, " — Unscalable Value", |s| {
            seen.push(s.string.clone());
            s.string == "a\nb"
        });
        assert_eq!(rest, vec!["c", "d"]);
        // multi-line candidates may span reminder lines (same as upstream)
        assert_eq!(
            seen,
            vec!["a", "a\nb", "c", "c\n(reminder", "c\n(reminder\ntext)", "c\n(reminder\ntext)\nd", "d"]
        );
    }
}
