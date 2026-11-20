//! Net-worth snapshots (`<data_dir>/wealth/<league>.jsonl`) and the history chart.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, vec2};
use rpt_m3::{TypeRole, theme, with_alpha};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unix seconds.
    pub ts: i64,
    pub total_chaos: f64,
    /// Divine Orb price in chaos at that time.
    pub divine_chaos: f64,
    #[serde(default)]
    pub per_tab: BTreeMap<String, f64>,
}

impl Snapshot {
    pub fn total_divine(&self) -> f64 {
        if self.divine_chaos > 0.0 { self.total_chaos / self.divine_chaos } else { 0.0 }
    }
}

/// File-name-safe league id: `Hardcore Settlers` → `Hardcore_Settlers`.
pub fn sanitize_league(league: &str) -> String {
    let s: String = league.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "_".into() } else { s }
}

pub fn history_path(dir: &Path, league: &str) -> PathBuf {
    dir.join("wealth").join(format!("{}.jsonl", sanitize_league(league)))
}

pub fn append(dir: &Path, league: &str, snap: &Snapshot) -> std::io::Result<()> {
    let path = history_path(dir, league);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(snap).map_err(std::io::Error::other)?;
    writeln!(f, "{line}")
}

/// All snapshots, oldest first. Corrupt lines are skipped.
pub fn load(dir: &Path, league: &str) -> Vec<Snapshot> {
    let Ok(s) = std::fs::read_to_string(history_path(dir, league)) else { return Vec::new() };
    let mut v: Vec<Snapshot> = s.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect();
    v.sort_by_key(|s| s.ts);
    v
}

/// Change of the latest snapshot against the newest one at least `secs` older
/// (`secs = 0` → against the previous snapshot). `None` without enough history.
pub fn change_over(history: &[Snapshot], secs: i64) -> Option<f64> {
    let last = history.last()?;
    let base = if secs == 0 {
        history.len().checked_sub(2).map(|i| &history[i])
    } else {
        history.iter().rev().find(|s| s.ts <= last.ts - secs)
    }?;
    Some(last.total_chaos - base.total_chaos)
}

pub fn fmt_ts(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// Line chart of net worth. `divine` plots in divines instead of chaos.
pub fn chart(ui: &mut Ui, history: &[Snapshot], divine: bool, height: f32) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, egui::CornerRadius::same(rpt_m3::shape::MEDIUM), c.surface_container_low);
    let value = |s: &Snapshot| if divine { s.total_divine() } else { s.total_chaos };
    let font = th.font(TypeRole::LabelSmall);
    if history.len() < 2 {
        p.text(rect.center(), Align2::CENTER_CENTER, "Two or more snapshots are needed for a chart", th.font(TypeRole::BodyMedium), c.on_surface_variant);
        return;
    }

    let plot = Rect::from_min_max(rect.min + vec2(56.0, 12.0), rect.max - vec2(12.0, 24.0));
    let (t0, t1) = (history[0].ts as f64, history[history.len() - 1].ts as f64);
    let tspan = (t1 - t0).max(1.0);
    let (mut lo, mut hi) = history.iter().map(value).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let pad = ((hi - lo) * 0.1).max(hi.abs() * 0.02).max(1e-6);
    lo = (lo - pad).max(0.0);
    hi += pad;
    let to_screen = |ts: i64, v: f64| {
        Pos2::new(
            plot.left() + plot.width() * ((ts as f64 - t0) / tspan) as f32,
            plot.bottom() - plot.height() * ((v - lo) / (hi - lo)) as f32,
        )
    };

    // grid + y labels
    for i in 0..=4 {
        let v = lo + (hi - lo) * i as f64 / 4.0;
        let y = plot.bottom() - plot.height() * i as f32 / 4.0;
        p.hline(plot.x_range(), y, Stroke::new(1.0, with_alpha(c.outline_variant, 0.6)));
        p.text(Pos2::new(plot.left() - 6.0, y), Align2::RIGHT_CENTER, compact(v), font.clone(), c.on_surface_variant);
    }
    // x labels (first / last)
    p.text(Pos2::new(plot.left(), plot.bottom() + 4.0), Align2::LEFT_TOP, fmt_ts(history[0].ts), font.clone(), c.on_surface_variant);
    p.text(Pos2::new(plot.right(), plot.bottom() + 4.0), Align2::RIGHT_TOP, fmt_ts(history[history.len() - 1].ts), font.clone(), c.on_surface_variant);

    let pts: Vec<Pos2> = history.iter().map(|s| to_screen(s.ts, value(s))).collect();
    let fill = with_alpha(c.primary, 0.12);
    for w in pts.windows(2) {
        let quad = vec![w[0], w[1], Pos2::new(w[1].x, plot.bottom()), Pos2::new(w[0].x, plot.bottom())];
        p.add(Shape::convex_polygon(quad, fill, Stroke::NONE));
    }
    p.add(Shape::line(pts.clone(), Stroke::new(2.0, c.primary)));
    for pt in &pts {
        p.circle_filled(*pt, 2.5, c.primary);
    }

    // hover: nearest snapshot by x
    if let Some(hover) = resp.hover_pos().filter(|h| plot.expand(8.0).contains(*h)) {
        let (i, pt) = pts.iter().enumerate().min_by(|a, b| (a.1.x - hover.x).abs().total_cmp(&(b.1.x - hover.x).abs())).map(|(i, p)| (i, *p)).unwrap();
        p.vline(pt.x, plot.y_range(), Stroke::new(1.0, c.outline));
        p.circle(pt, 5.0, c.primary, Stroke::new(2.0, c.surface));
        let s = &history[i];
        let text = format!(
            "{}\n{} chaos · {:.2} div\n1 div = {:.0}c",
            fmt_ts(s.ts),
            fmt_int(s.total_chaos),
            s.total_divine(),
            s.divine_chaos
        );
        let g = p.layout_no_wrap(text, th.font(TypeRole::BodySmall), c.inverse_on_surface);
        let size = g.size() + vec2(16.0, 12.0);
        let mut at = pt + vec2(10.0, -size.y - 8.0);
        if at.x + size.x > rect.right() {
            at.x = pt.x - size.x - 10.0;
        }
        at.y = at.y.max(rect.top());
        let box_rect = Rect::from_min_size(at, size);
        p.rect_filled(box_rect, egui::CornerRadius::same(rpt_m3::shape::EXTRA_SMALL), c.inverse_surface);
        p.galley(box_rect.min + vec2(8.0, 6.0), g, Color32::PLACEHOLDER);
    }
}

