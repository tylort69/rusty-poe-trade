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
        }
    }
}

/// Runtime (non-persisted) state.
#[derive(Default)]
pub struct WhispersState {
    pub book: WhisperBook,
    got_lines: bool,
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

fn own(cfg: &crate::config::Config) -> WhispersWidget {
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
            full(ui, w, st, now);
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

fn full(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut WhispersState, now: LogSecs) {
    let th = theme(ui.ctx());
    let c = th.colors;
    ui.set_width(380.0);
    ui.horizontal(|ui| {
        ui.label(rpt_m3::icon_text(icons::FORUM, 20.0).color(c.primary));
        ui.label(RichText::new("Trade whispers").font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
        let incoming = st.book.list.iter().filter(|x| x.dir == Dir::Incoming && !x.completed).count();
        if incoming > 0 {
            rpt_m3::tonal_badge(ui, &incoming.to_string(), c.primary_container, c.on_primary_container);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let gear = ui.add(IconButton::new(icons::SETTINGS).size(32.0).tooltip("Whisper settings"));
            rpt_m3::popover(&gear, |ui| settings_popover(ui, w));
            let any_done = st.book.list.iter().any(|x| x.completed);
            if ui.add(IconButton::new(ICON_DONE_ALL).size(32.0).enabled(any_done).tooltip("Dismiss completed")).clicked() {
                st.book.dismiss_completed();
            }
        });
    });
    super::mapping::client_log_hint(ui, w, st.got_lines);

    if st.book.list.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new("No trade whispers yet.").font(th.font(TypeRole::BodyMedium)).color(c.on_surface_variant));
        return;
    }
    let cfg = own(w.cfg);
    let max_h = (w.screen.height() * 0.6).max(200.0);
    let mut actions: Vec<(u64, Action)> = Vec::new();
    egui::ScrollArea::vertical().id_salt("whisper-cards").max_height(max_h).show(ui, |ui| {
        for card in &st.book.list {
            ui.add_space(4.0);
            whisper_card(ui, card, &cfg, now, &mut actions);
        }
    });
    for (id, a) in actions {
        run_action(w, st, id, a);
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
