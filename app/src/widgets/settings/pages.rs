//! App-wide settings pages: hotkeys, chat, general, appearance, widgets, debug, about.

use egui::{Color32, CornerRadius, RichText, Sense, Ui, vec2};
use rpt_m3::{Button, CardKind, Chip, IconButton, SchemeVariant, TypeRole, icons, shape, theme};

use super::hotkey::{hotkey_input, hotkey_row, split_hotkey_row};
use super::{Page, SettingsState, field_label, opt_text_field, switch_setting};
use crate::config::{Command, WidgetKind};
use crate::host::HostCommand;
use crate::widgets::WCtx;

// ---------------------------------------------------------------------------
// Hotkeys (upstream settings/hotkeys.vue)

pub fn hotkeys(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let i18n = w.i18n;
    rpt_m3::banner(ui, icons::INFO, i18n.t("settings.clear_hotkey"), false);

    rpt_m3::section_header(ui, i18n.t("price_check.name"));
    for wd in w.cfg.widgets.iter_mut() {
        if let WidgetKind::PriceCheck(pc) = &mut wd.kind {
            split_hotkey_row(ui, &mut st.cap, i18n, &i18n.t("price_check.hotkey"), &mut pc.hotkey_hold, &mut pc.hotkey);
            hotkey_row(ui, &mut st.cap, i18n, &i18n.t("price_check.hotkey_locked"), &mut pc.hotkey_locked, false);
        }
    }
    rpt_m3::divider(ui);

    let mut overlay = Some(w.cfg.overlay_key.clone());
    if hotkey_row(ui, &mut st.cap, i18n, &i18n.t("settings.overlay"), &mut overlay, true) {
        if let Some(k) = overlay {
            w.cfg.overlay_key = k;
        }
    }
    for wd in w.cfg.widgets.iter_mut() {
        match &mut wd.kind {
            WidgetKind::ItemCheck(ic) => {
                // Upstream binds both rows to the same item-check hotkey.
                hotkey_row(ui, &mut st.cap, i18n, &i18n.t("map_check.name"), &mut ic.hotkey, false);
                hotkey_row(ui, &mut st.cap, i18n, &i18n.t("item.info"), &mut ic.hotkey, false);
            }
            WidgetKind::DelveGrid(dg) => {
                hotkey_row(ui, &mut st.cap, i18n, &i18n.t("settings.delve_grid"), &mut dg.toggle_key, false);
            }
            _ => {}
        }
    }
    rpt_m3::divider(ui);
    rpt_m3::setting_row(ui, None, &i18n.t("settings.stash_scroll"), None, |ui| {
        let disabled = i18n.t("Disabled");
        rpt_m3::segmented(ui, &mut w.cfg.stash_scroll, &[(false, disabled.as_str()), (true, "Ctrl + MouseWheel")]);
    });
}

// ---------------------------------------------------------------------------
// Chat commands (upstream settings/chat.vue)

pub fn chat(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let i18n = w.i18n;
    let mut remove = None;
    for (idx, cmd) in w.cfg.commands.iter_mut().enumerate() {
        rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if rpt_m3::text_field(ui, &mut cmd.text, "/hideout").changed() {
                cmd.text = cmd.text.trim_start().to_owned();
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                rpt_m3::switch(ui, &mut cmd.send);
                ui.label(i18n.t("settings.chat_cmd_send"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    hotkey_input(ui, &mut st.cap, i18n, ("chat", idx), &mut cmd.hotkey, false, false, 200.0);
                    if ui.add(Button::text(i18n.t("Remove")).icon(icons::DELETE).small()).clicked() {
                        remove = Some(idx);
                    }
                });
            });
        });
        ui.add_space(8.0);
    }
    if let Some(i) = remove {
        w.cfg.commands.remove(i);
        st.cap.active = None;
    }
    if ui.add(Button::tonal(i18n.t("settings.chat_cmd_add")).icon(icons::ADD)).clicked() {
        w.cfg.commands.push(Command { text: String::new(), hotkey: None, send: true });
    }
}

