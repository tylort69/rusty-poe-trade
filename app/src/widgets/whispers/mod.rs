//! Trade whisper manager fed by Client.txt.
//!
//! Incoming (`@From`) and outgoing (`@To`) trade-site whispers become cards
//! with one-click chat actions (invite, trade, thanks, kick, …), each sent as
//! a `PasteInChat` host command. Area join/leave and "Trade accepted." lines
//! update the cards' status.
//!
//! While the game has focus the widget shrinks to a non-interactive
//! "N new trade whispers" pill. For that it drops its `invisible-on-blur` wm
//! flag while there are unseen incoming whispers (and restores it once they
//! were seen), like the stopwatch does while running.

pub mod model;

use std::time::Duration;

use egui::{Color32, CornerRadius, RichText, Sense, Ui, vec2};
use rpt_m3::{ButtonKind, CardKind, IconButton, TypeRole, icons, theme};
use serde::{Deserialize, Serialize};

use self::model::{Dir, Effect, Whisper, WhisperBook};
use super::mapping::parse::{LogSecs, now_secs};
use super::{WCtx, chrome};
use crate::config::{Anchor, WidgetKind};
use crate::host::HostCommand;

// Material Icons Round codepoints not in rpt_m3::icons.
const ICON_PERSON_ADD: &str = "\u{e7fe}";
const ICON_PERSON_REMOVE: &str = "\u{ef66}";
const ICON_HOME: &str = "\u{e88a}";
const ICON_THUMB_UP: &str = "\u{e8dc}";
const ICON_INCOMING: &str = "\u{e0b5}"; // call_received
const ICON_OUTGOING: &str = "\u{e0b2}"; // call_made
const ICON_HOURGLASS: &str = "\u{e88b}";
const ICON_QUESTION: &str = "\u{eb8b}";
const ICON_UNREAD: &str = "\u{f189}"; // mark_chat_unread
const ICON_DONE_ALL: &str = "\u{e877}";
const ICON_GROUP_REMOVE: &str = "\u{e7ad}";
const ICON_DOCK: &str = "\u{f1cf}"; // close_fullscreen
const ICON_PUSH_PIN: &str = "\u{f10d}";

/// Title of the popped-out window. The platform's Hyprland rule floats
/// windows with exactly this title, so keep them in sync.
pub const POPOUT_TITLE: &str = "Trade whispers";

/// Persisted settings for this widget (stored in config.json, `wmType: "rpt-whispers"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WhispersWidget {
    pub anchor: Anchor,
    /// Your character name (for "Leave party" = `/kick <you>`). Empty → hidden.
    pub character_name: String,
    /// Templates. `{name}`, `{item}`, `{price}` and `{league}` are substituted.
    pub thanks_text: String,
    pub sold_text: String,
    pub busy_text: String,
    pub still_interested_text: String,
    /// Kick the buyer from the party once a trade completes.
    pub auto_kick: bool,
    /// Show the "N new trade whispers" pill while playing.
    pub notify_in_game: bool,
    /// Shown in its own window (any monitor) instead of inside the overlay.
    pub pop_out: bool,
    /// Last position / size of the popped-out window (screen pixels).
    pub popout_pos: Option<[f32; 2]>,
    pub popout_size: Option<[f32; 2]>,
    /// Same, as Hyprland reports it (layout pixels). Preferred on Hyprland,
    /// where XWayland's X11 coordinates don't match the monitor layout.
    pub popout_hypr_pos: Option<[i32; 2]>,
    /// Keep the popped-out window above other windows.
    pub popout_on_top: bool,
    /// Global hotkeys for the quick actions, by [`QuickAction::key`].
    pub quick_keys: std::collections::BTreeMap<String, String>,
}

impl Default for WhispersWidget {
    fn default() -> Self {
        Self {
            anchor: Anchor { pos: "tr".into(), x: 98.0, y: 20.0 },
            character_name: String::new(),
            thanks_text: "ty, gl".into(),
            sold_text: "sorry, {item} is already sold".into(),
            busy_text: "busy right now, will invite you in a few minutes".into(),
            still_interested_text: "still interested in {item} for {price}?".into(),
            auto_kick: false,
            notify_in_game: true,
            pop_out: false,
            popout_pos: None,
            popout_size: None,
            popout_hypr_pos: None,
            popout_on_top: true,
            quick_keys: Default::default(),
        }
    }
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct WhispersState {
    pub book: WhisperBook,
    got_lines: bool,
    /// Where the popped-out window was created this time (kept constant so
    /// egui doesn't fight the user while they drag it).
    popout_spawn: Option<(Option<[f32; 2]>, [f32; 2])>,
    /// Window moved/resized at this time; read its Hyprland position once it settles.
    popout_moved: Option<std::time::Instant>,
}

/// Hyprland manages the pop-out's position (see `popout_hypr_pos`).
fn hypr_positions() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() && std::env::var_os("RPT_NO_HYPR_RULES").is_none()
}

