//! Port of `AppUpdater.ts`, reduced to "is there a newer GitHub release?".
//! Never downloads or installs anything: Arch users update through
//! pacman/AUR, Windows users get the release page URL.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::emit::Emitter;
use crate::types::{NoDownloadReason, PlatformEvent, UpdateInfo};

/// `owner/repo` on GitHub. Placeholder until the project is published.
pub const GITHUB_REPO: &str = "tylort69/rusty-poe-trade";
/// Version of this build (workspace version).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECK_INTERVAL: Duration = Duration::from_secs(16 * 60 * 60);

pub(crate) enum UpdaterCmd {
    Check,
    CheckAtStartup,
}

pub(crate) struct Updater {
    tx: Sender<UpdaterCmd>,
}

impl Updater {
    pub fn spawn(emit: Emitter) -> anyhow::Result<Self> {
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::Builder::new().name("rpt-updater".into()).spawn(move || run(rx, emit))?;
        Ok(Self { tx })
    }

    pub fn send(&self, cmd: UpdaterCmd) {
        let _ = self.tx.send(cmd);
    }
}

fn run(rx: Receiver<UpdaterCmd>, emit: Emitter) {
    let mut checked_at_startup = false;
    let mut next = Instant::now() + CHECK_INTERVAL;
    loop {
        let timeout = next.saturating_duration_since(Instant::now());
        let do_check = match rx.recv_timeout(timeout) {
            Ok(UpdaterCmd::Check) => true,
            Ok(UpdaterCmd::CheckAtStartup) => !std::mem::replace(&mut checked_at_startup, true),
            Err(RecvTimeoutError::Timeout) => true,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if do_check {
            next = Instant::now() + CHECK_INTERVAL;
            emit.send(PlatformEvent::UpdaterState(UpdateInfo::CheckingForUpdate));
            emit.send(PlatformEvent::UpdaterState(check()));
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Performs one blocking check against the GitHub releases API.
pub fn check() -> UpdateInfo {
    match fetch_latest() {
        Ok(rel) if !rel.draft && !rel.prerelease && is_newer(&rel.tag_name, APP_VERSION) => UpdateInfo::UpdateAvailable {
            version: rel.tag_name.trim_start_matches('v').to_string(),
            no_download_reason: Some(NoDownloadReason::NotSupported),
            url: rel.html_url,
        },
        Ok(_) => UpdateInfo::UpdateNotAvailable { checked_at: now_ms() },
        Err(e) => {
            tracing::warn!("update check failed: {e:#}");
            UpdateInfo::Error { checked_at: now_ms() }
        }
    }
}

fn fetch_latest() -> anyhow::Result<Release> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("rusty-poe-trade/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()?;
    let rel = client
        .get(format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()?
        .error_for_status()?
        .json::<Release>()?;
    Ok(rel)
}

fn parse_version(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// `true` if `candidate` is a strictly greater dotted version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert!(is_newer("v0.2.0-beta", "0.1.0"));
    }
}
