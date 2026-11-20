//! Bridge between the UI and the OS layer (`rpt-platform`).
//!
//! Widgets never call the platform directly: they push [`HostCommand`]s, which
//! `App` drains each frame and forwards. This mirrors upstream's renderer →
//! main-process IPC (`CLIENT->MAIN::*`, `OVERLAY->MAIN::*` events).

/// Commands from the UI to the OS layer.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCommand {
    /// `OVERLAY->MAIN::focus-game`
    FocusGame,
    /// `CLIENT->MAIN::user-action` stash-search
    StashSearch(String),
    PasteInChat { text: String, send: bool },
    /// Config changed in a way that affects hotkeys/log paths (`update-host-config`).
    PushHostConfig,
    /// `CLIENT->MAIN::save-config`
    SaveConfig,
    /// `OVERLAY->MAIN::track-area`
    TrackArea { hold_key: String, close_threshold: f32, from: egui::Pos2, area: egui::Rect, dpr: f32 },
    CheckForUpdate,
    Quit,
    OpenUrl(String),
    OpenConfigFolder,
}

/// Events from the OS layer to the UI (mirror of upstream `MAIN->CLIENT::*`).
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    ItemText { target: String, clipboard: String, position: egui::Pos2, focus_overlay: bool },
    WidgetAction { target: String },
    FocusChange { game: bool, overlay: bool, using_hotkey: bool },
    Visibility { visible: bool },
    GameLog(Vec<String>),
    LogEntry(String),
    HideExclusiveWidget,
    OverlayAttached,
    UpdateAvailable { version: String, url: String },
}

#[derive(Default)]
pub struct HostQueue {
    pub commands: Vec<HostCommand>,
}

impl HostQueue {
    pub fn push(&mut self, c: HostCommand) {
        if !self.commands.contains(&c) {
            self.commands.push(c);
        }
    }
}
