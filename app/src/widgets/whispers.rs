//! Trade whisper manager fed by Client.txt.
//!
//! SCAFFOLD: replace freely. Keep the `WhispersWidget` name, its `anchor` field, the
//! `WhispersState` name and the `show` signature: config.rs / widgets/mod.rs use them.

use serde::{Deserialize, Serialize};

use super::{WCtx, chrome};
use crate::config::Anchor;

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-whispers"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WhispersWidget {
    pub anchor: Anchor,
}

impl Default for WhispersWidget {
    fn default() -> Self {
        Self { anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 } }
    }
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct WhispersState {}

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, _st: &mut WhispersState, wm_id: u32) {
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, _w| {
        ui.label("Trade whispers");
    });
    chrome::apply(w, wm_id, &r);
}

/// Client.txt lines (already split, newest last). Called for every batch.
pub fn on_game_log(_w: &mut WCtx<'_>, _st: &mut WhispersState, _lines: &[String]) {}
