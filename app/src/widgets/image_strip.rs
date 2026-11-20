//! Port of upstream `overlay/WidgetImageStrip.vue` + `ui/FullscreenImage.vue`:
//! a titled strip of image cards (cheat sheets). Hovering a card (entering it
//! from outside) or clicking it shows the image fullscreen. In edit mode the
//! title can be changed, images reordered/removed, and new ones added from a
//! URL / file path or by dropping files onto the overlay.

use std::borrow::Cow;

use egui::{
    Align2, Area, Context, CornerRadius, Id, ImageSource, Order, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
    load::TexturePoll, vec2,
};
use rpt_m3::{Button, IconButton, IconButtonKind, TypeRole, icons, shape, theme, with_alpha};

use super::{WCtx, chrome};
use crate::config::{StripImage, WidgetKind};

const CARD: Vec2 = Vec2::new(192.0, 104.0);

#[derive(Default)]
pub struct ImageStripState {
    editing: bool,
    new_url: String,
    /// Index of the card whose image is shown fullscreen.
    fullscreen: Option<usize>,
    /// Pointer was already inside a card when it appeared; wait for it to leave.
    requires_leave: Option<usize>,
}

macro_rules! bundled_images {
    ($($name:literal),* $(,)?) => {
        /// Images shipped with the app (upstream `public/images/`).
        pub const BUNDLED: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../../assets/images/", $name)))),*
        ];
    };
}

bundled_images!(
    "chaos.png", "dc_reddit.png", "dc_tft.gif", "delve.png", "divine.png", "dust.png", "essence.png", "exa.png",
    "increased.png", "incursion.png", "influence-Crusader.png", "influence-Elder.png", "influence-Hunter.png",
    "influence-Redeemer.png", "influence-Shaper.png", "influence-Warlord.png", "maven-witness.png", "mercenary.png",
    "Patreon.svg", "peepoLove2x.webp", "rareonly.png", "ReflectingMist.png", "stash.png", "syndicate.jpg",
    "TransferOrb.png", "veiled.png", "VolatileVaalOrb.png", "warband.png", "wisdom.png",
);

/// Bytes of a bundled image by file name (e.g. `"syndicate.jpg"`).
pub fn bundled(name: &str) -> Option<&'static [u8]> {
    BUNDLED.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// `ImageSource` for a bundled image (for use by other widgets/pages).
pub fn bundled_source(name: &str) -> Option<ImageSource<'static>> {
    bundled(name).map(|b| ImageSource::Bytes { uri: Cow::Owned(format!("bytes://images/{name}")), bytes: b.into() })
}

/// Resolve an image-strip `url` the way upstream `FullscreenImage` does:
/// `scheme://…` as-is; short names → bundled images; long names → files
/// imported into the uploads dir; anything else → a local file path.
pub fn image_source(url: &str) -> ImageSource<'static> {
    if url.contains("://") {
        return ImageSource::Uri(Cow::Owned(url.to_owned()));
    }
    let is_bare_name = !url.contains('/') && !url.contains('\\');
    if is_bare_name && url.len() < 32 {
        if let Some(src) = bundled_source(url) {
            return src;
        }
    }
    let path = std::path::Path::new(url);
    if is_bare_name {
        let mut dirs = vec![crate::store::ConfigStore::new().dir().join("uploads")];
        dirs.extend(crate::store::apt_config_candidates().into_iter().filter_map(|p| p.parent().map(|d| d.join("uploads"))));
        if let Some(found) = dirs.into_iter().map(|d| d.join(url)).find(|p| p.exists()) {
            return file_uri(&found);
        }
    }
    file_uri(path)
}

fn file_uri(p: &std::path::Path) -> ImageSource<'static> {
    let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    ImageSource::Uri(Cow::Owned(format!("file://{}", abs.display())))
}

