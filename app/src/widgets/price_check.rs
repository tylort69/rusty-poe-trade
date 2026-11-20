//! TODO(ui-port): upstream `price-check/*`.
use super::WCtx;
use crate::host::HostEvent;
#[derive(Default)]
pub struct PriceCheckState;
pub fn show(_ctx: &egui::Context, _w: &mut WCtx<'_>, _st: &mut PriceCheckState, _wm_id: u32) {}
pub fn on_item_text(_w: &mut WCtx<'_>, _st: &mut PriceCheckState, _ev: &HostEvent) {}
