//! Port of upstream `filters/pseudo/mercenary.ts`.

use rpt_data::{MercenaryBuild, MercenaryBuildRef, Stat};
use rpt_parser::{
    ItemCategory, ModifierInfo, ModifierType, ParsedItem, ParsedModifier, ParsedStat, StatRoll, StatSource,
};

use super::item_property::{PropOpts, prop_to_filter};
use super::utils::{find_and_resolve_by_ref, meta_not_filter};
use crate::filters::{FilterGroup, FilterOption, FilterOrGroup, FilterTag, GroupKind, MercenaryFilterInfo, StatFilter};

struct BuildRule {
    build: &'static str,
    bad_skills: &'static [&'static str],
}

const BUILD_RULES: &[BuildRule] = &[
    BuildRule { build: "Kineticist", bad_skills: &["Kinetic Bolt"] },
    BuildRule { build: "Manyshot", bad_skills: &["Icicle Rain"] },
    BuildRule { build: "Combatant", bad_skills: &["Wild Strike", "Spectral Helix"] },
];

pub fn used_stat_refs() -> Vec<&'static str> {
    BUILD_RULES.iter().flat_map(|r| r.bad_skills.iter().copied()).collect()
}

/// Upstream `SearchMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SearchMode {
    Optional = 0,
    Required = 1,
}

impl SearchMode {
    pub fn value(self) -> f64 {
        self as u8 as f64
    }
}

fn merc_tier(stat: &Stat) -> u32 {
    stat.mercenary.as_ref().and_then(|m| m.tier).unwrap_or(0)
}
fn merc_synthetic(stat: &Stat) -> bool {
    stat.mercenary.as_ref().is_some_and(|m| m.synthetic_family)
}
fn merc_canonical(stat: &Stat) -> Option<&str> {
    stat.mercenary.as_ref().and_then(|m| m.canonical.as_deref())
}
fn pseudo_ids(stat: &Stat) -> Vec<String> {
    stat.trade.ids.get("pseudo").cloned().unwrap_or_default()
}
fn last_in_family(stat: &Stat) -> bool {
    stat.mod_family.as_ref().and_then(|f| f.last()).is_some_and(|last| *last == stat.ref_)
}

pub fn create_mercenary_filters(item: &ParsedItem) -> Vec<FilterOrGroup> {
    let mut out: Vec<FilterOrGroup> = Vec::new();

    let Some(build_info) = &item.mercenary_build else { return out };
    let rules = BUILD_RULES.iter().find(|r| r.build == build_info.ref_name);
    let bad_skills = rules.map(|r| r.bad_skills);
    let mercenary_build: MercenaryBuild = match &build_info.mercenary_build {
        Some(MercenaryBuildRef::Build(b)) => b.clone(),
        Some(MercenaryBuildRef::RefName(name)) => {
            let data = rpt_data::data();
            let found = data.item_by_ref(rpt_data::Namespace::MercenaryBuild, name).and_then(|v| {
                v.iter().find_map(|b| match &b.mercenary_build {
                    Some(MercenaryBuildRef::Build(b)) => Some(b.clone()),
                    _ => None,
                })
            });
            match found {
                Some(b) => b,
                None => return out,
            }
        }
        None => return out,
    };
    let skills_groups = item.mercenary_skills.clone().unwrap_or_default();

    for group in &skills_groups {
        let Some((skill, raw_supports)) = group.split_first() else { continue };
        let supports_filtered: Vec<&ParsedStat> = raw_supports
            .iter()
            .filter(|s| !merc_synthetic(&s.stat) || merc_tier(&s.stat) == 3)
            .collect();

        let skill_type = mercenary_build
            .skills
            .iter()
            .find(|b| b.name == skill.stat.ref_)
            .map(|b| b.type_.clone())
            .unwrap_or_default();
        let mut skill_filter = skill_to_filter(&skill.stat, &skill_type, true, bad_skills);

        if supports_filtered.is_empty() {
            if skill_type == "primary" && raw_supports.is_empty() {
                skill_filter.hidden = Some("filters.mercenary_primary_no_support".to_owned());
            }
            out.push(FilterOrGroup::Stat(skill_filter));
            continue;
        }

        let mut filter_group =
            FilterGroup { group: GroupKind::Mercenary, expanded: false, meta: skill_filter, stats: Vec::new() };

        let mut supports = supports_filtered;
        // bitwise AND changes tier 4 to 0, so it goes at the very end
        supports.sort_by(|a, b| (merc_tier(&b.stat) & 0x3).cmp(&(merc_tier(&a.stat) & 0x3)));

        let possible_supports: Vec<Vec<Stat>> = skill
            .stat
            .mercenary
            .as_ref()
            .and_then(|m| m.supports.clone())
            .unwrap_or_default()
            .iter()
            .filter_map(|family_name| {
                let family_stat = find_gem_by_ref(family_name)?;
                Some(match &family_stat.mod_family {
                    Some(fam) => fam.iter().filter_map(|n| find_gem_by_ref(n)).collect(),
                    None => vec![family_stat],
                })
            })
            .collect();

        if supports.len() == 5 && !possible_supports.is_empty() {
            let tier3_count = supports
                .iter()
                .filter(|s| merc_tier(&s.stat) >= 3 || last_in_family(&s.stat))
                .count();
            filter_group.stats.push(prop_to_filter(
                PropOpts {
                    ref_: "6-Link",
                    trade_id: "item.mercenary_6link",
                    roll: Some(StatRoll { min: 0.0, max: 5.0, value: tier3_count as f64 }),
                    sources: possible_supports.iter().map(|f| encode_family_to_source(f)).collect(),
                    dp: None,
                    disabled: Some(true),
                    hidden: None,
                },
                item,
                0.0,
            ));
        }

        for support in &supports {
            let tier = merc_tier(&support.stat);

            let trade_id = match &support.stat.mod_family {
                Some(family) if tier != 3 => {
                    let mut ids = Vec::new();
                    for r in family {
                        if let Some(family_stat) = find_gem_by_ref(r) {
                            if merc_tier(&family_stat) >= tier {
                                ids.extend(pseudo_ids(&family_stat));
                            }
                        }
                    }
                    ids
                }
                _ => pseudo_ids(&support.stat),
            };

            let canon_stat = merc_canonical(&support.stat)
                .and_then(find_gem_by_ref)
                .unwrap_or_else(|| support.stat.clone());

            let mut f = StatFilter::bare(
                trade_id,
                &canon_stat.ref_,
                canon_stat.matchers.first().map(|m| m.string.as_str()).unwrap_or(""),
                FilterTag::MercenarySupport,
                true,
            );
            f.mercenary = Some(MercenaryFilterInfo {
                icon: None,
                tier: Some(tier),
                max_tier: Some(tier >= 3 || last_in_family(&support.stat)),
            });
            f.option = Some(FilterOption { value: SearchMode::Required.value() });
            filter_group.stats.push(f);
        }

        if supports.len() == 5 && !possible_supports.is_empty() {
            for family in &possible_supports {
                let Some(first) = family.first() else { continue };
                let linked = match merc_canonical(first) {
                    Some(canon) => raw_supports.iter().any(|s| merc_canonical(&s.stat) == Some(canon)),
                    None => raw_supports.iter().any(|s| s.stat.ref_ == first.ref_),
                };
                if linked {
                    continue;
                }

                let canon_stat = match merc_canonical(first) {
                    Some(canon) => family.iter().find(|s| s.ref_ == canon).unwrap_or(first),
                    None => first,
                };

                let mut f = StatFilter::bare(
                    family.iter().flat_map(pseudo_ids).collect(),
                    &canon_stat.ref_,
                    canon_stat.matchers.first().map(|m| m.string.as_str()).unwrap_or(""),
                    FilterTag::MercenarySupport,
                    true,
                );
                f.not = true;
                filter_group.stats.push(f);
            }
        }

        out.push(FilterOrGroup::Group(filter_group));
    }

    let mut not_group =
        FilterGroup { group: GroupKind::Not, expanded: false, meta: meta_not_filter(true), stats: Vec::new() };

    for build_skill in &mercenary_build.skills {
        if skills_groups.iter().any(|g| g.first().is_some_and(|s| s.stat.ref_ == build_skill.name)) {
            continue;
        }
        let Some(stat) = find_gem_by_ref(&build_skill.name) else { continue };
        let mut filter = skill_to_filter(&stat, &build_skill.type_, true, bad_skills);
        if filter.tag == FilterTag::Brick {
            filter.disabled = false;
        }
        not_group.stats.push(filter);
    }

    if not_group.stats.iter().any(|s| !s.disabled) {
        not_group.meta.disabled = false;
    }
    out.push(FilterOrGroup::Group(not_group));

    out
}