// ---------------------------------------------------------------------------
// General (upstream settings/general.vue)

const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("ru", "Русский"), ("cmn-Hant", "正體中文 · Traditional Chinese"), ("ko", "한국어 · Korean")];
const TRANSPARENT_BG: &str = "rgba(255, 255, 255, 0)";

pub fn general(ui: &mut Ui, w: &mut WCtx<'_>, _st: &mut SettingsState) {
    let i18n = w.i18n;
    let cfg = &mut *w.cfg;

    rpt_m3::section_header(ui, i18n.t("settings.language"));
    let loaded = rpt_data::data().language.clone();
    if cfg.language != loaded {
        rpt_m3::banner(ui, icons::RESTART_ALT, format!("{} — {}", i18n.t("Restart required"), i18n.t("settings.language")), false);
    }
    let mut lang = cfg.language.clone();
    ui.horizontal_wrapped(|ui| {
        for (code, name) in LANGUAGES {
            rpt_m3::radio(ui, &mut lang, (*code).to_owned(), *name);
        }
    });
    if lang != cfg.language {
        // upstream language setter
        cfg.language = lang;
        if cfg.language != "cmn-Hant" {
            cfg.realm = "pc-ggg".into();
        }
        cfg.use_intl_site = cfg.realm == "pc-ggg" && cfg.language == "cmn-Hant";
    }
    if cfg.language == "cmn-Hant" {
        rpt_m3::setting_row(ui, None, &i18n.t("realm"), None, |ui| {
            let (intl, hotcool) = (i18n.t("realm_intl"), i18n.t("Hotcool"));
            if rpt_m3::segmented(ui, &mut cfg.realm, &[("pc-garena".to_owned(), hotcool.as_str()), ("pc-ggg".to_owned(), intl.as_str())]) {
                cfg.use_intl_site = cfg.realm == "pc-ggg" && cfg.language == "cmn-Hant";
            }
        });
    }
    if cfg.language != "en" && cfg.realm == "pc-ggg" {
        let forced = cfg.realm == "pc-ggg" && cfg.language == "cmn-Hant";
        switch_setting(ui, &i18n.t("settings.use_intl_site"), Some("www.pathofexile.com"), &mut cfg.use_intl_site);
        if forced {
            cfg.use_intl_site = true;
        }
    }

    rpt_m3::section_header(ui, i18n.t("settings.general"));
    switch_setting(ui, &i18n.t("settings.restore_clipboard"), None, &mut cfg.restore_clipboard);
    ui.add_space(8.0);
    field_label(ui, &i18n.t("settings.poe_log_file"));
    opt_text_field(ui, &mut cfg.client_log, "…/Grinding Gear Games/Path of Exile/logs/Client.txt");
    ui.add_space(12.0);
    field_label(ui, &i18n.t("settings.poe_cfg_file"));
    opt_text_field(ui, &mut cfg.game_config, "…/My Games/Path of Exile/production_Config.ini");
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        field_label(ui, &i18n.t("settings.window_title"));
        rpt_m3::key_cap(ui, &i18n.t("Restart required"));
    });
    rpt_m3::text_field(ui, &mut cfg.window_title, "Path of Exile");

    rpt_m3::section_header(ui, i18n.t("settings.overlay"));
    field_label(ui, &i18n.t("settings.overlay_bg"));
    ui.horizontal(|ui| {
        let mut col = crate::app::parse_css_rgba(&cfg.overlay_background).unwrap_or(Color32::TRANSPARENT);
        if egui::widgets::color_picker::color_edit_button_srgba(ui, &mut col, egui::widgets::color_picker::Alpha::OnlyBlend).changed() {
            let [r, g, b, a] = col.to_srgba_unmultiplied();
            cfg.overlay_background = format!("rgba({r}, {g}, {b}, {})", (a as f32 / 255.0 * 100.0).round() / 100.0);
        }
        ui.scope(|ui| {
            ui.set_width(220.0);
            rpt_m3::text_field(ui, &mut cfg.overlay_background, "rgba(129, 139, 149, 0.15)");
        });
        let transparent = cfg.overlay_background == TRANSPARENT_BG;
        if ui.add(Chip::filter(i18n.t("settings.overlay_bg_none"), transparent)).clicked() && !transparent {
            cfg.overlay_background = TRANSPARENT_BG.into();
        }
    });
    if cfg.overlay_background != TRANSPARENT_BG {
        switch_setting(ui, &i18n.t("settings.overlay_bg_focus_game"), None, &mut cfg.overlay_background_close);
    }
    switch_setting(ui, &i18n.t("settings.show_overlay_ready"), None, &mut cfg.show_attach_notification);
}

