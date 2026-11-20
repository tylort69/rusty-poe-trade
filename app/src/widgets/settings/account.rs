//! Settings → Account: sign in with the session cookie today, OAuth later.

use egui::{RichText, Ui};
use rpt_m3::{Button, CardKind, Chip, TypeRole, icons, theme};
use rpt_stash::AuthMethod;

use super::SettingsState;
use crate::account::{self, Status};
use crate::host::HostCommand;
use crate::widgets::WCtx;

#[derive(Default)]
pub struct AccountPageState {
    /// Typed cookie; cleared as soon as it's handed to the account service.
    sid_input: String,
    sid_invalid: bool,
}

pub fn page(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let acct = account::get(ui.ctx());
    acct.ensure_restored(w.cfg);
    let state = acct.state();
    let ps = &mut st.account;

    // Pick up the authoritative account name after sign-in.
    if let Status::SignedIn(info) = &state.status {
        if w.cfg.account_name != info.name {
            w.cfg.account_name = info.name.clone();
            w.save_requested = true;
        }
    }

    // ---- status card ------------------------------------------------------
    rpt_m3::card(ui, CardKind::Filled).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (icon, color, headline, support) = match &state.status {
                Status::SignedOut => (icons::ACCOUNT_CIRCLE, c.on_surface_variant, "Not signed in".to_owned(), None),
                Status::Working(msg) => (icons::ACCOUNT_CIRCLE, c.primary, msg.clone(), None),
                Status::SignedIn(i) => (icons::CHECK_CIRCLE, c.primary, format!("Signed in as {}", i.name), Some(i.method.label().to_owned())),
                Status::Failed(e) => (icons::ERROR, c.error, "Sign-in failed".to_owned(), Some(e.clone())),
            };
            ui.label(rpt_m3::icon_text(icon, 32.0).color(color));
            ui.vertical(|ui| {
                ui.label(RichText::new(headline).font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
                if let Some(s) = support {
                    ui.add(egui::Label::new(RichText::new(s).color(c.on_surface_variant)).wrap());
                }
            });
            if matches!(state.status, Status::Working(_)) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| rpt_m3::circular_progress(ui, 24.0));
            }
            if matches!(state.status, Status::SignedIn(_) | Status::Failed(_)) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::outlined("Sign out").icon(icons::LOGOUT)).clicked() {
                        acct.sign_out();
                    }
                });
            }
        });
    });
    if let Some(e) = &state.keychain_error {
        rpt_m3::banner(
            ui,
            icons::WARNING,
            format!("Couldn't use the system keychain ({e}). You're signed in for this session only."),
            true,
        );
    }

    // ---- method -----------------------------------------------------------
    rpt_m3::section_header(ui, "Sign-in method");
    let mut method = w.cfg.rpt.account.method;
    let opts = [(AuthMethod::SessionCookie, "Session cookie"), (AuthMethod::OAuth, "Official login (OAuth)")];
    if rpt_m3::segmented(ui, &mut method, &opts) {
        w.cfg.rpt.account.method = method;
        w.save_requested = true;
    }
    ui.add_space(8.0);

    let busy = matches!(state.status, Status::Working(_));
    match w.cfg.rpt.account.method {
        AuthMethod::SessionCookie => {
            rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("How to get your POESESSID").font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
                for step in [
                    "1. Sign in at pathofexile.com in your web browser.",
                    "2. Press F12 → Storage (Firefox) or Application (Chrome) → Cookies → https://www.pathofexile.com.",
                    "3. Copy the value of POESESSID and paste it below.",
                ] {
                    ui.add(egui::Label::new(RichText::new(step).color(c.on_surface_variant)).wrap());
                }
                if ui.add(Chip::assist("Open pathofexile.com").leading(icons::OPEN_IN_NEW)).clicked() {
                    w.host.push(HostCommand::OpenUrl("https://www.pathofexile.com/login".into()));
                }
            });
            rpt_m3::banner(
                ui,
                icons::LOCK,
                "Treat POESESSID like a password: anyone who has it can use your account. It's kept only in your system keychain and only sent to pathofexile.com. Signing out on the website invalidates it.",
                false,
            );
            ui.add_space(8.0);
            ui.label(RichText::new("POESESSID").color(c.on_surface_variant));
            let r = rpt_m3::password_field(ui, &mut ps.sid_input, "32 characters");
            if r.changed() {
                ps.sid_invalid = false;
            }
            if ps.sid_invalid {
                ui.label(RichText::new("That doesn't look like a POESESSID value.").color(c.error));
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Account name (optional, detected automatically)").color(c.on_surface_variant));
            if rpt_m3::text_field(ui, &mut w.cfg.account_name, "Name#1234").changed() {
                w.save_requested = true;
            }
            ui.add_space(4.0);
            if rpt_m3::switch_row(ui, "Remember on this computer (system keychain)", &mut w.cfg.rpt.account.remember).changed() {
                w.save_requested = true;
            }
            let submit = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let can = !busy && !ps.sid_input.trim().is_empty();
            if (ui.add(Button::filled("Sign in").icon(icons::LOCK_OPEN).enabled(can)).clicked() || (submit && can))
                && !busy
            {
                match rpt_stash::auth::normalize_poesessid(&ps.sid_input) {
                    Some(sid) => {
                        acct.sign_in_session(sid, &w.cfg.rpt.account, &w.cfg.account_name);
                        ps.sid_input.clear();
                    }
                    None => ps.sid_invalid = true,
                }
            }
        }
        AuthMethod::OAuth => {
            rpt_m3::banner(
                ui,
                icons::WARNING_AMBER,
                "GGG isn't accepting new OAuth app registrations right now. Once Rusty POE Trade has a client ID it will ship with it; until then use the session cookie, or paste a client ID GGG issued to you.",
                false,
            );
            ui.add_space(8.0);
            ui.label(RichText::new("OAuth client ID").color(c.on_surface_variant));
            if rpt_m3::text_field(ui, &mut w.cfg.rpt.account.oauth_client_id, "issued by GGG").changed() {
                w.save_requested = true;
            }
            let mut port = w.cfg.rpt.account.oauth_port as f64;
            rpt_m3::setting_row(ui, None, "Redirect port", Some("Must match the redirect URI registered with GGG (127.0.0.1)"), |ui| {
                if rpt_m3::number_field(ui, &mut port, 1024.0..=65535.0, 1.0, 0, "").changed() {
                    w.cfg.rpt.account.oauth_port = port as u16;
                    w.save_requested = true;
                }
            });
            let can = !busy && !w.cfg.rpt.account.oauth_client_id.trim().is_empty();
            if ui.add(Button::filled("Sign in with pathofexile.com").icon(icons::OPEN_IN_NEW).enabled(can)).clicked() && can {
                acct.sign_in_oauth(&w.cfg.rpt.account);
            }
        }
    }

    // ---- try it -----------------------------------------------------------
    if let Status::SignedIn(_) = &state.status {
        rpt_m3::section_header(ui, "Stash");
        let league = w.cfg.league_id.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            let can = !league.is_empty() && !state.loading_tabs;
            if ui.add(Button::tonal(format!("Load stash tabs ({})", if league.is_empty() { "pick a league in General" } else { &league })).icon(icons::INVENTORY_2).enabled(can)).clicked() && can {
                acct.load_tabs(league.clone());
            }
            if state.loading_tabs {
                rpt_m3::circular_progress(ui, 20.0);
            }
        });
        if let Some(e) = &state.tabs_error {
            rpt_m3::banner(ui, icons::ERROR, e.clone(), true);
        }
        if let Some((lg, tabs)) = &state.tabs {
            let flat = rpt_stash::StashTabInfo::flatten(tabs);
            ui.label(RichText::new(format!("{} tabs in {lg}", flat.len())).color(c.on_surface_variant));
            ui.horizontal_wrapped(|ui| {
                for t in flat.iter().filter(|t| !t.hidden) {
                    let mut chip = Chip::assist(t.name.as_str());
                    if let Some([r, g, b]) = t.colour {
                        chip = chip.colors(egui::Color32::from_rgb(r, g, b).gamma_multiply(0.35), c.on_surface);
                    }
                    ui.add(chip);
                }
            });
        }
    }

    ui.add_space(12.0);
    ui.add(egui::Label::new(RichText::new(rpt_stash::GGG_DISCLAIMER).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant)).wrap());
}
