//! # rpt-platform
//!
//! OS integration layer of Rusty POE Trade — the port of Awakened PoE
//! Trade's Electron main process (`main/src/**`). It does **not** create or
//! draw the overlay window; the `app` crate (eframe/egui, winit) owns that.
//!
//! ## Threads
//!
//! [`Platform::start`] spawns (all named `rpt-*`):
//! * `rpt-platform` — worker owning all state; performs input synthesis and
//!   clipboard access sequentially (like upstream's single JS thread).
//! * `rpt-hotkeys` — owns the `global-hotkey` manager (and pumps Win32
//!   messages on Windows).
//! * `rpt-input-listener` — passive global keyboard/mouse listener.
//! * `rpt-window-tracker` — game window tracking.
//! * `rpt-game-log`, `rpt-updater`.
//!
//! The tray ([`tray::Tray::spawn`]) is separate and optional; it also runs
//! on its own thread (GTK main loop on Linux, Win32 loop on Windows).
//!
//! ## App integration
//!
//! ```ignore
//! let (tx, rx) = crossbeam_channel::unbounded();
//! let ctx = egui_ctx.clone();
//! let platform = rpt_platform::Platform::start(host_config, tx.clone(),
//!     std::sync::Arc::new(move || ctx.request_repaint()))?;
//! let _tray = rpt_platform::tray::Tray::spawn(opts, tx, wake).ok();
//! // once the winit window exists:
//! use raw_window_handle::HasWindowHandle;
//! let raw = frame.window_handle()?.as_raw();
//! platform.set_overlay_window(rpt_platform::OverlayHandle::from_raw(raw));
//! // every frame: while let Ok(ev) = rx.try_recv() { ... }
//! ```
//!
//! * **Linux: force winit's X11 backend** (e.g. unset `WAYLAND_DISPLAY`
//!   before creating the event loop, or `EventLoopBuilderExtX11::with_x11`).
//!   The game is an XWayland client; a native Wayland overlay can neither be
//!   positioned over it nor focused by us. Create the overlay as a normal
//!   (not override-redirect), undecorated, always-on-top, skip-taskbar
//!   window.
//! * Windows: winit is per-monitor DPI aware by default; keep it that way so
//!   bounds/cursor coordinates are physical pixels.
//! * React to [`PlatformEvent::FocusChange`]: `overlay == true` → make the
//!   window accept input (`set_cursor_hittest(true)`), show widgets;
//!   `false` → click-through. Show the overlay window only while
//!   `game || overlay`; size/position it from [`PlatformEvent::GameWindow`].
//! * In-overlay keys (upstream `handleExtraCommands`): `Escape`/`Ctrl + W` →
//!   [`Platform::focus_game`]; `HostConfig.overlay_key` →
//!   [`Platform::toggle_overlay`] (build the string with
//!   [`keys::overlay_key_string`]).

pub mod clipboard;
mod core;
mod emit;
pub mod host_files;
mod hotkeys;
mod input;
pub mod keys;
mod listener;
pub mod not_ported;
pub mod shortcuts;
pub mod tray;
pub mod types;
pub mod updater;
mod window;

use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::Sender;
use parking_lot::Mutex;

pub use emit::WakeFn;
#[cfg(target_os = "linux")]
pub use window::hyprland_overlay_rule as prepare_window_rules;
pub use types::{
    AreaRect, HostConfig, NoDownloadReason, OverlayHandle, PlatformEvent, Rect, ShortcutAction,
    ShortcutBinding, TrackArea, UpdateInfo,
};

use crate::core::{Cmd, Core, Subsystems};
use crate::emit::{Emitter, Level};

