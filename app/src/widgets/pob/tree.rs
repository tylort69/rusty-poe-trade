//! Passive tree data from GGG's `skilltree-export` (fetched on demand, cached
//! in `data_dir()/pob-tree/<version>.json`) and a simple dot/line renderer.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Normal,
    Notable,
    Keystone,
    Mastery,
    JewelSocket,
    Start,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub ascendancy: Option<String>,
    pub pos: Option<Pos2>,
    pub out: Vec<u32>,
    pub stats: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TreeData {
    pub nodes: HashMap<u32, Node>,
    /// (class name, ascendancy names) indexed by classId.
    pub classes: Vec<(String, Vec<String>)>,
    pub bounds: Option<Rect>,
}

#[derive(Deserialize)]
struct RawTree {
    #[serde(default)]
    classes: Vec<RawClass>,
    #[serde(default)]
    groups: HashMap<String, RawGroup>,
    #[serde(default)]
    nodes: HashMap<String, RawNode>,
    #[serde(default)]
    constants: Option<RawConstants>,
}

#[derive(Deserialize)]
struct RawClass {
    #[serde(default)]
    name: String,
    #[serde(default)]
    ascendancies: Vec<RawAsc>,
}

#[derive(Deserialize)]
struct RawAsc {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawGroup {
    x: f32,
    y: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawNode {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    is_keystone: bool,
    #[serde(default)]
    is_notable: bool,
    #[serde(default)]
    is_mastery: bool,
    #[serde(default)]
    is_jewel_socket: bool,
    #[serde(default)]
    class_start_index: Option<u32>,
    #[serde(default)]
    ascendancy_name: Option<String>,
    #[serde(default)]
    group: Option<serde_json::Value>,
    #[serde(default)]
    orbit: Option<usize>,
    #[serde(default)]
    orbit_index: Option<usize>,
    #[serde(default)]
    out: Vec<serde_json::Value>,
    #[serde(default)]
    stats: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConstants {
    #[serde(default)]
    skills_per_orbit: Vec<usize>,
    #[serde(default)]
    orbit_radii: Vec<f32>,
}

fn id_of(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().map(|x| x as u32),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Angle (degrees) of a node on an orbit; 16/40-node orbits are not uniform (PoB's tables).
fn orbit_angle(skills: usize, index: usize) -> f32 {
    const A16: [f32; 16] = [0., 30., 45., 60., 90., 120., 135., 150., 180., 210., 225., 240., 270., 300., 315., 330.];
    const A40: [f32; 40] = [
        0., 10., 20., 30., 40., 45., 50., 60., 70., 80., 90., 100., 110., 120., 130., 135., 140., 150., 160., 170., 180., 190., 200., 210.,
        220., 225., 230., 240., 250., 260., 270., 280., 290., 300., 310., 315., 320., 330., 340., 350.,
    ];
    match skills {
        16 => A16[index % 16],
        40 => A40[index % 40],
        0 => 0.0,
        n => 360.0 * index as f32 / n as f32,
    }
}

/// Parse GGG's `data.json`.
pub fn parse_tree(json: &str) -> Result<TreeData, String> {
    let raw: RawTree = serde_json::from_str(json).map_err(|e| format!("tree data: {e}"))?;
    let (spo, radii) = raw.constants.map(|c| (c.skills_per_orbit, c.orbit_radii)).unwrap_or_default();
    let mut nodes = HashMap::new();
    let mut bounds: Option<Rect> = None;
    for (k, n) in raw.nodes {
        let Ok(id) = k.parse::<u32>() else { continue };
        let kind = if n.class_start_index.is_some() {
            NodeKind::Start
        } else if n.is_keystone {
            NodeKind::Keystone
        } else if n.is_notable {
            NodeKind::Notable
        } else if n.is_mastery {
            NodeKind::Mastery
        } else if n.is_jewel_socket {
            NodeKind::JewelSocket
        } else {
            NodeKind::Normal
        };
        let pos = (|| {
            let g = raw.groups.get(&n.group.as_ref().and_then(id_of)?.to_string())?;
            let orbit = n.orbit.unwrap_or(0);
            let r = *radii.get(orbit).unwrap_or(&0.0);
            let a = orbit_angle(*spo.get(orbit).unwrap_or(&1), n.orbit_index.unwrap_or(0)).to_radians();
            Some(Pos2::new(g.x + r * a.sin(), g.y - r * a.cos()))
        })();
        if let (Some(p), None) = (pos, &n.ascendancy_name) {
            let r = Rect::from_center_size(p, Vec2::ZERO);
            bounds = Some(bounds.map_or(r, |b| b.union(r)));
        }
        nodes.insert(
            id,
            Node {
                name: n.name.unwrap_or_default(),
                kind,
                ascendancy: n.ascendancy_name,
                pos,
                out: n.out.iter().filter_map(id_of).collect(),
                stats: n.stats,
            },
        );
    }
    let classes = raw.classes.into_iter().map(|c| (c.name, c.ascendancies.into_iter().map(|a| a.name).collect())).collect();
    Ok(TreeData { nodes, classes, bounds })
}

/// `3_27` → `3.27.0` (tag names in grindinggear/skilltree-export).
pub fn export_tag(tree_version: &str) -> Option<String> {
    let v = tree_version.split('_').take(2).collect::<Vec<_>>();
    match v.as_slice() {
        [a, b] if a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) && *a == "3" => Some(format!("{a}.{b}.0")),
        _ => None,
    }
}

pub fn export_url(tag: &str) -> String {
    format!("https://raw.githubusercontent.com/grindinggear/skilltree-export/{tag}/data.json")
}

pub fn cache_path(tag: &str) -> PathBuf {
    crate::store::data_dir().join("pob-tree").join(format!("{tag}.json"))
}

/// Load cached tree data (blocking file read + parse).
pub fn load_cached(tag: &str) -> Option<Result<TreeData, String>> {
    let s = std::fs::read_to_string(cache_path(tag)).ok()?;
    Some(parse_tree(&s))
}

/// Download, cache and parse the tree data for `tag`.
pub async fn fetch(tag: String) -> Result<TreeData, String> {
    let client = reqwest::Client::builder().user_agent("rusty-poe-trade").build().map_err(|e| e.to_string())?;
    let resp = client.get(export_url(&tag)).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("tree data for {tag}: HTTP {}", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let data = parse_tree(&body)?;
    let path = cache_path(&tag);
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&path, body);
    Ok(data)
}

/// Fallback ascendancy names (PoE 1, 3.x) by classId / ascendClassId.
pub fn ascendancy_fallback(class_id: u32, asc: u32) -> Option<&'static str> {
    const T: [[&str; 3]; 7] = [
        ["Ascendant", "", ""],
        ["Juggernaut", "Berserker", "Chieftain"],
        ["Raider", "Deadeye", "Pathfinder"],
        ["Occultist", "Elementalist", "Necromancer"],
        ["Slayer", "Gladiator", "Champion"],
        ["Inquisitor", "Hierophant", "Guardian"],
        ["Assassin", "Trickster", "Saboteur"],
    ];
    let n = *T.get(class_id as usize)?.get((asc as usize).checked_sub(1)?)?;
    (!n.is_empty()).then_some(n)
}

pub fn class_fallback(class_id: u32) -> Option<&'static str> {
    ["Scion", "Marauder", "Ranger", "Witch", "Duelist", "Templar", "Shadow"].get(class_id as usize).copied()
}

/// Zoom/pan state of the tree view.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub zoom: f32,
    pub offset: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 1.0, offset: Vec2::ZERO }
    }
}

