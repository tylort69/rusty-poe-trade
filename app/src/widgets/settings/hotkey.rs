//! Port of upstream `settings/HotkeyInput.vue`, `SettingsHotkey.vue` and
//! `HotkeysGeneric.vue`, plus the shortcut-string format from `ipc/KeyToCode.ts`.
//!
//! Click the field, then press a key combination. Escape cancels, Backspace
//! clears (unless the hotkey is required). F12 is reserved (devtools upstream).

use egui::{Align2, CornerRadius, Event, Id, Key, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use rpt_m3::{TypeRole, icons, shape, theme, with_alpha};

use crate::i18n::I18n;

/// Upstream `KeyToCode` name for an egui key (`None` → not bindable).
pub fn key_name(key: Key) -> Option<&'static str> {
    use Key::*;
    Some(match key {
        ArrowDown => "ArrowDown",
        ArrowLeft => "ArrowLeft",
        ArrowRight => "ArrowRight",
        ArrowUp => "ArrowUp",
        Tab => "Tab",
        Enter => "Enter",
        Space => "Space",
        Insert => "Insert",
        Delete => "Delete",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        Semicolon | Colon => "Semicolon",
        Equals | Plus => "Equal",
        Comma => "Comma",
        Minus => "Minus",
        Period => "Period",
        Slash | Questionmark => "Slash",
        Backtick => "Backquote",
        OpenBracket | OpenCurlyBracket => "BracketLeft",
        Backslash | Pipe => "Backslash",
        CloseBracket | CloseCurlyBracket => "BracketRight",
        Quote => "Quote",
        CapsLock => "CapsLock",
        F25 | F26 | F27 | F28 | F29 | F30 | F31 | F32 | F33 | F34 | F35 => return None,
        k => {
            let n = k.name();
            let letter = n.len() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric());
            let fkey = n.starts_with('F') && n.len() <= 3 && n[1..].chars().all(|c| c.is_ascii_digit()) && n.len() > 1;
            if letter || fkey {
                n
            } else {
                return None;
            }
        }
    })
}

/// Upstream `hotkeyToString`.
pub fn hotkey_to_string(keys: &[&str], mut ctrl: bool, mut shift: bool, mut alt: bool) -> String {
    ctrl |= keys.contains(&"Ctrl");
    shift |= keys.contains(&"Shift");
    alt |= keys.contains(&"Alt");
    let keys: Vec<&str> = keys.iter().copied().filter(|k| !matches!(*k, "Ctrl" | "Shift" | "Alt")).collect();
    let m = match (ctrl, shift, alt) {
        (true, true, true) => "Ctrl + Shift + Alt",
        (_, true, true) => "Shift + Alt",
        (true, true, _) => "Ctrl + Shift",
        (true, _, true) => "Ctrl + Alt",
        (_, _, true) => "Alt",
        (true, _, _) => "Ctrl",
        (_, true, _) => "Shift",
        _ => "",
    };
    let joined = keys.join(" + ");
    if !m.is_empty() && !keys.is_empty() {
        format!("{m} + {joined}")
    } else if joined.is_empty() {
        m.to_owned()
    } else {
        joined
    }
}

/// Which hotkey field is currently capturing (one at a time).
#[derive(Default)]
pub struct Capture {
    pub active: Option<Id>,
}

/// Outcome of a key press while capturing.
#[derive(Debug, PartialEq)]
enum Captured {
    Set(String),
    Clear,
    Cancel,
}

fn read_capture(ui: &Ui, no_mod_keys: bool) -> Option<Captured> {
    ui.input_mut(|i| {
        let mut out = None;
        i.events.retain(|e| {
            let Event::Key { key, physical_key, pressed: true, modifiers, .. } = e else { return true };
            if out.is_some() {
                return false;
            }
            let key = physical_key.unwrap_or(*key);
            match key {
                Key::Escape => out = Some(Captured::Cancel),
                Key::Backspace => out = Some(Captured::Clear),
                k => {
                    if let Some(name) = key_name(k) {
                        let s = hotkey_to_string(&[name], modifiers.ctrl || modifiers.command && !cfg!(target_os = "macos"), modifiers.shift, modifiers.alt);
                        let reserved = s.contains("F12");
                        let has_mod = s.contains('+');
                        if !reserved && !(no_mod_keys && has_mod) {
                            out = Some(Captured::Set(s));
                        }
                    }
                }
            }
            false
        });
        out
    })
}

