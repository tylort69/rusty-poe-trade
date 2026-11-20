//! Builds filters for constructed items and checks the trade API JSON
//! against the shape upstream's `createTradeRequest` would produce.

use rpt_data::{BaseType, Namespace, StatOrGroup};
use rpt_parser::modifiers::sum_stats_by_mod_type;
use rpt_parser::{
    Corrupted, ItemCategory, ItemRarity, ModGeneration, ModifierInfo, ModifierType, ParsedItem, ParsedModifier,
    ParsedRoll, ParsedStat, StackSize,
};
use rpt_trade::filters::create_presets::{PRESET_BULK, PRESET_PSEUDO};
use rpt_trade::filters::{CollapseListings, CreatePresetsOptions, FilterOrGroup, Presets, create_presets};
use rpt_trade::trade::bulk::create_bulk_trade_request;
use rpt_trade::trade::search::create_trade_request;
use rpt_trade::trade::{TradeApi, api_to_satisfy_search};
use serde_json::{Value, json};

fn init() {
    rpt_data::init("en").unwrap();
}

fn base(ns: Namespace, ref_name: &str) -> BaseType {
    rpt_data::data().item_by_ref(ns, ref_name).unwrap_or_else(|| panic!("no item {ref_name}"))[0].clone()
}

struct ModSpec<'a> {
    ref_: &'a str,
    type_: ModifierType,
    roll: (f64, f64, f64),
    tier: Option<u32>,
    generation: Option<ModGeneration>,
}

fn m(ref_: &str, type_: ModifierType, value: f64, min: f64, max: f64) -> ModSpec<'_> {
    ModSpec { ref_, type_, roll: (value, min, max), tier: None, generation: None }
}

fn modifier(spec: &ModSpec) -> ParsedModifier {
    let data = rpt_data::data();
    let stat = match data.stat_by_ref_v2(spec.ref_).unwrap_or_else(|| panic!("no stat {}", spec.ref_)) {
        StatOrGroup::Stat(s) => s.clone(),
        StatOrGroup::Group(g) => g.stats[0].clone(),
    };
    let mut info = ModifierInfo::new(spec.type_);
    info.tier = spec.tier;
    info.generation = spec.generation;
    let (value, min, max) = spec.roll;
    ParsedModifier {
        info,
        stats: vec![ParsedStat {
            translation: stat.matchers[0].clone(),
            stat,
            roll: Some(ParsedRoll { unscalable: false, generation: None, dp: false, value, min, max }),
        }],
    }
}

fn with_mods(mut item: ParsedItem, mods: &[ModSpec]) -> ParsedItem {
    item.new_mods = mods.iter().map(modifier).collect();
    item.stats_by_type = sum_stats_by_mod_type(&item.new_mods);
    item
}

fn opts() -> CreatePresetsOptions {
    CreatePresetsOptions {
        league: "Standard".into(),
        merchant_only: false,
        currency: None,
        collapse_listings: CollapseListings::Api,
        activate_stock_filter: false,
        use_en: false,
        search_stat_range: 10.0,
    }
}

fn active_request(presets: &Presets) -> Value {
    let p = presets.active_preset().unwrap();
    serde_json::to_value(create_trade_request(&p.filters, &p.stats).unwrap()).unwrap()
}

