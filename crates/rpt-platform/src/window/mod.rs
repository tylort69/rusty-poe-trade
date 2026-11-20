//! Game window tracking — replacement for the `electron-overlay-window`
//! native module (`OverlayController.attachByTitle/focusTarget/activateOverlay`).
//!
//! The tracker finds the top-level window whose title is exactly
//! `HostConfig.window_title`, reports its client-area bounds (physical screen
//! pixels) and whether it is the foreground window, and can focus either the
//! game or the overlay window on request.

use crate::types::{OverlayHandle, Rect};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;
#[cfg(not(any(target_os = "linux", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", windows)))]
use unsupported as imp;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WindowEvent {
    /// Target window found. `has_access == false` when it runs elevated and we
    /// do not (Windows).
    Attached { has_access: bool },
    Detached,
    Foreground { game: bool },
    Bounds(Option<Rect>),
}

pub(crate) type WindowSink = Box<dyn Fn(WindowEvent) + Send + Sync>;

pub(crate) struct GameWindowTracker {
    inner: imp::Tracker,
}

impl GameWindowTracker {
    pub fn spawn(title: String, sink: WindowSink) -> anyhow::Result<Self> {
        Ok(Self { inner: imp::Tracker::spawn(title, sink)? })
    }

    pub fn set_title(&self, title: String) {
        self.inner.set_title(title);
    }

    pub fn set_overlay(&self, handle: OverlayHandle) {
        self.inner.set_overlay(handle);
    }

    /// Brings the game to the foreground, waiting briefly until it actually is
    /// so that subsequent synthetic input lands in the game.
    pub fn focus_game(&self) -> bool {
        self.inner.focus_game()
    }

    /// Gives keyboard focus to the overlay window.
    pub fn focus_overlay(&self) -> bool {
        self.inner.focus_overlay()
    }
}
