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
    let screen = w.screen;
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
                        // Drag handle + actions row (upstream move handle & action strip).
                        ui.horizontal(|ui| {
                            let (r, drag) = ui.allocate_exact_size(Vec2::new(24.0, 24.0), Sense::drag());
                            ui.painter().text(
                                r.center(),
                                Align2::CENTER_CENTER,
                                icons::DRAG_INDICATOR,
                                egui::FontId::new(18.0, egui::FontFamily::Name(rpt_m3::theme::ICONS.into())),
                                c.on_surface_variant,
                            );
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
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if opts.removable
                                    && ui.add(IconButton::new(icons::DELETE).size(28.0).tooltip(w.i18n.t("widget.delete"))).double_clicked()
                                {
                                    remove = true;
                                }
                                if opts.hideable && ui.add(IconButton::new(icons::VISIBILITY_OFF).size(28.0).tooltip(w.i18n.t("widget.hide"))).clicked() {
                                    hide = true;
                                }
                            });
                        });
                    }
                    body(ui, w)
                })
                .inner
        });

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
