//! Widget manager. Port of the window-management parts of upstream
//! `renderer/src/web/overlay/OverlayWindow.vue` (show/hide/z-order/visibility).

use crate::config::{Config, Widget, WidgetKind, ZOrder};

/// Overlay-wide state that decides which widgets are visible.
#[derive(Debug, Clone, Copy, Default)]
pub struct OverlayState {
    /// The overlay currently has input focus (upstream `active`).
    pub active: bool,
    pub game_focused: bool,
    /// The game hid its UI (upstream `hideUI`, from `MAIN->OVERLAY::visibility`).
    pub hide_ui: bool,
}

pub fn find_mut(cfg: &mut Config, wm_id: u32) -> Option<&mut Widget> {
    cfg.widgets.iter_mut().find(|w| w.wm_id == wm_id)
}

pub fn id_of(cfg: &Config, type_name: &str) -> Option<u32> {
    cfg.widget(type_name).map(|w| w.wm_id)
}

/// Upstream `topmostWidget`: highest numeric z-order (menu guarantees one exists).
pub fn topmost(cfg: &Config) -> Option<&Widget> {
    cfg.widgets
        .iter()
        .filter_map(|w| match w.wm_zorder {
            Some(ZOrder::Order(z)) => Some((z, w)),
            _ => None,
        })
        .max_by_key(|(z, _)| *z)
        .map(|(_, w)| w)
}

pub fn shown_exclusive(cfg: &Config) -> Option<&Widget> {
    cfg.widgets.iter().find(|w| w.is_exclusive() && w.wants_show())
}

/// Upstream `topmostOrExclusiveWidget`.
pub fn topmost_or_exclusive(cfg: &Config) -> Option<&Widget> {
    shown_exclusive(cfg).or_else(|| topmost(cfg))
}

pub fn bring_to_top(cfg: &mut Config, wm_id: u32) {
    let Some(top) = topmost(cfg) else { return };
    if top.wm_id == wm_id {
        return;
    }
    let next = match top.wm_zorder {
        Some(ZOrder::Order(z)) => z + 1,
        _ => 1,
    };
    if let Some(w) = find_mut(cfg, wm_id) {
        if !w.is_exclusive() {
            w.wm_zorder = Some(ZOrder::Order(next));
        }
    }
}

pub fn show(cfg: &mut Config, wm_id: u32) {
    bring_to_top(cfg, wm_id);
    if let Some(ex) = topmost_or_exclusive(cfg) {
        if ex.is_exclusive() && ex.wm_id != wm_id {
            let id = ex.wm_id;
            hide(cfg, id);
        }
    }
    if let Some(w) = find_mut(cfg, wm_id) {
        w.wm_wants = "show".into();
    }
}

pub fn hide(cfg: &mut Config, wm_id: u32) {
    if let Some(w) = find_mut(cfg, wm_id) {
        w.wm_wants = "hide".into();
    }
}

pub fn toggle(cfg: &mut Config, wm_id: u32) {
    let shown = cfg.widgets.iter().any(|w| w.wm_id == wm_id && w.wants_show());
    if shown { hide(cfg, wm_id) } else { show(cfg, wm_id) }
}

pub fn remove(cfg: &mut Config, wm_id: u32) {
    cfg.widgets.retain(|w| w.wm_id != wm_id);
}

pub fn set_flag(cfg: &mut Config, wm_id: u32, flag: &str, state: bool) -> bool {
    let Some(w) = find_mut(cfg, wm_id) else { return false };
    let has = w.has_flag(flag);
    if has && !state {
        w.wm_flags.retain(|f| f != flag);
        true
    } else if !has && state {
        w.wm_flags.push(flag.to_owned());
        true
    } else {
        false
    }
}

/// Upstream `create(wmType)` — new multi-instance widget, shown immediately.
pub fn create(cfg: &mut Config, kind: WidgetKind, title: &str) -> u32 {
    let id = cfg.next_widget_id();
    cfg.widgets.push(Widget {
        wm_id: id,
        wm_title: title.to_owned(),
        wm_wants: "hide".into(),
        wm_zorder: None,
        wm_flags: vec!["invisible-on-blur".into()],
        kind,
    });
    show(cfg, id);
    id
}

/// Upstream focus-change handler: apply hide-on-blur / hide-on-focus.
pub fn on_focus_change(cfg: &mut Config, overlay_active: bool) {
    let flag = if overlay_active { "hide-on-focus" } else { "hide-on-blur" };
    let ids: Vec<u32> = cfg.widgets.iter().filter(|w| w.has_flag(flag)).map(|w| w.wm_id).collect();
    for id in ids {
        hide(cfg, id);
    }
}

/// Upstream `visibilityState` → `isVisible(wmId)`.
pub fn is_visible(cfg: &Config, st: OverlayState, w: &Widget) -> bool {
    let mut exclusive = shown_exclusive(cfg);
    if !st.active && exclusive.is_some_and(|e| e.has_flag("invisible-on-blur")) {
        exclusive = None;
    }
    if st.hide_ui {
        st.active && w.wants_show() && w.has_flag("ignore-ui-visibility")
    } else if !st.active && w.has_flag("invisible-on-blur") {
        false
    } else if let Some(e) = exclusive {
        e.wm_id == w.wm_id
    } else {
        w.wants_show()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_hides_others() {
        let mut c = Config::default();
        let pc = id_of(&c, "price-check").unwrap();
        let settings = id_of(&c, "settings").unwrap();
        show(&mut c, settings);
        show(&mut c, pc);
        assert!(!c.widgets.iter().find(|w| w.wm_id == settings).unwrap().wants_show());
        let st = OverlayState { active: true, ..Default::default() };
        let menu = c.widget("menu").unwrap();
        assert!(!is_visible(&c, st, menu));
        assert!(is_visible(&c, st, c.widget("price-check").unwrap()));
    }

    #[test]
    fn blur_hides_flagged() {
        let mut c = Config::default();
        let pc = id_of(&c, "price-check").unwrap();
        show(&mut c, pc);
        on_focus_change(&mut c, false);
        assert!(!c.widget("price-check").unwrap().wants_show());
    }
}
