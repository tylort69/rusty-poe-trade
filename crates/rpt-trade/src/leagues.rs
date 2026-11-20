//! Port of upstream `web/background/Leagues.ts`.
//!
//! The selected league id lives in [`TradeConfig::league_id`](crate::TradeConfig)
//! (upstream `AppConfig().leagueId`); this struct caches the trade league list.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::client::{Realm, TradeClient};

// pc-ggg, pc-garena
pub const PERMANENT_SC: &[&str] = &["Standard", "標準模式"];
const PERMANENT_HC: &[&str] = &["Hardcore", "專家模式"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiLeagueRule {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiLeague {
    pub id: String,
    #[serde(default)]
    pub event: Option<bool>,
    #[serde(default)]
    pub rules: Vec<ApiLeagueRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct League {
    pub id: String,
    pub is_popular: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedLeague {
    pub id: String,
    pub realm: Realm,
    pub is_popular: bool,
}

#[derive(Debug, Default)]
struct State {
    is_loading: bool,
    error: Option<String>,
    trade_leagues: Vec<League>,
}

/// App-wide league list cache (upstream `useLeagues`). Hold it in an `Arc`.
pub struct Leagues {
    client: Arc<TradeClient>,
    state: RwLock<State>,
}

impl Leagues {
    pub fn new(client: Arc<TradeClient>) -> Self {
        Self { client, state: RwLock::new(State::default()) }
    }

    pub fn is_loading(&self) -> bool {
        self.state.read().is_loading
    }

    pub fn error(&self) -> Option<String> {
        self.state.read().error.clone()
    }

    /// Upstream `list`.
    pub fn list(&self) -> Vec<League> {
        self.state.read().trade_leagues.clone()
    }

    /// Upstream `selectedId` getter: `None` until the league list is loaded.
    pub fn selected_id(&self) -> Option<String> {
        if self.state.read().trade_leagues.is_empty() {
            None
        } else {
            self.client.config().league_id
        }
    }

    /// Upstream `selectedId` setter (writes the app config league id).
    pub fn set_selected_id(&self, id: Option<String>) {
        self.client.update_config(|c| c.league_id = id);
    }

    /// Upstream `selected`.
    pub fn selected(&self) -> Option<SelectedLeague> {
        let cfg = self.client.config();
        let league_id = cfg.league_id?;
        let state = self.state.read();
        let listed = state.trade_leagues.iter().find(|l| l.id == league_id);
        Some(SelectedLeague {
            is_popular: !is_private_league(&league_id) && listed.is_some_and(|l| l.is_popular),
            id: league_id,
            realm: cfg.realm,
        })
    }

    /// Upstream `load`. Errors are stored in [`Leagues::error`] (like upstream)
    /// and also returned. Returns the selected league id after loading.
    pub async fn load(&self) -> Result<Option<String>, String> {
        {
            let mut s = self.state.write();
            s.is_loading = true;
            s.error = None;
        }
        let result = self.fetch().await;
        let out = match result {
            Ok(leagues) => {
                self.apply_api_leagues(leagues);
                Ok(self.selected_id())
            }
            Err(e) => {
                self.state.write().error = Some(e.clone());
                Err(e)
            }
        };
        self.state.write().is_loading = false;
        out
    }

    async fn fetch(&self) -> Result<Vec<ApiLeague>, String> {
        let url = format!("https://{}/api/leagues?type=main&realm=pc", self.client.trade_endpoint());
        let resp = self.client.http().get(url).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            let headers: serde_json::Map<String, serde_json::Value> = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_str().unwrap_or("").to_owned())))
                .collect();
            return Err(serde_json::Value::Object(headers).to_string());
        }
        resp.json::<Vec<ApiLeague>>().await.map_err(|e| e.to_string())
    }

    /// Applies an `/api/leagues` response (filtering + selected league fix-up).
    pub fn apply_api_leagues(&self, leagues: Vec<ApiLeague>) {
        let trade_leagues: Vec<League> = leagues
            .into_iter()
            .filter(|league| {
                !PERMANENT_HC.contains(&league.id.as_str())
                    && !league.rules.iter().any(|rule| {
                        rule.id == "NoParties" || (rule.id == "HardMode" && !league.event.unwrap_or(false))
                    })
            })
            .map(|league| League { id: league.id, is_popular: true })
            .collect();

        self.state.write().trade_leagues = trade_leagues.clone();

        let selected = self.selected_id();
        let league_is_alive = trade_leagues.iter().any(|l| Some(&l.id) == selected.as_ref());
        if !league_is_alive && !is_private_league(selected.as_deref().unwrap_or("")) {
            const TMP_CHALLENGE: usize = 1;
            const STANDARD: usize = 0;
            if trade_leagues.len() > 1 {
                self.set_selected_id(Some(trade_leagues[TMP_CHALLENGE].id.clone()));
            } else if let Some(l) = trade_leagues.get(STANDARD) {
                self.set_selected_id(Some(l.id.clone()));
            }
        }
    }
}

pub fn is_private_league(id: &str) -> bool {
    if id.contains("Ruthless") {
        return true;
    }
    // /\(PL\d+\)$/
    id.strip_suffix(')')
        .and_then(|s| s.rfind("(PL").map(|i| &s[i + 3..]))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TradeConfig;

    #[test]
    fn private_leagues() {
        assert!(is_private_league("My League (PL12345)"));
        assert!(is_private_league("Ruthless Settlers"));
        assert!(!is_private_league("Settlers"));
        assert!(!is_private_league("My League (PL)"));
    }

    #[test]
    fn apply_leagues() {
        let client = Arc::new(TradeClient::new(TradeConfig::default()).unwrap());
        let leagues = Leagues::new(client.clone());
        assert_eq!(leagues.selected_id(), None);
        let api: Vec<ApiLeague> = serde_json::from_value(serde_json::json!([
            { "id": "Standard", "rules": [] },
            { "id": "Hardcore", "rules": [{ "id": "Hardcore" }] },
            { "id": "Mercenaries", "rules": [] },
            { "id": "Hardcore Mercenaries", "rules": [{ "id": "Hardcore" }] },
            { "id": "SSF Mercenaries", "rules": [{ "id": "NoParties" }] },
            { "id": "Ruthless Mercenaries", "rules": [{ "id": "HardMode" }] },
        ]))
        .unwrap();
        leagues.apply_api_leagues(api);
        let ids: Vec<String> = leagues.list().into_iter().map(|l| l.id).collect();
        assert_eq!(ids, ["Standard", "Mercenaries", "Hardcore Mercenaries"]);
        assert_eq!(leagues.selected_id().as_deref(), Some("Mercenaries"));
        assert!(leagues.selected().unwrap().is_popular);
    }
}
