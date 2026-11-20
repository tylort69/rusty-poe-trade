//! Port of upstream `renderer/src/parser/Parser.ts`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::LazyLock;

use regex::Regex;
use rpt_data::{BaseType, GameData, MercenaryBuildRef, Namespace, StatBetter, StatMatcher};

use crate::advanced_mod_desc::{
    ENCHANT_LINE, IMPLICIT_LINE, SCOURGE_LINE, group_lines_by_mod, is_mod_info_line, parse_mod_info_line_with,
    parse_mod_type,
};
use crate::js;
use crate::magic_name::magic_basetype_with;
use crate::modifiers::{TotalMode, stat_sources_total, sum_stats_by_mod_type};
use crate::stat_translations::{
    StatString, get_roll_or_minmax_avg, lines_to_stat_strings_with, try_parse_translation_with,
};
use crate::types::*;

const PARSE_ERROR: &str = "item.parse_error";
const ITEM_UNKNOWN: &str = "item.unknown";
const VALUE_AUGMENTED: &str = " (augmented)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionParseResult {
    SectionParsed,
    SectionSkipped,
    ParserSkipped,
}
use SectionParseResult::*;

/// `Err` = upstream `throw` (always surfaces as `item.parse_error`).
type SectionResult = Result<SectionParseResult, String>;
type ParserFn = fn(&[String], &mut ParserState<'_>) -> SectionResult;
/// `Err` = upstream `return err(...)` or a `throw`.
type VirtualParserFn = fn(&mut ParserState<'_>) -> Result<(), String>;

struct ParserState<'d> {
    data: &'d GameData,
    item: ParsedItem,
    name: String,
    base_type: Option<String>,
    info_variants: Vec<BaseType>,
}

enum Parser {
    Section(ParserFn),
    Virtual(VirtualParserFn),
}
use Parser::{Section as S, Virtual as V};

static PARSERS: &[Parser] = &[
    S(parse_unidentified),
    V(parse_superior),
    V(parse_foulborn),
    V(parse_vestigial),
    S(parse_synthesised),
    S(parse_category_by_help_text),
    V(parse_map_tier),
    V(normalize_name),
    V(find_in_database),
    // -----------
    S(parse_item_level),
    S(parse_talisman_tier),
    S(parse_gem),
    S(parse_vaal_gem),
    S(parse_armour),
    S(parse_weapon),
    S(parse_accessory),
    S(parse_flask),
    S(parse_tincture),
    S(parse_stack_size),
    S(parse_corrupted),
    S(parse_imbued_gem),
    S(parse_foil),
    S(parse_influence),
    S(parse_map),
    S(parse_sockets),
    S(parse_heist_contract),
    S(parse_heist_blueprint),
    S(parse_chart),
    S(parse_area_level),
    S(parse_atzoatl_rooms),
    S(parse_mirrored_tablet),
    S(parse_filled_coffin),
    S(parse_mirrored),
    S(parse_split),
    S(parse_sentinel_charge),
    S(parse_scrying_orb),
    S(parse_mercenary),
    S(parse_viscera),
    S(parse_logbook_area),
    S(parse_logbook_area),
    S(parse_logbook_area),
    S(parse_mercenary_gems),
    S(parse_mercenary_gems),
    S(parse_mercenary_gems),
    S(parse_mercenary_gems),
    S(parse_mercenary_gems),
    S(parse_mercenary_gems),
    S(parse_modifiers), // enchant
    S(parse_modifiers), // scourge
    S(parse_modifiers), // implicit
    S(parse_modifiers), // explicit
    V(augment_modifiers),
    V(transform_to_legacy_modifiers),
    V(parse_fractured),
    V(pick_correct_variant),
    V(calc_disenchant_dust_v),
    V(calc_base_percentile),
];

/// Upstream `parseClipboard`. Returns `Err(i18n_key)` (`item.parse_error` or
/// `item.unknown`) for items that cannot be parsed.
pub fn parse_clipboard(clipboard: &str) -> Result<ParsedItem, String> {
    let data = rpt_data::data();
    // upstream wraps everything in try/catch; any unexpected failure is a parse error
    catch_unwind(AssertUnwindSafe(|| parse_clipboard_inner(&data, clipboard)))
        .unwrap_or_else(|_| Err(PARSE_ERROR.to_owned()))
}

fn parse_clipboard_inner(data: &GameData, clipboard: &str) -> Result<ParsedItem, String> {
    let cs = &data.client_strings;
    let mut sections = item_text_to_sections(clipboard);
    if sections.is_empty() {
        return Err(PARSE_ERROR.to_owned());
    }

    if sections[0].get(2) == Some(&cs.CANNOT_USE_ITEM) {
        sections[0].pop(); // remove CANNOT_USE_ITEM line
        if sections.len() < 2 {
            return Err(PARSE_ERROR.to_owned());
        }
        let first = sections.remove(0); // remove first section where CANNOT_USE_ITEM line was
        sections[0].splice(0..0, first); // prepend item class & rarity into second section
    }
    let mut state = parse_name_plate(data, &sections[0])?;
    sections.remove(0);
    state.item.raw_text = clipboard.to_owned();

    // each section can be parsed at most by one parser
    for parser in PARSERS {
        match parser {
            Parser::Virtual(f) => f(&mut state)?,
            Parser::Section(f) => {
                let mut i = 0;
                while i < sections.len() {
                    // workaround upstream bug
                    parse_eldritch_item(&mut sections[i], data);

                    match f(&sections[i], &mut state).map_err(|_| PARSE_ERROR.to_owned())? {
                        SectionParsed => {
                            sections.remove(i);
                            break;
                        }
                        ParserSkipped => break,
                        SectionSkipped => i += 1,
                    }
                }
            }
        }
    }
    Ok(state.item)
}

fn item_text_to_sections(text: &str) -> Vec<Vec<String>> {
    let mut lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }

    let mut sections: Vec<Vec<String>> = vec![Vec::new()];
    for line in lines {
        if line != "--------" {
            sections.last_mut().unwrap().push(line.to_owned());
        } else {
            sections.push(Vec::new());
        }
    }
    sections.into_iter().filter(|s| !s.is_empty()).collect()
}

// ---------------------------------------------------------------------------
// Data lookups with upstream index semantics

/// Upstream `ITEM_BY_TRANSLATED`. Upstream's index scan stops after the first
/// record that has neither `disc` nor `unique`; mirror that.
pub(crate) fn item_by_translated(data: &GameData, ns: Namespace, name: &str) -> Option<Vec<BaseType>> {
    data.item_by_translated(ns, name).map(take_variants)
}

/// Upstream `ITEM_BY_REF` (same scan semantics as [`item_by_translated`]).
pub(crate) fn item_by_ref(data: &GameData, ns: Namespace, ref_name: &str) -> Option<Vec<BaseType>> {
    data.item_by_ref(ns, ref_name).map(take_variants)
}

fn take_variants(all: Vec<&BaseType>) -> Vec<BaseType> {
    let mut out = Vec::new();
    for record in all {
        out.push(record.clone());
        if record.disc.is_none() && record.unique.is_none() {
            break;
        }
    }
    out
}

