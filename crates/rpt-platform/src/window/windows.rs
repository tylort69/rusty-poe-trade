//! Win32 game window tracker.
//!
//! * `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` (global, out-of-context) for
//!   foreground changes; `EVENT_OBJECT_LOCATIONCHANGE` and
//!   `EVENT_OBJECT_DESTROY` hooked only for the game's thread once found.
//! * A 1s timer re-finds the window (`FindWindowW` with the exact title) and
//!   validates it.
//! * Bounds: `GetClientRect` + `ClientToScreen`. These are physical pixels as
//!   long as the process is per-monitor DPI aware (winit sets PMv2 by default).
//! * Access check: if the game process is elevated and we are not, input
//!   injection and hotkeys will not reach it (UIPI) → `has_access: false`.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use parking_lot::Mutex;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_DESTROY, EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND,
    FindWindowW, GetClientRect, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, IsIconic,
    IsWindow, MSG, OBJID_WINDOW, PostThreadMessageW, SetForegroundWindow, SetTimer, TranslateMessage,
    WINEVENT_OUTOFCONTEXT, WM_APP, WM_TIMER,
};
use windows::core::HSTRING;

use super::{WindowEvent, WindowSink};
use crate::types::{OverlayHandle, Rect};

struct State {
    title: String,
    sink: WindowSink,
    target: Option<isize>,
    fg: bool,
    bounds: Option<Rect>,
    target_hook: Option<isize>,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static TARGET: AtomicIsize = AtomicIsize::new(0);
static OVERLAY: AtomicIsize = AtomicIsize::new(0);
static THREAD_ID: AtomicU32 = AtomicU32::new(0);

pub(super) struct Tracker;

impl Tracker {
    pub fn spawn(title: String, sink: WindowSink) -> anyhow::Result<Self> {
        let st = State { title, sink, target: None, fg: false, bounds: None, target_hook: None };
        if STATE.set(Mutex::new(st)).is_err() {
            anyhow::bail!("window tracker already started");
        }
        std::thread::Builder::new().name("rpt-window-tracker".into()).spawn(|| unsafe {
            THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
            let _fg_hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            SetTimer(None, 0, 1000, None);
            rescan();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_TIMER || msg.message == WM_APP {
                    periodic(msg.message == WM_APP);
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })?;
        Ok(Self)
    }

    pub fn set_title(&self, title: String) {
        if let Some(st) = STATE.get() {
            let mut s = st.lock();
            if s.title == title {
                return;
            }
            s.title = title;
        }
        let tid = THREAD_ID.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_APP, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn set_overlay(&self, handle: OverlayHandle) {
        let v = match handle {
            OverlayHandle::Win32 { hwnd } => hwnd,
            _ => 0,
        };
        OVERLAY.store(v, Ordering::SeqCst);
    }

    pub fn focus_game(&self) -> bool {
        focus_hwnd(TARGET.load(Ordering::SeqCst))
    }

    pub fn focus_overlay(&self) -> bool {
        focus_hwnd(OVERLAY.load(Ordering::SeqCst))
    }
}

fn hwnd(v: isize) -> HWND {
    HWND(v as *mut core::ffi::c_void)
}

fn focus_hwnd(v: isize) -> bool {
    if v == 0 {
        return false;
    }
    let target = hwnd(v);
    unsafe {
        let fg = GetForegroundWindow();
        if fg == target {
            return true;
        }
        let cur = GetCurrentThreadId();
        let fg_tid = GetWindowThreadProcessId(fg, None);
        let attached = fg_tid != 0 && fg_tid != cur && AttachThreadInput(cur, fg_tid, true).as_bool();
        let mut ok = SetForegroundWindow(target).as_bool();
        if attached {
            let _ = AttachThreadInput(cur, fg_tid, false);
        }
        if !ok {
            // Foreground lock: a synthetic Alt tap makes us "the last input
            // source", which allows SetForegroundWindow.
            let mk = |flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MENU, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
            };
            let inputs = [mk(Default::default()), mk(KEYEVENTF_KEYUP)];
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            ok = SetForegroundWindow(target).as_bool();
        }
        for _ in 0..30 {
            if GetForegroundWindow() == target {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        ok
    }
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd_ev: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    match event {
        EVENT_SYSTEM_FOREGROUND => update_foreground(),
        EVENT_OBJECT_LOCATIONCHANGE | EVENT_OBJECT_DESTROY => {
            if id_object != OBJID_WINDOW.0 || hwnd_ev.0 as isize != TARGET.load(Ordering::SeqCst) {
                return;
            }
            if event == EVENT_OBJECT_DESTROY {
                detach();
            } else {
                update_bounds();
            }
        }
        _ => {}
    }
}

fn periodic(title_changed: bool) {
    if title_changed {
        detach();
    }
    let target = TARGET.load(Ordering::SeqCst);
    if target == 0 {
        rescan();
        return;
    }
    let title = STATE.get().map(|s| s.lock().title.clone()).unwrap_or_default();
    let still_valid = unsafe { IsWindow(Some(hwnd(target))).as_bool() }
        && unsafe { FindWindowW(None, &HSTRING::from(title.as_str())) }
            .map(|h| h.0 as isize == target)
            .unwrap_or(false);
    if still_valid {
        update_bounds();
    } else {
        detach();
        rescan();
    }
}

fn rescan() {
    let Some(st) = STATE.get() else { return };
    let title = st.lock().title.clone();
    let Ok(h) = (unsafe { FindWindowW(None, &HSTRING::from(title.as_str())) }) else { return };
    if h.0.is_null() {
        return;
    }
    let v = h.0 as isize;
    TARGET.store(v, Ordering::SeqCst);
    let mut pid = 0u32;
    let tid = unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(win_event_proc),
            pid,
            tid,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    let has_access = has_access(pid);
    {
        let mut s = st.lock();
        s.target = Some(v);
        s.target_hook = if hook.is_invalid() { None } else { Some(hook.0 as isize) };
        (s.sink)(WindowEvent::Attached { has_access });
    }
    update_bounds();
    update_foreground();
}

fn detach() {
    let Some(st) = STATE.get() else { return };
    let mut s = st.lock();
    if s.target.take().is_none() {
        return;
    }
    TARGET.store(0, Ordering::SeqCst);
    if let Some(h) = s.target_hook.take() {
        unsafe {
            let _ = UnhookWinEvent(HWINEVENTHOOK(h as *mut core::ffi::c_void));
        }
    }
    s.bounds = None;
    (s.sink)(WindowEvent::Detached);
    (s.sink)(WindowEvent::Bounds(None));
    if s.fg {
        s.fg = false;
        (s.sink)(WindowEvent::Foreground { game: false });
    }
}

fn update_foreground() {
    let Some(st) = STATE.get() else { return };
    let target = TARGET.load(Ordering::SeqCst);
    let fg = target != 0 && unsafe { GetForegroundWindow() }.0 as isize == target;
    let mut s = st.lock();
    if s.fg != fg {
        s.fg = fg;
        (s.sink)(WindowEvent::Foreground { game: fg });
    }
}

fn update_bounds() {
    let Some(st) = STATE.get() else { return };
    let target = TARGET.load(Ordering::SeqCst);
    if target == 0 {
        return;
    }
    let h = hwnd(target);
    unsafe {
        if IsIconic(h).as_bool() {
            return;
        }
        let mut rc = RECT::default();
        if GetClientRect(h, &mut rc).is_err() {
            return;
        }
        let mut pt = POINT { x: 0, y: 0 };
        if !ClientToScreen(h, &mut pt).as_bool() {
            return;
        }
        let rect = Rect { x: pt.x, y: pt.y, width: rc.right - rc.left, height: rc.bottom - rc.top };
        let mut s = st.lock();
        if s.bounds != Some(rect) {
            s.bounds = Some(rect);
            (s.sink)(WindowEvent::Bounds(Some(rect)));
        }
    }
}

fn token_elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut elev = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let r = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        r.ok()?;
        Some(elev.TokenIsElevated != 0)
    }
}

fn has_access(pid: u32) -> bool {
    let self_elevated = token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false);
    if self_elevated {
        return true;
    }
    let Ok(proc_handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return true;
    };
    let game_elevated = token_elevated(proc_handle);
    unsafe {
        let _ = CloseHandle(proc_handle);
    }
    // If we cannot read the game's token while unelevated, it is most likely
    // elevated (OpenProcessToken is denied across integrity levels).
    !game_elevated.unwrap_or(true)
}