#[test]
fn rare_ring_with_stats() {
    init();
    let mut item = ParsedItem::virtual_item(base(Namespace::Item, "Coral Ring"));
    item.rarity = Some(ItemRarity::Rare);
    item.category = Some(ItemCategory::Ring);
    item.item_level = Some(84);
    let item = with_mods(
        item,
        &[
            ModSpec { tier: Some(1), generation: Some(ModGeneration::Prefix), ..m("+# to maximum Life", ModifierType::Explicit, 75.0, 70.0, 79.0) },
            ModSpec { tier: Some(2), generation: Some(ModGeneration::Suffix), ..m("+#% to Fire Resistance", ModifierType::Explicit, 40.0, 36.0, 41.0) },
            ModSpec { tier: Some(4), generation: Some(ModGeneration::Suffix), ..m("+#% to Cold Resistance", ModifierType::Explicit, 30.0, 30.0, 35.0) },
        ],
    );

    let presets = create_presets(&item, &opts());
    // finished item check: not finished, ilvl >= 82 → pseudo + base item presets
    assert_eq!(presets.active, PRESET_PSEUDO);
    assert_eq!(presets.presets.len(), 2);

    let stats = &presets.active_preset().unwrap().stats;
    let refs: Vec<(&str, bool, Option<&str>)> = stats
        .iter()
        .filter_map(FilterOrGroup::as_stat)
        .map(|s| (s.stat_ref.as_str(), s.disabled, s.hidden.as_deref()))
        .collect();
    assert_eq!(
        refs,
        vec![
            ("+#% total Elemental Resistance", false, None),
            ("+#% total to Fire Resistance", true, Some("filters.hide_ele_res")),
            ("+# total maximum Life", false, None),
        ]
    );

    let req = active_request(&presets);
    assert_eq!(
        req,
        json!({
            "query": {
                "status": { "option": "available" },
                // pseudo preset searches by category (searchRelaxed enabled), no base type
                "stats": [{
                    "type": "and",
                    "filters": [
                        // 70 - 10% = 63, clamped to the lowest possible roll 36 + 30
                        { "id": "pseudo.pseudo_total_elemental_resistance", "value": { "min": 66 }, "disabled": false },
                        { "id": "pseudo.pseudo_total_fire_resistance", "value": { "min": 36 }, "disabled": true },
                        { "id": "pseudo.pseudo_total_life", "value": { "min": 70 }, "disabled": false }
                    ]
                }],
                "filters": {
                    "trade_filters": { "filters": {
                        "collapse": { "option": "true" }
                    }},
                    "type_filters": { "filters": {
                        "rarity": { "option": "nonunique" },
                        "category": { "option": "accessory.ring" }
                    }},
                    "misc_filters": { "filters": {
                        "corrupted": { "option": "false" },
                        "fractured_item": { "option": "false" },
                        // no `split` filter in permanent leagues unless exact
                        "mirrored": { "option": "false" },
                        "quality": { "max": 0 }
                    }}
                }
            },
            "sort": { "price": "asc" }
        })
    );

    // trade site link encodes the same query
    let p = presets.active_preset().unwrap();
    let link = rpt_trade::trade::search::make_trade_link("www.pathofexile.com", &p.filters, &p.stats, None).unwrap();
    assert!(link.starts_with("https://www.pathofexile.com/trade/search/Standard?q=%7B%22query%22"));
    assert_eq!(api_to_satisfy_search(&item, &p.stats, &p.filters), TradeApi::Trade);
}

#[test]
fn base_item_preset_explicit_tiers() {
    init();
    let mut item = ParsedItem::virtual_item(base(Namespace::Item, "Coral Ring"));
    item.rarity = Some(ItemRarity::Rare);
    item.category = Some(ItemCategory::Ring);
    item.item_level = Some(84);
    let item = with_mods(
        item,
        &[
            ModSpec { tier: Some(1), ..m("+# to maximum Life", ModifierType::Explicit, 75.0, 70.0, 79.0) },
            ModSpec { tier: Some(4), ..m("+#% to Cold Resistance", ModifierType::Explicit, 30.0, 30.0, 35.0) },
        ],
    );
    let presets = create_presets(&item, &opts());
    let base_preset = &presets.presets[1];
    assert_eq!(base_preset.id, "filters.preset_base_item");
    // exact mode: ilvl filter enabled, rarity stays non-unique
    assert_eq!(base_preset.filters.item_level.unwrap().value, 84.0);
    assert!(!base_preset.filters.item_level.unwrap().disabled);
    // rare explicit stats are not kept by exact stat filters (only magic)
    assert!(base_preset.stats.is_empty());
}

#[test]
fn unique_body_armour() {
    init();
    let mut item = ParsedItem::virtual_item(base(Namespace::Unique, "Kaom's Heart"));
    item.rarity = Some(ItemRarity::Unique);
    item.category = Some(ItemCategory::BodyArmour);
    item.unique_base = Some(base(Namespace::Item, "Glorious Plate"));
    item.item_level = Some(80);
    item.is_corrupted = Some(Corrupted { twice: false });
    let item = with_mods(item, &[m("+# to maximum Life", ModifierType::Explicit, 500.0, 500.0, 500.0)]);

    let presets = create_presets(&item, &opts());
    assert_eq!(presets.active, PRESET_PSEUDO);
    let req = active_request(&presets);
    assert_eq!(req["query"]["name"], json!("Kaom's Heart"));
    assert_eq!(req["query"]["type"], json!("Glorious Plate"));
    assert_eq!(req["query"]["status"]["option"], json!("available"));
    // corrupted item: only `twice_corrupted: false` is sent
    assert_eq!(
        req["query"]["filters"]["misc_filters"],
        json!({ "filters": {
            "twice_corrupted": { "option": "false" },
            "foulborn_item": { "option": "false" },
            "vestigial": { "option": "false" }
        }})
    );
    // uniques never set currency collapse ("chaos_divine" only for non-craftable non-uniques)
    assert!(req["query"]["filters"]["trade_filters"]["filters"].get("price").is_none());
    // fixed roll on a corrupted unique stays visible and enabled (scalable stat)
    assert_eq!(
        req["query"]["stats"][0]["filters"],
        json!([{ "id": "explicit.stat_3299347043", "value": { "min": 500 }, "disabled": false }])
    );
}

