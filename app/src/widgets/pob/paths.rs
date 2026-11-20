//! Locating Path of Building's `Builds` folder and listing saved builds.
//!
//! Linux assumptions (documented, unverified where noted):
//! - Native / AppImage PoB-Community (the `pobfrontend` Linux port) keeps builds in
//!   `~/Documents/Path of Building/Builds` like on Windows. Some packagings use
//!   `$XDG_DATA_HOME/pobfrontend/Path of Building/Builds` (uncertain, probed anyway).
//! - Wine: `~/.wine/drive_c/users/$USER/Documents/Path of Building/Builds`.
//! - Steam (Proton, PoB added as a non-Steam game):
//!   `~/.local/share/Steam/steamapps/compatdata/*/pfx/drive_c/users/steamuser/Documents/...`,
//!   also `~/.steam/steam/...` and the Steam Flatpak's `~/.var/app/com.valvesoftware.Steam/...`.
//! - Flatpak: Flathub id `community.pathofbuilding.PathOfBuilding` (PoB-Community, Wine based).
//!   Where its Wine prefix keeps "Documents" is uncertain, so several plausible layouts
//!   are probed: `data/wine/drive_c/users/*/Documents`, `data/pobfrontend/...` and the
//!   app's own `Documents` folder.
//! - Lutris / Bottles style prefixes are not auto-detected; type the folder instead.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

const POB_DIR: &str = "Path of Building";
const BUILDS: &str = "Builds";
pub const FLATPAK_ID: &str = "community.pathofbuilding.PathOfBuilding";

/// Inputs for candidate generation (pure: no filesystem access).
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub windows: bool,
    pub home: PathBuf,
    pub user: Option<String>,
    /// Windows `%OneDrive%`.
    pub onedrive: Option<PathBuf>,
    /// `$XDG_DATA_HOME` (defaults to `~/.local/share`).
    pub xdg_data: Option<PathBuf>,
    /// `$XDG_DOCUMENTS_DIR` if it differs from `~/Documents`.
    pub documents: Option<PathBuf>,
}

impl Env {
    pub fn current() -> Self {
        let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).unwrap_or_default();
        Self {
            windows: cfg!(windows),
            home,
            user: std::env::var("USER").or_else(|_| std::env::var("USERNAME")).ok(),
            onedrive: std::env::var_os("OneDrive").map(PathBuf::from),
            xdg_data: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            documents: directories::UserDirs::new().and_then(|u| u.document_dir().map(Path::to_path_buf)),
        }
    }
}

fn builds_in(docs: PathBuf) -> PathBuf {
    docs.join(POB_DIR).join(BUILDS)
}

