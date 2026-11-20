//! Port of upstream `renderer/src/parser/advanced-mod-desc.ts`.
//! (`applyIncr` / `incrRoll` live in `modifiers.rs` and are re-exported here.)

use rpt_data::ClientStrings;

use crate::js;
use crate::types::*;

pub use crate::modifiers::{apply_incr, incr_roll};

pub const SCOURGE_LINE: &str = " (scourge)";
pub const ENCHANT_LINE: &str = " (enchant)";
pub const IMPLICIT_LINE: &str = " (implicit)";

/// Upstream `parseModInfoLine`. Parses an advanced mod description line like
/// `{ Prefix Modifier "Hale" (Tier: 8) — Life }`.
///
/// Returns `Err` where upstream throws.
pub fn parse_mod_info_line(line: &str) -> Result<ModifierInfo, String> {
    let data = rpt_data::data();
    parse_mod_info_line_with(line, &data.client_strings)
}

pub(crate) fn parse_mod_info_line_with(line: &str, cs: &ClientStrings) -> Result<ModifierInfo, String> {
    // line.slice(1, -1)
    let inner = {
        let mut chars = line.chars();
        chars.next();
        chars.next_back();
        chars.as_str()
    };
    let parts: Vec<&str> = inner.split('\u{2014}').map(str::trim).collect();
    let mod_text = parts[0];
    let x_text2 = parts.get(1).copied();
    let x_text3 = parts.get(2).copied();

    let mut type_ = ModifierType::Explicit;
    let mut generation = None;
    let mut mechanic = None;
    let mut name = None;
    let mut tier = None;
    let rank;

    let eldritch = cs
        .EATER_IMPLICIT
        .captures(mod_text)
        .or_else(|| cs.EXARCH_IMPLICIT.captures(mod_text));
    if let Some(m) = eldritch {
        type_ = ModifierType::Implicit;
        mechanic = Some(ModifierMechanic::Eldritch);

        let r = m.name("rank").map(|m| m.as_str());
        rank = match r {
            Some(r) if r == cs.ELDRITCH_MOD_R1 => Some(1),
            Some(r) if r == cs.ELDRITCH_MOD_R2 => Some(2),
            Some(r) if r == cs.ELDRITCH_MOD_R3 => Some(3),
            Some(r) if r == cs.ELDRITCH_MOD_R4 => Some(4),
            Some(r) if r == cs.ELDRITCH_MOD_R5 => Some(5),
            Some(r) if r == cs.ELDRITCH_MOD_R6 => Some(6),
            _ => None,
        };
    } else {
        let m = cs
            .MODIFIER_LINE
            .captures(mod_text)
            .ok_or_else(|| "Invalid regex for mod info line".to_owned())?;
        let t = m.name("type").map(|m| m.as_str()).unwrap_or("");

        if t == cs.IMPLICIT_MODIFIER || t == cs.CORRUPTED_IMPLICIT || t == cs.VESTIGIAL_IMPLICIT {
            type_ = ModifierType::Implicit;
        } else if t == cs.FRACTURED_PREFIX || t == cs.FRACTURED_SUFFIX {
            type_ = ModifierType::Fractured;
        } else if t == cs.CRAFTED_PREFIX || t == cs.CRAFTED_SUFFIX {
            type_ = ModifierType::Crafted;
        } else if t == cs.RARE_ONLY_PREFIX || t == cs.RARE_ONLY_SUFFIX {
            mechanic = Some(ModifierMechanic::RareOnly);
        }

        if t == cs.PREFIX_MODIFIER || t == cs.FRACTURED_PREFIX || t == cs.CRAFTED_PREFIX || t == cs.RARE_ONLY_PREFIX {
            generation = Some(ModGeneration::Prefix);
        } else if t == cs.SUFFIX_MODIFIER
            || t == cs.FRACTURED_SUFFIX
            || t == cs.CRAFTED_SUFFIX
            || t == cs.RARE_ONLY_SUFFIX
        {
            generation = Some(ModGeneration::Suffix);
        } else if t == cs.CORRUPTED_IMPLICIT {
            mechanic = Some(ModifierMechanic::Corruption);
        } else if t == cs.FOULBORN_MODIFIER {
            mechanic = Some(ModifierMechanic::Foulborn);
        } else if t == cs.VESTIGIAL_IMPLICIT {
            mechanic = Some(ModifierMechanic::Vestigial);
        }

        name = m.name("name").map(|m| m.as_str().to_owned());
        // Number(x) || undefined
        let num_or_none = |g: &str| {
            m.name(g)
                .map(|m| js::number(m.as_str()))
                .filter(|v| *v != 0.0 && !v.is_nan())
                .and_then(js::to_u32)
        };
        tier = num_or_none("tier");
        rank = num_or_none("rank");
    }

    if mechanic.is_none()
        && let Some(name) = name.as_deref().filter(|n| !n.is_empty()) {
            mechanic = mechanic_from_mod_name(name, cs);
        }

    let incr_text = match (x_text2, x_text3) {
        (_, Some(x3)) => Some(x3),
        (Some(x2), None) if cs.MODIFIER_INCREASED.is_match(x2) => Some(x2),
        _ => None,
    };
    let tags_text = match x_text2 {
        Some(x2) if incr_text != Some(x2) => Some(x2),
        _ => None,
    };
    let tags = match tags_text {
        Some(t) if !t.is_empty() => t.split(", ").map(str::to_owned).collect(),
        _ => Vec::new(),
    };
    let roll_incr = match incr_text {
        Some(t) if !t.is_empty() => {
            let g = cs
                .MODIFIER_INCREASED
                .group1(t)
                .ok_or_else(|| "Invalid regex for mod roll increase".to_owned())?;
            // `!rollIncr` checks upstream treat NaN/0 as absent
            js::non_nan(js::number(g)).filter(|v| *v != 0.0)
        }
        _ => None,
    };

    Ok(ModifierInfo { type_, generation, mechanic, name, tier, rank, tags, roll_incr })
}

