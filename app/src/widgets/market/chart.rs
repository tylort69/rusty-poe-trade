//! Reusable, self-painted line chart (egui_plot does not support egui 0.37):
//! nice-number axes, optional day-based x axis, area fill, hover crosshair and
//! tooltip, M3 colors.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, vec2};
use rpt_m3::{TypeRole, theme, with_alpha};

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    /// `[x, y]`, sorted by x.
    pub points: Vec<[f64; 2]>,
    /// `None` = theme primary.
    pub color: Option<Color32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XAxis {
    /// x is days since the Unix epoch; labels are dates.
    #[default]
    Days,
    #[allow(dead_code)] // for other callers of `line_chart`
    Number,
}

pub struct ChartOpts {
    pub height: f32,
    pub x_axis: XAxis,
    /// Appended to y labels and tooltip values (e.g. "c", " div").
    pub y_suffix: String,
    /// Area fill under the first series.
    pub fill: bool,
    pub empty_text: String,
}

impl Default for ChartOpts {
    fn default() -> Self {
        Self { height: 220.0, x_axis: XAxis::Days, y_suffix: String::new(), fill: true, empty_text: "No data".into() }
    }
}

fn nice_num(range: f64, round: bool) -> f64 {
    if range <= 0.0 || !range.is_finite() {
        return 1.0;
    }
    let exp = range.log10().floor();
    let f = range / 10f64.powf(exp);
    let nf = if round {
        if f < 1.5 {
            1.0
        } else if f < 3.0 {
            2.0
        } else if f < 7.0 {
            5.0
        } else {
            10.0
        }
    } else if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    nf * 10f64.powf(exp)
}

/// "Nice" tick values (1/2/5 × 10ⁿ steps) spanning `[min, max]`; the first tick
/// is ≤ min and the last ≥ max. Returns at least two ticks.
pub fn nice_ticks(min: f64, max: f64, max_ticks: usize) -> Vec<f64> {
    let (mut min, mut max) = if min <= max { (min, max) } else { (max, min) };
    if !min.is_finite() || !max.is_finite() {
        return vec![0.0, 1.0];
    }
    if (max - min).abs() < f64::EPSILON * max.abs().max(1.0) {
        let pad = if min == 0.0 { 1.0 } else { min.abs() * 0.1 };
        min -= pad;
        max += pad;
    }
    let n = max_ticks.max(2);
    let range = nice_num(max - min, false);
    let step = nice_num(range / (n - 1) as f64, true);
    let lo = (min / step).floor() * step;
    let hi = (max / step).ceil() * step;
    let count = ((hi - lo) / step).round() as usize;
    (0..=count).map(|i| clean(lo + i as f64 * step, step)).collect()
}

/// Remove float noise (0.30000000000000004 → 0.3).
fn clean(v: f64, step: f64) -> f64 {
    let decimals = (-step.log10().floor()).max(0.0) as i32 + 1;
    let p = 10f64.powi(decimals);
    (v * p).round() / p
}

/// Day ticks with steps of 1, 2, 3, 7, 14, 30, 60, 90, 180 or 365 days.
pub fn day_ticks(min_day: f64, max_day: f64, max_ticks: usize) -> Vec<f64> {
    const STEPS: [i64; 10] = [1, 2, 3, 7, 14, 30, 60, 90, 180, 365];
    let (lo, hi) = (min_day.ceil() as i64, max_day.floor() as i64);
    if hi < lo {
        return vec![];
    }
    let span = hi - lo;
    let step = STEPS.iter().copied().find(|s| span / s < max_ticks.max(1) as i64).unwrap_or(365);
    let first = lo + (step - lo.rem_euclid(step)) % step;
    (0..).map(|i| first + i * step).take_while(|d| *d <= hi).map(|d| d as f64).collect()
}

pub fn fmt_day(day: f64) -> String {
    chrono::DateTime::from_timestamp(day as i64 * 86_400, 0).map(|d| d.format("%b %-d").to_string()).unwrap_or_default()
}

