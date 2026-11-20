//! Data types mirroring `ipc/types.ts`. Serde representations are
//! wire-compatible with upstream JSON (camelCase fields, kebab-case tags) so
//! an imported Awakened PoE Trade config can be fed through unchanged.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Mirror of upstream `HostConfig` (`CLIENT->MAIN::update-host-config`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HostConfig {
    pub shortcuts: Vec<ShortcutBinding>,
    pub restore_clipboard: bool,
    /// Path to `Client.txt`; `None` or empty = auto-detect.
    pub client_log: Option<PathBuf>,
    /// Path to `production_Config.ini`; `None` or empty = auto-detect.
    pub game_config: Option<PathBuf>,
    pub stash_scroll: bool,
    /// Key that toggles the overlay *from inside the overlay window* (handled
    /// by the app, see [`crate::keys::overlay_key_string`]).
    pub overlay_key: String,
    pub log_keys: bool,
    pub window_title: String,
    pub language: String,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            shortcuts: Vec::new(),
            restore_clipboard: false,
            client_log: None,
            game_config: None,
            stash_scroll: true,
            overlay_key: "Shift + Space".into(),
            log_keys: false,
            window_title: "Path of Exile".into(),
            language: "en".into(),
        }
    }
}

/// One entry of `HostConfig.shortcuts` (upstream `ShortcutAction` interface).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    /// APT shortcut string, e.g. `Ctrl + D`.
    pub shortcut: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_mod_keys: bool,
    pub action: ShortcutAction,
}

/// The `action` union of upstream `ShortcutAction`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ShortcutAction {
    CopyItem {
        target: String,
        #[serde(default)]
        focus_overlay: bool,
    },
    /// Heist gem OCR. Not ported (see `vision` module); the hotkey is accepted
    /// and ignored with a log message.
    OcrText { target: String },
    TriggerEvent { target: String },
    StashSearch { text: String },
    ToggleOverlay,
    PasteInChat { text: String, send: bool },
    TestOnly,
}

impl ShortcutAction {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::CopyItem { .. } => "copy-item",
            Self::OcrText { .. } => "ocr-text",
            Self::TriggerEvent { .. } => "trigger-event",
            Self::StashSearch { .. } => "stash-search",
            Self::ToggleOverlay => "toggle-overlay",
            Self::PasteInChat { .. } => "paste-in-chat",
            Self::TestOnly => "test-only",
        }
    }
}

/// Mirror of upstream `UpdateInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum UpdateInfo {
    Initial,
    CheckingForUpdate,
    UpdateAvailable {
        version: String,
        /// Always `Some(NotSupported)`: this port never self-updates.
        no_download_reason: Option<NoDownloadReason>,
        /// Release page for the user to open (Windows) / informational (Arch).
        url: String,
    },
    /// Kept for wire compatibility; never emitted.
    UpdateDownloaded { version: String },
    UpdateNotAvailable {
        /// Unix time in milliseconds.
        checked_at: i64,
    },
    Error { checked_at: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoDownloadReason {
    NotSupported,
    DisabledByFlag,
}

/// Rectangle in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    /// Strict containment, as upstream `isPointInsideRect`.
    pub fn contains_strict(&self, x: f64, y: f64) -> bool {
        x > self.x as f64
            && x < (self.x + self.width) as f64
            && y > self.y as f64
            && y < (self.y + self.height) as f64
    }
}

/// Area tracking request (upstream `OVERLAY->MAIN::track-area`).
///
/// All coordinates are **physical screen pixels**. Upstream converted DIP to
/// screen pixels on Windows itself; here the app must do it: for egui that is
/// `screen = window_outer_position + point * pixels_per_point`, and
/// `close_threshold *= pixels_per_point`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackArea {
    /// `"Ctrl"`, `"Alt"` or anything else (= no hold key).
    pub hold_key: String,
    pub close_threshold: f64,
    pub from: (f64, f64),
    pub area: AreaRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AreaRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl AreaRect {
    pub fn contains_strict(&self, x: f64, y: f64) -> bool {
        x > self.x && x < self.x + self.width && y > self.y && y < self.y + self.height
    }
}

/// Native handle of the overlay window created by the app (winit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayHandle {
    /// Windows `HWND` as an integer.
    Win32 { hwnd: isize },
    /// X11 window id (XWayland included).
    X11 { window: u32 },
    /// E.g. a native Wayland surface: focus requests for it are no-ops.
    Unsupported,
}

impl OverlayHandle {
    /// Converts from `raw-window-handle` 0.6 (what winit 0.30 / eframe give
    /// you via `HasWindowHandle::window_handle()?.as_raw()`).
    pub fn from_raw(raw: raw_window_handle::RawWindowHandle) -> Self {
        use raw_window_handle::RawWindowHandle as R;
        match raw {
            R::Win32(h) => OverlayHandle::Win32 { hwnd: h.hwnd.get() },
            #[allow(clippy::unnecessary_cast)] // c_ulong is u64 on Linux, u32 on Windows
            R::Xlib(h) => OverlayHandle::X11 { window: h.window as u32 },
            R::Xcb(h) => OverlayHandle::X11 { window: h.window.get() },
            _ => OverlayHandle::Unsupported,
        }
    }
}

