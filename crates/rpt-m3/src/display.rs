//! Dense data-display pieces for overlay widgets: tonal badges, status dots,
//! sparklines, M3 menus / popovers, rich tooltips, an optional-number field
//! and a range slider (min/max handle over known bounds).

use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, Rect, Response, RichText, Sense, Shadow, Shape,
    Stroke, StrokeKind, Ui, Vec2, vec2,
};

use crate::scheme::with_alpha;
use crate::theme::{ICONS, TypeRole, shape, theme};

/// Small tonal pill with text (prices, counters, tags). `bg`/`fg` pick the tone.
pub fn tonal_badge(ui: &mut Ui, text: &str, bg: Color32, fg: Color32) -> Response {
    let th = theme(ui.ctx());
    let g = ui.painter().layout_no_wrap(text.to_owned(), th.font(TypeRole::LabelMedium), fg);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 12.0, (g.size().y + 4.0).max(18.0)), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, CornerRadius::same(shape::SMALL), bg);
        ui.painter().galley(rect.center() - g.size() / 2.0, g, fg);
    }
    resp
}

/// Filled circle used as an online/away/offline indicator.
pub fn status_dot(ui: &mut Ui, color: Color32, diameter: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::hover());
    ui.painter().circle_filled(rect.center(), diameter / 2.0, color);
    resp
}

/// Area sparkline drawn into a `size` box. `min`/`max` set the value range.
pub fn sparkline(ui: &mut Ui, points: &[f64], min: f64, max: f64, size: Vec2, line: Color32, fill: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if points.len() < 2 || !ui.is_rect_visible(rect) {
        return resp;
    }
    let span = if (max - min).abs() < f64::EPSILON { 1.0 } else { max - min };
    let pts: Vec<Pos2> = points
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.left() + rect.width() * i as f32 / (points.len() - 1) as f32;
            let t = ((v - min) / span).clamp(0.0, 1.0) as f32;
            Pos2::new(x, rect.bottom() - rect.height() * t)
        })
        .collect();
    let p = ui.painter();
    // fill as vertical quads (convex pieces), upstream "fillTo: end"
    for w in pts.windows(2) {
        let quad = vec![w[0], w[1], Pos2::new(w[1].x, rect.bottom()), Pos2::new(w[0].x, rect.bottom())];
        p.add(Shape::convex_polygon(quad, fill, Stroke::NONE));
    }
    p.add(Shape::line(pts, Stroke::new(1.25, line)));
    resp
}

/// M3 menu surface frame (surface-container, extra-small radius, elevation 2).
pub fn menu_frame(ctx: &egui::Context) -> Frame {
    let c = theme(ctx).colors;
    Frame::new()
        .fill(c.surface_container)
        .corner_radius(CornerRadius::same(shape::EXTRA_SMALL))
        .inner_margin(Margin::symmetric(0, 8))
        .shadow(Shadow { offset: [0, 3], blur: 8, spread: 0, color: with_alpha(c.shadow, 0.35) })
}

/// Shows an M3 menu/popover anchored below `anchor` while it is toggled open
/// (click the anchor to toggle, click outside to close).
pub fn popover<R>(anchor: &Response, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let frame = menu_frame(&anchor.ctx);
    egui::Popup::from_toggle_button_response(anchor)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(frame.inner_margin(Margin::same(12)))
        .show(body)
        .map(|r| r.inner)
}

