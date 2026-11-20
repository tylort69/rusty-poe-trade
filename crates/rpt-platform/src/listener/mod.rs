//! Passive global input listener (replacement for `uiohook-napi` events).
//!
//! * Linux: `rdev::listen` (XRecord). Sees input delivered to X11 clients,
//!   which includes the game under XWayland while it is focused.
//! * Windows: our own `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hooks. `rdev` is not used
//!   on Windows because its listener calls `ToUnicodeEx` for every key, which
//!   breaks dead keys for the user's other applications.
//!
//! The listener never blocks or swallows input; suppression of registered
//! shortcuts is done by `global-hotkey` (XGrabKey / RegisterHotKey).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::keys::{Key, Mods};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum InputEvent {
    KeyDown { key: Option<Key>, mods: Mods },
    KeyUp { key: Option<Key>, mods: Mods },
    MouseMove { x: f64, y: f64, mods: Mods },
    MouseDown { x: f64, y: f64 },
    /// `rotation` follows uiohook: positive = wheel scrolled down (towards user).
    Wheel { rotation: i32, x: f64, y: f64, mods: Mods },
}

/// Flags the core sets so the listener can drop high-frequency events early.
#[derive(Default)]
pub(crate) struct ListenerFlags {
    pub want_mouse_move: AtomicBool,
}

pub(crate) type Sink = Box<dyn Fn(InputEvent) + Send + Sync>;

/// Tracks modifier state from key events (uiohook provided it in a mask).
#[derive(Default)]
pub(crate) struct ModTracker {
    ctrl_l: bool,
    ctrl_r: bool,
    shift_l: bool,
    shift_r: bool,
    alt_l: bool,
    alt_r: bool,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // not every side is reported on every OS
pub(crate) enum ModSide {
    CtrlL,
    CtrlR,
    ShiftL,
    ShiftR,
    AltL,
    AltR,
}

impl ModTracker {
    pub fn set(&mut self, side: ModSide, down: bool) {
        match side {
            ModSide::CtrlL => self.ctrl_l = down,
            ModSide::CtrlR => self.ctrl_r = down,
            ModSide::ShiftL => self.shift_l = down,
            ModSide::ShiftR => self.shift_r = down,
            ModSide::AltL => self.alt_l = down,
            ModSide::AltR => self.alt_r = down,
        }
    }

    pub fn mods(&self) -> Mods {
        Mods {
            ctrl: self.ctrl_l || self.ctrl_r,
            shift: self.shift_l || self.shift_r,
            alt: self.alt_l || self.alt_r,
        }
    }
}

pub(crate) struct Listener {
    pub flags: Arc<ListenerFlags>,
}

impl Listener {
    pub fn want_mouse_move(&self, v: bool) {
        self.flags.want_mouse_move.store(v, Ordering::Relaxed);
    }
}

/// Starts the listener thread. Errors (e.g. no X display) are reported but
/// not fatal: shortcuts still work, only stash-scroll / area tracking / Alt
/// visibility / logKeys are lost.
pub(crate) fn spawn(sink: Sink) -> anyhow::Result<Listener> {
    let flags = Arc::new(ListenerFlags::default());
    #[cfg(target_os = "linux")]
    linux::spawn(sink, flags.clone())?;
    #[cfg(windows)]
    windows::spawn(sink, flags.clone())?;
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = sink;
        anyhow::bail!("global input listener is not supported on this OS");
    }
    #[allow(unreachable_code)]
    Ok(Listener { flags })
}
