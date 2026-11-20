//! X11 (incl. XWayland) game window tracker using `x11rb`.
//!
//! * Watches `_NET_ACTIVE_WINDOW` / `_NET_CLIENT_LIST` on the root window via
//!   `PropertyNotify` (EWMH window manager required — every mainstream
//!   X11 WM and every Wayland compositor's XWayland integration provides it).
//! * Matches the game by exact `_NET_WM_NAME` (UTF-8) or `WM_NAME`.
//! * Bounds: `GetGeometry` + `TranslateCoordinates` of the client window
//!   (Wine's X window is the Windows client area), refreshed on
//!   `ConfigureNotify` and every second as a fallback for WMs that do not send
//!   synthetic ConfigureNotify on frame moves.
//! * Focus: `_NET_ACTIVE_WINDOW` client message with source indication 2
//!   ("pager"), which focus-stealing prevention (KWin, Mutter) honours; for
//!   override-redirect windows `SetInputFocus` is used instead.
//! * The overlay is turned into an override-redirect ("unmanaged") window,
//!   like X11 menus and tooltips. Tiling compositors (Hyprland, Sway…) then
//!   don't tile or focus it, and draw it above a fullscreen game instead of
//!   kicking the game out of fullscreen when the overlay takes input.
//!   Set `RPT_MANAGED_OVERLAY=1` to keep a normal managed window.

use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt, EventMask,
    InputFocus, Window,
};
use x11rb::rust_connection::RustConnection;

use super::{WindowEvent, WindowSink};
use crate::types::{OverlayHandle, Rect};

struct Shared {
    title: String,
    title_changed: bool,
    target: Option<Window>,
    overlay: Option<Window>,
    overlay_unmanaged: bool,
}

#[derive(Clone, Copy)]
struct Atoms {
    net_active_window: Atom,
    net_client_list: Atom,
    net_wm_name: Atom,
    utf8_string: Atom,
}

impl Atoms {
    fn intern(conn: &RustConnection) -> anyhow::Result<Self> {
        let get = |name: &[u8]| -> anyhow::Result<Atom> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
        Ok(Self {
            net_active_window: get(b"_NET_ACTIVE_WINDOW")?,
            net_client_list: get(b"_NET_CLIENT_LIST")?,
            net_wm_name: get(b"_NET_WM_NAME")?,
            utf8_string: get(b"UTF8_STRING")?,
        })
    }
}

pub(super) struct Tracker {
    shared: Arc<Mutex<Shared>>,
    /// Separate connection for synchronous focus requests from other threads.
    focus_conn: Mutex<(RustConnection, Window, Atoms)>,
}

impl Tracker {
    pub fn spawn(title: String, sink: WindowSink) -> anyhow::Result<Self> {
        let (conn, screen) = RustConnection::connect(None)
            .map_err(|e| anyhow::anyhow!("cannot connect to X11 display (is DISPLAY set / XWayland running?): {e}"))?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::intern(&conn)?;

        let (fconn, fscreen) = RustConnection::connect(None)?;
        let froot = fconn.setup().roots[fscreen].root;

        let shared = Arc::new(Mutex::new(Shared { title, title_changed: true, target: None, overlay: None, overlay_unmanaged: false }));
        let sh = shared.clone();
        std::thread::Builder::new().name("rpt-window-tracker".into()).spawn(move || {
            let mut t = Loop { conn, root, atoms, shared: sh, sink, target: None, fg: false, bounds: None };
            if let Err(e) = t.run() {
                tracing::error!("X11 window tracker stopped: {e}");
            }
        })?;
        Ok(Self { shared, focus_conn: Mutex::new((fconn, froot, atoms)) })
    }

    pub fn set_title(&self, title: String) {
        let mut s = self.shared.lock();
        if s.title != title {
            s.title = title;
            s.title_changed = true;
        }
    }