fn first_line(section: &[String]) -> &str {
    section.first().map_or("", String::as_str)
}

/// `str.slice(n)` with `n` in UTF-16 code units.
fn js_slice_from(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, ch) in s.char_indices() {
        if units >= n {
            return &s[i..];
        }
        units += ch.len_utf16();
    }
    ""
}

fn truthy(v: Option<f64>) -> bool {
    v.is_some_and(|v| v != 0.0 && !v.is_nan())
}

fn placeholder_info() -> BaseType {
    BaseType {
        name: String::new(),
        ref_name: String::new(),
        namespace: Namespace::Item,
        icon: String::new(),
        w: None,
        h: None,
        trade_tag: None,
        exchangeable: false,
        trade_disc: None,
        disc: None,
        craftable: None,
        unique: None,
        area: None,
        gem: None,
        armour: None,
        mercenary_build: None,
    }
}

// ---------------------------------------------------------------------------
// Virtual parsers

fn normalize_name(st: &mut ParserState<'_>) -> Result<(), String> {
    let cs = &st.data.client_strings;
    if st.item.rarity == Some(ItemRarity::Magic)
        && let Some(base_type) = magic_basetype_with(st.data, &st.name) {
            st.name = base_type;
        }

    if st.item.category == Some(ItemCategory::MetamorphSample) {
        if cs.METAMORPH_BRAIN.is_match(&st.name) {
            st.name = "Metamorph Brain".to_owned();
        } else if cs.METAMORPH_EYE.is_match(&st.name) {
            st.name = "Metamorph Eye".to_owned();
        } else if cs.METAMORPH_LUNG.is_match(&st.name) {
            st.name = "Metamorph Lung".to_owned();
        } else if cs.METAMORPH_HEART.is_match(&st.name) {
            st.name = "Metamorph Heart".to_owned();
        } else if cs.METAMORPH_LIVER.is_match(&st.name) {
            st.name = "Metamorph Liver".to_owned();
        }
    }
    Ok(())
}

fn find_in_database(st: &mut ParserState<'_>) -> Result<(), String> {
    let d = st.data;
    let base_or_name = st.base_type.as_deref().unwrap_or(&st.name);
    let info = match st.item.category {
        Some(ItemCategory::DivinationCard) => item_by_translated(d, Namespace::DivinationCard, &st.name),
        Some(ItemCategory::CapturedBeast) => item_by_translated(d, Namespace::CapturedBeast, base_or_name),
        Some(ItemCategory::Gem) => item_by_translated(d, Namespace::Gem, &st.name),
        Some(ItemCategory::MetamorphSample) => item_by_translated(d, Namespace::Item, &st.name),
        Some(ItemCategory::Voidstone) => item_by_ref(d, Namespace::Item, "Charged Compass"),
        _ if st.item.rarity == Some(ItemRarity::Unique) && !st.item.is_unidentified => {
            item_by_translated(d, Namespace::Unique, &st.name)
        }
        _ => item_by_translated(d, Namespace::Item, base_or_name),
    };
    let mut info = match info {
        Some(info) if !info.is_empty() => info,
        _ => return Err(ITEM_UNKNOWN.to_owned()),
    };
    if info[0].unique.is_some() {
        // `ITEM_BY_TRANSLATED('ITEM', item.baseType!)`
        let base_types = st
            .base_type
            .as_deref()
            .and_then(|bt| item_by_translated(d, Namespace::Item, bt))
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ITEM_UNKNOWN.to_owned())?;

        let base_type_ref = base_types[0].ref_name.clone();
        info.retain(|info| info.unique.as_ref().is_some_and(|u| u.base == base_type_ref));
    }
    // choose 1st variant, correct one will be picked at the end of parsing
    st.item.info = info.first().cloned().ok_or_else(|| PARSE_ERROR.to_owned())?;
    st.info_variants = info;
    // same for every variant
    if st.item.category.is_none() {
        if let Some(craftable) = &st.item.info.craftable {
            st.item.category = ItemCategory::from_str(&craftable.category);
        } else if let Some(unique) = &st.item.info.unique {
            let base = item_by_ref(d, Namespace::Item, &unique.base)
                .and_then(|v| v.into_iter().next())
                .and_then(|b| b.craftable)
                .ok_or_else(|| PARSE_ERROR.to_owned())?;
            st.item.category = ItemCategory::from_str(&base.category);
        }
    }
    Ok(())
}

/// Upstream `makeIdentifiedUnique`.
pub fn make_identified_unique(unique_info: &BaseType, unidentified: &ParsedItem) -> ParsedItem {
    let mut new_item = unidentified.clone();
    new_item.info = unique_info.clone();
    new_item.unique_base = Some(unidentified.info.clone());

    calc_disenchant_dust(&mut new_item);

    new_item
}

fn parse_map_tier(st: &mut ParserState<'_>) -> Result<(), String> {
    let cs = &st.data.client_strings;
    let use_base = st.base_type.as_deref().is_some_and(|b| !b.is_empty());
    let target = if use_base { st.base_type.clone().unwrap() } else { st.name.clone() };
    let Some(caps) = cs.MAP_TIER.captures(&target) else { return Ok(()) };

    st.item.map_tier = caps.get(1).and_then(|m| js::to_u32(js::number(m.as_str())));

    let whole = caps.get(0).unwrap().as_str();
    let replaced = target.replacen(whole, "", 1);
    if use_base {
        st.base_type = Some(replaced);
    } else {
        st.name = replaced;
    }
    Ok(())
}

fn parse_fractured(st: &mut ParserState<'_>) -> Result<(), String> {
    if st.item.new_mods.iter().any(|m| m.info.type_ == ModifierType::Fractured) {
        st.item.is_fractured = true;
    }
    Ok(())
}

fn pick_correct_variant(st: &mut ParserState<'_>) -> Result<(), String> {
    let picked = pick_variant(&st.info_variants, &st.item).or_else(|| st.info_variants.first());
    st.item.info = picked.cloned().ok_or_else(|| PARSE_ERROR.to_owned())?;
    if let Some(unique) = &st.item.info.unique {
        let base_variants = item_by_ref(st.data, Namespace::Item, &unique.base)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| PARSE_ERROR.to_owned())?;
        let base = pick_variant(&base_variants, &st.item).unwrap_or(&base_variants[0]).clone();
        st.item.unique_base = Some(base);
    }
    Ok(())
}

