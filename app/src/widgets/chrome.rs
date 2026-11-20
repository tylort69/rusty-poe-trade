//! Shared widget chrome. Port of upstream `overlay/Widget.vue`:
//! anchor-based positioning (percent of overlay + 9 pivots), drag-to-move
//! while the overlay is active, and the hover action strip (hide / remove).

use egui::{Align2, Area, Context, Id, Order, Pos2, Rect, Sense, Ui, Vec2};
use rpt_m3::{IconButton, icons, shape, theme};

use super::WCtx;
use crate::config::Anchor;

pub struct ChromeOpts {
    /// Can be removed (multi-instance widgets).
    pub removable: bool,
    pub hideable: bool,
    /// Draw the M3 surface container (false for transparent widgets like image strips).
    pub surface: bool,
}

impl Default for ChromeOpts {
    fn default() -> Self {
        Self { removable: true, hideable: true, surface: true }
    }
}

pub fn pivot(pos: &str) -> Align2 {
    match pos {
        "tc" => Align2::CENTER_TOP,
        "tr" => Align2::RIGHT_TOP,
        "cr" => Align2::RIGHT_CENTER,
        "br" => Align2::RIGHT_BOTTOM,
        "bc" => Align2::CENTER_BOTTOM,
        "bl" => Align2::LEFT_BOTTOM,
        "cl" => Align2::LEFT_CENTER,
        "cc" => Align2::CENTER_CENTER,
        _ => Align2::LEFT_TOP,
    }
}

/// Max size in percent, per upstream `widgetPosition`.
fn max_size(a: &Anchor, screen: Rect) -> Vec2 {
    let (x, y) = (a.x, a.y);
    let (w, h) = match a.pos.as_str() {
        "tc" => (2.0 * (100.0 - x).min(x), 100.0 - y),
        "tr" => (x, 100.0 - y),
        "cr" => (x, 2.0 * (100.0 - y).min(y)),
        "br" => (x, y),
        "bc" => (2.0 * (100.0 - x).min(x), y),
        "bl" => (100.0 - x, y),
        "cl" => (100.0 - x, 2.0 * (100.0 - y).min(y)),
        "cc" => (2.0 * (100.0 - x).min(x), 2.0 * (100.0 - y).min(y)),
        _ => (100.0 - x, 100.0 - y),
    };
    Vec2::new(screen.width() * w / 100.0, screen.height() * h / 100.0)
}

/// 16:9 — the widest aspect the game's own HUD is designed for.
pub const WIDE_ASPECT: f32 = 16.0 / 9.0;
/// "auto" only centers when the game is noticeably wider than 16:9, so
/// rounding (e.g. 1366×768, 2560×1439 after a title bar) never kicks in.
pub const AUTO_EPSILON: f32 = 0.02;

/// The region anchored widgets are laid out in (Rusty-only, `rpt.widgetArea`).
///
/// On ultrawide (21:9, 32:9) games the HUD and the stash/inventory panels sit
/// at the real screen edges, but widget anchors expressed in percent of the
/// full width end up far in the corners. "auto" (when wider than 16:9) and
/// "center-16:9" (whenever wider) lay anchored widgets out in a centered 16:9
/// region instead; "full" uses the whole game. Narrower/portrait screens are
/// never letterboxed. Price-check/item-check placement next to the in-game
/// panels keeps using the real game rect (`WCtx::screen`).
pub fn widget_area(screen: Rect, mode: &str) -> Rect {
    let (w, h) = (screen.width(), screen.height());
    if w <= 0.0 || h <= 0.0 {
        return screen;
    }
    let aspect = w / h;
    let center = match mode {
        crate::config::WIDGET_AREA_FULL => false,
        crate::config::WIDGET_AREA_CENTER => aspect > WIDE_ASPECT,
        _ => aspect > WIDE_ASPECT + AUTO_EPSILON,
    };
    if !center {
        return screen;
    }
    let width = h * WIDE_ASPECT;
    Rect::from_center_size(screen.center(), Vec2::new(width, h))
}

pub fn anchor_pos(a: &Anchor, screen: Rect) -> Pos2 {
    Pos2::new(screen.left() + screen.width() * a.x / 100.0, screen.top() + screen.height() * a.y / 100.0)
}

/// Result of drawing chrome around a widget body.
pub struct ChromeResult<R> {
    pub inner: Option<R>,
    pub rect: Rect,
    pub hide: bool,
    pub remove: bool,
}