/// Draw allocated nodes (and their links) of the main tree, zoomable with
/// the scroll wheel and draggable.
pub fn draw(ui: &mut Ui, data: &TreeData, allocated: &[u32], view: &mut View, size: Vec2, accent: Color32, dim: Color32) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, dim.gamma_multiply(0.08));
    let Some(bounds) = data.bounds.filter(|b| b.width() > 0.0 && b.height() > 0.0) else { return };
    if resp.dragged() {
        view.offset += resp.drag_delta();
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let old = view.zoom;
            view.zoom = (view.zoom * (1.0 + scroll * 0.002)).clamp(0.5, 20.0);
            if let Some(p) = resp.hover_pos() {
                let rel = p - rect.center() - view.offset;
                view.offset -= rel * (view.zoom / old - 1.0);
            }
        }
    }
    if resp.double_clicked() {
        *view = View::default();
    }
    let base = (rect.width() / bounds.width()).min(rect.height() / bounds.height());
    let scale = base * view.zoom;
    let to_screen = |p: Pos2| rect.center() + view.offset + (p - bounds.center()) * scale;
    let alloc: std::collections::HashSet<u32> = allocated.iter().copied().collect();
    let main = |id: &u32| data.nodes.get(id).filter(|n| n.ascendancy.is_none() && n.pos.is_some());

    // faint unallocated skeleton when zoomed in a bit
    for n in data.nodes.values().filter(|n| n.ascendancy.is_none() && n.kind != NodeKind::Mastery) {
        if let Some(p) = n.pos {
            let s = to_screen(p);
            if rect.contains(s) {
                painter.circle_filled(s, (scale * 12.0).clamp(0.6, 3.0), dim.gamma_multiply(0.35));
            }
        }
    }
    for id in &alloc {
        let Some(n) = main(id) else { continue };
        for o in &n.out {
            if alloc.contains(o) {
                if let (Some(a), Some(b)) = (n.pos, main(o).and_then(|m| m.pos)) {
                    painter.line_segment([to_screen(a), to_screen(b)], Stroke::new((scale * 8.0).clamp(1.0, 3.0), accent.gamma_multiply(0.7)));
                }
            }
        }
    }
    let mut hovered: Option<&Node> = None;
    let hover = resp.hover_pos();
    for id in &alloc {
        let Some(n) = main(id) else { continue };
        let s = to_screen(n.pos.unwrap_or_default());
        let r = match n.kind {
            NodeKind::Keystone => 6.0,
            NodeKind::Notable | NodeKind::JewelSocket => 4.5,
            _ => 2.5,
        } * view.zoom.sqrt();
        painter.circle_filled(s, r, accent);
        if hover.is_some_and(|h| h.distance(s) <= r + 3.0) {
            hovered = Some(n);
        }
    }
    if let Some(n) = hovered {
        let mut txt = n.name.clone();
        for s in &n.stats {
            txt.push('\n');
            txt.push_str(s);
        }
        resp.on_hover_text(txt);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const TREE_JSON: &str = r#"{
        "tree": "Default",
        "classes": [{"name": "Scion", "ascendancies": [{"id": "Ascendant", "name": "Ascendant"}]},
                    {"name": "Marauder", "ascendancies": []}, {"name": "Ranger", "ascendancies": []},
                    {"name": "Witch", "ascendancies": [{"name": "Occultist"}, {"name": "Elementalist"}, {"name": "Necromancer"}]}],
        "groups": {"1": {"x": 0, "y": 0, "orbits": [0, 1], "nodes": ["58556", "1"]}, "2": {"x": 1000, "y": 500, "orbits": [2], "nodes": ["2", "3"]}},
        "nodes": {
            "root": {"group": 0, "orbit": 0, "orbitIndex": 0, "out": [], "in": []},
            "58556": {"skill": 58556, "name": "Divine Shield", "isKeystone": true, "stats": ["Cannot Recover Energy Shield"], "group": 1, "orbit": 0, "orbitIndex": 0, "out": ["1"], "in": []},
            "1": {"skill": 1, "name": "Life", "stats": ["+10 to maximum Life"], "group": 1, "orbit": 1, "orbitIndex": 3, "out": ["2"], "in": ["58556"]},
            "2": {"skill": 2, "name": "Heart of Flame", "isNotable": true, "group": 2, "orbit": 2, "orbitIndex": 4, "out": [], "in": ["1"]},
            "3": {"skill": 3, "name": "Mastery", "isMastery": true, "group": 2, "orbit": 2, "orbitIndex": 0, "out": [], "in": []},
            "4": {"skill": 4, "name": "Mistress of Sacrifice", "isNotable": true, "ascendancyName": "Necromancer", "group": 9, "orbit": 0, "orbitIndex": 0, "out": [], "in": []}
        },
        "constants": {"skillsPerOrbit": [1, 6, 16, 16, 40, 72, 72], "orbitRadii": [0, 82, 162, 335, 493, 662, 846]}
    }"#;

    #[test]
    fn parses_tree_json() {
        let t = parse_tree(TREE_JSON).unwrap();
        assert_eq!(t.nodes.len(), 5);
        assert_eq!(t.nodes[&58556].kind, NodeKind::Keystone);
        assert_eq!(t.nodes[&2].kind, NodeKind::Notable);
        assert_eq!(t.nodes[&4].ascendancy.as_deref(), Some("Necromancer"));
        assert_eq!(t.nodes[&58556].out, vec![1]);
        // orbit 1 (6 skills), index 3 → 180°: straight down
        let p = t.nodes[&1].pos.unwrap();
        assert!((p.x).abs() < 0.01 && (p.y - 82.0).abs() < 0.01, "{p:?}");
        // orbit 2 (16 skills), index 4 → 90°
        let p = t.nodes[&2].pos.unwrap();
        assert!((p.x - 1162.0).abs() < 0.01 && (p.y - 500.0).abs() < 0.01, "{p:?}");
        assert_eq!(t.classes[3].1[2], "Necromancer");
        assert!(t.bounds.unwrap().width() > 1000.0);
        assert!(parse_tree("{").is_err());
    }

    #[test]
    fn versions_and_fallbacks() {
        assert_eq!(export_tag("3_27").as_deref(), Some("3.27.0"));
        assert_eq!(export_tag("3_25_alternate").as_deref(), Some("3.25.0"));
        assert_eq!(export_tag("0_1"), None, "PoE 2 trees are not in the export repo");
        assert_eq!(export_tag("garbage"), None);
        assert_eq!(export_url("3.27.0"), "https://raw.githubusercontent.com/grindinggear/skilltree-export/3.27.0/data.json");
        assert_eq!(ascendancy_fallback(3, 3), Some("Necromancer"));
        assert_eq!(ascendancy_fallback(0, 2), None);
        assert_eq!(ascendancy_fallback(3, 0), None);
        assert_eq!(class_fallback(6), Some("Shadow"));
    }
}