/// Compact axis label: 1.2k, 3.5M, 0.25 …
pub fn fmt_axis(v: f64) -> String {
    let a = v.abs();
    let s = if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e4 {
        format!("{:.0}k", v / 1e3)
    } else if a >= 1e3 {
        format!("{:.1}k", v / 1e3)
    } else if a >= 10.0 || a == 0.0 {
        format!("{:.0}", v)
    } else if a >= 1.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    };
    s.replace(".0k", "k").replace(".0M", "M")
}

fn fmt_value(v: f64) -> String {
    if v.abs() >= 100.0 { format!("{:.0}", v) } else if v.abs() >= 1.0 { format!("{:.2}", v) } else { format!("{:.4}", v) }
}

/// Paint a line chart filling the available width. Returns the hover response.
pub fn line_chart(ui: &mut Ui, series: &[Series], opts: &ChartOpts) -> Response {
    let th = theme(ui.ctx());
    let c = th.colors;
    let width = ui.available_width().max(120.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, opts.height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter_at(rect.expand(2.0));
    let label_font = th.font(TypeRole::LabelSmall);
    p.rect(rect, CornerRadius::same(rpt_m3::shape::MEDIUM), c.surface_container_low, Stroke::NONE, StrokeKind::Inside);

    let pts = series.iter().flat_map(|s| s.points.iter()).filter(|p| p[0].is_finite() && p[1].is_finite());
    let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for q in pts {
        x0 = x0.min(q[0]);
        x1 = x1.max(q[0]);
        y0 = y0.min(q[1]);
        y1 = y1.max(q[1]);
    }
    if !x0.is_finite() {
        p.text(rect.center(), Align2::CENTER_CENTER, &opts.empty_text, th.font(TypeRole::BodyMedium), c.on_surface_variant);
        return resp;
    }
    if x1 - x0 < 1e-9 {
        x0 -= 1.0;
        x1 += 1.0;
    }
    let y_ticks = nice_ticks(y0.min(if y0 >= 0.0 && y0 < (y1 - y0) * 0.5 { 0.0 } else { y0 }), y1, 5);
    let (y0, y1) = (y_ticks[0], *y_ticks.last().unwrap_or(&y1));
    let y_labels: Vec<String> = y_ticks.iter().map(|v| format!("{}{}", fmt_axis(*v), opts.y_suffix)).collect();
    let label_w = y_labels.iter().map(|l| p.layout_no_wrap(l.clone(), label_font.clone(), c.on_surface_variant).size().x).fold(0.0, f32::max);
    let plot = Rect::from_min_max(Pos2::new(rect.left() + label_w + 14.0, rect.top() + 10.0), Pos2::new(rect.right() - 12.0, rect.bottom() - 24.0));
    let max_x_ticks = ((plot.width() / 70.0) as usize).max(2);
    let x_ticks = match opts.x_axis {
        XAxis::Days => day_ticks(x0, x1, max_x_ticks),
        XAxis::Number => nice_ticks(x0, x1, max_x_ticks).into_iter().filter(|v| *v >= x0 && *v <= x1).collect(),
    };
    let to_screen = |x: f64, y: f64| -> Pos2 {
        let tx = ((x - x0) / (x1 - x0)) as f32;
        let ty = ((y - y0) / (y1 - y0).max(f64::EPSILON)) as f32;
        Pos2::new(plot.left() + tx * plot.width(), plot.bottom() - ty * plot.height())
    };
    let grid = Stroke::new(1.0, with_alpha(c.outline_variant, 0.6));
    for (v, l) in y_ticks.iter().zip(&y_labels) {
        let y = to_screen(x0, *v).y;
        p.hline(plot.x_range(), y, grid);
        p.text(Pos2::new(plot.left() - 6.0, y), Align2::RIGHT_CENTER, l, label_font.clone(), c.on_surface_variant);
    }
    for v in &x_ticks {
        let x = to_screen(*v, y0).x;
        p.vline(x, plot.y_range(), Stroke::new(1.0, with_alpha(c.outline_variant, 0.3)));
        let text = match opts.x_axis {
            XAxis::Days => fmt_day(*v),
            XAxis::Number => fmt_axis(*v),
        };
        p.text(Pos2::new(x, plot.bottom() + 4.0), Align2::CENTER_TOP, text, label_font.clone(), c.on_surface_variant);
    }
    p.line_segment([plot.left_bottom(), plot.right_bottom()], Stroke::new(1.0, c.outline_variant));

    let palette = [c.primary, c.tertiary, c.secondary, c.error];
    let colors: Vec<Color32> = series.iter().enumerate().map(|(i, s)| s.color.unwrap_or(palette[i % palette.len()])).collect();
    for (si, s) in series.iter().enumerate() {
        let sp: Vec<Pos2> = s.points.iter().filter(|q| q[0].is_finite() && q[1].is_finite()).map(|q| to_screen(q[0], q[1])).collect();
        if si == 0 && opts.fill && sp.len() >= 2 {
            let fill = with_alpha(colors[si], 0.14);
            for w in sp.windows(2) {
                let quad = vec![w[0], w[1], Pos2::new(w[1].x, plot.bottom()), Pos2::new(w[0].x, plot.bottom())];
                p.add(Shape::convex_polygon(quad, fill, Stroke::NONE));
            }
        }
        if sp.len() >= 2 {
            p.add(Shape::line(sp.clone(), Stroke::new(2.0, colors[si])));
        }
        if sp.len() <= 40 {
            for q in &sp {
                p.circle_filled(*q, 2.5, colors[si]);
            }
        }
    }

    // hover: crosshair at the nearest x, tooltip with every series' value there
    if let Some(hp) = resp.hover_pos().filter(|hp| plot.expand(4.0).contains(*hp)) {
        let hx = x0 + ((hp.x - plot.left()) / plot.width()) as f64 * (x1 - x0);
        let nearest = series.iter().flat_map(|s| s.points.iter().map(|q| q[0])).min_by(|a, b| (a - hx).abs().total_cmp(&(b - hx).abs()));
        if let Some(nx) = nearest {
            let sx = to_screen(nx, y0).x;
            p.vline(sx, plot.y_range(), Stroke::new(1.0, c.outline));
            let mut lines = vec![match opts.x_axis {
                XAxis::Days => fmt_day(nx),
                XAxis::Number => fmt_axis(nx),
            }];
            for (si, s) in series.iter().enumerate() {
                if let Some(q) = s.points.iter().find(|q| (q[0] - nx).abs() < 1e-9) {
                    let sp = to_screen(q[0], q[1]);
                    p.hline(plot.x_range(), sp.y, Stroke::new(1.0, with_alpha(c.outline, 0.5)));
                    p.circle(sp, 4.5, colors[si], Stroke::new(2.0, c.surface));
                    let name = if series.len() > 1 { format!("{}: ", s.name) } else { String::new() };
                    lines.push(format!("{name}{}{}", fmt_value(q[1]), opts.y_suffix));
                }
            }
            tooltip_box(ui, &p, hp, plot, &lines, th.font(TypeRole::BodySmall));
        }
    }
    resp
}

fn tooltip_box(ui: &Ui, p: &egui::Painter, at: Pos2, plot: Rect, lines: &[String], font: FontId) {
    let c = theme(ui.ctx()).colors;
    let galley = p.layout_no_wrap(lines.join("\n"), font, c.inverse_on_surface);
    let size = galley.size() + vec2(16.0, 12.0);
    let mut min = at + vec2(12.0, -size.y - 8.0);
    if min.x + size.x > plot.right() {
        min.x = at.x - size.x - 12.0;
    }
    if min.y < plot.top() {
        min.y = at.y + 12.0;
    }
    let r = Rect::from_min_size(min, size);
    p.rect_filled(r, CornerRadius::same(rpt_m3::shape::EXTRA_SMALL), c.inverse_surface);
    p.galley(r.min + vec2(8.0, 6.0), galley, c.inverse_on_surface);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_nice() {
        assert_eq!(nice_ticks(0.0, 100.0, 5), vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        assert_eq!(nice_ticks(3.0, 97.0, 5), vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        assert_eq!(nice_ticks(0.12, 0.47, 5), vec![0.1, 0.2, 0.3, 0.4, 0.5]);
        assert_eq!(nice_ticks(151.7, 217.6, 5), vec![140.0, 160.0, 180.0, 200.0, 220.0]);
        let t = nice_ticks(-7.0, 13.0, 5);
        assert!(t[0] <= -7.0 && *t.last().unwrap() >= 13.0 && t.contains(&0.0));
        // flat and reversed input
        let t = nice_ticks(5.0, 5.0, 5);
        assert!(t.len() >= 2 && t[0] <= 5.0 && *t.last().unwrap() >= 5.0);
        assert_eq!(nice_ticks(100.0, 0.0, 5), nice_ticks(0.0, 100.0, 5));
        assert_eq!(nice_ticks(f64::NAN, 1.0, 5), vec![0.0, 1.0]);
        for (lo, hi) in [(0.001, 0.009), (1.0, 1e6), (12345.0, 12999.0)] {
            let t = nice_ticks(lo, hi, 6);
            assert!(t.len() >= 2 && t.len() <= 12, "{lo}..{hi}: {t:?}");
            let steps: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).collect();
            assert!(steps.iter().all(|s| (s - steps[0]).abs() < steps[0] * 1e-6), "{t:?}");
        }
    }

    #[test]
    fn day_ticks_fit() {
        assert_eq!(day_ticks(10.0, 16.0, 10), vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0]);
        let t = day_ticks(0.0, 60.0, 5);
        assert!(t.len() <= 5 && t.len() >= 2, "{t:?}");
        assert!(t.windows(2).all(|w| w[1] - w[0] == 14.0), "{t:?}");
        assert_eq!(day_ticks(0.0, 400.0, 6).windows(2).next().map(|w| w[1] - w[0]), Some(90.0));
        assert!(day_ticks(5.5, 5.6, 5).is_empty());
        assert_eq!(fmt_day(0.0), "Jan 1");
    }

    /// Headless paint with the pointer over the plot (crosshair + tooltip path), and empty data.
    #[test]
    fn paints_with_hover() {
        let ctx = egui::Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0));
        let series = vec![
            Series { name: "a".into(), points: (0..30).map(|d| [20_000.0 + d as f64, 100.0 + d as f64 * 3.0]).collect(), color: None },
            Series { name: "b".into(), points: vec![[20_010.0, 120.0], [20_020.0, 90.0]], color: None },
        ];
        let mut shapes = 0;
        for frame in 0..3 {
            let mut input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            input.events.push(egui::Event::PointerMoved(Pos2::new(400.0, 120.0 + frame as f32)));
            let mut out = ctx.run_ui(input, |ui| {
                line_chart(ui, &series, &ChartOpts { height: 300.0, ..Default::default() });
                line_chart(ui, &[], &ChartOpts { height: 60.0, x_axis: XAxis::Number, ..Default::default() });
            });
            out.textures_delta.clear();
            shapes = out.shapes.len();
        }
        assert!(shapes > 0);
    }

    #[test]
    fn axis_labels() {
        assert_eq!(fmt_axis(1500.0), "1.5k");
        assert_eq!(fmt_axis(20000.0), "20k");
        assert_eq!(fmt_axis(2_000_000.0), "2M");
        assert_eq!(fmt_axis(0.25), "0.25");
        assert_eq!(fmt_axis(0.0), "0");
    }
}
