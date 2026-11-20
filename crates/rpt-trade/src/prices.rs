//! Port of upstream `web/background/Prices.ts` (poe.ninja price index).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::client::Realm;
use crate::leagues::Leagues;
use crate::trends::DetailsId;
use crate::{Result, TradeClient, TradeError};

pub const RETRY_INTERVAL: Duration = Duration::from_secs(4 * 60);
const UPDATE_INTERVAL: Duration = Duration::from_secs(31 * 60);
const INTEREST_SPAN: Duration = Duration::from_secs(20 * 60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NinjaDenseInfo {
    pub chaos: f64,
    #[serde(default)]
    pub graph: Vec<Option<f64>>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// A price entry found in the index (`NinjaDenseInfo & { url }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NinjaPrice {
    pub chaos: f64,
    pub graph: Vec<Option<f64>>,
    pub name: String,
    pub variant: Option<String>,
    /// poe.ninja details page
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DbQuery {
    /// `BaseType.namespace` string, e.g. "ITEM", "UNIQUE", "GEM"
    pub ns: String,
    pub name: String,
    pub variant: Option<String>,
}

impl From<DetailsId> for DbQuery {
    fn from(d: DetailsId) -> Self {
        DbQuery { ns: d.ns, name: d.name, variant: d.variant }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PriceCurrency {
    Chaos,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurrencyValue {
    pub min: f64,
    pub max: f64,
    pub currency: PriceCurrency,
}

#[derive(Debug, Clone)]
struct PriceDbEntry {
    ns: &'static str,
    url: &'static str,
    lines: String,
}

#[derive(Debug, Default)]
struct State {
    db: Vec<PriceDbEntry>,
    xchg_rate: Option<f64>,
    last_update: Option<Instant>,
    last_interest: Option<Instant>,
    is_loading: bool,
}

/// App-wide poe.ninja price cache (upstream `usePoeninja`). Hold it in an `Arc`.
pub struct PriceIndex {
    client: Arc<TradeClient>,
    leagues: Arc<Leagues>,
    state: RwLock<State>,
    /// increments on every load start; older loads discard their result (upstream AbortController)
    generation: AtomicU64,
}

impl PriceIndex {
    pub fn new(client: Arc<TradeClient>, leagues: Arc<Leagues>) -> Self {
        Self { client, leagues, state: RwLock::new(State::default()), generation: AtomicU64::new(0) }
    }

    pub fn xchg_rate(&self) -> Option<f64> {
        self.state.read().xchg_rate
    }

    /// Upstream `initialLoading()`.
    pub fn initial_loading(&self) -> bool {
        let s = self.state.read();
        s.is_loading && s.db.is_empty()
    }

    /// Upstream `load(force)`.
    pub async fn load(&self, force: bool) -> Result<()> {
        let Some(league) = self.leagues.selected() else { return Ok(()) };
        if !league.is_popular || league.realm != Realm::PcGgg {
            return Ok(());
        }

        {
            let s = self.state.read();
            let since = |t: Option<Instant>| t.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
            if !force && (since(s.last_update) < UPDATE_INTERVAL || since(s.last_interest) > INTEREST_SPAN) {
                return Ok(());
            }
        }

        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.state.write().is_loading = true;
        let result = self.download(&league.id).await;
        let mut s = self.state.write();
        if self.generation.load(Ordering::SeqCst) != generation {
            // a newer load started (upstream aborted this one)
            return Ok(());
        }
        s.is_loading = false;
        let blob = result?;
        s.db = split_json_blob(&blob);
        if let Some(divine) = find_in_db(&s.db, &DbQuery { ns: "ITEM".into(), name: "Divine Orb".into(), variant: None }) {
            if divine.info.chaos >= 30.0 {
                s.xchg_rate = Some(divine.info.chaos);
            }
        }
        s.last_update = Some(Instant::now());
        Ok(())
    }

    async fn download(&self, league_id: &str) -> Result<String> {
        let resp = self
            .client
            .http()
            .get("https://poe.ninja/poe1/api/economy/current/dense/overviews")
            .query(&[("league", league_id), ("language", "en")])
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(TradeError::Status { status: status.as_u16(), body: text.chars().take(300).collect() });
        }
        Ok(text)
    }

    /// Upstream `queuePricesFetch`: mark interest and load if stale.
    pub async fn queue_prices_fetch(&self) -> Result<()> {
        self.state.write().last_interest = Some(Instant::now());
        self.load(false).await
    }

    /// Call when the selected league changes (upstream `watch(leagues.selectedId)`).
    pub async fn on_league_changed(&self) -> Result<()> {
        {
            let mut s = self.state.write();
            s.xchg_rate = None;
            s.db.clear();
        }
        self.load(true).await
    }

    /// Background refresh every 4 minutes (upstream `setInterval`). Requires a tokio runtime.
    pub fn spawn_refresh_loop(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let this = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(RETRY_INTERVAL);
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some(this) = this.upgrade() else { break };
                if let Err(e) = this.load(false).await {
                    tracing::warn!("poe.ninja refresh failed: {e}");
                }
            }
        })
    }

    /// Replace the index with a downloaded blob (for tests / offline use).
    pub fn load_from_blob(&self, blob: &str) {
        let mut s = self.state.write();
        s.db = split_json_blob(blob);
        if let Some(divine) = find_in_db(&s.db, &DbQuery { ns: "ITEM".into(), name: "Divine Orb".into(), variant: None }) {
            if divine.info.chaos >= 30.0 {
                s.xchg_rate = Some(divine.info.chaos);
            }
        }
        s.last_update = Some(Instant::now());
    }

    /// Upstream `findPriceByQuery`.
    pub fn find_price_by_query(&self, query: &DbQuery) -> Option<NinjaPrice> {
        let info = {
            let s = self.state.read();
            find_in_db(&s.db, query)
        }?;
        let league_url = selected_league_to_url(&self.leagues.selected_id().unwrap_or_default());
        Some(NinjaPrice {
            url: format!(
                "https://poe.ninja/poe1/economy/{}/{}/{}",
                league_url,
                info.url,
                dense_info_to_details_id(&info.info)
            ),
            chaos: info.info.chaos,
            graph: info.info.graph,
            name: info.info.name,
            variant: info.info.variant,
        })
    }

    /// Upstream `autoCurrency(value)`.
    pub fn auto_currency(&self, value: f64) -> CurrencyValue {
        auto_currency(self.xchg_rate(), value)
    }

    /// Upstream `autoCurrency([min, max])`.
    pub fn auto_currency_range(&self, min: f64, max: f64) -> CurrencyValue {
        auto_currency_range(self.xchg_rate(), min, max)
    }

    pub fn chaos_to_stable(&self, count: f64) -> f64 {
        chaos_to_stable(self.xchg_rate(), count)
    }
}

pub fn chaos_to_stable(xchg_rate: Option<f64>, count: f64) -> f64 {
    count / xchg_rate.filter(|r| *r != 0.0).unwrap_or(9999.0)
}

pub fn auto_currency(xchg_rate: Option<f64>, value: f64) -> CurrencyValue {
    let rate = xchg_rate.filter(|r| *r != 0.0).unwrap_or(9999.0);
    if value > rate * 0.94 {
        if value < rate * 1.06 {
            return CurrencyValue { min: 1.0, max: 1.0, currency: PriceCurrency::Div };
        }
        let v = chaos_to_stable(xchg_rate, value);
        return CurrencyValue { min: v, max: v, currency: PriceCurrency::Div };
    }
    CurrencyValue { min: value, max: value, currency: PriceCurrency::Chaos }
}

pub fn auto_currency_range(xchg_rate: Option<f64>, min: f64, max: f64) -> CurrencyValue {
    let rate = xchg_rate.filter(|r| *r != 0.0).unwrap_or(9999.0);
    if max > rate {
        return CurrencyValue {
            min: chaos_to_stable(xchg_rate, min),
            max: chaos_to_stable(xchg_rate, max),
            currency: PriceCurrency::Div,
        };
    }
    CurrencyValue { min, max, currency: PriceCurrency::Chaos }
}

/// Upstream `selectedLeagueToUrl`.
pub fn selected_league_to_url(league: &str) -> String {
    match league {
        "Standard" => "standard".to_owned(),
        "Hardcore" => "hardcore".to_owned(),
        _ => {
            let mut ninja_id = league.replacen("Hardcore ", "", 1).to_lowercase();
            if league.starts_with("Hardcore ") {
                ninja_id.push_str("hc");
            }
            ninja_id
        }
    }
}

struct FoundInfo {
    info: NinjaDenseInfo,
    url: &'static str,
}

fn find_in_db(db: &[PriceDbEntry], query: &DbQuery) -> Option<FoundInfo> {
    // NOTE: order of keys is important
    let mut search = String::from("{\"name\":");
    search.push_str(&serde_json::to_string(&query.name).ok()?);
    if let Some(v) = &query.variant {
        search.push_str(",\"variant\":");
        search.push_str(&serde_json::to_string(v).ok()?);
    }
    search.push_str(",\"chaos\":");

    for entry in db.iter().filter(|e| e.ns == query.ns) {
        let Some(start) = entry.lines.find(&search) else { continue };
        let Some(end_rel) = entry.lines[start..].find('}') else { continue };
        let slice = &entry.lines[start..start + end_rel + 1];
        match serde_json::from_str::<NinjaDenseInfo>(slice) {
            Ok(info) => return Some(FoundInfo { info, url: entry.url }),
            Err(e) => {
                tracing::warn!("poe.ninja entry parse error: {e}");
                return None;
            }
        }
    }
    None
}

/// Upstream `denseInfoToDetailsId`.
pub fn dense_info_to_details_id(info: &NinjaDenseInfo) -> String {
    let s = match &info.variant {
        Some(v) if !v.is_empty() => format!("{}, {}", info.name, v),
        _ => info.name.clone(),
    };
    s.nfkd()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | ' '))
        .collect::<String>()
        .to_lowercase()
        .replace(' ', "-")
}