/// Upstream `isModInfoLine`.
pub fn is_mod_info_line(line: &str) -> bool {
    line.starts_with('{') && line.ends_with('}')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedModLines {
    pub mod_line: String,
    pub stat_lines: Vec<String>,
}

/// Upstream `groupLinesByMod` (generator collected into a `Vec`).
pub fn group_lines_by_mod(lines: &[String]) -> Vec<GroupedModLines> {
    let mut out: Vec<GroupedModLines> = Vec::new();
    if lines.is_empty() || !is_mod_info_line(&lines[0]) {
        return out;
    }
    for line in lines {
        if !is_mod_info_line(line) {
            if let Some(last) = out.last_mut() {
                last.stat_lines.push(line.clone());
            }
        } else {
            out.push(GroupedModLines { mod_line: line.clone(), stat_lines: Vec::new() });
        }
    }
    out
}

/// Upstream `parseModType`. Returns `Err` where upstream throws.
pub fn parse_mod_type(lines: &[String]) -> Result<(ModifierType, Vec<String>), String> {
    let has = |ending: &str| lines.iter().any(|l| l.ends_with(ending));
    let (mod_type, ending) = if has(SCOURGE_LINE) {
        (ModifierType::Scourge, SCOURGE_LINE)
    } else if has(ENCHANT_LINE) {
        (ModifierType::Enchant, ENCHANT_LINE)
    } else if has(IMPLICIT_LINE) {
        (ModifierType::Implicit, IMPLICIT_LINE)
    } else {
        return Err("Expected to be used only on lines that have modifier type".to_owned());
    };
    Ok((mod_type, remove_lines_ending(lines, ending)))
}

fn remove_lines_ending(lines: &[String], ending: &str) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.strip_suffix(ending).unwrap_or(l).to_owned())
        .collect()
}

/// Upstream `mechanicFromModName`.
pub fn mechanic_from_mod_name(name: &str, cs: &ClientStrings) -> Option<ModifierMechanic> {
    let has = |list: &[String]| list.iter().any(|n| n == name);
    if has(&cs.SHAPER_MODS) {
        Some(ModifierMechanic::Shaper)
    } else if has(&cs.ELDER_MODS) {
        Some(ModifierMechanic::Elder)
    } else if has(&cs.HUNTER_MODS) {
        Some(ModifierMechanic::Hunter)
    } else if has(&cs.WARLORD_MODS) {
        Some(ModifierMechanic::Warlord)
    } else if has(&cs.REDEEMER_MODS) {
        Some(ModifierMechanic::Redeemer)
    } else if has(&cs.CRUSADER_MODS) {
        Some(ModifierMechanic::Crusader)
    } else if has(&cs.DELVE_MODS) {
        Some(ModifierMechanic::Delve)
    } else if has(&cs.VEILED_MODS) {
        Some(ModifierMechanic::Unveiled)
    } else if has(&cs.INCURSION_MODS) {
        Some(ModifierMechanic::Incursion)
    } else if has(&cs.ESSENCE_MODS) {
        Some(ModifierMechanic::Essence)
    } else if has(&cs.INFAMOUS_MODS) {
        Some(ModifierMechanic::Infamous)
    } else {
        None
    }
}
