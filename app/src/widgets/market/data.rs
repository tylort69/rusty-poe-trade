//! UI-free market logic: economy rows built from the poe.ninja dense data, 7-day
//! change math, sorting, favorites, saved-search validation, poe.ninja exchange
//! history parsing, local daily snapshots (`history.jsonl`) and series merging.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use rpt_trade::prices::{NinjaDenseInfo, dense_info_to_details_id, display_rounding, selected_league_to_url};
use serde::{Deserialize, Serialize};

/// A daily data point: (days since the Unix epoch, UTC; chaos value).
pub type DayPoint = (i64, f64);

pub const SECS_PER_DAY: i64 = 86_400;

pub fn day_of(unix_secs: i64) -> i64 {
    unix_secs.div_euclid(SECS_PER_DAY)
}

pub fn today() -> i64 {
    day_of(chrono::Utc::now().timestamp())
}

pub fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---------------------------------------------------------------------------
// Item identity

/// Identifies a poe.ninja line: overview type (e.g. `"Currency"`), name and variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemKey {
    #[serde(rename = "type")]
    pub type_: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

impl ItemKey {
    pub fn new(type_: &str, info: &NinjaDenseInfo) -> Self {
        Self { type_: type_.to_owned(), name: info.name.clone(), variant: info.variant.clone().filter(|v| !v.is_empty()) }
    }

    pub fn label(&self) -> String {
        match &self.variant {
            Some(v) => format!("{} ({v})", self.name),
            None => self.name.clone(),
        }
    }

    /// `BaseType.namespace` used by `PriceIndex::find_price_by_query`.
    pub fn ns(&self) -> &'static str {
        type_ns(&self.type_)
    }

    /// poe.ninja details id (`divine-orb`), used by pages and the exchange history API.
    pub fn details_id(&self) -> String {
        dense_info_to_details_id(&NinjaDenseInfo { chaos: 0.0, graph: vec![], name: self.name.clone(), variant: self.variant.clone() })
    }

    pub fn ninja_url(&self, league: &str, type_url: &str) -> String {
        format!("https://poe.ninja/poe1/economy/{}/{}/{}", selected_league_to_url(league), type_url, self.details_id())
    }
}

/// Mirrors `NAMESPACE_MAP` in `rpt_trade::prices`.
pub fn type_ns(type_: &str) -> &'static str {
    match type_ {
        "DivinationCard" => "DIVINATION_CARD",
        "Beast" => "CAPTURED_BEAST",
        "SkillGem" => "GEM",
        t if t.starts_with("Unique") => "UNIQUE",
        _ => "ITEM",
    }
}

