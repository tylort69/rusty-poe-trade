//! Input synthesis (replacement for `uIOhook.keyTap/keyToggle`) via `enigo`.
//!
//! * Windows: keys are sent as virtual-key codes (`enigo::Key::Other(vk)`),
//!   which is what PoE checks (layout independent, like uiohook).
//! * Linux/X11: enigo's `x11rb` backend uses XTEST. Letters/digits/punctuation
//!   are sent as keysyms (`Key::Unicode`) so the keycode resolved for the
//!   current layout is what Wine maps back to the right VK code.

use enigo::{Direction, Enigo, Keyboard, Mouse, Settings};

use crate::keys::Key;

pub(crate) struct Input {
    enigo: Option<Enigo>,
}

impl Input {
    pub fn new() -> (Self, Option<String>) {
        let settings = Settings { release_keys_when_dropped: true, ..Settings::default() };
        match Enigo::new(&settings) {
            Ok(enigo) => (Self { enigo: Some(enigo) }, None),
            Err(e) => (Self { enigo: None }, Some(format!("Input synthesis unavailable: {e}"))),
        }
    }

    fn key(&mut self, key: Key, dir: Direction) {
        let Some(enigo) = self.enigo.as_mut() else { return };
        let Some(k) = to_enigo(key) else {
            tracing::warn!("no enigo mapping for key {key}");
            return;
        };
        if let Err(e) = enigo.key(k, dir) {
            tracing::warn!("failed to send key {key}: {e}");
        }
    }

    pub fn toggle(&mut self, key: Key, down: bool) {
        self.key(key, if down { Direction::Press } else { Direction::Release });
    }

    /// `uIOhook.keyTap(key, modifiers)`.
    pub fn tap(&mut self, key: Key, modifiers: &[Key]) {
        for m in modifiers {
            self.toggle(*m, true);
        }
        self.key(key, Direction::Click);
        for m in modifiers.iter().rev() {
            self.toggle(*m, false);
        }
    }

    /// Cursor position in physical screen pixels.
    pub fn cursor(&self) -> (i32, i32) {
        self.enigo.as_ref().and_then(|e| e.location().ok()).unwrap_or((0, 0))
    }
}

#[cfg(windows)]
fn to_enigo(key: Key) -> Option<enigo::Key> {
    Some(enigo::Key::Other(key.vk() as u32))
}

#[cfg(not(windows))]
fn to_enigo(key: Key) -> Option<enigo::Key> {
    use enigo::Key as E;
    let vk = key.vk();
    Some(match vk {
        8 => E::Backspace,
        9 => E::Tab,
        13 => E::Return,
        16 => E::Shift,
        17 => E::Control,
        18 => E::Alt,
        20 => E::CapsLock,
        27 => E::Escape,
        32 => E::Space,
        33 => E::PageUp,
        34 => E::PageDown,
        35 => E::End,
        36 => E::Home,
        37 => E::LeftArrow,
        38 => E::UpArrow,
        39 => E::RightArrow,
        40 => E::DownArrow,
        45 => E::Insert,
        46 => E::Delete,
        48..=57 => E::Unicode(char::from(vk as u8)),
        65..=90 => E::Unicode(char::from(vk as u8).to_ascii_lowercase()),
        96 => E::Numpad0,
        97 => E::Numpad1,
        98 => E::Numpad2,
        99 => E::Numpad3,
        100 => E::Numpad4,
        101 => E::Numpad5,
        102 => E::Numpad6,
        103 => E::Numpad7,
        104 => E::Numpad8,
        105 => E::Numpad9,
        106 => E::Multiply,
        107 => E::Add,
        109 => E::Subtract,
        110 => E::Decimal,
        111 => E::Divide,
        112 => E::F1,
        113 => E::F2,
        114 => E::F3,
        115 => E::F4,
        116 => E::F5,
        117 => E::F6,
        118 => E::F7,
        119 => E::F8,
        120 => E::F9,
        121 => E::F10,
        122 => E::F11,
        123 => E::F12,
        124 => E::F13,
        125 => E::F14,
        126 => E::F15,
        127 => E::F16,
        128 => E::F17,
        129 => E::F18,
        130 => E::F19,
        131 => E::F20,
        132 => E::F21,
        133 => E::F22,
        134 => E::F23,
        135 => E::F24,
        186 => E::Unicode(';'),
        187 => E::Unicode('='),
        188 => E::Unicode(','),
        189 => E::Unicode('-'),
        190 => E::Unicode('.'),
        191 => E::Unicode('/'),
        192 => E::Unicode('`'),
        219 => E::Unicode('['),
        220 => E::Unicode('\\'),
        221 => E::Unicode(']'),
        222 => E::Unicode('\''),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_key_maps() {
        for k in crate::keys::Key::all() {
            assert!(super::to_enigo(k).is_some(), "{k}");
        }
    }
}
