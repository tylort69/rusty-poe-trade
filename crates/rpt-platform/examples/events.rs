//! Prints platform events, for manual testing without the UI.
//!
//! `cargo run -p rpt-platform --example events -- [seconds] [window title]`
//!
//! Registers a `Ctrl + D` copy-item shortcut (only while the game is focused),
//! unless `RPT_PASSIVE=1` is set (then nothing is registered or synthesized).

use std::sync::Arc;
use std::time::{Duration, Instant};

use rpt_platform::{HostConfig, Platform, ShortcutAction, ShortcutBinding};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    let mut cfg = HostConfig {
        shortcuts: vec![ShortcutBinding {
            shortcut: "Ctrl + D".into(),
            keep_mod_keys: false,
            action: ShortcutAction::CopyItem { target: "price-check".into(), focus_overlay: false },
        }],
        restore_clipboard: true,
        ..HostConfig::default()
    };
    if std::env::var_os("RPT_PASSIVE").is_some() {
        cfg.shortcuts.clear();
        cfg.stash_scroll = false;
    }
    if let Some(title) = args.next() {
        cfg.window_title = title;
    }
    let (tx, rx) = crossbeam_channel::unbounded();
    let platform = Platform::start(cfg, tx, Arc::new(|| {}))?;
    let end = Instant::now() + Duration::from_secs(secs);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Ok(ev) = rx.recv_timeout(left) {
            println!("{ev:?}");
        }
    }
    platform.quit();
    Ok(())
}
