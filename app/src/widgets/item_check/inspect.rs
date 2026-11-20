//! Item inspect (Rusty-only): per-modifier affix kind, tier/rank and roll
//! position for items copied with advanced mod descriptions (Ctrl+Alt+C).
//!
//! Tier data: the advanced copy only tells us the tier number (`Tier: 1` is
//! the best tier in PoE 1). The bundled game data (`stats.ndjson`) has no
//! per-mod tier counts for item affixes — its `tier`/`maxTier` keys only exist
//! for mercenary support gems — so we show "T3" without a "of N". The one
//! exception is eldritch implicits, whose six ranks are fixed by the client
//! strings (`ELDRITCH_MOD_R1..R6`).

use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, RichText, Sense, Ui, vec2};
use rpt_data::StatBetter;
use rpt_m3::{TypeRole, theme, with_alpha};
use rpt_parser::{ItemCategory, ItemRarity, ModGeneration, ModifierMechanic, ModifierType, ParsedItem, ParsedModifier, ParsedStat, RollGeneration};

/// Eldritch implicit ranks (Lesser … Exceptional) per client strings.
pub const ELDRITCH_RANKS: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierLabel {
    /// `(Tier: N)` — 1 is best.
    Tier(u32),
    /// `(Rank: N)` — crafted bench mods; higher is better, max unknown.
    Rank(u32),
    /// Eldritch implicit rank, out of [`ELDRITCH_RANKS`].
    EldritchRank(u32),
}

impl TierLabel {
    pub fn text(self) -> String {
        match self {
            Self::Tier(n) => format!("T{n}"),
            Self::Rank(n) => format!("R{n}"),
            Self::EldritchRank(n) => format!("R{n}/{ELDRITCH_RANKS}"),
        }
    }

    /// 0 = best, 1 = good, 2 = mediocre; `None` = no ordering known.
    pub fn grade(self) -> Option<u8> {
        match self {
            Self::Tier(1) => Some(0),
            Self::Tier(2 | 3) => Some(1),
            Self::Tier(_) => Some(2),
            Self::EldritchRank(n) if n >= ELDRITCH_RANKS => Some(0),
            Self::EldritchRank(n) if n + 2 >= ELDRITCH_RANKS => Some(1),
            Self::EldritchRank(_) => Some(2),
            Self::Rank(_) => None,
        }
    }

    pub fn tooltip(self) -> String {
        match self {
            Self::Tier(1) => "Tier 1 — the best tier for this modifier".to_owned(),
            Self::Tier(n) => format!("Tier {n} — {} tier{} below the best (Tier 1)", n - 1, if n == 2 { "" } else { "s" }),
            Self::Rank(n) => format!("Rank {n} — higher ranks are better"),
            Self::EldritchRank(n) => format!("Eldritch rank {n} of {ELDRITCH_RANKS} ({} is Exceptional)", ELDRITCH_RANKS),
        }
    }
}

/// Where a roll sits in its range: `0.0` = worst, `1.0` = best. Handles
/// reversed bounds, negative ranges and stats where lower is better. `None`
/// for fixed values (no range).
pub fn roll_percent(value: f64, min: f64, max: f64, better: StatBetter) -> Option<f32> {
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    if !(hi - lo > f64::EPSILON) || !value.is_finite() {
        return None;
    }
    let t = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    Some(match better {
        StatBetter::NegativeRoll => 1.0 - t,
        _ => t,
    } as f32)
}

#[derive(Debug, Clone, PartialEq)]
pub struct RollRow {
    /// Stat text as shown in game (values filled in when unambiguous).
    pub text: String,
    /// Displayed value and bounds (sign follows the shown text).
    pub value: f64,
    pub min: f64,
    pub max: f64,
    /// 0..1, 1 = best roll. `None` → fixed value.
    pub pct: Option<f32>,
    pub better: StatBetter,
    pub legacy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModRow {
    pub kind: ModifierType,
    pub generation: Option<ModGeneration>,
    pub mechanic: Option<ModifierMechanic>,
    pub name: Option<String>,
    pub tier: Option<TierLabel>,
    pub tags: Vec<String>,
    pub rolls: Vec<RollRow>,
}

impl ModRow {
    /// Occupies a prefix/suffix slot.
    pub fn is_affix(&self) -> bool {
        matches!(self.kind, ModifierType::Explicit | ModifierType::Crafted | ModifierType::Fractured | ModifierType::Veiled)
            && self.mechanic != Some(ModifierMechanic::Foulborn)
    }

