//! Game data for Rusty POE Trade: base types, stats, and localized client strings.
//!
//! Port of upstream `renderer/src/assets/data/index.ts`. Data files are embedded
//! in the binary (see `embedded.rs`) so the app ships as a single executable.
//!
//! Upstream used fnv1a-hashed binary indexes over the raw ndjson. Here we build
//! in-memory hash maps once per language load, but keep the raw ndjson lines so
//! the substring-based iterators (`items_iter`, `stats_iter`) behave identically.

mod client_strings;
mod embedded;
mod regex;
mod types;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;

pub use client_strings::ClientStrings;
pub use regex::JsRegex;
pub use types::*;

/// Languages that ship with game data.
pub const LANGUAGES: &[&str] = &["en", "ru", "cmn-Hant", "ko"];

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),
    #[error("failed to parse {file}: {source}")]
    Parse {
        file: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("stat validation failed: {0}")]
    Validation(String),
}

/// All data for one language plus the English reference strings.
pub struct GameData {
    pub language: String,
    pub client_strings: ClientStrings,
    /// English client strings (upstream `CLIENT_STRINGS_REF`).
    pub client_strings_ref: Arc<ClientStrings>,
    pub item_drop: Arc<Vec<DropEntry>>,
    /// Localized UI strings for the app (upstream `app_i18n.json`), kept as raw JSON.
    pub app_i18n: serde_json::Value,

    items: Vec<BaseType>,
    items_raw: Vec<&'static str>,
    items_by_name: HashMap<(Namespace, String), Vec<usize>>,
    items_by_ref: HashMap<(Namespace, String), Vec<usize>>,

    stats: Vec<StatOrGroup>,
    stats_raw: Vec<&'static str>,
    stats_by_ref: HashMap<String, usize>,
    stats_by_matcher: HashMap<String, usize>,
}

impl GameData {
    pub fn load(language: &str) -> Result<Self, DataError> {
        let files = embedded::files(language)
            .ok_or_else(|| DataError::UnsupportedLanguage(language.to_owned()))?;

        let client_strings: ClientStrings = serde_json::from_str(files.client_strings)
            .map_err(|source| DataError::Parse { file: "client_strings.json", source })?;
        let app_i18n = serde_json::from_str(files.app_i18n)
            .map_err(|source| DataError::Parse { file: "app_i18n.json", source })?;

        let mut items = Vec::new();
        let mut items_raw = Vec::new();
        let mut items_by_name: HashMap<(Namespace, String), Vec<usize>> = HashMap::new();
        let mut items_by_ref: HashMap<(Namespace, String), Vec<usize>> = HashMap::new();
        for line in files.items.lines().filter(|l| !l.is_empty()) {
            let item: BaseType = serde_json::from_str(line)
                .map_err(|source| DataError::Parse { file: "items.ndjson", source })?;
            let idx = items.len();
            items_by_name.entry((item.namespace, item.name.clone())).or_default().push(idx);
            items_by_ref.entry((item.namespace, item.ref_name.clone())).or_default().push(idx);
            items.push(item);
            items_raw.push(line);
        }

        let mut stats = Vec::new();
        let mut stats_raw = Vec::new();
        let mut stats_by_ref = HashMap::new();
        let mut stats_by_matcher = HashMap::new();
        for line in files.stats.lines().filter(|l| !l.is_empty()) {
            let sog: StatOrGroup = serde_json::from_str(line)
                .map_err(|source| DataError::Parse { file: "stats.ndjson", source })?;
            let idx = stats.len();
            for stat in sog.stats() {
                stats_by_ref.entry(stat.ref_.clone()).or_insert(idx);
                for m in &stat.matchers {
                    stats_by_matcher.entry(m.string.clone()).or_insert(idx);
                    if let Some(adv) = &m.advanced {
                        stats_by_matcher.entry(adv.clone()).or_insert(idx);
                    }
                }
            }
            stats.push(sog);
            stats_raw.push(line);
        }

        Ok(Self {
            language: language.to_owned(),
            client_strings,
            client_strings_ref: shared_ref_strings()?,
            item_drop: shared_item_drop()?,
            app_i18n,
            items,
            items_raw,
            items_by_name,
            items_by_ref,
            stats,
            stats_raw,
            stats_by_ref,
            stats_by_matcher,
        })
    }