const NAMESPACE_MAP: &[(&str, &str, &str)] = &[
    ("ITEM", "currency", "Currency"),
    ("ITEM", "fragments", "Fragment"),
    ("ITEM", "delirium-orbs", "DeliriumOrb"),
    ("ITEM", "artifacts", "Artifact"),
    ("ITEM", "base-types", "BaseType"),
    ("ITEM", "fossils", "Fossil"),
    ("ITEM", "resonators", "Resonator"),
    ("ITEM", "incubators", "Incubator"),
    ("ITEM", "oils", "Oil"),
    ("ITEM", "vials", "Vial"),
    ("ITEM", "invitations", "Invitation"),
    ("ITEM", "blighted-maps", "BlightedMap"),
    ("ITEM", "blight-ravaged-maps", "BlightRavagedMap"),
    ("ITEM", "essences", "Essence"),
    ("ITEM", "maps", "Map"),
    ("ITEM", "tattoos", "Tattoo"),
    ("ITEM", "omens", "Omen"),
    ("ITEM", "coffins", "Coffin"),
    ("ITEM", "allflame-embers", "AllflameEmber"),
    ("ITEM", "djinn-coins", "DjinnCoin"),
    ("ITEM", "astrolabes", "Astrolabe"),
    ("ITEM", "runegrafts", "Runegraft"),
    ("ITEM", "ducats", "Ducat"),
    ("ITEM", "enshrouding-crystals", "EnshroudingCrystal"),
    ("ITEM", "corpses", "Corpse"),
    ("ITEM", "threads", "Thread"),
    ("DIVINATION_CARD", "divination-cards", "DivinationCard"),
    ("CAPTURED_BEAST", "beasts", "Beast"),
    ("UNIQUE", "unique-jewels", "UniqueJewel"),
    ("UNIQUE", "unique-flasks", "UniqueFlask"),
    ("UNIQUE", "unique-weapons", "UniqueWeapon"),
    ("UNIQUE", "unique-armours", "UniqueArmour"),
    ("UNIQUE", "unique-accessories", "UniqueAccessory"),
    ("UNIQUE", "unique-maps", "UniqueMap"),
    ("UNIQUE", "unique-relics", "UniqueRelic"),
    ("UNIQUE", "unique-tinctures", "UniqueTincture"),
    ("GEM", "skill-gems", "SkillGem"),
];