/// "DivinationCard" → "Divination Card".
pub fn category_label(type_: &str) -> String {
    let mut out = String::new();
    for (i, ch) in type_.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------
// 7-day graph math (poe.ninja dense `graph`: % change vs. the first non-null day)

/// Change in percent over the sparkline window (first → last known point).
pub fn change_7d(graph: &[Option<f64>]) -> Option<f64> {
    let known: Vec<(usize, f64)> = graph.iter().enumerate().filter_map(|(i, v)| v.map(|v| (i, v))).collect();
    if known.len() < 2 {
        return None;
    }
    let (first, last) = (known[0].1, known[known.len() - 1].1);
    let base = 1.0 + first / 100.0;
    if base <= 0.0 {
        return None;
    }
    Some(((1.0 + last / 100.0) / base - 1.0) * 100.0)
}

/// Absolute daily chaos prices from the relative sparkline; the last graph slot is `today`.
pub fn prices_from_graph(chaos: f64, graph: &[Option<f64>], today: i64) -> Vec<DayPoint> {
    let Some(last) = graph.iter().rev().flatten().next().copied() else { return vec![] };
    let last_f = 1.0 + last / 100.0;
    if last_f <= 0.0 {
        return vec![];
    }
    let n = graph.len() as i64;
    graph
        .iter()
        .enumerate()
        .filter_map(|(i, g)| g.map(|g| (today - (n - 1 - i as i64), chaos * (1.0 + g / 100.0) / last_f)))
        .collect()
}

// ---------------------------------------------------------------------------
// Economy rows

#[derive(Debug, Clone, PartialEq)]
pub struct EconRow {
    pub info: NinjaDenseInfo,
    pub change: Option<f64>,
    /// lowercase "name variant" for filtering
    pub search: String,
}

impl EconRow {
    pub fn new(info: NinjaDenseInfo) -> Self {
        let change = change_7d(&info.graph);
        let search = match &info.variant {
            Some(v) => format!("{} {}", info.name, v).to_lowercase(),
            None => info.name.to_lowercase(),
        };
        Self { info, change, search }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Price,
    Change,
    /// Biggest absolute change first.
    Movers,
}

/// Sort row indices. Descending puts the largest value first; rows without a
/// change always go last when sorting by change.
pub fn sort_rows(rows: &[EconRow], idx: &mut [usize], by: SortBy, descending: bool) {
    let key = |r: &EconRow| -> Option<f64> {
        match by {
            SortBy::Price => Some(r.info.chaos),
            SortBy::Change => r.change,
            SortBy::Movers => r.change.map(f64::abs),
        }
    };
    idx.sort_by(|&a, &b| match (key(&rows[a]), key(&rows[b])) {
        (Some(x), Some(y)) => {
            let o = x.total_cmp(&y);
            if descending { o.reverse() } else { o }
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
}

/// Search like `item_search::find_names` (every word contained, some word of the
/// name starts with the longest query word) but without a result cap.
pub fn search_matcher(query: &str) -> impl Fn(&str) -> bool {
    let lc = query.trim().to_lowercase();
    let mut words: Vec<String> = lc.split_whitespace().map(str::to_owned).collect();
    words.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    move |hay: &str| {
        if words.is_empty() {
            return true;
        }
        words.iter().all(|w| hay.contains(w.as_str())) && hay.split(|c: char| c.is_whitespace() || c == '(' || c == ',').any(|p| p.starts_with(words[0].as_str()))
    }
}

pub fn filter_sort(rows: &[EconRow], query: &str, by: SortBy, descending: bool) -> Vec<usize> {
    let m = search_matcher(query);
    let mut idx: Vec<usize> = (0..rows.len()).filter(|&i| m(&rows[i].search)).collect();
    sort_rows(rows, &mut idx, by, descending);
    idx
}

// ---------------------------------------------------------------------------
// Price formatting

fn group_thousands(v: f64) -> String {
    let s = (v.abs().round() as u64).to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if v < 0.0 { format!("-{out}") } else { out }
}

pub fn fmt_chaos(v: f64) -> String {
    if v.abs() >= 1000.0 { group_thousands(v) } else { display_rounding(v, true) }
}

/// Divine amount when the value is worth at least one divine.
pub fn divine_text(chaos: f64, xchg: Option<f64>) -> Option<String> {
    let rate = xchg.filter(|r| *r > 0.0)?;
    (chaos >= rate).then(|| display_rounding(chaos / rate, false))
}

pub fn fmt_pct(v: f64) -> String {
    if v.abs() >= 1000.0 { format!("{:+.0}%", v) } else { format!("{:+.1}%", v) }
}

// ---------------------------------------------------------------------------
// Favorites

/// A watched item. Favorites are stored **per league** (each entry carries its
/// league; the tab shows only the selected league's entries).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Favorite {
    pub league: String,
    #[serde(flatten)]
    pub key: ItemKey,
    /// chaos price when starred
    pub added_chaos: f64,
    /// unix seconds
    pub added_at: i64,
    /// highlight when the price moved more than this many percent since added
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert_pct: Option<f64>,
}

impl Favorite {
    pub fn change_since_added(&self, current: f64) -> Option<f64> {
        (self.added_chaos > 0.0).then(|| (current / self.added_chaos - 1.0) * 100.0)
    }

    pub fn alert(&self, current: f64) -> bool {
        match (self.alert_pct, self.change_since_added(current)) {
            (Some(t), Some(c)) => c.abs() >= t,
            _ => false,
        }
    }
}

pub fn is_favorite(favs: &[Favorite], league: &str, key: &ItemKey) -> bool {
    favs.iter().any(|f| f.league == league && &f.key == key)
}

/// Star / unstar. Returns true if the item is now a favorite.
pub fn toggle_favorite(favs: &mut Vec<Favorite>, league: &str, key: &ItemKey, chaos: f64, now: i64) -> bool {
    if let Some(i) = favs.iter().position(|f| f.league == league && &f.key == key) {
        favs.remove(i);
        false
    } else {
        favs.push(Favorite { league: league.to_owned(), key: key.clone(), added_chaos: chaos, added_at: now, alert_pct: None });
        true
    }
}

// ---------------------------------------------------------------------------
// Saved searches

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedSearch {
    pub name: String,
    pub url: String,
    pub notes: String,
}

/// Accepts `http(s)://[*.]pathofexile.com/trade[2]/(search|exchange)/<league>[/<id>]`.
/// Returns the trimmed URL.
pub fn validate_trade_url(url: &str) -> Result<String, &'static str> {
    let url = url.trim();
    let parsed = reqwest::Url::parse(url).map_err(|_| "Not a valid URL")?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return Err("URL must start with https://");
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    if host != "pathofexile.com" && !host.ends_with(".pathofexile.com") {
        return Err("Only pathofexile.com trade links can be saved");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Not a valid trade link");
    }
    let segs: Vec<&str> = parsed.path_segments().map(|s| s.filter(|p| !p.is_empty()).collect()).unwrap_or_default();
    let ok = segs.len() >= 3 && matches!(segs[0], "trade" | "trade2") && matches!(segs[1], "search" | "exchange");
    if !ok {
        return Err("Expected a /trade/search/… or /trade/exchange/… link");
    }
    Ok(url.to_owned())
}

/// Default name for a saved search: the query id (last path segment).
pub fn default_search_name(url: &str) -> String {
    url.trim_end_matches('/').rsplit('/').next().unwrap_or("Search").to_owned()
}

/// Add (or rename, if the URL is already saved) a search. Returns the index.
pub fn upsert_search(list: &mut Vec<SavedSearch>, name: &str, url: &str, notes: &str) -> Result<usize, &'static str> {
    let url = validate_trade_url(url)?;
    let name = if name.trim().is_empty() { default_search_name(&url) } else { name.trim().to_owned() };
    if let Some(i) = list.iter().position(|s| s.url == url) {
        list[i].name = name;
        if !notes.is_empty() {
            list[i].notes = notes.to_owned();
        }
        return Ok(i);
    }
    list.push(SavedSearch { name, url, notes: notes.to_owned() });
    Ok(list.len() - 1)
}

/// Move an element one step up (`-1`) or down (`+1`). Returns the new index.
pub fn move_item<T>(list: &mut [T], i: usize, dir: i32) -> usize {
    let j = i as i64 + dir as i64;
    if i >= list.len() || j < 0 || j as usize >= list.len() {
        return i;
    }
    list.swap(i, j as usize);
    j as usize
}

// ---------------------------------------------------------------------------
// poe.ninja exchange history (`/poe1/api/economy/exchange/current/details`)

pub const EXCHANGE_DETAILS_URL: &str = "https://poe.ninja/poe1/api/economy/exchange/current/details";

/// Only overviews with `source: "cx"` (currency exchange) have this endpoint.
pub fn has_exchange_history(source: Option<&str>) -> bool {
    source == Some("cx")
}

pub fn parse_exchange_history(json: &str) -> Result<Vec<DayPoint>, String> {
    #[derive(Deserialize)]
    struct Details {
        #[serde(default)]
        pairs: Vec<Pair>,
    }
    #[derive(Deserialize)]
    struct Pair {
        id: String,
        #[serde(default)]
        history: Vec<Point>,
    }
    #[derive(Deserialize)]
    struct Point {
        timestamp: String,
        rate: f64,
    }
    let d: Details = serde_json::from_str(json).map_err(|e| format!("poe.ninja history: {e}"))?;
    let Some(pair) = d.pairs.into_iter().find(|p| p.id == "chaos") else { return Ok(vec![]) };
    let mut out: Vec<DayPoint> = pair
        .history
        .into_iter()
        .filter_map(|p| {
            let t = chrono::DateTime::parse_from_rfc3339(&p.timestamp).ok()?;
            (p.rate.is_finite() && p.rate > 0.0).then(|| (day_of(t.timestamp()), p.rate))
        })
        .collect();
    out.sort_by_key(|p| p.0);
    out.dedup_by_key(|p| p.0);
    Ok(out)
}

pub async fn fetch_exchange_history(http: reqwest::Client, league: String, type_: String, details_id: String) -> Result<Vec<DayPoint>, String> {
    let resp = http
        .get(EXCHANGE_DETAILS_URL)
        .query(&[("league", league.as_str()), ("type", type_.as_str()), ("id", details_id.as_str())])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("poe.ninja returned {}", status.as_u16()));
    }
    parse_exchange_history(&text)
}

