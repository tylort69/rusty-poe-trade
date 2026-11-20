//! Port of upstream `filters/pseudo/heist.ts`.

use rpt_parser::{HeistJob, ItemCategory, ItemRarity, ModifierType, StatRoll};

use super::item_property::{PropOpts, prop_to_filter};
use super::utils::stat_to_not_filter;
use crate::filters::FilterTag;
use crate::filters::create_stat_filters::FiltersCreationContext;

const ENCHANT_MODS: &str = "# Enchant Modifiers";

pub const USED_STAT_REFS: &[&str] = &[ENCHANT_MODS];

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub fn apply_heist_rules(ctx: &mut FiltersCreationContext<'_>) {
    apply_contract_rules(ctx);
    apply_blueprint_rules(ctx);
}

pub fn heist_job_internal_id(job: HeistJob) -> &'static str {
    match job {
        HeistJob::Lockpicking => "item.heist_job_lockpicking",
        HeistJob::BruteForce => "item.heist_job_bruteforce",
        HeistJob::Perception => "item.heist_job_perception",
        HeistJob::Demolition => "item.heist_job_demolition",
        HeistJob::CounterThaumaturgy => "item.heist_job_counterthaumaturgy",
        HeistJob::TrapDisarmament => "item.heist_job_trapdisarmament",
        HeistJob::Agility => "item.heist_job_agility",
        HeistJob::Deception => "item.heist_job_deception",
        HeistJob::Engineering => "item.heist_job_engineering",
    }
}

fn apply_contract_rules(ctx: &mut FiltersCreationContext<'_>) {
    if ctx.item.category != Some(ItemCategory::HeistContract) || ctx.item.rarity == Some(ItemRarity::Unique) {
        return;
    }
    let item = ctx.item;
    let Some(contract) = &item.heist_contract else { return };

    if let (Some(job), Some(level)) = (contract.required_job, contract.job_level.filter(|l| *l != 0)) {
        let internal_id = heist_job_internal_id(job);
        ctx.filters.push(prop_to_filter(
            PropOpts {
                ref_: internal_id,
                trade_id: internal_id,
                roll: Some(StatRoll { min: 0.0, max: MAX_SAFE_INTEGER, value: level as f64 }),
                sources: Vec::new(),
                dp: None,
                disabled: Some(false),
                hidden: None,
            },
            item,
            0.0,
        ));
    }

    if contract.target_value_priceless {
        ctx.filters.push(prop_to_filter(
            PropOpts {
                ref_: "Heist Target: Priceless",
                trade_id: "item.heist_target_priceless",
                roll: None,
                sources: Vec::new(),
                dp: None,
                disabled: Some(false),
                hidden: None,
            },
            item,
            ctx.search_in_range,
        ));
    }
}

fn apply_blueprint_rules(ctx: &mut FiltersCreationContext<'_>) {
    if ctx.item.category != Some(ItemCategory::HeistBlueprint) || ctx.item.rarity == Some(ItemRarity::Unique) {
        return;
    }

    if !ctx.filters.iter().any(|f| f.tag == FilterTag::ENCHANT) {
        let data = rpt_data::data();
        if let Some(stat) = data.pseudo_stat_by_ref(ENCHANT_MODS) {
            ctx.filters.push(stat_to_not_filter(stat, ModifierType::Pseudo, false, false));
        }
    }

    let item = ctx.item;
    let bp = item.heist_blueprint.clone().unwrap_or_default();
    ctx.filters.push(prop_to_filter(
        PropOpts {
            ref_: "Wings Revealed: #",
            trade_id: "item.heist_wings_revealed",
            roll: Some(StatRoll { min: 0.0, max: MAX_SAFE_INTEGER, value: bp.wings_revealed.unwrap_or(0) as f64 }),
            sources: Vec::new(),
            dp: None,
            disabled: Some(false),
            hidden: None,
        },
        item,
        0.0,
    ));
    ctx.filters.push(prop_to_filter(
        PropOpts {
            ref_: "Total Wings: #",
            trade_id: "item.heist_wings_total",
            roll: Some(StatRoll { min: 0.0, max: MAX_SAFE_INTEGER, value: bp.wings_total.unwrap_or(0) as f64 }),
            sources: Vec::new(),
            dp: None,
            disabled: Some(false),
            hidden: None,
        },
        item,
        0.0,
    ));
}
