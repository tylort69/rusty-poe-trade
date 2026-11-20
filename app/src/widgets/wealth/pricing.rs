//! Stash item JSON → poe.ninja lookup key, plus the valuation of whole tabs.
//!
//! Mirrors `rpt_trade::trends::get_details_id` (which works on a parsed
//! clipboard item) for the GGG stash JSON the account API returns.

use std::collections::HashMap;

use rpt_data::{GameData, Namespace};
use rpt_stash::model::{Item, StashTab};
use rpt_trade::filters::SPECIAL_SUPPORT_GEM;
use rpt_trade::prices::DbQuery;
use serde_json::Value;

/// Current atlas generation tag poe.ninja uses for regular maps (same as the price check).
const MAP_GENERATION: &str = "Gen-24";

// GGG `frameType`
const FRAME_NORMAL: u8 = 0;
const FRAME_MAGIC: u8 = 1;
const FRAME_RARE: u8 = 2;
const FRAME_UNIQUE: u8 = 3;
const FRAME_GEM: u8 = 4;
const FRAME_CURRENCY: u8 = 5;
const FRAME_DIV_CARD: u8 = 6;
const FRAME_FOIL: u8 = 9;
const FRAME_SUPPORTER_FOIL: u8 = 10;

/// Unique-item facts from `rpt-data` (English reference names, as the stash API uses).
#[derive(Default)]
pub struct UniqueDb {
    /// unique name → disenchant value
    disenchant: HashMap<String, f64>,
    /// base type → unique names on that base
    by_base: HashMap<String, Vec<String>>,
}

impl UniqueDb {
    pub fn from_data(data: &GameData) -> Self {
        let mut db = Self::default();
        for b in data.all_items().iter().filter(|b| b.namespace == Namespace::Unique) {
            let Some(u) = &b.unique else { continue };
            if let Some(d) = u.disenchant_value.filter(|d| *d > 0.0) {
                db.disenchant.entry(b.ref_name.clone()).or_insert(d);
            }
            let names = db.by_base.entry(u.base.clone()).or_default();
            if !names.contains(&b.ref_name) {
                names.push(b.ref_name.clone());
            }
        }
        db
    }

    /// The only unique on `base`, if exactly one exists (for unidentified uniques).
    fn single_unique_on(&self, base: &str) -> Option<&str> {
        match self.by_base.get(base).map(Vec::as_slice) {
            Some([one]) => Some(one.as_str()),
            _ => None,
        }
    }

    pub fn disenchant_value(&self, name: &str) -> Option<f64> {
        self.disenchant.get(name).copied()
    }
}

/// Thaumaturgic dust for a unique, same formula as the price check / item info
/// (`rpt_parser` `calc_disenchant_dust`), with no influences or corruption implicits.
pub fn dust_value(disenchant_value: f64, ilvl: f64, quality: f64) -> f64 {
    let factors_multi = (quality * 2.0 + 100.0) / 100.0;
    let term1 = 50.0;
    let term2 = 2.0 * (ilvl.clamp(46.0, 68.0) - 46.0);
    let term3 = (3.0 * (ilvl.clamp(46.0, 68.0) - 46.0) / 11.0).floor();
    let term4 = 25.0 * (ilvl.clamp(68.0, 84.0) - 68.0);
    (disenchant_value * 5.0 * (term1 + term2 + term3 + term4) * factors_multi).floor()
}

/// Wealthy Exile's assumption: every unique is disenchanted at ilvl 84 with 20% quality.
pub fn dust_assumed(disenchant_value: f64) -> f64 {
    dust_value(disenchant_value, 84.0, 20.0)
}

/// Remove GGG text markup like `<<set:MS>><<set:M>><<set:S>>`.
pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<<") {
        out.push_str(&rest[..i]);
        match rest[i..].find(">>") {
            Some(j) => rest = &rest[i + j + 2..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.trim().to_owned()
}

/// First value of a named item property (`properties[].values[0][0]`).
fn prop<'a>(item: &'a Item, name: &str) -> Option<&'a str> {
    item.extra.get("properties")?.as_array()?.iter().find(|p| p.get("name").and_then(Value::as_str) == Some(name))?.get("values")?.get(0)?.get(0)?.as_str()
}

/// Leading number of a property string: "20 (Max)" → 20, "+23%" → 23.
fn leading_number(s: &str) -> Option<f64> {
    let t: String = s.trim_start_matches('+').chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    t.parse().ok()
}

/// Largest socket link group (0 without sockets).
pub fn max_links(item: &Item) -> u32 {
    let Some(sockets) = item.extra.get("sockets").and_then(Value::as_array) else { return 0 };
    let mut groups: HashMap<u64, u32> = HashMap::new();
    for s in sockets {
        *groups.entry(s.get("group").and_then(Value::as_u64).unwrap_or(0)).or_default() += 1;
    }
    groups.into_values().max().unwrap_or(0)
}

