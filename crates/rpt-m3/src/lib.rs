//! Material You (Material Design 3) for egui.
//!
//! - [`scheme`]: dynamic color — seed → full M3 role palette (via `material-colors`).
//! - [`system`]: reads the OS accent color and dark-mode preference (Windows 11, KDE, GNOME).
//! - [`theme`]: installs Roboto + Material Icons, the M3 type & shape scales, and egui visuals.
//! - [`components`]: buttons, icon buttons, FAB, chips, switch, checkbox, radio, cards,
//!   lists, navigation rail, segmented buttons, progress, banners, snackbar, text field.

pub mod components;
pub mod icons;
pub mod scheme;
pub mod system;
pub mod theme;

pub use components::*;
pub use scheme::{Brightness, ColorScheme, DEFAULT_SEED, SchemeVariant, SeedSource, state_layer, with_alpha};
pub use theme::{M3Theme, ThemeSettings, TypeRole, colors, install, install_fonts, shape, theme};
