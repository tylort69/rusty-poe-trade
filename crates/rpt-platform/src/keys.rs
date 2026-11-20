//! Port of `ipc/KeyToCode.ts`: Awakened PoE Trade key names, their Windows
//! virtual-key codes, and APT shortcut strings such as `Ctrl + Shift + D`.
//!
//! The canonical key names are exactly the ones used by upstream configs
//! (`A`..`Z`, `0`..`9`, `F1`..`F24`, `ArrowLeft`, `Numpad0`, `Semicolon`, ...)
//! plus the three modifiers `Ctrl`, `Shift`, `Alt`. The numeric codes are
//! Windows VK codes; they are also what `production_Config.ini` stores.

use std::fmt;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

/// A key known to APT, identified by its Windows virtual-key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key(u16);

/// `(name, vk code, keyboard-types code)`. Modifiers have no `Code` entry
/// because they are never registered as the main key of a global hotkey.
const KEYS: &[(&str, u16, Option<Code>)] = &[
    ("Backspace", 8, Some(Code::Backspace)),
    ("Tab", 9, Some(Code::Tab)),
    ("Enter", 13, Some(Code::Enter)),
    ("Shift", 16, None),
    ("Ctrl", 17, None),
    ("Alt", 18, None),
    ("CapsLock", 20, Some(Code::CapsLock)),
    ("Escape", 27, Some(Code::Escape)),
    ("Space", 32, Some(Code::Space)),
    ("PageUp", 33, Some(Code::PageUp)),
    ("PageDown", 34, Some(Code::PageDown)),
    ("End", 35, Some(Code::End)),
    ("Home", 36, Some(Code::Home)),
    ("ArrowLeft", 37, Some(Code::ArrowLeft)),
    ("ArrowUp", 38, Some(Code::ArrowUp)),
    ("ArrowRight", 39, Some(Code::ArrowRight)),
    ("ArrowDown", 40, Some(Code::ArrowDown)),
    ("Insert", 45, Some(Code::Insert)),
    ("Delete", 46, Some(Code::Delete)),
    ("0", 48, Some(Code::Digit0)),
    ("1", 49, Some(Code::Digit1)),
    ("2", 50, Some(Code::Digit2)),
    ("3", 51, Some(Code::Digit3)),
    ("4", 52, Some(Code::Digit4)),
    ("5", 53, Some(Code::Digit5)),
    ("6", 54, Some(Code::Digit6)),
    ("7", 55, Some(Code::Digit7)),
    ("8", 56, Some(Code::Digit8)),
    ("9", 57, Some(Code::Digit9)),
    ("A", 65, Some(Code::KeyA)),
    ("B", 66, Some(Code::KeyB)),
    ("C", 67, Some(Code::KeyC)),
    ("D", 68, Some(Code::KeyD)),
    ("E", 69, Some(Code::KeyE)),
    ("F", 70, Some(Code::KeyF)),
    ("G", 71, Some(Code::KeyG)),
    ("H", 72, Some(Code::KeyH)),
    ("I", 73, Some(Code::KeyI)),
    ("J", 74, Some(Code::KeyJ)),
    ("K", 75, Some(Code::KeyK)),
    ("L", 76, Some(Code::KeyL)),
    ("M", 77, Some(Code::KeyM)),
    ("N", 78, Some(Code::KeyN)),
    ("O", 79, Some(Code::KeyO)),
    ("P", 80, Some(Code::KeyP)),
    ("Q", 81, Some(Code::KeyQ)),
    ("R", 82, Some(Code::KeyR)),
    ("S", 83, Some(Code::KeyS)),
    ("T", 84, Some(Code::KeyT)),
    ("U", 85, Some(Code::KeyU)),
    ("V", 86, Some(Code::KeyV)),
    ("W", 87, Some(Code::KeyW)),
    ("X", 88, Some(Code::KeyX)),
    ("Y", 89, Some(Code::KeyY)),
    ("Z", 90, Some(Code::KeyZ)),
    ("Numpad0", 96, Some(Code::Numpad0)),
    ("Numpad1", 97, Some(Code::Numpad1)),
    ("Numpad2", 98, Some(Code::Numpad2)),
    ("Numpad3", 99, Some(Code::Numpad3)),
    ("Numpad4", 100, Some(Code::Numpad4)),
    ("Numpad5", 101, Some(Code::Numpad5)),
    ("Numpad6", 102, Some(Code::Numpad6)),
    ("Numpad7", 103, Some(Code::Numpad7)),
    ("Numpad8", 104, Some(Code::Numpad8)),
    ("Numpad9", 105, Some(Code::Numpad9)),
    ("NumpadMultiply", 106, Some(Code::NumpadMultiply)),
    ("NumpadAdd", 107, Some(Code::NumpadAdd)),
    ("NumpadSubtract", 109, Some(Code::NumpadSubtract)),
    ("NumpadDecimal", 110, Some(Code::NumpadDecimal)),
    ("NumpadDivide", 111, Some(Code::NumpadDivide)),
    ("F1", 112, Some(Code::F1)),
    ("F2", 113, Some(Code::F2)),
    ("F3", 114, Some(Code::F3)),
    ("F4", 115, Some(Code::F4)),
    ("F5", 116, Some(Code::F5)),
    ("F6", 117, Some(Code::F6)),
    ("F7", 118, Some(Code::F7)),
    ("F8", 119, Some(Code::F8)),
    ("F9", 120, Some(Code::F9)),
    ("F10", 121, Some(Code::F10)),
    ("F11", 122, Some(Code::F11)),
    ("F12", 123, Some(Code::F12)),
    ("F13", 124, Some(Code::F13)),
    ("F14", 125, Some(Code::F14)),
    ("F15", 126, Some(Code::F15)),
    ("F16", 127, Some(Code::F16)),
    ("F17", 128, Some(Code::F17)),
    ("F18", 129, Some(Code::F18)),
    ("F19", 130, Some(Code::F19)),
    ("F20", 131, Some(Code::F20)),
    ("F21", 132, Some(Code::F21)),
    ("F22", 133, Some(Code::F22)),
    ("F23", 134, Some(Code::F23)),
    ("F24", 135, Some(Code::F24)),
    ("Semicolon", 186, Some(Code::Semicolon)),
    ("Equal", 187, Some(Code::Equal)),
    ("Comma", 188, Some(Code::Comma)),
    ("Minus", 189, Some(Code::Minus)),
    ("Period", 190, Some(Code::Period)),
    ("Slash", 191, Some(Code::Slash)),
    ("Backquote", 192, Some(Code::Backquote)),
    ("BracketLeft", 219, Some(Code::BracketLeft)),
    ("Backslash", 220, Some(Code::Backslash)),
    ("BracketRight", 221, Some(Code::BracketRight)),
    ("Quote", 222, Some(Code::Quote)),
];

