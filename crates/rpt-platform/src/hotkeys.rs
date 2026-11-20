//! Global shortcut registration (replacement for Electron `globalShortcut`)
//! using `global-hotkey`:
//!
//! * Windows: `RegisterHotKey` on a hidden window. The manager is created on
//!   this dedicated thread, which pumps its Win32 messages.
//! * Linux: `XGrabKey` on the root window of `$DISPLAY` (works under XWayland
//!   while an X11 client — the game — has keyboard focus). The crate runs its
//!   own X11 thread; we keep the same dedicated-thread structure.
//!
//! Like upstream, shortcuts are only registered while the game is focused, so
//! they never steal keys from other applications.

use crossbeam_channel::{Receiver, Sender};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::emit::{Emitter, Level};

pub(crate) struct HotkeyReg {
    pub hotkey: HotKey,
    pub shortcut: String,
    pub test_only: bool,
}

pub(crate) enum HotkeyCmd {
    Register(Vec<HotkeyReg>),
    UnregisterAll,
    Shutdown,
}

pub(crate) struct Hotkeys {
    tx: Sender<HotkeyCmd>,
}

impl Hotkeys {
    pub fn spawn(on_press: Box<dyn Fn(u32) + Send + Sync>, log: Emitter) -> anyhow::Result<Self> {
        GlobalHotKeyEvent::set_event_handler(Some(move |ev: GlobalHotKeyEvent| {
            if ev.state() == HotKeyState::Pressed {
                on_press(ev.id());
            }
        }));
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
        std::thread::Builder::new()
            .name("rpt-hotkeys".into())
            .spawn(move || match GlobalHotKeyManager::new() {
                Ok(manager) => {
                    let _ = ready_tx.send(Ok(()));
                    run(manager, rx, log);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                }
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { tx }),
            Ok(Err(e)) => anyhow::bail!("global hotkeys unavailable: {e}"),
            Err(_) => anyhow::bail!("hotkey thread exited"),
        }
    }

    pub fn send(&self, cmd: HotkeyCmd) {
        let _ = self.tx.send(cmd);
    }
}

fn handle(
    manager: &GlobalHotKeyManager,
    registered: &mut Vec<HotKey>,
    cmd: HotkeyCmd,
    log: &Emitter,
) -> bool {
    match cmd {
        HotkeyCmd::Register(list) => {
            for reg in list {
                match manager.register(reg.hotkey) {
                    Ok(()) => {
                        if reg.test_only {
                            let _ = manager.unregister(reg.hotkey);
                        } else {
                            registered.push(reg.hotkey);
                        }
                    }
                    Err(e) => log.log(
                        Level::Error,
                        "Shortcuts",
                        &format!(
                            "Failed to register a shortcut \"{}\". It is already registered by another application. ({e})",
                            reg.shortcut
                        ),
                    ),
                }
            }
        }
        HotkeyCmd::UnregisterAll => {
            for hk in registered.drain(..) {
                let _ = manager.unregister(hk);
            }
        }
        HotkeyCmd::Shutdown => {
            for hk in registered.drain(..) {
                let _ = manager.unregister(hk);
            }
            return false;
        }
    }
    true
}

#[cfg(not(windows))]
fn run(manager: GlobalHotKeyManager, rx: Receiver<HotkeyCmd>, log: Emitter) {
    let mut registered = Vec::new();
    while let Ok(cmd) = rx.recv() {
        if !handle(&manager, &mut registered, cmd, &log) {
            break;
        }
    }
}

#[cfg(windows)]
fn run(manager: GlobalHotKeyManager, rx: Receiver<HotkeyCmd>, log: Emitter) {
    use std::time::Duration;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    let mut registered = Vec::new();
    loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        match rx.recv_timeout(Duration::from_millis(8)) {
            Ok(cmd) => {
                if !handle(&manager, &mut registered, cmd, &log) {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
}