// ---------------------------------------------------------------------------
// Appearance (Rusty-only: Material You theme)

const PRESETS: &[(u32, &str)] = &[
    (0xC08A3E, "Exalted gold"),
    (0x6750A4, "Baseline purple"),
    (0xB3261E, "Chaos red"),
    (0x00639B, "Divine blue"),
    (0x386A20, "Verdant"),
    (0x006A6A, "Teal"),
    (0x8B5000, "Amber"),
    (0x7D5260, "Rose"),
];

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn variant_key(v: SchemeVariant) -> String {
    v.label().to_lowercase().replace(' ', "-")
}

pub fn appearance(ui: &mut Ui, w: &mut WCtx<'_>, _st: &mut SettingsState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let cfg = &mut *w.cfg;
    let rpt = &mut cfg.rpt;

    rpt_m3::section_header(ui, "Color");
    let custom = crate::config::parse_hex(&rpt.seed_color);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 8.0);
        if ui.add(Chip::filter("System accent", custom.is_none()).leading(icons::BRIGHTNESS_AUTO)).on_hover_text("Use the operating system accent color").clicked() {
            rpt.seed_color = "system".into();
        }
        ui.add_space(8.0);
        for (rgb, name) in PRESETS {
            let col = Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, *rgb as u8);
            if rpt_m3::color_swatch(ui, col, custom == Some(col), 36.0).on_hover_text(*name).clicked() {
                rpt.seed_color = hex(col);
            }
        }
        ui.add_space(8.0);
        let mut col = custom.unwrap_or(th.seed);
        let resp = egui::widgets::color_picker::color_edit_button_srgba(ui, &mut col, egui::widgets::color_picker::Alpha::Opaque).on_hover_text("Custom seed color");
        if resp.changed() {
            rpt.seed_color = hex(col);
        }
    });
    ui.label(RichText::new(format!("Seed {}", hex(th.seed))).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));

    rpt_m3::section_header(ui, "Theme");
    rpt_m3::setting_row(ui, Some(icons::DARK_MODE), "Brightness", None, |ui| {
        rpt_m3::segmented(ui, &mut rpt.brightness, &[("system".to_owned(), "System"), ("light".to_owned(), "Light"), ("dark".to_owned(), "Dark")]);
    });
    field_label(ui, "Scheme variant");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for v in SchemeVariant::ALL {
            let key = variant_key(*v);
            let sel = rpt.scheme_variant == key || (*v == SchemeVariant::default() && !SchemeVariant::ALL.iter().any(|x| variant_key(*x) == rpt.scheme_variant));
            if ui.add(Chip::filter(v.label(), sel)).clicked() {
                rpt.scheme_variant = key;
            }
        }
    });
    ui.add_space(8.0);
    rpt_m3::setting_row(ui, Some(icons::CONTRAST), "Contrast", Some(contrast_name(rpt.contrast)), |ui| {
        if ui.add(Button::text("").icon(icons::RESTART_ALT).small().enabled(rpt.contrast != 0.0)).on_hover_text("Standard").clicked() {
            rpt.contrast = 0.0;
        }
        rpt_m3::slider(ui, &mut rpt.contrast, -1.0..=1.0, 0.1, 220.0);
    });
    let mut fs = cfg.font_size as f64;
    let fs_label = w.i18n.t("settings.font_size");
    rpt_m3::setting_row(ui, Some(icons::FORMAT_SIZE), &fs_label, Some(&format!("{} px", fs)), |ui| {
        if rpt_m3::slider(ui, &mut fs, 10.0..=24.0, 1.0, 220.0).changed() {
            cfg.font_size = fs as f32;
        }
    });

    rpt_m3::section_header(ui, "Preview");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let tiles = [
            ("Primary", c.primary_container, c.on_primary_container, c.primary),
            ("Secondary", c.secondary_container, c.on_secondary_container, c.secondary),
            ("Tertiary", c.tertiary_container, c.on_tertiary_container, c.tertiary),
            ("Surface", c.surface_container_highest, c.on_surface, c.outline),
        ];
        for (name, bg, fg, accent) in tiles {
            let (r, _) = ui.allocate_exact_size(vec2(132.0, 88.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, CornerRadius::same(shape::LARGE), bg);
            p.circle_filled(r.right_top() + vec2(-20.0, 20.0), 10.0, accent);
            p.text(r.left_bottom() + vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, name, th.font(TypeRole::TitleSmall), fg);
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let _ = ui.add(Button::filled("Filled"));
        let _ = ui.add(Button::tonal("Tonal"));
        let _ = ui.add(Button::outlined("Outlined"));
        let mut on = true;
        rpt_m3::switch(ui, &mut on);
        let _ = ui.add(Chip::filter("Chip", true));
    });
}

