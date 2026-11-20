//! League list for the price-check settings page (upstream `background/Leagues.ts`).

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Deserialize;

const PERMANENT_HC: &[&str] = &["Hardcore", "專家模式"];

#[derive(Default, Clone)]
pub struct Leagues {
    pub loading: bool,
    pub error: Option<String>,
    pub list: Vec<String>,
    /// Set once after a load so the page can auto-select a live league.
    pub just_loaded: bool,
}

#[derive(Deserialize)]
struct ApiLeague {
    id: String,
    #[serde(default)]
    event: bool,
    #[serde(default)]
    rules: Vec<ApiRule>,
}

#[derive(Deserialize)]
struct ApiRule {
    id: String,
}

pub type Shared = Arc<Mutex<Leagues>>;

pub fn is_private_league(id: &str) -> bool {
    if id.contains("Ruthless") {
        return true;
    }
    // /\(PL\d+\)$/
    id.strip_suffix(')')
        .and_then(|s| s.rfind("(PL").map(|i| &s[i + 3..]))
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

pub fn load(shared: &Shared, rt: &tokio::runtime::Runtime, host: &str, ctx: egui::Context) {
    {
        let mut s = shared.lock();
        if s.loading {
            return;
        }
        s.loading = true;
        s.error = None;
    }
    let shared = shared.clone();
    let url = format!("https://{host}/api/leagues?type=main&realm=pc");
    rt.spawn(async move {
        let res = fetch(&url).await;
        let mut s = shared.lock();
        s.loading = false;
        match res {
            Ok(list) => {
                s.list = list;
                s.just_loaded = true;
            }
            Err(e) => s.error = Some(e),
        }
        ctx.request_repaint();
    });
}

async fn fetch(url: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("RustyPoeTrade/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let leagues: Vec<ApiLeague> = resp.json().await.map_err(|e| e.to_string())?;
    Ok(leagues
        .into_iter()
        .filter(|l| {
            !PERMANENT_HC.contains(&l.id.as_str()) && !l.rules.iter().any(|r| r.id == "NoParties" || (r.id == "HardMode" && !l.event))
        })
        .map(|l| l.id)
        .collect())
}

/// Upstream: if the selected league is gone (and not private), pick the
/// temp challenge league (index 1) or Standard.
pub fn auto_select(list: &[String], selected: &mut Option<String>) {
    let alive = selected.as_ref().is_some_and(|s| list.contains(s));
    if !alive && !selected.as_deref().is_some_and(is_private_league) {
        if let Some(l) = list.get(1).or_else(|| list.first()) {
            *selected = Some(l.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn private() {
        assert!(super::is_private_league("My League (PL12345)"));
        assert!(!super::is_private_league("Standard"));
    }
}