/// One row inside an M3 menu: optional leading icon, label, optional check.
pub fn menu_item(ui: &mut Ui, icon: Option<&str>, label: &str, selected: bool) -> Response {
    let th = theme(ui.ctx());
    let c = th.colors;
    let font = th.font(TypeRole::LabelLarge);
    let g = ui.painter().layout_no_wrap(label.to_owned(), font, c.on_surface);
    let lead = if icon.is_some() { 32.0 } else { 0.0 };
    let w = (12.0 + lead + g.size().x + 12.0 + 28.0).max(ui.available_width().min(280.0));
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, CornerRadius::ZERO, c.secondary_container);
        }
        if resp.hovered() {
            p.rect_filled(rect, CornerRadius::ZERO, with_alpha(c.on_surface, 0.08));
        }
        let fg = if selected { c.on_secondary_container } else { c.on_surface };
        let mut x = rect.left() + 12.0;
        if let Some(i) = icon {
            p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, i, FontId::new(20.0, egui::FontFamily::Name(ICONS.into())), c.on_surface_variant);
            x += lead;
        }
        p.galley_with_override_text_color(Pos2::new(x, rect.center().y - g.size().y / 2.0), g, fg);
        if selected {
            p.text(
                Pos2::new(rect.right() - 12.0, rect.center().y),
                Align2::RIGHT_CENTER,
                crate::icons::CHECK,
                FontId::new(18.0, egui::FontFamily::Name(ICONS.into())),
                fg,
            );
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// M3 rich tooltip: a surface-container card with optional title shown on hover.
pub fn rich_tooltip(resp: Response, title: Option<&str>, body: &str) -> Response {
    let ctx = resp.ctx.clone();
    let th = theme(&ctx);
    let c = th.colors;
    resp.on_hover_ui(|ui| {
        ui.set_max_width(300.0);
        if let Some(t) = title {
            ui.label(RichText::new(t).font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
        }
        ui.label(RichText::new(body).font(th.font(TypeRole::BodyMedium)).color(c.on_surface_variant));
    })
}

/// Formats a number like JS `String(n)` for small UI fields (no trailing `.0`).
pub fn fmt_number(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// Parses user input for [`optional_number_field`]: empty → `Ok(None)`.
pub fn parse_optional_number(s: &str) -> Result<Option<f64>, ()> {
    let t = s.trim().replace(',', ".");
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<f64>().map(Some).map_err(|_| ())
}

/// Dense outlined numeric field whose value may be empty (`None`), like an
/// HTML `<input type=number>`. Text is selected when the field gains focus;
/// the caller can fill a default in that frame (check `gained_focus()`).
pub fn optional_number_field(ui: &mut Ui, id: egui::Id, value: &mut Option<f64>, placeholder: &str, width: f32) -> Response {
    let c = theme(ui.ctx()).colors;
    let buf_id = id.with("buf");
    let focused_before = ui.memory(|m| m.has_focus(id));
    let mut text = if focused_before {
        ui.data(|d| d.get_temp::<String>(buf_id)).unwrap_or_else(|| value.map(fmt_number).unwrap_or_default())
    } else {
        value.map(fmt_number).unwrap_or_default()
    };
    let before = text.clone();
    let out = Frame::new()
        .fill(c.surface_container_highest)
        .stroke(Stroke::new(1.0, if focused_before { c.primary } else { Color32::TRANSPARENT }))
        .corner_radius(CornerRadius::same(shape::EXTRA_SMALL))
        .inner_margin(Margin::symmetric(4, 2))
        .show(ui, |ui| {
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .hint_text(RichText::new(placeholder).color(with_alpha(c.on_surface_variant, 0.6)))
                .frame(Frame::NONE)
                .align(egui::Align2::CENTER_CENTER)
                .desired_width(width)
                .show(ui)
        })
        .inner;
    let mut resp = out.response.response.clone();
    if resp.gained_focus() {
        let mut state = out.state.clone();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::select_all_str(&text)));
        state.store(ui.ctx(), id);
        ui.data_mut(|d| d.remove::<String>(buf_id));
    } else if resp.has_focus() {
        if text != before {
            if let Ok(v) = parse_optional_number(&text) {
                if *value != v {
                    *value = v;
                    resp.mark_changed();
                }
            }
        }
        ui.data_mut(|d| d.insert_temp(buf_id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(buf_id));
    }
    resp
}

/// Which end of a [`range_slider`] is adjustable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeEdge {
    Min,
    Max,
}

/// Range slider over fixed `bounds` (upstream `StatRollSlider`): shows a fill
/// for the selected `min`/`max` range and a tick at `tick` (the item's roll).
/// Only one-sided ranges are draggable; the new value is reported on release.
/// `quantize(value, edge)` rounds the raw pointer value.
pub fn range_slider(
    ui: &mut Ui,
    id: egui::Id,
    bounds: (f64, f64),
    tick: f64,
    min: Option<f64>,
    max: Option<f64>,
    width: f32,
    quantize: impl Fn(f64, RangeEdge) -> f64,
) -> Option<(RangeEdge, f64)> {
    let th = theme(ui.ctx());
    let c = th.colors;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click_and_drag());
    let (lo, hi) = bounds;
    let span = if (hi - lo).abs() < f64::EPSILON { 1.0 } else { hi - lo };
    let frac = |v: f64| ((v - lo) / span).clamp(0.0, 1.0) as f32;

    let dirty: Option<f64> = ui.data(|d| d.get_temp(id));
    let edge = match (min, max) {
        (Some(_), None) => Some(RangeEdge::Min),
        (None, Some(_)) => Some(RangeEdge::Max),
        _ => None,
    };
    let mut committed = None;
    if let (Some(edge), Some(p)) = (edge, resp.interact_pointer_pos()) {
        if resp.dragged() || resp.is_pointer_button_down_on() || resp.clicked() {
            let k = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
            let v = quantize(lo + span * k, edge);
            ui.data_mut(|d| d.insert_temp(id, v));
        }
    }
    if (resp.drag_stopped() || resp.clicked()) && edge.is_some() {
        if let Some(v) = ui.data(|d| d.get_temp::<f64>(id)) {
            committed = Some((edge.unwrap_or(RangeEdge::Min), v));
        }
        ui.data_mut(|d| d.remove::<f64>(id));
    }
    let (min, max) = match (edge, dirty) {
        (Some(RangeEdge::Min), Some(v)) => (Some(v), None),
        (Some(RangeEdge::Max), Some(v)) => (None, Some(v)),
        _ => (min, max),
    };

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let r = CornerRadius::same(shape::EXTRA_SMALL);
        p.rect_filled(rect, r, c.surface_container_highest);
        let inner = rect.shrink(2.0);
        let x_at = |v: f64| inner.left() + inner.width() * frac(v);
        let fill = match (min, max) {
            (Some(a), None) => Some(Rect::from_x_y_ranges(x_at(a).min(inner.right() - 12.0)..=inner.right(), inner.y_range())),
            (None, Some(b)) => Some(Rect::from_x_y_ranges(inner.left()..=x_at(b).max(inner.left() + 12.0), inner.y_range())),
            (Some(a), Some(b)) if a <= b => Some(Rect::from_x_y_ranges(x_at(a)..=x_at(b).max(x_at(a) + 12.0).min(inner.right()), inner.y_range())),
            _ => None,
        };
        if let Some(f) = fill {
            p.rect_filled(f, r, with_alpha(c.primary, 0.55));
        }
        let tx = x_at(tick).clamp(inner.left() + 1.0, inner.right() - 1.0);
        p.line_segment([Pos2::new(tx, rect.top()), Pos2::new(tx, rect.bottom())], Stroke::new(2.0, c.on_surface));
        let font = th.font(TypeRole::LabelSmall);
        let incl_min = matches!((min, max), (None, Some(_))) || min.is_some_and(|m| m <= lo);
        let incl_max = matches!((min, max), (Some(_), None)) || max.is_some_and(|m| m >= hi);
        let col = |incl: bool| if incl { c.on_primary_container } else { c.on_surface_variant };
        p.text(Pos2::new(rect.left() + 4.0, rect.center().y), Align2::LEFT_CENTER, fmt_number(lo), font.clone(), col(incl_min));
        p.text(Pos2::new(rect.right() - 4.0, rect.center().y), Align2::RIGHT_CENTER, fmt_number(hi), font.clone(), col(incl_max));
        if let Some(v) = dirty {
            let g = p.layout_no_wrap(fmt_number(v), th.font(TypeRole::LabelMedium), c.inverse_on_surface);
            let bubble = Rect::from_center_size(Pos2::new(x_at(v), rect.top() - 12.0), vec2((g.size().x + 12.0).max(28.0), 20.0));
            p.rect(bubble, CornerRadius::same(shape::FULL), c.inverse_surface, Stroke::NONE, StrokeKind::Inside);
            p.galley(bubble.center() - g.size() / 2.0, g, c.inverse_on_surface);
        }
    }
    if edge.is_some() {
        resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    }
    committed
}