impl Key {
    pub const SHIFT: Key = Key(16);
    pub const CTRL: Key = Key(17);
    pub const ALT: Key = Key(18);
    pub const ENTER: Key = Key(13);
    pub const ESCAPE: Key = Key(27);
    pub const HOME: Key = Key(36);
    pub const DELETE: Key = Key(46);
    pub const ARROW_LEFT: Key = Key(37);
    pub const ARROW_UP: Key = Key(38);
    pub const ARROW_RIGHT: Key = Key(39);
    pub const A: Key = Key(65);
    pub const C: Key = Key(67);
    pub const F: Key = Key(70);
    pub const V: Key = Key(86);

    /// Looks a key up by its APT name (`"A"`, `"F5"`, `"ArrowLeft"`, `"Ctrl"`).
    pub fn from_name(name: &str) -> Option<Key> {
        KEYS.iter().find(|(n, _, _)| *n == name).map(|(_, vk, _)| Key(*vk))
    }

    /// Port of `CodeToKey`: VK code → key.
    pub fn from_vk(vk: u16) -> Option<Key> {
        KEYS.iter().find(|(_, v, _)| *v == vk).map(|(_, vk, _)| Key(*vk))
    }

    pub fn name(self) -> &'static str {
        KEYS.iter()
            .find(|(_, v, _)| *v == self.0)
            .map(|(n, _, _)| *n)
            .unwrap_or("not_supported_key")
    }

    pub fn vk(self) -> u16 {
        self.0
    }

    pub fn is_modifier(self) -> bool {
        matches!(self.0, 16..=18)
    }

    pub(crate) fn code(self) -> Option<Code> {
        KEYS.iter().find(|(_, v, _)| *v == self.0).and_then(|(_, _, c)| *c)
    }

    /// All keys known to APT, in upstream order.
    pub fn all() -> impl Iterator<Item = Key> {
        KEYS.iter().map(|(_, vk, _)| Key(*vk))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Port of `isModKey`.
pub fn is_mod_key(key: &str) -> bool {
    key == "Ctrl" || key == "Shift" || key == "Alt"
}

/// Port of `hotkeyToString`: builds the canonical APT shortcut string with the
/// modifiers in upstream order (`Ctrl + Shift + Alt + ...`).
pub fn hotkey_to_string(keys: &[&str], ctrl: bool, shift: bool, alt: bool) -> String {
    let ctrl = ctrl || keys.contains(&"Ctrl");
    let shift = shift || keys.contains(&"Shift");
    let alt = alt || keys.contains(&"Alt");
    let keys: Vec<&str> = keys.iter().copied().filter(|k| !is_mod_key(k)).collect();

    let m = match (ctrl, shift, alt) {
        (true, true, true) => "Ctrl + Shift + Alt",
        (false, true, true) => "Shift + Alt",
        (true, true, false) => "Ctrl + Shift",
        (true, false, true) => "Ctrl + Alt",
        (false, false, true) => "Alt",
        (true, false, false) => "Ctrl",
        (false, true, false) => "Shift",
        (false, false, false) => "",
    };
    let joined = keys.join(" + ");
    if !m.is_empty() && !keys.is_empty() {
        format!("{m} + {joined}")
    } else if !joined.is_empty() {
        joined
    } else {
        m.to_string()
    }
}

/// Port of `mergeTwoHotkeys`.
pub fn merge_two_hotkeys(a: &str, b: &str) -> String {
    let mut keys: Vec<&str> = Vec::new();
    for k in a.split(" + ").chain(b.split(" + ")) {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    hotkey_to_string(&keys, false, false, false)
}

/// A parsed APT shortcut string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The non-modifier key. `None` for modifier-only shortcuts (not registrable).
    pub key: Option<Key>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutParseError {
    Empty,
    UnknownKey(String),
    MultipleKeys,
}

impl fmt::Display for ShortcutParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty shortcut"),
            Self::UnknownKey(k) => write!(f, "unknown key \"{k}\""),
            Self::MultipleKeys => f.write_str("more than one non-modifier key"),
        }
    }
}

