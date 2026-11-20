//! Logic from upstream `related-items/RelatedItems.vue`,
//! `expected-value/StackValue.vue` and `unidentified-resolver/UnidentifiedResolver.vue`.

use rpt_data::{BaseType, Namespace};
use rpt_parser::{ItemRarity, ParsedItem};
use serde::{Deserialize, Serialize};

use crate::filters::ItemFilters;
use crate::prices::{CurrencyValue, DbQuery, PriceCurrency, PriceIndex, display_rounding};
use crate::trends::get_details_id;

// ---------------------------------------------------------------------------
// RelatedItems.vue

fn parse_ns(ns: &str) -> Option<Namespace> {
    serde_json::from_value(serde_json::Value::String(ns.to_owned())).ok()
}

fn split_query_id(query_id: &str) -> (&str, &str, Option<&str>) {
    let (ns, encoded) = query_id.split_once("::").unwrap_or((query_id, ""));
    match encoded.split_once(" // ") {
        Some((name, variant)) => (ns, name, Some(variant)),
        None => (ns, encoded, None),
    }
}

fn find_item_by_query_id(query_id: &str) -> Option<BaseType> {
    let (ns, name, variant) = split_query_id(query_id);
    let data = rpt_data::data();
    let mut found: Vec<&BaseType> = data.item_by_ref(parse_ns(ns)?, name)?;
    if ns == "UNIQUE" {
        let filtered: Vec<&BaseType> = found
            .iter()
            .copied()
            .filter(|u| u.unique.as_ref().map(|x| x.base.as_str()) == variant)
            .collect();
        if !filtered.is_empty() {
            found = filtered;
        }
    }
    // return any first item
    found.first().map(|b| (*b).clone())
}

fn find_price_by_query_id(query_id: &str, prices: &PriceIndex) -> Option<CurrencyValue> {
    let (ns, name, variant) = split_query_id(query_id);
    let entry = prices.find_price_by_query(&DbQuery {
        ns: ns.to_owned(),
        name: name.to_owned(),
        variant: variant.map(str::to_owned),
    })?;
    Some(prices.auto_currency(entry.chaos))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelatedItem {
    pub name: String,
    pub icon: String,
    pub price: Option<CurrencyValue>,
    #[serde(default)]
    pub highlight: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelatedItems {
    pub related: Vec<RelatedItem>,
    pub items: Vec<RelatedItem>,
}

/// Upstream `getItemPrices`. `Ok(None)` = item has no drop table entry;
/// `Err` mirrors upstream's `{ error }` result.
pub fn get_item_prices(query_id: &str, prices: &PriceIndex) -> Result<Option<RelatedItems>, String> {
    let data = rpt_data::data();
    let Some(drop_entry) = data.item_drop.iter().find(|e| e.query.iter().any(|q| q == query_id)) else {
        return Ok(None);
    };

    let mut out = RelatedItems { related: Vec::new(), items: Vec::new() };
    for item_id in &drop_entry.query {
        let db_item = find_item_by_query_id(item_id).ok_or_else(|| format!("Can't find \"{item_id}\"."))?;
        out.related.push(RelatedItem {
            icon: db_item.icon,
            name: db_item.name,
            price: find_price_by_query_id(item_id, prices),
            highlight: item_id == query_id,
        });
    }
    for item_id in &drop_entry.items {
        let db_item = find_item_by_query_id(item_id).ok_or_else(|| format!("Can't find \"{item_id}\"."))?;
        out.items.push(RelatedItem {
            icon: db_item.icon,
            name: db_item.name,
            price: find_price_by_query_id(item_id, prices),
            highlight: false,
        });
    }
    Ok(Some(out))
}

/// `result` computed in `RelatedItems.vue`.
pub fn related_items(item: &ParsedItem, prices: &PriceIndex) -> Result<Option<RelatedItems>, String> {
    let Some(id) = get_details_id(item) else { return Ok(None) };
    get_item_prices(&id.query_id(), prices)
}

// ---------------------------------------------------------------------------
// StackValue.vue

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackAmountPrice {
    pub amount: f64,
    /// e.g. "12 chaos" / "1 . 5 div" (with hair spaces, upstream `displayRounding`)
    pub price: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackValue {
    pub have: StackAmountPrice,
    pub one_stack: Option<StackAmountPrice>,
}

/// `show` + `value` computed in `StackValue.vue`.
pub fn stack_value(item: &ParsedItem, filters: &ItemFilters, prices: &PriceIndex) -> Option<StackValue> {
    let stack = filters.stack_size?;
    let one = prices.find_price_by_query(&get_details_id(item)?.into())?;

    let price_for = |n: f64| {
        let price = if item.info.ref_name == "Divine Orb" {
            CurrencyValue { min: n * one.chaos, max: n * one.chaos, currency: PriceCurrency::Chaos }
        } else {
            prices.auto_currency(n * one.chaos)
        };
        let cur = match price.currency {
            PriceCurrency::Chaos => "chaos",
            PriceCurrency::Div => "div",
        };
        format!("{} {}", display_rounding(price.min, false), cur)
    };

    Some(StackValue {
        have: StackAmountPrice { amount: stack.value, price: price_for(stack.value) },
        one_stack: item.stack_size.map(|s| StackAmountPrice { amount: s.max, price: price_for(s.max) }),
    })
}

// ---------------------------------------------------------------------------
// UnidentifiedResolver.vue

/// `show` computed in `UnidentifiedResolver.vue`.
pub fn needs_unidentified_resolver(item: &ParsedItem) -> bool {
    item.rarity == Some(ItemRarity::Unique) && item.is_unidentified && item.info.unique.is_none()
}

/// `identifiedVariants`: uniques whose base is this item's base type.
/// If exactly one is returned the UI should auto-select it.
pub fn identified_variants(item: &ParsedItem) -> Vec<BaseType> {
    let base_type = &item.info.ref_name;
    let needle = serde_json::to_string(base_type).unwrap_or_default();
    let data = rpt_data::data();
    let mut possible: Vec<BaseType> = Vec::new();
    for m in data.items_iter(&needle, &[]) {
        if m.namespace == Namespace::Unique && m.unique.as_ref().is_some_and(|u| u.base == *base_type) {
            // TODO currently ignoring variants
            if !possible.iter().any(|u| u.ref_name == m.ref_name) {
                possible.push(m.clone());
            }
        }
    }
    possible
}

/// Replaces an unidentified unique with the chosen variant (`select` in
/// `UnidentifiedResolver.vue`; delegates to `rpt_parser::make_identified_unique`).
pub fn identify_unique(info: &BaseType, item: &ParsedItem) -> ParsedItem {
    rpt_parser::make_identified_unique(info, item)
}