    pub fn kind_label(&self) -> &'static str {
        match (self.kind, self.generation) {
            (ModifierType::Crafted, Some(ModGeneration::Prefix)) => "Crafted P",
            (ModifierType::Crafted, Some(ModGeneration::Suffix)) => "Crafted S",
            (ModifierType::Fractured, Some(ModGeneration::Prefix)) => "Fractured P",
            (ModifierType::Fractured, Some(ModGeneration::Suffix)) => "Fractured S",
            (_, Some(ModGeneration::Prefix)) => "Prefix",
            (_, Some(ModGeneration::Suffix)) => "Suffix",
            (ModifierType::Implicit, _) => "Implicit",
            (ModifierType::Enchant, _) => "Enchant",
            (ModifierType::Crafted, _) => "Crafted",
            (ModifierType::Fractured, _) => "Fractured",
            (ModifierType::Scourge, _) => "Scourge",
            (ModifierType::Necropolis, _) => "Necropolis",
            (ModifierType::Veiled, _) => "Veiled",
            (ModifierType::Imbued, _) => "Imbued",
            (ModifierType::Pseudo, _) => "Pseudo",
            (ModifierType::Explicit, _) => "Explicit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inspect {
    pub rows: Vec<ModRow>,
    pub unique: bool,
    pub prefixes: u32,
    pub suffixes: u32,
    /// `(max prefixes, max suffixes)` when the rarity/category defines it and
    /// every affix has a known generation.
    pub max_affixes: Option<(u32, u32)>,
    /// Mean of comparable explicit (affix / unique) roll percents.
    pub avg_pct: Option<f32>,
}

impl Inspect {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn open_prefixes(&self) -> Option<u32> {
        self.max_affixes.map(|(p, _)| p.saturating_sub(self.prefixes))
    }

    pub fn open_suffixes(&self) -> Option<u32> {
        self.max_affixes.map(|(_, s)| s.saturating_sub(self.suffixes))
    }

    /// e.g. "2 prefixes (1 open) · 3 suffixes · avg roll 74 %".
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.unique && (self.prefixes > 0 || self.suffixes > 0 || self.max_affixes.is_some()) {
            let plural = |n: u32, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "es" });
            let open = |o: Option<u32>| match o {
                Some(0) | None => String::new(),
                Some(n) => format!(" ({n} open)"),
            };
            parts.push(format!("{}{}", plural(self.prefixes, "prefix"), open(self.open_prefixes())));
            parts.push(format!("{}{}", plural(self.suffixes, "suffix"), open(self.open_suffixes())));
        }
        if let Some(a) = self.avg_pct {
            parts.push(format!("avg roll {:.0} %", a * 100.0));
        }
        parts.join(" · ")
    }
}

fn max_affixes(item: &ParsedItem) -> Option<(u32, u32)> {
    let jewel = matches!(item.category, Some(ItemCategory::Jewel | ItemCategory::AbyssJewel | ItemCategory::ClusterJewel));
    match item.rarity? {
        ItemRarity::Magic => Some((1, 1)),
        ItemRarity::Rare if jewel => Some((2, 2)),
        ItemRarity::Rare => Some((3, 3)),
        _ => None,
    }
}

fn tier_label(m: &ParsedModifier) -> Option<TierLabel> {
    let i = &m.info;
    if i.mechanic == Some(ModifierMechanic::Eldritch) {
        return i.rank.map(TierLabel::EldritchRank);
    }
    i.tier.map(TierLabel::Tier).or(i.rank.map(TierLabel::Rank))
}

