//! Port of upstream `overlay/WidgetDelveGrid.vue`: an 11×9 grid laid over the
//! Delve chart. Read-only, not movable; its size and position are derived from
//! the game's height (the chart is drawn relative to a 1080p reference). The
//! hotkey toggle is handled in `widgets::handle_event` ("delve-grid" action).

use egui::{Area, Context, CornerRadius, Id, Order, Pos2, Rect, Stroke, StrokeKind, vec2};
use rpt_m3::{theme, with_alpha};

use super::WCtx;

/// Grid geometry for an overlay of the given height (upstream `anchor` computed).
pub struct Geometry {
    pub width: f32,
    pub height: f32,
    pub top: f32,
    pub cell: f32,
}

pub fn geometry(wm_height: f32) -> Geometry {
    let height = (wm_height * 808.0 / 1080.0).round();
    let width = (height * 1030.0 / 808.0).round();
    let top = (wm_height * 67.0 / 1080.0).round();
    let cell = (wm_height * 97.0 / 1080.0).round();
    Geometry { width, height, top, cell }
}

pub fn show(ctx: &Context, w: &mut WCtx<'_>, wm_id: u32) {
    let screen = w.screen;
    let g = geometry(screen.height());
    let c = theme(ctx).colors;
    let outer = Rect::from_min_size(Pos2::new(screen.center().x - g.width / 2.0, screen.top() + g.top), vec2(g.width, g.height));

    Area::new(Id::unique(("rpt-delve-grid", wm_id)))
        .order(Order::Background)
        .fixed_pos(outer.min)
        .interactable(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(outer.size(), egui::Sense::hover());
            let p = ui.painter_at(rect);
            // Translucent scrim over the chart (upstream rgba(255,255,255,0.1)).
            p.rect_filled(rect, CornerRadius::same(rpt_m3::shape::SMALL), with_alpha(c.inverse_surface, 0.10));
            let grid_w = g.cell * 11.0;
            let grid_h = g.cell * 9.0;
            let origin = Pos2::new(rect.left() + (g.width - grid_w) / 2.0, rect.top() + (g.height - grid_h) / 2.0);
            let line = Stroke::new(1.0, with_alpha(c.outline, 0.85));
            for i in 0..9 {
                for j in 0..11 {
                    let cell = Rect::from_min_size(Pos2::new(origin.x + j as f32 * g.cell, origin.y + i as f32 * g.cell), vec2(g.cell, g.cell));
                    p.rect_stroke(cell.shrink(0.5), CornerRadius::ZERO, line, StrokeKind::Inside);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn geometry_1080p() {
        let g = super::geometry(1080.0);
        assert_eq!((g.width, g.height, g.top, g.cell), (1030.0, 808.0, 67.0, 97.0));
    }
}
