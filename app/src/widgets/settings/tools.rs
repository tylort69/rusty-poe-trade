//! Settings → Tools: the hub. Opens every tool and binds its hotkey.

use egui::{RichText, Ui};
use rpt_m3::{Button, CardKind, TypeRole, icons, theme};

use super::SettingsState;
use super::hotkey::hotkey_row;
use crate::widgets::{TOOLS, WCtx, open_tool};

pub fn page(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let i18n = w.i18n;

    rpt_m3::banner(
        ui,
        icons::INFO,
        format!(
            "In game: {} shows the overlay menu, {} opens this window. Give any tool its own hotkey below. Pressing a tool's hotkey again closes it. Esc returns to the game.",
            w.cfg.overlay_key,
            w.cfg.rpt.settings_key.as_deref().filter(|k| !k.is_empty()).unwrap_or("(no hotkey)"),
        ),
        false,
    );

    rpt_m3::section_header(ui, "Hotkeys");
    let mut overlay = Some(w.cfg.overlay_key.clone());
    if hotkey_row(ui, &mut st.cap, i18n, "Show overlay menu", &mut overlay, true) {
        if let Some(k) = overlay {
            w.cfg.overlay_key = k;
        }
    }
    hotkey_row(ui, &mut st.cap, i18n, "Open Settings / Tools", &mut w.cfg.rpt.settings_key, false);

    rpt_m3::section_header(ui, "Tools");
    let mut open = None;
    for (tool, title, icon, desc) in TOOLS {
        if w.cfg.widget(tool).is_none() {
            continue;
        }
        rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(rpt_m3::icon_text(icon, 28.0).color(c.primary));
                ui.vertical(|ui| {
                    ui.label(RichText::new(*title).font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
                    ui.add(egui::Label::new(RichText::new(*desc).color(c.on_surface_variant)).wrap());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::tonal("Open").icon(icons::OPEN_IN_NEW)).clicked() {
                        open = Some(*tool);
                    }
                });
            });
            let mut key = w.cfg.rpt.tool_keys.get(*tool).cloned();
            if hotkey_row(ui, &mut st.cap, i18n, "Hotkey", &mut key, false) {
                match key.filter(|k| !k.is_empty()) {
                    Some(k) => {
                        w.cfg.rpt.tool_keys.insert((*tool).to_owned(), k);
                    }
                    None => {
                        w.cfg.rpt.tool_keys.remove(*tool);
                    }
                }
            }
        });
        ui.add_space(6.0);
    }
    if let Some(tool) = open {
        open_tool(w, tool);
    }
}
