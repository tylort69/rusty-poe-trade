//! Adapter from the app to `rpt-platform`: starts the OS layer + tray, converts
//! `PlatformEvent`s into UI `HostEvent`s, and forwards `HostCommand`s.

use std::sync::Arc;

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use rpt_platform::{HostConfig, OverlayHandle, Platform, PlatformEvent, ShortcutAction, ShortcutBinding};

use crate::config::{Config, ShortcutKind};
use crate::host::{HostCommand, HostEvent};

pub struct PlatformGlue {
    platform: Option<Platform>,
    _tray: Option<rpt_platform::tray::Tray>,
    /// Latest game client area in physical pixels.
    game: Arc<Mutex<Option<rpt_platform::Rect>>>,
    window_set: bool,
    config_dir: std::path::PathBuf,
}

pub fn host_config(cfg: &Config) -> HostConfig {
    let shortcuts = cfg
        .shortcuts()
        .into_iter()
        .map(|s| ShortcutBinding {
            shortcut: s.shortcut,
            keep_mod_keys: s.keep_mod_keys,
            action: match s.action {
                ShortcutKind::CopyItem { target, focus_overlay } => ShortcutAction::CopyItem { target, focus_overlay },
                ShortcutKind::OcrText { target } => ShortcutAction::OcrText { target },
                ShortcutKind::TriggerEvent { target } => ShortcutAction::TriggerEvent { target },
                ShortcutKind::StashSearch { text } => ShortcutAction::StashSearch { text },
                ShortcutKind::ToggleOverlay => ShortcutAction::ToggleOverlay,
                ShortcutKind::PasteInChat { text, send } => ShortcutAction::PasteInChat { text, send },
            },
        })
        .collect();
    let path = |p: &Option<String>| p.as_deref().filter(|s| !s.is_empty()).map(std::path::PathBuf::from);
    HostConfig {
        shortcuts,
        restore_clipboard: cfg.restore_clipboard,
        client_log: path(&cfg.client_log),
        game_config: path(&cfg.game_config),
        stash_scroll: cfg.stash_scroll,
        overlay_key: cfg.overlay_key.clone(),
        log_keys: cfg.log_keys,
        window_title: cfg.window_title.clone(),
        language: cfg.language.clone(),
    }
}

impl PlatformGlue {
    pub fn start(cfg: &Config, tx: Sender<HostEvent>, ctx: egui::Context, config_dir: std::path::PathBuf) -> Self {
        let game = Arc::new(Mutex::new(None));
        let (ptx, prx) = crossbeam_channel::unbounded::<PlatformEvent>();
        let wake_ctx = ctx.clone();
        let wake: rpt_platform::WakeFn = Arc::new(move || wake_ctx.request_repaint());

        let platform = if std::env::var_os("RPT_NO_PLATFORM").is_some() {
            tracing::warn!("RPT_NO_PLATFORM set: hotkeys and game tracking disabled");
            None
        } else {
            match Platform::start(host_config(cfg), ptx.clone(), wake.clone()) {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::error!("platform layer failed to start: {e:#}");
                    None
                }
            }
        };

        let icon = image::load_from_memory(include_bytes!("../../assets/icon.png")).ok().map(|i| {
            let i = i.to_rgba8();
            let (w, h) = i.dimensions();
            (i.into_raw(), w, h)
        });
        let tray = rpt_platform::tray::Tray::spawn(
            rpt_platform::tray::TrayOptions { tooltip: "Rusty POE Trade".into(), icon, config_dir: config_dir.clone() },
            ptx,
            wake,
        )
        .map_err(|e| tracing::warn!("tray unavailable: {e:#}"))
        .ok();