/// Paint an image filling `rect` like CSS `object-fit: cover`. Returns false if it failed to load.
fn paint_cover(ui: &mut Ui, src: &ImageSource<'static>, rect: Rect, radius: CornerRadius) -> bool {
    let c = theme(ui.ctx()).colors;
    let img = egui::Image::new(src.clone());
    match img.load_for_size(ui.ctx(), rect.size()) {
        Ok(TexturePoll::Ready { texture }) => {
            let ts = texture.size;
            let (ra, ta) = (rect.width() / rect.height(), ts.x / ts.y.max(1.0));
            let uv = if ta > ra {
                let w = ra / ta;
                Rect::from_min_max(Pos2::new((1.0 - w) / 2.0, 0.0), Pos2::new((1.0 + w) / 2.0, 1.0))
            } else {
                let h = ta / ra;
                Rect::from_min_max(Pos2::new(0.0, (1.0 - h) / 2.0), Pos2::new(1.0, (1.0 + h) / 2.0))
            };
            egui::Image::from_texture(texture).uv(uv).corner_radius(radius).paint_at(ui, rect);
            true
        }
        Ok(TexturePoll::Pending { .. }) => {
            ui.painter().rect_filled(rect, radius, c.surface_container_highest);
            let t = ui.input(|i| i.time) as f32;
            let a = t * 5.0;
            let center = rect.center();
            let pts: Vec<Pos2> = (0..=16).map(|i| center + vec2((a + i as f32 * 0.25).cos(), (a + i as f32 * 0.25).sin()) * 10.0).collect();
            ui.painter().add(egui::Shape::line(pts, Stroke::new(3.0, c.primary)));
            ui.ctx().request_repaint();
            true
        }
        Err(_) => {
            ui.painter().rect_filled(rect, radius, c.error_container);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                icons::BROKEN_IMAGE,
                egui::FontId::new(32.0, egui::FontFamily::Name(rpt_m3::theme::ICONS.into())),
                c.on_error_container,
            );
            false
        }
    }
}

pub fn show(ctx: &Context, w: &mut WCtx<'_>, st: &mut ImageStripState, wm_id: u32) {
    if !w.st.active {
        st.editing = false;
    }
    let th = theme(ctx);
    let c = th.colors;
    let mut fullscreen_src: Option<ImageSource<'static>> = None;

    let r = chrome::anchored(ctx, w, wm_id, chrome::ChromeOpts::default(), |ui, w| {
        let active = w.st.active;
        let Some(widget) = w.cfg.widgets.iter_mut().find(|x| x.wm_id == wm_id) else { return false };
        let WidgetKind::ImageStrip(strip) = &mut widget.kind else { return false };
        let mut changed = false;

        // Header: title (or title field while editing) + edit toggle.
        ui.horizontal(|ui| {
            ui.set_min_width(80.0);
            if st.editing {
                ui.scope(|ui| {
                    ui.set_width(CARD.x);
                    if rpt_m3::text_field(ui, &mut widget.wm_title, &w.i18n.t("widget.title")).changed() {
                        changed = true;
                    }
                });
            } else {
                let title = if widget.wm_title.is_empty() { "Untitled" } else { widget.wm_title.as_str() };
                ui.label(RichText::new(title).font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
            }
            if active {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = IconButton::new(if st.editing { icons::DONE } else { icons::EDIT })
                        .kind(if st.editing { IconButtonKind::Filled } else { IconButtonKind::Standard })
                        .size(32.0)
                        .tooltip(w.i18n.t("widget.edit"));
                    if ui.add(b).clicked() {
                        st.editing = !st.editing;
                        st.fullscreen = None;
                    }
                });
            }
        });

        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let n = strip.images.len();
        let mut any_hovered = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (i, img) in strip.images.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(CARD, Sense::click());
                let radius = CornerRadius::same(shape::MEDIUM);
                ui.painter().rect_filled(rect, radius, c.surface_container_highest);
                let src = image_source(&img.url);
                paint_cover(ui, &src, rect.shrink(4.0), CornerRadius::same(shape::SMALL));
                if resp.hovered() {
                    ui.painter().rect_stroke(rect, radius, Stroke::new(2.0, c.primary), StrokeKind::Inside);
                }

                if st.editing {
                    // Remove (top-right) and reorder (left/right) controls replace upstream's drag handle.
                    let close = Rect::from_min_size(Pos2::new(rect.right() - 32.0, rect.top() + 2.0), Vec2::splat(30.0));
                    if ui.put(close, IconButton::new(icons::CLOSE).kind(IconButtonKind::Filled).size(28.0).tooltip(w.i18n.t("Remove"))).clicked() {
                        remove = Some(i);
                    }
                    let y = rect.center().y - 15.0;
                    if i > 0 {
                        let l = Rect::from_min_size(Pos2::new(rect.left() + 2.0, y), Vec2::splat(30.0));
                        if ui.put(l, IconButton::new(icons::CHEVRON_LEFT).kind(IconButtonKind::Tonal).size(28.0)).clicked() {
                            swap = Some((i, i - 1));
                        }
                    }
                    if i + 1 < n {
                        let rr = Rect::from_min_size(Pos2::new(rect.right() - 32.0, y), Vec2::splat(30.0));
                        if ui.put(rr, IconButton::new(icons::CHEVRON_RIGHT).kind(IconButtonKind::Tonal).size(28.0)).clicked() {
                            swap = Some((i, i + 1));
                        }
                    }
                    continue;
                }

                // FullscreenImage hover semantics.
                if resp.hovered() {
                    any_hovered = true;
                    if st.fullscreen != Some(i) && st.requires_leave != Some(i) {
                        let (pos, delta) = ui.input(|inp| (inp.pointer.hover_pos(), inp.pointer.delta()));
                        let was_outside = pos.is_some_and(|p| !rect.shrink(2.0).contains(p - delta));
                        if was_outside {
                            st.fullscreen = Some(i);
                        } else {
                            st.requires_leave = Some(i);
                        }
                    }
                    if resp.clicked() {
                        st.fullscreen = Some(i);
                    }
                } else {
                    if st.fullscreen == Some(i) {
                        st.fullscreen = None;
                    }
                    if st.requires_leave == Some(i) {
                        st.requires_leave = None;
                    }
                }
                if st.fullscreen == Some(i) {
                    fullscreen_src = Some(src);
                }
            }

            if st.editing {
                // "Add image" card: URL/path field + button; files can also be dropped on the overlay.
                ui.allocate_ui(CARD, |ui| {
                    rpt_m3::card(ui, rpt_m3::CardKind::Outlined).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                        ui.set_min_size(CARD - vec2(16.0, 16.0));
                        ui.set_max_width(CARD.x - 16.0);
                        ui.spacing_mut().item_spacing.y = 6.0;
                        let resp = ui.add(egui::TextEdit::singleline(&mut st.new_url).hint_text("https://… / C:\\…\\image.png").desired_width(f32::INFINITY));
                        let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let add = ui.add(Button::tonal(w.i18n.t("Add")).icon(icons::ADD_PHOTO_ALTERNATE).small().enabled(!st.new_url.trim().is_empty())).clicked();
                        if (add || submit) && !st.new_url.trim().is_empty() {
                            let url = st.new_url.trim().trim_matches('"').to_owned();
                            push_image(&mut strip.images, url);
                            st.new_url.clear();
                            changed = true;
                        }
                        ui.label(RichText::new(w.i18n.t("choose_file") + " ⤓").font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant))
                            .on_hover_text("Drop image files onto the overlay to add them");
                    });
                });
            }
        });
        if !any_hovered && !st.editing {
            st.fullscreen = None;
            st.requires_leave = None;
        }

        if st.editing {
            let dropped: Vec<String> = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().display().to_string()).filter(|s| !s.is_empty()).collect());
            for p in dropped {
                push_image(&mut strip.images, p);
                changed = true;
            }
        }
        if let Some((a, b)) = swap {
            strip.images.swap(a, b);
            changed = true;
        }
        if let Some(i) = remove {
            strip.images.remove(i);
            changed = true;
        }
        changed
    });
    if r.inner == Some(true) {
        w.save_requested = true;
    }
    chrome::apply(w, wm_id, &r);

    if let Some(src) = fullscreen_src {
        show_fullscreen(ctx, w.screen, src);
    }
}

