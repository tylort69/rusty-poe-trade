//! The overlay application: one transparent, always-on-top window laid over the
//! game's client area. Port of upstream `OverlayWindow.vue` + the Electron
//! `OverlayWindow.ts` glue (passthrough when inactive, focus handling).

use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use egui::{Color32, Context, Pos2, Rect, Sense, Ui, ViewportCommand};

use crate::config::Config;
use crate::host::{HostCommand, HostEvent, HostQueue};
use crate::i18n::I18n;
use crate::platform_glue::coords;
use crate::services::Services;
use crate::store::ConfigStore;
use crate::widgets::{self, Runtime, WCtx};
use crate::wm::{self, OverlayState};

pub struct LaunchOptions {
    /// Run as a normal decorated window that is always "active" (upstream's
    /// browser-client mode). Useful on Wayland-native setups and for development.
    pub windowed: bool,
}

pub struct App {
    cfg: Config,
    store: ConfigStore,
    rt: Runtime,
    i18n: I18n,
    services: Services,
    host: HostQueue,
    st: OverlayState,
    windowed: bool,
    events: Receiver<HostEvent>,
    platform: crate::platform_glue::PlatformGlue,
    theme_key: rpt_m3::ThemeSettings,
    dirty_since: Option<Instant>,
    passthrough: Option<bool>,
    /// Last game rect (physical px) + ppp bits the window was moved/resized for.
    game_sync: Option<(rpt_platform::Rect, u32)>,
    snackbar: rpt_m3::Snackbar,
    logs: Vec<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, store: ConfigStore, cfg: Config, opts: LaunchOptions) -> Self {
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
        rpt_m3::install_fonts(ctx, crate::fonts::system_fallbacks(&cfg.language));
        rpt_m3::install(ctx, rpt_m3::M3Theme::resolve(cfg.rpt.theme_settings(cfg.font_size)));
        // The config is the source of truth for zoom: egui's built-in Ctrl +/-/0
        // handler would change the zoom behind its back, so we handle those keys
        // ourselves (`keyboard_zoom`) and write the result to the config.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        ctx.set_zoom_factor(cfg.rpt.zoom());
        let data = rpt_data::data();
        let i18n = I18n::new(&data);
        let services = Services::new(&cfg, ctx.clone());

        let (tx, rx) = crossbeam_channel::unbounded();
        let platform = crate::platform_glue::PlatformGlue::start(&cfg, tx, ctx.clone(), store.dir());

        let st = OverlayState { active: opts.windowed, game_focused: false, hide_ui: false };
        Self {
            theme_key: cfg.rpt.theme_settings(cfg.font_size),
            cfg,
            store,
            rt: Runtime::default(),
            i18n,
            services,
            host: HostQueue::default(),
            st,
            windowed: opts.windowed,
            events: rx,
            platform,
            dirty_since: None,
            passthrough: None,
            game_sync: None,
            snackbar: rpt_m3::Snackbar::default(),
            logs: Vec::new(),
        }
    }

    fn handle_events(&mut self, ctx: &Context) {
        while let Ok(ev) = self.events.try_recv() {
            match &ev {
                HostEvent::FocusChange { game, overlay, .. } => {
                    if !self.windowed {
                        self.st.active = *overlay;
                        // upstream: the overlay window exists only over a focused game
                        ctx.send_viewport_cmd(ViewportCommand::Visible(*game || *overlay));
                    }
                    self.st.game_focused = *game;
                    wm::on_focus_change(&mut self.cfg, self.st.active);
                    if self.st.active {
                        if wm_focus_allowed() {
                            ctx.send_viewport_cmd(ViewportCommand::Focus);
                        }
                    } else {
                        self.save_now();
                    }
                }
                HostEvent::Visibility { visible } => self.st.hide_ui = !*visible,
                HostEvent::HideExclusiveWidget => {
                    if let Some(ex) = wm::shown_exclusive(&self.cfg).map(|w| w.wm_id) {
                        wm::hide(&mut self.cfg, ex);
                    }
                }
                HostEvent::OverlayAttached => {
                    if self.cfg.show_attach_notification {
                        let key = self.cfg.overlay_key.clone();
                        self.snackbar.push(ctx, self.i18n.tf("reopen_settings", &[&key]).replace("{0}", &key));
                    }
                }
                HostEvent::LogEntry(m) => {
                    self.logs.push(m.clone());
                    if self.logs.len() > 11 {
                        self.logs.remove(0);
                    }
                }
                HostEvent::Notice(m) => self.snackbar.push(ctx, m.clone()),
                HostEvent::OpenSettings => {
                    if let Some(id) = wm::id_of(&self.cfg, "settings") {
                        wm::show(&mut self.cfg, id);
                    }
                    if !self.windowed {
                        self.host.push(HostCommand::ToggleOverlay);
                    }
                }
                HostEvent::Quit => self.host.push(HostCommand::Quit),
                HostEvent::UpdateAvailable { version, .. } => {
                    self.snackbar.push(ctx, format!("Rusty POE Trade {version} is available"));
                }
                _ => {}
            }
            if let HostEvent::ItemText { focus_overlay: true, .. } = &ev {
                if wm_focus_allowed() {
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
            }
            let screen = self.screen_rect(ctx);
            let mut w = WCtx {
                cfg: &mut self.cfg,
                st: self.st,
                i18n: &self.i18n,
                host: &mut self.host,
                services: &self.services,
                screen,
                save_requested: false,
            };
            widgets::handle_event(&mut w, &mut self.rt, &ev);
            if w.save_requested {
                self.dirty_since.get_or_insert_with(Instant::now);
            }
        }
    }

    /// Keep the overlay window exactly over the game client area. Viewport
    /// commands are in points and egui-winit converts them with the ppp in effect
    /// *after* this pass (zoom changes apply at the end of the pass), so convert
    /// with the target ppp and re-send whenever the rect or the ppp changes.
    fn sync_game_rect(&mut self, ctx: &Context) {
        if self.windowed {
            return;
        }
        let Some(mut px) = self.platform.game_rect_px() else { return };
        // An unmanaged X11 window exactly the size of a monitor is taken for a
        // fullscreen client by Hyprland, which then hides the (fullscreen) game
        // behind it. One pixel short avoids that.
        if !wm_focus_allowed() && px.height > 1 {
            px.height -= 1;
        }
        let ppp = coords::target_ppp(ctx, self.cfg.rpt.zoom());
        let key = (px, ppp.to_bits());
        if self.game_sync != Some(key) {
            self.game_sync = Some(key);
            let r = coords::rect_to_points(px, ppp);
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(r.min));
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(r.size()));
        }
    }

    /// Apply `rpt.overlayZoom` (the source of truth) to egui.
    fn sync_zoom(&mut self, ctx: &Context) {
        let z = self.cfg.rpt.zoom();
        if ctx.zoom_factor() != z {
            ctx.set_zoom_factor(z);
        }
    }

    /// While the overlay has focus the global hotkeys are unregistered (like
    /// upstream), so the overlay key, the settings key and tool keys are
    /// matched here from window key events: overlay key → back to the game,
    /// settings / tool key → toggle that tool.
    fn overlay_hotkeys(&mut self, ctx: &Context) {
        if !self.st.active || self.windowed || ctx.egui_wants_keyboard_input() || self.rt.settings.cap.active.is_some() {
            return;
        }
        use crate::widgets::settings::hotkey::{hotkey_to_string, key_name};
        let pressed: Vec<String> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. } => {
                        key_name(physical_key.unwrap_or(*key)).map(|n| hotkey_to_string(&[n], modifiers.ctrl || modifiers.command, modifiers.shift, modifiers.alt))
                    }
                    _ => None,
                })
                .collect()
        });
        for k in pressed {
            let tool = if self.cfg.rpt.settings_key.as_deref() == Some(k.as_str()) {
                Some("settings".to_owned())
            } else {
                self.cfg.rpt.tool_keys.iter().find(|(_, v)| **v == k).map(|(t, _)| t.clone())
            };
            if let Some(tool) = tool {
                let screen = self.screen_rect(ctx);
                let mut w = WCtx {
                    cfg: &mut self.cfg,
                    st: self.st,
                    i18n: &self.i18n,
                    host: &mut self.host,
                    services: &self.services,
                    screen,
                    save_requested: false,
                };
                widgets::open_tool(&mut w, &tool);
                self.dirty_since.get_or_insert_with(Instant::now);
            } else if k == self.cfg.overlay_key {
                self.host.push(HostCommand::FocusGame);
            }
        }
    }

    /// Ctrl +/-/0 while the overlay has focus: step the configured zoom by 10 %.
    fn keyboard_zoom(&mut self, ctx: &Context) {
        if !self.st.active || ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::gui_zoom::kb_shortcuts as kb;
        let cur = self.cfg.rpt.zoom();
        let new = ctx.input_mut(|i| {
            if i.consume_shortcut(&kb::ZOOM_RESET) {
                Some(1.0)
            } else if i.consume_shortcut(&kb::ZOOM_IN) || i.consume_shortcut(&kb::ZOOM_IN_SECONDARY) {
                Some(step_zoom(cur, 1))
            } else if i.consume_shortcut(&kb::ZOOM_OUT) {
                Some(step_zoom(cur, -1))
            } else {
                None
            }
        });
        if let Some(z) = new.filter(|z| *z != self.cfg.rpt.overlay_zoom) {
            self.cfg.rpt.overlay_zoom = z;
            self.dirty_since.get_or_insert_with(Instant::now);
        }
    }

    /// The real game client area in points (overlay-local). Widgets that track
    /// in-game panels use this; anchored widgets derive their (possibly
    /// centered 16:9) area from it in `chrome::widget_area`.
    fn screen_rect(&self, ctx: &Context) -> Rect {
        ctx.content_rect()
    }

    fn sync_theme(&mut self, ctx: &Context) {
        let key = self.cfg.rpt.theme_settings(self.cfg.font_size);
        if key != self.theme_key {
            rpt_m3::install(ctx, rpt_m3::M3Theme::resolve(key));
            self.theme_key = key;
        }
    }

    fn sync_passthrough(&mut self, ctx: &Context) {
        if self.windowed {
            return;
        }
        let want = !self.st.active;
        if self.passthrough != Some(want) {
            ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(want));
            self.platform.set_overlay_active(self.st.active);
            self.passthrough = Some(want);
        }
    }

    fn flush_host_commands(&mut self, ctx: &Context) {
        for cmd in std::mem::take(&mut self.host.commands) {
            match cmd {
                HostCommand::SaveConfig => {
                    self.dirty_since.get_or_insert_with(Instant::now);
                }
                HostCommand::Quit => {
                    self.save_now();
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                HostCommand::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                HostCommand::PushHostConfig => self.platform.update_config(&self.cfg),
                other => self.platform.command(other),
            }
        }
    }

    fn save_now(&mut self) {
        if let Err(e) = self.store.save(&self.cfg) {
            tracing::error!("failed to save config: {e:#}");
        }
        self.dirty_since = None;
    }

    fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let screen = self.screen_rect(&ctx);
        let c = rpt_m3::colors(&ctx);

        // Background scrim while the overlay is active (upstream overlayBackground);
        // clicking it returns focus to the game.
        if self.st.active {
            let bg = parse_css_rgba(&self.cfg.overlay_background).unwrap_or(rpt_m3::with_alpha(c.scrim, 0.15));
            ui.painter().rect_filled(screen, 0.0, bg);
            let resp = ui.interact(screen, ui.scope_id().with("overlay-bg"), Sense::click());
            if resp.clicked() && !self.windowed && self.cfg.overlay_background_close {
                self.host.push(HostCommand::FocusGame);
            } else if resp.clicked() && self.windowed {
                if let Some(ex) = wm::shown_exclusive(&self.cfg).map(|w| w.wm_id) {
                    wm::hide(&mut self.cfg, ex);
                }
            }
        }

        // Draw in z-order (lowest first) so later Areas stack on top.
        let mut order: Vec<(i64, u32)> = self
            .cfg
            .widgets
            .iter()
            .filter(|w| wm::is_visible(&self.cfg, self.st, w))
            .map(|w| {
                let z = match w.wm_zorder {
                    Some(crate::config::ZOrder::Order(z)) => z,
                    Some(_) => i64::MAX,
                    None => 0,
                };
                (z, w.wm_id)
            })
            .collect();
        order.sort();

        let mut w = WCtx {
            cfg: &mut self.cfg,
            st: self.st,
            i18n: &self.i18n,
            host: &mut self.host,
            services: &self.services,
            screen,
            save_requested: false,
        };
        for (_, id) in order {
            widgets::draw_widget(&ctx, &mut w, &mut self.rt, id);
        }
        if w.save_requested {
            self.dirty_since.get_or_insert_with(Instant::now);
        }

        // Key log (upstream `logKeys`): shown while the overlay is inactive.
        if !self.st.active && self.cfg.log_keys && !self.logs.is_empty() {
            egui::Area::new(egui::Id::unique("rpt-keylog"))
                .fixed_pos(Pos2::new(screen.left() + 8.0, screen.bottom() - 200.0))
                .interactable(false)
                .show(&ctx, |ui| {
                    for l in &self.logs {
                        ui.label(egui::RichText::new(l).monospace().color(Color32::WHITE).background_color(Color32::from_black_alpha(160)));
                    }
                });
        }
        self.snackbar.show(&ctx);
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.windowed { _visuals.panel_fill.to_normalized_gamma_f32() } else { [0.0; 4] }
    }

    fn logic(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        self.platform.set_window(frame);
        // Escape inside the active overlay returns to the game (upstream main process did this).
        if self.st.active && !self.windowed && ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input() {
            self.host.push(HostCommand::FocusGame);
        }
        self.keyboard_zoom(ctx);
        self.overlay_hotkeys(ctx);
        self.handle_events(ctx);
        if self.services.sync_into(&mut self.cfg) { self.dirty_since.get_or_insert_with(Instant::now); }
        self.sync_zoom(ctx);
        self.sync_game_rect(ctx);
        self.sync_theme(ctx);
        self.sync_passthrough(ctx);
        self.flush_host_commands(ctx);
        if let Some(t) = self.dirty_since {
            // upstream saves on blur; we also debounce saves while the overlay stays open
            if t.elapsed() > Duration::from_secs(2) {
                self.save_now();
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
        // Keep polling platform events at a modest rate even when idle.
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
    }
}

/// One 10 % zoom step up (`dir > 0`) or down, snapped to the 10 % grid and clamped.
pub fn step_zoom(cur: f32, dir: i32) -> f32 {
    let steps = (cur * 10.0).round() as i32 + dir.signum();
    crate::config::clamp_zoom(steps as f32 / 10.0)
}

/// Parse `rgba(r, g, b, a)` / `#rrggbb` (upstream overlayBackground is a CSS color).
pub fn parse_css_rgba(s: &str) -> Option<Color32> {
    let s = s.trim();
    if let Some(c) = crate::config::parse_hex(s) {
        return Some(c);
    }
    let inner = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb("))?.strip_suffix(')')?;
    let p: Vec<f32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    match p.as_slice() {
        [r, g, b] => Some(Color32::from_rgb(*r as u8, *g as u8, *b as u8)),
        [r, g, b, a] => Some(Color32::from_rgba_unmultiplied(*r as u8, *g as u8, *b as u8, (a * 255.0) as u8)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::step_zoom;

    #[test]
    fn zoom_steps_snap_and_clamp() {
        assert_eq!(step_zoom(1.0, 1), 1.1);
        assert_eq!(step_zoom(1.0, -1), 0.9);
        assert_eq!(step_zoom(1.04, 1), 1.1);
        assert_eq!(step_zoom(0.5, -1), 0.5);
        assert_eq!(step_zoom(3.0, 1), 3.0);
        let mut z = 1.0;
        for _ in 0..30 {
            z = step_zoom(z, 1);
        }
        assert_eq!(z, 3.0);
    }
}

/// On Linux the overlay is an unmanaged (override-redirect) X11 window and the
/// platform layer gives it input focus directly. Asking the window manager to
/// activate it would make tiling compositors (Hyprland) un-fullscreen the game.
fn wm_focus_allowed() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("RPT_MANAGED_OVERLAY").is_some()
}