/// Everything the platform layer reports to the UI. Upstream event names are
/// noted on each variant.
#[derive(Debug, Clone, PartialEq)]
pub enum PlatformEvent {
    /// `MAIN->CLIENT::item-text`. `position` is the cursor in physical screen
    /// pixels at the moment the hotkey was pressed.
    ItemText { target: String, clipboard: String, position: (i32, i32), focus_overlay: bool },
    /// `MAIN->CLIENT::widget-action` (trigger-event shortcut).
    WidgetAction { target: String },
    /// Emitted when a `toggle-overlay` shortcut fires, right before the
    /// resulting [`PlatformEvent::FocusChange`]. Informational.
    ToggleOverlay,
    /// `MAIN->OVERLAY::focus-change`. `overlay == true` means the overlay
    /// must accept input (disable click-through, show it); `false` means
    /// passthrough. Emitted once at start and whenever `game` or `overlay`
    /// changes (upstream only emitted on game focus changes).
    FocusChange { game: bool, overlay: bool, using_hotkey: bool },
    /// Game window geometry/foreground state. `bounds` = client area in
    /// physical screen pixels; `None` = game window not found.
    GameWindow { bounds: Option<Rect>, focused: bool },
    /// `MAIN->OVERLAY::overlay-attached` — the game window was found and we can
    /// interact with it.
    OverlayAttached,
    /// The game window was found but runs elevated while we don't (Windows),
    /// so hotkeys/input injection will not reach it. Upstream showed an error
    /// dialog ("restart with administrator rights").
    GameNoAccess,
    /// `MAIN->CLIENT::game-log`
    GameLog { lines: Vec<String> },
    /// `MAIN->CLIENT::log-entry` — formatted `[time] level [Module] message\n`.
    LogEntry { message: String },
    /// `MAIN->OVERLAY::hide-exclusive-widget`
    HideExclusiveWidget,
    /// `MAIN->OVERLAY::visibility` — overlay should be hidden while the user
    /// holds Alt alone (port of `OverlayVisibility.ts`).
    Visibility { visible: bool },
    /// `MAIN->CLIENT::updater-state`
    UpdaterState(UpdateInfo),
    /// Tray menu "Settings".
    TrayOpenSettings,
    /// Tray menu "Quit". The app should close its window and call
    /// [`crate::Platform::quit`].
    QuitRequested,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_config_from_upstream_json() {
        let json = r#"{
            "shortcuts": [
                {"shortcut": "Ctrl + D", "action": {"type": "copy-item", "target": "price-check", "focusOverlay": false}},
                {"shortcut": "Alt + D", "keepModKeys": true, "action": {"type": "copy-item", "target": "price-check", "focusOverlay": true}},
                {"shortcut": "F5", "action": {"type": "paste-in-chat", "text": "/hideout", "send": true}},
                {"shortcut": "Shift + Space", "action": {"type": "toggle-overlay"}},
                {"shortcut": "Ctrl + Alt + W", "action": {"type": "trigger-event", "target": "delve-grid"}},
                {"shortcut": "Ctrl + Q", "action": {"type": "ocr-text", "target": "heist-gems"}},
                {"shortcut": "F9", "action": {"type": "stash-search", "text": "\"Map Tier\""}},
                {"shortcut": "F10", "action": {"type": "test-only"}}
            ],
            "restoreClipboard": true,
            "clientLog": null,
            "gameConfig": "",
            "stashScroll": true,
            "overlayKey": "Shift + Space",
            "logKeys": false,
            "windowTitle": "Path of Exile",
            "language": "en"
        }"#;
        let cfg: HostConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.shortcuts.len(), 8);
        assert!(cfg.shortcuts[1].keep_mod_keys);
        assert_eq!(
            cfg.shortcuts[1].action,
            ShortcutAction::CopyItem { target: "price-check".into(), focus_overlay: true }
        );
        assert_eq!(cfg.shortcuts[3].action, ShortcutAction::ToggleOverlay);
        assert!(cfg.restore_clipboard);
        assert_eq!(cfg.client_log, None);

        let back = serde_json::to_value(&cfg.shortcuts[2]).unwrap();
        assert_eq!(back["action"]["type"], "paste-in-chat");
        assert!(back.get("keepModKeys").is_none());
    }

    #[test]
    fn update_info_json() {
        let v = serde_json::to_value(UpdateInfo::UpdateNotAvailable { checked_at: 5 }).unwrap();
        assert_eq!(v, serde_json::json!({"state": "update-not-available", "checkedAt": 5}));
    }

    #[test]
    fn rect_contains() {
        let r = Rect { x: 0, y: 0, width: 10, height: 10 };
        assert!(r.contains_strict(5.0, 5.0));
        assert!(!r.contains_strict(0.0, 5.0));
    }
}
