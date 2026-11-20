//! Material You dynamic color: seed color → full M3 color scheme.

use egui::Color32;
use material_colors::{color::Rgb, dynamic_color::Variant, theme::ThemeBuilder};

/// How the seed color is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeedSource {
    /// Use the OS accent color (Windows 11 accent, KDE/GNOME accent). Falls back to [`DEFAULT_SEED`].
    #[default]
    System,
    /// A user-chosen seed color.
    Custom(Color32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Brightness {
    #[default]
    System,
    Light,
    Dark,
}

/// M3 scheme variants exposed in settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SchemeVariant {
    #[default]
    TonalSpot,
    Vibrant,
    Expressive,
    Neutral,
    Fidelity,
    Content,
    Monochrome,
}

impl SchemeVariant {
    pub const ALL: &'static [SchemeVariant] = &[
        Self::TonalSpot,
        Self::Vibrant,
        Self::Expressive,
        Self::Neutral,
        Self::Fidelity,
        Self::Content,
        Self::Monochrome,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::TonalSpot => "Tonal spot",
            Self::Vibrant => "Vibrant",
            Self::Expressive => "Expressive",
            Self::Neutral => "Neutral",
            Self::Fidelity => "Fidelity",
            Self::Content => "Content",
            Self::Monochrome => "Monochrome",
        }
    }
    fn to_mc(self) -> Variant {
        match self {
            Self::TonalSpot => Variant::TonalSpot,
            Self::Vibrant => Variant::Vibrant,
            Self::Expressive => Variant::Expressive,
            Self::Neutral => Variant::Neutral,
            Self::Fidelity => Variant::Fidelity,
            Self::Content => Variant::Content,
            Self::Monochrome => Variant::Monochrome,
        }
    }
}

/// Exalted-orb gold — the fallback seed when the OS exposes no accent color.
pub const DEFAULT_SEED: Color32 = Color32::from_rgb(0xC0, 0x8A, 0x3E);

/// All M3 color roles as egui colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorScheme {
    pub dark: bool,
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub inverse_primary: Color32,
    pub secondary: Color32,
    pub on_secondary: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub tertiary: Color32,
    pub on_tertiary: Color32,
    pub tertiary_container: Color32,
    pub on_tertiary_container: Color32,
    pub error: Color32,
    pub on_error: Color32,
    pub error_container: Color32,
    pub on_error_container: Color32,
    pub surface_dim: Color32,
    pub surface: Color32,
    pub surface_bright: Color32,
    pub surface_container_lowest: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
    pub inverse_surface: Color32,
    pub inverse_on_surface: Color32,
    pub surface_tint: Color32,
    pub shadow: Color32,
    pub scrim: Color32,
    /// Extended (custom) colors used for PoE price semantics.
    pub success: Color32,
    pub on_success_container: Color32,
    pub success_container: Color32,
}

fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.red, rgb.green, rgb.blue)
}

impl ColorScheme {
    pub fn from_seed(seed: Color32, dark: bool, variant: SchemeVariant, contrast: f64) -> Self {
        let theme = ThemeBuilder::with_source(Rgb::new(seed.r(), seed.g(), seed.b()))
            .variant(variant.to_mc())
            .contrast_level(contrast)
            .build();
        let s = if dark { &theme.schemes.dark } else { &theme.schemes.light };

        // Harmonized "success" green for positive price deltas / online sellers.
        let green = ThemeBuilder::with_source(Rgb::from_u32(0x3FA34D))
            .variant(Variant::TonalSpot)
            .build();
        let g = if dark { &green.schemes.dark } else { &green.schemes.light };

        Self {
            dark,
            primary: c(s.primary),
            on_primary: c(s.on_primary),
            primary_container: c(s.primary_container),
            on_primary_container: c(s.on_primary_container),
            inverse_primary: c(s.inverse_primary),
            secondary: c(s.secondary),
            on_secondary: c(s.on_secondary),
            secondary_container: c(s.secondary_container),
            on_secondary_container: c(s.on_secondary_container),
            tertiary: c(s.tertiary),
            on_tertiary: c(s.on_tertiary),
            tertiary_container: c(s.tertiary_container),
            on_tertiary_container: c(s.on_tertiary_container),
            error: c(s.error),
            on_error: c(s.on_error),
            error_container: c(s.error_container),
            on_error_container: c(s.on_error_container),
            surface_dim: c(s.surface_dim),
            surface: c(s.surface),
            surface_bright: c(s.surface_bright),
            surface_container_lowest: c(s.surface_container_lowest),
            surface_container_low: c(s.surface_container_low),
            surface_container: c(s.surface_container),
            surface_container_high: c(s.surface_container_high),
            surface_container_highest: c(s.surface_container_highest),
            on_surface: c(s.on_surface),
            on_surface_variant: c(s.on_surface_variant),
            outline: c(s.outline),
            outline_variant: c(s.outline_variant),
            inverse_surface: c(s.inverse_surface),
            inverse_on_surface: c(s.inverse_on_surface),
            surface_tint: c(s.surface_tint),
            shadow: c(s.shadow),
            scrim: c(s.scrim),
            success: c(g.primary),
            success_container: c(g.primary_container),
            on_success_container: c(g.on_primary_container),
        }
    }
}

/// Blend `overlay` over `base` with the given opacity (M3 state layers).
pub fn state_layer(base: Color32, overlay: Color32, opacity: f32) -> Color32 {
    let a = opacity.clamp(0.0, 1.0);
    let mix = |b: u8, o: u8| (b as f32 + (o as f32 - b as f32) * a).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(base.r(), overlay.r()),
        mix(base.g(), overlay.g()),
        mix(base.b(), overlay.b()),
        base.a().max((overlay.a() as f32 * a) as u8),
    )
}

/// `color` at `opacity` (unmultiplied alpha).
pub fn with_alpha(color: Color32, opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (opacity.clamp(0.0, 1.0) * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_scheme_has_dark_surface() {
        let s = ColorScheme::from_seed(DEFAULT_SEED, true, SchemeVariant::TonalSpot, 0.0);
        assert!(s.surface.r() < 40 && s.on_surface.r() > 200);
        let l = ColorScheme::from_seed(DEFAULT_SEED, false, SchemeVariant::TonalSpot, 0.0);
        assert!(l.surface.r() > 200);
    }
}
