//! Installing the Material You theme into an egui context: fonts, M3 type
//! scale, shape scale, and egui `Visuals` derived from the color scheme.

use std::sync::Arc;

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Margin, Shadow, Stroke,
    Style, TextStyle, Visuals,
    style::{Selection, WidgetVisuals, Widgets},
};

use crate::scheme::{Brightness, ColorScheme, DEFAULT_SEED, SchemeVariant, SeedSource, state_layer, with_alpha};

/// Family name for medium-weight Roboto (M3 "label"/"title" weight).
pub const MEDIUM: &str = "roboto-medium";
/// Family name for Material Icons.
pub const ICONS: &str = "material-icons";

/// M3 shape scale (dp).
pub mod shape {
    pub const NONE: u8 = 0;
    pub const EXTRA_SMALL: u8 = 4;
    pub const SMALL: u8 = 8;
    pub const MEDIUM: u8 = 12;
    pub const LARGE: u8 = 16;
    pub const EXTRA_LARGE: u8 = 28;
    pub const FULL: u8 = 255;
}

/// M3 type scale roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRole {
    DisplaySmall,
    HeadlineLarge,
    HeadlineMedium,
    HeadlineSmall,
    TitleLarge,
    TitleMedium,
    TitleSmall,
    BodyLarge,
    BodyMedium,
    BodySmall,
    LabelLarge,
    LabelMedium,
    LabelSmall,
}

impl TypeRole {
    pub const ALL: &'static [TypeRole] = &[
        Self::DisplaySmall,
        Self::HeadlineLarge,
        Self::HeadlineMedium,
        Self::HeadlineSmall,
        Self::TitleLarge,
        Self::TitleMedium,
        Self::TitleSmall,
        Self::BodyLarge,
        Self::BodyMedium,
        Self::BodySmall,
        Self::LabelLarge,
        Self::LabelMedium,
        Self::LabelSmall,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::DisplaySmall => "display-small",
            Self::HeadlineLarge => "headline-large",
            Self::HeadlineMedium => "headline-medium",
            Self::HeadlineSmall => "headline-small",
            Self::TitleLarge => "title-large",
            Self::TitleMedium => "title-medium",
            Self::TitleSmall => "title-small",
            Self::BodyLarge => "body-large",
            Self::BodyMedium => "body-medium",
            Self::BodySmall => "body-small",
            Self::LabelLarge => "label-large",
            Self::LabelMedium => "label-medium",
            Self::LabelSmall => "label-small",
        }
    }

    /// (size in sp, medium weight?)
    fn spec(self) -> (f32, bool) {
        match self {
            Self::DisplaySmall => (36.0, false),
            Self::HeadlineLarge => (32.0, false),
            Self::HeadlineMedium => (28.0, false),
            Self::HeadlineSmall => (24.0, false),
            Self::TitleLarge => (22.0, false),
            Self::TitleMedium => (16.0, true),
            Self::TitleSmall => (14.0, true),
            Self::BodyLarge => (16.0, false),
            Self::BodyMedium => (14.0, false),
            Self::BodySmall => (12.0, false),
            Self::LabelLarge => (14.0, true),
            Self::LabelMedium => (12.0, true),
            Self::LabelSmall => (11.0, true),
        }
    }

    pub fn text_style(self) -> TextStyle {
        TextStyle::Name(self.name().into())
    }

    pub fn font_id(self, scale: f32) -> FontId {
        let (size, medium) = self.spec();
        let family = if medium { FontFamily::Name(MEDIUM.into()) } else { FontFamily::Proportional };
        FontId::new(size * scale, family)
    }
}

/// User-facing theme settings (persisted by the app).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeSettings {
    pub seed: SeedSource,
    pub brightness: Brightness,
    pub variant: SchemeVariant,
    /// -1.0 (reduced) ..= 1.0 (high) — M3 contrast level.
    pub contrast: f64,
    /// Multiplies the M3 type scale (upstream `fontSize` setting, 16px = 1.0).
    pub text_scale: f32,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self { seed: SeedSource::System, brightness: Brightness::Dark, variant: SchemeVariant::TonalSpot, contrast: 0.0, text_scale: 1.0 }
    }
}

/// The resolved, active theme. Fetch it anywhere with [`theme`].
#[derive(Debug, Clone)]
pub struct M3Theme {
    pub settings: ThemeSettings,
    pub seed: Color32,
    pub colors: ColorScheme,
}

impl M3Theme {
    pub fn resolve(settings: ThemeSettings) -> Self {
        let sys = crate::system::detect();
        let seed = match settings.seed {
            SeedSource::Custom(c) => c,
            SeedSource::System => sys.accent.unwrap_or(DEFAULT_SEED),
        };
        let dark = match settings.brightness {
            Brightness::Dark => true,
            Brightness::Light => false,
            Brightness::System => sys.prefers_dark.unwrap_or(true),
        };
        let colors = ColorScheme::from_seed(seed, dark, settings.variant, settings.contrast);
        Self { settings, seed, colors }
    }

    pub fn font(&self, role: TypeRole) -> FontId {
        role.font_id(self.settings.text_scale)
    }
}

fn theme_id() -> Id {
    Id::unique("rpt-m3-theme")
}

/// The active theme (installed by [`install`]).
pub fn theme(ctx: &Context) -> Arc<M3Theme> {
    ctx.data(|d| d.get_temp::<Arc<M3Theme>>(theme_id()))
        .unwrap_or_else(|| Arc::new(M3Theme::resolve(ThemeSettings::default())))
}

/// Shorthand for `theme(ctx).colors`.
pub fn colors(ctx: &Context) -> ColorScheme {
    theme(ctx).colors
}