/// M3 hotkey field. Returns true if the value changed.
pub fn hotkey_input(ui: &mut Ui, cap: &mut Capture, i18n: &I18n, id_salt: impl std::hash::Hash + std::fmt::Debug, value: &mut Option<String>, required: bool, no_mod_keys: bool, width: f32) -> bool {
    let th = theme(ui.ctx());
    let c = th.colors;
    let id = ui.make_persistent_id(("hotkey", id_salt));
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let capturing = cap.active == Some(id);
    let mut changed = false;

    // Clear button (mouse alternative to Backspace).
    let clear_rect = Rect::from_center_size(egui::pos2(rect.right() - 20.0, rect.center().y), vec2(28.0, 28.0));
    let show_clear = value.is_some() && !required && !capturing;
    let clear_resp = show_clear.then(|| ui.interact(clear_rect, id.with("clear"), Sense::click()));

    if let Some(cr) = &clear_resp {
        if cr.clicked() {
            *value = None;
            changed = true;
        }
    }
    if resp.clicked() && !clear_resp.as_ref().is_some_and(|r| r.hovered()) {
        cap.active = if capturing { None } else { Some(id) };
    }
    if capturing {
        match read_capture(ui, no_mod_keys) {
            Some(Captured::Set(s)) => {
                if value.as_deref() != Some(s.as_str()) {
                    *value = Some(s);
                    changed = true;
                }
                cap.active = None;
            }
            Some(Captured::Clear) => {
                if !required && value.is_some() {
                    *value = None;
                    changed = true;
                }
                cap.active = None;
            }
            Some(Captured::Cancel) => cap.active = None,
            None => {
                // Clicking anywhere else cancels capture.
                if ui.input(|i| i.pointer.any_pressed()) && !resp.hovered() {
                    cap.active = None;
                }
            }
        }
        ui.ctx().request_repaint();
    }
    let capturing = cap.active == Some(id);

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let radius = CornerRadius::same(shape::EXTRA_SMALL);
        let unset = value.is_none();
        let (stroke_w, stroke_c) = if capturing {
            (2.0, c.primary)
        } else if unset && required {
            (1.0, c.error)
        } else {
            (1.0, c.outline)
        };
        p.rect(rect, radius, if capturing { with_alpha(c.primary, 0.08) } else { egui::Color32::TRANSPARENT }, Stroke::new(stroke_w, stroke_c), StrokeKind::Inside);
        if resp.hovered() && !capturing {
            p.rect_filled(rect, radius, with_alpha(c.on_surface, 0.08));
        }
        let (text, color) = if capturing {
            ("…".to_owned(), c.primary)
        } else {
            match value {
                Some(v) => (v.clone(), c.on_surface),
                None => (i18n.t("settings.no_key"), if required { c.error } else { c.on_surface_variant }),
            }
        };
        let icon_font = egui::FontId::new(18.0, egui::FontFamily::Name(rpt_m3::theme::ICONS.into()));
        p.text(egui::pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, icons::KEYBOARD, icon_font.clone(), if capturing { c.primary } else { c.on_surface_variant });
        let text_rect = Rect::from_min_max(egui::pos2(rect.left() + 36.0, rect.top()), egui::pos2(rect.right() - if show_clear { 36.0 } else { 8.0 }, rect.bottom()));
        p.with_clip_rect(text_rect).text(egui::pos2(text_rect.left(), rect.center().y), Align2::LEFT_CENTER, text, th.font(TypeRole::LabelLarge), color);
        if let Some(cr) = &clear_resp {
            if cr.hovered() {
                p.circle_filled(clear_rect.center(), 14.0, with_alpha(c.on_surface, 0.08));
            }
            p.text(clear_rect.center(), Align2::CENTER_CENTER, icons::CLOSE, icon_font, c.on_surface_variant);
        }
    }
    let tip = if capturing { "Esc — cancel · Backspace — clear" } else { "" };
    if !tip.is_empty() {
        resp.on_hover_text(tip);
    }
    changed
}

/// One row of upstream `SettingsHotkey`: label + hotkey field.
pub fn hotkey_row(ui: &mut Ui, cap: &mut Capture, i18n: &I18n, label: &str, value: &mut Option<String>, required: bool) -> bool {
    let label = if required { format!("{label} *") } else { label.to_owned() };
    rpt_m3::setting_row(ui, None, &label, None, |ui| hotkey_input(ui, cap, i18n, label.as_str(), value, required, false, 200.0))
}

/// Upstream split key: [Ctrl|Alt] + key (price check "auto-hide mode").
pub fn split_hotkey_row(ui: &mut Ui, cap: &mut Capture, i18n: &I18n, label: &str, mod_key: &mut String, key: &mut Option<String>) -> bool {
    rpt_m3::setting_row(ui, None, label, None, |ui| {
        let mut changed = hotkey_input(ui, cap, i18n, (label, "split"), key, false, true, 112.0);
        ui.label(rpt_m3::text(ui, "+", TypeRole::TitleMedium));
        let mut m = mod_key.clone();
        if rpt_m3::segmented(ui, &mut m, &[("Alt".to_owned(), "Alt"), ("Ctrl".to_owned(), "Ctrl")]) {
            *mod_key = m;
            changed = true;
        }
        changed
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_upstream() {
        assert_eq!(hotkey_to_string(&["D"], true, false, true), "Ctrl + Alt + D");
        assert_eq!(hotkey_to_string(&["Space"], false, true, false), "Shift + Space");
        assert_eq!(hotkey_to_string(&["F5"], false, false, false), "F5");
        assert_eq!(hotkey_to_string(&["A"], true, true, true), "Ctrl + Shift + Alt + A");
        assert_eq!(hotkey_to_string(&["Ctrl"], false, false, false), "Ctrl");
    }

    #[test]
    fn key_names() {
        assert_eq!(key_name(Key::A), Some("A"));
        assert_eq!(key_name(Key::Num7), Some("7"));
        assert_eq!(key_name(Key::F11), Some("F11"));
        assert_eq!(key_name(Key::Backtick), Some("Backquote"));
        assert_eq!(key_name(Key::Copy), None);
    }
}
