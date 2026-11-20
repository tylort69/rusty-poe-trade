//! Stash valuation, net-worth history, dust calculator and bulk-sell list (Wealthy Exile-style, no server).
//!
//! SCAFFOLD: replace freely. Keep the `WealthWidget` name, its `anchor` field, the
//! `WealthState` name and the `show` signature: config.rs / widgets/mod.rs use them.

use serde::{Deserialize, Serialize};

use super::{WCtx, chrome};
use crate::config::Anchor;

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-wealth"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WealthWidget {
    pub anchor: Anchor,
}

impl Default for WealthWidget {
    fn default() -> Self {
        Self { anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 } }
    }
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct WealthState {}

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, _st: &mut WealthState, wm_id: u32) {
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, _w| {
        ui.label("Wealth");
    });
    chrome::apply(w, wm_id, &r);
}