/// Candidate `Builds` folders, in priority order. A path component `*`
/// is a wildcard expanded by [`expand`].
pub fn candidate_patterns(env: &Env) -> Vec<PathBuf> {
    let h = &env.home;
    let mut out = Vec::new();
    if let Some(d) = &env.documents {
        out.push(builds_in(d.clone()));
    }
    out.push(builds_in(h.join("Documents")));
    if env.windows {
        if let Some(od) = &env.onedrive {
            out.push(builds_in(od.join("Documents")));
        }
        out.push(builds_in(h.join("OneDrive").join("Documents")));
    } else {
        let data = env.xdg_data.clone().unwrap_or_else(|| h.join(".local").join("share"));
        out.push(data.join("pobfrontend").join(POB_DIR).join(BUILDS));
        let users = |prefix: PathBuf| -> Vec<PathBuf> {
            let mut v = Vec::new();
            if let Some(u) = &env.user {
                v.push(builds_in(prefix.join("drive_c").join("users").join(u).join("Documents")));
            }
            v.push(builds_in(prefix.join("drive_c").join("users").join("*").join("Documents")));
            v
        };
        out.extend(users(h.join(".wine")));
        for steam in [
            data.join("Steam"),
            h.join(".steam").join("steam"),
            h.join(".var").join("app").join("com.valvesoftware.Steam").join(".local").join("share").join("Steam"),
        ] {
            out.push(builds_in(
                steam.join("steamapps").join("compatdata").join("*").join("pfx").join("drive_c").join("users").join("steamuser").join("Documents"),
            ));
        }
        let fp = h.join(".var").join("app").join(FLATPAK_ID);
        out.extend(users(fp.join("data").join("wine")));
        out.push(fp.join("data").join("pobfrontend").join(POB_DIR).join(BUILDS));
        out.push(builds_in(fp.join("Documents")));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

/// Expand `*` components against the filesystem; returns existing directories only.
pub fn expand(pattern: &Path) -> Vec<PathBuf> {
    let mut acc = vec![PathBuf::new()];
    for comp in pattern.components() {
        let s = comp.as_os_str();
        if s == "*" {
            acc = acc
                .iter()
                .flat_map(|base| std::fs::read_dir(base).into_iter().flatten().flatten())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .collect();
            acc.sort();
        } else {
            for p in &mut acc {
                p.push(s);
            }
        }
    }
    acc.into_iter().filter(|p| p.is_dir()).collect()
}

/// Existing `Builds` folders on this machine.
pub fn detect(env: &Env) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in candidate_patterns(env) {
        for d in expand(&p) {
            if !out.contains(&d) {
                out.push(d);
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildFile {
    pub path: PathBuf,
    /// Folder relative to the builds root ("" for the root).
    pub group: String,
    pub name: String,
    pub modified: SystemTime,
}

/// List `.xml` builds recursively, newest first within each group; groups sorted
/// by their newest build (root first).
pub fn list_builds(root: &Path) -> Vec<BuildFile> {
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    let newest = |g: &str, out: &[BuildFile]| out.iter().filter(|b| b.group == g).map(|b| b.modified).max();
    let mut groups: Vec<(String, Option<SystemTime>)> = Vec::new();
    for b in &out {
        if !groups.iter().any(|(g, _)| *g == b.group) {
            groups.push((b.group.clone(), newest(&b.group, &out)));
        }
    }
    groups.sort_by(|a, b| (!a.0.is_empty()).cmp(&!b.0.is_empty()).then(b.1.cmp(&a.1)));
    out.sort_by(|a, b| {
        let ga = groups.iter().position(|(g, _)| *g == a.group);
        let gb = groups.iter().position(|(g, _)| *g == b.group);
        ga.cmp(&gb).then(b.modified.cmp(&a.modified)).then(a.name.cmp(&b.name))
    });
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<BuildFile>) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(root, &p, depth + 1, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")) {
            let group = p
                .parent()
                .and_then(|d| d.strip_prefix(root).ok())
                .map(|d| d.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
                .unwrap_or_default();
            out.push(BuildFile {
                name: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                group,
                modified: e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH),
                path: p,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_candidates() {
        let env = Env { home: "/home/u".into(), user: Some("u".into()), ..Default::default() };
        let c: Vec<String> = candidate_patterns(&env).iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let has = |s: &str| assert!(c.iter().any(|x| x == s), "missing {s} in {c:#?}");
        has("/home/u/Documents/Path of Building/Builds");
        has("/home/u/.local/share/pobfrontend/Path of Building/Builds");
        has("/home/u/.wine/drive_c/users/u/Documents/Path of Building/Builds");
        has("/home/u/.wine/drive_c/users/*/Documents/Path of Building/Builds");
        has("/home/u/.local/share/Steam/steamapps/compatdata/*/pfx/drive_c/users/steamuser/Documents/Path of Building/Builds");
        has("/home/u/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/compatdata/*/pfx/drive_c/users/steamuser/Documents/Path of Building/Builds");
        has("/home/u/.var/app/community.pathofbuilding.PathOfBuilding/data/wine/drive_c/users/u/Documents/Path of Building/Builds");
        assert_eq!(c[0], "/home/u/Documents/Path of Building/Builds");
        assert!(!c.iter().any(|x| x.contains("OneDrive")));
    }

    #[test]
    fn windows_candidates() {
        let env = Env {
            windows: true,
            home: PathBuf::from("C:/Users/u"),
            onedrive: Some(PathBuf::from("D:/OneDrive")),
            documents: Some(PathBuf::from("C:/Users/u/Documents")),
            ..Default::default()
        };
        let c = candidate_patterns(&env);
        assert_eq!(c[0], PathBuf::from("C:/Users/u/Documents").join("Path of Building").join("Builds"));
        assert_eq!(c.len(), 3, "deduplicated documents + 2 OneDrive: {c:?}");
        assert!(c.contains(&PathBuf::from("D:/OneDrive/Documents").join("Path of Building").join("Builds")));
        assert!(!c.iter().any(|p| p.to_string_lossy().contains(".wine")));
    }

    #[test]
    fn expands_and_lists() {
        let tmp = std::env::temp_dir().join(format!("rpt-pob-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let a = tmp.join("compat").join("111").join("Builds");
        let b = tmp.join("compat").join("222").join("Builds");
        std::fs::create_dir_all(a.join("League").join("Sub")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("Old.xml"), "<x/>").unwrap();
        std::fs::write(a.join("notes.txt"), "").unwrap();
        std::fs::write(a.join("League").join("Sub").join("Deep.XML"), "<x/>").unwrap();
        let found = expand(&tmp.join("compat").join("*").join("Builds"));
        assert_eq!(found, vec![a.clone(), b]);
        let list = list_builds(&a);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].group, "", "root group first");
        assert_eq!(list[1].group, "League/Sub");
        assert_eq!(list[1].name, "Deep");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