#[cfg(test)]
fn display_name(item: &Item) -> String {
    let name = strip_markup(&item.name);
    let type_line = strip_markup(&item.type_line);
    if name.is_empty() { type_line } else { name }
}

fn base_of(item: &Item) -> String {
    let b = strip_markup(&item.base_type);
    if b.is_empty() { strip_markup(&item.type_line) } else { b }
}

fn is_map(item: &Item) -> bool {
    prop(item, "Map Tier").is_some()
}

fn is_unique(item: &Item) -> bool {
    matches!(item.frame_type, FRAME_UNIQUE | FRAME_FOIL | FRAME_SUPPORTER_FOIL)
}

/// Name of the unique (resolved through the base for unidentified singles).
pub fn unique_name(item: &Item, uniques: &UniqueDb) -> Option<String> {
    if !is_unique(item) {
        return None;
    }
    let name = strip_markup(&item.name);
    if !name.is_empty() {
        return Some(name);
    }
    uniques.single_unique_on(&base_of(item)).map(str::to_owned)
}

fn variant(parts: Vec<Option<String>>) -> Option<String> {
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    if parts.is_empty() { None } else { Some(parts.join(", ")) }
}

/// poe.ninja lookup for a stash item, `None` for things poe.ninja doesn't price
/// (rares, plain gear, quest items, unidentified uniques with several candidates).
pub fn item_query(item: &Item, uniques: &UniqueDb) -> Option<DbQuery> {
    let q = |ns: &str, name: String, variant: Option<String>| Some(DbQuery { ns: ns.to_owned(), name, variant });

    if is_map(item) {
        let tier = prop(item, "Map Tier").and_then(leading_number).unwrap_or(0.0) as u32;
        if is_unique(item) {
            // Upstream prices unique maps as "T0".
            return q("UNIQUE", unique_name(item, uniques)?, Some("T0".into()));
        }
        let generation = if tier != 0 { MAP_GENERATION } else { "Atlas" };
        return q("ITEM", base_of(item), variant(vec![Some(format!("T{tier}")), Some(generation.into())]));
    }

    match item.frame_type {
        FRAME_GEM => Some(gem_query(item)),
        FRAME_DIV_CARD => q("DIVINATION_CARD", base_of(item), None),
        FRAME_CURRENCY => q("ITEM", base_of(item), None),
        _ if is_unique(item) => {
            let name = unique_name(item, uniques)?;
            let base = base_of(item);
            let base_part = if base.contains(" Flask") {
                None
            } else if base.ends_with(" Relic") {
                Some("Relic".to_owned())
            } else {
                Some(base)
            };
            let links = max_links(item);
            q("UNIQUE", name, variant(vec![base_part, (links >= 5).then(|| format!("{links}L"))]))
        }
        // Fragments, scarabs, splinters, … are plain normal items; magic/normal gear won't match.
        FRAME_NORMAL | FRAME_MAGIC => q("ITEM", base_of(item), None),
        FRAME_RARE => None,
        _ => None,
    }
}

fn gem_query(item: &Item) -> DbQuery {
    let mut name = strip_markup(&item.type_line);
    for prefix in ["Superior ", "Anomalous ", "Divergent ", "Phantasmal "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            name = rest.to_owned();
        }
    }
    let r = name.as_str();
    let level = prop(item, "Level").and_then(leading_number).unwrap_or(1.0) as u32;
    let quality = prop(item, "Quality").and_then(leading_number).unwrap_or(0.0);
    let special = SPECIAL_SUPPORT_GEM.contains(&r);

    let mut v = if special || matches!(r, "Portal" | "Brand Recall" | "Blood and Sand") || level >= 20 {
        level.to_string()
    } else {
        "1".to_owned()
    };
    if quality != 0.0 && !special && !(r == "Brand Recall" && item.corrupted) {
        let q = if (16.0..=20.0).contains(&quality) { 20.0 } else { quality };
        v.push('/');
        v.push_str(&rpt_trade::js::num_to_string(q));
    }
    if item.corrupted && r != "Portal" {
        v.push('c');
    }
    DbQuery { ns: "GEM".into(), name, variant: Some(v) }
}

/// Can be listed in bulk / as a stack (currency, cards, fragments, …).
pub fn is_stackable(item: &Item) -> bool {
    item.stack_size.is_some() || matches!(item.frame_type, FRAME_CURRENCY | FRAME_DIV_CARD)
}

// ---------------------------------------------------------------------------
// Valuation

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Stackable,
    Unique,
    Gem,
    Map,
    Other,
}

