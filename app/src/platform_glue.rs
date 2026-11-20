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

    /// Game client area in physical screen pixels (as reported by the platform).
    pub fn game_rect_px(&self) -> Option<rpt_platform::Rect> {
        *self.game.lock()
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
            HostCommand::FocusOverlay => p.set_overlay_active(true),
            HostCommand::StashSearch(t) => p.stash_search(&t),
            HostCommand::PasteInChat { text, send } => p.paste_in_chat(&text, send),
            HostCommand::TrackArea { hold_key, close_threshold, from, area, dpr } => p.track_area(rpt_platform::TrackArea {
                hold_key,
                close_threshold: close_threshold as f64,
                from: coords::pos_to_physical(from, dpr),
                area: coords::rect_to_physical(area, dpr),
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

/// Physical-pixel ↔ egui-point conversions.
///
/// egui points relate to physical pixels by `pixels_per_point`, which egui-winit
/// computes as `native_pixels_per_point (OS DPI scale) × zoom_factor`. The
/// overlay zoom therefore changes how many points the game rect spans, so every
/// conversion uses the *full* ppp (never just the native one) — and the same
/// ppp egui-winit uses when it applies viewport commands after the pass.
pub mod coords {
    use egui::{Pos2, Rect, pos2, vec2};

    /// `pixels_per_point` egui-winit uses for a given OS scale and zoom.
    pub fn ppp(native_pixels_per_point: f32, zoom: f32) -> f32 {
        native_pixels_per_point * zoom
    }

    /// The ppp that this pass's viewport commands (and the next pass) will use
    /// once `zoom` is applied with `set_zoom_factor`. Falls back to the current
    /// pass when the OS scale is not known yet.
    pub fn target_ppp(ctx: &egui::Context, zoom: f32) -> f32 {
        let native = ctx.native_pixels_per_point().unwrap_or_else(|| ctx.pixels_per_point() / ctx.zoom_factor().max(f32::EPSILON));
        ppp(native, zoom)
    }

    /// A physical screen rect (the game client area) in absolute egui points.
    pub fn rect_to_points(r: rpt_platform::Rect, ppp: f32) -> Rect {
        Rect::from_min_size(pos2(r.x as f32, r.y as f32) / ppp, vec2(r.width as f32, r.height as f32) / ppp)
    }

    /// A physical screen position in absolute egui points.
    pub fn pos_to_points(p: Pos2, ppp: f32) -> Pos2 {
        pos2(p.x / ppp, p.y / ppp)
    }

    /// Absolute egui points → physical screen pixels.
    pub fn pos_to_physical(p: Pos2, ppp: f32) -> (f64, f64) {
        (p.x as f64 * ppp as f64, p.y as f64 * ppp as f64)
    }

    /// Absolute egui-point rect → physical screen pixels.
    pub fn rect_to_physical(r: Rect, ppp: f32) -> rpt_platform::AreaRect {
        let p = ppp as f64;
        rpt_platform::AreaRect { x: r.min.x as f64 * p, y: r.min.y as f64 * p, width: r.width() as f64 * p, height: r.height() as f64 * p }
    }

    /// A physical screen position (cursor from the platform) → overlay-local
    /// points, given the overlay window's top-left in absolute points.
    pub fn screen_to_local(p: Pos2, window_origin: Pos2, ppp: f32) -> Pos2 {
        pos_to_points(p, ppp) - window_origin.to_vec2()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Physical pixels egui-winit/winit end up requesting for a point value
        /// (`ppp * points`, rounded to integers by winit).
        fn applied_px(points: f32, ppp: f32) -> i32 {
            (points * ppp).round() as i32
        }

        const DPIS: [f32; 3] = [1.0, 1.5, 2.0];
        const ZOOMS: [f32; 3] = [0.5, 1.0, 2.0];
        const GAMES: [rpt_platform::Rect; 3] = [
            rpt_platform::Rect { x: 0, y: 0, width: 2560, height: 1440 },
            rpt_platform::Rect { x: 1920, y: 0, width: 3440, height: 1440 },
            rpt_platform::Rect { x: -1280, y: 37, width: 5120, height: 1441 },
        ];

        #[test]
        fn overlay_covers_game_exactly() {
            for dpi in DPIS {
                for zoom in ZOOMS {
                    let ppp = ppp(dpi, zoom);
                    for g in GAMES {
                        let r = rect_to_points(g, ppp);
                        // what egui-winit sends to winit for OuterPosition / InnerSize
                        assert_eq!(applied_px(r.min.x, ppp), g.x, "dpi {dpi} zoom {zoom}");
                        assert_eq!(applied_px(r.min.y, ppp), g.y);
                        assert_eq!(applied_px(r.width(), ppp), g.width);
                        assert_eq!(applied_px(r.height(), ppp), g.height);
                    }
                }
            }
        }

        #[test]
        fn cursor_and_track_area_round_trip() {
            for dpi in DPIS {
                for zoom in ZOOMS {
                    let ppp = ppp(dpi, zoom);
                    for g in GAMES {
                        let game = rect_to_points(g, ppp);
                        // cursor 100px right / 200px below the game's top-left
                        let cursor = pos2((g.x + 100) as f32, (g.y + 200) as f32);
                        let local = screen_to_local(cursor, game.min, ppp);
                        assert!((local.x - 100.0 / ppp).abs() < 1e-3 && (local.y - 200.0 / ppp).abs() < 1e-3);
                        // overlay-local → absolute points → physical (TrackArea `from`)
                        let (x, y) = pos_to_physical(local + game.min.to_vec2(), ppp);
                        assert!((x - cursor.x as f64).abs() < 0.01 && (y - cursor.y as f64).abs() < 0.01, "dpi {dpi} zoom {zoom}");
                        // a full-height column 10pt from the left edge (TrackArea `area`)
                        let area = Rect::from_min_size(pos2(10.0, 0.0), vec2(460.0, game.height())).translate(game.min.to_vec2());
                        let a = rect_to_physical(area, ppp);
                        assert!((a.x - (g.x as f64 + 10.0 * ppp as f64)).abs() < 0.01);
                        assert!((a.y - g.y as f64).abs() < 0.01);
                        assert!((a.width - 460.0 * ppp as f64).abs() < 0.01);
                        assert!((a.height - g.height as f64).abs() < 0.01);
                    }
                }
            }
        }

        #[test]
        fn zoom_scales_point_space() {
            let g = GAMES[0];
            assert_eq!(rect_to_points(g, ppp(1.0, 1.0)).width(), 2560.0);
            assert_eq!(rect_to_points(g, ppp(1.0, 2.0)).width(), 1280.0);
            assert_eq!(rect_to_points(g, ppp(1.0, 0.5)).width(), 5120.0);
            assert_eq!(rect_to_points(g, ppp(2.0, 1.0)).width(), 1280.0);
            assert_eq!(rect_to_points(g, ppp(1.5, 2.0)).width(), 2560.0 / 3.0);
        }

        #[test]
        fn target_ppp_uses_native_scale() {
            let ctx = egui::Context::default();
            // no viewport info yet: derived from the current pass (ppp 1, zoom 1)
            assert_eq!(target_ppp(&ctx, 2.0), 2.0);
            let mut input = egui::RawInput::default();
            input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.5);
            let mut out = ctx.run_ui(input, |ui| {
                assert_eq!(target_ppp(ui.ctx(), 2.0), 3.0);
                assert_eq!(target_ppp(ui.ctx(), 0.5), 0.75);
            });
            out.textures_delta.clear();
        }
    }
}
