//! Port of upstream `filters/pseudo/anointments.ts`.

use rpt_parser::{ModifierType, ParsedItem, StatCalculated};

use crate::filters::StatFilter;

const OILS: &[&str] = &[
    "Prismatic Oil",
    "Clear Oil",
    "Sepia Oil",
    "Amber Oil",
    "Verdant Oil",
    "Teal Oil",
    "Azure Oil",
    "Indigo Oil",
    "Violet Oil",
    "Crimson Oil",
    "Black Oil",
    "Opalescent Oil",
    "Silver Oil",
    "Golden Oil",
];

pub fn decode_oils(calc: &StatCalculated) -> Option<Vec<String>> {
    if calc.type_ != ModifierType::Enchant {
        return None;
    }

    let anointments = calc.stat.anointments.as_ref()?;
    let encoded = if anointments.len() == 1 {
        Some(anointments[0].oils.as_str())
    } else {
        let roll = calc.sources.first().and_then(|s| s.stat.roll.as_ref()).map(|r| r.value);
        anointments.iter().find(|a| Some(a.roll) == roll).map(|a| a.oils.as_str())
    };
    let encoded = encoded.filter(|e| !e.is_empty())?;

    let mut idx: Vec<i64> = encoded.split(',').map(|s| s.trim().parse::<i64>().unwrap_or(-1)).collect();
    idx.sort_by(|a, b| b.cmp(a)); // [Golden Oil ... Clear Oil]
    Some(
        idx.into_iter()
            .map(|i| usize::try_from(i).ok().and_then(|i| OILS.get(i)).copied().unwrap_or("").to_owned())
            .collect(),
    )
}

pub fn apply_anointment_rules(filters: &mut [StatFilter], item: &ParsedItem) {
    let Some(anointment) = filters.iter_mut().find(|f| f.oils.is_some()) else { return };

    if item.talisman_tier.is_some_and(|t| t != 0) {
        anointment.disabled = false;
    } else if item.is_corrupted.is_none() && !item.is_mirrored {
        let first = anointment.oils.as_ref().and_then(|o| o.first()).map(String::as_str);
        if first != Some("Golden Oil") && first != Some("Silver Oil") {
            anointment.hidden = Some("filters.hide_anointment".to_owned());
            anointment.disabled = true;
        }
    }
}
