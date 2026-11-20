//! Low-level keyboard/mouse hooks. Hook procedures run on this thread's
//! message loop and must return quickly, so they only forward events.

use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
    SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDOWN,
};

use super::{InputEvent, ListenerFlags, ModSide, ModTracker, Sink};
use crate::keys::Key;

struct HookState {
    sink: Sink,
    flags: Arc<ListenerFlags>,
    mods: Mutex<ModTracker>,
}

static STATE: OnceLock<HookState> = OnceLock::new();

pub(super) fn spawn(sink: Sink, flags: Arc<ListenerFlags>) -> anyhow::Result<()> {
    if STATE.set(HookState { sink, flags, mods: Mutex::new(ModTracker::default()) }).is_err() {
        anyhow::bail!("input listener already started");
    }
    let (tx, rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
    std::thread::Builder::new().name("rpt-input-listener".into()).spawn(move || unsafe {
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0);
        let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0);
        match (kb, ms) {
            (Ok(_), Ok(_)) => {
                let _ = tx.send(Ok(()));
            }
            (Err(e), _) | (_, Err(e)) => {
                let _ = tx.send(Err(e.to_string()));
                return;
            }
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    })?;
    match rx.recv() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => anyhow::bail!("SetWindowsHookExW failed: {e}"),
        Err(_) => anyhow::bail!("input listener thread exited"),
    }
}

fn normalize_vk(vk: u32) -> (u16, Option<ModSide>) {
    match vk {
        0xA0 => (16, Some(ModSide::ShiftL)),
        0xA1 => (16, Some(ModSide::ShiftR)),
        0xA2 => (17, Some(ModSide::CtrlL)),
        0xA3 => (17, Some(ModSide::CtrlR)),
        0xA4 => (18, Some(ModSide::AltL)),
        0xA5 => (18, Some(ModSide::AltR)),
        16 => (16, Some(ModSide::ShiftL)),
        17 => (17, Some(ModSide::CtrlL)),
        18 => (18, Some(ModSide::AltL)),
        other => (other as u16, None),
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(st) = STATE.get() {
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let msg = wparam.0 as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
            if down || up {
                let (vk, side) = normalize_vk(info.vkCode);
                let mods = {
                    let mut m = st.mods.lock();
                    if let Some(side) = side {
                        m.set(side, down);
                    }
                    m.mods()
                };
                let key = Key::from_vk(vk);
                (st.sink)(if down { InputEvent::KeyDown { key, mods } } else { InputEvent::KeyUp { key, mods } });
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(st) = STATE.get() {
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            let (x, y) = (info.pt.x as f64, info.pt.y as f64);
            match wparam.0 as u32 {
                WM_MOUSEMOVE => {
                    if st.flags.want_mouse_move.load(Ordering::Relaxed) {
                        let mods = st.mods.lock().mods();
                        (st.sink)(InputEvent::MouseMove { x, y, mods });
                    }
                }
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
                    (st.sink)(InputEvent::MouseDown { x, y });
                }
                WM_MOUSEWHEEL => {
                    let delta = ((info.mouseData >> 16) & 0xffff) as u16 as i16;
                    if delta != 0 {
                        let mods = st.mods.lock().mods();
                        // WHEEL_DELTA > 0 = away from user (up); uiohook positive = down
                        let rotation = if delta > 0 { -1 } else { 1 };
                        (st.sink)(InputEvent::Wheel { rotation, x, y, mods });
                    }
                }
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
