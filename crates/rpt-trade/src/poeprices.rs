//! Port of upstream `price-prediction/poeprices.ts` (poeprices.info rare item pricing).

use std::sync::LazyLock;
use std::time::Duration;

use base64::Engine;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::js::{encode_uri_component, math_round};
use crate::prices::{DbQuery, PriceCurrency, PriceIndex};
use crate::{Result, TradeClient, TradeError};

/// Value of the `s` (source) parameter sent to poeprices.info.
pub const POEPRICES_SOURCE: &str = "rusty-poe-trade";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoepricesApiResponse {
    /// "chaos" | "divine" | "exalt"
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub error: i64,
    #[serde(default)]
    pub error_msg: String,
    #[serde(default)]
    pub warning_msg: String,
    #[serde(default)]
    pub max: f64,
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub pred_confidence_score: f64,
    #[serde(default)]
    pub pred_explanation: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceExplanation {
    pub name: String,
    pub contrib: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RareItemPrice {
    pub max: f64,
    pub min: f64,
    pub confidence: f64,
    pub currency: PriceCurrency,
    pub explanation: Vec<PriceExplanation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedbackOption {
    Fair,
    Low,
    High,
}

impl FeedbackOption {
    fn as_str(self) -> &'static str {
        match self {
            FeedbackOption::Fair => "fair",
            FeedbackOption::Low => "low",
            FeedbackOption::High => "high",
        }
    }
}

fn utf8_to_base64(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
}

fn querystring(q: &[(&str, &str)]) -> String {
    q.iter()
        .map(|(k, v)| format!("{}={}", encode_uri_component(k), encode_uri_component(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Upstream `transformItemText` (deprecated upstream, but still used):
/// strips advanced-description roll ranges and `{ ... }` header lines.
pub fn transform_item_text(raw_text: &str) -> String {
    static RANGES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d)\([^)]+\)").expect("regex"));
    static HEADERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\{.+\}$\n").expect("regex"));
    let s = RANGES.replace_all(raw_text, "$1");
    HEADERS.replace_all(&s, "").into_owned()
}

fn item_query(raw_text: &str, league: &str, external: bool) -> String {
    let i = utf8_to_base64(&transform_item_text(raw_text));
    let mut q = vec![("i", i.as_str()), ("l", league), ("s", POEPRICES_SOURCE)];
    if external {
        q.push(("w", "1"));
    }
    querystring(&q)
}

/// Upstream `getExternalLink`.
pub fn get_external_link(raw_text: &str, league: &str) -> String {
    format!("https://www.poeprices.info/api?{}", item_query(raw_text, league, true))
}

/// Converts the API response into the displayed price (exalt → chaos/div conversion).
pub fn convert_response(mut data: PoepricesApiResponse, prices: &PriceIndex) -> Result<RareItemPrice> {
    if data.currency == "exalt" {
        let xchg = prices
            .find_price_by_query(&DbQuery { ns: "ITEM".into(), name: "Exalted Orb".into(), variant: None })
            .ok_or_else(|| TradeError::Poeprices("poeprices.info gave the price in Exalted Orbs.".into()))?;
        let converted = prices.auto_currency_range(data.min * xchg.chaos, data.max * xchg.chaos);
        data.min = converted.min;
        data.max = converted.max;
        data.currency = if converted.currency == PriceCurrency::Div { "divine" } else { "chaos" }.to_owned();
    } else if data.currency != "divine" && data.currency != "chaos" {
        return Err(TradeError::Poeprices("poeprices.info gave the price in unknown currency.".into()));
    }

    Ok(RareItemPrice {
        currency: if data.currency == "divine" { PriceCurrency::Div } else { PriceCurrency::Chaos },
        min: data.min,
        max: data.max,
        confidence: math_round(data.pred_confidence_score),
        explanation: data
            .pred_explanation
            .iter()
            .map(|(name, c)| PriceExplanation { name: name.clone(), contrib: math_round(c * 100.0) })
            .collect(),
    })
}

impl TradeClient {
    /// Upstream `requestPoeprices`. `raw_text` is `ParsedItem.raw_text`.
    pub async fn request_poeprices(&self, raw_text: &str, league: &str, prices: &PriceIndex) -> Result<RareItemPrice> {
        let query = item_query(raw_text, league, false);

        let data = match self.poeprices_cache.get(&query) {
            Some(d) => d,
            None => {
                let resp = self.http().get(format!("https://www.poeprices.info/api?{query}")).send().await?;
                let status = resp.status();
                let text = resp.text().await?;
                let data: PoepricesApiResponse = serde_json::from_str(&text).map_err(|_| {
                    TradeError::Poeprices(format!("{}, poeprices.info API is under load or down.", status.as_u16()))
                })?;
                if data.error != 0 {
                    return Err(TradeError::Poeprices(data.error_msg));
                }
                self.poeprices_cache.set(&query, data.clone(), Duration::from_secs(300));
                data
            }
        };

        convert_response(data, prices)
    }

    /// Upstream `sendFeedback`.
    pub async fn send_poeprices_feedback(
        &self,
        text: &str,
        option: FeedbackOption,
        prediction: &RareItemPrice,
        raw_text: &str,
        league: &str,
    ) -> Result<()> {
        let currency = match prediction.currency {
            PriceCurrency::Div => "divine",
            PriceCurrency::Chaos => "chaos",
        };
        let form = reqwest::multipart::Form::new()
            .text("selector", option.as_str())
            .text("feedbacktxt", text.to_owned())
            .text("qitem_txt", utf8_to_base64(&transform_item_text(raw_text)))
            .text("source", POEPRICES_SOURCE)
            .text("min", crate::js::num_to_string(prediction.min))
            .text("max", crate::js::num_to_string(prediction.max))
            .text("currency", currency)
            .text("league", league.to_owned());
        let resp = self.http().post("https://www.poeprices.info/send_feedback").multipart(form).send().await?;
        let _ = resp.text().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform() {
        let t = "Item Class: Rings\n{ Prefix Modifier \"Hale\" (Tier: 8) }\n+12(10-19) to maximum Life\n";
        assert_eq!(transform_item_text(t), "Item Class: Rings\n+12 to maximum Life\n");
        assert_eq!(utf8_to_base64("ä"), "w6Q=");
        assert!(get_external_link("x", "Standard").ends_with("i=eA%3D%3D&l=Standard&s=rusty-poe-trade&w=1"));
    }
}
