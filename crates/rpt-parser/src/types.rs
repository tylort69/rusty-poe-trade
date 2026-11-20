//! Shared parser output types. Ports of upstream:
//! - `renderer/src/parser/ParsedItem.ts`
//! - `renderer/src/parser/meta.ts`
//! - `renderer/src/parser/modifiers.ts` (types only)
//! - `renderer/src/parser/advanced-mod-desc.ts` (types only)
//! - `renderer/src/parser/stat-translations.ts` (`ParsedStat`)
//!
//! These types are the contract between `rpt-parser`, `rpt-trade` and the UI.
//! Change them only with care.

use rpt_data::{BaseType, Stat, StatMatcher};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// meta.ts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemCategory {
    Map,
    CapturedBeast,
    MetamorphSample,
    Helmet,
    BodyArmour,
    Gloves,
    Boots,
    Shield,
    Amulet,
    Belt,
    Ring,
    Flask,
    AbyssJewel,
    Jewel,
    Quiver,
    Claw,
    Bow,
    Sceptre,
    Wand,
    FishingRod,
    Staff,
    Warstaff,
    Dagger,
    RuneDagger,
    OneHandedAxe,
    TwoHandedAxe,
    OneHandedMace,
    TwoHandedMace,
    OneHandedSword,
    TwoHandedSword,
    ClusterJewel,
    HeistBlueprint,
    HeistContract,
    HeistTool,
    HeistBrooch,
    HeistGear,
    HeistCloak,
    Trinket,
    Invitation,
    ExpeditionLogbook,
    Gem,
    Currency,
    DivinationCard,
    Voidstone,
    Sentinel,
    MemoryLine,
    SanctumRelic,
    Tincture,
    Charm,
    Idol,
    Graft,
    Chart,
    MercenaryWarrant,
    UpperLimbViscera,
    LowerLimbViscera,
    HeadViscera,
    TorsoViscera,
}

