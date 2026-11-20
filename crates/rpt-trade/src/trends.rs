//! Port of upstream `trends/getDetailsId.ts` and the logic of `trends/PriceTrend.vue`.

use rpt_parser::{ItemCategory, ItemRarity, ParsedItem};
use serde::{Deserialize, Serialize};

use crate::filters::{SPECIAL_SUPPORT_GEM, floor_to_bracket};
use crate::js::{math_round, num_to_string};
use crate::prices::{CurrencyValue, PriceCurrency, PriceIndex};

/// poe.ninja lookup key for an item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DetailsId {
    pub ns: String,
    pub name: String,
    pub variant: Option<String>,
}

impl DetailsId {
    /// `${ns}::${name}${variant ? ` // ${variant}` : ''}` (used by related items).
    pub fn query_id(&self) -> String {
        match self.variant.as_deref().filter(|v| !v.is_empty()) {
            Some(v) => format!("{}::{} // {}", self.ns, self.name, v),
            None => format!("{}::{}", self.ns, self.name),
        }
    }
}

pub fn is_valuable_basetype(item: &ParsedItem) -> bool {
    if !matches!(item.rarity, Some(ItemRarity::Normal | ItemRarity::Magic | ItemRarity::Rare)) {
        return false;
    }
    let Some(c) = item.category else { return false };
    c.is_jewellery() || c.is_armour() || c.is_weapon() || c == ItemCategory::Quiver
}

fn ns(item: &ParsedItem) -> String {
    item.info.namespace.as_str().to_owned()
}

pub fn get_details_id(item: &ParsedItem) -> Option<DetailsId> {
    if item.category == Some(ItemCategory::Gem) {
        return Some(for_skill_gem(item));
    }
    if item.category == Some(ItemCategory::Map) {
        let unique = item.rarity == Some(ItemRarity::Unique);
        let tier = if unique { None } else { item.map_tier }.unwrap_or(0);
        let generation = if !unique {
            Some(if item.map_tier.is_some_and(|t| t != 0) { "Gen-24" } else { "Atlas" }.to_owned())
        } else {
            None
        };
        return Some(DetailsId {
            ns: ns(item),
            name: item.info.ref_name.clone(),
            variant: variant(vec![Some(format!("T{tier}")), generation]),
        });
    }
    if item.rarity == Some(ItemRarity::Unique) {
        return for_unique_item(item);
    }
    if is_valuable_basetype(item) {
        return for_basetype(item);
    }
    if item.info.ref_name == "Filled Coffin" {
        if let Some(stat) = item.stats_by_type.first() {
            let value = stat
                .sources
                .first()
                .and_then(|s| s.contributes)
                .map(|c| num_to_string(c.value))
                .unwrap_or_else(|| "undefined".to_owned());
            return Some(DetailsId {
                ns: ns(item),
                name: stat.stat.ref_.replacen('#', &value, 1),
                variant: Some(num_to_string(floor_to_bracket(
                    item.item_level.unwrap_or(0) as f64,
                    &[1.0, 70.0, 80.0, 84.0],
                ))),
            });
        }
    }
    Some(DetailsId { ns: ns(item), name: item.info.ref_name.clone(), variant: None })
}

fn for_skill_gem(item: &ParsedItem) -> DetailsId {
    let r = item.info.ref_name.as_str();
    let gem_level = item.gem_level.unwrap_or(0);
    let mut v = String::new();
    if SPECIAL_SUPPORT_GEM.contains(&r) || r == "Portal" || r == "Brand Recall" || r == "Blood and Sand" || gem_level >= 20 {
        v.push_str(&gem_level.to_string());
    } else {
        v.push('1');
    }
    if let Some(q) = item.quality.filter(|q| *q != 0.0) {
        if !SPECIAL_SUPPORT_GEM.contains(&r) && !(r == "Brand Recall" && item.is_corrupted.is_some()) {
            // Gem Q20 with up to 4xGCP (TODO: should this rule apply to corrupted gems?)
            let q = if (16.0..=20.0).contains(&q) { 20.0 } else { q };
            v.push('/');
            v.push_str(&num_to_string(q));
        }
    }
    if item.is_corrupted.is_some() && r != "Portal" {
        v.push('c');
    }
    DetailsId { ns: ns(item), name: item.info.ref_name.clone(), variant: Some(v) }
}