/// The quick-action bar at the top, each with an optional global hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickAction {
    Invite,
    Trade,
    Thanks,
    Kick,
}

impl QuickAction {
    pub const ALL: [QuickAction; 4] = [Self::Invite, Self::Trade, Self::Thanks, Self::Kick];

    /// Config / hotkey event key.
    pub fn key(self) -> &'static str {
        match self {
            Self::Invite => "invite",
            Self::Trade => "trade",
            Self::Thanks => "thanks",
            Self::Kick => "kick",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == k)
    }

    /// What it does, for buttons and the hotkey settings.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Invite => "Invite the newest buyer who whispered you",
            Self::Trade => "Trade with the buyer in your hideout",
            Self::Thanks => "Thank the player you last traded with",
            Self::Kick => "Kick the player you last traded with from your party",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Invite => "Invite",
            Self::Trade => "Trade",
            Self::Thanks => "Thanks",
            Self::Kick => "Kick",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Invite => ICON_PERSON_ADD,
            Self::Trade => icons::SWAP_HORIZ,
            Self::Thanks => ICON_THUMB_UP,
            Self::Kick => ICON_PERSON_REMOVE,
        }
    }

    fn action(self) -> Action {
        match self {
            Self::Invite => Action::Invite,
            Self::Trade => Action::Trade,
            Self::Thanks => Action::Thanks,
            Self::Kick => Action::Kick,
        }
    }

    /// The card this would act on right now.
    pub fn target(self, book: &WhisperBook) -> Option<&Whisper> {
        match self {
            Self::Invite => book.invite_target(),
            Self::Trade => book.trade_target(),
            Self::Thanks | Self::Kick => book.party_target(),
        }
    }
}

/// Run a quick action (button or hotkey). Does nothing without a target.
pub fn quick(w: &mut WCtx<'_>, st: &mut WhispersState, qa: QuickAction) {
    match qa.target(&st.book).map(|x| x.id) {
        Some(id) => run_action(w, st, id, qa.action()),
        None => tracing::info!("whispers: no player for quick action {:?}", qa),
    }
}

/// One-click actions on a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Invite,
    Trade,
    Thanks,
    Sold,
    Busy,
    StillInterested,
    Kick,
    Hideout,
    WhisperAgain,
    LeaveParty,
    Dismiss,
}

fn expand(template: &str, w: &Whisper) -> String {
    template
        .replace("{name}", &w.player)
        .replace("{item}", &w.item_label())
        .replace("{price}", &w.price_label())
        .replace("{league}", &w.league)
}

/// Chat text for an action (None = no chat command, or not available).
pub fn command_for(a: Action, w: &Whisper, cfg: &WhispersWidget) -> Option<String> {
    let at = |t: &str| Some(format!("@{} {}", w.player, expand(t, w)));
    match a {
        Action::Invite => Some(format!("/invite {}", w.player)),
        Action::Trade => Some(format!("/tradewith {}", w.player)),
        Action::Thanks => at(&cfg.thanks_text),
        Action::Sold => at(&cfg.sold_text),
        Action::Busy => at(&cfg.busy_text),
        Action::StillInterested => at(&cfg.still_interested_text),
        Action::Kick => Some(format!("/kick {}", w.player)),
        Action::Hideout => Some(format!("/hideout {}", w.player)),
        Action::WhisperAgain => Some(format!("@{} {}", w.player, w.raw)),
        Action::LeaveParty => {
            let me = cfg.character_name.trim();
            (!me.is_empty()).then(|| format!("/kick {me}"))
        }
        Action::Dismiss => None,
    }
}

