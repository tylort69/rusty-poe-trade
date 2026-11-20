//! Path of Building build model and a tolerant XML parser (pure, no UI).
//!
//! Covers `<Build>`, `<Skills>` (with or without `<SkillSet>`), `<Items>`
//! (`<Item>`, `<Slot>`, `<ItemSet>`), `<Tree>` / `<Spec>`, `<Notes>` and
//! `<Config>` (with or without `<ConfigSet>`). Unknown elements and
//! attributes are ignored, missing ones default.

use std::collections::HashMap;
use std::io::Read as _;

use base64::Engine as _;
use quick_xml::events::{BytesStart, Event};

/// Max decompressed size of an import code (zip bomb guard).
const MAX_XML: u64 = 64 * 1024 * 1024;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Build {
    pub level: Option<u32>,
    pub class_name: String,
    pub ascend_class_name: String,
    pub bandit: Option<String>,
    /// 1-based index into the active skill set's groups, as PoB stores it.
    pub main_socket_group: Option<usize>,
    pub target_version: Option<String>,
    /// `<PlayerStat>` in document order.
    pub stats: Vec<(String, f64)>,
    pub minion_stats: Vec<(String, f64)>,
    pub skill_sets: Vec<SkillSet>,
    /// Index into `skill_sets`.
    pub active_skill_set: usize,
    pub items: Vec<Item>,
    /// Item sets; older builds without `<ItemSet>` get one synthetic set from the top-level slots.
    pub item_sets: Vec<ItemSet>,
    /// Index into `item_sets`.
    pub active_item_set: usize,
    pub specs: Vec<Spec>,
    /// Index into `specs`.
    pub active_spec: usize,
    /// Raw notes (still containing PoB color codes).
    pub notes: String,
    pub config_sets: Vec<ConfigSet>,
    pub active_config_set: usize,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SkillSet {
    pub id: String,
    pub title: String,
    pub groups: Vec<SkillGroup>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SkillGroup {
    pub slot: Option<String>,
    pub label: Option<String>,
    pub enabled: bool,
    /// 1-based index among the group's active gems.
    pub main_active_skill: Option<usize>,
    pub include_in_full_dps: bool,
    /// Granted by an item (e.g. "Item:3:...").
    pub source: Option<String>,
    pub gems: Vec<Gem>,
}

impl SkillGroup {
    /// Display title: label, else first active gem names.
    pub fn title(&self) -> String {
        if let Some(l) = self.label.as_deref().filter(|l| !l.trim().is_empty()) {
            return l.to_owned();
        }
        let actives: Vec<&str> = self.gems.iter().filter(|g| !g.support).map(|g| g.name.as_str()).collect();
        if actives.is_empty() {
            self.gems.first().map(|g| g.name.clone()).unwrap_or_else(|| "<no gems>".into())
        } else {
            actives.join(", ")
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Gem {
    pub name: String,
    pub skill_id: Option<String>,
    pub gem_id: Option<String>,
    pub level: Option<u32>,
    pub quality: Option<u32>,
    /// Alternate quality ("Anomalous" etc.; "Default" is mapped to None).
    pub quality_id: Option<String>,
    pub enabled: bool,
    pub support: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Item {
    pub id: u32,
    /// Item text with indentation trimmed, one line per mod/property.
    pub text: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ItemSet {
    pub id: String,
    pub title: String,
    /// (slot name, item id); empty slots (id 0) are omitted.
    pub slots: Vec<(String, u32)>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Spec {
    pub title: String,
    pub tree_version: Option<String>,
    pub nodes: Vec<u32>,
    pub class_id: Option<u32>,
    pub ascend_class_id: Option<u32>,
    pub secondary_ascend_class_id: Option<u32>,
    /// (mastery node, effect id)
    pub mastery_effects: Vec<(u32, u32)>,
    pub url: Option<String>,
    /// (jewel socket node, item id)
    pub sockets: Vec<(u32, u32)>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ConfigSet {
    pub title: String,
    /// (input name, value as text)
    pub inputs: Vec<(String, String)>,
}

impl Build {
    pub fn stat(&self, name: &str) -> Option<f64> {
        self.stats.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
    }

    pub fn skill_set(&self) -> Option<&SkillSet> {
        self.skill_sets.get(self.active_skill_set).or(self.skill_sets.first())
    }

    pub fn item_set(&self) -> Option<&ItemSet> {
        self.item_sets.get(self.active_item_set).or(self.item_sets.first())
    }

    pub fn item(&self, id: u32) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn spec(&self) -> Option<&Spec> {
        self.specs.get(self.active_spec).or(self.specs.first())
    }

    /// "Necromancer" / "Witch" / "Level 92 Necromancer".
    pub fn title(&self) -> String {
        let class = if !self.ascend_class_name.is_empty() && self.ascend_class_name != "None" {
            self.ascend_class_name.as_str()
        } else if !self.class_name.is_empty() {
            self.class_name.as_str()
        } else {
            "Unknown class"
        };
        match self.level {
            Some(l) => format!("Level {l} {class}"),
            None => class.to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Import codes

/// Decode a PoB export code (URL-safe or standard base64 of zlib-deflated XML).
/// Raw XML is passed through unchanged.
pub fn decode_code(code: &str) -> Result<String, String> {
    let trimmed = code.trim();
    if trimmed.starts_with('<') {
        return Ok(trimmed.to_owned());
    }
    let mut b64: String = trimmed.chars().filter(|c| !c.is_whitespace()).map(|c| match c {
        '-' => '+',
        '_' => '/',
        c => c,
    }).collect();
    while b64.ends_with('=') {
        b64.pop();
    }
    if b64.is_empty() {
        return Err("Empty code".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(b64.as_bytes())
        .map_err(|e| format!("Not a valid build code (base64: {e})"))?;
    let mut out = Vec::new();
    let zlib = flate2::read::ZlibDecoder::new(bytes.as_slice()).take(MAX_XML).read_to_end(&mut out);
    if zlib.is_err() || out.is_empty() {
        out.clear();
        flate2::read::DeflateDecoder::new(bytes.as_slice())
            .take(MAX_XML)
            .read_to_end(&mut out)
            .map_err(|e| format!("Not a valid build code (inflate: {e})"))?;
    }
    String::from_utf8(out).map_err(|_| "Build code does not contain UTF-8 text".to_owned())
}

/// Encode XML the way PoB does (zlib + URL-safe base64). Used by tests and "copy code".
#[allow(dead_code)]
pub fn encode_code(xml: &str) -> String {
    use std::io::Write as _;
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = enc.write_all(xml.as_bytes());
    let bytes = enc.finish().unwrap_or_default();
    base64::engine::general_purpose::URL_SAFE.encode(bytes)
}

/// Where to fetch a pasted link from, if it is one.
pub fn import_url(input: &str) -> Option<String> {
    let s = input.trim();
    let rest = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let id_of = |p: &str| -> Option<String> {
        let id = p.split(['/', '?', '#']).find(|x| !x.is_empty())?;
        id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-').then(|| id.to_owned())
    };
    if let Some(p) = rest.strip_prefix("pobb.in/") {
        return id_of(p).map(|id| format!("https://pobb.in/{id}/raw"));
    }
    if let Some(p) = rest.strip_prefix("pastebin.com/") {
        let p = p.strip_prefix("raw/").unwrap_or(p);
        return id_of(p).map(|id| format!("https://pastebin.com/raw/{id}"));
    }
    None
}

// ---------------------------------------------------------------------------
// Notes color codes

/// Strip `^0`–`^9` and `^xRRGGBB` color codes.
pub fn strip_colors(s: &str) -> String {
    color_spans(s).into_iter().map(|(_, t)| t).collect()
}

/// Split text into (color, text) runs. `None` = default color.
pub fn color_spans(s: &str) -> Vec<(Option<[u8; 3]>, String)> {
    let mut out: Vec<(Option<[u8; 3]>, String)> = Vec::new();
    let mut cur: Option<[u8; 3]> = None;
    let mut buf = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if b[i] == b'^' {
            let next = b.get(i + 1).copied();
            let code = match next {
                Some(d @ b'0'..=b'9') => Some((digit_color(d - b'0'), 2)),
                Some(b'x' | b'X') if i + 8 <= s.len() && b[i + 2..i + 8].iter().all(u8::is_ascii_hexdigit) => {
                    let h = &s[i + 2..i + 8];
                    let p = |k: usize| u8::from_str_radix(&h[k..k + 2], 16).unwrap_or(0);
                    Some((Some([p(0), p(2), p(4)]), 8))
                }
                _ => None,
            };
            if let Some((c, len)) = code {
                if !buf.is_empty() {
                    out.push((cur, std::mem::take(&mut buf)));
                }
                cur = c;
                i += len;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap_or(' ');
        buf.push(ch);
        i += ch.len_utf8();
    }
    if !buf.is_empty() {
        out.push((cur, buf));
    }
    out
}

/// PoB's `colorCodes` for `^0`..`^9`; `^7` (normal) maps to the default color.
fn digit_color(d: u8) -> Option<[u8; 3]> {
    Some(match d {
        0 => [0, 0, 0],
        1 => [0xff, 0, 0],
        2 => [0, 0xff, 0],
        3 => [0, 0, 0xff],
        4 => [0xff, 0xff, 0],
        5 => [0xff, 0, 0xff],
        6 => [0, 0xff, 0xff],
        8 => [0x80, 0x80, 0x80],
        9 => [0x40, 0x40, 0x40],
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// XML

fn attrs(e: &BytesStart<'_>) -> HashMap<String, String> {
    e.attributes()
        .with_checks(false)
        .flatten()
        .filter_map(|a| {
            let k = String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned();
            let v = a.unescape_value().map(|v| v.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned());
            Some((k, v))
        })
        .collect()
}

fn num<T: std::str::FromStr>(a: &HashMap<String, String>, k: &str) -> Option<T> {
    a.get(k).and_then(|v| v.trim().parse().ok())
}

fn flag(a: &HashMap<String, String>, k: &str, default: bool) -> bool {
    match a.get(k).map(|s| s.trim()) {
        Some("true" | "1") => true,
        Some("false" | "0") => false,
        _ => default,
    }
}

fn non_empty(a: &HashMap<String, String>, k: &str) -> Option<String> {
    a.get(k).map(|s| s.trim()).filter(|s| !s.is_empty() && *s != "nil").map(str::to_owned)
}

fn is_support(a: &HashMap<String, String>, name: &str) -> bool {
    let gem_id = a.get("gemId").map(String::as_str).unwrap_or("");
    let skill_id = a.get("skillId").map(String::as_str).unwrap_or("");
    gem_id.contains("SupportGem") || gem_id.contains("/Support") || skill_id.starts_with("Support") || name.ends_with(" Support")
}

/// Text capture target.
#[derive(Clone, Copy, PartialEq)]
enum Capture {
    None,
    Item,
    Notes,
    Url,
}

/// Parse a PoB build XML document.
pub fn parse_build(xml: &str) -> Result<Build, String> {
    let mut r = quick_xml::Reader::from_str(xml);
    r.config_mut().check_end_names = false;
    let mut b = Build::default();
    let mut stack: Vec<String> = Vec::new();
    let mut seen_root = false;
    let mut active_skill_set_id: Option<String> = None;
    let mut active_item_set_id: Option<String> = None;
    let mut active_spec: Option<usize> = None;
    let mut active_config_id: Option<String> = None;
    let mut config_ids: Vec<String> = Vec::new();
    let mut top_slots: Vec<(String, u32)> = Vec::new();
    let mut capture = Capture::None;
    let mut text = String::new();
    let mut cur_item_id = 0u32;
    let mut has_config_sets = false;

    loop {
        let ev = r.read_event().map_err(|e| format!("XML error at {}: {e}", r.error_position()))?;
        match ev {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let parent = stack.last().map(String::as_str).unwrap_or("");
                let a = attrs(e);
                if name == "PathOfBuilding" || name == "PathOfBuilding2" {
                    seen_root = true;
                }
                match (name.as_str(), parent) {
                    ("Build", _) => {
                        seen_root = true;
                        b.level = num(&a, "level");
                        b.class_name = a.get("className").cloned().unwrap_or_default();
                        b.ascend_class_name = a.get("ascendClassName").cloned().unwrap_or_default();
                        b.bandit = non_empty(&a, "bandit");
                        b.main_socket_group = num(&a, "mainSocketGroup");
                        b.target_version = non_empty(&a, "targetVersion");
                    }
                    ("PlayerStat", "Build") => {
                        if let (Some(k), Some(v)) = (a.get("stat"), num::<f64>(&a, "value")) {
                            b.stats.push((k.clone(), v));
                        }
                    }
                    ("MinionStat", "Build") => {
                        if let (Some(k), Some(v)) = (a.get("stat"), num::<f64>(&a, "value")) {
                            b.minion_stats.push((k.clone(), v));
                        }
                    }
                    ("Skills", _) => active_skill_set_id = non_empty(&a, "activeSkillSet"),
                    ("SkillSet", "Skills") => b.skill_sets.push(SkillSet {
                        id: a.get("id").cloned().unwrap_or_default(),
                        title: a.get("title").cloned().unwrap_or_default(),
                        groups: Vec::new(),
                    }),
                    ("Skill", "Skills" | "SkillSet") => {
                        if parent == "Skills" && b.skill_sets.is_empty() {
                            // Older PoB: groups directly under <Skills>.
                            b.skill_sets.push(SkillSet { id: "1".into(), title: String::new(), groups: Vec::new() });
                        }
                        if let Some(set) = b.skill_sets.last_mut() {
                            set.groups.push(SkillGroup {
                                slot: non_empty(&a, "slot"),
                                label: non_empty(&a, "label"),
                                enabled: flag(&a, "enabled", true),
                                main_active_skill: num(&a, "mainActiveSkill"),
                                include_in_full_dps: flag(&a, "includeInFullDPS", false),
                                source: non_empty(&a, "source"),
                                gems: Vec::new(),
                            });
                        }
                    }
                    ("Gem", "Skill") => {
                        let name = non_empty(&a, "nameSpec")
                            .or_else(|| non_empty(&a, "skillId"))
                            .or_else(|| a.get("gemId").and_then(|g| g.rsplit('/').next()).map(str::to_owned))
                            .unwrap_or_else(|| "?".into());
                        let gem = Gem {
                            support: is_support(&a, &name),
                            name,
                            skill_id: non_empty(&a, "skillId"),
                            gem_id: non_empty(&a, "gemId"),
                            level: num(&a, "level"),
                            quality: num(&a, "quality"),
                            quality_id: non_empty(&a, "qualityId").filter(|q| q != "Default"),
                            enabled: flag(&a, "enabled", true),
                        };
                        if let Some(g) = b.skill_sets.last_mut().and_then(|s| s.groups.last_mut()) {
                            g.gems.push(gem);
                        }
                    }
                    ("Items", _) => active_item_set_id = non_empty(&a, "activeItemSet"),
                    ("Item", "Items") => {
                        cur_item_id = num(&a, "id").unwrap_or(0);
                        if empty {
                            b.items.push(Item { id: cur_item_id, text: String::new() });
                        } else {
                            capture = Capture::Item;
                            text.clear();
                        }
                    }
                    ("ItemSet", "Items") => b.item_sets.push(ItemSet {
                        id: a.get("id").cloned().unwrap_or_default(),
                        title: a.get("title").cloned().unwrap_or_default(),
                        slots: Vec::new(),
                    }),
                    ("Slot", "Items" | "ItemSet") => {
                        let id: u32 = num(&a, "itemId").unwrap_or(0);
                        if let (Some(n), true) = (a.get("name"), id != 0) {
                            if parent == "Items" {
                                top_slots.push((n.clone(), id));
                            } else if let Some(s) = b.item_sets.last_mut() {
                                s.slots.push((n.clone(), id));
                            }
                        }
                    }
                    ("Tree", _) => active_spec = num::<usize>(&a, "activeSpec").map(|x| x.saturating_sub(1)),
                    ("Spec", "Tree") => b.specs.push(Spec {
                        title: a.get("title").cloned().unwrap_or_default(),
                        tree_version: non_empty(&a, "treeVersion"),
                        nodes: a.get("nodes").map(|n| n.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default(),
                        class_id: num(&a, "classId"),
                        ascend_class_id: num(&a, "ascendClassId"),
                        secondary_ascend_class_id: num(&a, "secondaryAscendClassId"),
                        mastery_effects: a.get("masteryEffects").map(|m| parse_mastery_effects(m)).unwrap_or_default(),
                        url: None,
                        sockets: Vec::new(),
                    }),
                    ("URL", "Spec") if !empty => {
                        capture = Capture::Url;
                        text.clear();
                    }
                    ("Socket", "Sockets") => {
                        if let (Some(n), Some(i)) = (num::<u32>(&a, "nodeId"), num::<u32>(&a, "itemId")) {
                            if i != 0 {
                                if let Some(s) = b.specs.last_mut() {
                                    s.sockets.push((n, i));
                                }
                            }
                        }
                    }
                    ("Notes", _) if !empty && parent != "Spec" => {
                        capture = Capture::Notes;
                        text.clear();
                    }
                    ("Config", _) => active_config_id = non_empty(&a, "activeConfigSet"),
                    ("ConfigSet", "Config") => {
                        has_config_sets = true;
                        config_ids.push(a.get("id").cloned().unwrap_or_default());
                        b.config_sets.push(ConfigSet { title: a.get("title").cloned().unwrap_or_default(), inputs: Vec::new() });
                    }
                    ("Input", "Config" | "ConfigSet") => {
                        if parent == "Config" && !has_config_sets && b.config_sets.is_empty() {
                            b.config_sets.push(ConfigSet { title: "Default".into(), inputs: Vec::new() });
                            config_ids.push(String::new());
                        }
                        if let (Some(n), Some(set)) = (a.get("name"), b.config_sets.last_mut()) {
                            let v = ["boolean", "number", "string"].iter().find_map(|k| a.get(*k).cloned()).unwrap_or_default();
                            set.inputs.push((n.clone(), v));
                        }
                    }
                    _ => {}
                }
                if !empty {
                    stack.push(name);
                }
            }
            Event::Text(t) => {
                if capture != Capture::None {
                    text.push_str(&t.xml_content().map_err(|e| e.to_string())?);
                }
            }
            Event::CData(t) => {
                if capture != Capture::None {
                    text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::GeneralRef(rf) => {
                if capture != Capture::None {
                    if let Ok(Some(c)) = rf.resolve_char_ref() {
                        text.push(c);
                    } else {
                        let name = rf.xml_content().map_err(|e| e.to_string())?;
                        let ent = format!("&{name};");
                        text.push_str(&quick_xml::escape::unescape(&ent).map(|c| c.into_owned()).unwrap_or(ent));
                    }
                }
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match (name.as_str(), capture) {
                    ("Item", Capture::Item) => {
                        b.items.push(Item { id: cur_item_id, text: clean_item_text(&text) });
                        capture = Capture::None;
                    }
                    ("Notes", Capture::Notes) => {
                        b.notes = text.trim_matches(['\r', '\n']).replace("\r\n", "\n").trim_end().to_owned();
                        b.notes = dedent(&b.notes);
                        capture = Capture::None;
                    }
                    ("URL", Capture::Url) => {
                        if let Some(s) = b.specs.last_mut() {
                            let u = text.trim();
                            s.url = (!u.is_empty()).then(|| u.to_owned());
                        }
                        capture = Capture::None;
                    }
                    _ => {}
                }
                // Pop up to and including the matching element (tolerates bad nesting).
                if let Some(pos) = stack.iter().rposition(|s| *s == name) {
                    stack.truncate(pos);
                }
            }
            _ => {}
        }
    }
    if !seen_root {
        return Err("Not a Path of Building build (no <PathOfBuilding> / <Build> element)".into());
    }

    if b.item_sets.is_empty() {
        b.item_sets.push(ItemSet { id: "1".into(), title: String::new(), slots: top_slots });
    } else if let Some(set) = b.item_sets.iter_mut().find(|s| s.slots.is_empty()) {
        // Some versions mirror the active set at top level only.
        if !top_slots.is_empty() && Some(&set.id) == active_item_set_id.as_ref() {
            set.slots = top_slots;
        }
    }
    b.active_skill_set = active_skill_set_id.and_then(|id| b.skill_sets.iter().position(|s| s.id == id)).unwrap_or(0);
    b.active_item_set = active_item_set_id.and_then(|id| b.item_sets.iter().position(|s| s.id == id)).unwrap_or(0);
    b.active_spec = active_spec.filter(|i| *i < b.specs.len()).unwrap_or(0);
    b.active_config_set = active_config_id.and_then(|id| config_ids.iter().position(|s| *s == id)).unwrap_or(0);
    Ok(b)
}

/// `{1,2},{3,4}` → [(1,2),(3,4)]
fn parse_mastery_effects(s: &str) -> Vec<(u32, u32)> {
    s.split('}')
        .filter_map(|p| {
            let p = p.trim_start_matches([',', '{', ' ']);
            let (a, b) = p.split_once(',')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .collect()
}

/// Trim each line, drop empty leading/trailing lines.
fn clean_item_text(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.is_empty()).map(|i| i + 1).unwrap_or(start);
    lines[start..end].join("\n")
}

/// Remove common leading indentation (PoB indents the XML text content).
fn dedent(s: &str) -> String {
    let indent = s
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches(['\t', ' ']).len())
        .min()
        .unwrap_or(0);
    s.lines().map(|l| if l.len() >= indent { &l[indent..] } else { l.trim_start() }).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// Stat formatting

/// Format a PlayerStat for display.
pub fn fmt_stat(name: &str, v: f64) -> String {
    let pct = name.ends_with("Chance") || name.contains("Resist") || name.ends_with("Percent") || name.contains("Suppression");
    if name == "CritMultiplier" {
        return format!("{:.0}%", v * 100.0);
    }
    if name.ends_with("Speed") || name == "Speed" {
        return format!("{v:.2}");
    }
    if pct {
        return if v.fract().abs() < 0.05 { format!("{v:.0}%") } else { format!("{v:.1}%") };
    }
    fmt_big(v)
}

/// 1234567 → "1.23M", 12345 → "12,345".
pub fn fmt_big(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 {
        format!("{:.2}B", v / 1e9)
    } else if a >= 1e6 {
        format!("{:.2}M", v / 1e6)
    } else if a >= 1e4 {
        let s = format!("{:.0}", v.round().abs());
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        if v < 0.0 { format!("-{out}") } else { out }
    } else if v.fract().abs() < 0.005 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const FIXTURE: &str = include_str!("fixture_build.xml");
    pub const FIXTURE_OLD: &str = include_str!("fixture_build_old.xml");

    #[test]
    fn parses_full_fixture() {
        let b = parse_build(FIXTURE).unwrap();
        assert_eq!(b.level, Some(94));
        assert_eq!(b.class_name, "Witch");
        assert_eq!(b.ascend_class_name, "Necromancer");
        assert_eq!(b.bandit.as_deref(), Some("None"));
        assert_eq!(b.main_socket_group, Some(2));
        assert_eq!(b.stat("Life"), Some(4521.0));
        assert_eq!(b.stat("CombinedDPS"), Some(2_345_678.5));
        assert!(b.minion_stats.iter().any(|(k, _)| k == "TotalDPS"));
        // skill sets
        assert_eq!(b.skill_sets.len(), 2);
        assert_eq!(b.active_skill_set, 1);
        let set = b.skill_set().unwrap();
        assert_eq!(set.title, "Endgame");
        assert_eq!(set.groups.len(), 3);
        let g = &set.groups[1];
        assert_eq!(g.slot.as_deref(), Some("Body Armour"));
        assert_eq!(g.main_active_skill, Some(1));
        assert_eq!(g.gems.len(), 4);
        assert!(!g.gems[0].support);
        assert_eq!(g.gems[0].name, "Raise Spectre");
        assert_eq!(g.gems[0].level, Some(21));
        assert_eq!(g.gems[0].quality, Some(20));
        assert!(g.gems[1].support);
        assert!(!g.gems[3].enabled);
        assert_eq!(g.gems[2].quality_id.as_deref(), Some("Alternate1"));
        assert!(!set.groups[2].enabled);
        assert_eq!(set.groups[2].label.as_deref(), Some("Swap: Curse & Aura"));
        // items
        assert_eq!(b.items.len(), 4);
        let helm = b.item(1).unwrap();
        assert!(helm.text.starts_with("Rarity: RARE\nGale Crown"), "{}", helm.text);
        assert!(helm.text.contains("{crafted}+30 to maximum Life"));
        assert!(!helm.text.contains("ModRange"));
        assert_eq!(b.item_sets.len(), 2);
        assert_eq!(b.active_item_set, 0);
        assert!(b.item_set().unwrap().slots.contains(&("Helmet".into(), 1)));
        // tree
        assert_eq!(b.specs.len(), 2);
        assert_eq!(b.active_spec, 1);
        let spec = b.spec().unwrap();
        assert_eq!(spec.title, "Maps");
        assert_eq!(spec.tree_version.as_deref(), Some("3_27"));
        assert_eq!(spec.nodes, vec![58556, 1, 2, 3, 4, 5]);
        assert_eq!(spec.class_id, Some(3));
        assert_eq!(spec.ascend_class_id, Some(3));
        assert_eq!(spec.mastery_effects, vec![(100, 200), (101, 201)]);
        assert!(spec.url.as_deref().unwrap().starts_with("https://www.pathofexile.com/passive-skill-tree/"));
        assert_eq!(spec.sockets, vec![(26725, 4)]);
        // notes (entities resolved, indentation removed)
        assert!(b.notes.starts_with("^xE05030Leveling ^7& notes"), "{:?}", b.notes);
        assert!(b.notes.contains("\nUse <Arc> until"));
        // config
        assert_eq!(b.config_sets.len(), 1);
        assert!(b.config_sets[0].inputs.contains(&("enemyIsBoss".into(), "Pinnacle".into())));
        assert!(b.config_sets[0].inputs.contains(&("useFrenzyCharges".into(), "true".into())));
    }

    #[test]
    fn parses_older_format() {
        let b = parse_build(FIXTURE_OLD).unwrap();
        assert_eq!(b.level, Some(80));
        assert_eq!(b.ascend_class_name, "None");
        assert_eq!(b.title(), "Level 80 Marauder");
        assert_eq!(b.skill_sets.len(), 1);
        let set = b.skill_set().unwrap();
        assert_eq!(set.groups.len(), 2);
        assert_eq!(set.groups[0].gems[0].name, "Cleave");
        assert!(set.groups[0].gems[1].support, "older gem ids still detect supports");
        assert_eq!(b.item_sets.len(), 1);
        assert_eq!(b.item_set().unwrap().slots, vec![("Weapon 1".into(), 1)]);
        assert_eq!(b.specs.len(), 1);
        assert_eq!(b.config_sets.len(), 1);
        assert_eq!(b.config_sets[0].inputs.len(), 1);
        assert!(b.notes.is_empty());
    }

    #[test]
    fn tolerates_garbage() {
        assert!(parse_build("hello").is_err());
        let b = parse_build("<PathOfBuilding><Build/></PathOfBuilding>").unwrap();
        assert_eq!(b.level, None);
        assert!(b.skill_set().is_none());
        assert!(b.spec().is_none());
    }

    #[test]
    fn code_roundtrip() {
        let code = encode_code(FIXTURE);
        assert!(!code.contains('+') && !code.contains('/'));
        let xml = decode_code(&code).unwrap();
        assert_eq!(xml, FIXTURE);
        // with whitespace / line breaks and without padding
        let wrapped: String = code.trim_end_matches('=').chars().enumerate().flat_map(|(i, c)| if i % 60 == 59 { vec![c, '\n'] } else { vec![c] }).collect();
        assert_eq!(decode_code(&wrapped).unwrap(), FIXTURE);
        assert_eq!(parse_build(&decode_code(&code).unwrap()).unwrap().level, Some(94));
        assert!(decode_code("!!!").is_err());
        assert!(decode_code("aGVsbG8").is_err(), "valid base64 but not deflate");
        assert_eq!(decode_code("  <PathOfBuilding/>").unwrap(), "<PathOfBuilding/>");
    }

    #[test]
    fn import_urls() {
        assert_eq!(import_url("https://pobb.in/AbC_12-x").as_deref(), Some("https://pobb.in/AbC_12-x/raw"));
        assert_eq!(import_url("pobb.in/abc/").as_deref(), Some("https://pobb.in/abc/raw"));
        assert_eq!(import_url("https://pastebin.com/xYz123").as_deref(), Some("https://pastebin.com/raw/xYz123"));
        assert_eq!(import_url("https://pastebin.com/raw/xYz123").as_deref(), Some("https://pastebin.com/raw/xYz123"));
        assert_eq!(import_url("https://evil.com/pobb.in/x"), None);
        assert_eq!(import_url("eNrtWU1v4zYQ"), None);
    }

    #[test]
    fn note_colors() {
        assert_eq!(strip_colors("^1Red^7 normal ^xFF8800orange^"), "Red normal orange^");
        assert_eq!(strip_colors("^x12ZZ00bad"), "^x12ZZ00bad");
        let spans = color_spans("a^1b^xA0B0C0c^7d");
        assert_eq!(
            spans,
            vec![(None, "a".into()), (Some([255, 0, 0]), "b".into()), (Some([0xa0, 0xb0, 0xc0]), "c".into()), (None, "d".into())]
        );
        assert_eq!(strip_colors("ünïcode ^2ok"), "ünïcode ok");
    }

    #[test]
    fn stat_format() {
        assert_eq!(fmt_stat("Life", 4521.0), "4521");
        assert_eq!(fmt_stat("CombinedDPS", 2_345_678.5), "2.35M");
        assert_eq!(fmt_stat("FireResist", 75.0), "75%");
        assert_eq!(fmt_stat("CritChance", 55.25), "55.2%");
        assert_eq!(fmt_stat("Speed", 3.456), "3.46");
        assert_eq!(fmt_big(123456.0), "123,456");
    }
}
