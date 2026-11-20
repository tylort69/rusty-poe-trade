//! Port of `AppTray.ts` using `tray-icon`.
//!
//! The tray lives on its own thread with the event loop it needs, so the app
//! never has to integrate it into winit:
//! * Linux: a dedicated thread runs `gtk::init()` + `gtk::main()` (tray-icon's
//!   AppIndicator backend; needs `gtk3` and `libayatana-appindicator` or
//!   `libappindicator-gtk3` at runtime). **The app must not use GTK on any
//!   other thread** (e.g. `rfd` with its `gtk3` feature) — use the XDG portal
//!   backend for file dialogs instead.
//! * Windows: a dedicated thread runs a Win32 message loop (tray-icon creates
//!   its hidden window on the calling thread).
//!
//! Menu: "Settings/League" → [`PlatformEvent::TrayOpenSettings`],
//! "Open config folder" (handled here), "Quit" →
//! [`PlatformEvent::QuitRequested`]. The upstream "Open in Browser" item
//! is omitted because the browser client (`server.ts`) is not ported.

use std::path::PathBuf;
use crossbeam_channel::Sender;

use crate::emit::{Emitter, WakeFn};
use crate::types::PlatformEvent;

pub struct TrayOptions {
    pub tooltip: String,
    /// RGBA8 pixels + size. `None` = built-in placeholder icon.
    pub icon: Option<(Vec<u8>, u32, u32)>,
    /// Folder opened by "Open config folder".
    pub config_dir: PathBuf,
}

/// Handle to the tray thread. Dropping it removes the tray icon.
pub struct Tray {
    #[cfg(target_os = "linux")]
    _priv: (),
    #[cfg(windows)]
    thread_id: u32,
}

fn placeholder_icon() -> (Vec<u8>, u32, u32) {
    // 32x32 amber disc on transparent background.
    let n = 32u32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let inside = dx * dx + dy * dy <= 15.0 * 15.0;
            px.extend_from_slice(if inside { &[0xE0, 0x9A, 0x2B, 0xFF] } else { &[0, 0, 0, 0] });
        }
    }
    (px, n, n)
}

fn build(opts: &TrayOptions) -> anyhow::Result<(tray_icon::TrayIcon, Ids)> {
    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
    let menu = Menu::new();
    let settings = MenuItem::new("Settings/League", true, None);
    let open_cfg = MenuItem::new("Open config folder", true, None);
    let quit = MenuItem::new("Quit", true, None);
    menu.append(&settings)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&open_cfg)?;
    menu.append(&quit)?;
    let (rgba, w, h) = opts.icon.clone().unwrap_or_else(placeholder_icon);
    let icon = tray_icon::Icon::from_rgba(rgba, w, h)?;
    let tray = tray_icon::TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(&opts.tooltip)
        .with_icon(icon)
        .build()?;
    let ids = Ids { settings: settings.id().clone(), open_cfg: open_cfg.id().clone(), quit: quit.id().clone() };
    Ok((tray, ids))
}

struct Ids {
    settings: tray_icon::menu::MenuId,
    open_cfg: tray_icon::menu::MenuId,
    quit: tray_icon::menu::MenuId,
}

fn install_handler(ids: Ids, emit: Emitter, config_dir: PathBuf) {
    tray_icon::menu::MenuEvent::set_event_handler(Some(move |ev: tray_icon::menu::MenuEvent| {
        if ev.id == ids.settings {
            emit.send(PlatformEvent::TrayOpenSettings);
        } else if ev.id == ids.open_cfg {
            let _ = std::fs::create_dir_all(&config_dir);
            if let Err(e) = crate::host_files::open_path(&config_dir) {
                tracing::warn!("open config folder: {e:#}");
            }
        } else if ev.id == ids.quit {
            emit.send(PlatformEvent::QuitRequested);
        }
    }));
}

impl Tray {
    /// Spawns the tray thread. Call once, from any thread, after the event
    /// channel exists. Returns an error if the tray could not be created (no
    /// StatusNotifier host, missing libraries) — the app should continue
    /// without it.
    pub fn spawn(opts: TrayOptions, events: Sender<PlatformEvent>, wake_ui: WakeFn) -> anyhow::Result<Tray> {
        let emit = Emitter::new(events, wake_ui);
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<u32, String>>(1);
        std::thread::Builder::new().name("rpt-tray".into()).spawn(move || platform_thread(opts, emit, ready_tx))?;
        match ready_rx.recv() {
            Ok(Ok(_tid)) => Ok(Tray {
                #[cfg(target_os = "linux")]
                _priv: (),
                #[cfg(windows)]
                thread_id: _tid,
            }),
            Ok(Err(e)) => anyhow::bail!("tray unavailable: {e}"),
            Err(_) => anyhow::bail!("tray thread exited"),
        }
    }
}

#[cfg(target_os = "linux")]
fn platform_thread(opts: TrayOptions, emit: Emitter, ready: Sender<Result<u32, String>>) {
    if let Err(e) = gtk::init() {
        let _ = ready.send(Err(format!("gtk init failed: {e}")));
        return;
    }
    match build(&opts) {
        Ok((tray, ids)) => {
            install_handler(ids, emit, opts.config_dir.clone());
            let _ = ready.send(Ok(0));
            gtk::main();
            drop(tray);
        }
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for Tray {
    fn drop(&mut self) {
        gtk::glib::MainContext::default().invoke(gtk::main_quit);
    }
}

#[cfg(windows)]
fn platform_thread(opts: TrayOptions, emit: Emitter, ready: Sender<Result<u32, String>>) {
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG, TranslateMessage};
    match build(&opts) {
        Ok((tray, ids)) => {
            install_handler(ids, emit, opts.config_dir.clone());
            let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
            unsafe {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            drop(tray);
        }
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
        }
    }
}

#[cfg(windows)]
impl Drop for Tray {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_thread(_opts: TrayOptions, _emit: Emitter, ready: Sender<Result<u32, String>>) {
    let _ = ready.send(Err("tray is only implemented for Linux and Windows".into()));
}

