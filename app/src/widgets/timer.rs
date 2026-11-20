//! Port of upstream `stopwatch/WidgetTimer.vue`.

use std::time::{Duration, Instant};

use egui::{Context, RichText};
use rpt_m3::{IconButton, IconButtonKind, TypeRole, icons, theme};

use super::{WCtx, chrome};

#[derive(Default)]
pub struct TimerState {
    running_since: Option<Instant>,
    accumulated: Duration,
}

impl TimerState {
    pub fn elapsed(&self) -> Duration {
        self.accumulated + self.running_since.map(|s| s.elapsed()).unwrap_or_default()
    }
    pub fn is_running(&self) -> bool {
        self.running_since.is_some()
    }
    pub fn toggle(&mut self) {
        match self.running_since.take() {
            Some(s) => self.accumulated += s.elapsed(),
            None => self.running_since = Some(Instant::now()),
        }
    }
    /// upstream: resetting while running restarts at 1s so the widget stays visible
    pub fn reset(&mut self) {
        if self.is_running() {
            self.running_since = Some(Instant::now());
            self.accumulated = Duration::from_secs(1);
        } else {
            self.accumulated = Duration::ZERO;
        }
    }
}

pub fn show(ctx: &Context, w: &mut WCtx<'_>, st: &mut TimerState, wm_id: u32) {
    // upstream keeps a running (or non-zero) stopwatch visible while the game is focused
    let keep_visible = st.is_running() || st.elapsed() >= Duration::from_secs(1);
    if crate::wm::set_flag(w.cfg, wm_id, "invisible-on-blur", !keep_visible) {
        w.save_requested = true;
    }
    let th = theme(ctx);
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts::default(), |ui, _w| {
        ui.horizontal(|ui| {
            let secs = st.elapsed().as_secs();
            let text = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
            ui.label(RichText::new(text).font(th.font(TypeRole::HeadlineSmall)).color(th.colors.on_surface));
            let icon = if st.is_running() { icons::PAUSE } else { icons::PLAY_ARROW };
            if ui.add(IconButton::new(icon).kind(IconButtonKind::Filled).size(36.0)).clicked() {
                st.toggle();
            }
            if ui.add(IconButton::new(icons::RESTART_ALT).kind(IconButtonKind::Tonal).size(36.0)).clicked() {
                st.reset();
            }
        });
    });
    chrome::apply(w, wm_id, &r);
    if st.is_running() {
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}