fn contrast_name(v: f64) -> &'static str {
    match v {
        v if v <= -0.5 => "Reduced",
        v if v < 0.25 => "Standard",
        v if v < 0.75 => "Medium",
        _ => "High",
    }
}

// ---------------------------------------------------------------------------
// Widgets (Rusty-only: entry point to per-widget settings)

pub fn widgets(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let i18n = w.i18n;
    for wd in w.cfg.widgets.iter_mut() {
        if let WidgetKind::Menu(m) = &mut wd.kind {
            switch_setting(ui, &i18n.t("widget_menu.always_show"), None, &mut m.always_show);
        }
    }
    rpt_m3::section_header(ui, "Widgets");
    let list: Vec<(u32, &'static str, Page, String)> = w
        .cfg
        .widgets
        .iter()
        .filter_map(|x| {
            let (icon, page) = match x.kind {
                WidgetKind::StashSearch(_) => (icons::INVENTORY_2, Page::StashSearch),
                WidgetKind::Timer(_) => (icons::TIMER, Page::Stopwatch),
                _ => return None,
            };
            Some((x.wm_id, icon, page, crate::widgets::display_title(i18n, &x.kind, &x.wm_title)))
        })
        .collect();
    for (id, icon, page, title) in list {
        let kind_name = match page {
            Page::StashSearch => i18n.t("stash_search.name"),
            _ => i18n.t("stopwatch.name"),
        };
        if rpt_m3::list_item(ui, Some(icon), &title, Some(&kind_name), Some(icons::CHEVRON_RIGHT)).clicked() {
            st.target = Some(id);
            st.page = page;
        }
    }
}

// ---------------------------------------------------------------------------
// Debug (upstream settings/debug.vue)

pub fn debug(ui: &mut Ui, w: &mut WCtx<'_>, _st: &mut SettingsState) {
    let th = theme(ui.ctx());
    switch_setting(ui, &w.i18n.t("settings.debug_hotkeys"), Some("Key presses are shown in the bottom-left corner while the game is focused"), &mut w.cfg.log_keys);
    rpt_m3::section_header(ui, "Config file");
    let path = crate::store::ConfigStore::new().path;
    rpt_m3::card(ui, CardKind::Filled).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new(path.display().to_string()).monospace().color(th.colors.on_surface)).wrap());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(IconButton::new(icons::CONTENT_COPY).tooltip("Copy")).clicked() {
                    ui.ctx().copy_text(path.display().to_string());
                }
            });
        });
    });
    ui.add_space(8.0);
    if ui.add(Button::tonal("Open config folder").icon(icons::FOLDER_OPEN)).clicked() {
        w.host.push(HostCommand::OpenConfigFolder);
    }
}