impl std::error::Error for ShortcutParseError {}

impl Shortcut {
    /// Parses `Ctrl + Shift + D`. Whitespace around `+` is optional so that
    /// hand-edited configs (`Ctrl+D`) also work. A lone `+` is not a key name
    /// in APT (`Equal` is used instead), so splitting on `+` is unambiguous.
    pub fn parse(s: &str) -> Result<Shortcut, ShortcutParseError> {
        let mut sc = Shortcut { ctrl: false, shift: false, alt: false, key: None };
        let mut any = false;
        for part in s.split('+').map(str::trim) {
            if part.is_empty() {
                continue;
            }
            any = true;
            match part {
                "Ctrl" => sc.ctrl = true,
                "Shift" => sc.shift = true,
                "Alt" => sc.alt = true,
                other => {
                    let key = Key::from_name(other)
                        .ok_or_else(|| ShortcutParseError::UnknownKey(other.to_string()))?;
                    if sc.key.is_some() {
                        return Err(ShortcutParseError::MultipleKeys);
                    }
                    sc.key = Some(key);
                }
            }
        }
        if !any {
            return Err(ShortcutParseError::Empty);
        }
        Ok(sc)
    }

    /// Keys in the order they appear in the canonical string (mods first).
    pub fn keys(&self) -> Vec<Key> {
        let mut v = Vec::new();
        if self.ctrl {
            v.push(Key::CTRL);
        }
        if self.shift {
            v.push(Key::SHIFT);
        }
        if self.alt {
            v.push(Key::ALT);
        }
        if let Some(k) = self.key {
            v.push(k);
        }
        v
    }

    pub fn modifier_keys(&self) -> Vec<Key> {
        self.keys().into_iter().filter(|k| k.is_modifier()).collect()
    }

    /// Converts to a `global-hotkey` registration (the equivalent of upstream
    /// `shortcutToElectron`). `None` for modifier-only shortcuts.
    pub fn to_hotkey(&self) -> Option<HotKey> {
        let code = self.key?.code()?;
        let mut mods = Modifiers::empty();
        if self.ctrl {
            mods |= Modifiers::CONTROL;
        }
        if self.shift {
            mods |= Modifiers::SHIFT;
        }
        if self.alt {
            mods |= Modifiers::ALT;
        }
        Some(HotKey::new(if mods.is_empty() { None } else { Some(mods) }, code))
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys: Vec<&str> = self.key.iter().map(|k| k.name()).collect();
        f.write_str(&hotkey_to_string(&keys, self.ctrl, self.shift, self.alt))
    }
}