/// Merge daily sources; earlier sources win on the same day. Sorted by day.
pub fn merge_daily(sources: &[&[DayPoint]]) -> Vec<DayPoint> {
    let mut out: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
    for src in sources {
        for &(d, v) in src.iter() {
            out.entry(d).or_insert(v);
        }
    }
    out.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Local snapshots (`data_dir()/market/history.jsonl`)

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub day: i64,
    pub league: String,
    #[serde(flatten)]
    pub key: ItemKey,
    pub chaos: f64,
}

/// Append-only JSON-lines store with at most one record per (day, league, item).
#[derive(Debug, Default)]
pub struct HistoryStore {
    pub path: Option<PathBuf>,
    pub records: Vec<Snapshot>,
    loaded: bool,
}

impl HistoryStore {
    pub fn default_path() -> PathBuf {
        crate::store::data_dir().join("market").join("history.jsonl")
    }

    #[cfg(test)]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path: Some(path), ..Default::default() }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Read the file (once). Malformed lines are skipped.
    pub fn load(&mut self) {
        self.loaded = true;
        let Some(path) = &self.path else { return };
        let Ok(text) = std::fs::read_to_string(path) else { return };
        self.records = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    }

    fn has(&self, s: &Snapshot) -> bool {
        self.records.iter().any(|r| r.day == s.day && r.league == s.league && r.key == s.key)
    }

