//! Append-only mapping history: `data_dir()/mapping/history.jsonl`.
//!
//! One JSON object per line, tagged by `kind`:
//! - `{"kind":"map", …MapRun}`: a completed map;
//! - `{"kind":"session", …SessionTotals}`: a session snapshot. Several
//!   snapshots of the same session id may exist; the last one wins.
//!
//! Unreadable lines are skipped, so a torn last write never breaks loading.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::session::{MapRun, SessionTotals};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Record {
    Map(MapRun),
    Session(SessionTotals),
}

#[derive(Debug, Clone)]
pub struct History {
    pub path: PathBuf,
}

/// Everything read back from the file.
#[derive(Debug, Default)]
pub struct Loaded {
    /// All completed maps, oldest first.
    pub maps: Vec<MapRun>,
    /// Latest snapshot of the most recent session.
    pub last_session: Option<SessionTotals>,
}

impl History {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Default location under the app data dir.
    pub fn default_path() -> PathBuf {
        crate::store::data_dir().join("mapping").join("history.jsonl")
    }

    pub fn append(&self, rec: &Record) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).read(true).append(true).open(&self.path)?;
        let mut line = serde_json::to_string(rec).map_err(std::io::Error::other)?;
        // after a torn write, start on a fresh line so this record stays readable
        let len = f.metadata()?.len();
        if len > 0 {
            use std::io::{Read as _, Seek as _, SeekFrom};
            let mut last = [0u8; 1];
            f.seek(SeekFrom::Start(len - 1))?;
            f.read_exact(&mut last)?;
            if last[0] != b'\n' {
                line.insert(0, '\n');
            }
        }
        writeln!(f, "{line}")
    }

    pub fn load(&self) -> Loaded {
        load_from(&self.path)
    }
}

fn load_from(path: &Path) -> Loaded {
    let mut out = Loaded::default();
    let Ok(s) = std::fs::read_to_string(path) else { return out };
    for line in s.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Record>(line) {
            Ok(Record::Map(m)) => out.maps.push(m),
            Ok(Record::Session(s)) => {
                if out.last_session.as_ref().is_none_or(|l| s.id >= l.id) {
                    out.last_session = Some(s);
                }
            }
            Err(e) => tracing::debug!("skipping bad mapping history line: {e}"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("rpt-mapping-test-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);
        let h = History::new(dir.join("mapping").join("history.jsonl"));
        assert!(h.load().maps.is_empty());

        let run = MapRun { session: 10, area_id: "MapWorldsStrand".into(), name: "Strand".into(), level: 83, seed: 5, started: 20, ended: 400, map_secs: 300, deaths: 1, entries: 2 };
        h.append(&Record::Map(run.clone())).unwrap();
        h.append(&Record::Session(SessionTotals { id: 10, last: 400, maps: 1, map_secs: 300, ..Default::default() })).unwrap();
        h.append(&Record::Session(SessionTotals { id: 10, last: 500, maps: 1, map_secs: 300, hideout_secs: 90, ..Default::default() })).unwrap();
        // torn write
        std::fs::OpenOptions::new().append(true).open(&h.path).unwrap().write_all(b"{\"kind\":\"map\",\"sess").unwrap();

        let mut run2 = run.clone();
        run2.seed = 6;
        h.append(&Record::Map(run2.clone())).unwrap();

        let back = h.load();
        assert_eq!(back.maps, vec![run, run2]);
        let s = back.last_session.unwrap();
        assert_eq!((s.id, s.last, s.hideout_secs), (10, 500, 90));

        let text = std::fs::read_to_string(&h.path).unwrap();
        assert!(text.lines().next().unwrap().starts_with("{\"kind\":\"map\""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
