//! Market browser (saved searches, favorites), price-history charts and 7-day economy view.
//!
//! SCAFFOLD: replace freely. Keep the `MarketWidget` name, its `anchor` field, the
//! `MarketState` name and the `show` signature: config.rs / widgets/mod.rs use them.

use serde::{Deserialize, Serialize};

use super::{WCtx, chrome};
use crate::config::Anchor;

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-market"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MarketWidget {
    pub anchor: Anchor,
}

impl Default for MarketWidget {
    fn default() -> Self {
        Self { anchor: Anchor { pos: "cc".into(), x: 50.0, y: 50.0 } }
    }
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct MarketState {}

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, _st: &mut MarketState, wm_id: u32) {
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, _w| {
        ui.label("Market");
    });
    chrome::apply(w, wm_id, &r);
}
