use std::sync::Arc;
use std::sync::atomic::Ordering;

use rdev::{EventType, Key as RKey};

use super::{InputEvent, ListenerFlags, ModSide, ModTracker, Sink};
use crate::keys::Key;

pub(super) fn spawn(sink: Sink, flags: Arc<ListenerFlags>) -> anyhow::Result<()> {
    if std::env::var_os("DISPLAY").is_none() {
        anyhow::bail!("DISPLAY is not set; the global input listener needs X11/XWayland");
    }
    std::thread::Builder::new().name("rpt-input-listener".into()).spawn(move || {
        let mut mods = ModTracker::default();
        let mut pos = (0.0f64, 0.0f64);
        let res = rdev::listen(move |ev| match ev.event_type {
            EventType::KeyPress(k) => {
                if let Some(side) = mod_side(k) {
                    mods.set(side, true);
                }
                sink(InputEvent::KeyDown { key: map_key(k), mods: mods.mods() });
            }
            EventType::KeyRelease(k) => {
                if let Some(side) = mod_side(k) {
                    mods.set(side, false);
                }
                sink(InputEvent::KeyUp { key: map_key(k), mods: mods.mods() });
            }
            EventType::MouseMove { x, y } => {
                pos = (x, y);
                if flags.want_mouse_move.load(Ordering::Relaxed) {
                    sink(InputEvent::MouseMove { x, y, mods: mods.mods() });
                }
            }
            EventType::ButtonPress(_) => sink(InputEvent::MouseDown { x: pos.0, y: pos.1 }),
            EventType::ButtonRelease(_) => {}
            EventType::Wheel { delta_y, .. } => {
                if delta_y != 0 {
                    // rdev: +1 = up; uiohook rotation: positive = down
                    sink(InputEvent::Wheel {
                        rotation: -(delta_y as i32),
                        x: pos.0,
                        y: pos.1,
                        mods: mods.mods(),
                    });
                }
            }
        });
        if let Err(e) = res {
            tracing::error!("global input listener stopped: {e:?}");
        }
    })?;
    Ok(())
}

fn mod_side(k: RKey) -> Option<ModSide> {
    Some(match k {
        RKey::ControlLeft => ModSide::CtrlL,
        RKey::ControlRight => ModSide::CtrlR,
        RKey::ShiftLeft => ModSide::ShiftL,
        RKey::ShiftRight => ModSide::ShiftR,
        RKey::Alt => ModSide::AltL,
        RKey::AltGr => ModSide::AltR,
        _ => return None,
    })
}

pub(super) fn map_key(k: RKey) -> Option<Key> {
    let name = match k {
        RKey::Alt | RKey::AltGr => "Alt",
        RKey::ControlLeft | RKey::ControlRight => "Ctrl",
        RKey::ShiftLeft | RKey::ShiftRight => "Shift",
        RKey::Backspace => "Backspace",
        RKey::CapsLock => "CapsLock",
        RKey::Delete => "Delete",
        RKey::DownArrow => "ArrowDown",
        RKey::UpArrow => "ArrowUp",
        RKey::LeftArrow => "ArrowLeft",
        RKey::RightArrow => "ArrowRight",
        RKey::End => "End",
        RKey::Home => "Home",
        RKey::Escape => "Escape",
        RKey::PageDown => "PageDown",
        RKey::PageUp => "PageUp",
        RKey::Return | RKey::KpReturn => "Enter",
        RKey::Space => "Space",
        RKey::Tab => "Tab",
        RKey::Insert => "Insert",
        RKey::F1 => "F1",
        RKey::F2 => "F2",
        RKey::F3 => "F3",
        RKey::F4 => "F4",
        RKey::F5 => "F5",
        RKey::F6 => "F6",
        RKey::F7 => "F7",
        RKey::F8 => "F8",
        RKey::F9 => "F9",
        RKey::F10 => "F10",
        RKey::F11 => "F11",
        RKey::F12 => "F12",
        RKey::BackQuote => "Backquote",
        RKey::Num0 => "0",
        RKey::Num1 => "1",
        RKey::Num2 => "2",
        RKey::Num3 => "3",
        RKey::Num4 => "4",
        RKey::Num5 => "5",
        RKey::Num6 => "6",
        RKey::Num7 => "7",
        RKey::Num8 => "8",
        RKey::Num9 => "9",
        RKey::Minus => "Minus",
        RKey::Equal => "Equal",
        RKey::LeftBracket => "BracketLeft",
        RKey::RightBracket => "BracketRight",
        RKey::SemiColon => "Semicolon",
        RKey::Quote => "Quote",
        RKey::BackSlash | RKey::IntlBackslash => "Backslash",
        RKey::Comma => "Comma",
        RKey::Dot => "Period",
        RKey::Slash => "Slash",
        RKey::KeyA => "A",
        RKey::KeyB => "B",
        RKey::KeyC => "C",
        RKey::KeyD => "D",
        RKey::KeyE => "E",
        RKey::KeyF => "F",
        RKey::KeyG => "G",
        RKey::KeyH => "H",
        RKey::KeyI => "I",
        RKey::KeyJ => "J",
        RKey::KeyK => "K",
        RKey::KeyL => "L",
        RKey::KeyM => "M",
        RKey::KeyN => "N",
        RKey::KeyO => "O",
        RKey::KeyP => "P",
        RKey::KeyQ => "Q",
        RKey::KeyR => "R",
        RKey::KeyS => "S",
        RKey::KeyT => "T",
        RKey::KeyU => "U",
        RKey::KeyV => "V",
        RKey::KeyW => "W",
        RKey::KeyX => "X",
        RKey::KeyY => "Y",
        RKey::KeyZ => "Z",
        RKey::KpMinus => "NumpadSubtract",
        RKey::KpPlus => "NumpadAdd",
        RKey::KpMultiply => "NumpadMultiply",
        RKey::KpDivide => "NumpadDivide",
        RKey::KpDelete => "NumpadDecimal",
        RKey::Kp0 => "Numpad0",
        RKey::Kp1 => "Numpad1",
        RKey::Kp2 => "Numpad2",
        RKey::Kp3 => "Numpad3",
        RKey::Kp4 => "Numpad4",
        RKey::Kp5 => "Numpad5",
        RKey::Kp6 => "Numpad6",
        RKey::Kp7 => "Numpad7",
        RKey::Kp8 => "Numpad8",
        RKey::Kp9 => "Numpad9",
        _ => return None,
    };
    Key::from_name(name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_keys() {
        assert_eq!(super::map_key(rdev::Key::KeyD).unwrap().name(), "D");
        assert_eq!(super::map_key(rdev::Key::ControlRight).unwrap().name(), "Ctrl");
        assert!(super::map_key(rdev::Key::MetaLeft).is_none());
    }
}
