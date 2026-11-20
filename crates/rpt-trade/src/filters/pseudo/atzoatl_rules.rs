//! Port of upstream `filters/pseudo/atzoatl-rules.ts` (Chronicle of Atzoatl).

use rpt_parser::IncursionRoom;

use crate::filters::StatFilter;

struct FilterRule {
    ref_: &'static str,
    disabled: Option<bool>,
    remove: bool,
    hidden: Option<&'static str>,
    explosives_room: bool,
}

const HIDDEN_GREEDY: &str = "filters.hide_greedy";

const RULES: &[FilterRule] = &[
    FilterRule { ref_: "Has Room: Banquet Hall", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Antechamber", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Passageways", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Cloister", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Tunnels", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Cellar", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Chasm", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Halls", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Tombs", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Pits", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Shrine of Empowerment (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Sanctum of Unity (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Temple Nexus (Tier 3)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Explosives Room (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: true },
    FilterRule { ref_: "Has Room: Demolition Lab (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: true },
    FilterRule { ref_: "Has Room: Shrine of Unmaking (Tier 3)", disabled: None, remove: true, hidden: None, explosives_room: true },
    // TierApex
    FilterRule { ref_: "Has Room: Apex of Atzoatl", disabled: Some(false), remove: false, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Cradle of Creation (Tier 3)", disabled: Some(false), remove: false, hidden: None, explosives_room: false },
    // Tier0 (corrupt items)
    FilterRule { ref_: "Has Room: Corruption Chamber (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Catalyst of Corruption (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Locus of Corruption (Tier 3)", disabled: Some(false), remove: false, hidden: None, explosives_room: false },
    // Tier0 (corrupt gems)
    FilterRule { ref_: "Has Room: Gemcutter's Workshop (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Department of Thaumaturgy (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Doryani's Institute (Tier 3)", disabled: Some(false), remove: false, hidden: None, explosives_room: false },
    // Tier0 (sacrifice a unique item)
    FilterRule { ref_: "Has Room: Sacrificial Chamber (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Hall of Offerings (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Apex of Ascension (Tier 3)", disabled: Some(false), remove: false, hidden: None, explosives_room: false },
    // Tier1 (chests full of currency)
    FilterRule { ref_: "Has Room: Vault (Tier 1)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Treasury (Tier 2)", disabled: None, remove: false, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Wealth of the Vaal (Tier 3)", disabled: Some(false), remove: false, hidden: None, explosives_room: false },
    // Tier1 (corrupt maps)
    FilterRule { ref_: "Has Room: Surveyor's Study (Tier 1)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Office of Cartography (Tier 2)", disabled: None, remove: false, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Atlas of Worlds (Tier 3)", disabled: Some(false), remove: false, hidden: None, explosives_room: false },
    // Tier1 (Timeless Monolith)
    FilterRule { ref_: "Has Room: Hall of Mettle (Tier 1)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Hall of Heroes (Tier 2)", disabled: None, remove: false, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Hall of Legends (Tier 3)", disabled: None, remove: false, hidden: None, explosives_room: false },
    // Tier2 (chests full of items)
    FilterRule { ref_: "Has Room: Storage Room (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Warehouses (Tier 2)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Museum of Artefacts (Tier 3)", disabled: None, remove: false, hidden: None, explosives_room: false },
    // Tier2 (monster pack size)
    FilterRule { ref_: "Has Room: Guardhouse (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Barracks (Tier 2)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Hall of War (Tier 3)", disabled: None, remove: false, hidden: None, explosives_room: false },
    // Tier2 (item yield of the Temple)
    FilterRule { ref_: "Has Room: Workshop (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Engineering Department (Tier 2)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Factory (Tier 3)", disabled: None, remove: false, hidden: None, explosives_room: false },
    // Tier2 (Tormented Spirits)
    FilterRule { ref_: "Has Room: Torment Cells (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Torture Cages (Tier 2)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Sadist's Den (Tier 3)", disabled: None, remove: false, hidden: None, explosives_room: false },
    // Tier2 (Contains Strongboxes)
    FilterRule { ref_: "Has Room: Strongbox Chamber (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Hall of Locks (Tier 2)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Court of Sealed Death (Tier 3)", disabled: None, remove: false, hidden: None, explosives_room: false },
    // Tier2 (rarity of items dropped)
    FilterRule { ref_: "Has Room: Jeweller's Workshop (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Jewellery Forge (Tier 2)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: Glittering Halls (Tier 3)", disabled: None, remove: false, hidden: None, explosives_room: false },
    // Tier2 (Unstable Breaches)
    FilterRule { ref_: "Has Room: Anomaly Research Lab (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Breach Containment Chamber (Tier 2)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    FilterRule { ref_: "Has Room: House of the Others (Tier 3)", disabled: None, remove: false, hidden: None, explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Tempest Generator (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Hurricane Engine (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Storm of Corruption (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Armourer's Workshop (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Armoury (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Chamber of Iron (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Sparring Room (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Arena of Valour (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Hall of Champions (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Poison Garden (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Cultivar Chamber (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Toxic Grove (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Hatchery (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Automaton Lab (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Hybridisation Chamber (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Trap Workshop (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Temple Defense Workshop (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Defense Research Lab (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Flame Workshop (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Royal Forge (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Crucible of Flame (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Lightning Workshop (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Grand Reactor (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Conduit of Lightning (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // Tier3
    FilterRule { ref_: "Has Room: Pools of Restoration (Tier 1)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Sanctum of Vitality (Tier 2)", disabled: None, remove: true, hidden: None, explosives_room: false },
    FilterRule { ref_: "Has Room: Sanctum of Immortality (Tier 3)", disabled: None, remove: false, hidden: Some(HIDDEN_GREEDY), explosives_room: false },
    // search both Obstructed and Open rooms
];

pub fn used_stat_refs() -> Vec<&'static str> {
    RULES.iter().map(|r| r.ref_).collect()
}

const OPEN: f64 = IncursionRoom::Open as i32 as f64;
const OBSTRUCTED: f64 = IncursionRoom::Obstructed as i32 as f64;

pub fn apply_rules(filters: &mut Vec<StatFilter>) {
    for filter in filters.iter_mut() {
        filter.disabled = true;
    }

    let has_explosives = RULES.iter().filter(|r| r.explosives_room).any(|rule| {
        filters
            .iter()
            .any(|f| f.stat_ref == rule.ref_ && f.option.is_some_and(|o| o.value == OPEN))
    });

    for rule in RULES {
        let Some(index) = filters.iter().position(|f| f.stat_ref == rule.ref_) else { continue };

        let filter = &mut filters[index];
        let is_obstructed = filter.option.is_some_and(|o| o.value == OBSTRUCTED);
        if rule.remove || (is_obstructed && !has_explosives) {
            filters.remove(index);
            continue;
        }

        if let Some(h) = rule.hidden {
            filter.hidden = Some(h.to_owned());
        }
        if let Some(d) = rule.disabled {
            filter.disabled = d;
        }
        if is_obstructed {
            // search both Obstructed and Open rooms
            filter.option = None;
        }
    }
}
