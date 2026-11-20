use super::WindowSink;
use crate::types::OverlayHandle;

pub(super) struct Tracker;

impl Tracker {
    pub fn spawn(_title: String, _sink: WindowSink) -> anyhow::Result<Self> {
        anyhow::bail!("game window tracking is not supported on this OS")
    }
    pub fn set_title(&self, _title: String) {}
    pub fn set_overlay(&self, _handle: OverlayHandle) {}
    pub fn focus_game(&self) -> bool {
        false
    }
    pub fn focus_overlay(&self) -> bool {
        false
    }
}