fn pick_variant<'v>(variants: &'v [BaseType], item: &ParsedItem) -> Option<&'v BaseType> {
    if variants.len() <= 1 {
        return variants.first();
    }

    let default_disc = rpt_data::BaseTypeDisc::default();
    'variants: for variant in variants {
        let cond = variant.disc.as_ref().unwrap_or(&default_disc);

        if cond.prop_ar && !truthy(item.armour_ar) {
            continue;
        }
        if cond.prop_ev && !truthy(item.armour_ev) {
            continue;
        }
        if cond.prop_es && !truthy(item.armour_es) {
            continue;
        }

        if let Some(tier) = cond.map_tier.as_deref() {
            let Some(map_tier) = item.map_tier.filter(|t| *t != 0) else { continue };
            match tier {
                "W" if map_tier > 5 => continue 'variants,
                "Y" if !(6..=10).contains(&map_tier) => continue 'variants,
                "R" if map_tier < 11 => continue 'variants,
                _ => {}
            }
        }

        if let Some(has) = &cond.has_implicit
            && !item
                .stats_by_type
                .iter()
                .any(|c| c.type_ == ModifierType::Implicit && c.stat.ref_ == has.ref_)
            {
                continue;
            }

        if let Some(has) = &cond.has_explicit
            && !item
                .stats_by_type
                .iter()
                .any(|c| c.type_ == ModifierType::Explicit && c.stat.ref_ == has.ref_)
            {
                continue;
            }

        if let Some(text) = cond.section_text.as_deref().filter(|t| !t.is_empty())
            && !item.raw_text.contains(text) {
                continue;
            }

        return Some(variant);
    }

    // it may happen that we don't find correct variant
    // i.e. corrupted implicit on Two-Stone Ring
    None
}

fn parse_superior(st: &mut ParserState<'_>) -> Result<(), String> {
    let cs = &st.data.client_strings;
    let unid = st.item.is_unidentified;
    let applies = match st.item.rarity {
        Some(ItemRarity::Normal) => true,
        Some(ItemRarity::Magic | ItemRarity::Rare | ItemRarity::Unique) => unid,
        None => false,
    };
    if applies
        && let Some(name) = cs.ITEM_SUPERIOR.group1(&st.name) {
            st.name = name.to_owned();
        }
    Ok(())
}

fn parse_foulborn(st: &mut ParserState<'_>) -> Result<(), String> {
    if st.item.rarity != Some(ItemRarity::Unique) || st.item.is_unidentified {
        return Ok(());
    }
    if let Some(name) = st.data.client_strings.FOULBORN_NAME.group1(&st.name) {
        st.name = name.to_owned();
        st.item.is_foulborn = true;
    }
    Ok(())
}

fn parse_vestigial(st: &mut ParserState<'_>) -> Result<(), String> {
    if st.item.rarity != Some(ItemRarity::Unique) {
        return Ok(());
    }
    let Some(base_type) = st.base_type.as_deref().filter(|b| !b.is_empty()) else { return Ok(()) };
    if let Some(bt) = st.data.client_strings.VESTIGIAL_NAME.group1(base_type) {
        st.base_type = Some(bt.to_owned());
        st.item.is_vestigial = true;
    }
    Ok(())
}

fn augment_modifiers(st: &mut ParserState<'_>) -> Result<(), String> {
    let item = &mut st.item;
    let unique_corrupted = item.rarity == Some(ItemRarity::Unique) && item.is_corrupted.is_some();
    let reflecting = item.rarity == Some(ItemRarity::Rare)
        && item.is_mirrored
        && matches!(item.category, Some(ItemCategory::Ring | ItemCategory::Amulet));
    for m in &mut item.new_mods {
        if item.is_synthesised && m.info.type_ == ModifierType::Implicit && m.info.mechanic.is_none() {
            m.info.mechanic = Some(ModifierMechanic::Synthesised);
        }

        for stat in &mut m.stats {
            let Some(roll) = &mut stat.roll else { continue };
            if roll.generation != Some(RollGeneration::Legacy) || roll.unscalable {
                continue;
            }
            if unique_corrupted {
                roll.generation = Some(RollGeneration::Volatile);
            } else if reflecting {
                roll.generation = Some(RollGeneration::Reflecting);
            }
        }
    }
    Ok(())
}

/// Upstream `transformToLegacyModifiers` (deprecated upstream, but still the
/// source of `statsByType`).
fn transform_to_legacy_modifiers(st: &mut ParserState<'_>) -> Result<(), String> {
    st.item.stats_by_type = sum_stats_by_mod_type(&st.item.new_mods);
    Ok(())
}

// --- subset of calc-q20.ts needed for `basePercentile` (the full module lives in rpt-trade)

struct PropStatRefs {
    flat: &'static [&'static str],
    incr: &'static [&'static str],
}

const Q_ARMOUR: PropStatRefs = PropStatRefs {
    flat: &["+# to Armour"],
    incr: &[
        "#% increased Armour",
        "#% increased Armour and Energy Shield",
        "#% increased Armour and Evasion",
        "#% increased Armour, Evasion and Energy Shield",
    ],
};
const Q_EVASION: PropStatRefs = PropStatRefs {
    flat: &["+# to Evasion Rating"],
    incr: &[
        "#% increased Evasion Rating",
        "#% increased Armour and Evasion",
        "#% increased Evasion and Energy Shield",
        "#% increased Armour, Evasion and Energy Shield",
    ],
};
const Q_ENERGY_SHIELD: PropStatRefs = PropStatRefs {
    flat: &["+# to maximum Energy Shield"],
    incr: &[
        "#% increased Energy Shield",
        "#% increased Armour and Energy Shield",
        "#% increased Evasion and Energy Shield",
        "#% increased Armour, Evasion and Energy Shield",
    ],
};
const Q_WARD: PropStatRefs = PropStatRefs { flat: &["+# to Ward"], incr: &["#% increased Ward"] };

const QUALITY_CHANGING_ENCHANT: &[&str] =
    &["Quality does not increase Defences", "Quality does not increase Physical Damage"];

fn get_prop_quality(item: &ParsedItem) -> f64 {
    if item.stats_by_type.iter().any(|c| QUALITY_CHANGING_ENCHANT.contains(&c.stat.ref_.as_str())) {
        return 0.0;
    }
    item.quality.unwrap_or(0.0)
}

fn calc_prop_percentile(total: f64, bounds: [f64; 2], refs: &PropStatRefs, item: &ParsedItem) -> Option<f64> {
    let mut incr = 0.0;
    let mut flat = 0.0;
    for calc in &item.stats_by_type {
        let r = calc.stat.ref_.as_str();
        let target = if refs.flat.contains(&r) {
            &mut flat
        } else if refs.incr.contains(&r) {
            &mut incr
        } else {
            continue;
        };
        if let Some(roll) = stat_sources_total(&calc.sources, TotalMode::Sum) {
            *target += roll.value;
        }
    }
    // We cannot know original roll if the `morePct` or `incrPct` is equal to -100,
    // in that case a NaN will be returned.
    let more_pct = get_prop_quality(item);
    let roll = total / (1.0 + more_pct / 100.0) / (1.0 + incr / 100.0) - flat;
    let [min, max] = bounds;
    let result = js::math_round(((roll - min) / (max - min)) * 100.0);
    if result.is_nan() { None } else { Some(result.clamp(0.0, 100.0)) }
}

