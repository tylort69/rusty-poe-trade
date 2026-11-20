//! Port of upstream `filters/pseudo/index.ts` (+ submodules).

pub mod anointments;
pub mod atzoatl_rules;
pub mod flasks;
pub mod heist;
pub mod item_property;
pub mod maps;
pub mod mercenary;
pub mod reflection_rules;
pub mod timeless_jewel;
pub mod utils;

use std::collections::HashMap;

use rpt_parser::{ItemRarity, ModifierType, StatCalculated, StatRoll, StatSource};

use super::create_stat_filters::{FiltersCreationContext, calculated_stat_to_filter};
use super::StatFilter;

struct ResInfo {
    ref_: &'static str,
    elements: &'static [&'static str],
    chaos: bool,
}

const RESISTANCES_INFO: &[ResInfo] = &[
    ResInfo { ref_: "+#% to All Resistances", elements: &["fire", "cold", "lightning"], chaos: true },
    ResInfo { ref_: "+#% to all Elemental Resistances", elements: &["fire", "cold", "lightning"], chaos: false },
    ResInfo { ref_: "+#% to Fire Resistance", elements: &["fire"], chaos: false },
    ResInfo { ref_: "+#% to Cold Resistance", elements: &["cold"], chaos: false },
    ResInfo { ref_: "+#% to Lightning Resistance", elements: &["lightning"], chaos: false },
    ResInfo { ref_: "+#% to Fire and Lightning Resistances", elements: &["fire", "lightning"], chaos: false },
    ResInfo { ref_: "+#% to Fire and Cold Resistances", elements: &["fire", "cold"], chaos: false },
    ResInfo { ref_: "+#% to Cold and Lightning Resistances", elements: &["cold", "lightning"], chaos: false },
    ResInfo { ref_: "+#% to Chaos Resistance", elements: &[], chaos: true },
    ResInfo { ref_: "+#% to Fire and Chaos Resistances", elements: &["fire"], chaos: true },
    ResInfo { ref_: "+#% to Cold and Chaos Resistances", elements: &["cold"], chaos: true },
    ResInfo { ref_: "+#% to Lightning and Chaos Resistances", elements: &["lightning"], chaos: true },
];

struct AttrInfo {
    ref_: &'static str,
    attributes: &'static [&'static str],
}

const ATTRIBUTES_INFO: &[AttrInfo] = &[
    AttrInfo { ref_: "+# to all Attributes", attributes: &["str", "dex", "int"] },
    AttrInfo { ref_: "+# to Strength", attributes: &["str"] },
    AttrInfo { ref_: "+# to Dexterity", attributes: &["dex"] },
    AttrInfo { ref_: "+# to Intelligence", attributes: &["int"] },
    AttrInfo { ref_: "+# to Strength and Intelligence", attributes: &["str", "int"] },
    AttrInfo { ref_: "+# to Strength and Dexterity", attributes: &["str", "dex"] },
    AttrInfo { ref_: "+# to Dexterity and Intelligence", attributes: &["dex", "int"] },
];

#[derive(Debug, Clone)]
struct RuleStat {
    ref_: &'static str,
    multiplier: Option<f64>,
    required: bool,
    keep: bool,
}