    /// Append the snapshots not recorded yet. Returns how many were written.
    pub fn record(&mut self, snaps: Vec<Snapshot>) -> std::io::Result<usize> {
        if !self.loaded {
            self.load();
        }
        let new: Vec<Snapshot> = snaps.into_iter().filter(|s| s.chaos.is_finite() && s.chaos > 0.0).filter(|s| !self.has(s)).collect();
        if new.is_empty() {
            return Ok(0);
        }
        if let Some(path) = &self.path {
            append_lines(path, &new)?;
        }
        let n = new.len();
        self.records.extend(new);
        Ok(n)
    }

    pub fn series(&self, league: &str, key: &ItemKey) -> Vec<DayPoint> {
        let mut v: Vec<DayPoint> = self.records.iter().filter(|r| r.league == league && &r.key == key).map(|r| (r.day, r.chaos)).collect();
        v.sort_by_key(|p| p.0);
        v
    }
}

fn append_lines(path: &Path, snaps: &[Snapshot]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let mut buf = String::new();
    for s in snaps {
        buf.push_str(&serde_json::to_string(s).map_err(std::io::Error::other)?);
        buf.push('\n');
    }
    f.write_all(buf.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rpt_trade::prices::parse_overview_chunk;

    const DENSE: &str = include_str!("fixtures/dense_overviews.json");
    const DETAILS: &str = include_str!("fixtures/exchange_details_divine.json");

    fn chunk(type_: &str) -> &'static str {
        let start = DENSE.find(&format!("{{\"type\":\"{type_}\"")).unwrap();
        &DENSE[start..]
    }

    #[test]
    fn parses_dense_fixture() {
        let o = parse_overview_chunk(chunk("Oil"), "Oil", "oils");
        assert_eq!(o.source.as_deref(), Some("cx"));
        assert_eq!(o.lines.len(), 3);
        let golden = o.lines.iter().find(|l| l.name == "Golden Oil").unwrap();
        assert_eq!(golden.chaos, 65.37);
        let flasks = parse_overview_chunk(chunk("UniqueFlask"), "UniqueFlask", "unique-flasks");
        assert_eq!(flasks.source.as_deref(), Some("s"));
        assert_eq!(flasks.lines[1].graph[0], None);
        assert!(!has_exchange_history(flasks.source.as_deref()));
        // change math on real data: Golden Oil graph ends at +117.9 %
        let ch = change_7d(&golden.graph).unwrap();
        assert!((ch - 117.9).abs() < 1e-9);
        // Soul Catcher: first known is 0 at idx 2, last known 118.5
        assert!((change_7d(&flasks.lines[1].graph).unwrap() - 118.5).abs() < 1e-9);
        let pts = prices_from_graph(golden.chaos, &golden.graph, 100);
        assert_eq!(pts.len(), 7);
        assert_eq!(pts[0].0, 94);
        assert_eq!(pts[6], (100, 65.37));
        assert!((pts[0].1 - 65.37 / 2.179).abs() < 1e-9);
    }

    #[test]
    fn parses_exchange_details_fixture() {
        let h = parse_exchange_history(DETAILS).unwrap();
        assert_eq!(h.len(), 7);
        assert!(h.windows(2).all(|w| w[0].0 + 1 == w[1].0));
        assert_eq!(h.last().unwrap().1, 215.4);
        assert_eq!(h[0].1, 66.68);
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
        assert_eq!(h.last().unwrap().0, day_of(d));
        assert!(parse_exchange_history("{\"pairs\":[]}").unwrap().is_empty());
        assert!(parse_exchange_history("nope").is_err());
    }

    #[test]
    fn graph_edge_cases() {
        assert_eq!(change_7d(&[]), None);
        assert_eq!(change_7d(&[None, Some(5.0), None]), None);
        assert!((change_7d(&[Some(0.0), Some(-50.0)]).unwrap() + 50.0).abs() < 1e-9);
        // relative to a non-zero first point
        assert!((change_7d(&[Some(100.0), Some(300.0)]).unwrap() - 100.0).abs() < 1e-9);
        assert!(prices_from_graph(1.0, &[None, None], 0).is_empty());
    }

    fn row(name: &str, chaos: f64, graph: Vec<Option<f64>>) -> EconRow {
        EconRow::new(NinjaDenseInfo { chaos, graph, name: name.into(), variant: None })
    }

    #[test]
    fn mover_sorting() {
        let rows = vec![
            row("Up Small", 10.0, vec![Some(0.0), Some(5.0)]),
            row("Down Big", 50.0, vec![Some(0.0), Some(-60.0)]),
            row("No Data", 100.0, vec![]),
            row("Up Big", 1.0, vec![Some(0.0), Some(40.0)]),
        ];
        let names = |idx: &[usize]| idx.iter().map(|&i| rows[i].info.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names(&filter_sort(&rows, "", SortBy::Movers, true)), ["Down Big", "Up Big", "Up Small", "No Data"]);
        assert_eq!(names(&filter_sort(&rows, "", SortBy::Change, true)), ["Up Big", "Up Small", "Down Big", "No Data"]);
        assert_eq!(names(&filter_sort(&rows, "", SortBy::Change, false)), ["Down Big", "Up Small", "Up Big", "No Data"]);
        assert_eq!(names(&filter_sort(&rows, "", SortBy::Price, true)), ["No Data", "Down Big", "Up Small", "Up Big"]);
        assert_eq!(names(&filter_sort(&rows, "big", SortBy::Price, true)), ["Down Big", "Up Big"]);
        assert_eq!(names(&filter_sort(&rows, "ig", SortBy::Price, true)), Vec::<&str>::new());
    }

    #[test]
    fn favorites_round_trip_through_config() {
        use crate::config::{Config, WidgetKind};
        let mut cfg = Config::default();
        let key = ItemKey { type_: "Currency".into(), name: "Divine Orb".into(), variant: None };
        let mw = super::super::market_widget_mut(&mut cfg).unwrap();
        assert!(toggle_favorite(&mut mw.favorites, "Covens", &key, 215.4, 1_700_000_000));
        mw.favorites[0].alert_pct = Some(10.0);
        let k2 = ItemKey { type_: "UniqueArmour".into(), name: "Atziri's Splendour".into(), variant: Some("Armour/ES".into()) };
        toggle_favorite(&mut mw.favorites, "Standard", &k2, 3.0, 1);
        super::super::save_search(&mut cfg, "Mirrors", "https://www.pathofexile.com/trade/search/Covens/abc123").unwrap();
        let json = cfg.to_json();
        assert!(json.contains("\"wmType\": \"rpt-market\"") || json.contains("\"wmType\":\"rpt-market\""));
        let mut back = Config::from_json(&json).unwrap();
        let mw2 = back.widgets.iter().find_map(|w| match &w.kind {
            WidgetKind::Market(m) => Some(m.clone()),
            _ => None,
        });
        let mw2 = mw2.unwrap();
        assert_eq!(mw2.favorites.len(), 2);
        assert_eq!(mw2.favorites[0].key, key);
        assert_eq!(mw2.favorites[0].added_chaos, 215.4);
        assert_eq!(mw2.favorites[0].alert_pct, Some(10.0));
        assert_eq!(mw2.favorites[1].key.variant.as_deref(), Some("Armour/ES"));
        assert_eq!(mw2.saved_searches[0].name, "Mirrors");
        assert!(is_favorite(&mw2.favorites, "Covens", &key));
        assert!(!is_favorite(&mw2.favorites, "Standard", &key));
        // unstar
        let mw3 = super::super::market_widget_mut(&mut back).unwrap();
        assert!(!toggle_favorite(&mut mw3.favorites, "Covens", &key, 0.0, 0));
        assert_eq!(mw3.favorites.len(), 1);
    }

    #[test]
    fn favorite_alerts() {
        let mut f = Favorite { league: "L".into(), key: ItemKey { type_: "Currency".into(), name: "X".into(), variant: None }, added_chaos: 100.0, added_at: 0, alert_pct: None };
        assert!((f.change_since_added(125.0).unwrap() - 25.0).abs() < 1e-9);
        assert!(!f.alert(200.0));
        f.alert_pct = Some(20.0);
        assert!(f.alert(125.0));
        assert!(f.alert(79.0));
        assert!(!f.alert(110.0));
    }

    #[test]
    fn saved_search_validation() {
        assert!(validate_trade_url("https://www.pathofexile.com/trade/search/Covens/Ab12cD").is_ok());
        assert!(validate_trade_url("  https://pathofexile.com/trade/exchange/Standard/xyz  ").is_ok());
        assert!(validate_trade_url("https://www.pathofexile.com/trade2/search/poe2/Standard/xyz").is_ok());
        assert!(validate_trade_url("https://ru.pathofexile.com/trade/search/Covens").is_ok());
        assert!(validate_trade_url("https://evilpathofexile.com/trade/search/Covens/a").is_err());
        assert!(validate_trade_url("https://www.pathofexile.com.evil.io/trade/search/Covens/a").is_err());
        assert!(validate_trade_url("https://www.pathofexile.com@evil.io/trade/search/Covens/a").is_err());
        assert!(validate_trade_url("https://www.pathofexile.com/forum/view-thread/1").is_err());
        assert!(validate_trade_url("ftp://www.pathofexile.com/trade/search/Covens/a").is_err());
        assert!(validate_trade_url("pathofexile.com/trade/search/Covens/a").is_err());
        assert!(validate_trade_url("").is_err());

        let mut list = Vec::new();
        assert_eq!(upsert_search(&mut list, "", "https://www.pathofexile.com/trade/search/Covens/Ab12cD", ""), Ok(0));
        assert_eq!(list[0].name, "Ab12cD");
        assert_eq!(upsert_search(&mut list, "Renamed", "https://www.pathofexile.com/trade/search/Covens/Ab12cD", "n"), Ok(0));
        assert_eq!((list.len(), list[0].name.as_str(), list[0].notes.as_str()), (1, "Renamed", "n"));
        assert!(upsert_search(&mut list, "x", "https://example.com/trade/search/a/b", "").is_err());
        upsert_search(&mut list, "B", "https://www.pathofexile.com/trade/search/Covens/B", "").unwrap();
        assert_eq!(move_item(&mut list, 1, -1), 0);
        assert_eq!(list[0].name, "B");
        assert_eq!(move_item(&mut list, 0, -1), 0);
        assert_eq!(move_item(&mut list, 1, 1), 1);
    }

    #[test]
    fn history_file_round_trip() {
        let dir = std::env::temp_dir().join(format!("rpt-market-test-{}-{}", std::process::id(), now_secs()));
        let path = dir.join("market").join("history.jsonl");
        let key = ItemKey { type_: "Oil".into(), name: "Golden Oil".into(), variant: None };
        let snap = |day, chaos| Snapshot { day, league: "Covens".into(), key: key.clone(), chaos };
        {
            let mut h = HistoryStore::with_path(path.clone());
            assert_eq!(h.record(vec![snap(10, 30.0), snap(11, 33.0)]).unwrap(), 2);
            // same day again → ignored; zero price → ignored
            assert_eq!(h.record(vec![snap(11, 99.0), snap(12, 0.0)]).unwrap(), 0);
        }
        // a corrupt line is skipped
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"garbage\n").unwrap();
        let mut h = HistoryStore::with_path(path.clone());
        h.load();
        assert_eq!(h.series("Covens", &key), vec![(10, 30.0), (11, 33.0)]);
        assert!(h.series("Standard", &key).is_empty());
        assert_eq!(h.record(vec![snap(12, 40.0)]).unwrap(), 1);
        let mut h2 = HistoryStore::with_path(path.clone());
        h2.load();
        assert_eq!(h2.series("Covens", &key).len(), 3);
        let line = std::fs::read_to_string(&path).unwrap().lines().next().unwrap().to_owned();
        assert!(line.contains("\"type\":\"Oil\"") && line.contains("\"day\":10"), "{line}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merging_sources() {
        let ninja = [(5, 50.0), (6, 60.0)];
        let local = [(1, 10.0), (6, 66.0)];
        let spark = [(6, 61.0), (7, 70.0)];
        assert_eq!(merge_daily(&[&ninja, &local, &spark]), vec![(1, 10.0), (5, 50.0), (6, 60.0), (7, 70.0)]);
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_chaos(12345.4), "12,345");
        assert_eq!(divine_text(430.0, Some(215.0)).as_deref(), Some("2"));
        assert_eq!(divine_text(100.0, Some(215.0)), None);
        assert_eq!(fmt_pct(12.345), "+12.3%");
        assert_eq!(fmt_pct(-5.0), "-5.0%");
        assert_eq!(category_label("DivinationCard"), "Divination Card");
        assert_eq!(type_ns("UniqueFlask"), "UNIQUE");
        let k = ItemKey { type_: "Currency".into(), name: "Divine Orb".into(), variant: None };
        assert_eq!(k.ninja_url("Covens", "currency"), "https://poe.ninja/poe1/economy/covens/currency/divine-orb");
    }
}
