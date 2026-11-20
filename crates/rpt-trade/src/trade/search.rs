//! Port of upstream `trade/pathofexile-trade.ts` (+ the search/fetch flow of
//! `TradeListing.vue`).

use rpt_data::StatOrGroup;
use rpt_parser::ItemInfluence;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::category_to_trade_id;
use super::cache::Cache;
use super::rate_limiter::prevent_queue_creation;
use super::{Account, AccountStatus};
use crate::client::TradeClient;
use crate::filters::pseudo::mercenary::{SearchMode as MercSearchMode, decode_family_from_source};
use crate::filters::{
    FilterOption, FilterOrGroup, FilterRoll, FilterTag, GroupKind, ItemFilters, MapBlighted, SearchFilter, StatFilter,
    is_internal_trade_id,
};
use crate::js::{JsNum, encode_uri_component, json_num};
use crate::{Result, TradeError};

const CRAFTED_MODIFIERS: [&str; 3] = ["# Crafted Modifiers", "# Crafted Prefix Modifiers", "# Crafted Suffix Modifiers"];
const EMPTY_MODIFIERS: [&str; 3] = ["# Empty Modifiers", "# Empty Prefix Modifiers", "# Empty Suffix Modifiers"];
const TOTAL_MODIFIERS: [&str; 3] = ["# Modifiers", "# Prefix Modifiers", "# Suffix Modifiers"];

fn influence_pseudo_text(i: ItemInfluence) -> &'static str {
    match i {
        ItemInfluence::Shaper => "Has Shaper Influence",
        ItemInfluence::Crusader => "Has Crusader Influence",
        ItemInfluence::Hunter => "Has Hunter Influence",
        ItemInfluence::Elder => "Has Elder Influence",
        ItemInfluence::Redeemer => "Has Redeemer Influence",
        ItemInfluence::Warlord => "Has Warlord Influence",
    }
}

/// All stat refs this module looks up (for data validation in tests).
pub fn used_stat_refs() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.extend(CRAFTED_MODIFIERS);
    v.extend(EMPTY_MODIFIERS);
    v.extend(TOTAL_MODIFIERS);
    for i in [
        ItemInfluence::Shaper,
        ItemInfluence::Crusader,
        ItemInfluence::Hunter,
        ItemInfluence::Elder,
        ItemInfluence::Redeemer,
        ItemInfluence::Warlord,
    ] {
        v.push(influence_pseudo_text(i));
    }
    v
}

// ---------------------------------------------------------------------------
// Request JSON shapes

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusOption {
    pub option: String,
}

