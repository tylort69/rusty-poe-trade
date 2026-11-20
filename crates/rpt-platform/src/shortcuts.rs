//! Pure logic of `shortcuts/Shortcuts.ts` and `shortcuts/text-box.ts`:
//! filtering of configured shortcuts and the key sequences sent to the game.
//! The side-effecting parts live in `core.rs`.

use crate::keys::{merge_two_hotkeys, Key, Shortcut};
use crate::types::{ShortcutAction, ShortcutBinding};

/// Hotkeys the game itself uses; upstream refuses to register them.
pub const RESERVED_BY_GAME: &[&str] = &[
    "Ctrl + C",
    "Ctrl + V",
    "Ctrl + A",
    "Ctrl + F",
    "Ctrl + Enter",
    "Home",
    "Delete",
    "Enter",
    "ArrowUp",
    "ArrowRight",
    "ArrowLeft",
];

/// A shortcut that passed validation.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveShortcut {
    pub binding: ShortcutBinding,
    pub parsed: Shortcut,
}

/// Port of `Shortcuts.updateActions` filtering. Returns the accepted
/// shortcuts and the error messages upstream would log.
pub fn filter_actions(actions: &[ShortcutBinding]) -> (Vec<ActiveShortcut>, Vec<String>) {
    let mut errors = Vec::new();
    let mut all: Vec<String> = RESERVED_BY_GAME.iter().map(|s| s.to_string()).collect();

    // Normalise so that "Alt + Shift + X" and "Shift + Alt + X" compare equal.
    let mut parsed: Vec<(ShortcutBinding, Shortcut, String)> = Vec::new();
    for a in actions {
        match Shortcut::parse(&a.shortcut) {
            Ok(p) => {
                let canon = p.to_string();
                parsed.push((a.clone(), p, canon));
            }
            Err(e) => errors.push(format!("Hotkey \"{}\" is invalid: {e}.", a.shortcut)),
        }
    }

    for (a, _, canon) in &parsed {
        if all.contains(canon) && a.action != ShortcutAction::TestOnly {
            errors.push(format!(
                "Hotkey \"{}\" reserved by the game will not be registered.",
                a.shortcut
            ));
        }
    }
    parsed.retain(|(_, _, canon)| !RESERVED_BY_GAME.contains(&canon.as_str()));

    let mut duplicates: Vec<String> = Vec::new();
    for (a, _, canon) in &parsed {
        if all.contains(canon) {
            errors.push(format!(
                "It is not possible to use the same hotkey \"{}\" for multiple actions.",
                a.shortcut
            ));
            duplicates.push(canon.clone());
        } else {
            all.push(canon.clone());
        }
    }

    let accepted = parsed
        .into_iter()
        .filter(|(a, _, canon)| !duplicates.contains(canon) || a.action == ShortcutAction::ToggleOverlay)
        .map(|(binding, parsed, _)| ActiveShortcut { binding, parsed })
        .collect();
    (accepted, errors)
}

/// Keys to release when a hotkey fires, so the game does not see the
/// physically held keys combined with our synthetic input.
pub fn keys_to_release(sc: &Shortcut, keep_mod_keys: bool) -> Vec<Key> {
    if keep_mod_keys {
        sc.key.into_iter().collect()
    } else {
        let mut v = sc.keys();
        v.reverse();
        v
    }
}

/// Port of `pressKeysToCopyItemText`: modifiers to hold while tapping `C`.
pub fn copy_item_modifiers(pressed_mod_keys: &[Key], _show_mods_key: &str) -> Vec<Key> {
    // 3.29: Copying an item's text now always copies the advanced description format.
    let show_mods_key = "Ctrl";
    merge_two_hotkeys("Ctrl + C", show_mods_key)
        .split(" + ")
        .filter(|k| *k != "C")
        .filter_map(Key::from_name)
        .filter(|k| !pressed_mod_keys.contains(k))
        .collect()
}

const PLACEHOLDER_LAST: &str = "@last";
const AUTO_CLEAR: &[char] = &['#', '%', '@', '$', '&', '/'];

/// One synthetic key tap: key plus held modifiers.
pub type Tap = (Key, Vec<Key>);

/// Port of `typeInChat`: the text to place on the clipboard and the taps to
/// perform (Ctrl is the paste modifier on both Windows and Linux).
pub fn type_in_chat_plan(text: &str, send: bool) -> (String, Vec<Tap>) {
    let ctrl = vec![Key::CTRL];
    let mut taps: Vec<Tap> = Vec::new();
    let clip;
    if let Some(rest) = text.strip_prefix(PLACEHOLDER_LAST) {
        // upstream: text.slice('@last '.length)
        clip = rest.strip_prefix(' ').unwrap_or(rest).to_string();
        taps.push((Key::ENTER, ctrl.clone()));
    } else if let Some(rest) = text.strip_suffix(PLACEHOLDER_LAST) {
        clip = rest.to_string();
        taps.push((Key::ENTER, ctrl.clone()));
        taps.push((Key::HOME, vec![]));
        // press twice to focus input when using controller
        taps.push((Key::HOME, vec![]));
        taps.push((Key::DELETE, vec![]));
    } else {
        clip = text.to_string();
        taps.push((Key::ENTER, vec![]));
        if !text.starts_with(AUTO_CLEAR) {
            taps.push((Key::A, ctrl.clone()));
        }
    }
    taps.push((Key::V, ctrl));
    if send {
        taps.push((Key::ENTER, vec![]));
        // restore the last chat
        taps.push((Key::ENTER, vec![]));
        taps.push((Key::ARROW_UP, vec![]));
        taps.push((Key::ARROW_UP, vec![]));
        taps.push((Key::ESCAPE, vec![]));
    }
    (clip, taps)
}

