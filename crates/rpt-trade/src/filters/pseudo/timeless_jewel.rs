//! Port of upstream `filters/pseudo/timeless-jewel.ts`.

use rpt_data::StatBetter;
use rpt_parser::modifiers::sum_stats_by_mod_type;
use rpt_parser::{ParsedModifier, ParsedStat};

use super::utils::{find_and_resolve_by_ref, meta_count_one_filter};
use crate::filters::create_stat_filters::{FiltersCreationContext, calculated_stat_to_filter};
use crate::filters::{FilterGroup, FilterTag, GroupKind};

pub fn filter_timeless_jewel_keystones(ctx: &mut FiltersCreationContext<'_>) {
    let Some(seed_idx) = ctx
        .stats_by_type
        .iter()
        .position(|calc| calc.stat.better == StatBetter::NotComparable && calc.stat.mod_family.is_some())
    else {
        return;
    };
    let tree_seed_stat = ctx.stats_by_type[seed_idx].clone();

    let mut group = FilterGroup {
        group: GroupKind::One,
        expanded: true,
        meta: meta_count_one_filter(false),
        stats: Vec::new(),
    };

    for family_stat_ref in tree_seed_stat.stat.mod_family.as_deref().unwrap_or_default() {
        let is_seed = *family_stat_ref == tree_seed_stat.stat.ref_;
        let calc = if is_seed {
            tree_seed_stat.clone()
        } else {
            let Some(family_stat) =
                find_and_resolve_by_ref(family_stat_ref, tree_seed_stat.type_, ctx.item.category, None)
            else {
                continue;
            };
            let Some(first) = tree_seed_stat.sources.first() else { continue };
            let translation = family_stat.matchers[0].clone();
            let summed = sum_stats_by_mod_type(&[ParsedModifier {
                info: first.modifier.info.clone(),
                stats: vec![ParsedStat { stat: family_stat, translation, roll: first.stat.roll.clone() }],
            }]);
            let Some(calc) = summed.into_iter().next() else { continue };
            calc
        };
        let mut filter = calculated_stat_to_filter(&calc, ctx.search_in_range, ctx.item);
        if !is_seed {
            filter.tag = FilterTag::PSEUDO;
            filter.disabled = true;
        }
        group.stats.push(filter);
    }

    ctx.stats_by_type.remove(seed_idx);
    ctx.groups.push(group);
}