/// Modifier state accompanying an input event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Port of `eventToString` from `Shortcuts.ts` (used by the `logKeys` option).
pub fn event_to_string(key: Option<Key>, mods: Mods) -> String {
    let Some(key) = key else { return "not_supported_key".into() };
    if key.is_modifier() {
        return key.name().into();
    }
    hotkey_to_string(&[key.name()], mods.ctrl, mods.shift, mods.alt)
}

/// Converts an overlay-window key press (e.g. from egui/winit, already mapped to
/// APT names) to the canonical string, mirroring `handleExtraCommands` in
/// `OverlayWindow.ts` (which, unlike `hotkeyToString`, never yields all three
/// modifiers).
pub fn overlay_key_string(key_name: &str, ctrl: bool, shift: bool, alt: bool) -> String {
    let code = key_name;
    if shift && alt {
        format!("Shift + Alt + {code}")
    } else if ctrl && shift {
        format!("Ctrl + Shift + {code}")
    } else if ctrl && alt {
        format!("Ctrl + Alt + {code}")
    } else if alt {
        format!("Alt + {code}")
    } else if ctrl {
        format!("Ctrl + {code}")
    } else if shift {
        format!("Shift + {code}")
    } else {
        code.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let s = Shortcut::parse("Ctrl + D").unwrap();
        assert!(s.ctrl && !s.shift && !s.alt);
        assert_eq!(s.key, Key::from_name("D"));
        assert_eq!(s.to_string(), "Ctrl + D");

        let s = Shortcut::parse("Alt + Shift + F5").unwrap();
        assert_eq!(s.to_string(), "Shift + Alt + F5");
        let s = Shortcut::parse("Ctrl+Shift+Alt+Numpad1").unwrap();
        assert_eq!(s.to_string(), "Ctrl + Shift + Alt + Numpad1");
        assert_eq!(Shortcut::parse("Space").unwrap().key, Key::from_name("Space"));
    }

    #[test]
    fn parse_errors() {
        assert_eq!(Shortcut::parse(""), Err(ShortcutParseError::Empty));
        assert_eq!(
            Shortcut::parse("Ctrl + MouseWheel"),
            Err(ShortcutParseError::UnknownKey("MouseWheel".into()))
        );
        assert_eq!(Shortcut::parse("A + B"), Err(ShortcutParseError::MultipleKeys));
        let m = Shortcut::parse("Ctrl").unwrap();
        assert_eq!(m.key, None);
        assert!(m.to_hotkey().is_none());
    }

    #[test]
    fn hotkey_conversion() {
        let hk = Shortcut::parse("Ctrl + Alt + D").unwrap().to_hotkey().unwrap();
        assert_eq!(hk, HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyD));
        let hk = Shortcut::parse("F1").unwrap().to_hotkey().unwrap();
        assert_eq!(hk, HotKey::new(None, Code::F1));
        // every non-modifier key must be registrable
        for k in Key::all().filter(|k| !k.is_modifier()) {
            assert!(k.code().is_some(), "{k}");
        }
    }

    #[test]
    fn hotkey_strings() {
        assert_eq!(hotkey_to_string(&["C"], true, false, false), "Ctrl + C");
        assert_eq!(hotkey_to_string(&["Alt"], false, false, false), "Alt");
        assert_eq!(hotkey_to_string(&[], true, true, false), "Ctrl + Shift");
        assert_eq!(merge_two_hotkeys("Ctrl + C", "Ctrl"), "Ctrl + C");
        assert_eq!(merge_two_hotkeys("Ctrl + C", "Alt"), "Ctrl + Alt + C");
        assert_eq!(merge_two_hotkeys("Ctrl + C", "Shift + Alt"), "Ctrl + Shift + Alt + C");
    }

    #[test]
    fn vk_roundtrip() {
        assert_eq!(Key::from_vk(17), Some(Key::CTRL));
        assert_eq!(Key::from_vk(68).unwrap().name(), "D");
        assert_eq!(Key::from_vk(222).unwrap().name(), "Quote");
        assert_eq!(Key::from_vk(1), None);
    }

    #[test]
    fn event_strings() {
        let d = Key::from_name("D");
        assert_eq!(event_to_string(d, Mods { ctrl: true, ..Default::default() }), "Ctrl + D");
        assert_eq!(event_to_string(Some(Key::ALT), Mods { alt: true, ..Default::default() }), "Alt");
        assert_eq!(event_to_string(None, Mods::default()), "not_supported_key");
        assert_eq!(overlay_key_string("Space", false, true, false), "Shift + Space");
    }
}