pub fn own(cfg: &crate::config::Config) -> WhispersWidget {
    cfg.widgets
        .iter()
        .find_map(|w| match &w.kind {
            WidgetKind::Whispers(x) => Some(x.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn own_mut(cfg: &mut crate::config::Config) -> Option<&mut WhispersWidget> {
    cfg.widgets.iter_mut().find_map(|w| match &mut w.kind {
        WidgetKind::Whispers(x) => Some(x),
        _ => None,
    })
}

/// Apply an action: queue the chat command and update the card.
pub fn run_action(w: &mut WCtx<'_>, st: &mut WhispersState, id: u64, a: Action) {
    let cfg = own(w.cfg);
    let Some(card) = st.book.get(id).cloned() else { return };
    if a == Action::Dismiss {
        st.book.dismiss(id);
        return;
    }
    if let Some(text) = command_for(a, &card, &cfg) {
        w.host.push(HostCommand::PasteInChat { text, send: true });
    }
    match a {
        Action::Invite | Action::Hideout => {
            if let Some(c) = st.book.get_mut(id) {
                c.invited = true;
            }
        }
        Action::Trade => st.book.set_trade_target(&card.player),
        _ => {}
    }
}

/// Client.txt lines (already split, newest last). Called for every batch.
pub fn on_game_log(w: &mut WCtx<'_>, st: &mut WhispersState, lines: &[String]) {
    st.got_lines |= !lines.is_empty();
    let cfg = own(w.cfg);
    let mut new_incoming = false;
    for line in lines {
        for fx in st.book.feed(line, &w.cfg.language) {
            match fx {
                Effect::NewIncoming { .. } => new_incoming = true,
                Effect::Completed { dir: Dir::Incoming, player, .. } if cfg.auto_kick => {
                    w.host.push(HostCommand::PasteInChat { text: format!("/kick {player}"), send: true });
                }
                Effect::Completed { .. } => {}
            }
        }
    }
    if new_incoming && cfg.notify_in_game {
        // Make the pill visible while the game is focused (see module docs).
        if let Some(id) = crate::wm::id_of(w.cfg, "rpt-whispers") {
            let mut changed = crate::wm::set_flag(w.cfg, id, "invisible-on-blur", false);
            if let Some(wd) = crate::wm::find_mut(w.cfg, id) {
                if !wd.wants_show() {
                    // not `wm::show`: that would close an open exclusive widget (price check)
                    wd.wm_wants = "show".into();
                    changed = true;
                }
            }
            w.save_requested |= changed;
        }
    }
}

// ---------------------------------------------------------------------------
// UI

pub fn show(ctx: &egui::Context, w: &mut WCtx<'_>, st: &mut WhispersState, wm_id: u32) {
    let cfg = own(w.cfg);
    if cfg.pop_out {
        // drawn in its own window by `show_popout`
        return;
    }
    let active = w.st.active;
    if active {
        // The user is looking at the list now.
        st.book.mark_all_seen();
    }
    let keep_visible = cfg.notify_in_game && st.book.unseen() > 0;
    if crate::wm::set_flag(w.cfg, wm_id, "invisible-on-blur", !keep_visible) {
        w.save_requested = true;
    }
    if !active && !keep_visible {
        return;
    }
    let now = now_secs();
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: true, surface: true }, |ui, w| {
        if active {
            let max_h = (w.screen.height() * 0.6).max(200.0);
            full(ui, w, st, now, max_h, false);
        } else {
            pill(ui, st);
        }
    });
    chrome::apply(w, wm_id, &r);
    ctx.request_repaint_after(Duration::from_secs(15));
}

/// Compact, non-interactive indicator shown while the game is focused.
fn pill(ui: &mut Ui, st: &WhispersState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let n = st.book.unseen();
    ui.horizontal(|ui| {
        ui.label(rpt_m3::icon_text(ICON_UNREAD, 18.0).color(c.primary));
        let s = if n == 1 { "1 new trade whisper".to_owned() } else { format!("{n} new trade whispers") };
        ui.label(RichText::new(s).font(th.font(TypeRole::LabelLarge)).color(c.on_surface));
    });
    if let Some(latest) = st.book.list.iter().find(|w| !w.seen) {
        ui.label(
            RichText::new(format!("{} · {} · {}", latest.player, latest.item_label(), latest.price_label()))
                .font(th.font(TypeRole::BodySmall))
                .color(c.on_surface_variant),
        );
    }
}

/// Draw the popped-out whispers window (own OS window, any monitor). Call
/// every frame; does nothing unless the widget is popped out.
pub fn show_popout(ctx: &egui::Context, w: &mut WCtx<'_>, st: &mut WhispersState) {
    let cfg = own(w.cfg);
    if !cfg.pop_out {
        st.popout_spawn = None;
        return;
    }
    let first = st.popout_spawn.is_none();
    let pos = if hypr_positions() { None } else { cfg.popout_pos };
    let spawn = *st.popout_spawn.get_or_insert((pos, cfg.popout_size.unwrap_or([400.0, 640.0])));
    if first {
        st.popout_moved = Some(std::time::Instant::now());
    }
    let mut vb = egui::ViewportBuilder::default()
        .with_title(POPOUT_TITLE)
        .with_app_id("rusty-poe-trade")
        .with_decorations(false)
        .with_inner_size(spawn.1)
        .with_min_inner_size([320.0, 220.0])
        .with_taskbar(true);
    if let Some(p) = spawn.0 {
        vb = vb.with_position(p);
    }
    if cfg.popout_on_top {
        vb = vb.with_always_on_top();
    }
    let now = now_secs();
    let mut dock = false;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("rpt-whispers-popout"), vb, |ui, _class| {
        let vctx = ui.ctx().clone();
        let (close, outer, inner, hovered) = vctx.input(|i| {
            let v = i.viewport();
            (v.close_requested(), v.outer_rect, v.inner_rect, i.pointer.has_pointer())
        });
        if close {
            dock = true;
        }
        if hovered {
            st.book.mark_all_seen();
        }
        if vctx.input(|i| i.pointer.any_pressed()) {
            tracing::debug!("whispers pop-out: click received");
        }
        // remember where the user put it
        if let (Some(o), Some(inn)) = (outer, inner) {
            let pos = [o.min.x, o.min.y];
            let size = [inn.width(), inn.height()];
            if let Some(x) = own_mut(w.cfg) {
                if x.popout_pos != Some(pos) || x.popout_size != Some(size) {
                    x.popout_pos = Some(pos);
                    x.popout_size = Some(size);
                    w.save_requested = true;
                    st.popout_moved = Some(std::time::Instant::now());
                }
            }
        }
        // On Hyprland, remember the position Hyprland reports once the move settles.
        #[cfg(target_os = "linux")]
        if let Some(t) = st.popout_moved {
            if t.elapsed() >= Duration::from_millis(600) {
                st.popout_moved = None;
                if let Some((px, py)) = rpt_platform::hyprland_window_pos(POPOUT_TITLE) {
                    if let Some(x) = own_mut(w.cfg) {
                        if x.popout_hypr_pos != Some([px, py]) {
                            tracing::debug!("whispers pop-out at {px},{py} (Hyprland layout)");
                            x.popout_hypr_pos = Some([px, py]);
                            w.save_requested = true;
                        }
                    }
                    rpt_platform::set_popout_position(Some((px, py)));
                }
            } else {
                vctx.request_repaint_after(Duration::from_millis(650));
            }
        }
        let c = theme(&vctx).colors;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c.surface_container).inner_margin(egui::Margin::same(10)))
            .show(ui, |ui| {
                let h = ui.available_height();
                if full(ui, w, st, now, h, true) {
                    dock = true;
                }
            });
    });
    if dock {
        if let Some(x) = own_mut(w.cfg) {
            x.pop_out = false;
            w.save_requested = true;
        }
    }
    ctx.request_repaint_after(Duration::from_secs(15));
}