    /// Upstream `ITEM_BY_TRANSLATED`.
    pub fn item_by_translated(&self, ns: Namespace, name: &str) -> Option<Vec<&BaseType>> {
        self.items_by_name
            .get(&(ns, name.to_owned()))
            .map(|ids| ids.iter().map(|&i| &self.items[i]).collect())
    }

    /// Upstream `ITEM_BY_REF`.
    pub fn item_by_ref(&self, ns: Namespace, ref_name: &str) -> Option<Vec<&BaseType>> {
        self.items_by_ref
            .get(&(ns, ref_name.to_owned()))
            .map(|ids| ids.iter().map(|&i| &self.items[i]).collect())
    }

    /// Upstream `ITEMS_ITERATOR`: items whose raw JSON line contains `includes`
    /// and every string in `and_includes`.
    pub fn items_iter<'a>(
        &'a self,
        includes: &'a str,
        and_includes: &'a [&'a str],
    ) -> impl Iterator<Item = &'a BaseType> + 'a {
        self.items_raw
            .iter()
            .enumerate()
            .filter(move |(_, l)| l.contains(includes) && and_includes.iter().all(|s| l.contains(s)))
            .map(move |(i, _)| &self.items[i])
    }

    pub fn all_items(&self) -> &[BaseType] {
        &self.items
    }

    /// Upstream `REPLICA_UNIQUE_NAMES`.
    pub fn replica_unique_names(&self) -> impl Iterator<Item = &str> {
        self.items_iter("refName\":\"Replica", &[]).map(|i| i.name.as_str())
    }

    /// Upstream `ALTQ_GEM_NAMES`.
    pub fn altq_gem_names(&self) -> impl Iterator<Item = &str> {
        self.items_iter("altQuality\":[\"Anomalous", &[]).map(|i| i.name.as_str())
    }

    /// Upstream `STAT_BY_REF_V2`.
    pub fn stat_by_ref_v2(&self, ref_: &str) -> Option<&StatOrGroup> {
        self.stats_by_ref.get(ref_).map(|&i| &self.stats[i])
    }

    /// Upstream `STAT_BY_MATCH_STR_V2`.
    pub fn stat_by_match_str_v2(&self, match_str: &str) -> Option<&StatOrGroup> {
        self.stats_by_matcher.get(match_str).map(|&i| &self.stats[i])
    }

    /// Upstream `STAT_BY_MATCH_STR`: resolves to a single stat + the matcher that hit.
    pub fn stat_by_match_str(&self, match_str: &str) -> Option<(&Stat, &StatMatcher)> {
        let sog = self.stat_by_match_str_v2(match_str)?;
        let hits = |s: &&Stat| {
            s.matchers
                .iter()
                .any(|m| m.string == match_str || m.advanced.as_deref() == Some(match_str))
        };
        let stat = match sog {
            StatOrGroup::Group(g) => {
                let mut it = g.stats.iter().filter(hits);
                let first = it.next()?;
                if it.next().is_some() {
                    return None;
                }
                first
            }
            StatOrGroup::Stat(s) => s,
        };
        let matcher = stat
            .matchers
            .iter()
            .find(|m| m.string == match_str || m.advanced.as_deref() == Some(match_str))?;
        Some((stat, matcher))
    }

    /// Upstream `STATS_ITERATOR`: flattens groups.
    pub fn stats_iter<'a>(
        &'a self,
        includes: &'a str,
        and_includes: &'a [&'a str],
    ) -> impl Iterator<Item = &'a Stat> + 'a {
        self.stats_raw
            .iter()
            .enumerate()
            .filter(move |(_, l)| l.contains(includes) && and_includes.iter().all(|s| l.contains(s)))
            .flat_map(move |(i, _)| self.stats[i].stats().iter())
    }

    /// Upstream `pseudoStatByRef`.
    pub fn pseudo_stat_by_ref(&self, ref_: &str) -> Option<&Stat> {
        match self.stat_by_ref_v2(ref_)? {
            StatOrGroup::Group(g) => g
                .stats
                .iter()
                .find(|s| s.ref_ == ref_ && s.trade.ids.contains_key("pseudo")),
            StatOrGroup::Stat(s) => Some(s),
        }
    }

    /// Validates refs used by ported code (upstream `stat()` + `DELAYED_STAT_VALIDATION`).
    pub fn validate_stat_refs<'a>(&self, refs: impl IntoIterator<Item = &'a str>) -> Result<(), DataError> {
        for r in refs {
            let sog = self
                .stat_by_ref_v2(r)
                .ok_or_else(|| DataError::Validation(format!("Cannot find stat: {r}")))?;
            if let StatOrGroup::Group(g) = sog {
                if self.language == "en" && g.stats.iter().any(|s| s.ref_ != r) {
                    return Err(DataError::Validation(format!("Some stats have different ref text: {r}")));
                }
            }
        }
        Ok(())
    }
}