/// 1234567 → "1.23M", 12345 → "12.3k".
pub fn compact(v: f64) -> String {
    let a = v.abs();
    if a >= 1e6 {
        format!("{:.2}M", v / 1e6)
    } else if a >= 1e4 {
        format!("{:.1}k", v / 1e3)
    } else if a >= 100.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Thousands separators: 1234567.8 → "1,234,568".
pub fn fmt_int(v: f64) -> String {
    let n = v.round() as i64;
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(ts: i64, total: f64) -> Snapshot {
        Snapshot { ts, total_chaos: total, divine_chaos: 200.0, per_tab: [("$".to_owned(), total)].into() }
    }

    #[test]
    fn round_trip_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("rpt-wealth-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let league = "Hardcore Settlers (PL1)";
        assert!(load(&dir, league).is_empty());
        let a = snap(1_000, 500.0);
        let b = snap(2_000, 800.0);
        append(&dir, league, &a).unwrap();
        append(&dir, league, &b).unwrap();
        // a corrupt line is ignored
        let path = history_path(&dir, league);
        assert!(path.ends_with("wealth/Hardcore_Settlers__PL1_.jsonl"));
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{oops\n").unwrap();
        assert_eq!(load(&dir, league), vec![a, b]);
        let line = std::fs::read_to_string(&path).unwrap();
        assert!(line.starts_with(r#"{"ts":1000,"total_chaos":500.0,"divine_chaos":200.0,"per_tab":{"$":500.0}}"#));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn changes() {
        let day = 86_400;
        let h = vec![snap(0, 100.0), snap(day * 6, 300.0), snap(day * 7, 350.0), snap(day * 7 + 60, 400.0)];
        assert_eq!(change_over(&h, 0), Some(50.0));
        assert_eq!(change_over(&h, day), Some(100.0));
        assert_eq!(change_over(&h, 7 * day), Some(300.0));
        assert_eq!(change_over(&h, 30 * day), None);
        assert_eq!(change_over(&h[..1], 0), None);
        assert_eq!(fmt_int(1234567.4), "1,234,567");
        assert_eq!(compact(12345.0), "12.3k");
    }
}