fn rs(ref_: &'static str) -> RuleStat {
    RuleStat { ref_, multiplier: None, required: false, keep: false }
}
fn req(ref_: &'static str) -> RuleStat {
    RuleStat { required: true, ..rs(ref_) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mutate {
    ChaosRes,
}

struct PseudoRule {
    group: Option<&'static str>,
    pseudo: &'static str,
    disabled: Option<bool>,
    replaces: Option<&'static str>,
    stats: Vec<RuleStat>,
    mutate: Option<Mutate>,
}

fn rule(pseudo: &'static str, stats: Vec<RuleStat>) -> PseudoRule {
    PseudoRule { group: None, pseudo, disabled: None, replaces: None, stats, mutate: None }
}

fn res_for(element: &str) -> Vec<RuleStat> {
    RESISTANCES_INFO
        .iter()
        .filter(|i| i.elements.contains(&element))
        .map(|i| RuleStat { keep: i.elements.len() > 1 || i.chaos, ..rs(i.ref_) })
        .collect()
}

fn attr_for(attr: &str) -> Vec<RuleStat> {
    ATTRIBUTES_INFO
        .iter()
        .filter(|i| i.attributes.contains(&attr))
        .map(|i| RuleStat { keep: i.attributes.len() > 1, ..rs(i.ref_) })
        .collect()
}

fn attr_contrib(attr: &str) -> impl Iterator<Item = RuleStat> + '_ {
    ATTRIBUTES_INFO
        .iter()
        .filter(move |i| i.attributes.contains(&attr))
        .map(|i| RuleStat { multiplier: Some(5.0 / 10.0), keep: true, ..rs(i.ref_) })
}

fn pseudo_rules() -> Vec<PseudoRule> {
    vec![
        PseudoRule {
            disabled: Some(false),
            ..rule(
                "+#% total Elemental Resistance",
                RESISTANCES_INFO
                    .iter()
                    .filter(|i| !i.elements.is_empty())
                    .map(|i| RuleStat { multiplier: Some(i.elements.len() as f64), keep: i.chaos, ..rs(i.ref_) })
                    .collect(),
            )
        },
        PseudoRule { group: Some("to_x_ele_res"), ..rule("+#% total to Fire Resistance", res_for("fire")) },
        PseudoRule { group: Some("to_x_ele_res"), ..rule("+#% total to Cold Resistance", res_for("cold")) },
        PseudoRule { group: Some("to_x_ele_res"), ..rule("+#% total to Lightning Resistance", res_for("lightning")) },
        PseudoRule {
            mutate: Some(Mutate::ChaosRes),
            ..rule(
                "+#% total to Chaos Resistance",
                RESISTANCES_INFO
                    .iter()
                    .filter(|i| i.chaos)
                    .map(|i| RuleStat { keep: !i.elements.is_empty(), ..rs(i.ref_) })
                    .collect(),
            )
        },
        PseudoRule {
            group: Some("to_all_attrs"),
            // NOTE: not including other sources from `ATTRIBUTES_INFO`
            ..rule("+# total to all Attributes", vec![rs("+# to all Attributes")])
        },
        PseudoRule { group: Some("to_x_attr"), ..rule("+# total to Strength", attr_for("str")) },
        PseudoRule { group: Some("to_x_attr"), ..rule("+# total to Dexterity", attr_for("dex")) },
        PseudoRule { group: Some("to_x_attr"), ..rule("+# total to Intelligence", attr_for("int")) },
        PseudoRule {
            disabled: Some(false),
            ..rule(
                "+# total maximum Life",
                std::iter::once(req("+# to maximum Life")).chain(attr_contrib("str")).collect(),
            )
        },
        rule("+# total maximum Mana", std::iter::once(req("+# to maximum Mana")).chain(attr_contrib("int")).collect()),
        rule("#% total increased maximum Energy Shield", vec![rs("#% increased maximum Energy Shield")]),
        rule("+# total maximum Energy Shield", vec![rs("+# to maximum Energy Shield")]), // global
        rule("+#% total Attack Speed", vec![rs("#% increased Attack Speed")]),             // global
        rule("+#% total Cast Speed", vec![rs("#% increased Cast Speed")]),
        rule("#% increased Movement Speed", vec![rs("#% increased Movement Speed")]),
        rule("#% total increased Physical Damage", vec![rs("#% increased Global Physical Damage")]),
        PseudoRule {
            group: Some("global_crit_chance"),
            ..rule("+#% Global Critical Strike Chance", vec![rs("#% increased Global Critical Strike Chance")])
        },
        PseudoRule {
            replaces: Some("global_crit_chance"),
            ..rule(
                "+#% total Critical Strike Chance for Spells",
                vec![req("#% increased Spell Critical Strike Chance"), rs("#% increased Global Critical Strike Chance")],
            )
        },
        rule("+#% Global Critical Strike Multiplier", vec![rs("+#% to Global Critical Strike Multiplier")]),
        PseudoRule {
            group: Some("incr_ele_dmg"),
            ..rule("#% increased Elemental Damage", vec![rs("#% increased Elemental Damage")])
        },
        PseudoRule {
            replaces: Some("incr_ele_dmg"),
            ..rule(
                "#% increased Lightning Damage",
                vec![req("#% increased Lightning Damage"), rs("#% increased Elemental Damage")],
            )
        },
        PseudoRule {
            replaces: Some("incr_ele_dmg"),
            ..rule("#% increased Cold Damage", vec![req("#% increased Cold Damage"), rs("#% increased Elemental Damage")])
        },
        PseudoRule {
            group: Some("incr_fire_dmg"),
            replaces: Some("incr_ele_dmg"),
            ..rule("#% increased Fire Damage", vec![req("#% increased Fire Damage"), rs("#% increased Elemental Damage")])
        },
        PseudoRule {
            group: Some("incr_spell_dmg"),
            ..rule("#% increased Spell Damage", vec![rs("#% increased Spell Damage")])
        },
        PseudoRule {
            replaces: Some("incr_spell_dmg"),
            ..rule(
                "#% increased Lightning Spell Damage",
                vec![req("#% increased Lightning Spell Damage"), rs("#% increased Spell Damage")],
            )
        },
        PseudoRule {
            replaces: Some("incr_spell_dmg"),
            ..rule(
                "#% increased Cold Spell Damage",
                vec![req("#% increased Cold Spell Damage"), rs("#% increased Spell Damage")],
            )
        },
        PseudoRule {
            replaces: Some("incr_spell_dmg"),
            ..rule(
                "#% increased Fire Spell Damage",
                vec![req("#% increased Fire Spell Damage"), rs("#% increased Spell Damage")],
            )
        },
        PseudoRule {
            replaces: Some("incr_ele_dmg"),
            ..rule(
                "#% increased Elemental Damage with Attack Skills",
                vec![req("#% increased Elemental Damage with Attack Skills"), rs("#% increased Elemental Damage")],
            )
        },
        PseudoRule {
            replaces: Some("incr_fire_dmg"),
            ..rule(
                "#% increased Burning Damage",
                vec![
                    req("#% increased Burning Damage"),
                    rs("#% increased Fire Damage"),
                    rs("#% increased Elemental Damage"),
                ],
            )
        },
        rule("# Life Regenerated per Second", vec![rs("Regenerate # Life per second")]),
        rule("#% of Life Regenerated per Second", vec![rs("Regenerate #% of Life per second")]),
        rule(
            "#% of Physical Attack Damage Leeched as Life",
            vec![rs("#% of Physical Attack Damage Leeched as Life")],
        ),
        rule(
            "#% of Physical Attack Damage Leeched as Mana",
            vec![rs("#% of Physical Attack Damage Leeched as Mana")],
        ),
        rule("#% increased Mana Regeneration Rate", vec![rs("#% increased Mana Regeneration Rate")]),
    ]
}

/// All stat refs used by pseudo rules (for data validation in tests).
pub fn pseudo_rule_refs() -> Vec<&'static str> {
    let mut out = Vec::new();
    for r in pseudo_rules() {
        out.push(r.pseudo);
        out.extend(r.stats.iter().map(|s| s.ref_));
    }
    out
}