fn shared_ref_strings() -> Result<Arc<ClientStrings>, DataError> {
    static CELL: OnceLock<Arc<ClientStrings>> = OnceLock::new();
    if let Some(v) = CELL.get() {
        return Ok(v.clone());
    }
    let en = embedded::files("en").expect("en data is always embedded");
    let cs: ClientStrings = serde_json::from_str(en.client_strings)
        .map_err(|source| DataError::Parse { file: "en/client_strings.json", source })?;
    Ok(CELL.get_or_init(|| Arc::new(cs)).clone())
}

fn shared_item_drop() -> Result<Arc<Vec<DropEntry>>, DataError> {
    static CELL: OnceLock<Arc<Vec<DropEntry>>> = OnceLock::new();
    if let Some(v) = CELL.get() {
        return Ok(v.clone());
    }
    let v: Vec<DropEntry> = serde_json::from_str(embedded::ITEM_DROP)
        .map_err(|source| DataError::Parse { file: "item-drop.json", source })?;
    Ok(CELL.get_or_init(|| Arc::new(v)).clone())
}

/// Patreon supporters list shown in the About page (upstream `APP_PATRONS`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Patron {
    pub from: String,
    pub months: u32,
    pub style: u32,
}

pub fn patrons() -> Vec<Patron> {
    serde_json::from_str(embedded::PATRONS).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Global current-language data (upstream module-level `let` exports).

static CURRENT: OnceLock<RwLock<Arc<GameData>>> = OnceLock::new();

/// Load (or switch) the active language. Cheap to call again with the same language.
pub fn init(language: &str) -> Result<Arc<GameData>, DataError> {
    if let Some(lock) = CURRENT.get() {
        if lock.read().language == language {
            return Ok(lock.read().clone());
        }
    }
    let data = Arc::new(GameData::load(language)?);
    match CURRENT.get() {
        Some(lock) => *lock.write() = data.clone(),
        None => {
            let _ = CURRENT.set(RwLock::new(data.clone()));
        }
    }
    Ok(data)
}

/// The active language's data. Panics if [`init`] was never called.
pub fn data() -> Arc<GameData> {
    CURRENT
        .get()
        .expect("rpt_data::init() must be called before rpt_data::data()")
        .read()
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_all_languages() {
        for lang in LANGUAGES {
            let d = GameData::load(lang).unwrap_or_else(|e| panic!("{lang}: {e}"));
            assert!(d.all_items().len() > 1000, "{lang}");
        }
    }

    #[test]
    fn lookups() {
        let d = GameData::load("en").unwrap();
        assert!(d.item_by_ref(Namespace::Item, "Chaos Orb").is_some());
        let (stat, _) = d.stat_by_match_str("# Added Passive Skills are Jewel Sockets").unwrap();
        assert_eq!(stat.ref_, "# Added Passive Skills are Jewel Sockets");
        assert!(d.client_strings.MAP_TIER.is_match(" (Tier 16)"));
        let caps = d.client_strings.MODIFIER_LINE.captures("Prefix Modifier \"Hale\" (Tier: 8)").unwrap();
        assert_eq!(caps.name("name").unwrap().as_str(), "Hale");
    }
}