#[test]
fn currency_bulk() {
    init();
    let mut item = ParsedItem::virtual_item(base(Namespace::Item, "Chaos Orb"));
    item.category = Some(ItemCategory::Currency);
    item.stack_size = Some(StackSize { value: 15.0, max: 20.0 });

    let mut o = opts();
    o.activate_stock_filter = true;
    let presets = create_presets(&item, &o);
    let p = presets.active_preset().unwrap();
    assert_eq!(presets.active, "filters.preset_exact");
    assert!(p.stats.is_empty());
    assert_eq!(api_to_satisfy_search(&item, &p.stats, &p.filters), TradeApi::Bulk);

    let req = serde_json::to_value(create_bulk_trade_request(&p.filters, &item, &["divine".to_owned()])).unwrap();
    assert_eq!(
        req,
        json!({
            "engine": "new",
            "query": {
                "status": { "option": "online" },
                "have": ["divine"],
                "want": ["chaos"],
                "minimum": 15
            },
            "sort": { "have": "asc" }
        })
    );

    let link = rpt_trade::trade::bulk::make_bulk_trade_link("www.pathofexile.com", &p.filters, &item, &["chaos".into()])
        .unwrap();
    assert!(link.starts_with("https://www.pathofexile.com/trade/exchange/Standard?q=%7B%22exchange%22%3A"));

    // listing request for the same filters
    let req = active_request(&presets);
    assert_eq!(req["query"]["type"], json!("Chaos Orb"));
    assert_eq!(req["query"]["filters"]["misc_filters"]["filters"]["stack_size"], json!({ "min": 15 }));
    assert_eq!(req["query"]["filters"]["trade_filters"]["filters"]["price"], json!({ "option": "chaos_divine" }));
}

#[test]
fn skill_gem() {
    init();
    let mut item = ParsedItem::virtual_item(base(Namespace::Gem, "Arc"));
    item.category = Some(ItemCategory::Gem);
    item.gem_level = Some(20);
    item.quality = Some(20.0);
    item.is_corrupted = Some(Corrupted { twice: false });

    let presets = create_presets(&item, &opts());
    let req = active_request(&presets);
    assert_eq!(req["query"]["type"], json!("Arc"));
    assert_eq!(
        req["query"]["filters"]["misc_filters"],
        json!({ "filters": {
            "twice_corrupted": { "option": "false" },
            "gem_imbued": { "option": "false" },
            "gem_level": { "min": 20 },
            "quality": { "min": 20 }
        }})
    );
    let details = rpt_trade::trends::get_details_id(&item).unwrap();
    assert_eq!(details.variant.as_deref(), Some("20/20c"));
}

#[test]
fn rare_map() {
    init();
    let mut item = ParsedItem::virtual_item(base(Namespace::Item, "Map"));
    item.category = Some(ItemCategory::Map);
    item.rarity = Some(ItemRarity::Rare);
    item.map_area = Some(base(Namespace::Area, "Strand"));
    item.map_tier = Some(16);
    item.area_item_quantity = Some(80.0);
    item.area_pack_size = Some(30.0);

    let presets = create_presets(&item, &opts());
    assert_eq!(presets.presets.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), [PRESET_PSEUDO, PRESET_BULK]);
    assert_eq!(presets.active, PRESET_PSEUDO);

    let req = active_request(&presets);
    assert_eq!(req["query"]["type"], json!({ "discriminator": "map", "option": "Map" }));
    assert_eq!(
        req["query"]["filters"]["map_filters"],
        json!({ "filters": {
            "map_tier": { "min": 16, "max": 16 },
            "map_blighted": { "option": "false" },
            "map_uberblighted": { "option": "false" },
            "map_iiq": { "min": 72 },
            "map_packsize": { "min": 27 }
        }})
    );
    assert_eq!(req["query"]["filters"]["type_filters"], json!({ "filters": { "rarity": { "option": "nonunique" } } }));

    let bulk = &presets.presets[1];
    let req = serde_json::to_value(create_trade_request(&bulk.filters, &bulk.stats).unwrap()).unwrap();
    assert!(req["query"]["filters"]["map_filters"]["filters"].get("map_iiq").is_none());
}