    pub fn set_overlay(&self, handle: OverlayHandle) {
        let win = match handle {
            OverlayHandle::X11 { window } => Some(window),
            _ => None,
        };
        if win.is_some() {
            hyprland_overlay_rule();
        }
        let unmanaged = match win {
            Some(w) if std::env::var_os("RPT_MANAGED_OVERLAY").is_none() => match self.make_unmanaged(w) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!("could not make the overlay an unmanaged window: {e}");
                    false
                }
            },
            _ => false,
        };
        let mut s = self.shared.lock();
        s.overlay = win;
        s.overlay_unmanaged = unmanaged;
    }

    /// Re-map the overlay as override-redirect so the window manager stops managing it.
    fn make_unmanaged(&self, win: Window) -> anyhow::Result<()> {
        let guard = self.focus_conn.lock();
        let conn = &guard.0;
        let attrs = conn.get_window_attributes(win)?.reply()?;
        if attrs.override_redirect {
            return Ok(());
        }
        let mapped = attrs.map_state != x11rb::protocol::xproto::MapState::UNMAPPED;
        if mapped {
            conn.unmap_window(win)?.check()?;
        }
        conn.change_window_attributes(win, &ChangeWindowAttributesAux::new().override_redirect(1))?.check()?;
        if mapped {
            conn.map_window(win)?.check()?;
        }
        conn.flush()?;
        tracing::info!("overlay window {win:#x} is now unmanaged (override-redirect)");
        Ok(())
    }

    pub fn focus_game(&self) -> bool {
        let (target, unmanaged) = {
            let s = self.shared.lock();
            (s.target, s.overlay_unmanaged)
        };
        let Some(w) = target else { return false };
        if unmanaged {
            // The WM still thinks the game is active (an unmanaged overlay never
            // becomes `_NET_ACTIVE_WINDOW`), so its activation request is a no-op:
            // hand the X input focus back explicitly.
            let guard = self.focus_conn.lock();
            if let Ok(c) = guard.0.set_input_focus(InputFocus::PARENT, w, x11rb::CURRENT_TIME) {
                let _ = c.check();
            }
        }
        self.focus(w)
    }

    pub fn focus_overlay(&self) -> bool {
        let overlay = self.shared.lock().overlay;
        overlay.is_some_and(|w| self.focus(w))
    }

    fn focus(&self, win: Window) -> bool {
        let guard = self.focus_conn.lock();
        let (conn, root, atoms) = (&guard.0, guard.1, guard.2);
        let res = (|| -> anyhow::Result<bool> {
            let attrs = conn.get_window_attributes(win)?.reply()?;
            if attrs.override_redirect {
                conn.set_input_focus(InputFocus::PARENT, win, x11rb::CURRENT_TIME)?.check()?;
                return Ok(true);
            }
            let ev = ClientMessageEvent::new(32, win, atoms.net_active_window, [2u32, x11rb::CURRENT_TIME, 0, 0, 0]);
            conn.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev)?;
            conn.flush()?;
            let deadline = Instant::now() + Duration::from_millis(300);
            loop {
                if active_window(conn, root, atoms)? == Some(win) {
                    return Ok(true);
                }
                if Instant::now() >= deadline {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        match res {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("X11 focus request failed: {e}");
                false
            }
        }
    }
}

/// The overlay's X11 class / Wayland app id (`ViewportBuilder::with_app_id`).
const OVERLAY_CLASS: &str = "rusty-poe-trade";

/// On Hyprland, add a runtime-only window rule for the overlay's own class so
/// the compositor doesn't blur (or shadow / dim / animate) everything behind
/// the transparent full-game overlay, and never focuses it: with
/// focus-follows-mouse the overlay is "under the cursor" over the whole game,
/// and keybinds such as fullscreen would hit the overlay instead of the game.
/// The overlay still gets keyboard input when active via X11 `SetInputFocus`. Nothing is written to the user's config;
/// the rule disappears on `hyprctl reload`. `RPT_NO_HYPR_RULES=1` skips it.
fn hyprland_overlay_rule() {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() || std::env::var_os("RPT_NO_HYPR_RULES").is_some() {
        return;
    }
    let run = |args: &[&str]| -> bool {
        match std::process::Command::new("hyprctl").args(args).output() {
            Ok(o) => {
                let out = String::from_utf8_lossy(&o.stdout);
                o.status.success() && out.trim() == "ok"
            }
            Err(_) => false,
        }
    };
    // Hyprland 0.5x+ (Lua config)
    let lua = format!(
        "hl.window_rule({{ name = \"rusty-poe-trade-overlay-{}\", match = {{ class = \"^({OVERLAY_CLASS})$\" }}, no_focus = true, no_blur = true, no_shadow = true, no_dim = true, no_anim = true, border_size = 0 }})",
        std::process::id()
    );
    if run(&["eval", &lua]) {
        tracing::info!("Hyprland: added runtime window rule for the overlay (no focus/blur/shadow/dim/anim)");
        return;
    }
    // Older Hyprland (hyprlang config)
    let ok = ["nofocus", "noblur", "noshadow", "nodim", "noanim", "noborder"]
        .iter()
        .all(|r| run(&["keyword", "windowrulev2", &format!("{r}, class:^({OVERLAY_CLASS})$")]));
    if ok {
        tracing::info!("Hyprland: added runtime windowrulev2 rules for the overlay");
    } else {
        tracing::warn!("Hyprland: could not add the overlay window rule; the game may look blurred behind the overlay");
    }
}

fn active_window(conn: &RustConnection, root: Window, atoms: Atoms) -> anyhow::Result<Option<Window>> {
    let r = conn.get_property(false, root, atoms.net_active_window, AtomEnum::WINDOW, 0, 1)?.reply()?;
    Ok(r.value32().and_then(|mut v| v.next()).filter(|w| *w != 0))
}

struct Loop {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    shared: Arc<Mutex<Shared>>,
    sink: WindowSink,
    target: Option<Window>,
    fg: bool,
    bounds: Option<Rect>,
}

impl Loop {
    fn run(&mut self) -> anyhow::Result<()> {
        self.conn
            .change_window_attributes(self.root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE))?
            .check()?;
        let fd = self.conn.stream().as_raw_fd();
        let mut last_periodic = Instant::now() - Duration::from_secs(10);
        loop {
            while let Some(ev) = self.conn.poll_for_event()? {
                self.handle_event(ev);
            }

            let title_changed = {
                let mut s = self.shared.lock();
                std::mem::replace(&mut s.title_changed, false)
            };
            if title_changed {
                self.detach();
                self.rescan();
            }

            if last_periodic.elapsed() >= Duration::from_secs(1) {
                last_periodic = Instant::now();
                if self.target.is_none() {
                    self.rescan();
                } else {
                    self.validate_target();
                    self.update_bounds();
                }
            }
            self.conn.flush()?;

            let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            // SAFETY: valid pollfd array of length 1.
            unsafe { libc::poll(&mut pfd, 1, 250) };
        }
    }

    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::PropertyNotify(e) if e.window == self.root => {
                if e.atom == self.atoms.net_active_window {
                    self.update_foreground();
                } else if e.atom == self.atoms.net_client_list && self.target.is_none() {
                    self.rescan();
                }
            }
            Event::PropertyNotify(e) if Some(e.window) == self.target => {
                if e.atom == self.atoms.net_wm_name || e.atom == u32::from(AtomEnum::WM_NAME) {
                    self.validate_target();
                }
            }
            Event::ConfigureNotify(e) if Some(e.window) == self.target => self.update_bounds(),
            Event::MapNotify(e) if Some(e.window) == self.target => self.update_bounds(),
            Event::DestroyNotify(e) if Some(e.window) == self.target => self.detach(),
            _ => {}
        }
    }

    fn window_title(&self, w: Window) -> Option<String> {
        let r = self.conn.get_property(false, w, self.atoms.net_wm_name, self.atoms.utf8_string, 0, 1024).ok()?.reply().ok()?;
        if !r.value.is_empty() {
            return Some(String::from_utf8_lossy(&r.value).into_owned());
        }
        let r = self.conn.get_property(false, w, AtomEnum::WM_NAME, AtomEnum::ANY, 0, 1024).ok()?.reply().ok()?;
        if !r.value.is_empty() {
            // STRING is Latin-1; COMPOUND_TEXT is close enough for ASCII titles.
            return Some(r.value.iter().map(|b| *b as char).collect());
        }
        None
    }

    fn candidate_windows(&self) -> Vec<Window> {
        if let Ok(Ok(r)) = self
            .conn
            .get_property(false, self.root, self.atoms.net_client_list, AtomEnum::WINDOW, 0, u32::MAX)
            .map(|c| c.reply())
            && let Some(v) = r.value32() {
                let list: Vec<Window> = v.collect();
                if !list.is_empty() {
                    return list;
                }
            }
        // No EWMH client list: walk two levels of the tree (frames + clients).
        let mut out = Vec::new();
        if let Ok(Ok(tree)) = self.conn.query_tree(self.root).map(|c| c.reply()) {
            for w in tree.children {
                out.push(w);
                if let Ok(Ok(sub)) = self.conn.query_tree(w).map(|c| c.reply()) {
                    out.extend(sub.children);
                }
            }
        }
        out
    }

    fn rescan(&mut self) {
        let title = self.shared.lock().title.clone();
        let found = self.candidate_windows().into_iter().find(|w| self.window_title(*w).as_deref() == Some(title.as_str()));
        if let Some(w) = found {
            self.attach(w);
        }
    }

    fn attach(&mut self, w: Window) {
        self.target = Some(w);
        self.shared.lock().target = Some(w);
        let mask = EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE;
        if let Ok(c) = self.conn.change_window_attributes(w, &ChangeWindowAttributesAux::new().event_mask(mask)) {
            c.ignore_error();
        }
        // Under X11 any client may synthesize input, so access is always granted.
        (self.sink)(WindowEvent::Attached { has_access: true });
        self.update_bounds();
        self.update_foreground();
    }

    fn detach(&mut self) {
        if self.target.take().is_some() {
            self.shared.lock().target = None;
            self.bounds = None;
            (self.sink)(WindowEvent::Detached);
            (self.sink)(WindowEvent::Bounds(None));
            if self.fg {
                self.fg = false;
                (self.sink)(WindowEvent::Foreground { game: false });
            }
        }
    }

    fn validate_target(&mut self) {
        let Some(w) = self.target else { return };
        let title = self.shared.lock().title.clone();
        match self.window_title(w) {
            Some(t) if t == title => {}
            _ => self.detach(),
        }
    }

    fn update_foreground(&mut self) {
        let active = active_window(&self.conn, self.root, self.atoms).ok().flatten();
        let fg = self.target.is_some() && active == self.target;
        if fg != self.fg {
            self.fg = fg;
            (self.sink)(WindowEvent::Foreground { game: fg });
        }
    }

    fn update_bounds(&mut self) {
        let Some(w) = self.target else { return };
        let r = (|| -> anyhow::Result<Rect> {
            let g = self.conn.get_geometry(w)?.reply()?;
            let t = self.conn.translate_coordinates(w, self.root, 0, 0)?.reply()?;
            Ok(Rect { x: t.dst_x as i32, y: t.dst_y as i32, width: g.width as i32, height: g.height as i32 })
        })();
        match r {
            Ok(rect) => {
                if self.bounds != Some(rect) {
                    self.bounds = Some(rect);
                    (self.sink)(WindowEvent::Bounds(Some(rect)));
                }
            }
            Err(_) => self.detach(),
        }
        self.sync_overlay();
    }

    /// Keep an unmanaged overlay exactly over the game. Window-manager-style
    /// move/resize requests from the toolkit aren't reliable for
    /// override-redirect windows, so configure it directly. One pixel short
    /// of the game's height: Hyprland treats an unmanaged window covering a
    /// whole monitor as a fullscreen client.
    fn sync_overlay(&self) {
        let (overlay, unmanaged) = {
            let s = self.shared.lock();
            (s.overlay, s.overlay_unmanaged)
        };
        let (Some(ov), true, Some(b)) = (overlay, unmanaged, self.bounds) else { return };
        let want = (b.x, b.y, b.width.max(1) as u32, (b.height - 1).max(1) as u32);
        let cur = (|| -> anyhow::Result<(i32, i32, u32, u32)> {
            let g = self.conn.get_geometry(ov)?.reply()?;
            let t = self.conn.translate_coordinates(ov, self.root, 0, 0)?.reply()?;
            Ok((t.dst_x as i32, t.dst_y as i32, g.width as u32, g.height as u32))
        })();
        if cur.ok() == Some(want) {
            return;
        }
        let aux = x11rb::protocol::xproto::ConfigureWindowAux::new().x(want.0).y(want.1).width(want.2).height(want.3).stack_mode(x11rb::protocol::xproto::StackMode::ABOVE);
        match self.conn.configure_window(ov, &aux) {
            Ok(c) => {
                c.ignore_error();
                let _ = self.conn.flush();
                tracing::debug!("overlay moved over the game: {want:?} (was {cur:?})");
            }
            Err(e) => tracing::warn!("could not move the overlay over the game: {e}"),
        }
    }
}
