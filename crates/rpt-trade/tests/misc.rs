//! Data validation, response decoding and result grouping.

use rpt_trade::filters::pseudo;
use rpt_trade::trade::bulk::{BulkSearchResponse, merge_with_market_ratio, split_bulk_results, MarketRatio, BulkRow};
use rpt_trade::trade::search::{PricingResult, group_results};
use rpt_trade::trade::AccountStatus;

#[test]
fn all_used_stat_refs_exist() {
    let data = rpt_data::init("en").unwrap();
    let mut refs: Vec<&str> = Vec::new();
    refs.extend(pseudo::pseudo_rule_refs());
    refs.extend(pseudo::item_property::used_stat_refs());
    refs.extend(pseudo::maps::used_stat_refs());
    refs.extend(pseudo::heist::USED_STAT_REFS);
    refs.extend(pseudo::flasks::USED_STAT_REFS);
    refs.extend(pseudo::reflection_rules::used_stat_refs());
    refs.extend(pseudo::atzoatl_rules::used_stat_refs());
    refs.extend(pseudo::mercenary::used_stat_refs());
    refs.extend(rpt_trade::trade::search::used_stat_refs());
    refs.extend(rpt_trade::calc_q20::QUALITY_CHANGING_ENCHANT);
    let missing: Vec<&str> = refs.iter().copied().filter(|r| data.stat_by_ref_v2(r).is_none()).collect();
    assert!(missing.is_empty(), "missing stats: {missing:?}");
}

#[test]
fn bulk_split_and_ratio() {
    // parsed from text: key order of `result` must be preserved
    let resp: BulkSearchResponse = serde_json::from_str(r#"{
        "id": "Q1",
        "total": 3,
        "result": {
            "b": { "id": "b", "listing": { "indexed": "2024-01-01T00:00:00Z", "account": { "name": "B", "lastCharacterName": "b1", "online": { "status": "afk" } },
                "offers": [{ "exchange": { "currency": "chaos", "amount": 10 }, "item": { "amount": 1, "stock": 5 } }] } },
            "a": { "id": "a", "listing": { "indexed": "2024-01-01T00:00:00Z", "account": { "name": "Me", "lastCharacterName": "a1" },
                "offers": [{ "exchange": { "currency": "chaos", "amount": 9 }, "item": { "amount": 1, "stock": 2 } }] } },
            "c": { "id": "c", "listing": { "indexed": "2024-01-01T00:00:00Z", "account": { "name": "C", "lastCharacterName": "c1", "online": {} },
                "offers": [{ "exchange": { "currency": "divine", "amount": 1 }, "item": { "amount": 20, "stock": 40 } }] } }
        }
    }"#)
    .unwrap();
    assert_eq!(resp.result.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);

    let now = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:05:00Z").unwrap().with_timezone(&chrono::Utc);
    let split = split_bulk_results(&resp, &["divine".into(), "chaos".into()], "Me", now);
    let divine = split[0].as_ref().unwrap();
    // total is a best guess: query.total - results of other currencies
    assert_eq!(divine.total, 1);
    assert_eq!(divine.listed[0].account_status, AccountStatus::Online);
    let chaos = split[1].as_ref().unwrap();
    assert_eq!(chaos.total, 2);
    assert_eq!(chaos.listed.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert!(chaos.listed[0].is_mine);
    assert_eq!(chaos.listed[1].account_status, AccountStatus::Afk);
    assert_eq!(chaos.listed[0].relative_date, "5 min. ago");

    let rows = merge_with_market_ratio(&chaos.listed, Some(MarketRatio { exchange_amount: 9.5, item_amount: 1.0 }));
    assert!(matches!(rows[1], BulkRow::MarketRatio(_)));
    assert_eq!(rows.len(), 3);
}

fn pr(id: &str, account: &str, amount: f64, fee: bool) -> PricingResult {
    PricingResult {
        id: id.into(),
        item_level: None,
        stack_size: None,
        corrupted: None,
        quality: None,
        level: None,
        indexed: String::new(),
        relative_date: String::new(),
        price_amount: amount,
        price_currency: "chaos".into(),
        is_mine: false,
        has_note: true,
        has_fee: fee,
        account_name: account.into(),
        account_status: AccountStatus::Online,
        ign: String::new(),
    }
}

#[test]
fn listing_grouping() {
    let results = vec![
        pr("1", "A", 5.0, false),
        pr("2", "A", 5.0, false),
        pr("3", "B", 6.0, false),
        pr("4", "A", 7.0, false), // same account, 2 entries back → grouped
        pr("5", "C", 8.0, true),
        pr("6", "C", 8.0, true),
    ];
    let grouped = group_results(&results, false);
    assert_eq!(grouped.iter().map(|g| (g.result.id.as_str(), g.listed_times)).collect::<Vec<_>>(),
        [("1", 3), ("3", 1), ("5", 1), ("6", 1)]);
    let grouped = group_results(&results, true);
    assert_eq!(grouped.iter().map(|g| (g.result.id.as_str(), g.listed_times)).collect::<Vec<_>>(),
        [("1", 3), ("3", 1), ("5", 2)]);
}

#[test]
fn rate_limit_state_snapshot() {
    let client = rpt_trade::TradeClient::new(rpt_trade::TradeConfig::default()).unwrap();
    let state = client.rate_limit_state();
    assert_eq!(state.limits.len(), 3);
    assert_eq!(state.limits[0].policy, "trade-search-request-limit");
    assert!(!state.is_limited());
    assert_eq!(client.trade_endpoint(), "www.pathofexile.com");
    client.update_config(|c| c.language = "ko".into());
    assert_eq!(client.trade_endpoint(), "poe.kakaogames.com");
}

/// Hits the real trade API. Run with `cargo test -p rpt-trade -- --ignored`.
#[tokio::test]
#[ignore]
async fn live_bulk_search_and_leagues() {
    use std::sync::Arc;
    rpt_data::init("en").unwrap();
    let client = Arc::new(rpt_trade::TradeClient::new(rpt_trade::TradeConfig::default()).unwrap());
    let leagues = rpt_trade::Leagues::new(client.clone());
    let selected = leagues.load().await.expect("leagues");
    let league = selected.expect("selected league");

    let info = rpt_data::data().item_by_ref(rpt_data::Namespace::Item, "Chaos Orb").unwrap()[0].clone();
    let mut item = rpt_parser::ParsedItem::virtual_item(info);
    item.category = Some(rpt_parser::ItemCategory::Currency);
    let filters = rpt_trade::filters::create_filters(
        &item,
        &rpt_trade::filters::CreateOptions {
            league,
            merchant_only: false,
            currency: None,
            collapse_listings: rpt_trade::filters::CollapseListings::Api,
            activate_stock_filter: false,
            exact: true,
            use_en: false,
        },
    );
    let res = client.bulk_search(&item, &filters).await.expect("bulk search");
    assert!(res.xchg_stable.is_some());
    let listing = client.search_listings(&filters, &[], |_| {}).await.expect("listing search");
    assert!(!listing.results.is_empty());
    println!("{:?}", listing.grouped().first());
    println!("{:?}", client.rate_limit_state());
}
