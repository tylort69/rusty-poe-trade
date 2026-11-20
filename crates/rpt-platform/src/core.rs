//! The platform worker thread: owns all mutable state and performs every
//! side effect in order (like upstream's single-threaded main process).
//! Combines `Shortcuts.ts`, `OverlayWindow.ts` (focus state machine),
//! `OverlayVisibility.ts`, `WidgetAreaTracker.ts` and the host-config
//! fan-out from `main.ts`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError};

use crate::clipboard::HostClipboard;
use crate::emit::{Emitter, Level};
use crate::host_files::GameConfig;
use crate::host_files::game_log::GameLogWatcher;
use crate::hotkeys::{HotkeyCmd, HotkeyReg, Hotkeys};
use crate::input::Input;
use crate::keys::{Key, event_to_string};
use crate::listener::{InputEvent, Listener};
use crate::shortcuts::{
    self, ActiveShortcut, Tap, copy_item_modifiers, is_stash_area, keys_to_release, type_in_chat_plan,
};
use crate::types::{HostConfig, OverlayHandle, PlatformEvent, Rect, ShortcutAction, TrackArea};
use crate::updater::{Updater, UpdaterCmd};
use crate::window::{GameWindowTracker, WindowEvent};

pub(crate) enum Cmd {
    Config(Box<HostConfig>),
    Hotkey(u32),
    Input(InputEvent),
    Window(WindowEvent),
    SetOverlayHandle(OverlayHandle),
    SetOverlayActive(bool),
    ToggleOverlay,
    FocusGame,
    StashSearch(String),
    PasteInChat(String, bool),
    TrackArea(TrackArea),
    CancelTrackArea,
    UsedRecently(bool),
    CheckForUpdate,
    Shutdown,
}

pub(crate) struct Subsystems {
    pub hotkeys: Option<Hotkeys>,
    pub tracker: Option<GameWindowTracker>,
    pub listener: Option<Listener>,
    pub log_watcher: Option<GameLogWatcher>,
    pub updater: Option<Updater>,
}

pub(crate) struct Core {
    emit: Emitter,
    sys: Subsystems,
    input: Input,
    clip: HostClipboard,

    config: Option<HostConfig>,
    actions: Vec<ActiveShortcut>,
    by_id: HashMap<u32, usize>,
    registered: bool,
    game_config: Option<GameConfig>,

    // OverlayWindow / GameWindow state
    game_active: bool,
    interactable: bool,
    overlay_key_used: bool,
    used_recently: bool,
    last_focus: Option<(bool, bool)>,

    bounds: Option<Rect>,
    game_foreground: bool,

    area: Option<TrackArea>,

    visible: bool,
    vis_timer: Option<Instant>,
}

impl Core {
    pub fn new(emit: Emitter, sys: Subsystems) -> Self {
        let (input, err) = Input::new();
        if let Some(e) = err {
            emit.log(Level::Error, "Shortcuts", &e);
        }
        let (clip, err) = HostClipboard::new();
        if let Some(e) = err {
            emit.log(Level::Error, "Clipboard", &e);
        }
        Self {
            emit,
            sys,
            input,
            clip,
            config: None,
            actions: Vec::new(),
            by_id: HashMap::new(),
            registered: false,
            game_config: None,
            game_active: false,
            interactable: false,
            overlay_key_used: false,
            used_recently: true,
            last_focus: None,
            bounds: None,
            game_foreground: false,
            area: None,
            visible: true,
            vis_timer: None,
        }
    }

