//! Port of upstream `web/price-check/filters/*` (pure logic).
//!
//! The structs in this module are the filter state that the UI renders and
//! mutates (fields marked "mutable in UI" upstream). They serialize to
//! camelCase JSON matching upstream `interfaces.ts`.

pub mod create_item_filters;
pub mod create_presets;
pub mod create_stat_filters;
pub mod pseudo;
pub mod util;

use rpt_parser::{ItemCategory, ItemInfluence, ModifierMechanic, ModifierType, StatSource};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use create_item_filters::{CreateOptions, SPECIAL_SUPPORT_GEM, create_filters, floor_to_bracket};
pub use create_presets::{CreatePresetsOptions, create_presets};
pub use create_stat_filters::{
    FiltersCreationContext, StatFiltersMode, calculated_stat_to_filter, create_exact_stat_filters,
    init_ui_mod_filters,
};

fn is_false(v: &bool) -> bool {
    !*v
}

/// serde helpers to store `ItemCategory` as its upstream string value ("Body Armour").
pub mod serde_category {
    use super::*;

    pub fn serialize<S: Serializer>(v: &Option<ItemCategory>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(c) => s.serialize_str(c.as_str()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ItemCategory>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        match s {
            None => Ok(None),
            Some(s) => ItemCategory::from_str(&s)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("unknown item category: {s}"))),
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterPreset {
    pub id: String,
    pub filters: ItemFilters,
    pub stats: Vec<FilterOrGroup>,
}

/// Result of [`create_presets`]: all presets plus the id of the active one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presets {
    pub presets: Vec<FilterPreset>,
    pub active: String,
}

impl Presets {
    pub fn active_preset(&self) -> Option<&FilterPreset> {
        self.presets.iter().find(|p| p.id == self.active)
    }
    pub fn active_preset_mut(&mut self) -> Option<&mut FilterPreset> {
        let active = self.active.clone();
        self.presets.iter_mut().find(|p| p.id == active)
    }
}

/// Upstream `SearchFilter` / `SearchFilterSub` (`sub` and `disabled` merged in).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_trade: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_type_trade: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discriminator_trade: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "serde_category")]
    pub category: Option<ItemCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<Box<SearchFilter>>,
    /// Only meaningful for `searchRelaxed` and `sub` (mutable in UI).
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueDisabled<T> {
    pub value: T,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Disabled {
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DisabledHidden {
    pub disabled: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoolValue {
    pub value: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CorruptedFilter {
    pub value: bool,
    pub twice: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MapBlighted {
    #[serde(rename = "Blighted")]
    Blighted,
    #[serde(rename = "Blight-ravaged")]
    BlightRavaged,
    /// upstream `false`
    #[serde(rename = "false")]
    No,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapBlightedFilter {
    #[serde(serialize_with = "ser_map_blighted", deserialize_with = "de_map_blighted")]
    pub value: MapBlighted,
}

fn ser_map_blighted<S: Serializer>(v: &MapBlighted, s: S) -> Result<S::Ok, S::Error> {
    match v {
        MapBlighted::Blighted => s.serialize_str("Blighted"),
        MapBlighted::BlightRavaged => s.serialize_str("Blight-ravaged"),
        MapBlighted::No => s.serialize_bool(false),
    }
}

fn de_map_blighted<'de, D: Deserializer<'de>>(d: D) -> Result<MapBlighted, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::String(s) if s == "Blighted" => Ok(MapBlighted::Blighted),
        serde_json::Value::String(s) if s == "Blight-ravaged" => Ok(MapBlighted::BlightRavaged),
        _ => Ok(MapBlighted::No),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCompletionRewardFilter {
    pub name: String,
    pub name_trade: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VeiledFilter {
    pub stat_refs: Vec<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InfluenceFilter {
    pub value: ItemInfluence,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollapseListings {
    #[default]
    Api,
    App,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeOptions {
    pub offline: bool,
    pub online_in_league: bool,
    pub merchant_only: bool,
    /// Only sellers you trade with in person (no instant buyout listings).
    #[serde(default)]
    pub in_person_only: bool,
    pub listed: Option<String>,
    pub currency: Option<String>,
    pub league: String,
    pub collapse_listings: CollapseListings,
    pub collapse_merchant: bool,
}

impl TradeOptions {
    /// The trade site's `status.option` for these seller choices.
    ///
    /// The site offers exactly these modes: `securable` (instant buyout),
    /// `available` (instant buyout and in person, online), `online` (in person,
    /// online) and `any` (everything, offline sellers too).
    pub fn status(&self) -> &'static str {
        if self.offline {
            "any"
        } else if self.merchant_only {
            "securable"
        } else if self.in_person_only {
            "online"
        } else {
            "available"
        }
    }

    /// Logic from `OnlineFilterCore.vue` `onOfflineUpdate`.
    pub fn set_offline(&mut self, offline: bool, api: crate::trade::TradeApi) {
        self.offline = offline;
        if api == crate::trade::TradeApi::Trade {
            self.listed = if offline { Some("2months".to_owned()) } else { None };
        }
    }

    /// Logic from `OnlineFilter.vue` `showWarning`.
    pub fn show_warning(&self) -> bool {
        self.listed.as_deref().is_some_and(|l| ["1day", "3days", "1week"].contains(&l))
            || self.currency.as_deref().is_some_and(|c| c != "chaos_divine")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilterNumeric {
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    pub disabled: bool,
}

impl FilterNumeric {
    pub fn new(value: f64, disabled: bool) -> Self {
        Self { value, max: None, disabled }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilters {
    pub search_exact: SearchFilter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_relaxed: Option<SearchFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rarity: Option<ValueDisabled<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_sockets: Option<FilterNumeric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_sockets: Option<FilterNumeric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrupted: Option<CorruptedFilter>,
    /// upstream `{ value: false }`; presence means "not fractured"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fractured: Option<BoolValue>,
    /// upstream `{ disabled: true }`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imbued_gem: Option<Disabled>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirrored: Option<DisabledHidden>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<DisabledHidden>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_catalysed: Option<Disabled>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foil: Option<Disabled>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foulborn: Option<BoolValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vestigial: Option<BoolValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub influences: Option<Vec<InfluenceFilter>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<FilterNumeric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gem_level: Option<FilterNumeric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_tier: Option<FilterNumeric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_blighted: Option<MapBlightedFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_completion_reward: Option<MapCompletionRewardFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_level: Option<FilterNumeric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_size: Option<FilterNumeric>,
    /// upstream `{ value: true, disabled }`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unidentified: Option<ValueDisabled<bool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub veiled: Option<VeiledFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_level: Option<FilterNumeric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentinel_charge: Option<FilterNumeric>,
    pub trade: TradeOptions,
}

impl ItemFilters {
    pub fn new(trade: TradeOptions) -> Self {
        Self {
            search_exact: SearchFilter::default(),
            search_relaxed: None,
            rarity: None,
            linked_sockets: None,
            white_sockets: None,
            corrupted: None,
            fractured: None,
            imbued_gem: None,
            mirrored: None,
            split: None,
            not_catalysed: None,
            foil: None,
            foulborn: None,
            vestigial: None,
            influences: None,
            quality: None,
            gem_level: None,
            map_tier: None,
            map_blighted: None,
            map_completion_reward: None,
            item_level: None,
            stack_size: None,
            unidentified: None,
            veiled: None,
            area_level: None,
            sentinel_charge: None,
            trade,
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
    Not,
    One,
    Mercenary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterGroup {
    pub group: GroupKind,
    /// mutable in UI
    pub expanded: bool,
    pub meta: StatFilter,
    pub stats: Vec<StatFilter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterOrGroup {
    Group(FilterGroup),
    Stat(StatFilter),
}

impl FilterOrGroup {
    pub fn as_stat(&self) -> Option<&StatFilter> {
        match self {
            FilterOrGroup::Stat(s) => Some(s),
            FilterOrGroup::Group(_) => None,
        }
    }
    pub fn as_group(&self) -> Option<&FilterGroup> {
        match self {
            FilterOrGroup::Group(g) => Some(g),
            FilterOrGroup::Stat(_) => None,
        }
    }
    /// `s.group ? !s.meta.disabled : !s.disabled`
    pub fn is_enabled(&self) -> bool {
        match self {
            FilterOrGroup::Group(g) => !g.meta.disabled,
            FilterOrGroup::Stat(s) => !s.disabled,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MercenaryFilterInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tier: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MinMax {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterRoll {
    pub value: f64,
    /// mutable in UI; `None` = empty input (upstream `'' | undefined`)
    pub min: Option<f64>,
    /// mutable in UI; `None` = empty input (upstream `'' | undefined`)
    pub max: Option<f64>,
    pub default: MinMax,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<MinMax>,
    #[serde(default)]
    pub trade_invert: bool,
    pub dp: bool,
    pub is_negated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goodness: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilterOption {
    /// mutable in UI
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatFilter {
    pub trade_id: Vec<String>,
    pub stat_ref: String,
    pub text: String,
    pub tag: FilterTag,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oils: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mercenary: Option<MercenaryFilterInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder_text: Option<String>,
    pub sources: Vec<StatSource>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub not: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll: Option<FilterRoll>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<FilterOption>,
    /// i18n key explaining why the filter is hidden
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<String>,
    /// mutable in UI
    pub disabled: bool,
}

impl StatFilter {
    /// Minimal filter with the given identity; other fields empty.
    pub fn bare(trade_id: Vec<String>, stat_ref: &str, text: &str, tag: FilterTag, disabled: bool) -> Self {
        Self {
            trade_id,
            stat_ref: stat_ref.to_owned(),
            text: text.to_owned(),
            tag,
            oils: None,
            mercenary: None,
            reminder_text: None,
            sources: Vec::new(),
            not: false,
            roll: None,
            option: None,
            hidden: None,
            disabled,
        }
    }

    pub fn first_trade_id(&self) -> &str {
        self.trade_id.first().map(String::as_str).unwrap_or("")
    }
}

pub const INTERNAL_TRADE_IDS: &[&str] = &[
    "item.not_group",
    "item.count_one_group",
    "item.base_percentile",
    "item.memory_strands",
    "item.armour",
    "item.evasion_rating",
    "item.energy_shield",
    "item.ward",
    "item.block",
    "item.total_dps",
    "item.physical_dps",
    "item.elemental_dps",
    "item.crit",
    "item.aps",
    "item.has_empty_modifier",
    "item.mercenary_6link",
    "item.map_item_quantity",
    "item.map_atlas_drops",
    "item.map_item_rarity",
    "item.map_pack_size",
    "item.heist_job_lockpicking",
    "item.heist_job_bruteforce",
    "item.heist_job_perception",
    "item.heist_job_demolition",
    "item.heist_job_counterthaumaturgy",
    "item.heist_job_trapdisarmament",
    "item.heist_job_agility",
    "item.heist_job_deception",
    "item.heist_job_engineering",
    "item.heist_target_priceless",
    "item.heist_wings_revealed",
    "item.heist_wings_total",
    "item.chart_sulphur",
];

pub fn is_internal_trade_id(id: &str) -> bool {
    INTERNAL_TRADE_IDS.contains(&id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ItemHasEmptyModifier {
    Any = 0,
    Prefix = 1,
    Suffix = 2,
}

/// Union of `ModifierType`, `ModifierMechanic` and extra tags (upstream `FilterTag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterTag {
    Mod(ModifierType),
    Mechanic(ModifierMechanic),
    Variant,
    Property,
    Brick,
    MercenaryPrimary,
    MercenarySecondary,
    MercenaryUtility,
    MercenarySupport,
    FilterGroup,
}

impl FilterTag {
    pub const PSEUDO: FilterTag = FilterTag::Mod(ModifierType::Pseudo);
    pub const EXPLICIT: FilterTag = FilterTag::Mod(ModifierType::Explicit);
    pub const IMPLICIT: FilterTag = FilterTag::Mod(ModifierType::Implicit);
    pub const ENCHANT: FilterTag = FilterTag::Mod(ModifierType::Enchant);
    pub const FRACTURED: FilterTag = FilterTag::Mod(ModifierType::Fractured);
    pub const FOULBORN: FilterTag = FilterTag::Mechanic(ModifierMechanic::Foulborn);
    pub const VESTIGIAL: FilterTag = FilterTag::Mechanic(ModifierMechanic::Vestigial);

    const MOD_TYPES: &'static [ModifierType] = &[
        ModifierType::Pseudo,
        ModifierType::Explicit,
        ModifierType::Implicit,
        ModifierType::Crafted,
        ModifierType::Enchant,
        ModifierType::Scourge,
        ModifierType::Necropolis,
        ModifierType::Veiled,
        ModifierType::Fractured,
        ModifierType::Imbued,
    ];
    const MECHANICS: &'static [ModifierMechanic] = &[
        ModifierMechanic::Synthesised,
        ModifierMechanic::Corruption,
        ModifierMechanic::Eldritch,
        ModifierMechanic::Foulborn,
        ModifierMechanic::Vestigial,
        ModifierMechanic::RareOnly,
        ModifierMechanic::Shaper,
        ModifierMechanic::Elder,
        ModifierMechanic::Crusader,
        ModifierMechanic::Hunter,
        ModifierMechanic::Redeemer,
        ModifierMechanic::Warlord,
        ModifierMechanic::Delve,
        ModifierMechanic::Unveiled,
        ModifierMechanic::Incursion,
        ModifierMechanic::Infamous,
        ModifierMechanic::Essence,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FilterTag::Mod(t) => t.as_str(),
            FilterTag::Mechanic(m) => m.as_str(),
            FilterTag::Variant => "variant",
            FilterTag::Property => "property",
            FilterTag::Brick => "brick",
            FilterTag::MercenaryPrimary => "mercenary-primary",
            FilterTag::MercenarySecondary => "mercenary-secondary",
            FilterTag::MercenaryUtility => "mercenary-utility",
            FilterTag::MercenarySupport => "mercenary-support",
            FilterTag::FilterGroup => "filter-group",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        if let Some(t) = Self::MOD_TYPES.iter().find(|t| t.as_str() == s) {
            return Some(FilterTag::Mod(*t));
        }
        if let Some(m) = Self::MECHANICS.iter().find(|m| m.as_str() == s) {
            return Some(FilterTag::Mechanic(*m));
        }
        Some(match s {
            "variant" => FilterTag::Variant,
            "property" => FilterTag::Property,
            "brick" => FilterTag::Brick,
            "mercenary-primary" => FilterTag::MercenaryPrimary,
            "mercenary-secondary" => FilterTag::MercenarySecondary,
            "mercenary-utility" => FilterTag::MercenaryUtility,
            "mercenary-support" => FilterTag::MercenarySupport,
            "filter-group" => FilterTag::FilterGroup,
            _ => return None,
        })
    }
}

impl From<ModifierType> for FilterTag {
    fn from(t: ModifierType) -> Self {
        FilterTag::Mod(t)
    }
}

impl From<ModifierMechanic> for FilterTag {
    fn from(m: ModifierMechanic) -> Self {
        FilterTag::Mechanic(m)
    }
}

impl Serialize for FilterTag {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FilterTag {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        FilterTag::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown filter tag: {s}")))
    }
}
