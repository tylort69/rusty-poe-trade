//! Port of upstream `trade/pathofexile-bulk.ts` (+ logic of `TradeBulk.vue`).

use rpt_parser::ParsedItem;
use serde::{Deserialize, Serialize};

use super::cache::Cache;
use super::rate_limiter::prevent_queue_creation;
use super::{Account, AccountStatus, trade_tag};
use crate::client::TradeClient;
use crate::filters::ItemFilters;
use crate::js::{JsNum, encode_uri_component};
use crate::prices::PriceIndex;
use crate::{Result, TradeError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkStatus {
    /// "online" | "onlineleague" | "any"
    pub option: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkQuery {
    pub status: BulkStatus,
    pub have: Vec<String>,
    pub want: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<JsNum>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkSort {
    pub have: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkTradeRequest {
    /// always "new"
    pub engine: String,
    pub query: BulkQuery,
    pub sort: BulkSort,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkExchange {
    pub currency: String,
    pub amount: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkOfferItem {
    pub amount: f64,
    pub stock: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkOffer {
    pub exchange: BulkExchange,
    pub item: BulkOfferItem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkListing {
    pub indexed: String,
    pub offers: Vec<BulkOffer>,
    pub account: Account,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkFetchResult {
    pub id: String,
    pub listing: BulkListing,
}

/// Exchange API response (upstream `SearchResult` in pathofexile-bulk.ts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulkSearchResponse {
    pub id: String,
    /// keyed by listing id, in server order (upstream `Object.values`)
    #[serde(default, with = "ordered_map")]
    pub result: Vec<(String, BulkFetchResult)>,
    #[serde(default)]
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkPricingResult {
    pub id: String,
    pub indexed: String,
    pub relative_date: String,
    pub exchange_amount: f64,
    pub item_amount: f64,
    pub stock: f64,
    pub account_status: AccountStatus,
    pub is_mine: bool,
    pub account_name: String,
    pub ign: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSearch {
    pub query_id: String,
    pub have_tag: String,
    pub total: i64,
    pub listed: Vec<BulkPricingResult>,
}

/// Upstream `createTradeRequest` (bulk).
pub fn create_bulk_trade_request(filters: &ItemFilters, item: &ParsedItem, have: &[String]) -> BulkTradeRequest {
    BulkTradeRequest {
        engine: "new".to_owned(),
        query: BulkQuery {
            have: have.to_vec(),
            want: trade_tag(item).map(|t| vec![t.to_owned()]).unwrap_or_default(),
            status: BulkStatus {
                option: if filters.trade.offline {
                    "any"
                } else if filters.trade.online_in_league {
                    "onlineleague"
                } else {
                    "online"
                }
                .to_owned(),
            },
            minimum: filters.stack_size.filter(|s| !s.disabled).map(|s| JsNum(s.value)),
        },
        sort: BulkSort { have: "asc".to_owned() },
    }
}

fn to_pricing_result(
    result: &BulkFetchResult,
    account_name: &str,
    offer: usize,
    now: chrono::DateTime<chrono::Utc>,
) -> BulkPricingResult {
    let o = &result.listing.offers[offer];
    BulkPricingResult {
        id: result.id.clone(),
        indexed: result.listing.indexed.clone(),
        relative_date: crate::js::relative_date_short(&result.listing.indexed, now),
        exchange_amount: o.exchange.amount,
        item_amount: o.item.amount,
        stock: o.item.stock,
        is_mine: result.listing.account.name == account_name,
        ign: result.listing.account.last_character_name.clone(),
        account_name: result.listing.account.name.clone(),
        account_status: result.listing.account.status(),
    }
}

const SHOW_RESULTS: usize = 20;
const API_FETCH_LIMIT: u64 = 100;

/// Splits an exchange response per `have` currency (body of upstream `execBulkSearch`).
pub fn split_bulk_results(
    query: &BulkSearchResponse,
    have: &[String],
    account_name: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<Option<BulkSearch>> {
    let offer = 0;
    let results: Vec<&BulkFetchResult> = query.result.iter().map(|(_, r)| r).filter(|r| r.listing.offers.len() == 1).collect();

    have.iter()
        .map(|trade_tag| {
            let mut results_tag: Vec<&BulkFetchResult> =
                results.iter().copied().filter(|r| r.listing.offers[offer].exchange.currency == *trade_tag).collect();

            let loaded_on_demand =
                trade_tag == "chaos" && results_tag.len() < SHOW_RESULTS && query.total > API_FETCH_LIMIT;
            if loaded_on_demand {
                return None;
            }

            let ratio = |r: &BulkFetchResult| r.listing.offers[offer].exchange.amount / r.listing.offers[offer].item.amount;
            results_tag.sort_by(|a, b| ratio(a).partial_cmp(&ratio(b)).unwrap_or(std::cmp::Ordering::Equal));
            let listed: Vec<BulkPricingResult> = results_tag
                .iter()
                .take(SHOW_RESULTS)
                .map(|r| to_pricing_result(r, account_name, offer, now))
                .collect();

            let others = results.len() - results_tag.len();
            let chaos_is_loaded = trade_tag == "divine"
                && results_tag.len() < results.len()
                && (others >= SHOW_RESULTS || query.total <= API_FETCH_LIMIT);

            Some(BulkSearch {
                query_id: query.id.clone(),
                have_tag: trade_tag.clone(),
                // this is a best guess when making request with multiple `have` currencies
                total: if chaos_is_loaded { results_tag.len() as i64 } else { query.total as i64 - others as i64 },
                listed,
            })
        })
        .collect()
}

/// Result of a bulk search for both currencies (upstream `BulkSearchExtended`).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkResults {
    /// results priced in divine orbs; `None` = not loaded yet (load with [`TradeClient::load_bulk_have`])
    pub xchg_stable: Option<BulkSearch>,
    /// results priced in chaos orbs; `None` = not loaded yet
    pub xchg_chaos: Option<BulkSearch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BulkCurrency {
    XchgChaos,
    XchgStable,
}

impl BulkCurrency {
    pub fn have_tag(self) -> &'static str {
        match self {
            BulkCurrency::XchgChaos => "chaos",
            BulkCurrency::XchgStable => "divine",
        }
    }
}

impl BulkResults {
    pub fn get(&self, c: BulkCurrency) -> Option<&BulkSearch> {
        match c {
            BulkCurrency::XchgChaos => self.xchg_chaos.as_ref(),
            BulkCurrency::XchgStable => self.xchg_stable.as_ref(),
        }
    }

    /// Initial tab selection (watch on `result` in `TradeBulk.vue`).
    pub fn default_currency(&self) -> BulkCurrency {
        match (self.xchg_stable.as_ref().map(|r| r.total), self.xchg_chaos.as_ref().map(|r| r.total)) {
            (None, _) => BulkCurrency::XchgChaos,
            (Some(_), None) => BulkCurrency::XchgStable,
            (Some(s), Some(c)) => {
                if s > c {
                    BulkCurrency::XchgStable
                } else {
                    BulkCurrency::XchgChaos
                }
            }
        }
    }
}

/// Market ratio row inserted into bulk results (from poe.ninja).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketRatio {
    pub exchange_amount: f64,
    pub item_amount: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketRatioMap {
    pub xchg_chaos: Option<MarketRatio>,
    pub xchg_stable: Option<MarketRatio>,
}

/// Upstream `useMarketRatioFinder().find`. `selected_league` is the app's
/// selected league id.
pub fn find_market_ratio(
    item: &ParsedItem,
    filters: &ItemFilters,
    prices: &PriceIndex,
    selected_league: Option<&str>,
) -> MarketRatioMap {
    let mut out = MarketRatioMap::default();
    if Some(filters.trade.league.as_str()) != selected_league {
        return out;
    }
    let Some(entry) = crate::trends::get_details_id(item).and_then(|id| prices.find_price_by_query(&id.into())) else {
        return out;
    };
    let xchg_rate = prices.xchg_rate();
    let rate = xchg_rate.unwrap_or(9999.0);

    if entry.chaos < rate * 1.06 {
        let (price, items) = if entry.chaos > 4.0 {
            (entry.chaos, 1.0)
        } else {
            let price = if entry.chaos > 1.0 { 20.0 } else { 10.0 };
            (price, price / entry.chaos)
        };
        out.xchg_chaos = Some(MarketRatio { exchange_amount: price, item_amount: items });
    }

    if let Some(rate) = xchg_rate {
        let (price, items) = if entry.chaos > rate * 0.94 {
            (entry.chaos / rate, 1.0)
        } else {
            let price = if entry.chaos > rate * 0.2 { 2.0 } else { 1.0 };
            (price, price * (rate / entry.chaos))
        };
        out.xchg_stable = Some(MarketRatio { exchange_amount: price, item_amount: items });
    }
    out
}

/// A bulk results row: a listing or the market-ratio marker.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum BulkRow {
    Listing(BulkPricingResult),
    MarketRatio(MarketRatio),
}

/// Upstream `mergeWithMarketRatio` (+ the 19-row slice from `selectedResults`).
pub fn merge_with_market_ratio(results: &[BulkPricingResult], ratio: Option<MarketRatio>) -> Vec<BulkRow> {
    let Some(ratio) = ratio else {
        return results.iter().cloned().map(BulkRow::Listing).collect();
    };
    let results = &results[..results.len().min(19)];
    let mut out: Vec<BulkRow> = results.iter().cloned().map(BulkRow::Listing).collect();
    let r = ratio.exchange_amount / ratio.item_amount;
    let idx = results.iter().position(|x| x.exchange_amount / x.item_amount >= r).unwrap_or(results.len());
    out.insert(idx, BulkRow::MarketRatio(ratio));
    out
}

impl TradeClient {
    async fn request_bulk_result_list(&self, body: &BulkTradeRequest, league_id: &str) -> Result<BulkSearchResponse> {
        let key = (body, league_id);
        if let Some(cached) = self.bulk_cache.get(&key) {
            return Ok(cached);
        }

        prevent_queue_creation(&[(1, &self.exchange_limits)])?;
        self.exchange_limits.wait_multi().await;

        let url = format!("https://{}/api/trade/exchange/{}", self.trade_endpoint(), encode_uri_component(league_id));
        let req = self
            .http()
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(body)?);
        let data: BulkSearchResponse = self.trade_api(req, &self.exchange_limits).await?;

        let ttl = Cache::<BulkSearchResponse>::derive_ttl(self.exchange_limits.snapshot().len());
        self.bulk_cache.set(&key, data.clone(), ttl);
        Ok(data)
    }

    /// Upstream `execBulkSearch`.
    pub async fn exec_bulk_search(
        &self,
        item: &ParsedItem,
        filters: &ItemFilters,
        have: &[String],
    ) -> Result<Vec<Option<BulkSearch>>> {
        if trade_tag(item).is_none() {
            return Err(TradeError::Precondition("item has no trade tag for bulk exchange".to_owned()));
        }
        let query = self
            .request_bulk_result_list(&create_bulk_trade_request(filters, item, have), &filters.trade.league)
            .await?;
        let account = self.config().account_name;
        Ok(split_bulk_results(&query, have, &account, chrono::Utc::now()))
    }

    /// Optimistic search for both currencies (upstream `useBulkApi().search`).
    /// Entries left `None` are loaded on demand with [`TradeClient::load_bulk_have`].
    pub async fn bulk_search(&self, item: &ParsedItem, filters: &ItemFilters) -> Result<BulkResults> {
        // override, because at league start many players set wrong price, and this breaks optimistic search
        let have: Vec<String> = match item.info.ref_name.as_str() {
            "Chaos Orb" => vec!["divine".into()],
            "Divine Orb" => vec!["chaos".into()],
            _ => vec!["divine".into(), "chaos".into()],
        };
        let optimistic = self.exec_bulk_search(item, filters, &have).await?;
        let pick = |tag: &str| optimistic.iter().flatten().find(|r| r.have_tag == tag).cloned();
        Ok(BulkResults { xchg_stable: pick("divine"), xchg_chaos: pick("chaos") })
    }

    /// Lazy load for a currency tab that the optimistic search didn't cover
    /// (`listedLazy` in `TradeBulk.vue`). Also fixes the other tab's best-guess total.
    pub async fn load_bulk_have(
        &self,
        item: &ParsedItem,
        filters: &ItemFilters,
        results: &mut BulkResults,
        currency: BulkCurrency,
    ) -> Result<()> {
        if results.get(currency).is_some() {
            return Ok(());
        }
        let loaded = self
            .exec_bulk_search(item, filters, &[currency.have_tag().to_owned()])
            .await?
            .into_iter()
            .next()
            .flatten();
        let Some(loaded) = loaded else { return Ok(()) };
        // fix best guess we did while making optimistic search
        let other = match currency {
            BulkCurrency::XchgStable => &mut results.xchg_chaos,
            BulkCurrency::XchgChaos => &mut results.xchg_stable,
        };
        if let Some(other) = other {
            other.total -= loaded.total;
        }
        match currency {
            BulkCurrency::XchgStable => results.xchg_stable = Some(loaded),
            BulkCurrency::XchgChaos => results.xchg_chaos = Some(loaded),
        }
        Ok(())
    }

    /// Bulk exchange link (`makeTradeLink` in `TradeBulk.vue`).
    pub fn make_bulk_trade_link(&self, filters: &ItemFilters, item: &ParsedItem, have: &[String]) -> Result<String> {
        make_bulk_trade_link(self.trade_endpoint(), filters, item, have)
    }
}

pub fn make_bulk_trade_link(endpoint: &str, filters: &ItemFilters, item: &ParsedItem, have: &[String]) -> Result<String> {
    let body = create_bulk_trade_request(filters, item, have);
    let q = serde_json::to_string(&serde_json::json!({ "exchange": body.query }))?;
    Ok(format!(
        "https://{endpoint}/trade/exchange/{}?q={}",
        encode_uri_component(&filters.trade.league),
        encode_uri_component(&q)
    ))
}

/// (De)serializes a JSON object as an ordered list of entries.
mod ordered_map {
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, T: Serialize>(v: &[(String, T)], s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(v.len()))?;
        for (k, val) in v {
            m.serialize_entry(k, val)?;
        }
        m.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<(String, T)>, D::Error> {
        struct V<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = Vec<(String, T)>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, T>()? {
                    out.push((k, v));
                }
                Ok(out)
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(Vec::new())
            }
        }
        d.deserialize_any(V(std::marker::PhantomData))
    }
}
