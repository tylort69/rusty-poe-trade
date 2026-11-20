//! Port of upstream `stash-search/WidgetStashSearch.vue`: a titled list of
//! saved stash searches. Clicking an entry types it into the stash search box
//! (`HostCommand::StashSearch`). The entries are edited on the widget's
//! settings page (`settings::widget_pages::stash_search`).

use egui::{Align2, Context, CornerRadius, Pos2, RichText, Sense, vec2};
use rpt_m3::{IconButton, TypeRole, icons, shape, theme, with_alpha};

use super::{WCtx, chrome};
use crate::config::WidgetKind;
use crate::host::HostCommand;

pub fn show(ctx: &Context, w: &mut WCtx<'_>, wm_id: u32) {
    let th = theme(ctx);
    let c = th.colors;
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts::default(), |ui, w| {
        let active = w.st.active;
        let i18n = w.i18n;
        let Some(widget) = w.cfg.widgets.iter_mut().find(|x| x.wm_id == wm_id) else { return (None, false, false) };
        let WidgetKind::StashSearch(ss) = &mut widget.kind else { return (None, false, false) };
        let mut search = None;
        let mut keys_changed = false;
        let mut open_settings = false;

        ui.set_min_width(160.0);
        ui.horizontal(|ui| {
            let title = if widget.wm_title.is_empty() { "Untitled" } else { widget.wm_title.as_str() };
            ui.add(egui::Label::new(RichText::new(title).font(th.font(TypeRole::TitleSmall)).color(c.on_surface)).truncate());
            // left to right: a right-to-left layout fills the available width
            // and stretched the window to the screen edge
            if ss.entries.iter().any(|e| e.hotkey.is_some()) {
                ui.add_space(8.0);
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(i18n.t("stash_search.enable_keys")).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant));
                    if rpt_m3::switch(ui, &mut ss.enable_hotkeys).changed() {
                        keys_changed = true;
                    }
                });
            }
            if active && ui.add(IconButton::new(icons::SETTINGS).size(32.0).tooltip(i18n.t("settings.title"))).clicked() {
                open_settings = true;
            }
        });
        ui.add_space(4.0);

        egui::ScrollArea::vertical().max_height(ui.available_height().max(120.0)).auto_shrink([true, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for entry in &ss.entries {
                let label = if entry.name.is_empty() { entry.text.as_str() } else { entry.name.as_str() };
                let font = th.font(TypeRole::LabelLarge);
                let hotkey = entry.hotkey.as_deref().filter(|_| ss.enable_hotkeys);
                let key_g = hotkey.map(|k| ui.painter().layout_no_wrap(k.to_owned(), th.font(TypeRole::LabelSmall), c.on_tertiary_container));
                let key_w = key_g.as_ref().map_or(0.0, |g| g.size().x + 12.0 + 8.0);
                let max_w = 384.0f32.min(ui.available_width().max(160.0));
                let g = ui.painter().layout(label.to_owned(), font, c.on_secondary_container, f32::INFINITY);
                let w_btn = (g.size().x + 32.0 + key_w).clamp(160.0, max_w);
                let (rect, resp) = ui.allocate_exact_size(vec2(w_btn, 40.0), Sense::click());
                let radius = CornerRadius::same(shape::SMALL);
                let p = ui.painter();
                p.rect_filled(rect, radius, c.secondary_container);
                let o = if resp.is_pointer_button_down_on() { 0.12 } else if resp.hovered() { 0.08 } else { 0.0 };
                if o > 0.0 {
                    p.rect_filled(rect, radius, with_alpha(c.on_secondary_container, o));
                }
                let text_clip = egui::Rect::from_min_max(rect.min + vec2(16.0, 0.0), Pos2::new(rect.right() - 16.0 - key_w, rect.bottom()));
                p.with_clip_rect(text_clip).galley(Pos2::new(text_clip.left(), rect.center().y - g.size().y / 2.0), g.clone(), c.on_secondary_container);
                if let Some(kg) = key_g {
                    let kr = egui::Rect::from_min_size(Pos2::new(rect.right() - 12.0 - kg.size().x - 12.0, rect.center().y - 10.0), vec2(kg.size().x + 12.0, 20.0));
                    p.rect_filled(kr, CornerRadius::same(shape::EXTRA_SMALL), c.tertiary_container);
                    p.galley(kr.center() - kg.size() / 2.0, kg, c.on_tertiary_container);
                }
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                let resp = if g.size().x + 32.0 + key_w > w_btn { resp.on_hover_text(&entry.text) } else { resp };
                if resp.clicked() {
                    search = Some(entry.text.clone());
                }
            }
            if ss.entries.is_empty() {
                ui.painter().text(ui.cursor().min, Align2::LEFT_TOP, "—", th.font(TypeRole::BodyMedium), c.on_surface_variant);
                ui.add_space(20.0);
            }
        });
        (search, keys_changed, open_settings)
    });
    if let Some((search, keys_changed, open_settings)) = &r.inner {
        if let Some(text) = search {
            w.host.push(HostCommand::StashSearch(text.clone()));
        }
        if *keys_changed {
            w.host.push(HostCommand::PushHostConfig);
            w.save_requested = true;
        }
        if *open_settings {
            super::settings::open_for(w.cfg, wm_id);
        }
    }
    chrome::apply(w, wm_id, &r);
}
