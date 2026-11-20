//! TODO(ui-port): upstream `item-check/*` and `map-check/*`.
use super::WCtx;
use crate::host::HostEvent;
#[derive(Default)]
pub struct ItemCheckState;
pub fn show(_ctx: &egui::Context, _w: &mut WCtx<'_>, _st: &mut ItemCheckState, _wm_id: u32) {}
pub fn on_item_text(_w: &mut WCtx<'_>, _st: &mut ItemCheckState, _ev: &HostEvent) {}
pub fn on_game_log(_w: &mut WCtx<'_>, _st: &mut ItemCheckState, _lines: &[String]) {}
