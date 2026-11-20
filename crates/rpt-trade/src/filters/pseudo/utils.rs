//! Port of upstream `filters/pseudo/utils.ts`.

use rpt_data::{Stat, StatGroup, StatGroupResolver, StatOrGroup};
use rpt_parser::{ItemCategory, ModifierType};

use crate::filters::{FilterTag, StatFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveHint {
    PercentMerge { pick_percent: bool },
    FlagMerge { pick_flag: bool },
}

/// Upstream `testItemCategory` from `parser/stat-translations.ts`.
fn test_item_category(actual: Option<ItemCategory>, expected: &str) -> bool {
    let Some(actual) = actual else { return false };
    match expected {
        "WEAPON" => actual.is_weapon(),
        "ARMOUR" => actual.is_armour(),
        "HEIST_EQUIPMENT" => actual.is_heist_equipment(),
        _ => expected == actual.as_str(),
    }
}

fn merge_trade_ids_into(dest: &mut Stat, source: &Stat) {
    for (mod_type, ids) in &source.trade.ids {
        let Some(trade_id) = ids.first() else { continue };
        let entry = dest.trade.ids.entry(mod_type.clone()).or_default();
        if !entry.contains(trade_id) {
            entry.push(trade_id.clone());
        }
    }
}

/// Upstream `_resolveTranslation` with `matchStr: ''` and no roll — the only
/// way the filters code calls it. Returns an owned (possibly merged) stat
/// instead of mutating the shared database like upstream does.
fn resolve_translation_no_match(group: &StatGroup, mod_type: ModifierType, category: Option<ItemCategory>) -> Option<Stat> {
    match &group.resolve {
        StatGroupResolver::Select { test } => {
            // give priority to exact match
            let mut idx = test
                .iter()
                .position(|e| e.as_deref().is_some_and(|e| test_item_category(category, e)));
            // fallback to any match (if it exists at all)
            if idx.is_none() {
                idx = test.iter().position(Option::is_none);
            }
            idx.and_then(|i| group.stats.get(i)).cloned()
        }
        _ => {
            let on_trade: Vec<&Stat> =
                group.stats.iter().filter(|s| s.trade.ids.contains_key(mod_type.as_str())).collect();
            if on_trade.len() == 1 {
                return Some(on_trade[0].clone());
            }
            match &group.resolve {
                StatGroupResolver::TrivialMerge => {
                    let (first, rest) = on_trade.split_first()?;
                    let mut merged = (*first).clone();
                    for s in rest {
                        merge_trade_ids_into(&mut merged, s);
                    }
                    Some(merged)
                }
                // percent-merge with empty matchStr finds nothing; flag-merge needs a roll
                _ => None,
            }
        }
    }
}

fn find_and_resolve_by_ref_opt(
    name: &str,
    mod_type: ModifierType,
    category: Option<ItemCategory>,
    hint: Option<ResolveHint>,
) -> Option<Stat> {
    let data = rpt_data::data();
    let sog = data.stat_by_ref_v2(name)?;

    let stat = match sog {
        StatOrGroup::Stat(s) => Some(s.clone()),
        StatOrGroup::Group(group) => match (&group.resolve, hint) {
            (StatGroupResolver::Select { .. } | StatGroupResolver::TrivialMerge, _) => {
                resolve_translation_no_match(group, mod_type, category)
            }
            (StatGroupResolver::PercentMerge { kind }, Some(ResolveHint::PercentMerge { pick_percent })) => {
                // NOTE 'value' stat can actually be a flag
                let want = if pick_percent { "percent" } else { "value" };
                kind.iter().position(|k| k == want).and_then(|i| group.stats.get(i)).cloned()
            }
            (StatGroupResolver::FlagMerge { kind }, Some(ResolveHint::FlagMerge { pick_flag })) => {
                let want = if pick_flag { "flag" } else { "value" };
                kind.iter().position(|k| k == want).and_then(|i| group.stats.get(i)).cloned()
            }
            _ => None,
        },
    }?;

    stat.trade.ids.contains_key(mod_type.as_str()).then_some(stat)
}

/// Upstream `findAndResolveByRef`. Upstream throws on unexpected stat shapes;
/// here `None` is returned (and logged) so a data mismatch cannot crash the UI.
pub fn find_and_resolve_by_ref(
    name: &str,
    mod_type: ModifierType,
    category: Option<ItemCategory>,
    hint: Option<ResolveHint>,
) -> Option<Stat> {
    let stat = find_and_resolve_by_ref_opt(name, mod_type, category, hint);
    if stat.is_none() {
        tracing::warn!("Unexpected stat shape: {name}");
    }
    stat
}

pub fn stat_to_not_filter(stat: &Stat, type_: ModifierType, negate_string: bool, disabled: bool) -> StatFilter {
    let text = stat
        .matchers
        .iter()
        .find(|m| m.negate == negate_string)
        .or_else(|| stat.matchers.first())
        .map(|m| m.string.clone())
        .unwrap_or_default();
    let mut f = StatFilter::bare(
        stat.trade.ids.get(type_.as_str()).cloned().unwrap_or_default(),
        &stat.ref_,
        &text,
        FilterTag::Mod(type_),
        disabled,
    );
    f.not = true;
    f
}

pub fn meta_not_filter(disabled: bool) -> StatFilter {
    StatFilter::bare(vec!["item.not_group".to_owned()], "Not", "Not", FilterTag::FilterGroup, disabled)
}

pub fn meta_count_one_filter(disabled: bool) -> StatFilter {
    StatFilter::bare(vec!["item.count_one_group".to_owned()], "Count (1)", "Count (1)", FilterTag::FilterGroup, disabled)
}
