//! App configuration. Port of upstream `renderer/src/web/Config.ts`.
//!
//! The JSON shape is kept identical to Awakened PoE Trade's `config.json`
//! (configVersion 18), so an existing APT config can be imported as-is.
//! Rusty-only settings live under the extra `rpt` key, which APT ignores.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const CONFIG_VERSION: u32 = 18;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    pub text: String,
    pub hotkey: Option<String>,
    pub send: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub config_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub league_id: Option<String>,
    pub overlay_key: String,
    pub overlay_background: String,
    pub overlay_background_close: bool,
    pub restore_clipboard: bool,
    pub commands: Vec<Command>,
    pub client_log: Option<String>,
    pub game_config: Option<String>,
    pub window_title: String,
    pub log_keys: bool,
    pub account_name: String,
    pub stash_scroll: bool,
    /// "en" | "ru" | "cmn-Hant" | "ko"
    pub language: String,
    /// "pc-ggg" | "pc-garena"
    pub realm: String,
    pub use_intl_site: bool,
    pub widgets: Vec<Widget>,
    pub font_size: f32,
    pub show_attach_notification: bool,
    #[serde(default)]
    pub rpt: RptSettings,
    /// Unknown keys from APT configs are preserved on save.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Settings that only exist in Rusty POE Trade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RptSettings {
    /// "system" or "#rrggbb"
    pub seed_color: String,
    /// "system" | "light" | "dark"
    pub brightness: String,
    /// rpt_m3::SchemeVariant label in kebab case
    pub scheme_variant: String,
    pub contrast: f64,
    pub check_for_updates: bool,
    pub account: AccountSettings,
}

/// Path of Exile sign-in. Secrets are never stored here, only in the OS keychain.
/// The account name lives in the existing APT `accountName` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountSettings {
    pub method: rpt_stash::AuthMethod,
    /// Keep the session cookie in the keychain between restarts.
    pub remember: bool,
    /// GGG-issued OAuth client ID. Empty → OAuth sign-in disabled.
    pub oauth_client_id: String,
    pub oauth_port: u16,
}

impl Default for AccountSettings {
    fn default() -> Self {
        Self {
            method: rpt_stash::AuthMethod::SessionCookie,
            remember: true,
            oauth_client_id: String::new(),
            oauth_port: rpt_stash::oauth::DEFAULT_PORT,
        }
    }
}

impl Default for RptSettings {
    fn default() -> Self {
        Self {
            seed_color: "system".into(),
            brightness: "dark".into(),
            scheme_variant: "tonal-spot".into(),
            contrast: 0.0,
            check_for_updates: true,
            account: AccountSettings::default(),
        }
    }
}

impl RptSettings {
    pub fn theme_settings(&self, font_size: f32) -> rpt_m3::ThemeSettings {
        use rpt_m3::{Brightness, SchemeVariant, SeedSource};
        let seed = parse_hex(&self.seed_color).map(SeedSource::Custom).unwrap_or(SeedSource::System);
        let brightness = match self.brightness.as_str() {
            "light" => Brightness::Light,
            "system" => Brightness::System,
            _ => Brightness::Dark,
        };
        let variant = SchemeVariant::ALL
            .iter()
            .copied()
            .find(|v| v.label().to_lowercase().replace(' ', "-") == self.scheme_variant)
            .unwrap_or_default();
        rpt_m3::ThemeSettings { seed, brightness, variant, contrast: self.contrast, text_scale: font_size / 16.0 }
    }
}

pub fn parse_hex(s: &str) -> Option<egui::Color32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

// ---------------------------------------------------------------------------
// Widgets (upstream overlay/widgets.ts + each widget's own interface)

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    /// two letters: vertical (t/c/b) + horizontal (l/c/r), e.g. "tl", "cc"
    pub pos: String,
    /// percent of overlay width
    pub x: f32,
    /// percent of overlay height
    pub y: f32,
}

/// `wmZorder`: number | "exclusive" | null
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ZOrder {
    Order(i64),
    Exclusive(String),
}