fn calc_base_percentile(st: &mut ParserState<'_>) -> Result<(), String> {
    let item = &st.item;
    let Some(info) = item
        .unique_base
        .as_ref()
        .and_then(|b| b.armour.as_ref())
        .or(item.info.armour.as_ref())
    else {
        return Ok(());
    };

    // Base percentile is the same for all defences.
    // Using `AR/EV -> ES -> WARD` order to improve accuracy
    // of calculation (larger rolls = more precise).
    let pct = if let (true, Some(b)) = (truthy(item.armour_ar), info.ar) {
        calc_prop_percentile(item.armour_ar.unwrap(), b, &Q_ARMOUR, item)
    } else if let (true, Some(b)) = (truthy(item.armour_ev), info.ev) {
        calc_prop_percentile(item.armour_ev.unwrap(), b, &Q_EVASION, item)
    } else if let (true, Some(b)) = (truthy(item.armour_es), info.es) {
        calc_prop_percentile(item.armour_es.unwrap(), b, &Q_ENERGY_SHIELD, item)
    } else if let (true, Some(b)) = (truthy(item.armour_ward), info.ward) {
        calc_prop_percentile(item.armour_ward.unwrap(), b, &Q_WARD, item)
    } else {
        return Ok(());
    };
    st.item.base_percentile = pct;
    Ok(())
}

fn calc_disenchant_dust_v(st: &mut ParserState<'_>) -> Result<(), String> {
    calc_disenchant_dust(&mut st.item);
    Ok(())
}

fn calc_disenchant_dust(item: &mut ParsedItem) {
    let Some(disenchant_value) = item.info.unique.as_ref().and_then(|u| u.disenchant_value).filter(|v| *v != 0.0)
    else {
        return;
    };

    let mut increase_by_factors = 0.0;

    // +50% per Influence Type
    increase_by_factors += item.influences.len() as f64 * 50.0;

    // +2% per 1% Item Quality
    if let Some(q) = item.quality.filter(|q| *q != 0.0) {
        increase_by_factors += q * 2.0;
    }

    // +50% per Corruption Implicit
    if item.is_corrupted.is_some() {
        for m in &item.new_mods {
            if m.info.mechanic == Some(ModifierMechanic::Corruption) {
                increase_by_factors += 50.0;
            }
        }
    }

    // `Math.max(undefined, 46)` is NaN upstream
    let Some(ilvl) = item.item_level.map(f64::from) else {
        item.dust_equivalent = None;
        return;
    };

    let factors_multi = (increase_by_factors + 100.0) / 100.0;
    let term1 = 50.0; // ilvl 46 and below
    let term2 = 2.0 * (ilvl.clamp(46.0, 68.0) - 46.0); // ilvl 47 to 68
    let term3 = (3.0 * (ilvl.clamp(46.0, 68.0) - 46.0) / 11.0).floor(); // ilvl 47 to 68
    let term4 = 25.0 * (ilvl.clamp(68.0, 84.0) - 68.0); // ilvl 69 to 84
    let total_multi = 5.0 * (term1 + term2 + term3 + term4) * factors_multi;

    item.dust_equivalent = Some((disenchant_value * total_multi).floor());
}

// ---------------------------------------------------------------------------
// Name plate

fn parse_name_plate<'d>(data: &'d GameData, section: &[String]) -> Result<ParserState<'d>, String> {
    let cs = &data.client_strings;
    let mut lines = section.iter();

    let mut line = lines.next();
    if !line.is_some_and(|l| l.starts_with(&cs.ITEM_CLASS)) {
        return Err(PARSE_ERROR.to_owned());
    }

    line = lines.next();
    let mut rarity_text: Option<&str> = None;
    if let Some(l) = line.filter(|l| l.starts_with(&cs.RARITY)) {
        rarity_text = Some(&l[cs.RARITY.len()..]);
        line = lines.next();
    }

    let name = match line {
        Some(l) => markup_condition_parser(l),
        None => return Err(PARSE_ERROR.to_owned()),
    };

    let base_type = lines.next().map(|l| markup_condition_parser(l));

    let mut item = ParsedItem::virtual_item(placeholder_info());
    item.raw_text = String::new();

    if let Some(r) = rarity_text {
        if r == cs.RARITY_CURRENCY {
            item.category = Some(ItemCategory::Currency);
        } else if r == cs.RARITY_DIVCARD {
            item.category = Some(ItemCategory::DivinationCard);
        } else if r == cs.RARITY_GEM {
            item.category = Some(ItemCategory::Gem);
        } else if r == cs.RARITY_NORMAL || r == cs.RARITY_QUEST {
            item.rarity = Some(ItemRarity::Normal);
        } else if r == cs.RARITY_MAGIC {
            item.rarity = Some(ItemRarity::Magic);
        } else if r == cs.RARITY_RARE {
            item.rarity = Some(ItemRarity::Rare);
        } else if r == cs.RARITY_UNIQUE {
            item.rarity = Some(ItemRarity::Unique);
        }
    }

    Ok(ParserState { data, item, name, base_type, info_variants: Vec::new() })
}

static MARKUP_SET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<<set:.+?>>").unwrap());
static MARKUP_COND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(if:.+?|elif:.+?|else)>\{(.+?)\}").unwrap());

fn markup_condition_parser(text: &str) -> String {
    // ignores state set by <<set:__>>
    // always evaluates first condition to true <if:__>{...}
    // full markup: https://gist.github.com/SnosMe/151549b532df8ea08025a76ae2920ca4

    let text = MARKUP_SET.replace_all(text, "");
    MARKUP_COND
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            if caps[1].starts_with("if:") { caps[2].to_owned() } else { String::new() }
        })
        .into_owned()
}

// ---------------------------------------------------------------------------
// Section parsers

fn parse_area_prop_nested(line: &str, item: &mut ParsedItem, data: &GameData) -> bool {
    let cs = &data.client_strings;
    if let Some(rest) = line.strip_prefix(cs.MAP_ITEM_QUANTITY.as_str()) {
        item.area_item_quantity = js::non_nan(js::parse_int(rest));
        true
    } else if let Some(rest) = line.strip_prefix(cs.MAP_ATLAS_DROPS.as_str()) {
        item.area_atlas_drops = js::non_nan(js::parse_int(rest));
        true
    } else if let Some(rest) = line.strip_prefix(cs.MAP_ITEM_RARITY.as_str()) {
        item.area_item_rarity = js::non_nan(js::parse_int(rest));
        true
    } else if let Some(rest) = line.strip_prefix(cs.MAP_MONSTER_PACK_SIZE.as_str()) {
        item.area_pack_size = js::non_nan(js::parse_int(rest));
        true
    } else {
        false
    }
}

