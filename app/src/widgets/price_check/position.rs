//! Window placement math from upstream `PriceCheckWindow.vue` / `OverlayWindow.vue`.
//! All values are egui points relative to the overlay (game client area).

use egui::{Pos2, Rect, pos2, vec2};

/// Which side of the screen the item was checked on (upstream `clickPosition`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    /// left half: stash / vendor — window opens right of the left game panel
    #[default]
    Stash,
    /// right half: inventory — window opens left of the right game panel
    Inventory,
}

/// Upstream `poePanelWidth`: the game's side panel is 986px at 1600px height.
pub fn poe_panel_width(screen_height: f32) -> f32 {
    screen_height * 986.0 / 1600.0
}

pub fn click_side(pos: Pos2, screen: Rect) -> Side {
    if pos.x > screen.left() + screen.width() / 2.0 { Side::Inventory } else { Side::Stash }
}

/// Price check window rect: full height, next to the game panel on the click side.
pub fn window_rect(screen: Rect, side: Side, width: f32) -> Rect {
    let panel = poe_panel_width(screen.height());
    let width = width.min(screen.width());
    let x = match side {
        Side::Stash => screen.left() + panel,
        Side::Inventory => screen.right() - panel - width,
    };
    let x = x.clamp(screen.left(), (screen.right() - width).max(screen.left()));
    Rect::from_min_size(pos2(x, screen.top()), vec2(width, screen.height()))
}

/// The free column next to the window (towards the screen center) where
/// related items and the rate-limiter state are shown.
pub fn side_column(screen: Rect, window: Rect, side: Side) -> Rect {
    match side {
        Side::Stash => Rect::from_x_y_ranges(window.right()..=screen.right().max(window.right()), screen.y_range()),
        Side::Inventory => Rect::from_x_y_ranges(screen.left().min(window.left())..=window.left(), screen.y_range()),
    }
}

/// Area for `OVERLAY->MAIN::track-area` (window column, full height) in
/// overlay-local points; the caller converts to screen coordinates.
pub fn track_area(screen: Rect, check_pos: Pos2, width: f32) -> Rect {
    window_rect(screen, click_side(check_pos, screen), width)
}

/// Item-check anchor (`WidgetItemCheck.vue`): `(pivot, x%, y%)`.
pub fn item_check_anchor(screen: Rect, check_pos: Pos2) -> (egui::Align2, f32, f32) {
    let width = screen.width().max(1.0);
    let panel = poe_panel_width(screen.height());
    match click_side(check_pos, screen) {
        Side::Stash => (egui::Align2::LEFT_CENTER, panel / width * 100.0, 50.0),
        Side::Inventory => (egui::Align2::RIGHT_CENTER, (width - panel) / width * 100.0, 50.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { min: Pos2 { x: 0.0, y: 0.0 }, max: Pos2 { x: 2560.0, y: 1440.0 } };

    #[test]
    fn panel_width() {
        assert!((poe_panel_width(1600.0) - 986.0).abs() < 0.01);
        assert!((poe_panel_width(1440.0) - 887.4).abs() < 0.01);
    }

    #[test]
    fn sides() {
        assert_eq!(click_side(pos2(100.0, 500.0), SCREEN), Side::Stash);
        assert_eq!(click_side(pos2(2000.0, 500.0), SCREEN), Side::Inventory);
    }

    #[test]
    fn window_placement() {
        let w = window_rect(SCREEN, Side::Stash, 460.0);
        assert!((w.left() - 887.4).abs() < 0.01);
        assert_eq!(w.height(), 1440.0);
        let w = window_rect(SCREEN, Side::Inventory, 460.0);
        assert!((w.right() - (2560.0 - 887.4)).abs() < 0.01);
        // tiny screens: stays on screen
        let small = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 1000.0));
        let w = window_rect(small, Side::Stash, 460.0);
        assert!(w.right() <= 600.0 && w.left() >= 0.0);
        let col = side_column(SCREEN, window_rect(SCREEN, Side::Stash, 460.0), Side::Stash);
        assert!(col.left() >= 887.4 + 460.0 - 0.01);
        let col = side_column(SCREEN, window_rect(SCREEN, Side::Inventory, 460.0), Side::Inventory);
        assert!(col.right() <= 2560.0 - 887.4 - 460.0 + 0.01);
    }

    #[test]
    fn item_check_anchor_sides() {
        let (p, x, y) = item_check_anchor(SCREEN, pos2(10.0, 10.0));
        assert_eq!(p, egui::Align2::LEFT_CENTER);
        assert!((x - 887.4 / 2560.0 * 100.0).abs() < 0.01);
        assert_eq!(y, 50.0);
        let (p, _, _) = item_check_anchor(SCREEN, pos2(2500.0, 10.0));
        assert_eq!(p, egui::Align2::RIGHT_CENTER);
    }
}
