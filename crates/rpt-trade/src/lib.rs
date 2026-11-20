//! Price-check core for Rusty POE Trade.
//!
//! Ports of upstream (Awakened PoE Trade) `renderer/src`:
//! - `parser/calc-q20.ts` → [`calc_q20`]
//! - `web/price-check/filters/**` → [`filters`]
//! - `web/price-check/trade/**` → [`trade`]
//! - `web/background/Prices.ts` → [`prices`]
//! - `web/background/Leagues.ts` → [`leagues`]
//! - `web/price-check/price-prediction/poeprices.ts` → [`poeprices`]
//! - `web/price-check/trends/*` → [`trends`]
//! - logic parts of `expected-value`, `related-items`, `unidentified-resolver`
//!   and `CheckedItem.vue` → [`extras`], [`check`]
//!
//! Everything here is UI-agnostic: filter state is plain serde structs the UI
//! renders and mutates, network calls are `async fn`s on [`TradeClient`].

// Upstream control flow is kept 1:1 for easier diffing against the TS source.
#![allow(clippy::collapsible_if, clippy::if_same_then_else)]

pub mod calc_q20;
pub mod check;
pub mod client;
pub mod error;
pub mod extras;
pub mod filters;
pub mod js;
pub mod leagues;
pub mod poeprices;
pub mod prices;
pub mod trade;
pub mod trends;

pub use client::{PriceCheckOptions, Realm, TradeClient, TradeConfig};
pub use error::TradeError;
pub use filters::{
    FilterGroup, FilterOrGroup, FilterPreset, FilterTag, ItemFilters, Presets, StatFilter,
};
pub use leagues::Leagues;
pub use prices::PriceIndex;

pub type Result<T, E = TradeError> = std::result::Result<T, E>;
