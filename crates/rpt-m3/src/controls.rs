//! More M3 controls: slider, color swatch, list rows with trailing controls,
//! and a dialog scaffold (title bar + body + action row).

use std::ops::RangeInclusive;

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Layout, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui,
    Vec2, emath::easing, vec2,
};

use crate::scheme::with_alpha;
use crate::theme::{ICONS, TypeRole, shape, theme};

/// M3 continuous/discrete slider (4dp track, 20dp handle, value label while dragging).
///
/// `step = 0.0` → continuous. Returns the response (`changed()` when the value moved).
pub fn slider(ui: &mut Ui, value: &mut f64, range: RangeInclusive<f64>, step: f64, width: f32) -> Response {
    let th = theme(ui.ctx());
    let c = th.colors;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let track = Rect::from_min_max(Pos2::new(rect.left() + 10.0, rect.center().y - 2.0), Pos2::new(rect.right() - 10.0, rect.center().y + 2.0));
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
            let mut v = lo + t * (hi - lo);
            if step > 0.0 {
                v = lo + ((v - lo) / step).round() * step;
            }
            let v = v.clamp(lo, hi);
            if (v - *value).abs() > f64::EPSILON {
                *value = v;
                resp.mark_changed();
            }
        }
    }
    if resp.hovered() {
        let delta = ui.input(|i| {
            let mut d = 0.0;
            if i.key_pressed(egui::Key::ArrowRight) {
                d += 1.0;
            }
            if i.key_pressed(egui::Key::ArrowLeft) {
                d -= 1.0;
            }
            d
        });
        if delta != 0.0 {
            let s = if step > 0.0 { step } else { (hi - lo) / 100.0 };
            *value = (*value + delta * s).clamp(lo, hi);
            resp.mark_changed();
        }
    }
    let t = if hi > lo { ((*value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.0 };
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let x = track.left() + track.width() * t;
        let r = CornerRadius::same(2);
        p.rect_filled(track, r, c.secondary_container);
        p.rect_filled(Rect::from_min_max(track.min, Pos2::new(x, track.max.y)), r, c.primary);
        if step > 0.0 && (hi - lo) / step <= 20.0 {
            let n = ((hi - lo) / step).round() as i32;
            for i in 0..=n {
                let tx = track.left() + track.width() * i as f32 / n as f32;
                let col = if tx <= x { c.on_primary } else { c.on_secondary_container };
                p.circle_filled(Pos2::new(tx, track.center().y), 1.0, with_alpha(col, 0.6));
            }
        }
        let center = Pos2::new(x, rect.center().y);
        let active = resp.dragged() || resp.is_pointer_button_down_on();
        let o = if active { 0.12 } else if resp.hovered() { 0.08 } else { 0.0 };
        if o > 0.0 {
            p.circle_filled(center, 20.0, with_alpha(c.primary, o));
        }
        p.circle_filled(center, 10.0, c.primary);
        if active {
            let label = if step >= 1.0 { format!("{:.0}", *value) } else { format!("{:.2}", *value) };
            let g = p.layout_no_wrap(label, th.font(TypeRole::LabelMedium), c.inverse_on_surface);
            let bubble = Rect::from_center_size(Pos2::new(x, rect.top() - 14.0), vec2((g.size().x + 16.0).max(28.0), 24.0));
            p.rect_filled(bubble, CornerRadius::same(shape::FULL), c.inverse_surface);
            p.galley(bubble.center() - g.size() / 2.0, g, c.inverse_on_surface);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Circular color swatch (seed picker). Selected swatches get an outline ring and a check.
pub fn color_swatch(ui: &mut Ui, color: Color32, selected: bool, size: f32) -> Response {
    let c = theme(ui.ctx()).colors;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time_and_easing(resp.id, selected, 0.15, easing::cubic_out);
        let p = ui.painter();
        let r = size / 2.0;
        if resp.hovered() {
            p.circle_filled(rect.center(), r, with_alpha(c.on_surface, 0.08));
        }
        p.circle_filled(rect.center(), r - 4.0 - 2.0 * t, color);
        if t > 0.0 {
            p.circle_stroke(rect.center(), r - 1.0, Stroke::new(2.0, with_alpha(c.primary, t)));
            let fg = if color.r() as u32 + color.g() as u32 + color.b() as u32 > 384 { Color32::BLACK } else { Color32::WHITE };
            p.text(rect.center(), Align2::CENTER_CENTER, crate::icons::CHECK, FontId::new(size * 0.45, egui::FontFamily::Name(ICONS.into())), with_alpha(fg, t));
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A settings list row: optional leading icon, headline + optional supporting text,
/// and a trailing control laid out right-to-left. Min height 56dp (64 with supporting text).
pub fn setting_row<R>(
    ui: &mut Ui,
    icon: Option<&str>,
    headline: &str,
    supporting: Option<&str>,
    trailing: impl FnOnce(&mut Ui) -> R,
) -> R {
    let th = theme(ui.ctx());
    let c = th.colors;
    let min_h = if supporting.is_some() { 64.0 } else { 56.0 };
    ui.horizontal(|ui| {
        ui.set_min_height(min_h);
        ui.add_space(8.0);
        if let Some(i) = icon {
            ui.label(RichText::new(i).font(FontId::new(24.0, egui::FontFamily::Name(ICONS.into()))).color(c.on_surface_variant));
            ui.add_space(8.0);
        }
        // Trailing first (right-to-left) so the text column gets the remaining width.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(8.0);
            let r = trailing(ui);
            ui.with_layout(Layout::top_down(Align::Min).with_main_justify(false), |ui| {
                ui.add_space(((min_h - if supporting.is_some() { 40.0 } else { 20.0 }) / 2.0).max(0.0));
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(RichText::new(headline).font(th.font(TypeRole::BodyLarge)).color(c.on_surface)).wrap());
                if let Some(s) = supporting {
                    ui.add(egui::Label::new(RichText::new(s).font(th.font(TypeRole::BodyMedium)).color(c.on_surface_variant)).wrap());
                }
            });
            r
        })
        .inner
    })
    .inner
}

/// Small rounded "key cap" label, e.g. for showing a hotkey string.
pub fn key_cap(ui: &mut Ui, text: &str) -> Response {
    let th = theme(ui.ctx());
    let c = th.colors;
    let g = ui.painter().layout_no_wrap(text.to_owned(), th.font(TypeRole::LabelMedium), c.on_tertiary_container);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 12.0, 20.0), Sense::hover());
    ui.painter().rect(rect, CornerRadius::same(shape::EXTRA_SMALL), c.tertiary_container, Stroke::NONE, StrokeKind::Inside);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, c.on_tertiary_container);
    resp
}

/// Outlined, M3-styled numeric field (drag or type). Returns the response.
pub fn number_field(ui: &mut Ui, value: &mut f64, range: RangeInclusive<f64>, speed: f64, decimals: usize, suffix: &str) -> Response {
    let c = theme(ui.ctx()).colors;
    egui::Frame::new()
        .stroke(Stroke::new(1.0, c.outline))
        .corner_radius(CornerRadius::same(shape::EXTRA_SMALL))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.add(
                egui::DragValue::new(value)
                    .range(range)
                    .speed(speed)
                    .fixed_decimals(decimals)
                    .suffix(suffix),
            )
        })
        .inner
}