fn parse_map(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::Map) {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    let cs = &d.client_strings;

    let mut is_parsed = SectionSkipped;

    for line in section {
        if parse_area_prop_nested(line, &mut st.item, d) {
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.MAP_MORE_MAPS.as_str()) {
            st.item.map_more_maps = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.MAP_MORE_THREADS.as_str()) {
            st.item.map_more_threads = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.MAP_MORE_CURRENCY.as_str()) {
            st.item.map_more_currency = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.MAP_MORE_DIVINATION_CARDS.as_str()) {
            st.item.map_more_div_cards = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        } else if line.starts_with(&cs.MAP_AREA) {
            // NOTE: upstream reads the area name from the first line of the section
            let area_name = js_slice_from(first_line(section), js::js_len(&cs.MAP_AREA));
            let area_info = item_by_translated(d, Namespace::Area, area_name)
                .ok_or_else(|| "Unknown Area name.".to_owned())?;
            st.item.map_area = area_info.into_iter().next();
            is_parsed = SectionParsed;
        } else if let Some(reward_name) = cs.MAP_COMPLETION_REWARD.group1(line) {
            let reward_info = item_by_translated(d, Namespace::Unique, reward_name)
                .ok_or_else(|| "Unknown Unique Item.".to_owned())?;
            st.item.map_completion_reward = reward_info.into_iter().next();
            is_parsed = SectionParsed;
        }
    }

    Ok(is_parsed)
}

fn parse_influence(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let cs = &st.data.client_strings;
    if section.len() <= 2 {
        let count_before = st.item.influences.len();

        for line in section {
            let infl = if *line == cs.INFLUENCE_CRUSADER {
                Some(ItemInfluence::Crusader)
            } else if *line == cs.INFLUENCE_ELDER {
                Some(ItemInfluence::Elder)
            } else if *line == cs.INFLUENCE_SHAPER {
                Some(ItemInfluence::Shaper)
            } else if *line == cs.INFLUENCE_HUNTER {
                Some(ItemInfluence::Hunter)
            } else if *line == cs.INFLUENCE_REDEEMER {
                Some(ItemInfluence::Redeemer)
            } else if *line == cs.INFLUENCE_WARLORD {
                Some(ItemInfluence::Warlord)
            } else {
                None
            };
            if let Some(infl) = infl {
                st.item.influences.push(infl);
            }
        }

        if count_before < st.item.influences.len() {
            return Ok(SectionParsed);
        }
    }
    Ok(SectionSkipped)
}

fn parse_corrupted(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let cs = &st.data.client_strings;
    let first = first_line(section);
    if first == cs.CORRUPTED {
        st.item.is_corrupted = Some(Corrupted { twice: false });
        Ok(SectionParsed)
    } else if first == cs.TWICE_CORRUPTED {
        st.item.is_corrupted = Some(Corrupted { twice: true });
        Ok(SectionParsed)
    } else if first == cs.UNMODIFIABLE {
        st.item.is_corrupted = Some(Corrupted { twice: false });
        st.item.is_unmodifiable = true;
        Ok(SectionParsed)
    } else {
        Ok(SectionSkipped)
    }
}