/// Upstream `splitJsonBlob`.
fn split_json_blob(blob: &str) -> Vec<PriceDbEntry> {
    const NINJA_OVERVIEW: &str = "{\"type\":\"";
    let mut database = Vec::new();
    let Some(mut start) = blob.find(NINJA_OVERVIEW) else { return database };

    loop {
        let end = blob[start + 1..].find(NINJA_OVERVIEW).map(|i| i + start + 1);
        let type_start = start + NINJA_OVERVIEW.len();
        let type_end = blob[type_start..].find('"').map(|i| i + type_start).unwrap_or(blob.len());
        let type_ = &blob[type_start..type_end];
        let lines = &blob[start..end.unwrap_or(blob.len())];

        if let Some((ns, url, _)) = NAMESPACE_MAP.iter().find(|(_, _, t)| *t == type_) {
            database.push(PriceDbEntry { ns, url, lines: lines.to_owned() });
        }

        match end {
            None => break,
            Some(e) => start = e,
        }
    }
    database
}

/// Upstream `displayRounding`.
pub fn display_rounding(value: f64, fraction: bool) -> String {
    if fraction && value.abs() < 1.0 {
        if value == 0.0 {
            return "0".to_owned();
        }
        let r = format!("1\u{200A}/\u{200A}{}", display_rounding(1.0 / value, false));
        return if r == "1\u{200A}/\u{200A}1" { "1".to_owned() } else { r };
    }
    if value.abs() < 10.0 {
        let fixed: f64 = crate::js::to_fixed(value, 1).parse().unwrap_or(value);
        return crate::js::num_to_string(fixed).replace('.', "\u{200A}.\u{200A}");
    }
    crate::js::num_to_string(crate::js::math_round(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TradeConfig;

    const BLOB: &str = r#"{"items":[{"type":"Currency","lines":[{"name":"Divine Orb","chaos":180.5,"graph":[0,1.5,null,-2]},{"name":"Exalted Orb","chaos":12,"graph":[]}]},{"type":"UniqueArmour","lines":[{"name":"Atziri's Splendour","variant":"Armour/ES/Life","chaos":3,"graph":[1,2,3,4,5,6,7]},{"name":"Kaom's Heart","chaos":50,"graph":[]}]},{"type":"Unknown","lines":[{"name":"Divine Orb","chaos":1}]}]}"#;

    fn index() -> PriceIndex {
        let client = Arc::new(TradeClient::new(TradeConfig::default()).unwrap());
        let leagues = Arc::new(Leagues::new(client.clone()));
        leagues.apply_api_leagues(vec![
            crate::leagues::ApiLeague { id: "Standard".into(), event: None, rules: vec![] },
            crate::leagues::ApiLeague { id: "Hardcore Mercenaries".into(), event: None, rules: vec![] },
        ]);
        let idx = PriceIndex::new(client, leagues);
        idx.load_from_blob(BLOB);
        idx
    }

    #[test]
    fn lookup() {
        let idx = index();
        assert_eq!(idx.xchg_rate(), Some(180.5));
        let p = idx
            .find_price_by_query(&DbQuery {
                ns: "UNIQUE".into(),
                name: "Atziri's Splendour".into(),
                variant: Some("Armour/ES/Life".into()),
            })
            .unwrap();
        assert_eq!(p.chaos, 3.0);
        assert_eq!(
            p.url,
            "https://poe.ninja/poe1/economy/mercenarieshc/unique-armours/atziris-splendour-armoureslife"
        );
        assert!(idx.find_price_by_query(&DbQuery { ns: "UNIQUE".into(), name: "Divine Orb".into(), variant: None }).is_none());
        let k = idx.find_price_by_query(&DbQuery { ns: "UNIQUE".into(), name: "Kaom's Heart".into(), variant: None }).unwrap();
        assert_eq!(k.chaos, 50.0);
    }

    #[test]
    fn currency_math() {
        let rate = Some(200.0);
        assert_eq!(auto_currency(rate, 100.0), CurrencyValue { min: 100.0, max: 100.0, currency: PriceCurrency::Chaos });
        assert_eq!(auto_currency(rate, 195.0), CurrencyValue { min: 1.0, max: 1.0, currency: PriceCurrency::Div });
        assert_eq!(auto_currency(rate, 400.0), CurrencyValue { min: 2.0, max: 2.0, currency: PriceCurrency::Div });
        assert_eq!(
            auto_currency_range(rate, 100.0, 300.0),
            CurrencyValue { min: 0.5, max: 1.5, currency: PriceCurrency::Div }
        );
        assert_eq!(auto_currency(None, 5000.0).currency, PriceCurrency::Chaos);
        assert_eq!(selected_league_to_url("Hardcore Mercenaries"), "mercenarieshc");
        assert_eq!(selected_league_to_url("Standard"), "standard");
    }

    #[test]
    fn rounding() {
        let hs = "\u{200A}";
        assert_eq!(display_rounding(1.25, false), format!("1{hs}.{hs}3"));
        assert_eq!(display_rounding(3.0, false), "3");
        assert_eq!(display_rounding(12.5, false), "13");
        assert_eq!(display_rounding(0.0, true), "0");
        assert_eq!(display_rounding(0.5, true), format!("1{hs}/{hs}2"));
        assert_eq!(display_rounding(0.96, true), "1");
        assert_eq!(display_rounding(0.04, false), "0");
    }
}
