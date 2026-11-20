//! Non-view logic of upstream `CheckedItem.vue` (price check session).

use rpt_parser::{ItemCategory, ItemRarity, ParsedItem};

use crate::client::TradeConfig;
use crate::filters::create_presets::PRESET_BASE_ITEM;
use crate::filters::{CreatePresetsOptions, Presets, create_presets};
use crate::trade::{TradeApi, api_to_satisfy_search, category_to_trade_id};

/// Builds presets for a newly checked item (watch on `props.item`).
///
/// `prev` is the previously checked item and the trade currency selected for it:
/// the currency is kept when the same item is checked again, otherwise the
/// widget's `defaultCurrency` is used.
pub fn presets_for_item(
    item: &ParsedItem,
    prev: Option<(&ParsedItem, Option<String>)>,
    config: &TradeConfig,
    league_id: &str,
) -> Presets {
    let w = &config.price_check;
    let currency = match prev {
        Some((prev_item, prev_currency))
            if item.info.namespace == prev_item.info.namespace && item.info.ref_name == prev_item.info.ref_name =>
        {
            prev_currency
        }
        _ => w.default_currency.clone(),
    };
    create_presets(
        item,
        &CreatePresetsOptions {
            league: league_id.to_owned(),
            merchant_only: w.merchant_only,
            collapse_listings: w.collapse_listings,
            activate_stock_filter: w.activate_stock_filter,
            search_stat_range: w.search_stat_range,
            use_en: config.use_intl_site,
            currency,
        },
    )
}

/// Whether to start searching immediately (`doSearch` initial value).
pub fn initial_do_search(item: &ParsedItem, presets: &Presets, advanced_check: bool, config: &TradeConfig) -> bool {
    let w = &config.price_check;
    if (!advanced_check && !w.smart_initial_search) || (advanced_check && !w.locked_initial_search) {
        return false;
    }
    item.rarity == Some(ItemRarity::Unique)
        || presets.active == crate::filters::create_presets::PRESET_BULK
        || item.map_completion_reward.is_some()
        || matches!(
            item.category,
            Some(
                ItemCategory::HeistContract
                    | ItemCategory::HeistBlueprint
                    | ItemCategory::SanctumRelic
                    | ItemCategory::Charm
                    | ItemCategory::Idol
            )
        )
        || item.info.ref_name == "Viscera"
        || (item.category.and_then(category_to_trade_id).is_none() && item.info.ref_name != "Mercenary Warrant")
        || item.is_unidentified
        || item.is_veiled
}

/// Which API (listing or bulk exchange) to use for the active preset.
pub fn trade_api_for(item: &ParsedItem, presets: &Presets) -> TradeApi {
    match presets.active_preset() {
        Some(p) => api_to_satisfy_search(item, &p.stats, &p.filters),
        None => TradeApi::Trade,
    }
}

/// `showPredictedPrice` in `CheckedItem.vue`.
pub fn show_predicted_price(item: &ParsedItem, presets: &Presets, config: &TradeConfig, league_is_popular: bool) -> bool {
    if !config.price_check.request_price_prediction || config.language != "en" || !league_is_popular {
        return false;
    }
    if presets.active == PRESET_BASE_ITEM {
        return false;
    }
    item.rarity == Some(ItemRarity::Rare)
        && !matches!(
            item.category,
            Some(
                ItemCategory::Map
                    | ItemCategory::CapturedBeast
                    | ItemCategory::HeistContract
                    | ItemCategory::HeistBlueprint
                    | ItemCategory::Chart
                    | ItemCategory::Invitation
            )
        )
        && item.info.ref_name != "Expedition Logbook"
        && !item.is_unidentified
}

/// `show` in `CheckedItem.vue` (hidden while an unidentified unique is unresolved).
pub fn show_checked_item(item: &ParsedItem) -> bool {
    !(item.rarity == Some(ItemRarity::Unique) && item.is_unidentified && item.info.unique.is_none())
}

/// `showComplexityHint` in `CheckedItem.vue`.
pub fn show_complexity_hint(item: &ParsedItem, do_search: bool, config: &TradeConfig) -> bool {
    !config.price_check.builtin_browser && !do_search && item.info.ref_name == "Mercenary Warrant"
}
