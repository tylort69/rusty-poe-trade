//! Config persistence (upstream `main/src/host-files/ConfigStore.ts`).
//! Stored as `config.json` in the OS config dir:
//! - Linux: `$XDG_CONFIG_HOME/rusty-poe-trade/config.json`
//! - Windows: `%APPDATA%\rusty-poe-trade\config\config.json`

use std::path::PathBuf;

use anyhow::Context as _;

use crate::config::Config;

pub struct ConfigStore {
    pub path: PathBuf,
}

impl ConfigStore {
    pub fn new() -> Self {
        let dir = directories::ProjectDirs::from("", "", "rusty-poe-trade")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Self { path: dir.join("config.json") }
    }

    pub fn dir(&self) -> PathBuf {
        self.path.parent().map(PathBuf::from).unwrap_or_default()
    }

    /// Load config; on first run, try importing an Awakened PoE Trade config.
    pub fn load(&self) -> Config {
        if let Ok(s) = std::fs::read_to_string(&self.path) {
            match Config::from_json(&s) {
                Ok(c) => return c,
                Err(e) => tracing::warn!("ignoring invalid config {}: {e}", self.path.display()),
            }
        } else if let Some(c) = import_apt_config() {
            tracing::info!("imported Awakened PoE Trade config");
            return c;
        }
        Config::default()
    }

    pub fn save(&self, cfg: &Config) -> anyhow::Result<()> {
        std::fs::create_dir_all(self.dir()).context("create config dir")?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, cfg.to_json()).context("write config")?;
        std::fs::rename(&tmp, &self.path).context("replace config")?;
        Ok(())
    }
}

/// Known locations of APT's Electron `userData/apt-data/config.json`.
pub fn apt_config_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        for name in ["awakened-poe-trade", "Awakened PoE Trade"] {
            out.push(base.config_dir().join(name).join("apt-data").join("config.json"));
        }
    }
    out
}

pub fn import_apt_config() -> Option<Config> {
    apt_config_candidates().into_iter().find_map(|p| {
        let s = std::fs::read_to_string(&p).ok()?;
        Config::from_json(&s).map_err(|e| tracing::warn!("cannot import {}: {e}", p.display())).ok()
    })
}

/// Directory for app-generated data that isn't config (history, snapshots,
/// caches). Linux: `$XDG_DATA_HOME/rusty-poe-trade`, Windows: `%APPDATA%\rusty-poe-trade\data`.
/// Created on first use.
pub fn data_dir() -> PathBuf {
    let dir = directories::ProjectDirs::from("", "", "rusty-poe-trade")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("rpt-data"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}