impl ItemCategory {
    pub const ALL: &'static [ItemCategory] = &[
        Self::Map, Self::CapturedBeast, Self::MetamorphSample, Self::Helmet, Self::BodyArmour,
        Self::Gloves, Self::Boots, Self::Shield, Self::Amulet, Self::Belt, Self::Ring, Self::Flask,
        Self::AbyssJewel, Self::Jewel, Self::Quiver, Self::Claw, Self::Bow, Self::Sceptre, Self::Wand,
        Self::FishingRod, Self::Staff, Self::Warstaff, Self::Dagger, Self::RuneDagger,
        Self::OneHandedAxe, Self::TwoHandedAxe, Self::OneHandedMace, Self::TwoHandedMace,
        Self::OneHandedSword, Self::TwoHandedSword, Self::ClusterJewel, Self::HeistBlueprint,
        Self::HeistContract, Self::HeistTool, Self::HeistBrooch, Self::HeistGear, Self::HeistCloak,
        Self::Trinket, Self::Invitation, Self::ExpeditionLogbook, Self::Gem, Self::Currency,
        Self::DivinationCard, Self::Voidstone, Self::Sentinel, Self::MemoryLine, Self::SanctumRelic,
        Self::Tincture, Self::Charm, Self::Idol, Self::Graft, Self::Chart, Self::MercenaryWarrant,
        Self::UpperLimbViscera, Self::LowerLimbViscera, Self::HeadViscera, Self::TorsoViscera,
    ];

    /// The upstream string value (also used by `BaseType.craftable.category`).
    pub fn as_str(self) -> &'static str {
        use ItemCategory::*;
        match self {
            Map => "Map",
            CapturedBeast => "Captured Beast",
            MetamorphSample => "Metamorph Sample",
            Helmet => "Helmet",
            BodyArmour => "Body Armour",
            Gloves => "Gloves",
            Boots => "Boots",
            Shield => "Shield",
            Amulet => "Amulet",
            Belt => "Belt",
            Ring => "Ring",
            Flask => "Flask",
            AbyssJewel => "Abyss Jewel",
            Jewel => "Jewel",
            Quiver => "Quiver",
            Claw => "Claw",
            Bow => "Bow",
            Sceptre => "Sceptre",
            Wand => "Wand",
            FishingRod => "Fishing Rod",
            Staff => "Staff",
            Warstaff => "Warstaff",
            Dagger => "Dagger",
            RuneDagger => "Rune Dagger",
            OneHandedAxe => "One-Handed Axe",
            TwoHandedAxe => "Two-Handed Axe",
            OneHandedMace => "One-Handed Mace",
            TwoHandedMace => "Two-Handed Mace",
            OneHandedSword => "One-Handed Sword",
            TwoHandedSword => "Two-Handed Sword",
            ClusterJewel => "Cluster Jewel",
            HeistBlueprint => "Heist Blueprint",
            HeistContract => "Heist Contract",
            HeistTool => "Heist Tool",
            HeistBrooch => "Heist Brooch",
            HeistGear => "Heist Gear",
            HeistCloak => "Heist Cloak",
            Trinket => "Trinket",
            Invitation => "Invitation",
            ExpeditionLogbook => "Expedition Logbook",
            Gem => "Gem",
            Currency => "Currency",
            DivinationCard => "Divination Card",
            Voidstone => "Voidstone",
            Sentinel => "Sentinel",
            MemoryLine => "Memory Line",
            SanctumRelic => "Sanctum Relic",
            Tincture => "Tincture",
            Charm => "Charm",
            Idol => "Idol",
            Graft => "Graft",
            Chart => "Chart",
            MercenaryWarrant => "Mercenary Warrant",
            UpperLimbViscera => "Upper Limb Viscera",
            LowerLimbViscera => "Lower Limb Viscera",
            HeadViscera => "Head Viscera",
            TorsoViscera => "Torso Viscera",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == s)
    }

    pub fn is_weapon_one_handed_melee(self) -> bool {
        use ItemCategory::*;
        matches!(self, OneHandedAxe | OneHandedMace | OneHandedSword | Claw | Dagger | RuneDagger | Sceptre)
    }
    pub fn is_weapon_one_handed(self) -> bool {
        self == ItemCategory::Wand || self.is_weapon_one_handed_melee()
    }
    pub fn is_weapon_two_handed_melee(self) -> bool {
        use ItemCategory::*;
        matches!(self, TwoHandedAxe | TwoHandedMace | TwoHandedSword | Staff | Warstaff)
    }
    pub fn is_weapon(self) -> bool {
        use ItemCategory::*;
        matches!(self, FishingRod | Bow) || self.is_weapon_one_handed() || self.is_weapon_two_handed_melee()
    }
    pub fn is_armour(self) -> bool {
        use ItemCategory::*;
        matches!(self, BodyArmour | Boots | Gloves | Helmet | Shield)
    }
    pub fn is_jewellery(self) -> bool {
        use ItemCategory::*;
        matches!(self, Amulet | Belt | Ring)
    }
    pub fn is_heist_equipment(self) -> bool {
        use ItemCategory::*;
        matches!(self, HeistTool | HeistBrooch | HeistGear | HeistCloak)
    }
    /// Upstream `MAGIC_ONLY_OR_UNIQUE_ITEM`.
    pub fn is_magic_only_or_unique(self) -> bool {
        use ItemCategory::*;
        matches!(self, Flask | Tincture | Voidstone | SanctumRelic | Charm)
    }
    /// Upstream `CONSUMABLE_CRAFTABLE_ITEM`.
    pub fn is_consumable_craftable(self) -> bool {
        use ItemCategory::*;
        matches!(self, Map | HeistBlueprint | HeistContract | Invitation | MemoryLine | ExpeditionLogbook | Chart)
    }
}

// ---------------------------------------------------------------------------
// modifiers.ts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModifierType {
    Pseudo,
    Explicit,
    Implicit,
    Crafted,
    Enchant,
    Scourge,
    Necropolis,
    Veiled,
    Fractured,
    Imbued,
}