/// Fill the `#` placeholder with the value when the stat has exactly one.
fn stat_text(stat: &ParsedStat, shown: f64) -> String {
    let s = &stat.translation.string;
    if s.matches('#').count() != 1 {
        return s.clone();
    }
    // Matchers drop the sign of "+#" stats (the roll is captured signed).
    let num = rpt_m3::fmt_number(shown);
    let num = if s.starts_with('#') && stat.stat.ref_.starts_with("+#") && shown >= 0.0 { format!("+{num}") } else { num };
    s.replacen('#', &num, 1)
}

fn roll_row(stat: &ParsedStat) -> Option<RollRow> {
    let r = stat.roll.as_ref()?;
    // Rolls are stored in stat space (negated for "reduced"-style translations);
    // show them with the sign of the text the player sees.
    let sign = if stat.translation.negate { -1.0 } else { 1.0 };
    let (a, b) = (r.min * sign, r.max * sign);
    let better = stat.stat.better;
    Some(RollRow {
        text: stat_text(stat, r.value * sign),
        value: r.value * sign,
        min: a.min(b),
        max: a.max(b),
        pct: roll_percent(r.value, r.min, r.max, better),
        better,
        legacy: matches!(r.generation, Some(RollGeneration::Legacy | RollGeneration::Volatile)),
    })
}

/// Build the inspect model for an item.
pub fn inspect(item: &ParsedItem) -> Inspect {
    let unique = item.rarity == Some(ItemRarity::Unique);
    let mut out = Inspect { unique, ..Default::default() };
    let mut sum = 0.0f32;
    let mut n = 0u32;
    let mut all_generations_known = true;
    for m in &item.new_mods {
        if m.info.type_ == ModifierType::Pseudo {
            continue;
        }
        let row = ModRow {
            kind: m.info.type_,
            generation: m.info.generation,
            mechanic: m.info.mechanic,
            name: m.info.name.clone().filter(|s| !s.is_empty()),
            tier: if unique { None } else { tier_label(m) },
            tags: m.info.tags.clone(),
            rolls: m.stats.iter().filter_map(roll_row).collect(),
        };
        if row.is_affix() {
            match row.generation {
                Some(ModGeneration::Prefix) => out.prefixes += 1,
                Some(ModGeneration::Suffix) => out.suffixes += 1,
                None => all_generations_known = false,
            }
        }
        let explicit = row.is_affix() || (unique && row.kind == ModifierType::Explicit);
        if explicit {
            for r in &row.rolls {
                if let (Some(p), true) = (r.pct, r.better != StatBetter::NotComparable) {
                    sum += p;
                    n += 1;
                }
            }
        }
        out.rows.push(row);
    }
    if !unique && all_generations_known {
        out.max_affixes = max_affixes(item);
    }
    out.avg_pct = (n > 0).then(|| sum / n as f32);
    out
}

// ---------------------------------------------------------------------------
// UI

/// Bar fill/label color by roll quality.
fn pct_color(c: &rpt_m3::ColorScheme, pct: f32, comparable: bool) -> egui::Color32 {
    if !comparable {
        c.tertiary
    } else if pct >= 0.9 {
        c.primary
    } else if pct >= 0.5 {
        c.secondary
    } else {
        c.outline
    }
}

/// Small linear roll bar (M3 linear progress look) + percent label.
fn roll_bar(ui: &mut Ui, r: &RollRow) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let Some(pct) = r.pct else {
        ui.label(RichText::new("fixed").font(th.font(TypeRole::LabelSmall)).color(c.outline));
        return;
    };
    let comparable = r.better != StatBetter::NotComparable;
    let col = pct_color(&c, pct, comparable);
    ui.label(RichText::new(format!("{:>3.0}%", pct * 100.0)).font(th.font(TypeRole::LabelMedium)).color(col));
    let (rect, resp) = ui.allocate_exact_size(vec2(64.0, 14.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
        let p = ui.painter();
        p.rect_filled(track, CornerRadius::same(2), c.surface_container_highest);
        let fill = Rect::from_min_size(track.min, vec2(track.width() * pct.clamp(0.0, 1.0), 4.0));
        if fill.width() > 0.0 {
            p.rect_filled(fill, CornerRadius::same(2), col);
        }
        // stop indicator at the best end (M3 linear progress stop dot)
        p.circle_filled(egui::pos2(track.right() - 2.0, track.center().y), 2.0, with_alpha(col, 0.6));
    }
    let dir = match r.better {
        StatBetter::NegativeRoll => "lower is better",
        StatBetter::PositiveRoll => "higher is better",
        StatBetter::NotComparable => "no better direction",
    };
    let legacy = if r.legacy { "\nLegacy roll outside the current range" } else { "" };
    resp.on_hover_text(format!(
        "{} in {}–{} · {dir}{legacy}",
        rpt_m3::fmt_number(r.value),
        rpt_m3::fmt_number(r.min),
        rpt_m3::fmt_number(r.max)
    ));
}