fn parse_foil(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.rarity != Some(ItemRarity::Unique) {
        return Ok(ParserSkipped);
    }
    if first_line(section) == st.data.client_strings.FOIL_UNIQUE {
        st.item.is_foil = true;
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_unidentified(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if first_line(section) == st.data.client_strings.UNIDENTIFIED {
        st.item.is_unidentified = true;
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_item_level(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let cs = &st.data.client_strings;
    let prefix = if st.item.info.ref_name == "Filled Coffin" { &cs.CORPSE_LEVEL } else { &cs.ITEM_LEVEL };

    for line in section {
        if let Some(rest) = line.strip_prefix(prefix.as_str()) {
            st.item.item_level = js::to_u32(js::number(rest));
            return Ok(SectionParsed);
        }
    }
    Ok(SectionSkipped)
}

fn parse_talisman_tier(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let cs = &st.data.client_strings;
    if let Some(rest) = first_line(section).strip_prefix(cs.TALISMAN_TIER.as_str()) {
        st.item.talisman_tier = js::to_u32(js::number(rest));
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_vaal_gem(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::Gem) {
        return Ok(ParserSkipped);
    }
    if section.len() == 1
        && let Some(gem_info) = item_by_translated(st.data, Namespace::Gem, &section[0]) {
            st.item.vaal_gem = gem_info.into_iter().next();
            return Ok(SectionParsed);
        }
    Ok(SectionSkipped)
}

fn parse_gem(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::Gem) {
        return Ok(ParserSkipped);
    }
    let cs = &st.data.client_strings;
    if let Some(rest) = section.get(1).and_then(|l| l.strip_prefix(cs.GEM_LEVEL.as_str())) {
        // "Level: 20 (Max)"
        st.item.gem_level = js::to_u32(js::parse_int(rest));

        parse_quality_nested(section, &mut st.item, st.data);

        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_imbued_gem(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::Gem) {
        return Ok(ParserSkipped);
    }
    if section.len() == 1 {
        let Some((stat, matcher)) = st.data.stat_by_match_str(&section[0]) else {
            return Ok(SectionSkipped);
        };

        st.item.new_mods.push(ParsedModifier {
            info: ModifierInfo::new(ModifierType::Imbued),
            stats: vec![ParsedStat { stat: stat.clone(), translation: matcher.clone(), roll: None }],
        });
        st.item.imbued_gem = true;

        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_stack_size(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.rarity != Some(ItemRarity::Normal)
        && st.item.category != Some(ItemCategory::Currency)
        && st.item.category != Some(ItemCategory::DivinationCard)
    {
        return Ok(ParserSkipped);
    }
    let cs = &st.data.client_strings;
    if let Some(rest) = first_line(section).strip_prefix(cs.STACK_SIZE.as_str()) {
        // Portal Scroll "Stack Size: 2[localized separator]448/40"
        let digits: String = rest.chars().filter(|c| c.is_ascii_digit() || *c == '/').collect();
        let mut parts = digits.split('/').map(js::number);
        let value = parts.next().unwrap_or(f64::NAN);
        let max = parts.next().unwrap_or(f64::NAN);
        st.item.stack_size = Some(StackSize { value, max });

        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_sockets(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let cs = &st.data.client_strings;
    if let Some(rest) = first_line(section).strip_prefix(cs.SOCKETS.as_str()) {
        let sockets = rest.trim_end();

        let white = sockets.matches('W').count() as u32;
        let shape: String = sockets.chars().map(|c| if c == ' ' || c == '-' { c } else { '#' }).collect();
        let linked = if shape == "#-#-#-#-#-#" {
            Some(6)
        } else if shape == "# #-#-#-#-#" || shape == "#-#-#-#-# #" || shape == "#-#-#-#-#" {
            Some(5)
        } else {
            None
        };
        st.item.sockets = Some(Sockets { linked, white });
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_quality_nested(section: &[String], item: &mut ParsedItem, data: &GameData) -> bool {
    let cs = &data.client_strings;
    for line in section {
        if let Some(rest) = line.strip_prefix(cs.QUALITY.as_str()) {
            // "Quality: +20% (augmented)"
            item.quality = js::non_nan(js::parse_int(rest));
            return true;
        }
    }
    false
}

fn parse_memory_strands_nested(section: &[String], item: &mut ParsedItem, data: &GameData) -> bool {
    let cs = &data.client_strings;
    for line in section {
        if let Some(rest) = line.strip_prefix(cs.MEMORY_STRANDS.as_str()) {
            item.memory_strands = js::to_u32(js::parse_int(rest));
            return true;
        }
    }
    false
}

fn parse_armour(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let d = st.data;
    let cs = &d.client_strings;
    let item = &mut st.item;
    let mut is_parsed = SectionSkipped;

    for line in section {
        if let Some(rest) = line.strip_prefix(cs.ARMOUR.as_str()) {
            item.armour_ar = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.EVASION.as_str()) {
            item.armour_ev = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.ENERGY_SHIELD.as_str()) {
            item.armour_es = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.TAG_WARD.as_str()) {
            item.armour_ward = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.BLOCK_CHANCE.as_str()) {
            item.armour_block = js::non_nan(js::parse_int(rest));
            is_parsed = SectionParsed;
        }
    }

    if is_parsed == SectionParsed {
        parse_quality_nested(section, item, d);
        parse_memory_strands_nested(section, item, d);
    }

    Ok(is_parsed)
}

fn parse_weapon(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let d = st.data;
    let cs = &d.client_strings;
    let item = &mut st.item;
    let mut is_parsed = SectionSkipped;

    let range_avg = |s: &str| get_roll_or_minmax_avg(&s.split('-').map(js::parse_int).collect::<Vec<_>>());

    for line in section {
        if let Some(rest) = line.strip_prefix(cs.CRIT_CHANCE.as_str()) {
            item.weapon_crit = js::non_nan(js::parse_float(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.ATTACK_SPEED.as_str()) {
            item.weapon_as = js::non_nan(js::parse_float(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.PHYSICAL_DAMAGE.as_str()) {
            item.weapon_physical = js::non_nan(range_avg(rest));
            is_parsed = SectionParsed;
        } else if let Some(rest) = line.strip_prefix(cs.ELEMENTAL_DAMAGE.as_str()) {
            item.weapon_elemental = js::non_nan(rest.split(", ").map(range_avg).sum());
            is_parsed = SectionParsed;
        }
    }

    if is_parsed == SectionParsed {
        parse_quality_nested(section, item, d);
        parse_memory_strands_nested(section, item, d);
    }

    Ok(is_parsed)
}

fn parse_accessory(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let is_target = st.item.category.is_some_and(|c| c.is_jewellery() || c == ItemCategory::Quiver);
    if !is_target {
        return Ok(ParserSkipped);
    }
    let d = st.data;

    let mut is_parsed = SectionSkipped;

    for line in section {
        if let Some(text) = line.strip_suffix(VALUE_AUGMENTED) {
            let found = try_parse_translation_with(
                d,
                &StatString { string: text.to_owned(), unscalable: true },
                ModifierType::Pseudo,
                None,
            );
            if let Some(found) = found.filter(|f| f.stat.jewellery_quality.is_some()) {
                // `found.roll!.value`
                let roll = found.roll.as_ref().ok_or_else(|| "missing roll".to_owned())?;
                st.item.quality = Some(roll.value);
                st.item.new_mods.push(ParsedModifier {
                    info: ModifierInfo::new(ModifierType::Pseudo),
                    stats: vec![found],
                });
                is_parsed = SectionParsed;
            }
        }
    }

    if parse_memory_strands_nested(section, &mut st.item, d) {
        is_parsed = SectionParsed;
    }

    Ok(is_parsed)
}

fn parse_viscera(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.info.ref_name != "Viscera" {
        return Ok(ParserSkipped);
    }
    let cs = &st.data.client_strings;
    if section.len() == 1 {
        let line = &section[0];
        if *line == cs.VISCERA_HEAD {
            st.item.category = Some(ItemCategory::HeadViscera);
        } else if *line == cs.VISCERA_LOWER_LIMB {
            st.item.category = Some(ItemCategory::LowerLimbViscera);
        } else if *line == cs.VISCERA_UPPER_LIMB {
            st.item.category = Some(ItemCategory::UpperLimbViscera);
        } else if *line == cs.VISCERA_TORSO {
            st.item.category = Some(ItemCategory::TorsoViscera);
        }
        if st.item.category.is_some() {
            return Ok(SectionParsed);
        }
    }
    Ok(SectionSkipped)
}

fn parse_logbook_area(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.info.ref_name != "Expedition Logbook" {
        return Ok(ParserSkipped);
    }
    if section.len() < 3 {
        return Ok(SectionSkipped);
    }
    let d = st.data;

    // skip Logbook Area line, parse Faction
    let Some((faction, faction_matcher)) =
        d.stat_by_match_str(&section[1]).filter(|(s, _)| s.ref_.starts_with("Has Logbook Faction:"))
    else {
        return Ok(SectionSkipped);
    };

    let mut area_mods = vec![ParsedModifier {
        info: ModifierInfo::new(ModifierType::Pseudo),
        stats: vec![ParsedStat { stat: faction.clone(), translation: faction_matcher.clone(), roll: None }],
    }];

    let (mod_type, lines) = parse_mod_type(&section[2..])?;
    for line in &lines {
        // Area contains an Expedition Boss (#)
        if let Some((stat, matcher)) = d.stat_by_match_str(line).filter(|(s, _)| s.better == StatBetter::NotComparable)
        {
            area_mods.push(ParsedModifier {
                info: ModifierInfo::new(mod_type),
                stats: vec![ParsedStat { stat: stat.clone(), translation: matcher.clone(), roll: None }],
            });
        }
    }

    st.item.logbook_area_mods.get_or_insert_with(Vec::new).push(area_mods);

    Ok(SectionParsed)
}

fn parse_mercenary(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.info.ref_name != "Mercenary Warrant" {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    let cs = &d.client_strings;

    for line in section {
        if let Some(rest) = line.strip_prefix(cs.MERCENARY_LEVEL.as_str()) {
            st.item.item_level = js::to_u32(js::number(rest));
        } else if let Some(rest) = line.strip_prefix(cs.MERCENARY_BUILD.as_str()) {
            let mut build_info = item_by_translated(d, Namespace::MercenaryBuild, rest)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| "Unknown Mercenary Build.".to_owned())?;

            if let Some(MercenaryBuildRef::RefName(ref_name)) = &build_info[0].mercenary_build {
                build_info = item_by_ref(d, Namespace::MercenaryBuild, ref_name)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| "Unknown Mercenary Build.".to_owned())?;
            }
            st.item.mercenary_build = build_info.into_iter().next();
        }
    }

    if st.item.mercenary_build.is_some() {
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_mercenary_gems(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.info.ref_name != "Mercenary Warrant" {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    let parse = |s: &str| {
        try_parse_translation_with(
            d,
            &StatString { string: s.to_owned(), unscalable: true },
            ModifierType::Pseudo,
            Some(ItemCategory::MercenaryWarrant),
        )
    };

    let Some(skill) = parse(first_line(section)) else { return Ok(SectionSkipped) };

    let mut group: Vec<ParsedStat> = vec![skill];

    for line in &section[1..] {
        let support = parse(line);
        let flagged = match &support {
            None => true,
            Some(s) => {
                let merc = s.stat.mercenary.clone().unwrap_or_default();
                merc.synthetic_family && merc.tier != Some(3)
            }
        };
        if let Some(s) = support {
            group.push(s);
        }
        if flagged {
            st.item.unknown_modifiers.push(UnknownModifier {
                text: format!("{} [{}]", line, section[0]),
                type_: ModifierType::Pseudo,
            });
        }
    }

    st.item.mercenary_skills.get_or_insert_with(Vec::new).push(group);

    Ok(SectionParsed)
}

fn parse_modifiers(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.rarity.is_none() && st.item.info.ref_name != "Viscera" {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    let cs = &d.client_strings;

    let recognized_line = section
        .iter()
        .find(|line| line.ends_with(ENCHANT_LINE) || line.ends_with(SCOURGE_LINE) || is_mod_info_line(line));

    let Some(recognized_line) = recognized_line else {
        return Ok(SectionSkipped);
    };

    if is_mod_info_line(recognized_line) {
        for group in group_lines_by_mod(section) {
            let mut mod_info = parse_mod_info_line_with(&group.mod_line, cs)?;
            if group.stat_lines.first().is_some_and(|l| *l == cs.VEILED_PREFIX || *l == cs.VEILED_SUFFIX) {
                mod_info.type_ = ModifierType::Veiled;
                st.item.is_veiled = true;
            }
            parse_stats_from_mod(&group.stat_lines, st, ParsedModifier { info: mod_info, stats: Vec::new() });
        }
    } else {
        let (mod_type, lines) = parse_mod_type(section)?;
        let mod_info = ModifierInfo::new(mod_type);
        parse_stats_from_mod(&lines, st, ParsedModifier { info: mod_info, stats: Vec::new() });
    }

    Ok(SectionParsed)
}

fn parse_mirrored(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if first_line(section) == st.data.client_strings.MIRRORED {
        st.item.is_mirrored = true;
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_split(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if section.len() == 1 && section[0] == st.data.client_strings.SPLIT {
        st.item.is_split = true;
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_flask(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::Flask) {
        return Ok(ParserSkipped);
    }
    let d = st.data;

    // the purpose of this parser is to "consume" flask buffs
    // so they are not recognized as modifiers

    if section.iter().any(|line| d.client_strings.FLASK_CHARGES.is_match(line)) {
        parse_quality_nested(section, &mut st.item, d);
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_tincture(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::Tincture) {
        return Ok(ParserSkipped);
    }
    if parse_quality_nested(section, &mut st.item, st.data) {
        return Ok(SectionParsed);
    }
    Ok(SectionSkipped)
}

fn parse_sentinel_charge(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::Sentinel) {
        return Ok(ParserSkipped);
    }
    let cs = &st.data.client_strings;
    if section.len() == 1
        && let Some(rest) = section[0].strip_prefix(cs.SENTINEL_CHARGE.as_str()) {
            st.item.sentinel_charge = js::to_u32(js::parse_int(rest));
            return Ok(SectionParsed);
        }
    Ok(SectionSkipped)
}

fn parse_scrying_orb(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.info.ref_name != "Scrying Orb" {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    if section.len() == 1
        && let Some(area_name) = section[0].strip_prefix(d.client_strings.MAP_AREA.as_str()) {
            let area_info = item_by_translated(d, Namespace::Area, area_name)
                .ok_or_else(|| "Unknown Area name.".to_owned())?;
            st.item.map_area = area_info.into_iter().next();
            return Ok(SectionParsed);
        }
    Ok(SectionSkipped)
}

fn parse_eldritch_item(section: &mut Vec<String>, data: &GameData) {
    let cs = &data.client_strings;
    // these don't have a dedicated section and are appended to last-ish one,
    // it can be Explicit Mods section, Corrupted, or Mirrored line.
    while let Some(last) = section.last() {
        if *last == cs.ITEM_EATER || *last == cs.ITEM_EXARCH {
            section.pop();
        } else {
            break;
        }
    }
}

fn parse_synthesised(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let cs = &st.data.client_strings;
    if section.len() == 1 && section[0] == cs.SECTION_SYNTHESISED {
        st.item.is_synthesised = true;
        let use_base = st.base_type.as_deref().is_some_and(|b| !b.is_empty());
        let target = if use_base { st.base_type.as_deref().unwrap_or("") } else { st.name.as_str() };
        // `exec(...)![1]` throws upstream when it doesn't match
        let stripped = cs
            .ITEM_SYNTHESISED
            .group1(target)
            .ok_or_else(|| "synthesised name".to_owned())?
            .to_owned();
        if use_base {
            st.base_type = Some(stripped);
        } else {
            st.name = stripped;
        }
        return Ok(SectionParsed);
    }

    Ok(SectionSkipped)
}

fn parse_category_by_help_text(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    let cs = &st.data.client_strings;
    let first = first_line(section);
    if first == cs.BEAST_HELP {
        st.item.category = Some(ItemCategory::CapturedBeast);
        Ok(SectionParsed)
    } else if first == cs.METAMORPH_HELP {
        st.item.category = Some(ItemCategory::MetamorphSample);
        Ok(SectionParsed)
    } else if first == cs.VOIDSTONE_HELP {
        st.item.category = Some(ItemCategory::Voidstone);
        Ok(SectionParsed)
    } else if first == cs.VISCERA_HELP {
        st.name = "Viscera".to_owned();
        Ok(SectionParsed)
    } else {
        Ok(SectionSkipped)
    }
}

fn parse_heist_contract(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::HeistContract) {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    let cs = &d.client_strings;

    if !parse_area_level_nested(section, &mut st.item, d) {
        return Ok(SectionSkipped);
    }

    let mut contract = HeistContract::default();

    for line in section {
        if let Some(job_match) = cs.HEIST_CONTRACT_JOB.captures(line) {
            let job = job_match.name("job").map_or("", |m| m.as_str());
            let required = if job == cs.HEIST_JOB_LOCKPICKING {
                Some(HeistJob::Lockpicking)
            } else if job == cs.HEIST_JOB_BRUTEFORCE {
                Some(HeistJob::BruteForce)
            } else if job == cs.HEIST_JOB_PERCEPTION {
                Some(HeistJob::Perception)
            } else if job == cs.HEIST_JOB_DEMOLITION {
                Some(HeistJob::Demolition)
            } else if job == cs.HEIST_JOB_COUNTERTHAUMATURGY {
                Some(HeistJob::CounterThaumaturgy)
            } else if job == cs.HEIST_JOB_TRAPDISARMAMENT {
                Some(HeistJob::TrapDisarmament)
            } else if job == cs.HEIST_JOB_AGILITY {
                Some(HeistJob::Agility)
            } else if job == cs.HEIST_JOB_DECEPTION {
                Some(HeistJob::Deception)
            } else if job == cs.HEIST_JOB_ENGINEERING {
                Some(HeistJob::Engineering)
            } else {
                None
            };
            if required.is_some() {
                contract.required_job = required;
            }
            contract.job_level = job_match.name("level").and_then(|m| js::to_u32(js::number(m.as_str())));
            continue;
        }

        if let Some(target_match) = cs.HEIST_CONTRACT_TARGET.captures(line) {
            if target_match.get(1).map(|m| m.as_str()) == Some(cs.HEIST_TARGET_PRICELESS.as_str()) {
                contract.target_value_priceless = true;
            }
            continue;
        }
    }

    st.item.heist_contract = Some(contract);
    Ok(SectionParsed)
}

fn parse_heist_blueprint(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::HeistBlueprint) {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    let cs = &d.client_strings;

    if !parse_area_level_nested(section, &mut st.item, d) {
        return Ok(SectionSkipped);
    }

    let mut blueprint = HeistBlueprint::default();

    for line in section {
        if let Some(target_text) = line.strip_prefix(cs.HEIST_BLUEPRINT_TARGET.as_str()) {
            if target_text == cs.HEIST_BLUEPRINT_ENCHANTS {
                blueprint.target = Some(HeistTarget::Enchants);
            } else if target_text == cs.HEIST_BLUEPRINT_GEMS {
                blueprint.target = Some(HeistTarget::Gems);
            } else if target_text == cs.HEIST_BLUEPRINT_REPLICAS {
                blueprint.target = Some(HeistTarget::Replicas);
            } else if target_text == cs.HEIST_BLUEPRINT_TRINKETS {
                blueprint.target = Some(HeistTarget::Trinkets);
            }
        } else if let Some(rest) = line.strip_prefix(cs.HEIST_WINGS_REVEALED.as_str()) {
            let mut parts = rest.split('/');
            blueprint.wings_revealed = parts.next().and_then(|s| js::to_u32(js::parse_int(s)));
            blueprint.wings_total = parts.next().and_then(|s| js::to_u32(js::parse_int(s)));
        }
    }

    st.item.heist_blueprint = Some(blueprint);
    Ok(SectionParsed)
}

fn parse_chart(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.category != Some(ItemCategory::Chart) {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    let cs = &d.client_strings;

    if !parse_area_level_nested(section, &mut st.item, d) {
        return Ok(SectionSkipped);
    }

    let area_info =
        item_by_translated(d, Namespace::Area, first_line(section)).ok_or_else(|| "Unknown Area name.".to_owned())?;
    st.item.map_area = area_info.into_iter().next();

    for line in section {
        if parse_area_prop_nested(line, &mut st.item, d) {
            // line parsed
        } else if let Some(rest) = line.strip_prefix(cs.CHART_SULPHUR.as_str()) {
            st.item.chart_sulphur = js::non_nan(js::parse_int(rest));
        }
    }

    Ok(SectionParsed)
}

fn parse_area_level_nested(section: &[String], item: &mut ParsedItem, data: &GameData) -> bool {
    let cs = &data.client_strings;
    for line in section {
        if let Some(rest) = line.strip_prefix(cs.AREA_LEVEL.as_str()) {
            item.area_level = js::to_u32(js::number(rest));
            return true;
        }
    }
    false
}

fn parse_area_level(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if !matches!(
        st.item.info.ref_name.as_str(),
        "Chronicle of Atzoatl" | "Expedition Logbook" | "Mirrored Tablet" | "Forbidden Tome"
    ) {
        return Ok(ParserSkipped);
    }

    if !parse_area_level_nested(section, &mut st.item, st.data) {
        return Ok(SectionSkipped);
    }

    Ok(SectionParsed)
}

fn parse_atzoatl_rooms(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.info.ref_name != "Chronicle of Atzoatl" {
        return Ok(ParserSkipped);
    }
    let d = st.data;
    let cs = &d.client_strings;
    if first_line(section) != cs.INCURSION_OPEN {
        return Ok(SectionSkipped);
    }

    let mut state = IncursionRoom::Open;
    for line in &section[1..] {
        if *line == cs.INCURSION_OBSTRUCTED {
            state = IncursionRoom::Obstructed;
            continue;
        }

        if let Some((stat, matcher)) = d.stat_by_match_str(line) {
            let string = if state == IncursionRoom::Open {
                matcher.string.clone()
            } else {
                format!("{} {}", cs.INCURSION_OBSTRUCTED, matcher.string)
            };
            let v = state as i32 as f64;
            st.item.new_mods.push(ParsedModifier {
                info: ModifierInfo::new(ModifierType::Pseudo),
                stats: vec![ParsedStat {
                    stat: stat.clone(),
                    translation: StatMatcher { string, advanced: None, negate: false, value: None },
                    roll: Some(ParsedRoll { value: v, min: v, max: v, dp: false, unscalable: true, generation: None }),
                }],
            });
        } else {
            st.item.unknown_modifiers.push(UnknownModifier { text: line.clone(), type_: ModifierType::Pseudo });
        }
    }

    Ok(SectionParsed)
}

fn parse_mirrored_tablet(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.info.ref_name != "Mirrored Tablet" {
        return Ok(ParserSkipped);
    }
    if section.len() < 8 {
        return Ok(SectionSkipped);
    }

    for line in section {
        let found = try_parse_translation_with(
            st.data,
            &StatString { string: line.clone(), unscalable: true },
            ModifierType::Pseudo,
            None,
        );
        match found {
            Some(found) => st.item.new_mods.push(ParsedModifier {
                info: ModifierInfo::new(ModifierType::Pseudo),
                stats: vec![found],
            }),
            None => st.item.unknown_modifiers.push(UnknownModifier { text: line.clone(), type_: ModifierType::Pseudo }),
        }
    }

    Ok(SectionParsed)
}

fn parse_filled_coffin(section: &[String], st: &mut ParserState<'_>) -> SectionResult {
    if st.item.info.ref_name != "Filled Coffin" {
        return Ok(ParserSkipped);
    }
    if !section.iter().any(|line| line.ends_with(IMPLICIT_LINE)) {
        return Ok(SectionSkipped);
    }

    let (_, lines) = parse_mod_type(section)?;
    let mod_info = ModifierInfo::new(ModifierType::Necropolis);
    parse_stats_from_mod(&lines, st, ParsedModifier { info: mod_info, stats: Vec::new() });

    Ok(SectionParsed)
}

fn parse_stats_from_mod(lines: &[String], st: &mut ParserState<'_>, mut modifier: ParsedModifier) {
    let d = st.data;
    let mod_type = modifier.info.type_;

    if mod_type == ModifierType::Veiled {
        let name = modifier.info.name.clone().unwrap_or_default();
        match d.stat_by_match_str(&name) {
            Some((stat, matcher)) => modifier.stats.push(ParsedStat {
                stat: stat.clone(),
                translation: matcher.clone(),
                roll: None,
            }),
            None => st.item.unknown_modifiers.push(UnknownModifier { text: name, type_: mod_type }),
        }
        st.item.new_mods.push(modifier);
        return;
    }

    let category = st.item.category;
    let not_parsed = lines_to_stat_strings_with(lines, &d.client_strings.UNSCALABLE_VALUE, |stat| {
        match try_parse_translation_with(d, &stat, mod_type, category) {
            Some(parsed) => {
                modifier.stats.push(parsed);
                true
            }
            None => false,
        }
    });
    st.item.new_mods.push(modifier);

    st.item
        .unknown_modifiers
        .extend(not_parsed.into_iter().map(|text| UnknownModifier { text, type_: mod_type }));
}