/// Draw an anchored widget. `anchor` is read from and written back to the config.
pub fn anchored<R>(
    ctx: &Context,
    w: &mut WCtx<'_>,
    wm_id: u32,
    opts: ChromeOpts,
    body: impl FnOnce(&mut Ui, &mut WCtx<'_>) -> R,
) -> ChromeResult<R> {
    // Anchors (and drag deltas) are percentages of the widget area, not of the
    // full (possibly ultrawide) game.
    let screen = widget_area(w.screen, &w.cfg.rpt.widget_area);
    let Some(widget) = w.cfg.widgets.iter_mut().find(|x| x.wm_id == wm_id) else {
        return ChromeResult { inner: None, rect: Rect::NOTHING, hide: false, remove: false };
    };
    let z = match widget.wm_zorder {
        Some(crate::config::ZOrder::Order(z)) => z,
        _ => 0,
    };
    let Some(anchor) = widget.kind.anchor_mut().cloned() else {
        return ChromeResult { inner: None, rect: Rect::NOTHING, hide: false, remove: false };
    };
    let active = w.st.active;
    let th = theme(ctx);
    let c = th.colors;

    let id = Id::unique(("rpt-widget", wm_id));
    let pos = anchor_pos(&anchor, screen);
    let max = max_size(&anchor, screen);
    let mut hide = false;
    let mut remove = false;

    let width_id = id.with("last-width");
    let last_w: f32 = ctx.data(|d| d.get_temp(width_id)).unwrap_or(0.0);
    let resp = Area::new(id)
        .order(if z > 0 { Order::Middle } else { Order::Background })
        .pivot(pivot(&anchor.pos))
        .fixed_pos(pos)
        .constrain_to(screen)
        .interactable(active)
        .show(ctx, |ui| {
            ui.set_max_size(max);
            let frame = if opts.surface {
                rpt_m3::surface(ui, c.surface_container, shape::LARGE).inner_margin(egui::Margin::same(8))
            } else {
                egui::Frame::NONE
            };
            frame
                .show(ui, |ui| {
                    if active {
                        // Drag strip + actions row (upstream move handle & action strip).
                        // The whole strip moves the widget, not just the grip icon.
                        let mut drags: Vec<egui::Response> = Vec::new();
                        ui.horizontal(|ui| {
                            let (r, grip) = ui.allocate_exact_size(Vec2::new(24.0, 24.0), Sense::drag());
                            ui.painter().text(
                                r.center(),
                                Align2::CENTER_CENTER,
                                icons::DRAG_INDICATOR,
                                egui::FontId::new(18.0, egui::FontFamily::Name(rpt_m3::theme::ICONS.into())),
                                c.on_surface_variant,
                            );
                            drags.push(grip);
                            // Left to right on purpose: a right-to-left layout fills the
                            // whole available width, which stretched every widget to its
                            // maximum size so it hugged the screen edge and couldn't move.
                            let n_buttons = opts.removable as usize + opts.hideable as usize;
                            let sp = ui.spacing().item_spacing.x;
                            let fixed = 16.0 + 24.0 + n_buttons as f32 * (28.0 + sp) + sp;
                            let want = (last_w - fixed).max(24.0);
                            let (strip, resp) = ui.allocate_exact_size(Vec2::new(want, 24.0), Sense::drag());
                            if resp.hovered() || resp.dragged() {
                                ui.painter().text(
                                    strip.left_center(),
                                    Align2::LEFT_CENTER,
                                    "drag to move",
                                    egui::FontId::proportional(11.0),
                                    c.on_surface_variant,
                                );
                            }
                            drags.push(resp);
                            if opts.hideable && ui.add(IconButton::new(icons::VISIBILITY_OFF).size(28.0).tooltip(w.i18n.t("widget.hide"))).clicked() {
                                hide = true;
                            }
                            if opts.removable
                                && ui.add(IconButton::new(icons::DELETE).size(28.0).tooltip(w.i18n.t("widget.delete"))).double_clicked()
                            {
                                remove = true;
                            }
                        });
                        for drag in drags {
                            let drag = drag.on_hover_cursor(egui::CursorIcon::Grab);
                            if drag.dragged() {
                                let d = drag.drag_delta();
                                if let Some(a) = w.cfg.widgets.iter_mut().find(|x| x.wm_id == wm_id).and_then(|x| x.kind.anchor_mut()) {
                                    a.x = (a.x + d.x / screen.width() * 100.0).clamp(0.0, 100.0);
                                    a.y = (a.y + d.y / screen.height() * 100.0).clamp(0.0, 100.0);
                                }
                            }
                            if drag.drag_stopped() {
                                w.save_requested = true;
                            }
                            if drag.drag_started() {
                                crate::wm::bring_to_top(w.cfg, wm_id);
                            }
                        }
                    }
                    body(ui, w)
                })
                .inner
        });

    ctx.data_mut(|d| d.insert_temp(width_id, resp.response.rect.width()));
    ChromeResult { inner: Some(resp.inner), rect: resp.response.rect, hide, remove }
}

/// Apply chrome hide/remove requests.
pub fn apply<R>(w: &mut WCtx<'_>, wm_id: u32, r: &ChromeResult<R>) {
    if r.hide {
        crate::wm::hide(w.cfg, wm_id);
        w.save_requested = true;
    }
    if r.remove {
        crate::wm::remove(w.cfg, wm_id);
        w.save_requested = true;
    }
}

/// Centered, exclusive, modal-ish window (settings, price check fallback).
pub fn exclusive_window<R>(ctx: &Context, id: &str, size: Vec2, pos: Option<Pos2>, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let c = theme(ctx).colors;
    let mut area = Area::new(Id::unique(("rpt-exclusive", id))).order(Order::Foreground);
    area = match pos {
        Some(p) => area.fixed_pos(p),
        None => area.anchor(Align2::CENTER_CENTER, Vec2::ZERO),
    };
    area.show(ctx, |ui| {
        egui::Frame::new()
            .fill(c.surface_container_high)
            .corner_radius(egui::CornerRadius::same(shape::EXTRA_LARGE))
            .inner_margin(egui::Margin::same(16))
            .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: rpt_m3::with_alpha(c.shadow, 0.45) })
            .show(ui, |ui| {
                ui.set_width(size.x);
                ui.set_max_height(size.y);
                body(ui)
            })
            .inner
    })
    .inner
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{WIDGET_AREA_AUTO as AUTO, WIDGET_AREA_CENTER as CENTER, WIDGET_AREA_FULL as FULL};
    use egui::{pos2, vec2};

    fn screen(w: f32, h: f32) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(w, h))
    }

    fn close(a: Rect, b: Rect) -> bool {
        (a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01
    }

    #[test]
    fn sixteen_nine_is_untouched() {
        for s in [screen(1920.0, 1080.0), screen(2560.0, 1440.0), screen(1280.0, 720.0)] {
            for mode in [AUTO, FULL, CENTER] {
                assert!(close(widget_area(s, mode), s), "{mode} {s:?}");
            }
        }
        // 1366×768 is a hair wider than 16:9: auto leaves it alone
        assert!(close(widget_area(screen(1366.0, 768.0), AUTO), screen(1366.0, 768.0)));
    }

    #[test]
    fn ultrawide_21_9_centers() {
        let s = screen(3440.0, 1440.0);
        let a = widget_area(s, AUTO);
        assert!((a.width() - 2560.0).abs() < 0.01 && a.height() == 1440.0);
        assert!((a.left() - 440.0).abs() < 0.01 && (a.right() - 3000.0).abs() < 0.01);
        assert!(close(widget_area(s, CENTER), a));
        assert!(close(widget_area(s, FULL), s));
    }

    #[test]
    fn super_ultrawide_32_9_centers() {
        let s = screen(5120.0, 1440.0);
        let a = widget_area(s, AUTO);
        assert!(close(a, Rect::from_min_size(pos2(1280.0, 0.0), vec2(2560.0, 1440.0))));
        assert!(close(widget_area(s, FULL), s));
    }

    #[test]
    fn offset_screens_stay_centered() {
        // e.g. the overlay at a zoom where points don't start at 0
        let s = Rect::from_min_size(pos2(100.0, 50.0), vec2(2580.0, 1080.0));
        let a = widget_area(s, CENTER);
        assert!((a.center() - s.center()).length() < 0.01);
        assert!((a.width() - 1920.0).abs() < 0.01 && a.top() == 50.0 && a.bottom() == 1130.0);
    }

    #[test]
    fn portrait_and_narrow_are_never_letterboxed() {
        for s in [screen(1080.0, 1920.0), screen(1600.0, 1200.0), screen(1920.0, 1200.0)] {
            for mode in [AUTO, FULL, CENTER] {
                assert!(close(widget_area(s, mode), s), "{mode} {s:?}");
            }
        }
        assert!(close(widget_area(screen(0.0, 0.0), AUTO), screen(0.0, 0.0)));
    }

    #[test]
    fn anchors_resolve_inside_area() {
        let s = screen(3440.0, 1440.0);
        let area = widget_area(s, AUTO);
        let a = Anchor { pos: "tr".into(), x: 100.0, y: 0.0 };
        let p = anchor_pos(&a, area);
        assert!((p.x - 3000.0).abs() < 0.01 && p.y == 0.0);
        assert!((max_size(&a, area).x - 2560.0).abs() < 0.01);
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;
    use crate::host::HostQueue;

    /// Press on a widget's top strip, move, release: the anchor follows.
    #[test]
    fn dragging_the_strip_moves_the_widget() {
        rpt_data::init("en").unwrap();
        let ctx = Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let data = rpt_data::data();
        let i18n = crate::i18n::I18n::new(&data);
        let mut cfg = crate::config::Config::default();
        let services = crate::services::Services::new(&cfg, ctx.clone());
        let mut host = HostQueue::default();
        let id = crate::wm::id_of(&cfg, "rpt-wealth").unwrap();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0));
        let before = cfg.widgets.iter().find(|x| x.wm_id == id).unwrap().kind.clone().anchor_mut().cloned().unwrap();
        let rect = std::cell::Cell::new(Rect::NOTHING);
        let mut frame = |cfg: &mut crate::config::Config, events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let mut w = super::WCtx { cfg, st: crate::wm::OverlayState { active: true, ..Default::default() }, i18n: &i18n, host: &mut host, services: &services, screen, save_requested: false };
                let r = anchored(ui.ctx(), &mut w, id, ChromeOpts::default(), |ui, _| { ui.label("body"); ui.add_space(200.0); });
                rect.set(r.rect);
            });
            out.textures_delta.clear();
        };
        frame(&mut cfg, vec![]);
        frame(&mut cfg, vec![]);
        // middle of the top strip, left of the hide/remove buttons
        let rect = rect.get();
        assert!(rect.width() < 600.0, "widget sized to its content, not the screen: {rect:?}");
        let p0 = Pos2::new(rect.left() + 60.0, rect.top() + 20.0);
        eprintln!("widget rect {rect:?}, press at {p0:?}");
        let btn = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&mut cfg, vec![egui::Event::PointerMoved(p0)]);
        frame(&mut cfg, vec![btn(p0, true)]);
        for i in 1..=10 {
            frame(&mut cfg, vec![egui::Event::PointerMoved(p0 + Vec2::new(-20.0 * i as f32, 10.0 * i as f32))]);
        }
        let p1 = p0 + Vec2::new(-200.0, 100.0);
        frame(&mut cfg, vec![btn(p1, false)]);
        let after = cfg.widgets.iter().find(|x| x.wm_id == id).unwrap().kind.clone().anchor_mut().cloned().unwrap();
        eprintln!("anchor before {before:?} after {after:?}");
        assert!((after.x - before.x + 200.0 / 1280.0 * 100.0).abs() < 1.0, "x moved by the drag");
        assert!((after.y - before.y - 100.0 / 800.0 * 100.0).abs() < 1.0, "y moved by the drag");
    }
}