fn kind_colors(c: &rpt_m3::ColorScheme, row: &ModRow) -> (egui::Color32, egui::Color32) {
    match (row.kind, row.generation) {
        (ModifierType::Crafted, _) => (c.tertiary_container, c.on_tertiary_container),
        (ModifierType::Fractured, _) => (with_alpha(c.secondary, 0.35), c.on_surface),
        (_, Some(_)) => (c.secondary_container, c.on_secondary_container),
        _ => (c.surface_container_highest, c.on_surface_variant),
    }
}

fn tier_colors(c: &rpt_m3::ColorScheme, t: TierLabel) -> (egui::Color32, egui::Color32) {
    match t.grade() {
        Some(0) => (c.primary, c.on_primary),
        Some(1) => (c.primary_container, c.on_primary_container),
        Some(_) => (c.surface_container_highest, c.on_surface_variant),
        None => (c.tertiary_container, c.on_tertiary_container),
    }
}

/// The inspect card. `max_height` bounds the scrollable mod list.
pub fn show(ui: &mut Ui, ins: &Inspect, max_height: f32) {
    if ins.is_empty() {
        return;
    }
    let th = theme(ui.ctx());
    let c = th.colors;
    Frame::new().fill(c.surface_container).corner_radius(CornerRadius::same(rpt_m3::shape::MEDIUM)).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Inspect").font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
            let summary = ins.summary();
            if !summary.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(summary).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant)).truncate());
                });
            }
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("rpt-item-inspect").max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
            for (i, row) in ins.rows.iter().enumerate() {
                if i > 0 {
                    ui.add_space(2.0);
                    let y = ui.cursor().top();
                    ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, c.outline_variant));
                    ui.add_space(4.0);
                }
                mod_row(ui, row);
            }
        });
    });
}

