//! Mapping session stats fed by Client.txt.
//!
//! SCAFFOLD: replace freely. Keep the `MappingWidget` name, its `anchor` field, the
//! `MappingState` name and the `show` signature: config.rs / widgets/mod.rs use them.

use serde::{Deserialize, Serialize};

use super::{WCtx, chrome};
use crate::config::Anchor;

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-mapping"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MappingWidget {
    pub anchor: Anchor,
}

impl Default for MappingWidget {
    fn default() -> Self {
        Self { anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 } }
    }
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct MappingState {}

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, _st: &mut MappingState, wm_id: u32) {
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, _w| {
        ui.label("Mapping");
    });
    chrome::apply(w, wm_id, &r);
}

/// Client.txt lines (already split, newest last). Called for every batch.
pub fn on_game_log(_w: &mut WCtx<'_>, _st: &mut MappingState, _lines: &[String]) {}