pub fn filter_pseudo(ctx: &mut FiltersCreationContext<'_>) {
    let rules = pseudo_rules();
    let data = rpt_data::data();

    // pseudo filters created here; `None` = removed by a `replaces` rule
    let mut created: Vec<Option<StatFilter>> = Vec::new();
    let mut filter_by_group: HashMap<&'static str, Vec<usize>> = HashMap::new();
    let mut applied_rules: Vec<usize> = Vec::new();

    'rules: for (rule_idx, rule) in rules.iter().enumerate() {
        let sources = filter_pseudo_sources(&ctx.stats_by_type, |calc, source| {
            let info = rule.stats.iter().find(|info| info.ref_ == calc.stat.ref_)?;
            let multi = info.multiplier.unwrap_or(1.0);
            let c = source.contributes.unwrap_or(StatRoll { value: 0.0, min: 0.0, max: 0.0 });
            Some(StatSource {
                contributes: Some(StatRoll { value: c.value * multi, min: c.min * multi, max: c.max * multi }),
                ..source.clone()
            })
        });
        if sources.is_empty() {
            continue;
        }

        for stat in rule.stats.iter().filter(|s| s.required) {
            if !sources.iter().any(|source| source.stat.stat.ref_ == stat.ref_) {
                continue 'rules;
            }
        }

        if ctx.item.rarity == Some(ItemRarity::Unique) {
            if ctx.item.is_corrupted.is_some() {
                if sources.len() < 2 {
                    continue;
                }
            } else {
                let explicit = sources.iter().filter(|s| s.modifier.info.type_ == ModifierType::Explicit).count();
                if explicit < 2 {
                    continue;
                }
            }
        }

        let Some(pseudo_stat) = data.pseudo_stat_by_ref(rule.pseudo) else {
            tracing::warn!("pseudo stat not found: {}", rule.pseudo);
            continue;
        };

        let mut filter = calculated_stat_to_filter(
            &StatCalculated { stat: pseudo_stat.clone(), type_: ModifierType::Pseudo, sources },
            ctx.search_in_range,
            ctx.item,
        );

        filter.disabled = rule.disabled.unwrap_or(true);

        if rule.mutate == Some(Mutate::ChaosRes) {
            if filter.sources.len() == 1 && filter.sources[0].modifier.info.type_ == ModifierType::Crafted {
                filter.hidden = Some("filters.hide_crafted_chaos".to_owned());
            } else {
                filter.disabled = false;
            }
        }

        let idx = created.len();
        created.push(Some(filter));
        applied_rules.push(rule_idx);

        if let Some(replaced) = rule.replaces.and_then(|r| filter_by_group.get(r)) {
            for &i in replaced {
                created[i] = None;
            }
        }

        if let Some(group) = rule.group {
            filter_by_group.entry(group).or_default().push(idx);
        }
    }

    let is_unique = ctx.item.rarity == Some(ItemRarity::Unique);
    ctx.stats_by_type.retain(|m| {
        !applied_rules.iter().any(|&ri| {
            rules[ri].stats.iter().any(|s| m.stat.ref_ == s.ref_ && (!s.keep || !is_unique))
        })
    });

    let value_of = |created: &Vec<Option<StatFilter>>, i: usize| -> f64 {
        created[i].as_ref().and_then(|f| f.roll.as_ref()).map(|r| r.value).unwrap_or(f64::NAN)
    };
    let sort_desc = |created: &Vec<Option<StatFilter>>, ids: &mut Vec<usize>| {
        ids.sort_by(|&a, &b| {
            value_of(created, b).partial_cmp(&value_of(created, a)).unwrap_or(std::cmp::Ordering::Equal)
        });
    };

    if let Some(mut res_filters) = filter_by_group.get("to_x_ele_res").cloned() {
        // NOTE: upstream sorts the group array which may contain filters already removed
        // from ctx.filters; we keep the same semantics.
        sort_desc(&created, &mut res_filters);
        let v0 = res_filters.first().map(|&i| value_of(&created, i));
        let v1 = res_filters.get(1).map(|&i| value_of(&created, i));
        let max_filter = if v0 == v1 { None } else { res_filters.first().copied() };

        if let Some(i) = max_filter {
            if let Some(f) = &mut created[i] {
                f.hidden = Some("filters.hide_ele_res".to_owned());
            }
        }
        for &i in &res_filters {
            if Some(i) != max_filter {
                created[i] = None;
            }
        }
    }

    if let Some(mut attr_filters) = filter_by_group.get("to_x_attr").cloned() {
        sort_desc(&created, &mut attr_filters);
        if attr_filters.len() == 3 {
            let to_all = filter_by_group.get("to_all_attrs").cloned();
            let first = value_of(&created, attr_filters[0]);
            let all_equal = attr_filters.iter().all(|&i| value_of(&created, i) == first);
            if all_equal && to_all.is_some() {
                for &i in &attr_filters {
                    created[i] = None;
                }
            } else {
                if let Some(to_all) = &to_all {
                    for &i in to_all {
                        created[i] = None;
                    }
                }

                let v1 = value_of(&created, attr_filters[1]);
                let v2 = value_of(&created, attr_filters[2]);
                if v2 / first < 0.3 {
                    if v1 == v2 {
                        for &i in &attr_filters[1..3] {
                            if let Some(f) = &mut created[i] {
                                f.hidden = Some("hide_attr_same_2nd_n_3rd".to_owned());
                            }
                        }
                    } else if let Some(f) = &mut created[attr_filters[2]] {
                        f.hidden = Some("hide_attr_smallest_total".to_owned());
                    }
                }
            }
        }
    }

    ctx.filters.extend(created.into_iter().flatten());
}

fn filter_pseudo_sources(
    stats: &[StatCalculated],
    map_fn: impl Fn(&StatCalculated, &StatSource) -> Option<StatSource>,
) -> Vec<StatSource> {
    let mut out = Vec::new();
    for calc in stats {
        for source in &calc.sources {
            if let Some(result) = map_fn(calc, source) {
                out.push(result);
            }
        }
    }
    out
}