fn mod_row(ui: &mut Ui, row: &ModRow) {
    let th = theme(ui.ctx());
    let c = th.colors;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let (bg, fg) = kind_colors(&c, row);
        rpt_m3::tonal_badge(ui, row.kind_label(), bg, fg);
        if let Some(t) = row.tier {
            let (bg, fg) = tier_colors(&c, t);
            rpt_m3::tonal_badge(ui, &t.text(), bg, fg).on_hover_text(t.tooltip());
        }
        if let Some(name) = &row.name {
            ui.label(RichText::new(name).font(th.font(TypeRole::LabelLarge)).color(c.on_surface));
        }
        for tag in &row.tags {
            let g = ui.painter().layout_no_wrap(tag.clone(), th.font(TypeRole::LabelSmall), c.on_surface_variant);
            let (r, _) = ui.allocate_exact_size(vec2(g.size().x + 8.0, g.size().y + 2.0), Sense::hover());
            ui.painter().rect_stroke(r, CornerRadius::same(rpt_m3::shape::SMALL), egui::Stroke::new(1.0, c.outline_variant), egui::StrokeKind::Inside);
            ui.painter().galley(r.center() - g.size() / 2.0, g, c.on_surface_variant);
        }
    });
    for r in &row.rolls {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                roll_bar(ui, r);
                ui.with_layout(Layout::left_to_right(Align::Center).with_main_wrap(true), |ui| {
                    ui.add(egui::Label::new(RichText::new(&r.text).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant)).wrap());
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use StatBetter::*;

    fn parse(text: &str) -> ParsedItem {
        rpt_data::init("en").unwrap();
        rpt_parser::parse_clipboard(text).unwrap_or_else(|e| panic!("failed to parse: {e}\n{text}"))
    }

    fn approx(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-4)
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

    const MAGIC_JEWEL: &str = "Item Class: Jewels
Rarity: Magic
Vivid Cobalt Jewel of the Lynx
--------
Item Level: 84
--------
{ Prefix Modifier \"Vivid\" (Tier: 1) — Life }
7(5-7)% increased maximum Life
--------
Place into an allocated Jewel Socket on the Passive Skill Tree. Right click to remove from the Socket.
";

    const UNIQUE: &str = "Item Class: Two Hand Swords
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
";

    #[test]
    fn roll_percent_math() {
        assert!(approx(roll_percent(89.0, 80.0, 91.0, PositiveRoll), 9.0 / 11.0));
        assert!(approx(roll_percent(80.0, 80.0, 91.0, PositiveRoll), 0.0));
        assert!(approx(roll_percent(91.0, 80.0, 91.0, PositiveRoll), 1.0));
        // reversed bounds are normalised
        assert!(approx(roll_percent(89.0, 91.0, 80.0, PositiveRoll), 9.0 / 11.0));
        // negative range, higher (closer to zero) is better
        assert!(approx(roll_percent(-10.0, -15.0, -5.0, PositiveRoll), 0.5));
        assert!(approx(roll_percent(-6.0, -15.0, -5.0, PositiveRoll), 0.9));
        // lower is better (e.g. "#% increased Mana Cost" stored as -12..-8 for "reduced")
        assert!(approx(roll_percent(-12.0, -12.0, -8.0, NegativeRoll), 1.0));
        assert!(approx(roll_percent(-10.0, -12.0, -8.0, NegativeRoll), 0.5));
        assert!(approx(roll_percent(10.0, 8.0, 12.0, NegativeRoll), 0.5));
        assert!(approx(roll_percent(8.0, 8.0, 12.0, NegativeRoll), 1.0));
        // range crossing zero
        assert!(approx(roll_percent(0.0, -10.0, 10.0, PositiveRoll), 0.5));
        // out-of-range (legacy) values clamp
        assert!(approx(roll_percent(120.0, 80.0, 91.0, PositiveRoll), 1.0));
        // fixed values have no position
        assert_eq!(roll_percent(5.0, 5.0, 5.0, PositiveRoll), None);
        assert_eq!(roll_percent(f64::NAN, 1.0, 5.0, PositiveRoll), None);
    }

    #[test]
    fn rare_armour_tiers_and_slots() {
        let ins = inspect(&parse(RARE_ARMOUR));
        assert!(!ins.unique);
        let tiers: Vec<_> = ins.rows.iter().map(|r| r.tier).collect();
        use TierLabel::*;
        assert_eq!(tiers, vec![Some(Tier(4)), Some(Tier(8)), Some(Tier(3)), Some(Tier(3)), Some(Tier(4)), Some(Rank(2))]);
        assert_eq!(ins.rows[0].kind, ModifierType::Fractured);
        assert_eq!(ins.rows[0].kind_label(), "Fractured P");
        assert_eq!(ins.rows[1].name.as_deref(), Some("Hale"));
        assert_eq!(ins.rows[3].tags, vec!["Elemental", "Fire", "Resistance"]);
        assert_eq!((ins.prefixes, ins.suffixes), (3, 3));
        assert_eq!(ins.max_affixes, Some((3, 3)));
        assert_eq!((ins.open_prefixes(), ins.open_suffixes()), (Some(0), Some(0)));
        // life 29 in 25-29 → 100 %
        let life = &ins.rows[1].rolls[0];
        assert_eq!(life.text, "+29 to maximum Life");
        assert!(approx(life.pct, 1.0));
        // strength 38 in 38-42 → 0 %
        assert!(approx(ins.rows[4].rolls[0].pct, 0.0));
        let avg = ins.avg_pct.unwrap();
        let expect = [1.0 / 19.0, 1.0, 6.0 / 11.0, 4.0 / 5.0, 0.0, 3.0 / 7.0].iter().sum::<f32>() / 6.0;
        assert!((avg - expect).abs() < 1e-4, "{avg} vs {expect}");
        assert!(ins.summary().starts_with("3 prefixes · 3 suffixes · avg roll "), "{}", ins.summary());
        assert_eq!(TierLabel::Tier(1).grade(), Some(0));
        assert_eq!(TierLabel::Tier(3).grade(), Some(1));
        assert_eq!(TierLabel::Tier(8).grade(), Some(2));
        assert_eq!(TierLabel::Rank(2).grade(), None);
    }

    #[test]
    fn crafted_mod_is_ranked() {
        let ins = inspect(&parse(RARE_ARMOUR));
        let crafted = ins.rows.iter().find(|r| r.kind == ModifierType::Crafted).unwrap();
        assert_eq!(crafted.tier, Some(TierLabel::Rank(2)));
        assert_eq!(crafted.tier.unwrap().text(), "R2");
        assert_eq!(crafted.kind_label(), "Crafted S");
        assert!(crafted.is_affix());
        assert!(approx(crafted.rolls[0].pct, 3.0 / 7.0));
    }

    #[test]
    fn magic_jewel_slots() {
        let item = parse(MAGIC_JEWEL);
        assert_eq!(item.category, Some(ItemCategory::Jewel));
        let ins = inspect(&item);
        assert_eq!(ins.rows.len(), 1);
        assert_eq!(ins.rows[0].tier, Some(TierLabel::Tier(1)));
        assert_eq!(ins.rows[0].generation, Some(ModGeneration::Prefix));
        assert_eq!(ins.max_affixes, Some((1, 1)));
        assert_eq!((ins.open_prefixes(), ins.open_suffixes()), (Some(0), Some(1)));
        assert!(approx(ins.avg_pct, 1.0));
        assert_eq!(ins.summary(), "1 prefix · 0 suffixes (1 open) · avg roll 100 %");
    }

    #[test]
    fn unique_rolls_without_tiers() {
        let ins = inspect(&parse(UNIQUE));
        assert!(ins.unique);
        assert!(ins.rows.iter().all(|r| r.tier.is_none()));
        assert_eq!(ins.max_affixes, None);
        // legacy 520 stretched the range to 400-520 → best
        let phys = &ins.rows[0].rolls[0];
        assert!(phys.legacy);
        assert!(approx(phys.pct, 1.0));
        let life = &ins.rows[1].rolls[0];
        assert!(approx(life.pct, 0.5));
        assert!(ins.summary().starts_with("avg roll"), "{}", ins.summary());
    }

    #[test]
    fn eldritch_rank_out_of_six() {
        let ins = inspect(&parse(
            "Item Class: Helmets
Rarity: Rare
Hate Veil
Hubris Circlet
--------
Energy Shield: 150
--------
Item Level: 85
--------
{ Searing Exarch Implicit Modifier (Greater) — Damage }
12(11-13)% increased Damage
--------
{ Prefix Modifier \"Hale\" (Tier: 8) — Life }
+14(10-19) to maximum Life
",
        ));
        assert_eq!(ins.rows[0].tier, Some(TierLabel::EldritchRank(2)));
        assert_eq!(ins.rows[0].tier.unwrap().text(), "R2/6");
        assert_eq!(ins.rows[0].kind_label(), "Implicit");
        // implicits don't count towards affixes or the average
        assert_eq!((ins.prefixes, ins.suffixes), (1, 0));
        assert!(approx(ins.avg_pct, 4.0 / 9.0));
    }

    #[test]
    fn renders_headless() {
        let ctx = egui::Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let items: Vec<Inspect> = [RARE_ARMOUR, MAGIC_JEWEL, UNIQUE].iter().map(|t| inspect(&parse(t))).collect();
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(480.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                ui.set_max_width(440.0);
                for ins in &items {
                    show(ui, ins, 400.0);
                }
            });
            out.textures_delta.clear();
        }
    }
}
