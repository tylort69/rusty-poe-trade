//! Small shared pieces: upstream `ui/ItemQuickPrice.vue`, `ui/ItemModifierText.vue`,
//! `trade/artificial-slowdown.ts`, bundled images, rarity / tag colors.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use egui::text::LayoutJob;
use egui::{Color32, ImageSource, RichText, TextFormat, Ui, vec2};
use rpt_m3::{CustomColor, TypeRole, custom_color, icons, theme};
use rpt_parser::{ItemCategory, ItemRarity, ParsedItem};
use rpt_trade::prices::{CurrencyValue, PriceCurrency, display_rounding};

/// Image shipped with the app (upstream `/images/<name>`).
pub fn bundled(name: &str) -> ImageSource<'static> {
    crate::widgets::image_strip::bundled_source(name).unwrap_or_else(|| ImageSource::Uri(Cow::Owned(format!("bytes://images/{name}"))))
}

/// Remote item icon (web.poecdn.com) or nothing when the data has no icon.
pub fn icon(ui: &mut Ui, url: &str, size: egui::Vec2) -> egui::Response {
    if url.is_empty() {
        return ui.allocate_exact_size(size, egui::Sense::hover()).1;
    }
    ui.add(egui::Image::new(url.to_owned()).fit_to_exact_size(size).maintain_aspect_ratio(true))
}

/// `artificialSlowdown(ms)`: content counts as ready `ms` after the item changed.
#[derive(Debug, Clone, Copy)]
pub struct Slowdown {
    since: Instant,
}

impl Default for Slowdown {
    fn default() -> Self {
        Self { since: Instant::now() }
    }
}

impl Slowdown {
    pub fn is_ready(&self, ctx: &egui::Context, ms: u64) -> bool {
        let d = Duration::from_millis(ms);
        let e = self.since.elapsed();
        if e < d {
            ctx.request_repaint_after(d - e);
            false
        } else {
            true
        }
    }
}

/// Upstream rarity colors (PoE item frame text), harmonized with the theme.
pub fn rarity_design_color(item: &ParsedItem) -> Color32 {
    if item.category == Some(ItemCategory::Gem) {
        return Color32::from_rgb(0x1B, 0xA2, 0x9B);
    }
    if item.category == Some(ItemCategory::DivinationCard) {
        return Color32::from_rgb(0x0E, 0xBA, 0xFF);
    }
    match item.rarity {
        Some(ItemRarity::Normal) => Color32::from_rgb(0xC8, 0xC8, 0xC8),
        Some(ItemRarity::Magic) => Color32::from_rgb(0x88, 0x88, 0xFF),
        Some(ItemRarity::Rare) => Color32::from_rgb(0xFF, 0xFF, 0x77),
        Some(ItemRarity::Unique) => Color32::from_rgb(0xAF, 0x60, 0x25),
        None => Color32::from_rgb(0xAA, 0x9E, 0x82),
    }
}

pub fn harmonized(ui: &Ui, design: Color32) -> CustomColor {
    let c = theme(ui.ctx()).colors;
    custom_color(design, c.primary, c.dark)
}

/// Golden "divine" accent (upstream `#e4c29a`).
pub fn golden(ui: &Ui) -> Color32 {
    harmonized(ui, Color32::from_rgb(0xE4, 0xC2, 0x9A)).accent
}

pub fn currency_name(c: PriceCurrency) -> &'static str {
    match c {
        PriceCurrency::Chaos => "chaos",
        PriceCurrency::Div => "div",
    }
}

/// Options of upstream `ItemQuickPrice.vue`.
#[derive(Default, Clone)]
pub struct QuickPrice<'a> {
    /// item icon url (`None` → wisdom scroll)
    pub item_img: Option<&'a str>,
    /// bundled image instead of a remote icon (e.g. `divine.png`)
    pub item_bundled: Option<&'static str>,
    /// base type size in inventory cells (w, h)
    pub item_cells: (u32, u32),
    /// text instead of the image (e.g. "Base item")
    pub item_label: Option<String>,
    pub approx: bool,
    pub fraction: bool,
    pub currency_text: bool,
}

