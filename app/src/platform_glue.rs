//! Adapter from the app to `rpt-platform`. TODO(platform): wire to the real crate.

use crossbeam_channel::Sender;

use crate::config::Config;
use crate::host::{HostCommand, HostEvent};

pub struct PlatformGlue {
    _tx: Sender<HostEvent>,
}

impl PlatformGlue {
    pub fn start(_cfg: &Config, tx: Sender<HostEvent>, _ctx: egui::Context, _windowed: bool) -> Self {
        Self { _tx: tx }
    }
    pub fn game_rect(&self) -> Option<egui::Rect> {
        None
    }
    pub fn set_overlay_active(&self, _active: bool) {}
    pub fn update_config(&self, _cfg: &Config) {}
    pub fn command(&self, cmd: HostCommand) {
        tracing::debug!(?cmd, "platform command (not wired)");
    }
}