/// The full whispers list. `popped` = drawn in its own window. Returns true
/// when the user asked to dock a popped-out window back into the overlay.
fn full(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut WhispersState, now: LogSecs, max_h: f32, popped: bool) -> bool {
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut dock = false;
    if !popped {
        ui.set_width(380.0);
    }
    let header = ui.horizontal(|ui| {
        ui.label(rpt_m3::icon_text(icons::FORUM, 20.0).color(c.primary));
        ui.label(RichText::new("Trade whispers").font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
        let incoming = st.book.list.iter().filter(|x| x.dir == Dir::Incoming && !x.completed).count();
        if incoming > 0 {
            rpt_m3::tonal_badge(ui, &incoming.to_string(), c.primary_container, c.on_primary_container);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if popped {
                if ui.add(IconButton::new(ICON_DOCK).size(32.0).tooltip("Put back into the game overlay")).clicked() {
                    dock = true;
                }
                if let Some(x) = own_mut(w.cfg) {
                    let tip = if x.popout_on_top { "Stays above other windows (click to stop)" } else { "Keep above other windows" };
                    if ui.add(IconButton::new(ICON_PUSH_PIN).size(32.0).selected(x.popout_on_top).tooltip(tip)).clicked() {
                        x.popout_on_top = !x.popout_on_top;
                        w.save_requested = true;
                        let level = if x.popout_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                    }
                }
            } else if ui.add(IconButton::new(icons::OPEN_IN_NEW).size(32.0).tooltip("Pop out into its own window you can drag to any monitor")).clicked() {
                if let Some(x) = own_mut(w.cfg) {
                    x.pop_out = true;
                    w.save_requested = true;
                }
                if w.st.active {
                    w.host.push(HostCommand::FocusGame);
                }
            }
            let gear = ui.add(IconButton::new(icons::SETTINGS).size(32.0).tooltip("Whisper settings"));
            rpt_m3::popover(&gear, |ui| settings_popover(ui, w));
            let any_done = st.book.list.iter().any(|x| x.completed);
            if ui.add(IconButton::new(ICON_DONE_ALL).size(32.0).enabled(any_done).tooltip("Dismiss completed")).clicked() {
                st.book.dismiss_completed();
            }
        });
    });
    if popped {
        // drag the window by its header (empty space between title and buttons)
        let drag = ui.interact(header.response.rect, ui.scope_id().with("popout-drag"), egui::Sense::drag());
        if drag.on_hover_cursor(egui::CursorIcon::Grab).drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }
    super::mapping::client_log_hint(ui, w, st.got_lines);
    quick_bar(ui, w, st);

    if st.book.list.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new("No trade whispers yet.").font(th.font(TypeRole::BodyMedium)).color(c.on_surface_variant));
        return dock;
    }
    let cfg = own(w.cfg);
    let max_h = (max_h - (ui.min_rect().height())).max(120.0);
    let mut actions: Vec<(u64, Action)> = Vec::new();
    egui::ScrollArea::vertical().id_salt("whisper-cards").max_height(max_h).auto_shrink([false, !popped]).show(ui, |ui| {
        for card in &st.book.list {
            ui.add_space(4.0);
            whisper_card(ui, card, &cfg, now, &mut actions);
        }
    });
    for (id, a) in actions {
        run_action(w, st, id, a);
    }
    dock
}