pub fn quick_price(ui: &mut Ui, price: Option<&CurrencyValue>, o: &QuickPrice<'_>) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let font = th.font(TypeRole::TitleMedium);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if let Some(l) = &o.item_label {
            ui.label(RichText::new(l).color(c.on_surface_variant));
        } else {
            let size = match o.item_cells {
                (_, h) if h > 1 => vec2(32.0, 40.0),
                (w, _) if w > 1 => vec2(48.0, 32.0),
                _ => vec2(32.0, 32.0),
            };
            match o.item_img {
                _ if o.item_bundled.is_some() => {
                    ui.add(egui::Image::new(bundled(o.item_bundled.unwrap_or("wisdom.png"))).fit_to_exact_size(size));
                }
                Some(url) if !url.is_empty() => {
                    icon(ui, url, size);
                }
                _ => {
                    ui.add(egui::Image::new(bundled("wisdom.png")).fit_to_exact_size(size));
                }
            }
        }
        ui.label(rpt_m3::icon_text(icons::ARROW_FORWARD, 16.0).color(c.outline));
        let valuable = price.is_some_and(|p| p.currency == PriceCurrency::Div);
        let val_color = if valuable { golden(ui) } else { c.on_surface };
        let min_t = price.map(|p| display_rounding(p.min, o.fraction)).unwrap_or_else(|| "?".into());
        let max_t = price.map(|p| display_rounding(p.max, o.fraction)).unwrap_or_else(|| "?".into());
        let is_range = min_t != max_t;
        if o.approx && !is_range {
            ui.label(RichText::new("~").font(font.clone()).color(c.outline));
        }
        ui.label(RichText::new(min_t).font(font.clone()).color(val_color));
        if is_range {
            ui.label(RichText::new("~").font(font.clone()).color(c.outline));
            ui.label(RichText::new(max_t).font(font.clone()).color(val_color));
        }
        if !o.currency_text {
            ui.label(RichText::new("×").font(font.clone()).color(val_color));
            ui.add(egui::Image::new(bundled(if valuable { "divine.png" } else { "chaos.png" })).fit_to_exact_size(vec2(28.0, 28.0)));
        } else if let Some(p) = price {
            ui.label(RichText::new(currency_name(p.currency)).font(font).color(val_color));
        }
    });
}

/// One piece of upstream `ItemModifierText` (`#` placeholders replaced by the roll).
#[derive(Debug, Clone, PartialEq)]
pub struct TextPart {
    pub text: String,
    pub placeholder: bool,
}

/// Splits `text` on `(?<![#])[+-]?#` and inserts `roll` (or `#`) at each match.
pub fn modifier_parts(text: &str, roll: Option<f64>) -> Vec<TextPart> {
    let chars: Vec<char> = text.chars().collect();
    let mut pieces: Vec<String> = vec![String::new()];
    let mut i = 0;
    while i < chars.len() {
        let prev_hash = i > 0 && chars[i - 1] == '#';
        let c = chars[i];
        let is_sign_hash = (c == '+' || c == '-') && chars.get(i + 1) == Some(&'#');
        if !prev_hash && (c == '#' || is_sign_hash) {
            pieces.push(String::new());
            i += if is_sign_hash { 2 } else { 1 };
            continue;
        }
        pieces.last_mut().expect("non-empty").push(c);
        i += 1;
    }
    let mut out = Vec::new();
    let n = pieces.len();
    for (idx, p) in pieces.into_iter().enumerate() {
        if !p.is_empty() {
            out.push(TextPart { text: p, placeholder: false });
        }
        if idx != n - 1 {
            match roll {
                None => out.push(TextPart { text: "#".into(), placeholder: false }),
                Some(r) => out.push(TextPart { text: rpt_trade::js::num_to_string(r), placeholder: true }),
            }
        }
    }
    out
}

/// Layout job for modifier text with highlighted roll values.
pub fn modifier_job(ui: &Ui, text: &str, roll: Option<f64>, wrap: bool, color: Option<Color32>) -> LayoutJob {
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut job = LayoutJob::default();
    let normal = TextFormat { font_id: th.font(TypeRole::BodyMedium), color: color.unwrap_or(c.on_surface), ..Default::default() };
    let value = TextFormat { font_id: th.font(TypeRole::LabelLarge), color: c.primary, ..Default::default() };
    for p in modifier_parts(text, roll) {
        job.append(&p.text, 0.0, if p.placeholder { value.clone() } else { normal.clone() });
    }
    if !wrap {
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(v: &[TextPart]) -> Vec<(&str, bool)> {
        v.iter().map(|p| (p.text.as_str(), p.placeholder)).collect()
    }

    #[test]
    fn modifier_text_parts() {
        assert_eq!(texts(&modifier_parts("+#% to Fire Resistance", Some(42.0))), [("42", true), ("% to Fire Resistance", false)]);
        assert_eq!(texts(&modifier_parts("Adds # to # Damage", None)), [("Adds ", false), ("#", false), (" to ", false), ("#", false), (" Damage", false)]);
        assert_eq!(texts(&modifier_parts("no rolls", Some(1.0))), [("no rolls", false)]);
        assert_eq!(texts(&modifier_parts("-#% x", Some(-3.5))), [("-3.5", true), ("% x", false)]);
    }
}