// ---------------------------------------------------------------------------
// About (upstream settings/about.vue + the patrons strip of SettingsWindow.vue)

const UPSTREAM_URL: &str = "https://github.com/SnosMe/awakened-poe-trade";

pub fn about(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut SettingsState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let i18n = w.i18n;
    let repo = env!("CARGO_PKG_REPOSITORY");

    rpt_m3::card(ui, CardKind::Filled).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if let Some(src) = crate::widgets::image_strip::bundled_source("TransferOrb.png") {
                ui.add(egui::Image::new(src).fit_to_exact_size(vec2(56.0, 56.0)));
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("Rusty POE Trade").font(th.font(TypeRole::HeadlineSmall)).color(c.on_surface));
                ui.label(RichText::new(i18n.tf("app.version", &[env!("CARGO_PKG_VERSION")])).color(c.on_surface_variant));
            });
        });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add(Chip::assist(i18n.t("app.release_notes")).leading(icons::OPEN_IN_NEW)).clicked() {
                w.host.push(HostCommand::OpenUrl(format!("{repo}/releases")));
            }
            if ui.add(Chip::assist(i18n.t("app.report_bug")).leading(icons::BUG_REPORT)).clicked() {
                w.host.push(HostCommand::OpenUrl(format!("{repo}/issues")));
            }
        });
    });

    rpt_m3::section_header(ui, "Updates");
    switch_setting(ui, "Check for updates automatically", None, &mut w.cfg.rpt.check_for_updates);
    if ui.add(Button::tonal(i18n.t("updates.check_now")).icon(icons::UPDATE)).clicked() {
        w.host.push(HostCommand::CheckForUpdate);
    }

    rpt_m3::section_header(ui, "Credits");
    rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label("A Rust rewrite of Awakened PoE Trade by SnosMe, used under the MIT License.");
        ui.horizontal_wrapped(|ui| {
            if ui.add(Chip::assist("Awakened PoE Trade on GitHub").leading(icons::LINK)).clicked() {
                w.host.push(HostCommand::OpenUrl(UPSTREAM_URL.into()));
            }
            if ui.add(Chip::assist(i18n.t("Support development on") + " Patreon").leading(icons::VOLUNTEER_ACTIVISM)).clicked() {
                w.host.push(HostCommand::OpenUrl("https://patreon.com/awakened_poe_trade".into()));
            }
        });
    });

    rpt_m3::section_header(ui, i18n.t("settings.thank_you"));
    let tiers = st.patrons.get_or_insert_with(|| {
        let all = rpt_data::patrons();
        (1..=5u32).rev().map(|s| all.iter().filter(|p| p.style == s).cloned().collect()).collect()
    });
    for (i, tier) in tiers.iter().enumerate() {
        if tier.is_empty() {
            continue;
        }
        let style = 5 - i as u32;
        let (bg, fg, role) = match style {
            5 => (Color32::WHITE, Color32::from_rgb(255, 0, 0), TypeRole::TitleMedium),
            4 => (Color32::from_rgb(240, 90, 35), Color32::WHITE, TypeRole::TitleSmall),
            3 => (Color32::from_rgb(213, 159, 0), Color32::BLACK, TypeRole::LabelLarge),
            2 => (Color32::from_rgb(210, 178, 135), Color32::BLACK, TypeRole::LabelLarge),
            _ => (c.surface_container_highest, c.on_surface_variant, TypeRole::LabelMedium),
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for p in tier {
                let text = if p.months > 1 { format!("{} ×{}", p.from, p.months) } else { p.from.clone() };
                let g = ui.painter().layout_no_wrap(text, th.font(role), fg);
                let (r, _) = ui.allocate_exact_size(g.size() + vec2(16.0, 8.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(shape::SMALL), bg);
                ui.painter().galley(r.center() - g.size() / 2.0, g, fg);
            }
        });
        ui.add_space(8.0);
    }
}