fn push_image(images: &mut Vec<StripImage>, url: String) {
    let id = images.iter().map(|x| x.id).max().unwrap_or(0) + 1;
    images.push(StripImage { id, url });
}

/// Upstream `.imgFullscreenWrapper`: centered, max-height image with a frame, no pointer events.
fn show_fullscreen(ctx: &Context, screen: Rect, src: ImageSource<'static>) {
    let c = theme(ctx).colors;
    Area::new(Id::unique("rpt-image-fullscreen"))
        .order(Order::Tooltip)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(c.surface_container_highest)
                .corner_radius(CornerRadius::same(shape::MEDIUM))
                .inner_margin(egui::Margin::same(4))
                .shadow(egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: with_alpha(c.shadow, 0.75) })
                .show(ui, |ui| {
                    let max = screen.size() - vec2(40.0, 40.0);
                    ui.add(egui::Image::new(src).max_size(max).fit_to_exact_size(max).maintain_aspect_ratio(true).corner_radius(CornerRadius::same(shape::SMALL)));
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_sources() {
        assert!(matches!(image_source("syndicate.jpg"), ImageSource::Bytes { .. }));
        assert!(matches!(image_source("https://example.com/a.png"), ImageSource::Uri(u) if u == "https://example.com/a.png"));
        assert!(matches!(image_source("/tmp/x.png"), ImageSource::Uri(u) if u.starts_with("file://")));
    }
}