fn find_gem_by_ref(name: &str) -> Option<Stat> {
    find_and_resolve_by_ref(name, ModifierType::Pseudo, Some(ItemCategory::MercenaryWarrant), None)
}

/// Upstream stores a family of support gems inside a fake `StatSource`.
fn encode_family_to_source(family: &[Stat]) -> StatSource {
    let to_parsed = |stat: &Stat| ParsedStat {
        stat: stat.clone(),
        translation: stat.matchers.first().cloned().unwrap_or(rpt_data::StatMatcher {
            string: stat.ref_.clone(),
            advanced: None,
            negate: false,
            value: None,
        }),
        roll: None,
    };
    let first = family.first().map(to_parsed);
    StatSource {
        modifier: ParsedModifier {
            info: ModifierInfo::new(ModifierType::Pseudo),
            stats: family.iter().map(to_parsed).collect(),
        },
        stat: first.unwrap_or_else(|| ParsedStat {
            stat: Stat {
                ref_: String::new(),
                dp: false,
                matchers: Vec::new(),
                better: rpt_data::StatBetter::NotComparable,
                mod_family: None,
                reminder_text: None,
                from_area_mods: None,
                anointments: None,
                mercenary: None,
                jewellery_quality: None,
                trade: Default::default(),
            },
            translation: rpt_data::StatMatcher { string: String::new(), advanced: None, negate: false, value: None },
            roll: None,
        }),
        contributes: None,
    }
}

pub fn decode_family_from_source(source: &StatSource) -> Vec<Stat> {
    source.modifier.stats.iter().map(|s| s.stat.clone()).collect()
}

fn skill_to_filter(stat: &Stat, type_: &str, disabled: bool, bad_skills: Option<&[&str]>) -> StatFilter {
    let tag = match type_ {
        "primary" => FilterTag::MercenaryPrimary,
        "secondary" => FilterTag::MercenarySecondary,
        _ => FilterTag::MercenaryUtility,
    };
    let mut filter = StatFilter::bare(
        pseudo_ids(stat),
        &stat.ref_,
        stat.matchers.first().map(|m| m.string.as_str()).unwrap_or(""),
        tag,
        disabled,
    );
    filter.mercenary = Some(MercenaryFilterInfo {
        icon: stat.mercenary.as_ref().and_then(|m| m.icon.clone()),
        tier: None,
        max_tier: None,
    });

    if bad_skills.is_some_and(|b| b.contains(&stat.ref_.as_str())) {
        filter.tag = FilterTag::Brick;
    }
    filter
}