/// Install fonts once at startup. `extra_fallbacks` are system font files
/// (e.g. CJK) loaded by the app for non-Latin game languages.
pub fn install_fonts(ctx: &Context, extra_fallbacks: Vec<(String, Vec<u8>)>) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("roboto".into(), Arc::new(FontData::from_static(include_bytes!("../fonts/Roboto-Regular.ttf"))));
    fonts
        .font_data
        .insert(MEDIUM.into(), Arc::new(FontData::from_static(include_bytes!("../fonts/Roboto-Medium.ttf"))));
    let mut icon_data = FontData::from_static(include_bytes!("../fonts/MaterialIconsRound-Regular.otf"));
    // Icons sit a bit high relative to Roboto's baseline.
    icon_data.tweak.y_offset_factor = 0.08;
    fonts.font_data.insert(ICONS.into(), Arc::new(icon_data));

    let fallback_names: Vec<String> = extra_fallbacks
        .into_iter()
        .map(|(name, bytes)| {
            fonts.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
            name
        })
        .collect();

    let defaults_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = vec!["roboto".to_owned()];
    prop.extend(fallback_names.iter().cloned());
    prop.extend(defaults_prop.iter().cloned());
    prop.push(ICONS.into());
    fonts.families.insert(FontFamily::Proportional, prop);

    let mut med = vec![MEDIUM.to_owned()];
    med.extend(fallback_names.iter().cloned());
    med.extend(defaults_prop.iter().cloned());
    med.push(ICONS.into());
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), med);

    fonts.families.insert(FontFamily::Name(ICONS.into()), vec![ICONS.into()]);
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.extend(fallback_names);
    }
    ctx.set_fonts(fonts);
}

/// Apply (or re-apply) the theme to the context's style.
pub fn install(ctx: &Context, theme: M3Theme) {
    let t = Arc::new(theme);
    ctx.data_mut(|d| d.insert_temp(theme_id(), t.clone()));
    ctx.set_theme(if t.colors.dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|style| apply_style(style, &t));
}

fn wv(bg: Color32, weak_bg: Color32, stroke: Stroke, fg: Color32, radius: u8) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: weak_bg,
        bg_stroke: stroke,
        corner_radius: CornerRadius::same(radius),
        fg_stroke: Stroke::new(1.5, fg),
        expansion: 0.0,
    }
}

fn apply_style(style: &mut Style, t: &M3Theme) {
    let c = &t.colors;
    let scale = t.settings.text_scale;

    style.text_styles = [
        (TextStyle::Small, TypeRole::BodySmall.font_id(scale)),
        (TextStyle::Body, TypeRole::BodyMedium.font_id(scale)),
        (TextStyle::Button, TypeRole::LabelLarge.font_id(scale)),
        (TextStyle::Heading, TypeRole::TitleLarge.font_id(scale)),
        (TextStyle::Monospace, FontId::monospace(13.0 * scale)),
    ]
    .into();
    for role in TypeRole::ALL {
        style.text_styles.insert(role.text_style(), role.font_id(scale));
    }

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 8.0);
    s.button_padding = egui::vec2(16.0, 6.0);
    s.interact_size = egui::vec2(40.0, 32.0);
    s.window_margin = Margin::same(16);
    s.menu_margin = Margin::same(8);
    s.indent = 16.0;
    s.slider_width = 160.0;
    s.combo_width = 160.0;
    s.icon_width = 18.0;
    s.icon_spacing = 8.0;
    s.scroll.bar_width = 6.0;
    s.scroll.floating = true;

    let v: &mut Visuals = &mut style.visuals;
    v.dark_mode = c.dark;
    v.override_text_color = Some(c.on_surface);
    v.weak_text_color = Some(c.on_surface_variant);
    v.hyperlink_color = c.primary;
    v.faint_bg_color = c.surface_container_low;
    v.extreme_bg_color = c.surface_container_highest;
    v.text_edit_bg_color = Some(c.surface_container_highest);
    v.code_bg_color = c.surface_container_high;
    v.warn_fg_color = c.tertiary;
    v.error_fg_color = c.error;
    v.window_corner_radius = CornerRadius::same(shape::EXTRA_LARGE);
    v.window_fill = c.surface_container_high;
    v.window_stroke = Stroke::NONE;
    v.window_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: with_alpha(c.shadow, 0.35) };
    v.popup_shadow = Shadow { offset: [0, 2], blur: 8, spread: 0, color: with_alpha(c.shadow, 0.3) };
    v.menu_corner_radius = CornerRadius::same(shape::EXTRA_SMALL);
    v.panel_fill = c.surface;
    v.selection = Selection { bg_fill: with_alpha(c.primary, 0.35), stroke: Stroke::new(1.0, c.on_surface) };
    v.text_cursor.stroke = Stroke::new(2.0, c.primary);
    v.slider_trailing_fill = true;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.handle_shape = egui::style::HandleShape::Circle;

    let hover = state_layer(c.surface_container_highest, c.on_surface, 0.08);
    let press = state_layer(c.surface_container_highest, c.on_surface, 0.12);
    v.widgets = Widgets {
        noninteractive: wv(c.surface_container, c.surface_container, Stroke::new(1.0, c.outline_variant), c.on_surface, shape::SMALL),
        inactive: wv(c.surface_container_highest, c.secondary_container, Stroke::NONE, c.on_surface_variant, shape::SMALL),
        hovered: wv(hover, state_layer(c.secondary_container, c.on_secondary_container, 0.08), Stroke::NONE, c.on_surface, shape::SMALL),
        active: wv(press, state_layer(c.secondary_container, c.on_secondary_container, 0.12), Stroke::new(1.0, c.primary), c.on_surface, shape::SMALL),
        open: wv(c.secondary_container, c.secondary_container, Stroke::NONE, c.on_secondary_container, shape::SMALL),
    };
}