        let game2 = game.clone();
        std::thread::Builder::new()
            .name("rpt-events".into())
            .spawn(move || {
                for ev in prx {
                    let out = match ev {
                        PlatformEvent::ItemText { target, clipboard, position, focus_overlay } => Some(HostEvent::ItemText {
                            target,
                            clipboard,
                            position: egui::pos2(position.0 as f32, position.1 as f32),
                            focus_overlay,
                        }),
                        PlatformEvent::WidgetAction { target } => Some(HostEvent::WidgetAction { target }),
                        PlatformEvent::FocusChange { game, overlay, using_hotkey } => Some(HostEvent::FocusChange { game, overlay, using_hotkey }),
                        PlatformEvent::GameWindow { bounds, .. } => {
                            *game2.lock() = bounds;
                            None
                        }
                        PlatformEvent::OverlayAttached => Some(HostEvent::OverlayAttached),
                        PlatformEvent::GameNoAccess => Some(HostEvent::Notice(
                            "Path of Exile runs as administrator — restart Rusty POE Trade as administrator so hotkeys can reach the game.".into(),
                        )),
                        PlatformEvent::GameLog { lines } => Some(HostEvent::GameLog(lines)),
                        PlatformEvent::LogEntry { message } => Some(HostEvent::LogEntry(message)),
                        PlatformEvent::HideExclusiveWidget => Some(HostEvent::HideExclusiveWidget),
                        PlatformEvent::Visibility { visible } => Some(HostEvent::Visibility { visible }),
                        PlatformEvent::UpdaterState(rpt_platform::UpdateInfo::UpdateAvailable { version, url, .. }) => {
                            Some(HostEvent::UpdateAvailable { version, url })
                        }
                        PlatformEvent::TrayOpenSettings => Some(HostEvent::OpenSettings),
                        PlatformEvent::QuitRequested => Some(HostEvent::Quit),
                        _ => None,
                    };
                    if let Some(e) = out {
                        if tx.send(e).is_err() {
                            break;
                        }
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn event thread");

        Self { platform, _tray: tray, game, window_set: false, config_dir }
    }

    /// Hand the overlay's native window to the platform layer (once).
    pub fn set_window(&mut self, frame: &eframe::Frame) {
        if self.window_set {
            return;
        }
        use raw_window_handle::HasWindowHandle as _;
        if let (Some(p), Ok(h)) = (&self.platform, frame.window_handle()) {
            p.set_overlay_window(OverlayHandle::from_raw(h.as_raw()));
            self.window_set = true;
        }
    }

    /// Game client area in egui points.
    pub fn game_rect(&self, pixels_per_point: f32) -> Option<egui::Rect> {
        self.game.lock().map(|r| {
            egui::Rect::from_min_size(
                egui::pos2(r.x as f32, r.y as f32) / pixels_per_point,
                egui::vec2(r.width as f32, r.height as f32) / pixels_per_point,
            )
        })
    }

    pub fn set_overlay_active(&self, active: bool) {
        if let Some(p) = &self.platform {
            p.set_overlay_active(active);
        }
    }

    pub fn update_config(&self, cfg: &Config) {
        if let Some(p) = &self.platform {
            p.update_config(host_config(cfg));
        }
    }

    pub fn command(&self, cmd: HostCommand) {
        let Some(p) = &self.platform else {
            if let HostCommand::OpenConfigFolder = cmd {
                let _ = rpt_platform::host_files::open_url(&self.config_dir.to_string_lossy());
            }
            return;
        };
        match cmd {
            HostCommand::FocusGame => p.focus_game(),
            HostCommand::StashSearch(t) => p.stash_search(&t),
            HostCommand::PasteInChat { text, send } => p.paste_in_chat(&text, send),
            HostCommand::TrackArea { hold_key, close_threshold, from, area, dpr } => p.track_area(rpt_platform::TrackArea {
                hold_key,
                close_threshold: close_threshold as f64,
                from: (from.x as f64 * dpr as f64, from.y as f64 * dpr as f64),
                area: rpt_platform::AreaRect {
                    x: area.min.x as f64 * dpr as f64,
                    y: area.min.y as f64 * dpr as f64,
                    width: area.width() as f64 * dpr as f64,
                    height: area.height() as f64 * dpr as f64,
                },
            }),
            HostCommand::CheckForUpdate => p.check_for_update(),
            HostCommand::ToggleOverlay => p.toggle_overlay(),
            HostCommand::OpenConfigFolder => {
                let _ = rpt_platform::host_files::open_url(&self.config_dir.to_string_lossy());
            }
            HostCommand::Quit => p.quit(),
            HostCommand::SaveConfig | HostCommand::PushHostConfig | HostCommand::OpenUrl(_) => {}
        }
    }
}