/// Custom-color roles for a "design color" (e.g. item rarity, decision
/// colors) harmonized with the theme's `source` (M3 "custom colors").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CustomColor {
    /// Text/icon color on surfaces (tone 80 dark / 40 light).
    pub accent: Color32,
    pub container: Color32,
    pub on_container: Color32,
}

/// Harmonize `design` toward `source` (hue shifted ≤15°) and derive tonal roles.
pub fn custom_color(design: Color32, source: Color32, dark: bool) -> CustomColor {
    use material_colors::color::Rgb;
    use material_colors::hct::Hct;
    let h = material_colors::blend::harmonize(Rgb::new(design.r(), design.g(), design.b()), Rgb::new(source.r(), source.g(), source.b()));
    let hct = Hct::new(h);
    let (hue, chroma) = (hct.get_hue(), hct.get_chroma());
    let tone = |t: f64, c: f64| {
        let rgb: Rgb = Hct::from(hue, c, t).into();
        Color32::from_rgb(rgb.red, rgb.green, rgb.blue)
    };
    let c_acc = chroma.max(16.0);
    let c_cont = chroma.clamp(8.0, 36.0);
    if dark {
        CustomColor { accent: tone(80.0, c_acc), container: tone(30.0, c_cont), on_container: tone(90.0, c_cont) }
    } else {
        CustomColor { accent: tone(40.0, c_acc), container: tone(90.0, c_cont), on_container: tone(10.0, c_cont) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(fmt_number(12.0), "12");
        assert_eq!(fmt_number(1.25), "1.25");
        assert_eq!(fmt_number(-0.5), "-0.5");
        assert_eq!(parse_optional_number(" "), Ok(None));
        assert_eq!(parse_optional_number("1,5"), Ok(Some(1.5)));
        assert!(parse_optional_number("x").is_err());
    }

    #[test]
    fn custom_colors_keep_hue_family() {
        let source = Color32::from_rgb(0x67, 0x50, 0xA4);
        let red = custom_color(Color32::from_rgb(0xD2, 0x00, 0x00), source, true);
        let blue = custom_color(Color32::from_rgb(0x88, 0x88, 0xFF), source, true);
        assert!(red.accent.r() > red.accent.b());
        assert!(blue.accent.b() > blue.accent.r());
        assert!(red.container.r() < red.accent.r());
    }
}
