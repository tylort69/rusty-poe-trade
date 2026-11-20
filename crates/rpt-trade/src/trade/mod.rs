//! Port of upstream `web/price-check/trade/*`.

pub mod bulk;
pub mod cache;
pub mod rate_limiter;
pub mod search;

use rpt_parser::{ItemCategory, ParsedItem};
use serde::{Deserialize, Serialize};

use crate::filters::{FilterOrGroup, ItemFilters};

pub use rate_limiter::{RateLimitPolicyState, RateLimitRuleState, RateLimitRules, RateLimitState, RateLimiter};

/// Which trade site API satisfies a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TradeApi {
    Trade,
    Bulk,
}

/// Upstream `Account`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    #[serde(default)]
    pub last_character_name: String,
    #[serde(default)]
    pub online: Option<AccountOnline>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountOnline {
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Offline,
    Online,
    Afk,
}

impl Account {
    pub fn status(&self) -> AccountStatus {
        match &self.online {
            Some(o) if o.status.as_deref() == Some("afk") => AccountStatus::Afk,
            Some(_) => AccountStatus::Online,
            None => AccountStatus::Offline,
        }
    }
}

/// `{ error: { code, message } }` part of `TradeResponse<T>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: i64,
    pub message: String,
}

/// Upstream `apiToSatisfySearch`.
pub fn api_to_satisfy_search(item: &ParsedItem, stats: &[FilterOrGroup], _filters: &ItemFilters) -> TradeApi {
    if stats.iter().any(FilterOrGroup::is_enabled) {
        return TradeApi::Trade;
    }
    if trade_tag(item).is_some() { TradeApi::Bulk } else { TradeApi::Trade }
}

/// Upstream `tradeTag`.
pub fn trade_tag(item: &ParsedItem) -> Option<&str> {
    item.info.trade_tag.as_deref()
}

/// Upstream `CATEGORY_TO_TRADE_ID`.
pub fn category_to_trade_id(category: ItemCategory) -> Option<&'static str> {
    use ItemCategory::*;
    Some(match category {
        Map => "map",
        AbyssJewel => "jewel.abyss",
        Amulet => "accessory.amulet",
        Belt => "accessory.belt",
        BodyArmour => "armour.chest",
        Boots => "armour.boots",
        Bow => "weapon.bow",
        Claw => "weapon.claw",
        Dagger => "weapon.dagger",
        FishingRod => "weapon.rod",
        Flask => "flask",
        Gloves => "armour.gloves",
        Helmet => "armour.helmet",
        Jewel => "jewel",
        OneHandedAxe => "weapon.oneaxe",
        OneHandedMace => "weapon.onemace",
        OneHandedSword => "weapon.onesword",
        Quiver => "armour.quiver",
        Ring => "accessory.ring",
        RuneDagger => "weapon.runedagger",
        Sceptre => "weapon.sceptre",
        Shield => "armour.shield",
        Staff => "weapon.staff",
        TwoHandedAxe => "weapon.twoaxe",
        TwoHandedMace => "weapon.twomace",
        TwoHandedSword => "weapon.twosword",
        Wand => "weapon.wand",
        Warstaff => "weapon.warstaff",
        ClusterJewel => "jewel.cluster",
        HeistBlueprint => "heistmission.blueprint",
        HeistContract => "heistmission.contract",
        HeistTool => "heistequipment.heisttool",
        HeistBrooch => "heistequipment.heistreward",
        HeistGear => "heistequipment.heistweapon",
        HeistCloak => "heistequipment.heistutility",
        Trinket => "accessory.trinket",
        SanctumRelic => "sanctum.relic",
        Tincture => "tincture",
        Charm => "azmeri.charm",
        Idol => "idol",
        Graft => "graft",
        Chart => "chart",
        HeadViscera => "viscera.head",
        TorsoViscera => "viscera.body",
        LowerLimbViscera => "viscera.leg",
        UpperLimbViscera => "viscera.arm",
        _ => return None,
    })
}