/// Handle to the running platform layer. Cheap to share by reference; all
/// methods are non-blocking (they enqueue work for the worker thread).
pub struct Platform {
    tx: Sender<Cmd>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Platform {
    /// Starts all subsystems. Must be called once per process. Failures of
    /// individual subsystems (no X display, hotkeys unavailable, …) are
    /// reported as [`PlatformEvent::LogEntry`] errors and do not fail start.
    pub fn start(
        config: HostConfig,
        events: Sender<PlatformEvent>,
        wake_ui: Arc<dyn Fn() + Send + Sync>,
    ) -> anyhow::Result<Self> {
        let emit = Emitter::new(events, wake_ui);
        let (tx, rx) = crossbeam_channel::unbounded::<Cmd>();

        emit.log(
            Level::Info,
            "Platform",
            &format!("{} {} / v{}", std::env::consts::OS, std::env::consts::ARCH, updater::APP_VERSION),
        );
        #[cfg(target_os = "linux")]
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            emit.log(
                Level::Info,
                "Platform",
                "Wayland session detected: using XWayland for hotkeys, input and window tracking.",
            );
        }

        let hotkeys = {
            let tx = tx.clone();
            match hotkeys::Hotkeys::spawn(Box::new(move |id| { let _ = tx.send(Cmd::Hotkey(id)); }), emit.clone()) {
                Ok(h) => Some(h),
                Err(e) => {
                    emit.log(Level::Error, "Shortcuts", &format!("{e:#}"));
                    None
                }
            }
        };
        let tracker = {
            let tx = tx.clone();
            let sink: window::WindowSink = Box::new(move |ev| { let _ = tx.send(Cmd::Window(ev)); });
            match window::GameWindowTracker::spawn(config.window_title.clone(), sink) {
                Ok(t) => Some(t),
                Err(e) => {
                    emit.log(Level::Error, "Overlay", &format!("Game window tracking unavailable: {e:#}"));
                    None
                }
            }
        };
        let listener = {
            let tx = tx.clone();
            match listener::spawn(Box::new(move |ev| { let _ = tx.send(Cmd::Input(ev)); })) {
                Ok(l) => Some(l),
                Err(e) => {
                    emit.log(Level::Error, "Shortcuts", &format!("Global input listener unavailable: {e:#}"));
                    None
                }
            }
        };
        let log_watcher = host_files::game_log::GameLogWatcher::spawn(emit.clone()).ok();
        let updater = updater::Updater::spawn(emit.clone()).ok();

        let sys = Subsystems { hotkeys, tracker, listener, log_watcher, updater };
        let worker = std::thread::Builder::new().name("rpt-platform".into()).spawn(move || {
            Core::new(emit, sys).run(rx);
        })?;
        let platform = Self { tx, worker: Mutex::new(Some(worker)) };
        platform.update_config(config);
        Ok(platform)
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    /// `CLIENT->MAIN::update-host-config`: re-applies shortcuts, window title,
    /// log/ini paths. Triggers the startup update check the first time.
    pub fn update_config(&self, config: HostConfig) {
        self.send(Cmd::Config(Box::new(config)));
    }

    /// Native handle of the overlay window, used to focus it.
    pub fn set_overlay_window(&self, handle: OverlayHandle) {
        self.send(Cmd::SetOverlayHandle(handle));
    }

    /// `true` = upstream `assertOverlayActive` (focus overlay, it takes
    /// input); `false` = `assertGameActive` (focus game, overlay becomes
    /// passthrough). Results in a [`PlatformEvent::FocusChange`].
    pub fn set_overlay_active(&self, active: bool) {
        self.send(Cmd::SetOverlayActive(active));
    }

    /// Upstream `toggleActiveState` (overlay key pressed inside the overlay).
    pub fn toggle_overlay(&self) {
        self.send(Cmd::ToggleOverlay);
    }

    /// `OVERLAY->MAIN::focus-game`.
    pub fn focus_game(&self) {
        self.send(Cmd::FocusGame);
    }

    /// `CLIENT->MAIN::user-action { action: 'stash-search' }`: focuses the
    /// game, Ctrl+F, pastes `text`, Enter (clipboard restored if enabled).
    pub fn stash_search(&self, text: &str) {
        self.send(Cmd::StashSearch(text.to_owned()));
    }

    /// Same as the `paste-in-chat` shortcut action, for UI buttons.
    pub fn paste_in_chat(&self, text: &str, send: bool) {
        self.send(Cmd::PasteInChat(text.to_owned(), send));
    }

    /// `OVERLAY->MAIN::track-area` (upstream `WidgetAreaTracker`): closes an
    /// "exclusive" widget ([`PlatformEvent::HideExclusiveWidget`]) once the
    /// cursor moves `close_threshold` away from `from` without the hold key,
    /// and focuses the overlay when the cursor enters `area`.
    /// Coordinates are physical screen pixels (see [`TrackArea`]).
    pub fn track_area(&self, req: TrackArea) {
        self.send(Cmd::TrackArea(req));
    }

    /// Stops area tracking (e.g. the widget was closed by the UI).
    pub fn cancel_track_area(&self) {
        self.send(Cmd::CancelTrackArea);
    }

    /// `CLIENT->MAIN::used-recently`: whether the overlay (vs. a browser
    /// client upstream) was used last. Gates `focusOverlay` of copy-item.
    pub fn set_used_recently(&self, is_overlay: bool) {
        self.send(Cmd::UsedRecently(is_overlay));
    }

    /// `user-action: check-for-update`. Result arrives as
    /// [`PlatformEvent::UpdaterState`].
    pub fn check_for_update(&self) {
        self.send(Cmd::CheckForUpdate);
    }

    /// Unregisters global shortcuts and stops the worker. Call before the
    /// app exits (also on [`PlatformEvent::QuitRequested`]). Idempotent.
    pub fn quit(&self) {
        self.send(Cmd::Shutdown);
        if let Some(h) = self.worker.lock().take() {
            let _ = h.join();
        }
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        self.quit();
    }
}