fn for_basetype(item: &ParsedItem) -> Option<DetailsId> {
    if item.influences.len() > 1 {
        return None;
    }
    let ilvl = item.item_level.unwrap_or(0);
    Some(DetailsId {
        ns: ns(item),
        name: item.info.ref_name.clone(),
        variant: variant(vec![
            Some(if ilvl >= 86 { "86+".to_owned() } else { ilvl.to_string() }),
            item.influences.first().map(|i| i.as_str().to_owned()),
        ]),
    })
}

fn for_unique_item(item: &ParsedItem) -> Option<DetailsId> {
    let unique_base = item.unique_base.as_ref()?;
    Some(DetailsId {
        ns: ns(item),
        name: item.info.ref_name.clone(),
        variant: variant(vec![
            get_unique_variant(item).map(str::to_owned),
            match item.category {
                Some(ItemCategory::Flask) => None,
                Some(ItemCategory::SanctumRelic) => Some("Relic".to_owned()),
                _ => Some(unique_base.ref_name.clone()),
            },
            item.sockets.and_then(|s| s.linked).filter(|l| *l != 0).map(|l| format!("{l}L")),
        ]),
    })
}

fn variant(props: Vec<Option<String>>) -> Option<String> {
    let props: Vec<String> = props.into_iter().flatten().collect();
    if props.is_empty() { None } else { Some(props.join(", ")) }
}

