//! Port of upstream `overlay/WidgetMenu.vue`: the row of toggle buttons for
//! every widget, plus the "add widget" menu and settings/quit shortcuts.

use egui::Context;
use rpt_m3::{Button, ButtonKind, IconButton, IconButtonKind, icons};

use super::{WCtx, chrome};
use crate::config::{Anchor, ImageStripWidget, StashSearchWidget, StopwatchWidget, StripImage, WidgetKind};
use crate::host::HostCommand;
use crate::wm;

#[derive(Default)]
pub struct MenuState {
    add_open: bool,
}

fn random_center() -> Anchor {
    // upstream: random position in the 40–60% range so new widgets don't stack
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let r1 = (t % 1000) as f32 / 1000.0;
    let r2 = ((t / 1000) % 1000) as f32 / 1000.0;
    Anchor { pos: "cc".into(), x: 40.0 + r1 * 20.0, y: 40.0 + r2 * 20.0 }
}

pub fn show(ctx: &Context, w: &mut WCtx<'_>, st: &mut MenuState, wm_id: u32) {
    let always_show = matches!(w.cfg.widgets.iter().find(|x| x.wm_id == wm_id).map(|x| &x.kind), Some(WidgetKind::Menu(m)) if m.always_show);
    if !w.st.active && !always_show {
        return;
    }
    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts { removable: false, hideable: false, surface: true }, |ui, w| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let entries: Vec<(u32, String, bool)> = w
                .cfg
                .widgets
                .iter()
                .filter(|x| !x.has_flag("menu::skip"))
                .map(|x| (x.wm_id, super::display_title(w.i18n, &x.kind, &x.wm_title), x.wants_show()))
                .collect();
            for (id, title, shown) in entries {
                let kind = w.cfg.widgets.iter().find(|x| x.wm_id == id).map(|x| x.kind.type_name()).unwrap_or("");
                let btn = match kind {
                    "settings" => Button::new(ButtonKind::Tonal, "").icon(icons::SETTINGS),
                    "item-search" => Button::new(ButtonKind::Tonal, "").icon(icons::SEARCH),
                    _ => Button::new(ButtonKind::Tonal, title),
                };
                if ui.add(btn.small().selected(shown)).clicked() {
                    wm::toggle(w.cfg, id);
                    w.save_requested = true;
                }
            }
            let add = ui.add(IconButton::new(icons::ADD).kind(IconButtonKind::Tonal).size(32.0).tooltip(w.i18n.t("widget.add")));
            if add.clicked() {
                st.add_open = !st.add_open;
            }
            if ui.add(IconButton::new(icons::CLOSE).size(32.0).tooltip(w.i18n.t("app.quit"))).double_clicked() {
                w.host.push(HostCommand::Quit);
            }
            if st.add_open {
                ui.separator();
                let mut created = None;
                if ui.add(Button::text(w.i18n.t("stopwatch.name")).icon(icons::TIMER).small()).clicked() {
                    created = Some((WidgetKind::Timer(StopwatchWidget { anchor: random_center(), toggle_key: None, reset_key: None }), String::new()));
                }
                if ui.add(Button::text(w.i18n.t("stash_search.name")).icon(icons::SEARCH).small()).clicked() {
                    created = Some((WidgetKind::StashSearch(StashSearchWidget { anchor: random_center(), enable_hotkeys: true, entries: vec![] }), String::new()));
                }
                if ui.add(Button::text(w.i18n.t("image_strip.name")).icon(icons::IMAGE).small()).clicked() {
                    created = Some((
                        WidgetKind::ImageStrip(ImageStripWidget {
                            anchor: Anchor { pos: "tc".into(), ..random_center() },
                            images: vec![StripImage { id: 1, url: "syndicate.jpg".into() }],
                        }),
                        String::new(),
                    ));
                }
                if let Some((kind, title)) = created {
                    wm::create(w.cfg, kind, &title);
                    st.add_open = false;
                    w.save_requested = true;
                }
            }
        });
    });
    chrome::apply(w, wm_id, &r);
}
