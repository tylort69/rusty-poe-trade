//! Normalized stash types shared by both backends, plus the wire formats.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A stash tab without its items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StashTabInfo {
    /// GGG stash id (stable across renames).
    pub id: String,
    /// Parent tab id for folder children / special sub-tabs (OAuth only).
    pub parent: Option<String>,
    /// Position in the tab bar (legacy `tabIndex`, needed to fetch by session cookie).
    pub index: Option<u32>,
    pub name: String,
    /// `NormalStash`, `PremiumStash`, `QuadStash`, `CurrencyStash`, `MapStash`, `Folder`, ...
    pub kind: String,
    pub colour: Option<[u8; 3]>,
    pub hidden: bool,
    pub children: Vec<StashTabInfo>,
}

impl StashTabInfo {
    pub fn is_folder(&self) -> bool {
        self.kind == "Folder"
    }

    /// Flattened list of every tab that can hold items (folders expanded).
    pub fn flatten(tabs: &[StashTabInfo]) -> Vec<&StashTabInfo> {
        let mut out = Vec::new();
        for t in tabs {
            if !t.is_folder() {
                out.push(t);
            }
            out.extend(Self::flatten(&t.children));
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StashTab {
    pub info: StashTabInfo,
    pub items: Vec<Item>,
}

/// GGG item JSON (same shape for both APIs). Common fields are typed; the
/// rest is kept in `extra` so nothing is lost for later features.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub type_line: String,
    #[serde(default)]
    pub base_type: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub ilvl: u32,
    #[serde(default)]
    pub identified: bool,
    #[serde(default)]
    pub corrupted: bool,
    /// 0 normal, 1 magic, 2 rare, 3 unique, 4 gem, 5 currency, 6 div card, ...
    #[serde(default)]
    pub frame_type: u8,
    #[serde(default)]
    pub stack_size: Option<u32>,
    #[serde(default)]
    pub max_stack_size: Option<u32>,
    #[serde(default)]
    pub x: Option<u32>,
    #[serde(default)]
    pub y: Option<u32>,
    #[serde(default = "one")]
    pub w: u32,
    #[serde(default = "one")]
    pub h: u32,
    /// Price note, e.g. `~price 5 chaos`.
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub explicit_mods: Vec<String>,
    #[serde(default)]
    pub implicit_mods: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn one() -> u32 {
    1
}

impl Item {
    /// What poe.ninja and the trade site would call it.
    pub fn display_name(&self) -> String {
        if self.name.is_empty() { self.type_line.clone() } else { format!("{} {}", self.name, self.type_line) }
    }

    pub fn quantity(&self) -> u32 {
        self.stack_size.unwrap_or(1)
    }

    pub fn is_unique(&self) -> bool {
        self.frame_type == 3
    }
}

// ---------------------------------------------------------------------------
// Legacy `character-window/get-stash-items`

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyStashResponse {
    #[serde(default)]
    #[allow(dead_code)]
    pub num_tabs: Option<u32>,
    #[serde(default)]
    pub tabs: Vec<LegacyTab>,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LegacyTab {
    pub n: String,
    pub i: u32,
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub colour: Option<LegacyColour>,
    #[serde(default)]
    pub children: Vec<LegacyTab>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LegacyColour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<LegacyTab> for StashTabInfo {
    fn from(t: LegacyTab) -> Self {
        Self {
            id: t.id,
            parent: None,
            index: Some(t.i),
            name: t.n,
            kind: t.kind,
            colour: t.colour.map(|c| [c.r, c.g, c.b]),
            hidden: t.hidden,
            children: t.children.into_iter().map(Into::into).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// OAuth `api.pathofexile.com/stash`

#[derive(Debug, Deserialize)]
pub(crate) struct ApiStashList {
    #[serde(default)]
    pub stashes: Vec<ApiStash>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiStashOne {
    pub stash: ApiStash,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiStash {
    pub id: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub metadata: ApiStashMeta,
    #[serde(default)]
    pub children: Vec<ApiStash>,
    #[serde(default)]
    pub items: Option<Vec<Item>>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ApiStashMeta {
    #[serde(default)]
    pub colour: Option<String>,
}

impl ApiStash {
    pub fn into_tab(mut self) -> StashTab {
        let items = self.items.take().unwrap_or_default();
        StashTab { info: self.into(), items }
    }
}

impl From<ApiStash> for StashTabInfo {
    fn from(s: ApiStash) -> Self {
        let colour = s.metadata.colour.as_deref().and_then(|h| {
            let h = h.trim_start_matches('#');
            let v = u32::from_str_radix(h, 16).ok()?;
            (h.len() == 6).then(|| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
        });
        Self {
            id: s.id,
            parent: s.parent,
            index: s.index,
            name: s.name,
            kind: s.kind,
            colour,
            hidden: false,
            children: s.children.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_and_api_normalize_alike() {
        let l: LegacyStashResponse = serde_json::from_str(include_str!("../tests/fixtures/legacy.json")).unwrap();
        assert_eq!(l.num_tabs, Some(3));
        let tabs: Vec<StashTabInfo> = l.tabs.into_iter().map(Into::into).collect();
        assert_eq!(tabs[1].index, Some(1));
        assert_eq!(tabs[0].colour, Some([124, 84, 54]));
        assert_eq!(l.items[0].quantity(), 12);
        assert_eq!(l.items[1].display_name(), "Tabula Rasa Simple Robe");
        assert!(l.items[1].is_unique());
        assert!(l.items[1].extra.contains_key("sockets"), "unknown fields are kept");

        let a: ApiStashList = serde_json::from_str(include_str!("../tests/fixtures/api_list.json")).unwrap();
        let tabs: Vec<StashTabInfo> = a.stashes.into_iter().map(Into::into).collect();
        assert_eq!(tabs[0].colour, Some([124, 84, 54]));
        let flat = StashTabInfo::flatten(&tabs);
        assert_eq!(flat.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a1", "b2"]);
        assert_eq!(flat[1].parent.as_deref(), Some("f1"));

        let one: ApiStashOne = serde_json::from_str(include_str!("../tests/fixtures/api_one.json")).unwrap();
        let tab = one.stash.into_tab();
        assert_eq!(tab.items[0].quantity(), 3);
        assert_eq!(tab.info.name, "dump");
    }
}