    pub fn run(mut self, rx: Receiver<Cmd>) {
        // initial state so the UI does not have to assume it
        self.sync_focus();
        loop {
            let deadline = [self.clip.restore_deadline(), self.vis_timer].into_iter().flatten().min();
            let msg = match deadline {
                Some(d) => match rx.recv_timeout(d.saturating_duration_since(Instant::now())) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(m) => Some(m),
                    Err(_) => break,
                },
            };
            let now = Instant::now();
            self.clip.on_timer(now);
            if self.vis_timer.is_some_and(|t| t <= now) {
                self.vis_timer = None;
                self.visible = false;
                self.emit.send(PlatformEvent::Visibility { visible: false });
                self.update_listener_flags();
            }
            match msg {
                Some(Cmd::Shutdown) => break,
                Some(cmd) => self.handle(cmd),
                None => {}
            }
        }
        if let Some(h) = &self.sys.hotkeys {
            h.send(HotkeyCmd::Shutdown);
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Config(cfg) => self.update_config(*cfg),
            Cmd::Hotkey(id) => self.on_hotkey(id),
            Cmd::Input(ev) => self.on_input(ev),
            Cmd::Window(ev) => self.on_window(ev),
            Cmd::SetOverlayHandle(h) => {
                if let Some(t) = &self.sys.tracker {
                    t.set_overlay(h);
                }
            }
            Cmd::SetOverlayActive(true) => self.assert_overlay_active(),
            Cmd::SetOverlayActive(false) => self.assert_game_active(),
            Cmd::ToggleOverlay => self.toggle_active_state(),
            Cmd::FocusGame => self.assert_game_active(),
            Cmd::StashSearch(text) => self.stash_search(&text),
            Cmd::PasteInChat(text, send) => self.type_in_chat(&text, send),
            Cmd::TrackArea(req) => {
                self.area = Some(req);
                self.update_listener_flags();
            }
            Cmd::CancelTrackArea => self.remove_area_listeners(),
            Cmd::UsedRecently(v) => self.used_recently = v,
            Cmd::CheckForUpdate => {
                if let Some(u) = &self.sys.updater {
                    u.send(UpdaterCmd::Check);
                }
            }
            Cmd::Shutdown => {}
        }
    }

    // ----- host config (main.ts `update-host-config`) -----

    fn update_config(&mut self, cfg: HostConfig) {
        let prev = self.config.take();
        if let Some(t) = &self.sys.tracker {
            t.set_title(cfg.window_title.clone());
        }
        self.clip.should_restore = cfg.restore_clipboard;

        let (actions, errors) = shortcuts::filter_actions(&cfg.shortcuts);
        for e in errors {
            self.emit.log(Level::Error, "Shortcuts", &e);
        }
        self.actions = actions;
        self.by_id = self
            .actions
            .iter()
            .enumerate()
            .filter_map(|(i, a)| a.parsed.to_hotkey().map(|hk| (hk.id(), i)))
            .collect();
        if self.registered {
            self.unregister();
            self.register();
        }

        if let Some(w) = &self.sys.log_watcher {
            w.restart(cfg.client_log.clone());
        }
        if prev.as_ref().map(|p| &p.game_config) != Some(&cfg.game_config) {
            match GameConfig::read(cfg.game_config.as_deref()) {
                Ok(gc) => self.game_config = Some(gc),
                Err(e) => {
                    self.game_config = None;
                    self.emit.log(Level::Error, "GameConfig", &e.to_string());
                }
            }
        }
        if let Some(u) = &self.sys.updater {
            u.send(UpdaterCmd::CheckAtStartup);
        }
        self.config = Some(cfg);
    }

    fn log_keys(&self) -> bool {
        self.config.as_ref().is_some_and(|c| c.log_keys)
    }

    // ----- registration (Shortcuts.register/unregister) -----

    fn register(&mut self) {
        let Some(h) = &self.sys.hotkeys else { return };
        let regs = self
            .actions
            .iter()
            .filter_map(|a| {
                let hk = a.parsed.to_hotkey();
                if hk.is_none() {
                    self.emit.log(
                        Level::Error,
                        "Shortcuts",
                        &format!("Failed to register a shortcut \"{}\": modifier-only shortcuts are not supported.", a.binding.shortcut),
                    );
                }
                Some(HotkeyReg {
                    hotkey: hk?,
                    shortcut: a.binding.shortcut.clone(),
                    test_only: a.binding.action == ShortcutAction::TestOnly,
                })
            })
            .collect();
        h.send(HotkeyCmd::Register(regs));
        self.registered = true;
    }

    fn unregister(&mut self) {
        if let Some(h) = &self.sys.hotkeys {
            h.send(HotkeyCmd::UnregisterAll);
        }
        self.registered = false;
    }

    // ----- OverlayWindow focus state machine -----

    fn set_game_active(&mut self, active: bool) {
        if self.game_active == active {
            return;
        }
        self.game_active = active;
        // Shortcuts: register only while the game is focused
        if active {
            self.register();
        } else {
            self.unregister();
        }
        // OverlayWindow.handlePoeWindowActiveChange
        if active && self.interactable {
            self.interactable = false;
        }
        self.sync_focus();
    }

    fn sync_focus(&mut self) {
        let cur = (self.game_active, self.interactable);
        if self.last_focus != Some(cur) {
            self.last_focus = Some(cur);
            self.emit.send(PlatformEvent::FocusChange {
                game: cur.0,
                overlay: cur.1,
                using_hotkey: self.overlay_key_used,
            });
            self.overlay_key_used = false;
        }
    }

    fn assert_overlay_active(&mut self) {
        if !self.interactable {
            self.interactable = true;
            if let Some(t) = &self.sys.tracker
                && !t.focus_overlay() {
                    tracing::debug!("could not focus the overlay window");
                }
            self.set_game_active(false);
            self.sync_focus();
        }
    }

    fn refocus_overlay(&self) {
        tracing::info!("game re-activated while the overlay is open: giving input back to the overlay");
        if let Some(t) = &self.sys.tracker
            && !t.focus_overlay() {
                tracing::debug!("could not re-focus the overlay window");
            }
    }

    fn assert_game_active(&mut self) {
        if self.interactable {
            self.interactable = false;
            if let Some(t) = &self.sys.tracker
                && !t.focus_game() {
                    tracing::debug!("could not focus the game window");
                }
            self.set_game_active(true);
            self.sync_focus();
        }
    }

    fn toggle_active_state(&mut self) {
        self.overlay_key_used = true;
        if self.interactable {
            self.assert_game_active();
        } else {
            self.assert_overlay_active();
        }
        self.overlay_key_used = false;
    }

    // ----- window tracker -----

    fn on_window(&mut self, ev: WindowEvent) {
        match ev {
            WindowEvent::Attached { has_access: true } => self.emit.send(PlatformEvent::OverlayAttached),
            WindowEvent::Attached { has_access: false } => {
                self.emit.log(Level::Error, "Overlay", "PoE is running with administrator rights");
                self.emit.send(PlatformEvent::GameNoAccess);
            }
            WindowEvent::Detached => {}
            WindowEvent::Foreground { game } => {
                self.game_foreground = game;
                self.emit_game_window();
                if game && self.interactable && cfg!(target_os = "linux") {
                    // Back to the game while the overlay is open: the window
                    // manager handed input to the game, but the overlay is still
                    // shown on top. Keep it usable instead of leaving an open,
                    // click-through overlay. (Esc / overlay key return to the game.)
                    self.refocus_overlay();
                    return;
                }
                self.set_game_active(game);
            }
            WindowEvent::GameReactivated => {
                if self.interactable {
                    self.refocus_overlay();
                }
            }
            WindowEvent::Bounds(b) => {
                self.bounds = b;
                self.emit_game_window();
            }
        }
    }

    fn emit_game_window(&self) {
        self.emit.send(PlatformEvent::GameWindow { bounds: self.bounds, focused: self.game_foreground });
    }

    // ----- hotkey actions (Shortcuts.register callback) -----

    fn on_hotkey(&mut self, id: u32) {
        let Some(entry) = self.by_id.get(&id).and_then(|i| self.actions.get(*i)).cloned() else { return };
        if !self.registered {
            return;
        }
        if self.log_keys() {
            self.emit.log(Level::Debug, "Shortcuts", &format!("Action type: {}", entry.binding.action.type_name()));
        }

        for k in keys_to_release(&entry.parsed, entry.binding.keep_mod_keys) {
            self.input.toggle(k, false);
        }

        match &entry.binding.action {
            ShortcutAction::ToggleOverlay => {
                self.remove_area_listeners();
                self.emit.send(PlatformEvent::ToggleOverlay);
                self.toggle_active_state();
            }
            ShortcutAction::PasteInChat { text, send } => self.type_in_chat(text, *send),
            ShortcutAction::TriggerEvent { target } => {
                self.emit.send(PlatformEvent::WidgetAction { target: target.clone() })
            }
            ShortcutAction::StashSearch { text } => self.stash_search(text),
            ShortcutAction::CopyItem { target, focus_overlay } => {
                let position = self.input.cursor();
                let before = self.clip.prepare_item_read();
                let held: Vec<Key> =
                    if entry.binding.keep_mod_keys { entry.parsed.modifier_keys() } else { Vec::new() };
                let show_mods = self.game_config.as_ref().map(|g| g.show_mods_key()).unwrap_or_else(|| "Alt".into());
                let mods = copy_item_modifiers(&held, &show_mods);
                for m in &mods {
                    self.input.toggle(*m, true);
                }
                self.input.tap(Key::C, &[]);
                for m in mods.iter().rev() {
                    self.input.toggle(*m, false);
                }
                match self.clip.poll_item(&before) {
                    Some(clipboard) => {
                        self.remove_area_listeners();
                        self.emit.send(PlatformEvent::ItemText {
                            target: target.clone(),
                            clipboard,
                            position,
                            focus_overlay: *focus_overlay,
                        });
                        if *focus_overlay && self.used_recently {
                            self.assert_overlay_active();
                        }
                    }
                    None => self.emit.log(Level::Warn, "ClipboardPoller", "No item text found."),
                }
            }
            ShortcutAction::OcrText { .. } => {
                self.emit.log(Level::Warn, "Shortcuts", "OCR (heist gems) is not supported in this version.");
            }
            ShortcutAction::TestOnly => {}
        }
    }

    fn taps(&mut self, taps: &[Tap]) {
        for (k, mods) in taps {
            self.input.tap(*k, mods);
        }
    }

    /// `text-box.ts typeInChat`
    fn type_in_chat(&mut self, text: &str, send: bool) {
        if !self.clip.begin_restore_shortly() {
            return;
        }
        // From an overlay button (e.g. Whisper on a trade listing) the overlay
        // has the keyboard: hand it back to the game first, or the chat
        // keystrokes land in the overlay. No-op when the game is already active.
        self.assert_game_active();
        let (clip, taps) = type_in_chat_plan(text, send);
        self.clip.write(&clip);
        self.taps(&taps);
    }

    /// `text-box.ts stashSearch`
    fn stash_search(&mut self, text: &str) {
        if !self.clip.begin_restore_shortly() {
            return;
        }
        self.assert_game_active();
        self.clip.write(text);
        self.taps(&shortcuts::stash_search_taps());
    }

    // ----- global input (uiohook listeners) -----

    fn on_input(&mut self, ev: InputEvent) {
        match ev {
            InputEvent::KeyDown { key, mods } => {
                if self.log_keys() {
                    self.emit.log(Level::Debug, "Shortcuts", &format!("Keydown {}", event_to_string(key, mods)));
                }
                // OverlayVisibility
                if mods.alt && !mods.shift && !mods.ctrl && key == Some(Key::ALT) {
                    self.make_invisible();
                } else {
                    self.make_visible();
                }
            }
            InputEvent::KeyUp { key, mods } => {
                if self.log_keys() {
                    let name = key.map(|k| k.name()).unwrap_or("not_supported_key");
                    self.emit.log(Level::Debug, "Shortcuts", &format!("Keyup {name}"));
                }
                if !mods.alt {
                    self.make_visible();
                }
            }
            InputEvent::MouseMove { x, y, mods } => {
                if !mods.alt {
                    self.make_visible();
                }
                self.area_mouse_move(x, y, mods.ctrl, mods.alt);
            }
            InputEvent::MouseDown { x, y } => {
                if let Some(area) = &self.area
                    && area.area.contains_strict(x, y) {
                        self.remove_area_listeners();
                        self.assert_overlay_active();
                    }
            }
            InputEvent::Wheel { rotation, x, y, mods } => {
                let stash_scroll = self.config.as_ref().is_some_and(|c| c.stash_scroll);
                if !mods.ctrl || !self.game_active || !stash_scroll {
                    return;
                }
                if !is_stash_area(x, y, self.bounds) {
                    if rotation > 0 {
                        self.input.tap(Key::ARROW_RIGHT, &[]);
                    } else if rotation < 0 {
                        self.input.tap(Key::ARROW_LEFT, &[]);
                    }
                }
            }
        }
    }

    fn make_visible(&mut self) {
        if self.visible && self.vis_timer.is_none() {
            return;
        }
        if self.vis_timer.take().is_none() {
            self.visible = true;
            self.emit.send(PlatformEvent::Visibility { visible: true });
        }
        self.update_listener_flags();
    }

    fn make_invisible(&mut self) {
        if !self.visible || self.vis_timer.is_some() {
            return;
        }
        let delay = if self.interactable { 85 } else { 275 };
        self.vis_timer = Some(Instant::now() + Duration::from_millis(delay));
        self.update_listener_flags();
    }

    // ----- WidgetAreaTracker -----

    fn remove_area_listeners(&mut self) {
        if self.area.take().is_some() {
            self.update_listener_flags();
        }
    }

    fn area_mouse_move(&mut self, x: f64, y: f64, ctrl: bool, alt: bool) {
        let Some(area) = self.area.clone() else { return };
        let modifier = if ctrl {
            Some("Ctrl")
        } else if alt {
            Some("Alt")
        } else {
            None
        };
        if !self.interactable && modifier != Some(area.hold_key.as_str()) {
            let distance = (x - area.from.0).hypot(y - area.from.1);
            if distance > area.close_threshold {
                self.emit.send(PlatformEvent::HideExclusiveWidget);
                self.remove_area_listeners();
            }
        } else if area.area.contains_strict(x, y) {
            self.assert_overlay_active();
        } else if self.interactable {
            tracing::info!("back to game: pointer ({x:.0}, {y:.0}) left the tracked area {:?}", area.area);
            self.remove_area_listeners();
            self.assert_game_active();
        }
    }

    fn update_listener_flags(&self) {
        if let Some(l) = &self.sys.listener {
            l.want_mouse_move(self.area.is_some() || self.vis_timer.is_some() || !self.visible);
        }
    }
}