impl ZOrder {
    pub fn is_exclusive(z: &Option<ZOrder>) -> bool {
        matches!(z, Some(ZOrder::Exclusive(_)))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Widget {
    pub wm_id: u32,
    pub wm_title: String,
    /// "show" | "hide"
    pub wm_wants: String,
    pub wm_zorder: Option<ZOrder>,
    pub wm_flags: Vec<String>,
    #[serde(flatten)]
    pub kind: WidgetKind,
}

impl Widget {
    pub fn wants_show(&self) -> bool {
        self.wm_wants == "show"
    }
    pub fn has_flag(&self, f: &str) -> bool {
        self.wm_flags.iter().any(|x| x == f)
    }
    pub fn is_exclusive(&self) -> bool {
        ZOrder::is_exclusive(&self.wm_zorder)
    }
    pub fn type_name(&self) -> &'static str {
        self.kind.type_name()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "wmType")]
pub enum WidgetKind {
    #[serde(rename = "menu")]
    Menu(MenuWidget),
    #[serde(rename = "settings")]
    Settings(EmptyWidget),
    #[serde(rename = "item-search")]
    ItemSearch(ItemSearchWidget),
    #[serde(rename = "timer")]
    Timer(StopwatchWidget),
    #[serde(rename = "stash-search")]
    StashSearch(StashSearchWidget),
    #[serde(rename = "price-check")]
    PriceCheck(Box<PriceCheckWidget>),
    #[serde(rename = "item-check")]
    ItemCheck(ItemCheckWidget),
    #[serde(rename = "image-strip")]
    ImageStrip(ImageStripWidget),
    #[serde(rename = "delve-grid")]
    DelveGrid(DelveGridWidget),
}

impl WidgetKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Menu(_) => "menu",
            Self::Settings(_) => "settings",
            Self::ItemSearch(_) => "item-search",
            Self::Timer(_) => "timer",
            Self::StashSearch(_) => "stash-search",
            Self::PriceCheck(_) => "price-check",
            Self::ItemCheck(_) => "item-check",
            Self::ImageStrip(_) => "image-strip",
            Self::DelveGrid(_) => "delve-grid",
        }
    }
    pub fn anchor_mut(&mut self) -> Option<&mut Anchor> {
        match self {
            Self::Menu(w) => Some(&mut w.anchor),
            Self::ItemSearch(w) => Some(&mut w.anchor),
            Self::Timer(w) => Some(&mut w.anchor),
            Self::StashSearch(w) => Some(&mut w.anchor),
            Self::ImageStrip(w) => Some(&mut w.anchor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EmptyWidget {
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuWidget {
    pub anchor: Anchor,
    pub always_show: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSearchWidget {
    pub anchor: Anchor,
    pub ocr_gems_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopwatchWidget {
    #[serde(default = "default_center_anchor")]
    pub anchor: Anchor,
    #[serde(default)]
    pub toggle_key: Option<String>,
    #[serde(default)]
    pub reset_key: Option<String>,
}

fn default_center_anchor() -> Anchor {
    Anchor { pos: "cc".into(), x: 50.0, y: 50.0 }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StashSearchEntry {
    pub id: u32,
    pub name: String,
    pub text: String,
    pub hotkey: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StashSearchWidget {
    #[serde(default = "default_center_anchor")]
    pub anchor: Anchor,
    #[serde(default)]
    pub enable_hotkeys: bool,
    #[serde(default)]
    pub entries: Vec<StashSearchEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShowSeller {
    Off(bool),
    Mode(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceCheckWidget {
    pub hotkey: Option<String>,
    pub hotkey_hold: String,
    pub hotkey_locked: Option<String>,
    /// false | "account" | "ign"
    pub show_seller: ShowSeller,
    pub search_stat_range: f64,
    pub show_rate_limit_state: bool,
    pub api_latency_seconds: f64,
    /// "api" | "app"
    pub collapse_listings: String,
    pub smart_initial_search: bool,
    pub locked_initial_search: bool,
    pub activate_stock_filter: bool,
    #[serde(default = "yes")]
    pub merchant_only: bool,
    #[serde(default)]
    pub default_currency: Option<String>,
    pub show_cursor: bool,
    pub request_price_prediction: bool,
    #[serde(default)]
    pub builtin_browser: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapCheckStat {
    pub matcher: String,
    /// three profiles stored as e.g. "d--": '-' none, 'd' danger, 'w' warning, 'g' good
    pub decision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCheckConfig {
    pub profile: u32,
    pub show_new_stats: bool,
    pub selected_stats: Vec<MapCheckStat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemCheckWidget {
    pub hotkey: Option<String>,
    pub wiki_key: Option<String>,
    pub poedb_key: Option<String>,
    pub craft_of_exile_key: Option<String>,
    pub stash_search_key: Option<String>,
    pub maps: MapCheckConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StripImage {
    pub id: u32,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageStripWidget {
    #[serde(default = "default_center_anchor")]
    pub anchor: Anchor,
    #[serde(default)]
    pub images: Vec<StripImage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelveGridWidget {
    pub toggle_key: Option<String>,
}

// ---------------------------------------------------------------------------
// Defaults (upstream `defaultConfig()` + each widget's initInstance/defaultInstances)

fn w(title: &str, wants: &str, z: Option<ZOrder>, flags: &[&str], kind: WidgetKind) -> Widget {
    Widget {
        wm_id: 0,
        wm_title: title.into(),
        wm_wants: wants.into(),
        wm_zorder: z,
        wm_flags: flags.iter().map(|s| (*s).to_owned()).collect(),
        kind,
    }
}

fn anchor(pos: &str, x: f32, y: f32) -> Anchor {
    Anchor { pos: pos.into(), x, y }
}

fn excl() -> Option<ZOrder> {
    Some(ZOrder::Exclusive("exclusive".into()))
}

fn stash_entries(items: &[&str]) -> Vec<StashSearchEntry> {
    items
        .iter()
        .enumerate()
        .map(|(i, t)| StashSearchEntry { id: i as u32 + 1, name: String::new(), text: (*t).into(), hotkey: None })
        .collect()
}

impl Default for Config {
    fn default() -> Self {
        let cmd = |text: &str, hotkey: Option<&str>| Command { text: text.into(), hotkey: hotkey.map(Into::into), send: true };
        let mut widgets = vec![
            w("", "show", Some(ZOrder::Order(1)), &["invisible-on-blur", "menu::skip"], WidgetKind::Menu(MenuWidget { anchor: anchor("tl", 5.0, 5.0), always_show: false })),
            w("{icon=fa-cog}", "hide", excl(), &["invisible-on-blur", "ignore-ui-visibility"], WidgetKind::Settings(EmptyWidget::default())),
            w("{icon=fa-search}", "hide", None, &["invisible-on-blur"], WidgetKind::ItemSearch(ItemSearchWidget { anchor: anchor("tl", 10.0, 20.0), ocr_gems_key: None })),
            w("Map rolling", "hide", None, &["invisible-on-blur"], WidgetKind::StashSearch(StashSearchWidget {
                anchor: anchor("tl", 35.0, 46.0),
                enable_hotkeys: true,
                entries: stash_entries(&["\"Pack Size: +3\"", "Reflect", "\"Cannot Leech Life\"", "\"Cannot Leech Mana\""]),
            })),
            w("Dump sorting", "hide", None, &["invisible-on-blur"], WidgetKind::StashSearch(StashSearchWidget {
                anchor: anchor("tl", 34.0, 56.0),
                enable_hotkeys: true,
                entries: stash_entries(&["Currency", "\"Divination Card\"", "Fossil", "\"Map Tier\"", "\"Map Device\" \"Rarity: Normal\"", "Tane Laboratory"]),
            })),
            w("", "hide", excl(), &["hide-on-blur", "menu::skip"], WidgetKind::PriceCheck(Box::new(PriceCheckWidget {
                hotkey: Some("D".into()),
                hotkey_hold: "Ctrl".into(),
                hotkey_locked: Some("Ctrl + Alt + D".into()),
                show_seller: ShowSeller::Off(false),
                search_stat_range: 10.0,
                show_rate_limit_state: false,
                api_latency_seconds: 2.0,
                collapse_listings: "api".into(),
                smart_initial_search: true,
                locked_initial_search: true,
                activate_stock_filter: false,
                merchant_only: true,
                default_currency: None,
                show_cursor: true,
                request_price_prediction: false,
                builtin_browser: false,
                extra: Map::new(),
            }))),
            w("", "hide", excl(), &["hide-on-blur", "menu::skip"], WidgetKind::ItemCheck(ItemCheckWidget {
                hotkey: None,
                wiki_key: None,
                poedb_key: None,
                craft_of_exile_key: None,
                stash_search_key: None,
                maps: MapCheckConfig {
                    profile: 1,
                    show_new_stats: false,
                    selected_stats: vec![
                        MapCheckStat { matcher: "Players have #% to all maximum Resistances".into(), decision: "w--".into() },
                        MapCheckStat { matcher: "Monsters reflect #% of Physical Damage".into(), decision: "d--".into() },
                        MapCheckStat { matcher: "Monsters reflect #% of Elemental Damage".into(), decision: "d--".into() },
                        MapCheckStat { matcher: "Area contains two Unique Bosses".into(), decision: "g--".into() },
                    ],
                },
            })),
            w("Cheat sheets", "hide", None, &["invisible-on-blur"], WidgetKind::ImageStrip(ImageStripWidget {
                anchor: anchor("tc", 50.0, 10.0),
                images: vec![StripImage { id: 1, url: "syndicate.jpg".into() }],
            })),
            w("", "hide", None, &["hide-on-focus", "menu::skip"], WidgetKind::DelveGrid(DelveGridWidget { toggle_key: None })),
        ];
        for (i, wd) in widgets.iter_mut().enumerate() {
            wd.wm_id = i as u32 + 1;
        }
        Self {
            config_version: CONFIG_VERSION,
            league_id: None,
            overlay_key: "Shift + Space".into(),
            overlay_background: "rgba(129, 139, 149, 0.15)".into(),
            overlay_background_close: true,
            restore_clipboard: false,
            commands: vec![
                cmd("/hideout", Some("F5")),
                cmd("/exit", Some("F9")),
                cmd("@last ty", None),
                cmd("/invite @last", None),
                cmd("/tradewith @last", None),
                cmd("/hideout @last", None),
            ],
            client_log: None,
            game_config: None,
            window_title: "Path of Exile".into(),
            log_keys: false,
            account_name: String::new(),
            stash_scroll: true,
            language: "en".into(),
            realm: "pc-ggg".into(),
            use_intl_site: false,
            widgets,
            font_size: 16.0,
            show_attach_notification: true,
            rpt: RptSettings::default(),
            extra: Map::new(),
        }
    }
}

impl Config {
    /// Parse a config (ours or an APT `config.json`). Older APT versions
    /// (< 18) are not migrated; they fall back to defaults with a warning.
    pub fn from_json(s: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(s).map_err(|e| e.to_string())?;
        let ver = v.get("configVersion").and_then(Value::as_u64).unwrap_or(0);
        if ver < CONFIG_VERSION as u64 {
            return Err(format!("config version {ver} is older than {CONFIG_VERSION}; please update Awakened PoE Trade first or start fresh"));
        }
        serde_json::from_value(v).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("config serializes")
    }

    pub fn widget(&self, type_name: &str) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.type_name() == type_name)
    }

    pub fn price_check(&self) -> &PriceCheckWidget {
        self.widgets
            .iter()
            .find_map(|w| match &w.kind {
                WidgetKind::PriceCheck(p) => Some(p.as_ref()),
                _ => None,
            })
            .expect("price-check widget always exists")
    }

    pub fn item_check(&self) -> &ItemCheckWidget {
        self.widgets
            .iter()
            .find_map(|w| match &w.kind {
                WidgetKind::ItemCheck(p) => Some(p),
                _ => None,
            })
            .expect("item-check widget always exists")
    }

    pub fn next_widget_id(&self) -> u32 {
        self.widgets.iter().map(|w| w.wm_id).max().unwrap_or(0) + 1
    }

    /// Upstream `poeWebApi()`.
    pub fn poe_web_api(&self) -> &'static str {
        if self.use_intl_site {
            return "www.pathofexile.com";
        }
        match self.language.as_str() {
            "ru" => "ru.pathofexile.com",
            "cmn-Hant" => "pathofexile.tw",
            "ko" => "poe.kakaogames.com",
            _ => "www.pathofexile.com",
        }
    }
}

/// One registered hotkey (mirror of upstream `ShortcutAction`), independent of
/// the platform crate so it can be unit-tested.
#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub shortcut: String,
    pub keep_mod_keys: bool,
    pub action: ShortcutKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShortcutKind {
    CopyItem { target: String, focus_overlay: bool },
    OcrText { target: String },
    TriggerEvent { target: String },
    StashSearch { text: String },
    ToggleOverlay,
    PasteInChat { text: String, send: bool },
}

impl Config {
    /// Upstream `getConfigForHost().shortcuts`.
    pub fn shortcuts(&self) -> Vec<Shortcut> {
        let mut out = Vec::new();
        let mut push = |shortcut: String, keep: bool, action: ShortcutKind| out.push(Shortcut { shortcut, keep_mod_keys: keep, action });
        let copy = |t: &str, focus: bool| ShortcutKind::CopyItem { target: t.into(), focus_overlay: focus };

        let pc = self.price_check();
        if let Some(k) = &pc.hotkey {
            push(format!("{} + {}", pc.hotkey_hold, k), true, copy("price-check", false));
        }
        if let Some(k) = &pc.hotkey_locked {
            push(k.clone(), false, copy("price-check", true));
        }
        push(self.overlay_key.clone(), true, ShortcutKind::ToggleOverlay);
        let ic = self.item_check();
        for (key, target) in [
            (&ic.wiki_key, "open-wiki"),
            (&ic.craft_of_exile_key, "open-craft-of-exile"),
            (&ic.poedb_key, "open-poedb"),
            (&ic.stash_search_key, "search-similar"),
        ] {
            if let Some(k) = key {
                push(k.clone(), false, copy(target, false));
            }
        }
        if let Some(k) = &ic.hotkey {
            push(k.clone(), false, copy("item-check", true));
        }
        for wd in &self.widgets {
            if let WidgetKind::DelveGrid(d) = &wd.kind {
                if let Some(k) = &d.toggle_key {
                    push(k.clone(), true, ShortcutKind::TriggerEvent { target: "delve-grid".into() });
                }
            }
        }
        for c in &self.commands {
            if let Some(k) = &c.hotkey {
                push(k.clone(), false, ShortcutKind::PasteInChat { text: c.text.clone(), send: c.send });
            }
        }
        for wd in &self.widgets {
            match &wd.kind {
                WidgetKind::StashSearch(s) if s.enable_hotkeys => {
                    for e in &s.entries {
                        if let Some(k) = &e.hotkey {
                            push(k.clone(), false, ShortcutKind::StashSearch { text: e.text.clone() });
                        }
                    }
                }
                WidgetKind::Timer(t) => {
                    if let Some(k) = &t.toggle_key {
                        push(k.clone(), true, ShortcutKind::TriggerEvent { target: format!("stopwatch-start-stop:{}", wd.wm_id) });
                    }
                    if let Some(k) = &t.reset_key {
                        push(k.clone(), true, ShortcutKind::TriggerEvent { target: format!("stopwatch-reset:{}", wd.wm_id) });
                    }
                }
                WidgetKind::ItemSearch(s) => {
                    if let Some(k) = &s.ocr_gems_key {
                        push(k.clone(), true, ShortcutKind::OcrText { target: "heist-gems".into() });
                    }
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let c = Config::default();
        let j = c.to_json();
        assert!(j.contains("\"wmType\": \"price-check\""));
        let back = Config::from_json(&j).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn default_shortcuts() {
        let s = Config::default().shortcuts();
        assert_eq!(s[0].shortcut, "Ctrl + D");
        assert!(s.iter().any(|x| x.action == ShortcutKind::ToggleOverlay && x.shortcut == "Shift + Space"));
        assert!(s.iter().any(|x| matches!(&x.action, ShortcutKind::PasteInChat { text, .. } if text == "/hideout")));
    }

    #[test]
    fn preserves_unknown_keys() {
        let mut v: Value = serde_json::from_str(&Config::default().to_json()).unwrap();
        v["someFutureKey"] = Value::from(42);
        let c = Config::from_json(&v.to_string()).unwrap();
        assert!(c.to_json().contains("someFutureKey"));
    }
}
