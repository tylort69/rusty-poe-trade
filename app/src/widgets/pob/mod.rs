//! Path of Building build viewer.
//!
//! Sources: the local PoB `Builds` folder (auto-detected, overridable) or a
//! pasted export code / pobb.in / pastebin link. Tabs: Overview, Skills,
//! Items (with a mod-by-mod comparison against the last checked item), Tree
//! and Notes. Network is only used on explicit user action (link import,
//! tree data download).

pub mod items;
pub mod model;
pub mod paths;
pub mod tree;
mod ui;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{WCtx, chrome};
use crate::config::Anchor;
use crate::host::HostEvent;
use crate::services::Task;

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-pob"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PobWidget {
    pub anchor: Anchor,
    /// User-chosen `Builds` folder; `None` = first auto-detected one.
    pub builds_dir: Option<String>,
    /// Absolute path of the last opened build file.
    pub last_build: Option<String>,
    /// Show the source sidebar.
    pub sidebar: bool,
    /// Render PoB color codes in notes.
    pub note_colors: bool,
}

impl Default for PobWidget {
    fn default() -> Self {
        Self { anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 }, builds_dir: None, last_build: None, sidebar: true, note_colors: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Overview,
    Skills,
    Items,
    Tree,
    Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    Folder,
    Import,
}

/// A loaded build plus pre-parsed item texts.
pub struct Loaded {
    pub build: model::Build,
    /// Where it came from (file path or "Import code").
    pub origin: String,
    pub items: HashMap<u32, items::ItemText>,
}

impl Loaded {
    pub fn new(build: model::Build, origin: String) -> Self {
        let items = build.items.iter().map(|i| (i.id, items::parse_pob_item(&i.text))).collect();
        Self { build, origin, items }
    }
}

/// The last item the user price-/item-checked in the overlay.
#[derive(Debug, Clone)]
pub struct LastChecked {
    pub item: items::ItemText,
    /// PoB slots this item can go in.
    pub slots: Vec<String>,
}

type TreeLoad = Task<Result<Option<tree::TreeData>, String>>;

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct PobState {
    pub tab: Tab,
    pub source: Source,
    /// Detected `Builds` folders (lazily computed).
    candidates: Option<Vec<PathBuf>>,
    dir_input: Option<String>,
    /// (folder, files) of the current listing.
    listing: Option<(PathBuf, Vec<paths::BuildFile>)>,
    pub import_input: String,
    fetch: Option<Task<Result<String, String>>>,
    pub loaded: Option<Loaded>,
    pub error: Option<String>,
    tried_last: bool,
    pub selected_slot: Option<String>,
    /// Slot being compared with the last checked item.
    pub compare_slot: Option<String>,
    pub last_checked: Option<LastChecked>,
    /// Tree data by export tag; `None` value = no cache, not yet downloaded.
    pub tree_data: HashMap<String, Option<tree::TreeData>>,
    tree_task: Option<(String, TreeLoad)>,
    tree_error: Option<String>,
    tree_view: tree::View,
    /// Tests: never touch the filesystem cache or the network.
    offline: bool,
}

impl PobState {
    pub fn load_xml(&mut self, xml: &str, origin: String) -> bool {
        match model::parse_build(xml) {
            Ok(b) => {
                self.loaded = Some(Loaded::new(b, origin));
                self.error = None;
                self.selected_slot = None;
                self.compare_slot = None;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    fn load_file(&mut self, path: &std::path::Path) -> bool {
        match std::fs::read_to_string(path) {
            Ok(s) => self.load_xml(&s, path.to_string_lossy().into_owned()),
            Err(e) => {
                self.error = Some(format!("Cannot read {}: {e}", path.display()));
                false
            }
        }
    }

    /// Record the clipboard text of a price/item check.
    pub fn set_last_checked(&mut self, clipboard: &str) {
        let item = items::parse_game_item(clipboard);
        if item.mods.is_empty() && item.name.is_empty() {
            return;
        }
        let mut slots = item.item_class.as_deref().map(items::slots_for_item_class).unwrap_or_default();
        if slots.is_empty() {
            // Non-English client: fall back to the parser's category.
            if let Ok(p) = rpt_parser::parse_clipboard(clipboard) {
                slots = p.category.map(slots_for_category).unwrap_or_default();
            }
        }
        self.last_checked = Some(LastChecked { item, slots });
    }
}

fn slots_for_category(c: rpt_parser::ItemCategory) -> Vec<String> {
    use rpt_parser::ItemCategory as C;
    let class = match c {
        C::Helmet => "Helmets",
        C::BodyArmour => "Body Armours",
        C::Gloves => "Gloves",
        C::Boots => "Boots",
        C::Belt => "Belts",
        C::Amulet => "Amulets",
        C::Ring => "Rings",
        C::Shield => "Shields",
        C::Quiver => "Quivers",
        C::Flask => "Life Flasks",
        C::Bow => "Bows",
        C::Staff => "Staves",
        C::Warstaff => "Warstaves",
        C::TwoHandedAxe => "Two Hand Axes",
        C::TwoHandedMace => "Two Hand Maces",
        C::TwoHandedSword => "Two Hand Swords",
        C::FishingRod => "Fishing Rods",
        C::Claw | C::Dagger | C::RuneDagger | C::OneHandedAxe | C::OneHandedMace | C::OneHandedSword | C::Sceptre | C::Wand => "Wands",
        _ => "",
    };
    items::slots_for_item_class(class)
}

/// Host event hook: remember the item text of price / item checks.
pub fn on_item_text(st: &mut PobState, ev: &HostEvent) {
    if let HostEvent::ItemText { target, clipboard, .. } = ev {
        if matches!(target.as_str(), "price-check" | "item-check") {
            st.set_last_checked(clipboard);
        }
    }
}

fn widget_cfg<'a>(w: &'a mut WCtx<'_>, wm_id: u32) -> Option<&'a mut PobWidget> {
    w.cfg.widgets.iter_mut().find(|x| x.wm_id == wm_id).and_then(|x| match &mut x.kind {
        crate::config::WidgetKind::Pob(p) => Some(p),
        _ => None,
    })
}

/// Download a pasted pobb.in / pastebin link and decode it.
async fn fetch_code(url: String) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("rusty-poe-trade")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| format!("{url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    model::decode_code(&body)
}

impl PobState {
    /// Import button: decode a code, or start fetching a link.
    fn start_import(&mut self, w: &WCtx<'_>) {
        let input = self.import_input.trim().to_owned();
        if input.is_empty() {
            return;
        }
        if let Some(url) = model::import_url(&input) {
            if self.offline {
                self.error = Some("offline".into());
                return;
            }
            self.error = None;
            self.fetch = Some(w.services.spawn(fetch_code(url)));
        } else {
            match model::decode_code(&input) {
                Ok(xml) => {
                    self.load_xml(&xml, "Import code".into());
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    fn poll_tasks(&mut self) {
        if let Some(t) = &mut self.fetch {
            if let Some(r) = t.poll_take() {
                self.fetch = None;
                match r {
                    Ok(xml) => {
                        let origin = self.import_input.trim().to_owned();
                        self.load_xml(&xml, origin);
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        if let Some((tag, t)) = &mut self.tree_task {
            if let Some(r) = t.poll_take() {
                let tag = tag.clone();
                self.tree_task = None;
                match r {
                    Ok(d) => {
                        self.tree_data.insert(tag, d);
                        self.tree_error = None;
                    }
                    Err(e) => {
                        self.tree_data.entry(tag).or_insert(None);
                        self.tree_error = Some(e);
                    }
                }
            }
        }
    }

    /// Load cached tree data for `tag` in the background (no network).
    fn ensure_tree_cache(&mut self, w: &WCtx<'_>, tag: &str) {
        if self.offline || self.tree_data.contains_key(tag) || self.tree_task.is_some() {
            return;
        }
        let t = tag.to_owned();
        let task = w.services.spawn(async move { tree::load_cached(&t).transpose() });
        self.tree_task = Some((tag.to_owned(), task));
    }

    /// Download tree data (explicit user action).
    fn download_tree(&mut self, w: &WCtx<'_>, tag: &str) {
        if self.offline || self.tree_task.is_some() {
            return;
        }
        self.tree_error = None;
        let task = w.services.spawn(tree::fetch(tag.to_owned()).map_ok_some());
        self.tree_task = Some((tag.to_owned(), task));
    }
}

/// Small helper to turn `Result<T, E>` futures into `Result<Option<T>, E>`.
trait MapOkSome<T> {
    fn map_ok_some(self) -> impl std::future::Future<Output = Result<Option<T>, String>> + Send;
}

impl<T: Send, F: std::future::Future<Output = Result<T, String>> + Send> MapOkSome<T> for F {
    fn map_ok_some(self) -> impl std::future::Future<Output = Result<Option<T>, String>> + Send {
        async move { self.await.map(Some) }
    }
}

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, st: &mut PobState, wm_id: u32) {
    st.poll_tasks();
    // Reopen the last build once.
    if !st.tried_last && st.loaded.is_none() {
        st.tried_last = true;
        if let Some(p) = widget_cfg(w, wm_id).and_then(|c| c.last_build.clone()) {
            let p = PathBuf::from(p);
            if p.is_file() {
                st.load_file(&p);
                st.error = None;
            }
        }
    }
    let screen = w.screen;
    let size = egui::vec2((screen.width() * 0.9).min(1000.0), (screen.height() * 0.85).min(720.0));
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, w| {
        ui.set_width(size.x);
        ui.set_height(size.y);
        ui::body(ui, w, st, wm_id, size);
    });
    chrome::apply(w, wm_id, &r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::host::HostQueue;

    struct Harness {
        ctx: egui::Context,
        services: crate::services::Services,
    }

    impl Harness {
        /// One context + one `Services` for all renders (its league refresh is the
        /// only background request, same as the settings render test).
        fn new() -> Self {
            rpt_data::init("en").unwrap();
            let ctx = egui::Context::default();
            rpt_m3::install_fonts(&ctx, vec![]);
            rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
            let services = crate::services::Services::new(&Config::default(), ctx.clone());
            Self { ctx, services }
        }

        /// Render `frames` frames; returns the number of shapes of the last one.
        fn render(&self, st: &mut PobState, cfg: &mut Config, frames: usize) -> usize {
            let data = rpt_data::data();
            let i18n = crate::i18n::I18n::new(&data);
            let mut host = HostQueue::default();
            let id = crate::wm::id_of(cfg, "rpt-pob").unwrap();
            crate::wm::show(cfg, id);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
            let mut shapes = 0;
            for _ in 0..frames {
                let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut out = self.ctx.run_ui(input, |ui| {
                    let mut w = WCtx {
                        cfg: &mut *cfg,
                        st: crate::wm::OverlayState { active: true, ..Default::default() },
                        i18n: &i18n,
                        host: &mut host,
                        services: &self.services,
                        screen,
                        save_requested: false,
                    };
                    show(ui.ctx(), &mut w, st, id);
                });
                out.textures_delta.clear();
                shapes = out.shapes.len();
            }
            shapes
        }
    }

    const GAME_HELM: &str = "Item Class: Helmets\nRarity: Rare\nDoom Visor\nHubris Circlet\n--------\nItem Level: 84\n--------\n+90 to maximum Life\n+20% to Lightning Resistance (crafted)\n";

    #[test]
    fn renders_every_tab_headless() {
        let h = Harness::new();
        let tabs = [Tab::Overview, Tab::Skills, Tab::Items, Tab::Tree, Tab::Notes];
        // Nothing loaded (both sources).
        for source in [Source::Folder, Source::Import] {
            for tab in tabs {
                let mut cfg = Config::default();
                let mut st = PobState { offline: true, tab, source, candidates: Some(vec![]), ..Default::default() };
                let empty = h.render(&mut st, &mut cfg, 2);
                assert!(st.loaded.is_none());
                assert!(empty > 0);
            }
        }
        // Fixture loaded, with and without tree data, with a checked item to compare.
        for with_tree in [false, true] {
            for tab in tabs {
                let mut cfg = Config::default();
                let mut st = PobState { offline: true, tab, candidates: Some(vec![]), ..Default::default() };
                assert!(st.load_xml(model::tests::FIXTURE, "fixture".into()));
                if with_tree {
                    st.tree_data.insert("3.27.0".into(), Some(tree::parse_tree(tree::tests::TREE_JSON).unwrap()));
                    st.selected_slot = Some("Body Armour".into());
                } else {
                    st.tree_data.insert("3.27.0".into(), None);
                    st.set_last_checked(GAME_HELM);
                    st.compare_slot = Some("Helmet".into());
                }
                let n = h.render(&mut st, &mut cfg, 2);
                assert!(n > 20, "{tab:?} drew only {n} shapes");
                assert!(st.loaded.is_some(), "{tab:?}");
                assert_eq!(st.error, None);
            }
        }
        // Older format + note colors off.
        let mut cfg = Config::default();
        if let Some(crate::config::WidgetKind::Pob(p)) = cfg.widgets.iter_mut().map(|w| &mut w.kind).find(|k| matches!(k, crate::config::WidgetKind::Pob(_))) {
            p.note_colors = false;
            p.sidebar = false;
        }
        for tab in tabs {
            let mut st = PobState { offline: true, tab, candidates: Some(vec![]), ..Default::default() };
            assert!(st.load_xml(model::tests::FIXTURE_OLD, "old".into()));
            h.render(&mut st, &mut cfg, 1);
        }
    }

    #[test]
    fn last_checked_from_host_event() {
        let mut st = PobState::default();
        let ev = HostEvent::ItemText { target: "price-check".into(), clipboard: GAME_HELM.into(), position: Default::default(), focus_overlay: false };
        on_item_text(&mut st, &ev);
        let lc = st.last_checked.as_ref().unwrap();
        assert_eq!(lc.slots, vec!["Helmet"]);
        assert_eq!(lc.item.mods.len(), 2);
        let other = HostEvent::ItemText { target: "open-wiki".into(), clipboard: "x".into(), position: Default::default(), focus_overlay: false };
        st.last_checked = None;
        on_item_text(&mut st, &other);
        assert!(st.last_checked.is_none());
    }

    #[test]
    fn widget_config_roundtrip() {
        let w = PobWidget { builds_dir: Some("/x/Builds".into()), last_build: Some("/x/Builds/a.xml".into()), ..Default::default() };
        let j = serde_json::to_string(&w).unwrap();
        assert!(j.contains("\"buildsDir\"") && j.contains("\"lastBuild\""));
        let back: PobWidget = serde_json::from_str(&j).unwrap();
        assert_eq!(back, w);
        // Old scaffold config (anchor only) still loads.
        let old: PobWidget = serde_json::from_str(r#"{"anchor":{"pos":"cc","x":50,"y":50}}"#).unwrap();
        assert!(old.sidebar && old.builds_dir.is_none());
    }
}
