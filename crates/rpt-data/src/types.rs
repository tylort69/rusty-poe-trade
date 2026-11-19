//! Serde mirrors of upstream `renderer/src/assets/data/interfaces.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatMatcher {
    pub string: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub negate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// `StatBetter` in upstream: -1 negative roll is better, 1 positive, 0 not comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "i8", into = "i8")]
pub enum StatBetter {
    NegativeRoll,
    PositiveRoll,
    NotComparable,
}
impl From<i8> for StatBetter {
    fn from(v: i8) -> Self {
        match v {
            -1 => Self::NegativeRoll,
            1 => Self::PositiveRoll,
            _ => Self::NotComparable,
        }
    }
}
impl From<StatBetter> for i8 {
    fn from(v: StatBetter) -> i8 {
        match v {
            StatBetter::NegativeRoll => -1,
            StatBetter::PositiveRoll => 1,
            StatBetter::NotComparable => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anointment {
    pub roll: f64,
    pub oils: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MercenaryStat {
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub supports: Option<Vec<String>>,
    #[serde(default)]
    pub tier: Option<u32>,
    #[serde(default)]
    pub max_tier: Option<bool>,
    #[serde(default)]
    pub canonical: Option<String>,
    #[serde(default)]
    pub synthetic_family: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JewelleryQuality {
    pub catalyst: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StatTrade {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inverted: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub option: bool,
    /// keyed by trade stat type: "explicit", "implicit", "pseudo", "fractured", ...
    pub ids: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    #[serde(rename = "ref")]
    pub ref_: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dp: bool,
    pub matchers: Vec<StatMatcher>,
    pub better: StatBetter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_family: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder_text: Option<String>,
    /// "yes" | "ubermap_exclusive" | "heist_exclusive"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_area_mods: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anointments: Option<Vec<Anointment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mercenary: Option<MercenaryStat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jewellery_quality: Option<JewelleryQuality>,
    pub trade: StatTrade,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "strat", rename_all = "kebab-case")]
pub enum StatGroupResolver {
    Select { test: Vec<Option<String>> },
    TrivialMerge,
    /// kind entries: "percent" | "value"
    PercentMerge { kind: Vec<String> },
    /// kind entries: "flag" | "value"
    FlagMerge { kind: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatGroup {
    pub resolve: StatGroupResolver,
    pub stats: Vec<Stat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StatOrGroup {
    Group(StatGroup),
    Stat(Stat),
}

impl StatOrGroup {
    pub fn stats(&self) -> &[Stat] {
        match self {
            StatOrGroup::Group(g) => &g.stats,
            StatOrGroup::Stat(s) => std::slice::from_ref(s),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DropEntry {
    pub query: Vec<String>,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Namespace {
    DivinationCard,
    CapturedBeast,
    Unique,
    Item,
    Gem,
    Area,
    MercenaryBuild,
}

impl Namespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Namespace::DivinationCard => "DIVINATION_CARD",
            Namespace::CapturedBeast => "CAPTURED_BEAST",
            Namespace::Unique => "UNIQUE",
            Namespace::Item => "ITEM",
            Namespace::Gem => "GEM",
            Namespace::Area => "AREA",
            Namespace::MercenaryBuild => "MERCENARY_BUILD",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatRef {
    #[serde(rename = "ref")]
    pub ref_: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseTypeDisc {
    #[serde(default, rename = "propAR")]
    pub prop_ar: bool,
    #[serde(default, rename = "propEV")]
    pub prop_ev: bool,
    #[serde(default, rename = "propES")]
    pub prop_es: bool,
    #[serde(default)]
    pub has_implicit: Option<StatRef>,
    #[serde(default)]
    pub has_explicit: Option<StatRef>,
    #[serde(default)]
    pub section_text: Option<String>,
    /// "W" | "Y" | "R"
    #[serde(default)]
    pub map_tier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Craftable {
    /// Matches `ItemCategory` string values (e.g. "Body Armour").
    pub category: String,
    #[serde(default)]
    pub corrupted: bool,
    #[serde(default)]
    pub unique_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UniqueInfo {
    pub base: String,
    #[serde(default)]
    pub fixed_stats: Option<Vec<String>>,
    #[serde(default)]
    pub disenchant_value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AreaInfo {
    #[serde(default)]
    pub screenshot: Option<String>,
    #[serde(default)]
    pub special: bool,
    #[serde(default)]
    pub blighted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GemInfo {
    #[serde(default)]
    pub vaal: bool,
    #[serde(default)]
    pub transfigured: bool,
    #[serde(default)]
    pub normal_variant: Option<String>,
    pub max_level: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ArmourInfo {
    #[serde(default)]
    pub ar: Option<[f64; 2]>,
    #[serde(default)]
    pub ev: Option<[f64; 2]>,
    #[serde(default)]
    pub es: Option<[f64; 2]>,
    #[serde(default)]
    pub ward: Option<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MercenarySkill {
    /// "primary" | "secondary" | "utility"
    #[serde(rename = "type")]
    pub type_: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MercenaryBuild {
    pub skills: Vec<MercenarySkill>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MercenaryBuildRef {
    RefName(String),
    Build(MercenaryBuild),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseType {
    pub name: String,
    pub ref_name: String,
    pub namespace: Namespace,
    pub icon: String,
    #[serde(default)]
    pub w: Option<u32>,
    #[serde(default)]
    pub h: Option<u32>,
    #[serde(default)]
    pub trade_tag: Option<String>,
    #[serde(default)]
    pub exchangeable: bool,
    #[serde(default)]
    pub trade_disc: Option<String>,
    #[serde(default)]
    pub disc: Option<BaseTypeDisc>,
    #[serde(default)]
    pub craftable: Option<Craftable>,
    #[serde(default)]
    pub unique: Option<UniqueInfo>,
    #[serde(default)]
    pub area: Option<AreaInfo>,
    #[serde(default)]
    pub gem: Option<GemInfo>,
    #[serde(default)]
    pub armour: Option<ArmourInfo>,
    #[serde(default)]
    pub mercenary_build: Option<MercenaryBuildRef>,
}
