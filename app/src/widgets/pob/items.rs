//! Item text handling for the PoB viewer: parse PoB's item text and the
//! game's clipboard text into normalized modifier lines, and compare two
//! items mod by mod. This is a text comparison, NOT a PoB stat calculation.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rarity {
    #[default]
    Normal,
    Magic,
    Rare,
    Unique,
    Relic,
    Gem,
    Other,
}

impl Rarity {
    fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Self::Normal,
            "magic" => Self::Magic,
            "rare" => Self::Rare,
            "unique" => Self::Unique,
            "relic" => Self::Relic,
            "gem" => Self::Gem,
            _ => Self::Other,
        }
    }

    /// PoE item frame text colors (same as the price check widget).
    pub fn design_color(self) -> egui::Color32 {
        use egui::Color32;
        match self {
            Self::Normal => Color32::from_rgb(0xC8, 0xC8, 0xC8),
            Self::Magic => Color32::from_rgb(0x88, 0x88, 0xFF),
            Self::Rare => Color32::from_rgb(0xFF, 0xFF, 0x77),
            Self::Unique => Color32::from_rgb(0xAF, 0x60, 0x25),
            Self::Relic => Color32::from_rgb(0x82, 0xAD, 0x6A),
            Self::Gem => Color32::from_rgb(0x1B, 0xA2, 0x9B),
            Self::Other => Color32::from_rgb(0xAA, 0x9E, 0x82),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Enchant,
    Implicit,
    Explicit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModLine {
    pub kind: ModKind,
    /// Clean display text, e.g. `+78 to maximum Life`.
    pub text: String,
    /// Numbers replaced by `#`, e.g. `+# to maximum Life`.
    pub template: String,
    pub values: Vec<f64>,
    pub crafted: bool,
    pub fractured: bool,
}

impl ModLine {
    fn new(kind: ModKind, text: &str, crafted: bool, fractured: bool) -> Self {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let (template, values) = template_of(&text);
        Self { kind, text, template, values, crafted, fractured }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemText {
    pub rarity: Rarity,
    pub name: String,
    pub base: Option<String>,
    pub item_class: Option<String>,
    pub mods: Vec<ModLine>,
    pub corrupted: bool,
}

impl ItemText {
    pub fn title(&self) -> String {
        match &self.base {
            Some(b) if !self.name.is_empty() => format!("{} {}", self.name, b),
            Some(b) => b.clone(),
            None => self.name.clone(),
        }
    }
}

static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+(?:\.\d+)?").unwrap());
static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((-?\d+(?:\.\d+)?)-(-?\d+(?:\.\d+)?)\)").unwrap());
/// Advanced-copy roll ranges right after a value: `+78(70-79)` or `12(10)`.
static ADV_RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d)\(-?\d+(?:\.\d+)?(?:--?\d+(?:\.\d+)?)?\)").unwrap());
static PROPERTY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z][A-Za-z' ]{0,40}:(\s|$)").unwrap());

/// Template + numeric values (sign kept in the template).
pub fn template_of(text: &str) -> (String, Vec<f64>) {
    let values = NUM.find_iter(text).filter_map(|m| m.as_str().parse().ok()).collect();
    (NUM.replace_all(text, "#").into_owned(), values)
}

/// Lines that are item flags rather than modifiers.
fn is_flag_line(l: &str) -> bool {
    matches!(
        l,
        "Corrupted"
            | "Mirrored"
            | "Split"
            | "Unidentified"
            | "Has Alt Variant"
            | "Has Alt Variant Two"
            | "Synthesised Item"
            | "Fractured Item"
            | "Foulborn Item"
    ) || (l.ends_with(" Item") && l.split_whitespace().count() <= 4 && !l.chars().any(|c| c.is_ascii_digit()))
}

// ---------------------------------------------------------------------------
// PoB item text

/// Resolve `(min-max)` ranges at `range` (0..1) like PoB does.
fn resolve_ranges(s: &str, range: f64) -> String {
    RANGE
        .replace_all(s, |c: &regex::Captures<'_>| {
            let a: f64 = c[1].parse().unwrap_or(0.0);
            let b: f64 = c[2].parse().unwrap_or(0.0);
            let v = a + (b - a) * range;
            if c[1].contains('.') || c[2].contains('.') { format!("{:.1}", v) } else { format!("{}", v.round() as i64) }
        })
        .into_owned()
}

/// Parse PoB's item text (the content of `<Item>`).
pub fn parse_pob_item(text: &str) -> ItemText {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut it = ItemText::default();
    let mut i = 0;
    if let Some(r) = lines.first().and_then(|l| l.strip_prefix("Rarity:")) {
        it.rarity = Rarity::parse(r);
        i = 1;
    }
    let two_names = matches!(it.rarity, Rarity::Rare | Rarity::Unique | Rarity::Relic);
    if let Some(n) = lines.get(i) {
        it.name = n.to_string();
        i += 1;
    }
    if two_names {
        if let Some(b) = lines.get(i) {
            it.base = Some(b.to_string());
            i += 1;
        }
    }
    let selected_variant: Option<u32> =
        lines.iter().find_map(|l| l.strip_prefix("Selected Variant:")).and_then(|v| v.trim().parse().ok());
    let implicits_at = lines.iter().position(|l| l.starts_with("Implicits:"));
    let mut implicit_left: usize = implicits_at.and_then(|p| lines[p]["Implicits:".len()..].trim().parse().ok()).unwrap_or(0);
    let start = implicits_at.map(|p| p + 1).unwrap_or(i);

    for l in &lines[start..] {
        if is_flag_line(l) {
            if *l == "Corrupted" {
                it.corrupted = true;
            }
            continue;
        }
        // Without an `Implicits:` line, skip property lines (older PoB).
        if implicits_at.is_none() && PROPERTY.is_match(l) {
            continue;
        }
        let mut rest: &str = l;
        let mut range = 0.5;
        let mut variants: Option<Vec<u32>> = None;
        let (mut crafted, mut fractured, mut enchant, mut implicit_tag) = (false, false, false, false);
        while let Some(stripped) = rest.strip_prefix('{') {
            let Some(end) = stripped.find('}') else { break };
            let tag = &stripped[..end];
            rest = &stripped[end + 1..];
            let (k, v) = tag.split_once(':').unwrap_or((tag, ""));
            match k {
                "range" => range = v.parse().unwrap_or(0.5),
                "variant" => variants = Some(v.split(',').filter_map(|x| x.trim().parse().ok()).collect()),
                "crafted" => crafted = true,
                "fractured" => fractured = true,
                "enchant" => enchant = true,
                "implicit" => implicit_tag = true,
                _ => {}
            }
        }
        let is_implicit = if implicit_left > 0 {
            implicit_left -= 1;
            true
        } else {
            implicit_tag
        };
        if let (Some(vs), Some(sel)) = (&variants, selected_variant) {
            if !vs.contains(&sel) {
                continue;
            }
        }
        let rest = rest.trim();
        if rest.is_empty() {
            continue;
        }
        let kind = if enchant {
            ModKind::Enchant
        } else if is_implicit {
            ModKind::Implicit
        } else {
            ModKind::Explicit
        };
        it.mods.push(ModLine::new(kind, &resolve_ranges(rest, range), crafted, fractured));
    }
    it
}

// ---------------------------------------------------------------------------
// Game clipboard text (Ctrl+C / Ctrl+Alt+C)

const SUFFIXES: &[(&str, Option<ModKind>)] = &[
    (" (implicit)", Some(ModKind::Implicit)),
    (" (enchant)", Some(ModKind::Enchant)),
    (" (crafted)", None),
    (" (fractured)", None),
    (" (crucible)", None),
    (" (scourge)", None),
    (" (augmented)", None),
    (" (unmet)", None),
];

/// Parse item text copied from the game.
pub fn parse_game_item(text: &str) -> ItemText {
    let text = text.replace("\r\n", "\n");
    let sections: Vec<Vec<&str>> = text
        .split("\n--------")
        .map(|s| s.lines().map(str::trim).filter(|l| !l.is_empty() && *l != "--------").collect::<Vec<_>>())
        .filter(|s| !s.is_empty())
        .collect();
    let mut it = ItemText::default();
    let Some(header) = sections.first() else { return it };
    let mut names = Vec::new();
    for l in header {
        if let Some(c) = l.strip_prefix("Item Class:") {
            it.item_class = Some(c.trim().to_owned());
        } else if let Some(r) = l.strip_prefix("Rarity:") {
            it.rarity = Rarity::parse(r);
        } else {
            names.push(l.to_string());
        }
    }
    match names.len() {
        0 => {}
        1 => it.name = names.remove(0),
        _ => {
            it.name = names.remove(0);
            it.base = Some(names.join(" "));
        }
    }

    let mut seen_mods = false;
    for sec in &sections[1..] {
        let content: Vec<&str> = sec.iter().copied().filter(|l| !l.starts_with('{')).collect();
        if content.is_empty() {
            continue;
        }
        if content.iter().all(|l| is_flag_line(l)) {
            it.corrupted |= content.contains(&"Corrupted");
            continue;
        }
        let props = content.iter().filter(|l| PROPERTY.is_match(l)).count();
        if props * 2 >= content.len() || content[0].ends_with(':') {
            continue;
        }
        let first = content[0];
        if ["Place into", "Right click", "Travel to", "Can be used", "Shift click", "Use Reforging"].iter().any(|p| first.starts_with(p)) {
            continue;
        }
        let tagged = content.iter().any(|l| SUFFIXES.iter().any(|(s, _)| l.ends_with(s)));
        // Unique flavour text: after the explicits, no numbers, no mod suffixes.
        if seen_mods && it.rarity == Rarity::Unique && !tagged && !content.iter().any(|l| l.chars().any(|c| c.is_ascii_digit())) {
            continue;
        }
        for l in content {
            if is_flag_line(l) {
                it.corrupted |= l == "Corrupted";
                continue;
            }
            let mut line = l.split(" — ").next().unwrap_or(l).to_owned();
            let mut kind = ModKind::Explicit;
            let (mut crafted, mut fractured) = (false, false);
            loop {
                let Some((s, k)) = SUFFIXES.iter().find(|(s, _)| line.ends_with(s)) else { break };
                line.truncate(line.len() - s.len());
                if let Some(k) = k {
                    kind = *k;
                }
                crafted |= *s == " (crafted)";
                fractured |= *s == " (fractured)";
            }
            let line = ADV_RANGE.replace_all(&line, "$1");
            if kind == ModKind::Explicit {
                seen_mods = true;
            }
            it.mods.push(ModLine::new(kind, &line, crafted, fractured));
        }
    }
    it
}

/// PoB slot names an in-game item class can go in.
pub fn slots_for_item_class(class: &str) -> Vec<String> {
    let c = class.trim();
    let one_hand = [
        "Claws", "Daggers", "Rune Daggers", "One Hand Axes", "One Hand Maces", "One Hand Swords",
        "Thrusting One Hand Swords", "Sceptres", "Wands",
    ];
    let two_hand = ["Bows", "Two Hand Axes", "Two Hand Maces", "Two Hand Swords", "Staves", "Warstaves", "Fishing Rods"];
    let v: Vec<&str> = match c {
        "Helmets" => vec!["Helmet"],
        "Body Armours" => vec!["Body Armour"],
        "Gloves" => vec!["Gloves"],
        "Boots" => vec!["Boots"],
        "Belts" => vec!["Belt"],
        "Amulets" => vec!["Amulet"],
        "Rings" => vec!["Ring 1", "Ring 2"],
        "Shields" | "Quivers" => vec!["Weapon 2"],
        c if one_hand.contains(&c) => vec!["Weapon 1", "Weapon 2"],
        c if two_hand.contains(&c) => vec!["Weapon 1"],
        c if c.ends_with("Flasks") => vec!["Flask 1", "Flask 2", "Flask 3", "Flask 4", "Flask 5"],
        _ => vec![],
    };
    v.into_iter().map(str::to_owned).collect()
}

// ---------------------------------------------------------------------------
// Comparison

#[derive(Debug, Clone, PartialEq)]
pub struct DiffRow {
    pub template: String,
    /// Display text in A / B (summed lines with the same template).
    pub a: Option<String>,
    pub b: Option<String>,
    /// A − B per value position, when both have the same number of values.
    pub deltas: Option<Vec<f64>>,
}

impl DiffRow {
    pub fn only_a(&self) -> bool {
        self.a.is_some() && self.b.is_none()
    }
    pub fn only_b(&self) -> bool {
        self.b.is_some() && self.a.is_none()
    }
}

/// Sum lines with the same template (stats add up in game).
fn aggregate(mods: &[ModLine]) -> Vec<(String, Vec<f64>, String)> {
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, (Vec<f64>, String, usize)> = HashMap::new();
    for m in mods {
        match map.get_mut(&m.template) {
            Some((vals, _, n)) if vals.len() == m.values.len() => {
                for (a, b) in vals.iter_mut().zip(&m.values) {
                    *a += b;
                }
                *n += 1;
            }
            Some(_) => {}
            None => {
                order.push(m.template.clone());
                map.insert(m.template.clone(), (m.values.clone(), m.text.clone(), 1));
            }
        }
    }
    order
        .into_iter()
        .map(|t| {
            let (vals, text, n) = map.remove(&t).unwrap_or_default();
            let text = if n > 1 { fill_template(&t, &vals) } else { text };
            (t, vals, text)
        })
        .collect()
}

fn fill_template(t: &str, vals: &[f64]) -> String {
    let mut it = vals.iter();
    let mut out = String::new();
    for (i, part) in t.split('#').enumerate() {
        if i > 0 {
            match it.next() {
                Some(v) => out.push_str(&fmt_num(*v)),
                None => out.push('#'),
            }
        }
        out.push_str(part);
    }
    out
}

pub fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-6 { format!("{}", v.round() as i64) } else { format!("{v:.1}") }
}

/// Compare A (e.g. the checked item) against B (the PoB item): rows in both
/// first (A order), then only-in-A, then only-in-B.
pub fn compare(a: &[ModLine], b: &[ModLine]) -> Vec<DiffRow> {
    let aa = aggregate(a);
    let mut bb = aggregate(b);
    let mut both = Vec::new();
    let mut only_a = Vec::new();
    for (t, av, at) in aa {
        if let Some(pos) = bb.iter().position(|(bt, _, _)| *bt == t) {
            let (_, bv, btext) = bb.remove(pos);
            let deltas = (av.len() == bv.len() && !av.is_empty()).then(|| av.iter().zip(&bv).map(|(x, y)| x - y).collect());
            both.push(DiffRow { template: t, a: Some(at), b: Some(btext), deltas });
        } else {
            only_a.push(DiffRow { template: t, a: Some(at), b: None, deltas: None });
        }
    }
    let only_b = bb.into_iter().map(|(t, _, bt)| DiffRow { template: t, a: None, b: Some(bt), deltas: None });
    both.into_iter().chain(only_a).chain(only_b).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const POB_HELM: &str = "Rarity: RARE\nGale Crown\nHubris Circlet\nUnique ID: 7f3a\nItem Level: 86\nQuality: 20\nSockets: B-B-B-B\nLevelReq: 69\nImplicits: 1\n{enchant}{tags:minion}Minions deal 10% increased Damage\n{tags:resource,life}+78 to maximum Life\n{tags:defences}+52 to maximum Energy Shield\n{tags:elemental,fire,resistance}+38% to Fire Resistance\n{crafted}+30 to maximum Life\n{fractured}+12% to Cold Resistance\nCorrupted";

    const GAME_HELM: &str = "Item Class: Helmets\nRarity: Rare\nDoom Visor\nHubris Circlet\n--------\nQuality: +20% (augmented)\nEnergy Shield: 250 (augmented)\n--------\nRequirements:\nLevel: 69\nInt: 154\n--------\nSockets: B-B-B-B \n--------\nItem Level: 84\n--------\nMinions deal 12% increased Damage (enchant)\n--------\n{ Prefix Modifier \"Virile\" (Tier: 4) — Life }\n+90(80-89) to maximum Life\n{ Suffix Modifier \"of the Kiln\" (Tier: 3) — Elemental, Fire, Resistance }\n+40(36-41)% to Fire Resistance\n+20% to Lightning Resistance (crafted)\n--------\nCorrupted\n";

    #[test]
    fn pob_item_normalization() {
        let it = parse_pob_item(POB_HELM);
        assert_eq!(it.rarity, Rarity::Rare);
        assert_eq!(it.name, "Gale Crown");
        assert_eq!(it.base.as_deref(), Some("Hubris Circlet"));
        assert!(it.corrupted);
        let texts: Vec<&str> = it.mods.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Minions deal 10% increased Damage",
                "+78 to maximum Life",
                "+52 to maximum Energy Shield",
                "+38% to Fire Resistance",
                "+30 to maximum Life",
                "+12% to Cold Resistance"
            ]
        );
        assert_eq!(it.mods[0].kind, ModKind::Enchant);
        assert_eq!(it.mods[1].kind, ModKind::Explicit, "enchant consumed the single implicit slot");
        assert!(it.mods[4].crafted);
        assert!(it.mods[5].fractured);
        assert_eq!(it.mods[1].template, "+# to maximum Life");
    }

    #[test]
    fn pob_unique_variants_and_ranges() {
        let text = "Rarity: UNIQUE\nCloak of Defiance\nLacquered Garb\nVariant: Pre 3.0.0\nVariant: Current\nSelected Variant: 2\nImplicits: 1\n{tags:life}+(15-25) to maximum Life\n{range:0.5}+(90-120) to maximum Mana\n{variant:1}{range:1}(100-120)% increased Evasion and Energy Shield\n{variant:2}{range:0.25}(140-180)% increased Evasion and Energy Shield\nMind Over Matter\n{range:1}Adds (1.5-2.5) to (3-4) Cold Damage";
        let it = parse_pob_item(text);
        let texts: Vec<&str> = it.mods.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "+20 to maximum Life",
                "+105 to maximum Mana",
                "150% increased Evasion and Energy Shield",
                "Mind Over Matter",
                "Adds 2.5 to 4 Cold Damage"
            ]
        );
        assert_eq!(it.mods[0].kind, ModKind::Implicit);
    }

    #[test]
    fn pob_magic_and_old_format() {
        let it = parse_pob_item("Rarity: MAGIC\nSeething Divine Life Flask of Staunching\nLevelReq: 60\nImplicits: 0\n50% increased Charge Recovery");
        assert_eq!(it.name, "Seething Divine Life Flask of Staunching");
        assert_eq!(it.base, None);
        assert_eq!(it.mods.len(), 1);
        // No Implicits line: property lines skipped.
        let old = parse_pob_item("Rarity: RARE\nX\nY\nQuality: 20\nItem Level: 80\n+10 to Strength");
        assert_eq!(old.mods.len(), 1);
        assert_eq!(old.mods[0].text, "+10 to Strength");
    }

    #[test]
    fn game_item_normalization() {
        let it = parse_game_item(GAME_HELM);
        assert_eq!(it.item_class.as_deref(), Some("Helmets"));
        assert_eq!(it.rarity, Rarity::Rare);
        assert_eq!(it.title(), "Doom Visor Hubris Circlet");
        assert!(it.corrupted);
        let texts: Vec<&str> = it.mods.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["Minions deal 12% increased Damage", "+90 to maximum Life", "+40% to Fire Resistance", "+20% to Lightning Resistance"]
        );
        assert_eq!(it.mods[0].kind, ModKind::Enchant);
        assert!(it.mods[3].crafted);
    }

    #[test]
    fn game_unique_skips_flavour() {
        let t = "Item Class: Body Armours\nRarity: Unique\nCloak of Defiance\nLacquered Garb\n--------\nEvasion Rating: 500\n--------\nItem Level: 80\n--------\n+110 to maximum Mana\nMind Over Matter\n--------\nWhen the world bears down\nthe defiant stand tall\n--------\nMirrored";
        let it = parse_game_item(t);
        let texts: Vec<&str> = it.mods.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, vec!["+110 to maximum Mana", "Mind Over Matter"]);
    }

    #[test]
    fn mod_comparison() {
        let game = parse_game_item(GAME_HELM);
        let pob = parse_pob_item(POB_HELM);
        let rows = compare(&game.mods, &pob.mods);
        let find = |t: &str| rows.iter().find(|r| r.template == t).unwrap_or_else(|| panic!("{t} in {rows:#?}"));
        // life: 90 vs 78 + 30 crafted = 108
        let life = find("+# to maximum Life");
        assert_eq!(life.b.as_deref(), Some("+108 to maximum Life"));
        assert_eq!(life.deltas, Some(vec![-18.0]));
        assert_eq!(find("+#% to Fire Resistance").deltas, Some(vec![2.0]));
        assert_eq!(find("Minions deal #% increased Damage").deltas, Some(vec![2.0]));
        assert!(find("+#% to Lightning Resistance").only_a());
        assert!(find("+# to maximum Energy Shield").only_b());
        assert!(find("+#% to Cold Resistance").only_b());
        // ordering: both, then only A, then only B
        let kinds: Vec<u8> = rows.iter().map(|r| if r.only_a() { 1 } else if r.only_b() { 2 } else { 0 }).collect();
        let mut sorted = kinds.clone();
        sorted.sort();
        assert_eq!(kinds, sorted);
    }

    #[test]
    fn item_class_slots() {
        assert_eq!(slots_for_item_class("Rings"), vec!["Ring 1", "Ring 2"]);
        assert_eq!(slots_for_item_class("Body Armours"), vec!["Body Armour"]);
        assert_eq!(slots_for_item_class("Bows"), vec!["Weapon 1"]);
        assert_eq!(slots_for_item_class("Wands"), vec!["Weapon 1", "Weapon 2"]);
        assert_eq!(slots_for_item_class("Utility Flasks").len(), 5);
        assert!(slots_for_item_class("Stackable Currency").is_empty());
    }
}
