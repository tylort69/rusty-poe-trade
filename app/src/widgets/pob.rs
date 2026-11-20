//! Path of Building build viewer.
//!
//! SCAFFOLD: replace freely. Keep the `PobWidget` name, its `anchor` field, the
//! `PobState` name and the `show` signature: config.rs / widgets/mod.rs use them.

use serde::{Deserialize, Serialize};

use super::{WCtx, chrome};
use crate::config::Anchor;

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-pob"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PobWidget {
    pub anchor: Anchor,
}

impl Default for PobWidget {
    fn default() -> Self {
        Self { anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 } }
    }
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct PobState {}

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, _st: &mut PobState, wm_id: u32) {
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, _w| {
        ui.label("Path of Building");
    });
    chrome::apply(w, wm_id, &r);
}