#[cfg(test)]
mod width_tests {
    use super::*;
    use crate::host::HostQueue;

    /// No tool window should stretch to the whole overlay (it would hug the
    /// screen edge and look impossible to move).
    #[test]
    fn tool_windows_are_not_full_width() {
        rpt_data::init("en").unwrap();
        let ctx = Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let data = rpt_data::data();
        let i18n = crate::i18n::I18n::new(&data);
        let mut cfg = crate::config::Config::default();
        let services = crate::services::Services::new(&cfg, ctx.clone());
        let mut host = HostQueue::default();
        let mut rt = crate::widgets::Runtime::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 1440.0));
        let tools = ["menu", "rpt-wealth", "rpt-whispers", "rpt-mapping", "rpt-market", "rpt-pob", "item-search", "stash-search", "timer"];
        let mut bad = Vec::new();
        for t in tools {
            let Some(id) = crate::wm::id_of(&cfg, t) else { continue };
            for _ in 0..3 {
                let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut out = ctx.run_ui(input, |ui| {
                    let mut w = super::WCtx { cfg: &mut cfg, st: crate::wm::OverlayState { active: true, ..Default::default() }, i18n: &i18n, host: &mut host, services: &services, screen, save_requested: false };
                    crate::widgets::draw_widget(ui.ctx(), &mut w, &mut rt, id);
                });
                out.textures_delta.clear();
            }
            let w: f32 = ctx.data(|d| d.get_temp(Id::unique(("rpt-widget", id)).with("last-width"))).unwrap_or(0.0);
            eprintln!("{t}: width {w}");
            if w > 1200.0 {
                bad.push((t, w));
            }
        }
        assert!(bad.is_empty(), "full-width tool windows: {bad:?}");
    }
}