/// Port of `stashSearch` key sequence (after focusing the game and putting
/// `text` on the clipboard).
pub fn stash_search_taps() -> Vec<Tap> {
    vec![(Key::F, vec![Key::CTRL]), (Key::V, vec![Key::CTRL]), (Key::ENTER, vec![])]
}

/// Port of `isStashArea`. `bounds` is the game client area.
pub fn is_stash_area(x: f64, y: f64, bounds: Option<crate::types::Rect>) -> bool {
    let Some(b) = bounds else { return false };
    // sidebar is 370px at 800x600
    let sidebar = (b.height as f64 * 370.0 / 600.0).round();
    if x > b.x as f64 + sidebar {
        return false;
    }
    y > b.y as f64 + b.height as f64 * 154.0 / 1600.0
        && y < b.y as f64 + b.height as f64 * 1192.0 / 1600.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Rect;

    fn b(shortcut: &str, action: ShortcutAction) -> ShortcutBinding {
        ShortcutBinding { shortcut: shortcut.into(), keep_mod_keys: false, action }
    }

    #[test]
    fn filters_reserved_and_duplicates() {
        let pc = || ShortcutAction::CopyItem { target: "price-check".into(), focus_overlay: false };
        let actions = vec![
            b("Ctrl + C", pc()),
            b("Ctrl + D", pc()),
            b("Ctrl + D", ShortcutAction::TriggerEvent { target: "x".into() }),
            b("Shift + Space", ShortcutAction::ToggleOverlay),
            b("Shift + Space", ShortcutAction::TriggerEvent { target: "y".into() }),
            b("Ctrl + Bogus", pc()),
            b("F1", ShortcutAction::TestOnly),
        ];
        let (ok, errs) = filter_actions(&actions);
        let names: Vec<_> = ok.iter().map(|a| (a.binding.shortcut.as_str(), a.binding.action.type_name())).collect();
        assert_eq!(names, vec![("Shift + Space", "toggle-overlay"), ("F1", "test-only")]);
        assert_eq!(errs.len(), 4, "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("reserved by the game")));
        assert!(errs.iter().any(|e| e.contains("invalid")));
    }

    #[test]
    fn modifier_order_is_normalised() {
        let actions = vec![
            b("Alt + Shift + X", ShortcutAction::TriggerEvent { target: "a".into() }),
            b("Shift + Alt + X", ShortcutAction::TriggerEvent { target: "b".into() }),
        ];
        let (ok, errs) = filter_actions(&actions);
        assert!(ok.is_empty());
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn release_and_copy_keys() {
        let sc = Shortcut::parse("Ctrl + Alt + D").unwrap();
        let d = Key::from_name("D").unwrap();
        assert_eq!(keys_to_release(&sc, false), vec![d, Key::ALT, Key::CTRL]);
        assert_eq!(keys_to_release(&sc, true), vec![d]);
        assert_eq!(copy_item_modifiers(&[], "Alt"), vec![Key::CTRL]);
        assert_eq!(copy_item_modifiers(&[Key::CTRL], "Alt"), Vec::<Key>::new());
    }

    #[test]
    fn chat_plans() {
        let (clip, taps) = type_in_chat_plan("/hideout", true);
        assert_eq!(clip, "/hideout");
        let keys: Vec<_> = taps.iter().map(|(k, _)| k.name()).collect();
        assert_eq!(keys, vec!["Enter", "V", "Enter", "Enter", "ArrowUp", "ArrowUp", "Escape"]);

        let (clip, taps) = type_in_chat_plan("hello", false);
        assert_eq!(clip, "hello");
        assert_eq!(taps[1], (Key::A, vec![Key::CTRL]));

        let (clip, taps) = type_in_chat_plan("@last thanks", false);
        assert_eq!(clip, "thanks");
        assert_eq!(taps[0], (Key::ENTER, vec![Key::CTRL]));
        assert_eq!(taps.len(), 2);

        let (clip, taps) = type_in_chat_plan("hi @last", false);
        assert_eq!(clip, "hi ");
        assert_eq!(taps.len(), 5);
    }

    #[test]
    fn stash_area() {
        let r = Some(Rect { x: 0, y: 0, width: 1920, height: 1080 });
        assert!(is_stash_area(100.0, 500.0, r));
        assert!(!is_stash_area(1000.0, 500.0, r));
        assert!(!is_stash_area(100.0, 50.0, r));
        assert!(!is_stash_area(100.0, 500.0, None));
    }
}
