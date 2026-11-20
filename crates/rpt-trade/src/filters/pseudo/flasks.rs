//! Port of upstream `filters/pseudo/flasks.ts`.

use rpt_parser::{ItemRarity, ModifierType};

use super::utils::{find_and_resolve_by_ref, stat_to_not_filter};
use crate::filters::create_stat_filters::FiltersCreationContext;

const INCR_CHARGE_RECOVERY: &str = "#% increased Charge Recovery";
const INCR_EFFECT: &str = "#% increased effect";

pub const USED_STAT_REFS: &[&str] = &[INCR_CHARGE_RECOVERY, INCR_EFFECT];

// https://github.com/SnosMe/awakened-poe-trade/issues/758
pub fn apply_flask_hybrid_mod(ctx: &mut FiltersCreationContext<'_>) {
    let applicable = ctx.item.rarity == Some(ItemRarity::Magic)
        && ctx.filters.iter().any(|f| f.stat_ref == INCR_CHARGE_RECOVERY)
        && !ctx.filters.iter().any(|f| f.stat_ref == INCR_EFFECT);
    if !applicable {
        return;
    }

    let Some(effect_stat) = find_and_resolve_by_ref(INCR_EFFECT, ModifierType::Explicit, ctx.item.category, None)
    else {
        return;
    };
    let filter = stat_to_not_filter(&effect_stat, ModifierType::Explicit, true, false);
    let insert_after = ctx.filters.iter().position(|f| f.stat_ref == INCR_CHARGE_RECOVERY);
    let at = insert_after.map(|i| i + 1).unwrap_or(0);
    ctx.filters.insert(at, filter);
}
