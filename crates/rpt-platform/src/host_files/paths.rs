//! Default file locations (`host-files/utils.ts` + the `POSSIBLE_PATH` tables
//! of `GameLogWatcher.ts` / `GameConfig.ts`), extended with Steam library
//! discovery, Flatpak Steam, and Proton prefixes.

use std::path::{Path, PathBuf};

/// Steam app id of Path of Exile (Proton prefix `compatdata/238960`).
pub const POE_STEAM_APP_ID: &str = "238960";

/// Inputs for path detection, injectable for tests.
#[derive(Debug, Clone, Default)]
pub struct PathEnv {
    pub home: Option<PathBuf>,
    /// The user's "Documents" folder (XDG documents dir / `FOLDERID_Documents`).
    pub documents: Option<PathBuf>,
    /// Extra Steam library roots (each containing `steamapps/`).
    pub extra_steam_libraries: Vec<PathBuf>,
}

impl PathEnv {
    pub fn from_system() -> Self {
        let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
        let documents = directories::UserDirs::new().and_then(|u| u.document_dir().map(Path::to_path_buf));
        Self { home, documents, extra_steam_libraries: Vec::new() }
    }

    /// Default Steam installation roots.
    pub fn steam_roots(&self) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if cfg!(windows) {
            v.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
            v.push(PathBuf::from(r"C:\Program Files\Steam"));
        } else if let Some(home) = &self.home {
            v.push(home.join(".local/share/Steam"));
            v.push(home.join(".steam/steam"));
            v.push(home.join(".steam/root"));
            v.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        }
        v
    }

    /// All Steam library roots: default roots plus every `"path"` entry of
    /// `steamapps/libraryfolders.vdf`, de-duplicated by canonical path.
    pub fn steam_libraries(&self) -> Vec<PathBuf> {
        let mut libs: Vec<PathBuf> = Vec::new();
        for root in self.steam_roots() {
            libs.push(root.clone());
            if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
                libs.extend(parse_library_folders(&vdf));
            }
        }
        libs.extend(self.extra_steam_libraries.iter().cloned());
        dedup_existing(libs)
    }

    /// Candidates for `Client.txt`.
    pub fn client_log_candidates(&self) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if cfg!(windows) {
            v.push(PathBuf::from(r"C:\Program Files (x86)\Grinding Gear Games\Path of Exile\logs\Client.txt"));
        } else if let Some(home) = &self.home {
            v.push(home.join(".wine/drive_c/Program Files (x86)/Grinding Gear Games/Path of Exile/logs/Client.txt"));
        }
        for lib in self.steam_libraries() {
            v.push(lib.join("steamapps/common/Path of Exile/logs/Client.txt"));
        }
        if cfg!(windows) && v.iter().all(|p| !p.starts_with(r"C:\Program Files (x86)\Steam")) {
            v.push(PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Path of Exile\logs\Client.txt"));
        }
        v
    }

    /// Candidates for `production_Config.ini`.
    pub fn game_config_candidates(&self) -> Vec<PathBuf> {
        let rel = Path::new("My Games").join("Path of Exile").join("production_Config.ini");
        let mut v = Vec::new();
        if let Some(docs) = &self.documents {
            v.push(docs.join(&rel));
        }
        if !cfg!(windows) {
            if let Some(home) = &self.home {
                v.push(home.join(".wine/drive_c/users").join(user_name()).join("Documents").join(&rel));
            }
            for lib in self.steam_libraries() {
                v.push(
                    lib.join("steamapps/compatdata")
                        .join(POE_STEAM_APP_ID)
                        .join("pfx/drive_c/users/steamuser/Documents")
                        .join(&rel),
                );
            }
        }
        v
    }
}

fn user_name() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".into())
}

/// Extracts library paths from Steam's `libraryfolders.vdf`.
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let mut parts = line.split('"').filter(|s| !s.trim().is_empty());
            let key = parts.next()?;
            if key != "path" {
                return None;
            }
            let value = parts.next()?;
            Some(PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

fn dedup_existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for p in paths {
        let Ok(canon) = p.canonicalize() else { continue };
        if !seen.contains(&canon) {
            seen.push(canon);
            out.push(p);
        }
    }
    out
}

/// Port of `guessFileLocation`: returns the single readable candidate. If
/// more than one distinct file exists (e.g. standalone + Steam) returns
/// `None`, like upstream, so the user has to choose. Symlinked duplicates
/// (`~/.steam/steam` → `~/.local/share/Steam`) count once.
pub fn guess_file_location<I: IntoIterator<Item = PathBuf>>(entries: I) -> Option<PathBuf> {
    let mut found: Option<(PathBuf, PathBuf)> = None;
    for p in entries {
        if std::fs::File::open(&p).is_err() {
            continue;
        }
        let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
        match &found {
            Some((_, c)) if *c == canon => {}
            Some(_) => return None,
            None => found = Some((p, canon)),
        }
    }
    found.map(|(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_parsing() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/u/.local/share/Steam"
		"label"		""
		"apps"
		{
			"238960"		"123"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        let libs = parse_library_folders(vdf);
        assert_eq!(libs, vec![PathBuf::from("/home/u/.local/share/Steam"), PathBuf::from("D:\\SteamLibrary")]);
    }

    #[test]
    fn guess_picks_single_and_rejects_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let missing = dir.path().join("missing.txt");
        std::fs::write(&a, "x").unwrap();
        assert_eq!(guess_file_location(vec![missing.clone(), a.clone()]), Some(a.clone()));
        assert_eq!(guess_file_location(vec![a.clone(), a.clone()]), Some(a.clone()));
        std::fs::write(&b, "y").unwrap();
        assert_eq!(guess_file_location(vec![a.clone(), b.clone()]), None);
        assert_eq!(guess_file_location(vec![missing]), None);
    }

    #[cfg(unix)]
    #[test]
    fn proton_paths_and_symlinked_steam_roots() {
        let home = tempfile::tempdir().unwrap();
        let steam = home.path().join(".local/share/Steam");
        let logs = steam.join("steamapps/common/Path of Exile/logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("Client.txt"), "").unwrap();
        let ini_dir = steam.join(
            "steamapps/compatdata/238960/pfx/drive_c/users/steamuser/Documents/My Games/Path of Exile",
        );
        std::fs::create_dir_all(&ini_dir).unwrap();
        std::fs::write(ini_dir.join("production_Config.ini"), "").unwrap();
        std::fs::create_dir_all(home.path().join(".steam")).unwrap();
        std::os::unix::fs::symlink(&steam, home.path().join(".steam/steam")).unwrap();

        let env = PathEnv { home: Some(home.path().to_path_buf()), documents: None, extra_steam_libraries: vec![] };
        assert_eq!(env.steam_libraries().len(), 1);
        assert_eq!(guess_file_location(env.client_log_candidates()), Some(logs.join("Client.txt")));
        assert_eq!(guess_file_location(env.game_config_candidates()), Some(ini_dir.join("production_Config.ini")));
    }
}