fn get_unique_variant(item: &ParsedItem) -> Option<&'static str> {
    let has = |r: &str| item.stats_by_type.iter().any(|m| m.stat.ref_ == r);
    let first_roll = |pred: &dyn Fn(&rpt_parser::StatCalculated) -> bool| {
        item.stats_by_type
            .iter()
            .find(|m| pred(m))
            .and_then(|m| m.sources.first())
            .and_then(|s| s.contributes)
            .map(|c| c.value)
    };

    match item.info.ref_name.as_str() {
        "Vessel of Vinktar" => {
            if has("Adds # to # Lightning Damage to Attacks during Flask effect") {
                Some("Added Attacks")
            } else if has("Adds # to # Lightning Damage to Spells during Flask effect") {
                Some("Added Spells")
            } else if has("Damage Penetrates #% Lightning Resistance during Flask effect") {
                Some("Penetration")
            } else if has("#% of Physical Damage Converted to Lightning during Flask effect") {
                Some("Conversion")
            } else {
                None
            }
        }
        "Atziri's Splendour" => {
            if has("#% increased Armour, Evasion and Energy Shield") {
                Some("Armour/Evasion/ES")
            } else if has("#% increased Evasion and Energy Shield") && has("+# to maximum Energy Shield") {
                Some("Evasion/ES")
            } else if has("#% increased Evasion and Energy Shield") && has("+# to maximum Life") {
                Some("Evasion/ES/Life")
            } else if has("#% increased Armour and Energy Shield") && has("+# to maximum Energy Shield") {
                Some("Armour/ES")
            } else if has("#% increased Armour and Energy Shield") && has("+# to maximum Life") {
                Some("Armour/ES/Life")
            } else if has("#% increased Armour and Evasion") && has("+# to maximum Life") {
                Some("Armour/Evasion")
            } else if has("+# to maximum Energy Shield") && has("#% increased Energy Shield") {
                Some("ES")
            } else if has("#% increased Evasion Rating") && has("+# to maximum Life") {
                Some("Evasion")
            } else if has("#% increased Armour") && has("+# to maximum Life") {
                Some("Armour")
            } else {
                None
            }
        }
        "Bubonic Trail" | "Lightpoacher" | "Shroud of the Lightless" | "Tombfist" => {
            match first_roll(&|m| {
                m.type_ == rpt_parser::ModifierType::Explicit && m.stat.ref_ == "Has # Abyssal Sockets"
            }) {
                Some(2.0) => Some("2 Jewels"),
                Some(1.0) => Some("1 Jewel"),
                _ => None,
            }
        }
        "Volkuur's Guidance" => {
            if has("Adds # to # Cold Damage to Spells and Attacks") {
                Some("Cold")
            } else if has("Adds # to # Fire Damage to Spells and Attacks") {
                Some("Fire")
            } else if has("Adds # to # Lightning Damage to Spells and Attacks") {
                Some("Lightning")
            } else {
                None
            }
        }
        "Yriel's Fostering" => {
            if has("Projectiles from Attacks have #% chance to Maim on Hit while\nyou have a Bestial Minion") {
                Some("Maim")
            } else if has("Projectiles from Attacks have #% chance to Poison on Hit while\nyou have a Bestial Minion") {
                Some("Poison")
            } else if has(
                "Projectiles from Attacks have #% chance to inflict Bleeding on Hit while\nyou have a Bestial Minion",
            ) {
                Some("Bleeding")
            } else {
                None
            }
        }
        "Doryani's Invitation" => {
            if has("#% increased Global Physical Damage") {
                Some("Physical")
            } else if has("#% increased Fire Damage") {
                Some("Fire")
            } else if has("#% increased Cold Damage") {
                Some("Cold")
            } else if has("#% increased Lightning Damage") {
                Some("Lightning")
            } else {
                None
            }
        }
        "Impresence" => {
            if has("Adds # to # Cold Damage") {
                Some("Cold")
            } else if has("Adds # to # Chaos Damage") {
                Some("Chaos")
            } else if has("Adds # to # Fire Damage") {
                Some("Fire")
            } else if has("Adds # to # Lightning Damage") {
                Some("Lightning")
            } else if has("Adds # to # Physical Damage") {
                Some("Physical")
            } else {
                None
            }
        }
        "Voices" => match first_roll(&|m| m.stat.ref_ == "Adds # Small Passive Skills which grant nothing") {
            Some(7.0) => Some("7 passives"),
            Some(5.0) => Some("5 passives"),
            Some(3.0) => Some("3 passives"),
            _ => None,
        },
        "The First Crest" => {
            if has("Aureus Coins are converted to Experience upon defeating the Herald of the Scourge") {
                Some("Experience")
            } else if has("Aureus Coins are converted to Relics upon defeating the Herald of the Scourge") {
                Some("Relics")
            } else if has("Aureus Coins are converted to Tainted Currency upon defeating the Herald of the Scourge") {
                Some("Tainted Currency")
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// PriceTrend.vue

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Forecast {
    Const,
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendGraph {
    pub points: Vec<f64>,
    pub draw_min: f64,
    pub draw_max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrendChange {
    pub graph: TrendGraph,
    pub forecast: Forecast,
    /// e.g. "12 %" (thin space)
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceTrend {
    pub price: CurrencyValue,
    pub change: Option<TrendChange>,
    pub url: String,
}

/// `trend` computed in `PriceTrend.vue`.
pub fn price_trend(item: &ParsedItem, prices: &PriceIndex) -> Option<PriceTrend> {
    let trend = prices.find_price_by_query(&get_details_id(item)?.into())?;
    let price = if item.info.ref_name == "Divine Orb" {
        CurrencyValue { min: trend.chaos, max: trend.chaos, currency: PriceCurrency::Chaos }
    } else {
        prices.auto_currency(trend.chaos)
    };
    Some(PriceTrend { price, change: delta_from_graph(&trend.graph), url: trend.url })
}

/// `showFallback` in `PriceTrend.vue`.
pub fn show_trend_fallback(item: &ParsedItem) -> bool {
    item.info.craftable.is_none() && item.info.ref_name != "Mercenary Warrant" && item.info.ref_name != "Viscera"
}

pub fn delta_from_graph(graph_points: &[Option<f64>]) -> Option<TrendChange> {
    let points: Vec<f64> = graph_points.iter().flatten().copied().collect();
    if points.len() < 2 {
        return None;
    }

    let mut forecast = Forecast::Const;
    if points.len() == 7 {
        if points.iter().filter(|p| **p > 0.0).count() >= 4 || points[4..].iter().all(|p| *p > 0.0) {
            forecast = Forecast::Up;
        } else if points.iter().filter(|p| **p < 0.0).count() >= 4 || points[4..].iter().all(|p| *p < 0.0) {
            forecast = Forecast::Down;
        }
    }

    let mean = points.iter().sum::<f64>() / points.len() as f64;
    let change_val =
        (points.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (points.len() as f64 - 1.0)).sqrt();
    let pad = if change_val == 0.0 || change_val.is_nan() { 1.0 } else { change_val };
    let min = points.iter().copied().fold(f64::INFINITY, f64::min);
    let max = points.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    Some(TrendChange {
        graph: TrendGraph { points, draw_min: min - pad, draw_max: max + pad },
        forecast,
        text: format!("{}\u{2009}%", num_to_string(math_round(change_val * 2.0))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph() {
        let c = delta_from_graph(&[Some(1.0), Some(2.0), None, Some(3.0), Some(4.0), Some(5.0), Some(6.0), Some(7.0)])
            .unwrap();
        assert_eq!(c.forecast, Forecast::Up);
        assert_eq!(c.graph.points.len(), 7);
        assert_eq!(c.text, "4\u{2009}%");
        assert!(delta_from_graph(&[Some(1.0)]).is_none());
    }
}