/// One priced line in the top-items list (identical items across tabs merged).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub key: String,
    pub name: String,
    pub detail: Option<String>,
    pub kind: RowKind,
    pub qty: u32,
    pub unit_chaos: f64,
    pub total_chaos: f64,
    pub url: Option<String>,
}

/// One unique item for the dust calculator.
#[derive(Debug, Clone, PartialEq)]
pub struct UniqueRow {
    pub name: String,
    pub base: String,
    pub tab: String,
    pub chaos: Option<f64>,
    pub dust: f64,
}

impl UniqueRow {
    pub fn dust_per_chaos(&self) -> Option<f64> {
        self.chaos.filter(|c| *c > 0.0).map(|c| self.dust / c)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TabValue {
    pub name: String,
    pub chaos: f64,
    pub items: usize,
    pub unpriced: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Valuation {
    pub total_chaos: f64,
    /// Divine Orb price in chaos at valuation time.
    pub divine_chaos: Option<f64>,
    pub tabs: Vec<TabValue>,
    /// Sorted by total value, highest first.
    pub rows: Vec<Row>,
    pub uniques: Vec<UniqueRow>,
    /// Item stacks without a poe.ninja price.
    pub unpriced: usize,
}

/// A price lookup result: chaos value and poe.ninja page.
pub struct Found {
    pub chaos: f64,
    pub url: Option<String>,
}

/// Price every item in `tabs`. `lookup` is usually `PriceIndex::find_price_by_query`.
pub fn value_tabs(tabs: &[StashTab], uniques: &UniqueDb, divine_chaos: Option<f64>, lookup: impl Fn(&DbQuery) -> Option<Found>) -> Valuation {
    let mut cache: HashMap<DbQuery, Option<(f64, Option<String>)>> = HashMap::new();
    let mut price = |q: &DbQuery| -> Option<(f64, Option<String>)> {
        if q.ns == "ITEM" && q.name == "Chaos Orb" && q.variant.is_none() {
            return Some((1.0, None));
        }
        cache.entry(q.clone()).or_insert_with(|| lookup(q).map(|f| (f.chaos, f.url))).clone()
    };

    let mut v = Valuation { divine_chaos, ..Default::default() };
    let mut rows: HashMap<String, Row> = HashMap::new();
    for tab in tabs {
        let mut tv = TabValue { name: tab.info.name.clone(), chaos: 0.0, items: 0, unpriced: 0 };
        for item in &tab.items {
            tv.items += 1;
            let query = item_query(item, uniques);
            let found = query.as_ref().and_then(&mut price);
            let qty = item.quantity().max(1);

            if is_unique(item) && !is_map(item) {
                if let Some(name) = unique_name(item, uniques) {
                    if let Some(d) = uniques.disenchant_value(&name) {
                        v.uniques.push(UniqueRow {
                            name,
                            base: base_of(item),
                            tab: tab.info.name.clone(),
                            chaos: found.as_ref().map(|f| f.0),
                            dust: dust_assumed(d),
                        });
                    }
                }
            }

            let (Some(q), Some((unit, url))) = (query, found) else {
                tv.unpriced += 1;
                v.unpriced += 1;
                continue;
            };
            let total = unit * qty as f64;
            tv.chaos += total;
            let kind = if is_map(item) {
                RowKind::Map
            } else if is_unique(item) {
                RowKind::Unique
            } else if item.frame_type == FRAME_GEM {
                RowKind::Gem
            } else if is_stackable(item) {
                RowKind::Stackable
            } else {
                RowKind::Other
            };
            let key = query_key(&q);
            let row = rows.entry(key.clone()).or_insert_with(|| Row {
                key,
                name: q.name.clone(),
                detail: q.variant.clone(),
                kind,
                qty: 0,
                unit_chaos: unit,
                total_chaos: 0.0,
                url,
            });
            row.qty += qty;
            row.total_chaos += total;
        }
        v.total_chaos += tv.chaos;
        v.tabs.push(tv);
    }
    v.rows = rows.into_values().collect();
    v.rows.sort_by(|a, b| b.total_chaos.total_cmp(&a.total_chaos).then_with(|| a.name.cmp(&b.name)));
    v
}

pub fn query_key(q: &DbQuery) -> String {
    match &q.variant {
        Some(v) => format!("{}::{} // {}", q.ns, q.name, v),
        None => format!("{}::{}", q.ns, q.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rpt_stash::model::StashTabInfo;

    const FIXTURE: &str = include_str!("fixtures/items.json");

    fn items() -> Vec<Item> {
        serde_json::from_str(FIXTURE).unwrap()
    }

    fn db() -> UniqueDb {
        rpt_data::init("en").unwrap();
        UniqueDb::from_data(&rpt_data::data())
    }

    fn q(ns: &str, name: &str, variant: Option<&str>) -> Option<DbQuery> {
        Some(DbQuery { ns: ns.into(), name: name.into(), variant: variant.map(Into::into) })
    }

    #[test]
    fn stash_items_map_to_ninja_queries() {
        let db = db();
        let got: Vec<(String, Option<DbQuery>)> = items().iter().map(|i| (display_name(i), item_query(i, &db))).collect();
        let expect = [
            ("Divine Orb", q("ITEM", "Divine Orb", None)),
            ("Sacrifice at Dawn", q("ITEM", "Sacrifice at Dawn", None)),
            ("The Doctor", q("DIVINATION_CARD", "The Doctor", None)),
            ("Deafening Essence of Greed", q("ITEM", "Deafening Essence of Greed", None)),
            ("Gilded Ambush Scarab", q("ITEM", "Gilded Ambush Scarab", None)),
            ("Golden Oil", q("ITEM", "Golden Oil", None)),
            ("Prismatic Fossil", q("ITEM", "Prismatic Fossil", None)),
            ("Strand Map", q("ITEM", "Strand Map", Some("T16, Gen-24"))),
            ("Vaal Grace", q("GEM", "Vaal Grace", Some("21/20c"))),
            ("Arc", q("GEM", "Arc", Some("1/20"))),
            ("Empower Support", q("GEM", "Empower Support", Some("4c"))),
            ("Tabula Rasa", q("UNIQUE", "Tabula Rasa", Some("Simple Robe, 6L"))),
            ("Headhunter", q("UNIQUE", "Headhunter", Some("Leather Belt"))),
            ("Dying Sun", q("UNIQUE", "Dying Sun", None)),
            ("Rumi's Concoction", q("UNIQUE", "Rumi's Concoction", None)),
            ("Doom Grip", None),
        ];
        assert_eq!(got.len(), expect.len());
        for ((name, query), (en, eq)) in got.iter().zip(expect) {
            assert_eq!(name, en);
            assert_eq!(query, &eq, "{name}");
        }
    }

    #[test]
    fn unidentified_unique_resolves_single_base() {
        let db = db();
        let it: Item = serde_json::from_str(r#"{"name":"","typeLine":"Leather Belt","baseType":"Leather Belt","frameType":3,"identified":false}"#).unwrap();
        // several uniques on Leather Belt → unpriceable
        assert_eq!(item_query(&it, &db), None);
        let it: Item = serde_json::from_str(r#"{"name":"","typeLine":"Assassin's Garb","baseType":"Assassin's Garb","frameType":3,"identified":false}"#).unwrap();
        assert_eq!(item_query(&it, &db).map(|q| q.name), Some("Cospri's Will".into()));
    }

    #[test]
    fn markup_and_dust() {
        assert_eq!(strip_markup("<<set:MS>><<set:M>><<set:S>>Headhunter"), "Headhunter");
        // ilvl 84 q20 = 3500 × disenchant value (same terms as the parser)
        assert_eq!(dust_assumed(1.0), 3500.0);
        assert_eq!(dust_value(2.12, 84.0, 0.0), (2.12f64 * 2500.0).floor());
        let db = db();
        assert!(db.disenchant_value("Headhunter").is_some_and(|d| d > 0.0));
    }

    #[test]
    fn values_tabs_with_lookup() {
        let db = db();
        let tab = StashTab {
            info: StashTabInfo { id: "a".into(), parent: None, index: Some(0), name: "$".into(), kind: "CurrencyStash".into(), colour: None, hidden: false, children: vec![] },
            items: items(),
        };
        let prices: HashMap<&str, f64> = [("Divine Orb", 200.0), ("Headhunter", 5000.0), ("The Doctor", 900.0)].into();
        let v = value_tabs(std::slice::from_ref(&tab), &db, Some(200.0), |q| prices.get(q.name.as_str()).map(|c| Found { chaos: *c, url: None }));
        // 3 Divine + 1 HH + 2 Doctor
        assert_eq!(v.total_chaos, 3.0 * 200.0 + 5000.0 + 2.0 * 900.0);
        assert_eq!(v.rows[0].name, "Headhunter");
        assert_eq!(v.rows.iter().find(|r| r.name == "Divine Orb").unwrap().qty, 3);
        assert_eq!(v.tabs[0].items, items().len());
        assert_eq!(v.unpriced, items().len() - 3);
        let hh = v.uniques.iter().find(|u| u.name == "Headhunter").unwrap();
        assert_eq!(hh.chaos, Some(5000.0));
        assert!(hh.dust_per_chaos().unwrap() > 0.0);
    }
}
