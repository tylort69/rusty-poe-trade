//! The overlay application: one transparent, always-on-top window laid over the
//! game's client area. Port of upstream `OverlayWindow.vue` + the Electron
//! `OverlayWindow.ts` glue (passthrough when inactive, focus handling).

use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use egui::{Color32, Context, Pos2, Rect, Sense, Ui, ViewportCommand};

use crate::config::Config;
use crate::host::{HostCommand, HostEvent, HostQueue};
use crate::i18n::I18n;
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
    theme_key: (crate::config::RptSettings, f32),
    dirty_since: Option<Instant>,
    passthrough: Option<bool>,
    game_rect: Option<Rect>,
    snackbar: rpt_m3::Snackbar,
    logs: Vec<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, store: ConfigStore, cfg: Config, opts: LaunchOptions) -> Self {
        let ctx = &cc.egui_ctx;
        rpt_m3::install_fonts(ctx, crate::fonts::system_fallbacks(&cfg.language));
        rpt_m3::install(ctx, rpt_m3::M3Theme::resolve(cfg.rpt.theme_settings(cfg.font_size)));
        let data = rpt_data::data();
        let i18n = I18n::new(&data);
        let services = Services::new(&cfg, ctx.clone());

        let (tx, rx) = crossbeam_channel::unbounded();
        let platform = crate::platform_glue::PlatformGlue::start(&cfg, tx, ctx.clone(), opts.windowed);

        let st = OverlayState { active: opts.windowed, game_focused: false, hide_ui: false };
        Self {
            theme_key: (cfg.rpt.clone(), cfg.font_size),
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
            game_rect: None,
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
                    }
                    self.st.game_focused = *game;
                    wm::on_focus_change(&mut self.cfg, self.st.active);
                    if self.st.active {
                        ctx.send_viewport_cmd(ViewportCommand::Focus);
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
                HostEvent::UpdateAvailable { version, .. } => {
                    self.snackbar.push(ctx, format!("Rusty POE Trade {version} is available"));
                }
                _ => {}
            }
            if let HostEvent::ItemText { focus_overlay: true, .. } = &ev {
                ctx.send_viewport_cmd(ViewportCommand::Focus);
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
        if let Some(r) = self.platform.game_rect() {
            if self.game_rect != Some(r) && !self.windowed {
                self.game_rect = Some(r);
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(r.min));
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(r.size()));
            }
        }
    }

    fn screen_rect(&self, ctx: &Context) -> Rect {
        ctx.content_rect()
    }

    fn sync_theme(&mut self, ctx: &Context) {
        let key = (self.cfg.rpt.clone(), self.cfg.font_size);
        if key != self.theme_key {
            rpt_m3::install(ctx, rpt_m3::M3Theme::resolve(self.cfg.rpt.theme_settings(self.cfg.font_size)));
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

    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
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
