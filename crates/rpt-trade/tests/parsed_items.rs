//! End-to-end: clipboard text → rpt-parser → presets → trade query.

use rpt_trade::filters::{CollapseListings, CreatePresetsOptions, create_presets};
use rpt_trade::trade::search::create_trade_request;

const RARE_HELMET: &str = r#"Item Class: Helmets
Rarity: Rare
Doom Visor
Hubris Circlet
--------
Energy Shield: 220 (augmented)
--------
Requirements:
Level: 69
Int: 154
--------
Sockets: B-B-B B
--------
Item Level: 84
--------
{ Prefix Modifier "Seething" (Tier: 3) — Defences, Energy Shield }
+89(80-91) to maximum Energy Shield
{ Prefix Modifier "Rotund" (Tier: 3) — Life }
+71(70-79) to maximum Life
{ Suffix Modifier "of the Furnace" (Tier: 3) — Elemental, Fire, Resistance }
+42(42-45)% to Fire Resistance
{ Suffix Modifier "of the Thunderhead" (Tier: 4) — Elemental, Lightning, Resistance }
+33(30-35)% to Lightning Resistance
"#;

// The overlay always copies with Ctrl+Alt+C (advanced mod descriptions), like upstream.
#[test]
fn rare_helmet_from_clipboard() {
    rpt_data::init("en").unwrap();
    let item = match rpt_parser::parse_clipboard(RARE_HELMET) {
        Ok(item) => item,
        Err(e) => panic!("parser error: {e}"),
    };
    let presets = create_presets(
        &item,
        &CreatePresetsOptions {
            league: "Standard".into(),
            merchant_only: true,
            currency: None,
            collapse_listings: CollapseListings::Api,
            activate_stock_filter: false,
            use_en: false,
            search_stat_range: 10.0,
        },
    );
    let p = presets.active_preset().unwrap();
    let req = serde_json::to_value(create_trade_request(&p.filters, &p.stats).unwrap()).unwrap();
    let ids: Vec<&str> = req["query"]["stats"][0]["filters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"pseudo.pseudo_total_life"), "{req:#}");
    assert!(ids.contains(&"pseudo.pseudo_total_elemental_resistance"), "{req:#}");
    assert_eq!(req["query"]["status"]["option"], "securable");
    assert_eq!(req["query"]["filters"]["type_filters"]["filters"]["category"]["option"], "armour.helmet");
    // ES property filter (single-attribute armour → enabled)
    assert!(req["query"]["filters"]["armour_filters"]["filters"]["es"]["min"].is_number(), "{req:#}");
}

#[test]
fn widget_defaults_apply_to_presets() {
    use rpt_trade::client::TradeConfig;
    rpt_data::init("en").unwrap();
    let item = rpt_parser::parse_clipboard(RARE_HELMET).unwrap();
    let mut cfg = TradeConfig::default();
    cfg.price_check.default_offline = true;
    cfg.price_check.default_listed = Some("3days".into());
    cfg.price_check.fill_roll_max = true;
    let presets = rpt_trade::check::presets_for_item(&item, None, &cfg, "Standard");
    let p = presets.active_preset().unwrap();
    let req = serde_json::to_value(create_trade_request(&p.filters, &p.stats).unwrap()).unwrap();
    assert_eq!(req["query"]["status"]["option"], "any", "{req:#}");
    assert_eq!(req["query"]["filters"]["trade_filters"]["filters"]["indexed"]["option"], "3days", "{req:#}");
    let es = &req["query"]["filters"]["armour_filters"]["filters"]["es"];
    assert!(es["min"].is_number() && es["max"].is_number(), "{req:#}");

    // "Every item" auto-search for the locked hotkey
    cfg.price_check.locked_initial_always = true;
    assert!(rpt_trade::check::initial_do_search(&item, &presets, true, &cfg));
    assert!(!rpt_trade::check::initial_do_search(&item, &presets, false, &cfg));
}