/// Buttons along the top that act on the most relevant player.
fn quick_bar(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut WhispersState) {
    let th = theme(ui.ctx());
    let cfg = own(w.cfg);
    let mut clicked = None;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for qa in QuickAction::ALL {
            let target = qa.target(&st.book).map(|x| x.player.clone());
            let label = match &target {
                Some(p) => format!("{} {p}", qa.label()),
                None => qa.label().to_owned(),
            };
            let mut tip = qa.describe().to_owned();
            if target.is_none() {
                tip.push_str("\n(nobody right now)");
            }
            if let Some(k) = cfg.quick_keys.get(qa.key()).filter(|k| !k.is_empty()) {
                tip.push_str(&format!("\nHotkey: {k}"));
            }
            let b = rpt_m3::Button::tonal(label).icon(qa.icon()).small().enabled(target.is_some());
            if ui.add(b).on_hover_text(tip).on_disabled_hover_text(qa.describe()).clicked() {
                clicked = Some(qa);
            }
        }
    });
    let _ = th;
    ui.add_space(2.0);
    if let Some(qa) = clicked {
        quick(w, st, qa);
    }
}

fn settings_popover(ui: &mut Ui, w: &mut WCtx<'_>) {
    let th = theme(ui.ctx());
    ui.set_width(300.0);
    let Some(cfg) = own_mut(w.cfg) else { return };
    let mut changed = false;
    let label = |ui: &mut Ui, s: &str| {
        ui.label(RichText::new(s).font(th.font(TypeRole::LabelLarge)).color(th.colors.on_surface_variant));
    };
    label(ui, "Your character name");
    changed |= rpt_m3::text_field(ui, &mut cfg.character_name, "for Leave party and deaths").changed();
    ui.label(RichText::new("Templates: {name} {item} {price} {league}").font(th.font(TypeRole::BodySmall)).color(th.colors.on_surface_variant));
    for (name, value) in [
        ("Thanks", &mut cfg.thanks_text),
        ("Sold", &mut cfg.sold_text),
        ("Busy", &mut cfg.busy_text),
        ("Still interested?", &mut cfg.still_interested_text),
    ] {
        label(ui, name);
        changed |= rpt_m3::text_field(ui, value, name).changed();
    }
    ui.add_space(4.0);
    changed |= rpt_m3::switch_row(ui, "Kick buyer after trade", &mut cfg.auto_kick).changed();
    changed |= rpt_m3::switch_row(ui, "Show new whispers in game", &mut cfg.notify_in_game).changed();
    if changed {
        w.save_requested = true;
    }
}

