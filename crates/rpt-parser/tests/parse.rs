//! Integration tests for `parse_clipboard` with realistic Ctrl+Alt+C item texts (English).

use rpt_parser::*;

fn parse(text: &str) -> ParsedItem {
    rpt_data::init("en").unwrap();
    parse_clipboard(text).unwrap_or_else(|e| panic!("failed to parse: {e}\n{text}"))
}

fn calc<'a>(item: &'a ParsedItem, ref_: &str, type_: ModifierType) -> &'a StatCalculated {
    item.stats_by_type
        .iter()
        .find(|c| c.stat.ref_ == ref_ && c.type_ == type_)
        .unwrap_or_else(|| {
            let have: Vec<_> = item.stats_by_type.iter().map(|c| (c.type_, c.stat.ref_.as_str())).collect();
            panic!("no {type_:?} stat {ref_:?}; have {have:#?}")
        })
}

fn roll(c: &StatCalculated) -> StatRoll {
    modifiers::stat_sources_total(&c.sources, modifiers::TotalMode::Sum).expect("stat has a roll")
}

fn assert_no_unknown(item: &ParsedItem) {
    assert!(item.unknown_modifiers.is_empty(), "unknown modifiers: {:#?}", item.unknown_modifiers);
}

const RARE_ARMOUR: &str = "Item Class: Body Armours
Rarity: Rare
Carrion Shell
Glorious Plate
--------
Quality: +20% (augmented)
Armour: 1772 (augmented)
--------
Requirements:
Level: 68
Str: 191
--------
Sockets: R-R-R-R-R B
--------
Item Level: 86
--------
{ Fractured Prefix Modifier \"Fortified\" (Tier: 4) — Defences }
+62(61-80) to Armour
{ Prefix Modifier \"Hale\" (Tier: 8) — Life }
+29(25-29) to maximum Life
{ Prefix Modifier \"Impenetrable\" (Tier: 3) — Defences }
62(56-67)% increased Armour
{ Suffix Modifier \"of the Volcano\" (Tier: 3) — Elemental, Fire, Resistance }
+40(36-41)% to Fire Resistance
{ Suffix Modifier \"of the Titan\" (Tier: 4) — Attribute }
+38(38-42) to Strength
{ Master Crafted Suffix Modifier \"of Craft\" (Rank: 2) — Elemental, Cold, Resistance }
+24(21-28)% to Cold Resistance
--------
Fractured Item
--------
Warlord Item
";

#[test]
fn rare_armour() {
    let item = parse(RARE_ARMOUR);
    assert_eq!(item.rarity, Some(ItemRarity::Rare));
    assert_eq!(item.category, Some(ItemCategory::BodyArmour));
    assert_eq!(item.info.ref_name, "Glorious Plate");
    assert_eq!(item.armour_ar, Some(1772.0));
    assert_eq!(item.quality, Some(20.0));
    assert_eq!(item.item_level, Some(86));
    assert_eq!(item.sockets, Some(Sockets { linked: Some(5), white: 0 }));
    assert_eq!(item.influences, vec![ItemInfluence::Warlord]);
    assert!(item.is_fractured);
    assert!(item.is_corrupted.is_none());
    assert_no_unknown(&item);

    assert_eq!(item.new_mods.len(), 6);
    let fractured = &item.new_mods[0];
    assert_eq!(fractured.info.type_, ModifierType::Fractured);
    assert_eq!(fractured.info.generation, Some(ModGeneration::Prefix));
    assert_eq!(fractured.info.name.as_deref(), Some("Fortified"));
    assert_eq!(fractured.info.tier, Some(4));
    assert_eq!(fractured.info.tags, vec!["Defences"]);
    let crafted = &item.new_mods[5];
    assert_eq!(crafted.info.type_, ModifierType::Crafted);
    assert_eq!(crafted.info.generation, Some(ModGeneration::Suffix));
    assert_eq!(crafted.info.rank, Some(2));
    assert_eq!(crafted.info.tags, vec!["Elemental", "Cold", "Resistance"]);

    let life = roll(calc(&item, "+# to maximum Life", ModifierType::Explicit));
    assert_eq!((life.value, life.min, life.max), (29.0, 25.0, 29.0));
    let fire = roll(calc(&item, "+#% to Fire Resistance", ModifierType::Explicit));
    assert_eq!(fire.value, 40.0);
    calc(&item, "+# to Armour", ModifierType::Fractured);
    calc(&item, "+#% to Cold Resistance", ModifierType::Crafted);
    calc(&item, "+# to Strength", ModifierType::Explicit);
    let incr = roll(calc(&item, "#% increased Armour", ModifierType::Explicit));
    assert_eq!(incr.value, 62.0);

    // 1772 / 1.2 (quality) / 1.62 (incr) - 62 (flat) = ~849.5 in [776, 892]
    assert_eq!(item.base_percentile, Some(63.0));
}

#[test]
fn rare_ring_variant_and_corruption() {
    let item = parse(
        "Item Class: Rings
Rarity: Rare
Dread Loop
Two-Stone Ring
--------
Requirements:
Level: 36
--------
Item Level: 84
--------
{ Implicit Modifier — Elemental, Cold, Lightning, Resistance }
+14(12-16)% to Cold and Lightning Resistances
--------
{ Prefix Modifier \"Hale\" (Tier: 8) — Life }
+14(10-19) to maximum Life
{ Suffix Modifier \"of the Gale\" (Tier: 5) — Attack, Speed }
+12(11-12) to Dexterity
--------
Corrupted
--------
Searing Exarch Item
Eater of Worlds Item
",
    );
    assert_eq!(item.category, Some(ItemCategory::Ring));
    assert_eq!(item.info.ref_name, "Two-Stone Ring");
    // variant picked by implicit (TopazSapphire icon)
    assert!(item.info.icon.contains("TopazSapphire"), "{}", item.info.icon);
    assert_eq!(item.is_corrupted, Some(Corrupted { twice: false }));
    let imp = roll(calc(&item, "+#% to Cold and Lightning Resistances", ModifierType::Implicit));
    assert_eq!((imp.value, imp.min, imp.max), (14.0, 12.0, 16.0));
    assert_eq!(item.new_mods[0].info.type_, ModifierType::Implicit);
    assert_eq!(item.new_mods[0].info.generation, None);
    assert_no_unknown(&item);
}

#[test]
fn unique_weapon() {
    let item = parse(
        "Item Class: Two Hand Swords
Rarity: Unique
Starforge
Infernal Sword
--------
Two Hand Sword
Physical Damage: 366-621 (augmented)
Critical Strike Chance: 5.00%
Attacks per Second: 1.37 (augmented)
Weapon Range: 1.3 metres
--------
Requirements:
Level: 67
Str: 113
Dex: 113
--------
Sockets: R-R-R-G-G-B
--------
Item Level: 85
--------
{ Implicit Modifier — Damage, Elemental, Attack }
30% increased Elemental Damage with Attack Skills
--------
{ Unique Modifier — Damage, Physical, Attack }
477(400-500)% increased Physical Damage
{ Unique Modifier — Attack, Speed }
6(5-8)% increased Attack Speed
{ Unique Modifier — Life }
+95(90-100) to maximum Life
{ Unique Modifier }
Your Physical Damage can Shock
{ Unique Modifier — Elemental, Damage }
Deal no Elemental Damage
--------
Shaper Item
",
    );
    assert_eq!(item.rarity, Some(ItemRarity::Unique));
    assert_eq!(item.category, Some(ItemCategory::TwoHandedSword));
    assert_eq!(item.info.ref_name, "Starforge");
    assert_eq!(item.unique_base.as_ref().map(|b| b.ref_name.as_str()), Some("Infernal Sword"));
    assert_eq!(item.weapon_physical, Some(493.5));
    assert_eq!(item.weapon_crit, Some(5.0));
    assert_eq!(item.weapon_as, Some(1.37));
    assert_eq!(item.sockets, Some(Sockets { linked: Some(6), white: 0 }));
    assert_eq!(item.influences, vec![ItemInfluence::Shaper]);
    let phys = roll(calc(&item, "#% increased Physical Damage", ModifierType::Explicit));
    assert_eq!((phys.value, phys.min, phys.max), (477.0, 400.0, 500.0));
    calc(&item, "Your Physical Damage can Shock", ModifierType::Explicit);
    calc(&item, "#% increased Elemental Damage with Attack Skills", ModifierType::Implicit);
    // ilvl 85 -> 5 * (50 + 44 + 6 + 400) = 2500; +50% for one influence
    let expected = (703.49f64 * 2500.0 * 1.5).floor();
    assert_eq!(item.dust_equivalent, Some(expected));
    assert_no_unknown(&item);
}

#[test]
fn currency_stack() {
    let item = parse(
        "Item Class: Stackable Currency
Rarity: Currency
Chaos Orb
--------
Stack Size: 2,448/5,000
--------
Reforges a rare item with new random modifiers
--------
Right click this item then left click a rare item to apply it.
Shift click to unstack.
",
    );
    assert_eq!(item.category, Some(ItemCategory::Currency));
    assert_eq!(item.rarity, None);
    assert_eq!(item.info.ref_name, "Chaos Orb");
    assert_eq!(item.stack_size, Some(StackSize { value: 2448.0, max: 5000.0 }));
    assert!(item.new_mods.is_empty());
    assert_no_unknown(&item);
}

#[test]
fn divination_card() {
    let item = parse(
        "Item Class: Divination Cards
Rarity: Divination Card
The Doctor
--------
Stack Size: 3/8
--------
Headhunter
Leather Belt
--------
A surgeon's skill is a gift.
",
    );
    assert_eq!(item.category, Some(ItemCategory::DivinationCard));
    assert_eq!(item.info.ref_name, "The Doctor");
    assert_eq!(item.info.namespace, rpt_data::Namespace::DivinationCard);
    assert_eq!(item.stack_size, Some(StackSize { value: 3.0, max: 8.0 }));
}

#[test]
fn transfigured_gem() {
    let item = parse(
        "Item Class: Skill Gems
Rarity: Gem
Lightning Arrow of Electrocution
--------
Attack, AoE, Projectile, Lightning, Bow
Level: 20 (Max)
Cost: 7 Mana
Attack Speed: 100% of base
Effectiveness of Added Damage: 135%
Quality: +20% (augmented)
--------
Requirements:
Level: 70
Dex: 155
--------
Fires an arrow which converts physical damage to lightning damage.
--------
Experience: 1/1
--------
Place into an item socket of the right colour to gain this skill. Right click to remove from a socket.
",
    );
    assert_eq!(item.category, Some(ItemCategory::Gem));
    assert_eq!(item.info.ref_name, "Lightning Arrow of Electrocution");
    assert!(item.info.gem.as_ref().unwrap().transfigured);
    assert_eq!(item.gem_level, Some(20));
    assert_eq!(item.quality, Some(20.0));
    assert!(item.vaal_gem.is_none());
}

#[test]
fn vaal_gem_corrupted() {
    let item = parse(
        "Item Class: Skill Gems
Rarity: Gem
Vaal Double Strike
--------
Vaal, Attack, Melee, Strike, Duration, Physical
Level: 21
Cost: 5 Mana
Quality: +23% (augmented)
--------
Requirements:
Level: 72
Dex: 155
--------
Performs two fast strikes with a melee weapon.
--------
Vaal Double Strike
--------
Souls Per Use: 30
Can Store 1 Use
Soul Gain Prevention: 4 sec
--------
Performs two fast strikes with a melee weapon. Each strike creates a clone.
--------
Experience: 1/1
--------
Corrupted
",
    );
    assert_eq!(item.info.ref_name, "Vaal Double Strike");
    assert_eq!(item.gem_level, Some(21));
    assert_eq!(item.quality, Some(23.0));
    assert_eq!(item.vaal_gem.as_ref().map(|g| g.ref_name.as_str()), Some("Vaal Double Strike"));
    assert_eq!(item.is_corrupted, Some(Corrupted { twice: false }));
}

#[test]
fn map_with_tier_and_area_stats() {
    let item = parse(
        "Item Class: Maps
Rarity: Rare
Grim Halls
Map (Tier 16)
--------
Map Area: Cemetery
Item Quantity: +80% (augmented)
Item Rarity: +45% (augmented)
Monster Pack Size: +30% (augmented)
More Currency: +10% (augmented)
--------
Item Level: 84
--------
Monster Level: 83
--------
{ Prefix Modifier \"Abhorrent\" (Tier: 1) }
Area is inhabited by Abominations
{ Prefix Modifier \"Fecund\" (Tier: 1) — Life }
40(40-49)% more Monster Life
{ Suffix Modifier \"of Smothering\" (Tier: 1) }
Players have 60% less Recovery Rate of Life and Energy Shield
--------
Travel to this Map by using it in a personal Map Device. Maps can only be used once.
",
    );
    assert_eq!(item.category, Some(ItemCategory::Map));
    assert_eq!(item.info.ref_name, "Map");
    assert_eq!(item.map_tier, Some(16));
    assert_eq!(item.map_area.as_ref().map(|a| a.ref_name.as_str()), Some("Cemetery"));
    assert_eq!(item.area_item_quantity, Some(80.0));
    assert_eq!(item.area_item_rarity, Some(45.0));
    assert_eq!(item.area_pack_size, Some(30.0));
    assert_eq!(item.map_more_currency, Some(10.0));
    assert_eq!(item.item_level, Some(84));
    calc(&item, "Area is inhabited by Abominations", ModifierType::Explicit);
    let life = roll(calc(&item, "#% more Monster Life", ModifierType::Explicit));
    assert_eq!(life.value, 40.0);
    // negated matcher
    let rec = roll(calc(&item, "Players have #% less Recovery Rate of Life and Energy Shield", ModifierType::Explicit));
    assert_eq!(rec.value, -60.0);
    assert_no_unknown(&item);
}

#[test]
fn unidentified_unique() {
    let item = parse(
        "Item Class: Body Armours
Rarity: Unique
Simple Robe
--------
Energy Shield: 17
--------
Requirements:
Level: 1
--------
Sockets: W-W-W-W-W-W
--------
Item Level: 46
--------
Unidentified
",
    );
    assert!(item.is_unidentified);
    assert_eq!(item.rarity, Some(ItemRarity::Unique));
    assert_eq!(item.category, Some(ItemCategory::BodyArmour));
    assert_eq!(item.info.ref_name, "Simple Robe");
    assert_eq!(item.armour_es, Some(17.0));
    assert_eq!(item.sockets, Some(Sockets { linked: Some(6), white: 6 }));
    assert!(item.dust_equivalent.is_none());

    rpt_data::init("en").unwrap();
    let data = rpt_data::data();
    let tabula = data.item_by_ref(rpt_data::Namespace::Unique, "Tabula Rasa").unwrap()[0].clone();
    let identified = make_identified_unique(&tabula, &item);
    assert_eq!(identified.info.ref_name, "Tabula Rasa");
    assert_eq!(identified.unique_base.as_ref().unwrap().ref_name, "Simple Robe");
    // ilvl 46 -> 5 * 50 = 250
    assert_eq!(identified.dust_equivalent, Some((2.12f64 * 250.0).floor()));
}

#[test]
fn superior_normal_item() {
    let item = parse(
        "Item Class: Body Armours
Rarity: Normal
Superior Glorious Plate
--------
Quality: +12% (augmented)
Armour: 950 (augmented)
--------
Item Level: 80
",
    );
    assert_eq!(item.rarity, Some(ItemRarity::Normal));
    assert_eq!(item.info.ref_name, "Glorious Plate");
    assert_eq!(item.quality, Some(12.0));
}

#[test]
fn cluster_jewel() {
    let item = parse(
        "Item Class: Jewels
Rarity: Rare
Kraken Spark
Large Cluster Jewel
--------
Requirements:
Level: 54
--------
Item Level: 84
--------
Adds 8 Passive Skills (enchant)
2 Added Passive Skills are Jewel Sockets (enchant)
Added Small Passive Skills grant: 12% increased Fire Damage (enchant)
--------
{ Prefix Modifier \"Notable\" (Tier: 1) }
1 Added Passive Skill is Burning Bright
{ Prefix Modifier \"Notable\" (Tier: 1) }
1 Added Passive Skill is Smoking Remains
{ Suffix Modifier \"of Potency\" (Tier: 2) }
Added Small Passive Skills also grant: +6(5-7) to Dexterity
--------
Place into an allocated Large Jewel Socket on the Passive Skill Tree. Right click to remove from the Socket.
",
    );
    assert_eq!(item.category, Some(ItemCategory::ClusterJewel));
    assert_eq!(item.info.ref_name, "Large Cluster Jewel");
    let passives = roll(calc(&item, "Adds # Passive Skills", ModifierType::Enchant));
    assert_eq!(passives.value, 8.0);
    calc(&item, "# Added Passive Skills are Jewel Sockets", ModifierType::Enchant);
    calc(&item, "1 Added Passive Skill is Burning Bright", ModifierType::Explicit);
    calc(&item, "1 Added Passive Skill is Smoking Remains", ModifierType::Explicit);
    calc(&item, "Added Small Passive Skills grant: 12% increased Fire Damage", ModifierType::Enchant);
    let dex = roll(calc(&item, "Added Small Passive Skills also grant: +# to Dexterity", ModifierType::Explicit));
    assert_eq!((dex.value, dex.min, dex.max), (6.0, 5.0, 7.0));
    assert!(item.new_mods.iter().any(|m| m.info.type_ == ModifierType::Enchant));
    assert_no_unknown(&item);
}

#[test]
fn magic_flask() {
    let item = parse(
        "Item Class: Life Flasks
Rarity: Magic
Seething Divine Life Flask of Staunching
--------
Quality: +20% (augmented)
Recovers 1440 (augmented) Life over 0.30 (augmented) Seconds
Consumes 15 of 45 Charges on use
Currently has 45 Charges
--------
Requirements:
Level: 60
--------
Item Level: 84
--------
{ Prefix Modifier \"Seething\" (Tier: 1) — Life }
66% reduced Amount Recovered
Instant Recovery
{ Suffix Modifier \"of Staunching\" (Tier: 1) — Life, Ailment }
Grants Immunity to Bleeding for 6 seconds if used while Bleeding
Grants Immunity to Corrupted Blood for 6 seconds if used while affected by Corrupted Blood
--------
Right click to drink. Can only hold charges while in belt. Refills as you kill monsters.
",
    );
    assert_eq!(item.rarity, Some(ItemRarity::Magic));
    assert_eq!(item.category, Some(ItemCategory::Flask));
    assert_eq!(item.info.ref_name, "Divine Life Flask");
    assert_eq!(item.quality, Some(20.0));
    assert_eq!(item.new_mods.len(), 2);
    assert_eq!(item.new_mods[0].info.generation, Some(ModGeneration::Prefix));
    assert!(!item.new_mods[0].stats.is_empty());
    assert_no_unknown(&item);
}

#[test]
fn heist_contract() {
    let item = parse(
        "Item Class: Contracts
Rarity: Normal
Contract: Smuggler's Den
--------
Client: Hard Hendrick
Heist Target: Golden Idol (Priceless)
Area Level: 83
Requires Lockpicking (Level 5)
--------
Item Level: 84
--------
Area has patrolling Guards
--------
Take this item to the Rogue Harbour to start a Heist.
",
    );
    assert_eq!(item.category, Some(ItemCategory::HeistContract));
    assert_eq!(item.area_level, Some(83));
    let c = item.heist_contract.as_ref().unwrap();
    assert_eq!(c.required_job, Some(HeistJob::Lockpicking));
    assert_eq!(c.job_level, Some(5));
    assert!(c.target_value_priceless);
}

#[test]
fn heist_blueprint() {
    let item = parse(
        "Item Class: Blueprints
Rarity: Normal
Blueprint: Bunker
--------
Heist Target: Replicas or Experimented Items
Area Level: 83
Wings Revealed: 2/4
Escape Routes Revealed: 4/9
Reward Rooms Revealed: 10/25
--------
Item Level: 83
",
    );
    assert_eq!(item.category, Some(ItemCategory::HeistBlueprint));
    let b = item.heist_blueprint.as_ref().unwrap();
    assert_eq!(b.target, Some(HeistTarget::Replicas));
    assert_eq!(b.wings_revealed, Some(2));
    assert_eq!(b.wings_total, Some(4));
}

#[test]
fn expedition_logbook() {
    let item = parse(
        "Item Class: Expedition Logbooks
Rarity: Normal
Expedition Logbook
--------
Area Level: 81
--------
Item Level: 83
--------
Battleground Graves
Druids of the Broken Circle
Area contains Medved, Feller of Heroes (implicit)
Area contains an additional Underground Area (implicit)
--------
Dried Riverbed
Knights of the Sun
Area contains 2 additional Chest Markers (implicit)
--------
Take this item to Dannig in your hideout to embark on an Expedition.
",
    );
    assert_eq!(item.category, Some(ItemCategory::ExpeditionLogbook));
    assert_eq!(item.area_level, Some(81));
    let areas = item.logbook_area_mods.as_ref().unwrap();
    assert_eq!(areas.len(), 2);
    assert_eq!(areas[0][0].stats[0].stat.ref_, "Has Logbook Faction: Druids of the Broken Circle");
    assert_eq!(areas[0].len(), 2, "only NotComparable area mods are kept");
    assert_eq!(areas[0][1].info.type_, ModifierType::Implicit);
    assert_eq!(areas[0][1].stats[0].stat.ref_, "Area contains Medved, Feller of Heroes");
    assert_eq!(areas[1][0].stats[0].stat.ref_, "Has Logbook Faction: Knights of the Sun");
}

#[test]
fn twice_corrupted_with_corruption_implicit() {
    let item = parse(
        "Item Class: Amulets
Rarity: Rare
Rune Locket
Onyx Amulet
--------
Requirements:
Level: 60
--------
Item Level: 85
--------
{ Corruption Implicit Modifier — Attribute }
+12(10-16) to all Attributes
--------
{ Prefix Modifier \"Hale\" (Tier: 8) — Life }
+14(10-19) to maximum Life
--------
Twice Corrupted
",
    );
    assert_eq!(item.category, Some(ItemCategory::Amulet));
    assert_eq!(item.is_corrupted, Some(Corrupted { twice: true }));
    assert_eq!(item.new_mods[0].info.type_, ModifierType::Implicit);
    assert_eq!(item.new_mods[0].info.mechanic, Some(ModifierMechanic::Corruption));
    calc(&item, "+# to all Attributes", ModifierType::Implicit);
    assert_no_unknown(&item);
}

#[test]
fn eldritch_implicits_and_synthesised() {
    let item = parse(
        "Item Class: Helmets
Rarity: Rare
Mind Crown
Synthesised Hubris Circlet
--------
Energy Shield: 120 (augmented)
--------
Item Level: 85
--------
{ Searing Exarch Implicit Modifier (Greater) — Damage }
12(11-13)% increased Damage
--------
{ Prefix Modifier \"Hale\" (Tier: 8) — Life }
+14(10-19) to maximum Life
--------
Synthesised Item
",
    );
    assert!(item.is_synthesised);
    assert_eq!(item.info.ref_name, "Hubris Circlet");
    let imp = &item.new_mods[0];
    assert_eq!(imp.info.type_, ModifierType::Implicit);
    assert_eq!(imp.info.mechanic, Some(ModifierMechanic::Eldritch));
    assert_eq!(imp.info.rank, Some(2));
    assert_no_unknown(&item);
}

#[test]
fn errors() {
    rpt_data::init("en").unwrap();
    assert_eq!(parse_clipboard("").unwrap_err(), "item.parse_error");
    assert_eq!(parse_clipboard("hello world").unwrap_err(), "item.parse_error");
    assert_eq!(
        parse_clipboard("Item Class: Rings\nRarity: Rare\nFoo\nNot A Real Base\n--------\nItem Level: 1").unwrap_err(),
        "item.unknown"
    );
}

#[test]
fn advanced_mod_desc_helpers() {
    use rpt_parser::advanced_mod_desc::*;
    rpt_data::init("en").unwrap();
    let info = parse_mod_info_line("{ Prefix Modifier \"Hale\" (Tier: 8) — Life — 20% Increased }").unwrap();
    assert_eq!(info.type_, ModifierType::Explicit);
    assert_eq!(info.tier, Some(8));
    assert_eq!(info.tags, vec!["Life"]);
    assert_eq!(info.roll_incr, Some(20.0));
    let info = parse_mod_info_line("{ Implicit Modifier — 20% Increased }").unwrap();
    assert_eq!(info.type_, ModifierType::Implicit);
    assert!(info.tags.is_empty());
    assert_eq!(info.roll_incr, Some(20.0));
    assert!(is_mod_info_line("{ Unique Modifier }"));
    let (t, lines) = parse_mod_type(&["Foo (enchant)".to_owned()]).unwrap();
    assert_eq!(t, ModifierType::Enchant);
    assert_eq!(lines, vec!["Foo"]);
    assert!(parse_mod_type(&["Foo".to_owned()]).is_err());
}

#[test]
fn foulborn_volatile_and_cannot_use() {
    let item = parse(
        "Item Class: Two Hand Swords
Rarity: Unique
Foulborn Starforge
Infernal Sword
You cannot use this item. Its stats will be ignored
--------
Two Hand Sword
Physical Damage: 366-621 (augmented)
--------
Item Level: 85
--------
{ Unique Modifier — Damage, Physical, Attack }
520(400-500)% increased Physical Damage
{ Foulborn Unique Modifier — Life }
+95(90-100) to maximum Life
--------
Corrupted
",
    );
    assert!(item.is_foulborn);
    assert_eq!(item.info.ref_name, "Starforge");
    assert_eq!(item.category, Some(ItemCategory::TwoHandedSword));
    let phys = &item.new_mods[0].stats[0];
    let r = phys.roll.as_ref().unwrap();
    // legacy roll on a corrupted unique becomes "volatile"
    assert_eq!(r.generation, Some(RollGeneration::Volatile));
    assert_eq!((r.min, r.max), (400.0, 520.0));
    assert_eq!(item.new_mods[1].info.mechanic, Some(ModifierMechanic::Foulborn));
    assert_no_unknown(&item);
}
