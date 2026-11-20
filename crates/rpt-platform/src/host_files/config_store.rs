//! Port of `host-files/ConfigStore.ts` and `host-files/file-uploads.ts`.
//!
//! Layout (via `directories::ProjectDirs` for "rusty-poe-trade"):
//! * Linux:   `~/.config/rusty-poe-trade/config.json`, uploads in
//!   `~/.local/share/rusty-poe-trade/files/`
//! * Windows: `%APPDATA%\rusty-poe-trade\config\config.json`, uploads in
//!   `%APPDATA%\rusty-poe-trade\data\files\`

use std::path::{Path, PathBuf};

use anyhow::Context;

pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
    uploads_dir: PathBuf,
    cfg_path: PathBuf,
    is_tmp_file: bool,
}

impl ConfigStore {
    /// Store in the standard per-user location.
    pub fn open_default() -> anyhow::Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "rusty-poe-trade")
            .context("cannot determine the user's home directory")?;
        Ok(Self::at(dirs.config_dir(), &dirs.data_dir().join("files")))
    }

    /// Store in explicit directories (portable mode / tests).
    pub fn at(config_dir: &Path, uploads_dir: &Path) -> Self {
        Self {
            dir: config_dir.to_path_buf(),
            uploads_dir: uploads_dir.to_path_buf(),
            cfg_path: config_dir.join(CONFIG_FILE),
            is_tmp_file: false,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.dir
    }

    pub fn config_path(&self) -> &Path {
        &self.cfg_path
    }

    pub fn uploads_dir(&self) -> &Path {
        &self.uploads_dir
    }

    /// Returns the raw JSON, or `None` if there is no config yet.
    pub fn load(&self) -> Option<String> {
        std::fs::read_to_string(&self.cfg_path).ok()
    }

    /// Saves the config. Like upstream, once a temporary save happens
    /// (`is_temporary`, e.g. while the user tries settings), all further saves
    /// of this session go to `config.json.tmp`. Writes are atomic
    /// (write to a sibling file, then rename).
    pub fn save(&mut self, contents: &str, is_temporary: bool) -> anyhow::Result<()> {
        if is_temporary && !self.is_tmp_file {
            self.cfg_path = self.dir.join(format!("{CONFIG_FILE}.tmp"));
            self.is_tmp_file = true;
        }
        std::fs::create_dir_all(&self.dir).with_context(|| format!("creating {}", self.dir.display()))?;
        let partial = self.cfg_path.with_extension("partial");
        std::fs::write(&partial, contents).with_context(|| format!("writing {}", partial.display()))?;
        std::fs::rename(&partial, &self.cfg_path)
            .with_context(|| format!("replacing {}", self.cfg_path.display()))?;
        Ok(())
    }

    /// `POST /uploads/<name>`: stores an imported file (stash-tab images etc.)
    /// under a content-hash name and returns that name. Max 16 MB.
    pub fn save_upload(&self, original_name: &str, contents: &[u8]) -> anyhow::Result<String> {
        anyhow::ensure!(contents.len() <= 16_000_000, "upload too large");
        let ext = Path::new(original_name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let name = format!("{}{}", content_hash(contents), ext);
        std::fs::create_dir_all(&self.uploads_dir)?;
        std::fs::write(self.uploads_dir.join(&name), contents)?;
        Ok(name)
    }

    /// `GET /uploads/<name>`. Rejects path traversal.
    pub fn read_upload(&self, name: &str) -> Option<Vec<u8>> {
        if name.contains(['/', '\\']) || name.contains("..") {
            return None;
        }
        std::fs::read(self.uploads_dir.join(name)).ok()
    }

    /// Tray "Open config folder".
    pub fn open_config_folder(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        open_path(&self.dir)
    }
}

/// 128-bit FNV-1a (two independent 64-bit lanes) as hex. Upstream used MD5;
/// the name only needs to be stable and collision-resistant for dedup.
fn content_hash(data: &[u8]) -> String {
    use std::hash::Hasher;
    let mut a = fnv::FnvHasher::default();
    a.write(data);
    let mut b = fnv::FnvHasher::with_key(0x6c62_272e_07bb_0142);
    b.write(data);
    b.write_usize(data.len());
    format!("{:016x}{:016x}", a.finish(), b.finish())
}

/// Opens a folder/file with the desktop's default handler.
pub fn open_path(path: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("explorer");
        c.arg(path);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.spawn().with_context(|| format!("opening {}", path.display()))?;
    Ok(())
}

/// Opens a URL in the default browser (e.g. the release page).
pub fn open_url(url: &str) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn()?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

/// Candidate locations of an Awakened PoE Trade `config.json`
/// (`<Electron userData>/apt-data/config.json`). Electron's userData is
/// `<appData>/<app name>`; APT builds have used both the package name and the
/// product name.
pub fn apt_config_candidates(base_config_dir: &Path) -> Vec<PathBuf> {
    ["awakened-poe-trade", "Awakened PoE Trade"]
        .iter()
        .map(|name| base_config_dir.join(name).join("apt-data").join(CONFIG_FILE))
        .collect()
}

/// Finds an existing Awakened PoE Trade config to import:
/// Linux `~/.config/awakened-poe-trade/apt-data/config.json` (also the
/// Flatpak/AppImage location), Windows
/// `%APPDATA%\awakened-poe-trade\apt-data\config.json`.
pub fn find_apt_config() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let mut candidates = apt_config_candidates(base.config_dir());
    if cfg!(target_os = "linux") {
        // Electron ignores XDG_CONFIG_HOME only in very old versions; check ~/.config too.
        candidates.extend(apt_config_candidates(&base.home_dir().join(".config")));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Reads an APT config for import. The JSON schema is the renderer's
/// `Config` (handled by the app crate); this only validates it is JSON.
pub fn read_apt_config(path: &Path) -> anyhow::Result<String> {
    let s = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str::<serde_json::Value>(&s).with_context(|| format!("parsing {}", path.display()))?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = ConfigStore::at(&dir.path().join("cfg"), &dir.path().join("files"));
        assert_eq!(st.load(), None);
        st.save("{\"a\":1}", false).unwrap();
        assert_eq!(st.load().as_deref(), Some("{\"a\":1}"));
        st.save("{\"a\":2}", true).unwrap();
        st.save("{\"a\":3}", false).unwrap();
        assert!(st.config_path().ends_with("config.json.tmp"));
        assert_eq!(std::fs::read_to_string(dir.path().join("cfg/config.json")).unwrap(), "{\"a\":1}");
        assert_eq!(st.load().as_deref(), Some("{\"a\":3}"));
    }

    #[test]
    fn uploads() {
        let dir = tempfile::tempdir().unwrap();
        let st = ConfigStore::at(dir.path(), &dir.path().join("files"));
        let n1 = st.save_upload("tab.png", b"abc").unwrap();
        let n2 = st.save_upload("other.png", b"abc").unwrap();
        assert_eq!(n1, n2);
        assert!(n1.ends_with(".png"));
        assert_eq!(st.read_upload(&n1).unwrap(), b"abc");
        assert!(st.read_upload("../x").is_none());
    }

    #[test]
    fn apt_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let c = apt_config_candidates(dir.path());
        assert!(c[0].ends_with("awakened-poe-trade/apt-data/config.json"));
        std::fs::create_dir_all(c[1].parent().unwrap()).unwrap();
        std::fs::write(&c[1], "{\"leagueId\":\"Standard\"}").unwrap();
        assert!(read_apt_config(&c[1]).is_ok());
        std::fs::write(&c[1], "nope").unwrap();
        assert!(read_apt_config(&c[1]).is_err());
    }
}