/// `string | { discriminator, option }`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NameQuery {
    Plain(String),
    Discriminated { discriminator: String, option: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct FilterRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<JsNum>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<JsNum>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct StatValue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<JsNum>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<JsNum>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<JsNum>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeStatFilter {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<StatValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeStatGroup {
    /// "and" | "if" | "count" | "not" | "mercenary"
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<FilterRange>,
    pub filters: Vec<TradeStatFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

impl TradeStatGroup {
    fn new(type_: &str) -> Self {
        Self { type_: type_.to_owned(), value: None, filters: Vec::new(), disabled: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeQuery {
    pub status: StatusOption,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<NameQuery>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<NameQuery>,
    pub stats: Vec<TradeStatGroup>,
    /// `type_filters`, `misc_filters`, ... built dynamically like upstream's `dot-prop` calls.
    pub filters: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeRequest {
    pub query: TradeQuery,
    pub sort: Map<String, Value>,
}

/// `dot-prop` `setProperty`: creates intermediate objects; a `None` value
/// still creates the parents (upstream sets `undefined`, which JSON drops).
fn prop_set(obj: &mut Map<String, Value>, path: &str, value: Option<Value>) {
    let mut parts: Vec<&str> = path.split('.').collect();
    let last = parts.pop().unwrap_or_default();
    let mut cur = obj;
    for p in parts {
        let entry = cur.entry(p.to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        cur = entry.as_object_mut().expect("object");
    }
    match value {
        Some(v) => {
            cur.insert(last.to_owned(), v);
        }
        None => {
            cur.remove(last);
        }
    }
}

fn s(v: &str) -> Option<Value> {
    Some(Value::String(v.to_owned()))
}
fn n(v: f64) -> Option<Value> {
    Some(json_num(v))
}

fn name_to_query(name: &str, discriminator: Option<&str>) -> NameQuery {
    match discriminator.filter(|d| !d.is_empty()) {
        None => NameQuery::Plain(name.to_owned()),
        Some(d) => NameQuery::Discriminated { discriminator: d.to_owned(), option: name.to_owned() },
    }
}

/// Upstream `createTradeRequest`.
pub fn create_trade_request(filters: &ItemFilters, stats: &[FilterOrGroup]) -> Result<TradeRequest> {
    let trade = &filters.trade;
    let status = if trade.offline {
        "any"
    } else if trade.merchant_only {
        "securable"
    } else {
        "available"
    };
    let mut sort = Map::new();
    sort.insert("price".to_owned(), Value::String("asc".to_owned()));
    let mut query = TradeQuery {
        status: StatusOption { option: status.to_owned() },
        name: None,
        type_: None,
        stats: vec![TradeStatGroup::new("and")],
        filters: Map::new(),
    };
    let qf = &mut query.filters;

    if let Some(currency) = &trade.currency {
        prop_set(qf, "trade_filters.filters.price.option", s(currency));
    }

    if trade.collapse_listings == crate::filters::CollapseListings::Api
        && (trade.offline || !trade.merchant_only || trade.collapse_merchant)
    {
        prop_set(qf, "trade_filters.filters.collapse.option", s("true"));
    }

    if let Some(listed) = &trade.listed {
        prop_set(qf, "trade_filters.filters.indexed.option", s(listed));
    }

    let mut active_search: &SearchFilter = match &filters.search_relaxed {
        Some(r) if !r.disabled => r,
        _ => &filters.search_exact,
    };
    if let Some(sub) = active_search.sub.as_deref().filter(|s| !s.disabled) {
        active_search = sub;
    }

    let disc = active_search.discriminator_trade.as_deref();
    if let Some(name) = active_search.name_trade.as_deref().filter(|v| !v.is_empty()) {
        query.name = Some(name_to_query(name, disc));
    } else if let Some(name) = active_search.name.as_deref().filter(|v| !v.is_empty()) {
        query.name = Some(name_to_query(name, disc));
    }

    if let Some(t) = active_search.base_type_trade.as_deref().filter(|v| !v.is_empty()) {
        query.type_ = Some(name_to_query(t, disc));
    } else if let Some(t) = active_search.base_type.as_deref().filter(|v| !v.is_empty()) {
        query.type_ = Some(name_to_query(t, disc));
    }

    if filters.foil.is_some_and(|f| !f.disabled) {
        prop_set(qf, "type_filters.filters.rarity.option", s("uniquefoil"));
    } else if let Some(rarity) = &filters.rarity {
        let v = if rarity.disabled { "nonunique" } else { rarity.value.as_str() };
        prop_set(qf, "type_filters.filters.rarity.option", s(v));
    }

    if let Some(category) = active_search.category {
        match category_to_trade_id(category) {
            Some(id) => prop_set(qf, "type_filters.filters.category.option", s(id)),
            None => return Err(TradeError::InvalidCategory(category.as_str().to_owned())),
        }
    }

    if let Some(c) = &filters.corrupted {
        if !c.value || c.exact == Some(true) {
            prop_set(qf, "misc_filters.filters.corrupted.option", s(if c.value { "true" } else { "false" }));
        }
        if c.value && !c.twice {
            prop_set(qf, "misc_filters.filters.twice_corrupted.option", s("false"));
        }
    }
    if filters.fractured.is_some_and(|f| !f.value) {
        prop_set(qf, "misc_filters.filters.fractured_item.option", s("false"));
    }
    if filters.imbued_gem.is_some_and(|f| f.disabled) {
        prop_set(qf, "misc_filters.filters.gem_imbued.option", s("false"));
    }
    if filters.split.is_some_and(|f| f.disabled) {
        prop_set(qf, "misc_filters.filters.split.option", s("false"));
    }
    if filters.foulborn.is_some_and(|f| !f.value) {
        prop_set(qf, "misc_filters.filters.foulborn_item.option", s("false"));
    }
    if filters.vestigial.is_some_and(|f| !f.value) {
        prop_set(qf, "misc_filters.filters.vestigial.option", s("false"));
    }
    if filters.mirrored.is_some_and(|f| f.disabled) {
        prop_set(qf, "misc_filters.filters.mirrored.option", s("false"));
    }
    if filters.not_catalysed.is_some_and(|f| !f.disabled) {
        prop_set(qf, "misc_filters.filters.quality.max", n(0.0));
    }

    if let Some(f) = filters.gem_level.filter(|f| !f.disabled) {
        prop_set(qf, "misc_filters.filters.gem_level.min", n(f.value));
    }
    if let Some(f) = filters.quality.filter(|f| !f.disabled) {
        prop_set(qf, "misc_filters.filters.quality.min", n(f.value));
    }
    if let Some(f) = filters.item_level.filter(|f| !f.disabled) {
        prop_set(qf, "misc_filters.filters.ilvl.min", n(f.value));
        if let Some(max) = f.max.filter(|m| *m != 0.0) {
            prop_set(qf, "misc_filters.filters.ilvl.max", n(max));
        }
    }
    if let Some(f) = filters.stack_size.filter(|f| !f.disabled) {
        prop_set(qf, "misc_filters.filters.stack_size.min", n(f.value));
    }
    if let Some(f) = filters.linked_sockets.filter(|f| !f.disabled) {
        prop_set(qf, "socket_filters.filters.links.min", n(f.value));
    }
    if let Some(f) = filters.white_sockets.filter(|f| !f.disabled) {
        prop_set(qf, "socket_filters.filters.sockets.w", n(f.value));
    }
    if let Some(f) = filters.map_tier.filter(|f| !f.disabled) {
        prop_set(qf, "map_filters.filters.map_tier.min", n(f.value));
        prop_set(qf, "map_filters.filters.map_tier.max", n(f.value));
    }
    if let Some(b) = filters.map_blighted {
        match b.value {
            MapBlighted::Blighted => prop_set(qf, "map_filters.filters.map_blighted.option", s("true")),
            MapBlighted::BlightRavaged => prop_set(qf, "map_filters.filters.map_uberblighted.option", s("true")),
            MapBlighted::No => {
                prop_set(qf, "map_filters.filters.map_blighted.option", s("false"));
                prop_set(qf, "map_filters.filters.map_uberblighted.option", s("false"));
            }
        }
    }
    if let Some(r) = &filters.map_completion_reward {
        prop_set(qf, "map_filters.filters.map_completion_reward.option", s(&r.name_trade));
    }
    if filters.unidentified.as_ref().is_some_and(|f| !f.disabled) {
        prop_set(qf, "misc_filters.filters.identified.option", s("false"));
    }
    if let Some(f) = filters.area_level.filter(|f| !f.disabled) {
        prop_set(qf, "map_filters.filters.area_level.min", n(f.value));
        if let Some(max) = f.max.filter(|m| *m != 0.0) {
            prop_set(qf, "map_filters.filters.area_level.max", n(max));
        }
    }
    if let Some(f) = filters.sentinel_charge.filter(|f| !f.disabled) {
        prop_set(qf, "sentinel_filters.filters.sentinel_durability.min", n(f.value));
    }

    let data = rpt_data::data();
    let pseudo_id = |ref_: &str| -> Option<String> {
        data.pseudo_stat_by_ref(ref_).and_then(|s| s.trade.ids.get("pseudo")).and_then(|ids| ids.first().cloned())
    };

    for stat in stats.iter().filter_map(FilterOrGroup::as_stat) {
        let id0 = stat.first_trade_id();
        if !id0.starts_with("item.") {
            continue;
        }

        if id0 == "item.has_empty_modifier" {
            let opt = stat.option.map(|o| o.value as usize).unwrap_or(0).min(2);
            let crafted = pseudo_id(CRAFTED_MODIFIERS[opt]).unwrap_or_default();
            let empty = pseudo_id(EMPTY_MODIFIERS[opt]).unwrap_or_default();
            let total = pseudo_id(TOTAL_MODIFIERS[0]).unwrap_or_default();
            let mk = |id: &str, min: f64, max: Option<f64>| TradeStatFilter {
                id: id.to_owned(),
                value: Some(StatValue { min: Some(JsNum(min)), max: max.map(JsNum), option: None }),
                disabled: Some(stat.disabled),
            };
            let one = Some(FilterRange { min: Some(JsNum(1.0)), max: Some(JsNum(1.0)) });
            query.stats.push(TradeStatGroup {
                type_: "count".to_owned(),
                value: one,
                disabled: Some(stat.disabled),
                filters: vec![mk(&empty, 1.0, Some(1.0)), mk(&crafted, 1.0, None)],
            });
            query.stats.push(TradeStatGroup {
                type_: "count".to_owned(),
                value: one,
                disabled: Some(stat.disabled),
                filters: vec![mk(&empty, 1.0, Some(1.0)), mk(&total, 6.0, None)],
            });
        }

        if stat.disabled {
            continue;
        }

        let roll = stat.roll.as_ref();
        let rmin = roll.and_then(|r| r.min);
        let rmax = roll.and_then(|r| r.max);
        let qf = &mut query.filters;
        let mut range = |path: &str, min: Option<f64>, max: Option<f64>| {
            prop_set(qf, &format!("{path}.min"), min.and_then(n));
            prop_set(qf, &format!("{path}.max"), max.and_then(n));
        };
        let heist_min = rmin.or(Some(1.0));
        match id0 {
            "item.base_percentile" => range("armour_filters.filters.base_defence_percentile", rmin, rmax),
            "item.memory_strands" => range("misc_filters.filters.memory_level", rmin, rmax),
            "item.armour" => range("armour_filters.filters.ar", rmin, rmax),
            "item.evasion_rating" => range("armour_filters.filters.ev", rmin, rmax),
            "item.energy_shield" => range("armour_filters.filters.es", rmin, rmax),
            "item.ward" => range("armour_filters.filters.ward", rmin, rmax),
            "item.block" => range("armour_filters.filters.block", rmin, rmax),
            "item.total_dps" => range("weapon_filters.filters.dps", rmin, rmax),
            "item.physical_dps" => range("weapon_filters.filters.pdps", rmin, rmax),
            "item.elemental_dps" => range("weapon_filters.filters.edps", rmin, rmax),
            "item.crit" => range("weapon_filters.filters.crit", rmin, rmax),
            "item.aps" => range("weapon_filters.filters.aps", rmin, rmax),
            "item.map_item_quantity" => range("map_filters.filters.map_iiq", rmin, rmax),
            "item.map_atlas_drops" => range("map_filters.filters.map_bonus", rmin, rmax),
            "item.map_item_rarity" => range("map_filters.filters.map_iir", rmin, rmax),
            "item.map_pack_size" => range("map_filters.filters.map_packsize", rmin, rmax),
            "item.heist_job_agility" => range("heist_filters.filters.heist_agility", heist_min, rmax),
            "item.heist_job_bruteforce" => range("heist_filters.filters.heist_brute_force", heist_min, rmax),
            "item.heist_job_counterthaumaturgy" => {
                range("heist_filters.filters.heist_counter_thaumaturgy", heist_min, rmax)
            }
            "item.heist_job_deception" => range("heist_filters.filters.heist_deception", heist_min, rmax),
            "item.heist_job_demolition" => range("heist_filters.filters.heist_demolition", heist_min, rmax),
            "item.heist_job_engineering" => range("heist_filters.filters.heist_engineering", heist_min, rmax),
            "item.heist_job_lockpicking" => range("heist_filters.filters.heist_lockpicking", heist_min, rmax),
            "item.heist_job_perception" => range("heist_filters.filters.heist_perception", heist_min, rmax),
            "item.heist_job_trapdisarmament" => {
                range("heist_filters.filters.heist_trap_disarmament", heist_min, rmax)
            }
            "item.heist_target_priceless" => {
                prop_set(qf, "heist_filters.filters.heist_objective_value.option", s("priceless"))
            }
            "item.heist_wings_revealed" => range("heist_filters.filters.heist_wings", rmin, rmax),
            "item.heist_wings_total" => range("heist_filters.filters.heist_max_wings", rmin, rmax),
            "item.chart_sulphur" => range("map_filters.filters.chart_sulphur", rmin, rmax),
            _ => {}
        }
    }

    // MercenaryPrimary skills are always enabled
    let stats: Vec<FilterOrGroup> = stats
        .iter()
        .map(|g| match g {
            FilterOrGroup::Stat(s) if s.tag == FilterTag::MercenaryPrimary => {
                FilterOrGroup::Stat(StatFilter { disabled: false, ..s.clone() })
            }
            other => other.clone(),
        })
        .collect();

    let mut real_stats: Vec<BareStatFilter> = stats
        .iter()
        .filter_map(FilterOrGroup::as_stat)
        .filter(|s| !is_internal_trade_id(s.first_trade_id()))
        .map(BareStatFilter::from)
        .collect();

    if let Some(veiled) = &filters.veiled {
        for stat_ref in &veiled.stat_refs {
            let Some(sog) = data.stat_by_ref_v2(stat_ref) else { continue };
            let db_stats = match sog {
                StatOrGroup::Group(g) => &g.stats[..],
                StatOrGroup::Stat(s) => std::slice::from_ref(s),
            };
            real_stats.push(BareStatFilter {
                disabled: veiled.disabled,
                trade_id: db_stats
                    .iter()
                    .filter_map(|s| s.trade.ids.get("veiled").and_then(|ids| ids.first().cloned()))
                    .collect(),
                roll: None,
                option: None,
                not: false,
            });
        }
    }

    if let Some(influences) = &filters.influences {
        for influence in influences {
            let ids = data
                .pseudo_stat_by_ref(influence_pseudo_text(influence.value))
                .and_then(|s| s.trade.ids.get("pseudo").cloned())
                .unwrap_or_default();
            real_stats.push(BareStatFilter { disabled: influence.disabled, trade_id: ids, roll: None, option: None, not: false });
        }
    }

    let mut q_and = std::mem::replace(&mut query.stats[0], TradeStatGroup::new("and"));
    let mut q_not = TradeStatGroup::new("not");
    let mut extra: Vec<TradeStatGroup> = Vec::new();

    for group in stats.iter().filter_map(FilterOrGroup::as_group) {
        match group.group {
            GroupKind::Not => extra.push(TradeStatGroup {
                type_: "not".to_owned(),
                value: None,
                disabled: Some(group.meta.disabled),
                filters: group.stats.iter().flat_map(|s| every_trade_id_to_query(&BareStatFilter::from(s))).collect(),
            }),
            GroupKind::One => extra.push(TradeStatGroup {
                type_: "count".to_owned(),
                value: Some(FilterRange { min: Some(JsNum(1.0)), max: None }),
                disabled: Some(group.meta.disabled || group.stats.iter().all(|s| s.disabled)),
                filters: group.stats.iter().flat_map(|s| every_trade_id_to_query(&BareStatFilter::from(s))).collect(),
            }),
            GroupKind::Mercenary => {
                mercenary_group_to_query(group, &mut q_and, &mut q_not, &mut extra);
            }
        }
    }

    for stat in &real_stats {
        if stat.not {
            q_not.filters.extend(every_trade_id_to_query(stat));
        } else {
            append_and_filter(stat, &mut q_and, &mut extra);
        }
    }

    query.stats[0] = q_and;
    query.stats.extend(extra);
    if !q_not.filters.is_empty() {
        query.stats.push(q_not);
    }

    Ok(TradeRequest { query, sort })
}

fn mercenary_group_to_query(
    group: &crate::filters::FilterGroup,
    q_and: &mut TradeStatGroup,
    q_not: &mut TradeStatGroup,
    all: &mut Vec<TradeStatGroup>,
) {
    let skill = &group.meta;
    let stats = &group.stats;

    if skill.tag == FilterTag::MercenaryPrimary {
        append_and_filter(&BareStatFilter { disabled: false, ..BareStatFilter::from(skill) }, q_and, all);
    } else if !skill.disabled {
        append_and_filter(&BareStatFilter::from(skill), q_and, all);
    }

    let opt_is = |s: &StatFilter, m: MercSearchMode| s.option.is_some_and(|o| o.value == m.value());
    let socketed: Vec<&StatFilter> =
        stats.iter().filter(|s| !s.not && !is_internal_trade_id(s.first_trade_id())).collect();
    let enabled_optional: Vec<&StatFilter> =
        socketed.iter().copied().filter(|s| !s.disabled && opt_is(s, MercSearchMode::Optional)).collect();
    let enabled_required: Vec<&StatFilter> =
        socketed.iter().copied().filter(|s| !s.disabled && opt_is(s, MercSearchMode::Required)).collect();

    let local_not_mode = stats.iter().any(|s| s.first_trade_id() == "item.mercenary_6link");
    let local_not_stats: Vec<&StatFilter> =
        if local_not_mode { stats.iter().filter(|s| s.not && !s.disabled).collect() } else { Vec::new() };

    let max_tier = |s: &&&StatFilter| s.mercenary.as_ref().and_then(|m| m.max_tier).unwrap_or(false);

    for stat in stats {
        if skill.disabled || enabled_required.len() == 5 {
            break;
        }

        if stat.not {
            if local_not_mode {
                continue;
            }
            // add only when enabled, so we don't clutter web UI when players
            // want to open in a browser and check the filters applied
            if !stat.disabled {
                q_not.filters.extend(every_trade_id_to_query(&BareStatFilter::from(stat)));
            }
        } else if stat.first_trade_id() == "item.mercenary_6link" {
            let force_enabled = stat.disabled && !local_not_stats.is_empty();
            if stat.disabled && !force_enabled {
                continue;
            }

            let possible_supports: Vec<Vec<rpt_data::Stat>> = stat
                .sources
                .iter()
                .map(decode_family_from_source)
                .filter(|family| {
                    let Some(first) = family.first() else { return false };
                    let canonical = first.mercenary.as_ref().and_then(|m| m.canonical.as_deref());
                    !local_not_stats
                        .iter()
                        .any(|n| Some(n.stat_ref.as_str()) == canonical || n.stat_ref == first.ref_)
                })
                .collect();
            let mut tier3_count = stat.roll.as_ref().and_then(|r| r.min).map(|m| m.clamp(0.0, 5.0)).unwrap_or(0.0);

            let max_tier_enabled_count = enabled_required.iter().filter(max_tier).count() as f64
                + if enabled_optional.len() >= 2 {
                    enabled_optional.iter().filter(max_tier).count() as f64 - 1.0
                } else {
                    0.0
                };
            if force_enabled || max_tier_enabled_count >= tier3_count {
                tier3_count = 0.0;
            }

            let pseudo = |s: &rpt_data::Stat| s.trade.ids.get("pseudo").cloned().unwrap_or_default();
            let tier = |s: &rpt_data::Stat| s.mercenary.as_ref().and_then(|m| m.tier).unwrap_or(0);

            if tier3_count < 5.0 {
                // 6-Link group
                all.push(weighted_group_to_query(
                    vec![skill.trade_id.clone()],
                    5.0,
                    possible_supports
                        .iter()
                        .map(|family| {
                            let mut fam: Vec<&rpt_data::Stat> = family.iter().collect();
                            if fam.len() >= 3 {
                                let synthetic =
                                    fam[0].mercenary.as_ref().is_some_and(|m| m.synthetic_family);
                                let min_tier = if synthetic { 3 } else { 2 };
                                fam.retain(|s| tier(s) >= min_tier);
                            }
                            fam.iter().flat_map(|s| pseudo(s)).collect()
                        })
                        .collect(),
                ));
            }

            if tier3_count > 0.0 {
                // Tier-3 Gems group
                all.push(weighted_group_to_query(
                    vec![skill.trade_id.clone()],
                    tier3_count,
                    possible_supports
                        .iter()
                        // we simply count any last gem in the family as Tier 3,
                        // users can override this with "Not" filter, e.g. to remove "Knockback (Tier: 1)"
                        .map(|family| family.last().map(pseudo).unwrap_or_default())
                        .collect(),
                ));
            }
        }
    }

    if enabled_optional.len() >= 2 {
        // Mixed N-1 & AND group
        let mut all_of = vec![skill.trade_id.clone()];
        all_of.extend(enabled_required.iter().map(|s| s.trade_id.clone()));
        all.push(weighted_group_to_query(
            all_of,
            enabled_optional.len() as f64 - 1.0,
            enabled_optional.iter().map(|s| s.trade_id.clone()).collect(),
        ));
    } else {
        // AND group. Not using `weightedGroupToQuery` for better trade site experience
        let mut filters = every_trade_id_to_query(&BareStatFilter::from(skill));
        for stat in &socketed {
            let optional = opt_is(stat, MercSearchMode::Optional);
            filters.extend(every_trade_id_to_query(&BareStatFilter {
                option: None,
                disabled: if optional { true } else { stat.disabled },
                ..BareStatFilter::from(*stat)
            }));
        }
        all.push(TradeStatGroup {
            type_: "mercenary".to_owned(),
            value: (!skill.disabled && !enabled_required.is_empty())
                .then_some(FilterRange { min: Some(JsNum(1.0 + enabled_required.len() as f64)), max: None }),
            // for a Skill without any checked Support Gems we use a simple AND filter
            disabled: Some(skill.disabled || enabled_required.is_empty()),
            filters,
        });
    }
}

/// Upstream `weightedGroupToQuery` (+ `type: 'mercenary', disabled: false`).
fn weighted_group_to_query(all_of: Vec<Vec<String>>, some_of_min: f64, some_of_ids: Vec<Vec<String>>) -> TradeStatGroup {
    // max possible surplus from `someOf` ids
    let surplus = (some_of_ids.len() as f64 - some_of_min).max(0.0);
    // the weight for all `allOf` conditions must overpower the `someOf` surplus
    let weight = surplus + 1.0;

    let total_min = some_of_min + all_of.len() as f64 * weight;
    let mut flat_ids: Vec<String> = Vec::new();
    for family in &all_of {
        for id in family {
            for _ in 0..(weight as usize) {
                flat_ids.push(id.clone());
            }
        }
    }
    flat_ids.extend(some_of_ids.into_iter().flatten());

    TradeStatGroup {
        type_: "mercenary".to_owned(),
        value: Some(FilterRange { min: Some(JsNum(total_min)), max: None }),
        filters: flat_ids.into_iter().map(|id| TradeStatFilter { id, value: None, disabled: None }).collect(),
        disabled: Some(false),
    }
}

/// Upstream `BareStatFilter` (+ `not`).
#[derive(Debug, Clone)]
struct BareStatFilter {
    trade_id: Vec<String>,
    roll: Option<FilterRoll>,
    option: Option<FilterOption>,
    disabled: bool,
    not: bool,
}

impl From<&StatFilter> for BareStatFilter {
    fn from(s: &StatFilter) -> Self {
        Self { trade_id: s.trade_id.clone(), roll: s.roll.clone(), option: s.option, disabled: s.disabled, not: s.not }
    }
}

fn append_and_filter(stat: &BareStatFilter, default_and: &mut TradeStatGroup, all: &mut Vec<TradeStatGroup>) {
    if stat.trade_id.len() == 1 {
        default_and.filters.push(trade_id_to_query(&stat.trade_id[0], stat));
    } else {
        all.push(TradeStatGroup {
            type_: "count".to_owned(),
            value: Some(FilterRange { min: Some(JsNum(1.0)), max: None }),
            disabled: Some(stat.disabled),
            filters: every_trade_id_to_query(stat),
        });
    }
}

fn every_trade_id_to_query(stat: &BareStatFilter) -> Vec<TradeStatFilter> {
    stat.trade_id.iter().map(|id| trade_id_to_query(id, stat)).collect()
}

fn get_min_max(roll: Option<&FilterRoll>, divisor: f64) -> (Option<f64>, Option<f64>) {
    let Some(roll) = roll else { return (None, None) };
    let sign = if roll.trade_invert { -1.0 } else { 1.0 };
    let a = roll.min.map(|v| v * sign / divisor);
    let b = roll.max.map(|v| v * sign / divisor);
    if !roll.trade_invert { (a, b) } else { (b, a) }
}

fn trade_id_to_query(id: &str, stat: &BareStatFilter) -> TradeStatFilter {
    let mut roll = stat.roll.as_ref();

    let div = if id.starts_with("{div_by_100}") { 100.0 } else { 1.0 };
    if id.starts_with("{empty}") || (id.starts_with("{empty_if_100}") && roll.is_some_and(|r| r.value == 100.0)) {
        roll = None;
    }
    let id = if id.starts_with('{') { id.split_once('}').map(|(_, rest)| rest).unwrap_or(id) } else { id };

    let (min, max) = get_min_max(roll, div);
    TradeStatFilter {
        id: id.to_owned(),
        value: Some(StatValue { min: min.map(JsNum), max: max.map(JsNum), option: stat.option.map(|o| JsNum(o.value)) }),
        disabled: Some(stat.disabled),
    }
}

// ---------------------------------------------------------------------------
// Responses

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    #[serde(default)]
    pub result: Vec<String>,
    #[serde(default)]
    pub total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inexact: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FetchItemProperty {
    #[serde(default)]
    pub values: Vec<(String, Value)>,
    /// 78 Corpse Level (Filled Coffin), 30 Monster Level (Harvest), 6 Quality, 5 Level
    #[serde(rename = "type", default)]
    pub type_: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchItem {
    #[serde(default)]
    pub ilvl: Option<u32>,
    #[serde(default)]
    pub stack_size: Option<f64>,
    #[serde(default)]
    pub corrupted: Option<bool>,
    #[serde(default)]
    pub properties: Option<Vec<FetchItemProperty>>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListingPrice {
    pub amount: f64,
    pub currency: String,
    #[serde(rename = "type", default)]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub indexed: String,
    #[serde(default)]
    pub price: Option<ListingPrice>,
    #[serde(default)]
    pub fee: Option<f64>,
    pub account: Account,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FetchResult {
    pub id: String,
    pub item: FetchItem,
    pub listing: Listing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingResult {
    pub id: String,
    pub item_level: Option<String>,
    pub stack_size: Option<f64>,
    pub corrupted: Option<bool>,
    pub quality: Option<String>,
    pub level: Option<String>,
    /// ISO date from the API; the UI may localize it itself.
    pub indexed: String,
    /// English short relative date (luxon `toRelative({ style: 'short' })`).
    pub relative_date: String,
    pub price_amount: f64,
    pub price_currency: String,
    pub is_mine: bool,
    pub has_note: bool,
    pub has_fee: bool,
    pub account_name: String,
    pub account_status: AccountStatus,
    pub ign: String,
}

fn fetch_result_to_pricing(result: &FetchResult, account_name: &str, now: chrono::DateTime<chrono::Utc>) -> PricingResult {
    let prop = |t: i64| {
        result
            .item
            .properties
            .as_ref()
            .and_then(|ps| ps.iter().find(|p| p.type_ == Some(t)))
            .and_then(|p| p.values.first())
            .map(|v| v.0.clone())
    };
    let listing = &result.listing;
    PricingResult {
        id: result.id.clone(),
        item_level: prop(78).or_else(|| result.item.ilvl.map(|v| v.to_string())),
        stack_size: result.item.stack_size,
        corrupted: result.item.corrupted,
        quality: prop(6),
        level: prop(5),
        indexed: listing.indexed.clone(),
        relative_date: crate::js::relative_date_short(&listing.indexed, now),
        price_amount: listing.price.as_ref().map(|p| p.amount).unwrap_or(0.0),
        price_currency: listing.price.as_ref().map(|p| p.currency.clone()).unwrap_or_else(|| "no price".to_owned()),
        has_note: result.item.note.is_some(),
        has_fee: listing.fee.is_some(),
        is_mine: listing.account.name == account_name,
        ign: listing.account.last_character_name.clone(),
        account_name: listing.account.name.clone(),
        account_status: if listing.fee.is_some() { AccountStatus::Online } else { listing.account.status() },
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupedResult {
    #[serde(flatten)]
    pub result: PricingResult,
    pub listed_times: u32,
}

/// Upstream `groupedResults` computed in `TradeListing.vue`.
pub fn group_results(results: &[PricingResult], collapse_merchant: bool) -> Vec<GroupedResult> {
    let mut out: Vec<GroupedResult> = Vec::new();
    for result in results {
        if out.is_empty() || (result.has_fee && !collapse_merchant) {
            out.push(GroupedResult { result: result.clone(), listed_times: 1 });
            continue;
        }
        let len = out.len();
        let existing = out.iter_mut().enumerate().find(|(idx, added)| {
            (added.result.account_name == result.account_name
                && added.result.price_currency == result.price_currency
                && added.result.price_amount == result.price_amount)
                || (added.result.account_name == result.account_name && (len - idx) <= 2) // last or prev
        });
        match existing {
            Some((_, existing)) => {
                if let Some(ss) = existing.result.stack_size.filter(|v| *v != 0.0) {
                    existing.result.stack_size = Some(ss + result.stack_size.unwrap_or(f64::NAN));
                } else {
                    existing.listed_times += 1;
                }
            }
            None => out.push(GroupedResult { result: result.clone(), listed_times: 1 }),
        }
    }
    out
}

/// State of a running listing search (upstream `useTradeApi`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingSearch {
    pub search_result: SearchResult,
    pub results: Vec<PricingResult>,
    pub collapse_merchant: bool,
}

impl ListingSearch {
    pub fn grouped(&self) -> Vec<GroupedResult> {
        group_results(&self.results, self.collapse_merchant)
    }
}

pub const SHOW_RESULTS: usize = 20;
const API_FETCH_LIMIT: usize = 100;
const MIN_NOT_GROUPED: usize = 7;
const MIN_GROUPED: usize = 10;

impl TradeClient {
    /// Upstream `requestTradeResultList`: POST `/api/trade/search/{league}`.
    pub async fn request_trade_result_list(&self, body: &TradeRequest, league_id: &str) -> Result<SearchResult> {
        let key = (body, league_id);
        if let Some(cached) = self.search_cache.get(&key) {
            return Ok(cached);
        }

        prevent_queue_creation(&[(1, &self.search_limits), (1, &self.fetch_limits)])?;
        self.search_limits.wait_multi().await;

        let url = format!("https://{}/api/trade/search/{}", self.trade_endpoint(), encode_uri_component(league_id));
        let req = self
            .http()
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(body)?);
        let data: SearchResult = self.trade_api(req, &self.search_limits).await?;

        let ttl = Cache::<SearchResult>::derive_ttl(
            self.search_limits.snapshot().len() + self.fetch_limits.snapshot().len(),
        );
        self.search_cache.set(&key, data.clone(), ttl);
        Ok(data)
    }

    /// Upstream `requestResults`: GET `/api/trade/fetch/{ids}?query={id}`.
    pub async fn request_results(&self, query_id: &str, result_ids: &[String]) -> Result<Vec<PricingResult>> {
        let data = match self.fetch_cache.get(&result_ids) {
            Some(d) => d,
            None => {
                self.fetch_limits.wait_multi().await;

                let url = format!(
                    "https://{}/api/trade/fetch/{}?query={}",
                    self.trade_endpoint(),
                    result_ids.join(","),
                    encode_uri_component(query_id)
                );
                #[derive(Deserialize)]
                struct FetchResponse {
                    result: Vec<Option<FetchResult>>,
                }
                let resp: FetchResponse = self.trade_api(self.http().get(url), &self.fetch_limits).await?;
                let data: Vec<FetchResult> = resp.result.into_iter().flatten().collect();
                let ttl = Cache::<Vec<FetchResult>>::derive_ttl(
                    self.search_limits.snapshot().len() + self.fetch_limits.snapshot().len(),
                );
                self.fetch_cache.set(&result_ids, data.clone(), ttl);
                data
            }
        };

        let account_name = self.config().account_name;
        let now = chrono::Utc::now();
        Ok(data.iter().map(|r| fetch_result_to_pricing(r, &account_name, now)).collect())
    }

    /// Full listing search as `TradeListing.vue` does it: search, fetch the
    /// first 20 results (two requests), then keep fetching 10 at a time while
    /// the grouped list is too short. `on_progress` is called after each
    /// fetched batch so the UI can render partial results. Drop the future to cancel.
    pub async fn search_listings(
        &self,
        filters: &ItemFilters,
        stats: &[FilterOrGroup],
        mut on_progress: impl FnMut(&ListingSearch),
    ) -> Result<ListingSearch> {
        let request = create_trade_request(filters, stats)?;
        let search_result = self.request_trade_result_list(&request, &filters.trade.league).await?;
        let mut state = ListingSearch {
            search_result: search_result.clone(),
            results: Vec::new(),
            collapse_merchant: filters.trade.collapse_merchant,
        };
        on_progress(&state);

        let ids = &search_result.result;
        // first two req are parallel, then sequential on demand
        let r1 = async {
            if !ids.is_empty() {
                self.request_results(&search_result.id, &ids[..ids.len().min(10)]).await
            } else {
                Ok(Vec::new())
            }
        };
        let r2 = async {
            if ids.len() > 10 {
                self.request_results(&search_result.id, &ids[10..ids.len().min(20)]).await
            } else {
                Ok(Vec::new())
            }
        };
        let (a, b) = tokio::join!(r1, r2);
        state.results.extend(a?);
        state.results.extend(b?);
        on_progress(&state);

        let mut fetched = 20usize;
        loop {
            let grouped = state.grouped();
            let total_grouped = grouped.len();
            let total_not_grouped = grouped.iter().filter(|r| r.listed_times <= 2).count();
            if (total_not_grouped < MIN_NOT_GROUPED || total_grouped < MIN_GROUPED)
                && fetched < ids.len()
                && fetched < API_FETCH_LIMIT
            {
                let end = (fetched + 10).min(ids.len());
                let more = self.request_results(&search_result.id, &ids[fetched..end]).await?;
                state.results.extend(more);
                fetched += 10;
                on_progress(&state);
            } else {
                break;
            }
        }
        Ok(state)
    }

    /// Trade site link (`makeTradeLink` in `TradeListing.vue` / `CheckedItem.vue`).
    pub fn make_trade_link(&self, filters: &ItemFilters, stats: &[FilterOrGroup], search_id: Option<&str>) -> Result<String> {
        make_trade_link(self.trade_endpoint(), filters, stats, search_id)
    }
}

/// `https://{endpoint}/trade/search/{league}/{id}` or `...?q={query json}`.
pub fn make_trade_link(
    endpoint: &str,
    filters: &ItemFilters,
    stats: &[FilterOrGroup],
    search_id: Option<&str>,
) -> Result<String> {
    let league = encode_uri_component(&filters.trade.league);
    Ok(match search_id {
        Some(id) => format!("https://{endpoint}/trade/search/{league}/{id}"),
        None => {
            let q = serde_json::to_string(&create_trade_request(filters, stats)?)?;
            format!("https://{endpoint}/trade/search/{league}?q={}", encode_uri_component(&q))
        }
    })
}