impl ModifierType {
    /// The trade API stat type prefix (same as upstream enum value).
    pub fn as_str(self) -> &'static str {
        use ModifierType::*;
        match self {
            Pseudo => "pseudo",
            Explicit => "explicit",
            Implicit => "implicit",
            Crafted => "crafted",
            Enchant => "enchant",
            Scourge => "scourge",
            Necropolis => "necropolis",
            Veiled => "veiled",
            Fractured => "fractured",
            Imbued => "imbued",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModifierMechanic {
    Synthesised,
    Corruption,
    Eldritch,
    Foulborn,
    Vestigial,
    RareOnly,
    Shaper,
    Elder,
    Crusader,
    Hunter,
    Redeemer,
    Warlord,
    Delve,
    Unveiled,
    Incursion,
    Infamous,
    Essence,
}

impl ModifierMechanic {
    /// Upstream enum string value.
    pub fn as_str(self) -> &'static str {
        use ModifierMechanic::*;
        match self {
            Synthesised => "synthesised",
            Corruption => "corrupted",
            Eldritch => "eldritch",
            Foulborn => "foulborn",
            Vestigial => "vestigial",
            RareOnly => "explicit-rareonly",
            Shaper => "explicit-shaper",
            Elder => "explicit-elder",
            Crusader => "explicit-crusader",
            Hunter => "explicit-hunter",
            Redeemer => "explicit-redeemer",
            Warlord => "explicit-warlord",
            Delve => "explicit-delve",
            Unveiled => "explicit-veiled",
            Incursion => "explicit-incursion",
            Infamous => "explicit-infamous",
            Essence => "explicit-essence",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StatRoll {
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatSource {
    pub modifier: ParsedModifier,
    pub stat: ParsedStat,
    pub contributes: Option<StatRoll>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatCalculated {
    pub stat: Stat,
    #[serde(rename = "type")]
    pub type_: ModifierType,
    pub sources: Vec<StatSource>,
}

// ---------------------------------------------------------------------------
// advanced-mod-desc.ts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModGeneration {
    Prefix,
    Suffix,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModifierInfo {
    #[serde(rename = "type")]
    pub type_: ModifierType,
    pub generation: Option<ModGeneration>,
    pub mechanic: Option<ModifierMechanic>,
    pub name: Option<String>,
    pub tier: Option<u32>,
    pub rank: Option<u32>,
    pub tags: Vec<String>,
    pub roll_incr: Option<f64>,
}

impl ModifierInfo {
    pub fn new(type_: ModifierType) -> Self {
        Self { type_, generation: None, mechanic: None, name: None, tier: None, rank: None, tags: Vec::new(), roll_incr: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedModifier {
    pub info: ModifierInfo,
    pub stats: Vec<ParsedStat>,
}

// ---------------------------------------------------------------------------
// stat-translations.ts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RollGeneration {
    Legacy,
    Volatile,
    Reflecting,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedRoll {
    pub unscalable: bool,
    pub generation: Option<RollGeneration>,
    /// value has decimal places
    pub dp: bool,
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedStat {
    pub stat: Stat,
    pub translation: StatMatcher,
    pub roll: Option<ParsedRoll>,
}

// ---------------------------------------------------------------------------
// ParsedItem.ts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemRarity {
    Normal,
    Magic,
    Rare,
    Unique,
}

impl ItemRarity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Magic => "Magic",
            Self::Rare => "Rare",
            Self::Unique => "Unique",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemInfluence {
    Crusader,
    Elder,
    Hunter,
    Redeemer,
    Shaper,
    Warlord,
}

impl ItemInfluence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crusader => "Crusader",
            Self::Elder => "Elder",
            Self::Hunter => "Hunter",
            Self::Redeemer => "Redeemer",
            Self::Shaper => "Shaper",
            Self::Warlord => "Warlord",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeistTarget {
    Enchants,
    Trinkets,
    Gems,
    Replicas,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeistBlueprint {
    pub wings_revealed: Option<u32>,
    pub wings_total: Option<u32>,
    pub target: Option<HeistTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeistJob {
    Lockpicking,
    BruteForce,
    Perception,
    Demolition,
    CounterThaumaturgy,
    TrapDisarmament,
    Agility,
    Deception,
    Engineering,
}

impl HeistJob {
    /// Upstream string value (used on trade).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lockpicking => "Lockpicking",
            Self::BruteForce => "Brute Force",
            Self::Perception => "Perception",
            Self::Demolition => "Demolition",
            Self::CounterThaumaturgy => "Counter-Thaumaturgy",
            Self::TrapDisarmament => "Trap Disarmament",
            Self::Agility => "Agility",
            Self::Deception => "Deception",
            Self::Engineering => "Engineering",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeistContract {
    pub required_job: Option<HeistJob>,
    pub job_level: Option<u32>,
    /// only `Priceless` upstream
    pub target_value_priceless: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sockets {
    /// only 5 or 6
    pub linked: Option<u32>,
    pub white: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StackSize {
    pub value: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Corrupted {
    pub twice: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnknownModifier {
    pub text: String,
    #[serde(rename = "type")]
    pub type_: ModifierType,
}

/// NOTE: should match option values on trade
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncursionRoom {
    Open = 1,
    Obstructed = 2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedItem {
    pub rarity: Option<ItemRarity>,
    pub item_level: Option<u32>,

    pub armour_ar: Option<f64>,
    pub armour_ev: Option<f64>,
    pub armour_es: Option<f64>,
    pub armour_ward: Option<f64>,
    pub armour_block: Option<f64>,
    pub base_percentile: Option<f64>,
    pub weapon_crit: Option<f64>,
    pub weapon_as: Option<f64>,
    pub weapon_physical: Option<f64>,
    pub weapon_elemental: Option<f64>,

    pub map_area: Option<BaseType>,
    pub area_level: Option<u32>,
    pub area_item_quantity: Option<f64>,
    pub area_atlas_drops: Option<f64>,
    pub area_item_rarity: Option<f64>,
    pub area_pack_size: Option<f64>,
    pub map_completion_reward: Option<BaseType>,
    pub map_tier: Option<u32>,
    pub map_more_maps: Option<f64>,
    pub map_more_threads: Option<f64>,
    pub map_more_currency: Option<f64>,
    pub map_more_div_cards: Option<f64>,
    pub heist_blueprint: Option<HeistBlueprint>,
    pub heist_contract: Option<HeistContract>,
    pub logbook_area_mods: Option<Vec<Vec<ParsedModifier>>>,
    pub chart_sulphur: Option<f64>,

    pub gem_level: Option<u32>,
    pub vaal_gem: Option<BaseType>,
    pub imbued_gem: bool,
    pub mercenary_build: Option<BaseType>,
    pub mercenary_skills: Option<Vec<Vec<ParsedStat>>>,
    pub talisman_tier: Option<u32>,
    pub memory_strands: Option<u32>,
    pub quality: Option<f64>,
    pub sockets: Option<Sockets>,
    pub stack_size: Option<StackSize>,
    pub is_unidentified: bool,
    pub is_corrupted: Option<Corrupted>,
    pub is_unmodifiable: bool,
    pub is_mirrored: bool,
    pub is_split: bool,
    pub influences: Vec<ItemInfluence>,
    pub sentinel_charge: Option<u32>,
    pub is_synthesised: bool,
    pub is_fractured: bool,
    pub is_veiled: bool,
    pub is_foil: bool,
    pub is_foulborn: bool,
    pub is_vestigial: bool,
    pub dust_equivalent: Option<f64>,
    pub stats_by_type: Vec<StatCalculated>,
    pub new_mods: Vec<ParsedModifier>,
    pub unknown_modifiers: Vec<UnknownModifier>,
    pub category: Option<ItemCategory>,
    pub info: BaseType,
    pub unique_base: Option<BaseType>,
    pub raw_text: String,
}

impl ParsedItem {
    /// Upstream `createVirtualItem`: an item with only `info` set.
    pub fn virtual_item(info: BaseType) -> Self {
        Self {
            rarity: None,
            item_level: None,
            armour_ar: None,
            armour_ev: None,
            armour_es: None,
            armour_ward: None,
            armour_block: None,
            base_percentile: None,
            weapon_crit: None,
            weapon_as: None,
            weapon_physical: None,
            weapon_elemental: None,
            map_area: None,
            area_level: None,
            area_item_quantity: None,
            area_atlas_drops: None,
            area_item_rarity: None,
            area_pack_size: None,
            map_completion_reward: None,
            map_tier: None,
            map_more_maps: None,
            map_more_threads: None,
            map_more_currency: None,
            map_more_div_cards: None,
            heist_blueprint: None,
            heist_contract: None,
            logbook_area_mods: None,
            chart_sulphur: None,
            gem_level: None,
            vaal_gem: None,
            imbued_gem: false,
            mercenary_build: None,
            mercenary_skills: None,
            talisman_tier: None,
            memory_strands: None,
            quality: None,
            sockets: None,
            stack_size: None,
            is_unidentified: false,
            is_corrupted: None,
            is_unmodifiable: false,
            is_mirrored: false,
            is_split: false,
            influences: Vec::new(),
            sentinel_charge: None,
            is_synthesised: false,
            is_fractured: false,
            is_veiled: false,
            is_foil: false,
            is_foulborn: false,
            is_vestigial: false,
            dust_equivalent: None,
            stats_by_type: Vec::new(),
            new_mods: Vec::new(),
            unknown_modifiers: Vec::new(),
            category: None,
            info,
            unique_base: None,
            raw_text: "VIRTUAL_ITEM".to_owned(),
        }
    }
}
