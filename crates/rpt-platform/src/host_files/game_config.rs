//! Port of `host-files/GameConfig.ts`: reads `production_Config.ini` to find
//! the game's own key bindings (`[ACTION_KEYS]`).
//!
//! Bindings are stored as `<vk code>[ <modifier>]`, where modifier `1` =
//! Shift, `2` = Ctrl, `3` = Alt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ini::{Ini, ParseOption};

use super::paths::{PathEnv, guess_file_location};
use crate::keys::{Key, hotkey_to_string};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameConfig {
    /// The file actually read.
    pub path: PathBuf,
    /// Raw `[ACTION_KEYS]` entries.
    pub action_keys: BTreeMap<String, String>,
}

#[derive(Debug)]
pub enum GameConfigError {
    NotFound,
    Read(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for GameConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("Failed to find game configuration file in the default location."),
            Self::Read(e) => write!(f, "Failed to read game configuration file: {e}"),
            Self::Parse(e) => write!(f, "Failed to parse game configuration file: {e}"),
        }
    }
}

impl std::error::Error for GameConfigError {}

impl GameConfig {
    /// `readConfig(filePath)`: `None`/empty path = auto-detect.
    pub fn read(path: Option<&Path>) -> Result<GameConfig, GameConfigError> {
        let path = match path.filter(|p| !p.as_os_str().is_empty()) {
            Some(p) => p.to_path_buf(),
            None => guess_file_location(PathEnv::from_system().game_config_candidates())
                .ok_or(GameConfigError::NotFound)?,
        };
        let bytes = std::fs::read(&path).map_err(GameConfigError::Read)?;
        let mut cfg = Self::parse(&String::from_utf8_lossy(&bytes))?;
        cfg.path = path;
        Ok(cfg)
    }

    pub fn parse(contents: &str) -> Result<GameConfig, GameConfigError> {
        // remove BOM / leading whitespace like upstream's trimStart()
        let contents = contents.trim_start_matches('\u{feff}').trim_start();
        let opt = ParseOption { enabled_quote: false, enabled_escape: false, ..Default::default() };
        let ini = Ini::load_from_str_opt(contents, opt).map_err(|e| GameConfigError::Parse(e.to_string()))?;
        let action_keys = ini
            .section(Some("ACTION_KEYS"))
            .map(|s| s.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
            .unwrap_or_default();
        Ok(GameConfig { path: PathBuf::new(), action_keys })
    }

    /// A binding as an APT shortcut string, e.g. `binding("chat")` → `"Enter"`.
    pub fn binding(&self, action: &str) -> Option<String> {
        parse_config_hotkey(self.action_keys.get(action)?)
    }

    /// `showModsKey` (upstream default `Alt` when unset). Note: since PoE 3.29
    /// copying always yields the advanced description, so the copy-item flow
    /// ignores this and uses Ctrl, exactly like upstream.
    pub fn show_mods_key(&self) -> String {
        self.binding("show_advanced_item_descriptions").unwrap_or_else(|| "Alt".into())
    }

    /// The key that opens chat (`Enter` by default).
    pub fn chat_key(&self) -> String {
        self.binding("chat").unwrap_or_else(|| "Enter".into())
    }
}

/// Port of `parseConfigHotkey`.
pub fn parse_config_hotkey(cfg_key: &str) -> Option<String> {
    let mut parts = cfg_key.split(' ');
    let main = parts.next()?;
    let key = Key::from_vk(main.trim().parse().ok()?)?;
    let (ctrl, shift, alt) = match parts.next() {
        None | Some("") => (false, false, false),
        Some("1") => (false, true, false),
        Some("2") => (true, false, false),
        Some("3") => (false, false, true),
        Some(_) => return None,
    };
    Some(hotkey_to_string(&[key.name()], ctrl, shift, alt))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}\n[DISPLAY]\nfullscreen=false\n\n[ACTION_KEYS]\nchat=13\nshow_advanced_item_descriptions=18\nuse_bound_skill1=68 2\nweird=999\nbadmod=65 7\n";

    #[test]
    fn parses_ini() {
        let cfg = GameConfig::parse(SAMPLE).unwrap();
        assert_eq!(cfg.chat_key(), "Enter");
        assert_eq!(cfg.show_mods_key(), "Alt");
        assert_eq!(cfg.binding("use_bound_skill1").as_deref(), Some("Ctrl + D"));
        assert_eq!(cfg.binding("weird"), None);
        assert_eq!(cfg.binding("badmod"), None);
        assert_eq!(cfg.binding("missing"), None);
    }

    #[test]
    fn parses_hotkeys() {
        assert_eq!(parse_config_hotkey("17").as_deref(), Some("Ctrl"));
        assert_eq!(parse_config_hotkey("70 1").as_deref(), Some("Shift + F"));
        assert_eq!(parse_config_hotkey("112 3").as_deref(), Some("Alt + F1"));
        assert_eq!(parse_config_hotkey("abc"), None);
    }

    #[test]
    fn missing_section() {
        let cfg = GameConfig::parse("[DISPLAY]\na=1\n").unwrap();
        assert!(cfg.action_keys.is_empty());
        assert_eq!(cfg.chat_key(), "Enter");
    }
}