/// Small non-interactive status chip.
fn status_chip(ui: &mut Ui, icon: &str, label: &str, bg: Color32, fg: Color32) {
    let th = theme(ui.ctx());
    let font = th.font(TypeRole::LabelMedium);
    let g = ui.painter().layout_no_wrap(label.to_owned(), font, fg);
    let size = vec2(g.size().x + 30.0, 22.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(rpt_m3::shape::SMALL), bg);
    p.text(
        egui::pos2(rect.left() + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::new(14.0, egui::FontFamily::Name(rpt_m3::theme::ICONS.into())),
        fg,
    );
    p.galley(egui::pos2(rect.left() + 24.0, rect.center().y - g.size().y / 2.0), g, fg);
}

fn ago(now: LogSecs, t: LogSecs) -> String {
    let d = (now - t).max(0);
    match d {
        0..60 => "now".into(),
        60..3600 => format!("{}m ago", d / 60),
        _ => format!("{}h ago", d / 3600),
    }
}

fn whisper_card(ui: &mut Ui, wh: &Whisper, cfg: &WhispersWidget, now: LogSecs, actions: &mut Vec<(u64, Action)>) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let kind = if wh.completed { CardKind::Outlined } else { CardKind::Filled };
    rpt_m3::card(ui, kind).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // Player row
        ui.horizontal(|ui| {
            let (icon, tint, tip) = match wh.dir {
                Dir::Incoming => (ICON_INCOMING, c.primary, "Incoming: wants to buy from you"),
                Dir::Outgoing => (ICON_OUTGOING, c.tertiary, "Outgoing: you want to buy"),
            };
            ui.label(rpt_m3::icon_text(icon, 16.0).color(tint)).on_hover_text(tip);
            if let Some(g) = &wh.guild {
                ui.label(RichText::new(format!("<{g}>")).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
            }
            ui.label(RichText::new(&wh.player).font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
            if wh.count > 1 {
                rpt_m3::tonal_badge(ui, &format!("×{}", wh.count), c.tertiary_container, c.on_tertiary_container)
                    .on_hover_text("Repeated whispers");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let time = super::mapping::fmt_clock(wh.last);
                let time = time.split(' ').nth(1).unwrap_or(&time).to_owned();
                ui.label(RichText::new(format!("{time} · {}", ago(now, wh.last))).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant))
                    .on_hover_text(format!("first {}", super::mapping::fmt_clock(wh.received)));
            });
        });
        // Item + price; hover shows the original whisper
        ui.horizontal_wrapped(|ui| {
            let item = ui.label(RichText::new(wh.item_label()).font(th.font(TypeRole::BodyMedium)).color(c.on_surface));
            rpt_m3::rich_tooltip(item, Some("Original message"), &wh.raw);
            rpt_m3::tonal_badge(ui, &wh.price_label(), c.secondary_container, c.on_secondary_container);
        });
        // League + stash position
        let mut meta = wh.league.clone();
        if let Some(t) = &wh.tab {
            meta.push_str(&format!(" · tab \"{}\" · left {}, top {}", t.name, t.left, t.top));
        }
        ui.label(RichText::new(meta).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));
        // Status chips
        let mut chips: Vec<(&str, &str, Color32, Color32)> = Vec::new();
        if wh.completed {
            chips.push((icons::DONE, "Traded", c.success_container, c.on_success_container));
        }
        if wh.in_area && !wh.completed {
            let label = if wh.dir == Dir::Incoming { "In your hideout" } else { "In area" };
            chips.push((ICON_HOME, label, c.primary_container, c.on_primary_container));
        }
        if wh.left && !wh.completed {
            chips.push((icons::LOGOUT, "Left", c.surface_container_highest, c.on_surface_variant));
        }
        if wh.invited && !wh.completed {
            let label = if wh.dir == Dir::Incoming { "Invited" } else { "Visited" };
            chips.push((icons::SEND, label, c.secondary_container, c.on_secondary_container));
        }
        if !chips.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (i, l, bg, fg) in chips {
                    status_chip(ui, i, l, bg, fg);
                }
            });
        }
        // Actions
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut btn = |ui: &mut Ui, icon: &str, a: Action, tip: &str| {
                let tip = match command_for(a, wh, cfg) {
                    Some(cmd) => format!("{tip}\n{cmd}"),
                    None => tip.to_owned(),
                };
                if ui.add(IconButton::new(icon).size(30.0).tooltip(tip)).clicked() {
                    actions.push((wh.id, a));
                }
            };
            match wh.dir {
                Dir::Incoming => {
                    if !wh.completed {
                        btn(ui, ICON_PERSON_ADD, Action::Invite, "Invite");
                        btn(ui, icons::SWAP_HORIZ, Action::Trade, "Trade");
                    }
                    btn(ui, ICON_THUMB_UP, Action::Thanks, "Thanks");
                    if !wh.completed {
                        btn(ui, icons::SELL, Action::Sold, "Sold");
                        btn(ui, ICON_HOURGLASS, Action::Busy, "Busy");
                        btn(ui, ICON_QUESTION, Action::StillInterested, "Still interested?");
                    }
                    btn(ui, ICON_PERSON_REMOVE, Action::Kick, "Kick");
                }
                Dir::Outgoing => {
                    btn(ui, ICON_HOME, Action::Hideout, "Visit hideout");
                    btn(ui, icons::SEND, Action::WhisperAgain, "Whisper again");
                    if !cfg.character_name.trim().is_empty() {
                        btn(ui, ICON_GROUP_REMOVE, Action::LeaveParty, "Leave party");
                    }
                }
            }
            if ui.add(rpt_m3::Button::new(ButtonKind::Text, "Dismiss").small()).clicked() {
                actions.push((wh.id, Action::Dismiss));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::HostQueue;

    fn l(hms: &str, msg: &str) -> String {
        format!("2024/01/01 {hms} 123456789 cffb0719 [INFO Client 1234] {msg}")
    }

    fn sample() -> Whisper {
        rpt_data::init("en").unwrap();
        let mut b = WhisperBook::default();
        b.feed(&l("12:00:00", r#"@From Buyer: Hi, I would like to buy your Tabula Rasa listed for 10 chaos in Settlers (stash tab "T"; position: left 3, top 7)"#), "en");
        b.list.remove(0)
    }

    #[test]
    fn commands() {
        let w = sample();
        let mut cfg = WhispersWidget::default();
        assert_eq!(command_for(Action::Invite, &w, &cfg).unwrap(), "/invite Buyer");
        assert_eq!(command_for(Action::Trade, &w, &cfg).unwrap(), "/tradewith Buyer");
        assert_eq!(command_for(Action::Thanks, &w, &cfg).unwrap(), "@Buyer ty, gl");
        assert_eq!(command_for(Action::Sold, &w, &cfg).unwrap(), "@Buyer sorry, Tabula Rasa is already sold");
        assert_eq!(command_for(Action::StillInterested, &w, &cfg).unwrap(), "@Buyer still interested in Tabula Rasa for 10 chaos?");
        assert_eq!(command_for(Action::Kick, &w, &cfg).unwrap(), "/kick Buyer");
        assert_eq!(command_for(Action::Hideout, &w, &cfg).unwrap(), "/hideout Buyer");
        assert!(command_for(Action::WhisperAgain, &w, &cfg).unwrap().starts_with("@Buyer Hi, I would like to buy your Tabula Rasa"));
        assert_eq!(command_for(Action::LeaveParty, &w, &cfg), None);
        cfg.character_name = "Me".into();
        assert_eq!(command_for(Action::LeaveParty, &w, &cfg).unwrap(), "/kick Me");
        assert_eq!(command_for(Action::Dismiss, &w, &cfg), None);
    }

    #[test]
    fn quick_action_targets_and_hotkeys() {
        rpt_data::init("en").unwrap();
        let mut b = WhisperBook::default();
        let buy = |who: &str, item: &str| format!(r#"@From {who}: Hi, I would like to buy your {item} listed for 10 chaos in Settlers (stash tab "T"; position: left 1, top 1)"#);
        b.feed(&l("12:00:00", &buy("Old", "Tabula Rasa")), "en");
        b.feed(&l("12:00:10", &buy("New", "Goldrim")), "en");
        assert_eq!(QuickAction::Invite.target(&b).unwrap().player, "New");
        assert!(QuickAction::Trade.target(&b).is_none(), "nobody in the hideout yet");
        b.feed(&l("12:00:20", ": Old has joined the area."), "en");
        assert_eq!(QuickAction::Trade.target(&b).unwrap().player, "Old");
        b.feed(&l("12:00:30", ": Trade accepted."), "en");
        assert_eq!(QuickAction::Thanks.target(&b).unwrap().player, "Old");
        assert_eq!(QuickAction::Kick.target(&b).unwrap().player, "Old");
        assert_eq!(QuickAction::Invite.target(&b).unwrap().player, "New");

        let mut cfg = crate::config::Config::default();
        own_mut(&mut cfg).unwrap().quick_keys.insert("invite".into(), "Alt + I".into());
        let sc = cfg.shortcuts();
        let s = sc.iter().find(|x| x.shortcut == "Alt + I").expect("hotkey registered");
        assert!(matches!(&s.action, crate::config::ShortcutKind::TriggerEvent { target } if target == "whisper-quick:invite"));
        assert!(!s.keep_mod_keys);
        assert_eq!(QuickAction::from_key("kick"), Some(QuickAction::Kick));
    }

    /// on_game_log → flags/host commands, then headless renders (active and in-game pill).
    #[test]
    fn renders_headless_and_auto_kicks() {
        rpt_data::init("en").unwrap();
        let ctx = egui::Context::default();
        rpt_m3::install_fonts(&ctx, vec![]);
        rpt_m3::install(&ctx, rpt_m3::M3Theme::resolve(rpt_m3::ThemeSettings::default()));
        let data = rpt_data::data();
        let i18n = crate::i18n::I18n::new(&data);
        let mut cfg = crate::config::Config::default();
        own_mut(&mut cfg).unwrap().auto_kick = true;
        let services = crate::services::Services::new(&cfg, ctx.clone());
        let mut host = HostQueue::default();
        let mut st = WhispersState::default();
        let id = crate::wm::id_of(&cfg, "rpt-whispers").unwrap();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0));
        let lines = vec![
            l("12:00:00", r#"@From <GLD> Buyer: Hi, I would like to buy your level 21 20% Vaal Grace listed for 5 chaos in Settlers (stash tab "~b/o"; position: left 3, top 7) hello"#),
            l("12:00:02", r#"@To Seller: Hi, I'd like to buy your 10 Divine Orb for my 2000 Chaos Orb in Settlers."#),
            l("12:00:05", ": Buyer has joined the area."),
        ];
        let frame = |cfg: &mut crate::config::Config, host: &mut HostQueue, st: &mut WhispersState, active: bool, feed: &[String]| {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let mut w = WCtx {
                    cfg,
                    st: crate::wm::OverlayState { active, ..Default::default() },
                    i18n: &i18n,
                    host,
                    services: &services,
                    screen,
                    save_requested: false,
                };
                if !feed.is_empty() {
                    on_game_log(&mut w, st, feed);
                }
                let visible = crate::wm::is_visible(w.cfg, w.st, w.cfg.widgets.iter().find(|x| x.wm_id == id).unwrap());
                if visible {
                    show(ui.ctx(), &mut w, st, id);
                }
            });
            out.textures_delta.clear();
        };
        // game focused: the new whisper makes the widget visible as a pill
        frame(&mut cfg, &mut host, &mut st, false, &lines);
        let wd = cfg.widgets.iter().find(|x| x.wm_id == id).unwrap();
        assert!(wd.wants_show() && !wd.has_flag("invisible-on-blur"));
        assert_eq!(st.book.unseen(), 1);
        assert!(st.book.list.iter().any(|x| x.player == "Buyer" && x.in_area));

        // overlay active: full list, whispers become seen, flag restored
        frame(&mut cfg, &mut host, &mut st, true, &[]);
        frame(&mut cfg, &mut host, &mut st, true, &[]);
        assert_eq!(st.book.unseen(), 0);
        assert!(cfg.widgets.iter().find(|x| x.wm_id == id).unwrap().has_flag("invisible-on-blur"));

        // actions
        let buyer = st.book.list.iter().find(|x| x.player == "Buyer").unwrap().id;
        {
            let mut w = WCtx { cfg: &mut cfg, st: Default::default(), i18n: &i18n, host: &mut host, services: &services, screen, save_requested: false };
            run_action(&mut w, &mut st, buyer, Action::Invite);
            run_action(&mut w, &mut st, buyer, Action::Trade);
        }
        assert!(host.commands.contains(&HostCommand::PasteInChat { text: "/invite Buyer".into(), send: true }));
        assert!(host.commands.contains(&HostCommand::PasteInChat { text: "/tradewith Buyer".into(), send: true }));

        // trade completes → auto kick
        frame(&mut cfg, &mut host, &mut st, true, &[l("12:01:00", ": Trade accepted.")]);
        assert!(st.book.get(buyer).unwrap().completed);
        assert!(host.commands.contains(&HostCommand::PasteInChat { text: "/kick Buyer".into(), send: true }));
    }
}
