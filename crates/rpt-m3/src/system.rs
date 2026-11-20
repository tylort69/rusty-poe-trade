//! OS integration for dynamic color: accent color + light/dark preference.
//!
//! - Windows 11: `HKCU\Software\Microsoft\Windows\DWM\AccentColor` (0xAABBGGRR) and
//!   `...\Themes\Personalize\AppsUseLightTheme`.
//! - Linux: KDE Plasma `kdeglobals` `[General] AccentColor=r,g,b`, GNOME 47+
//!   `org.gnome.desktop.interface accent-color`, and the freedesktop/GNOME
//!   `color-scheme` preference.

use egui::Color32;

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTheme {
    pub accent: Option<Color32>,
    /// `Some(true)` if the OS prefers dark mode.
    pub prefers_dark: Option<bool>,
}

pub fn detect() -> SystemTheme {
    let t = imp::detect();
    tracing::debug!(?t, "detected system theme");
    t
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    pub fn detect() -> SystemTheme {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let accent = hkcu
            .open_subkey("Software\\Microsoft\\Windows\\DWM")
            .and_then(|k| k.get_value::<u32, _>("AccentColor"))
            .ok()
            .map(|abgr| Color32::from_rgb((abgr & 0xFF) as u8, ((abgr >> 8) & 0xFF) as u8, ((abgr >> 16) & 0xFF) as u8));
        let prefers_dark = hkcu
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
            .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
            .ok()
            .map(|v| v == 0);
        SystemTheme { accent, prefers_dark }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use std::process::Command;

    pub fn detect() -> SystemTheme {
        let mut t = SystemTheme::default();
        if let Some((accent, dark)) = kde() {
            t.accent = accent;
            t.prefers_dark = dark;
        }
        if t.accent.is_none() {
            t.accent = gsettings("accent-color").and_then(|v| gnome_accent(&v));
        }
        if t.prefers_dark.is_none() {
            t.prefers_dark = gsettings("color-scheme").map(|v| v.contains("dark"));
        }
        t
    }

    fn gsettings(key: &str) -> Option<String> {
        let out = Command::new("gsettings").args(["get", "org.gnome.desktop.interface", key]).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_owned();
        (!s.is_empty()).then_some(s)
    }

    /// GNOME 47 named accents (libadwaita palette).
    fn gnome_accent(name: &str) -> Option<Color32> {
        Some(match name {
            "blue" => Color32::from_rgb(0x35, 0x84, 0xe4),
            "teal" => Color32::from_rgb(0x21, 0x90, 0xa4),
            "green" => Color32::from_rgb(0x3a, 0x94, 0x4a),
            "yellow" => Color32::from_rgb(0xc8, 0x88, 0x00),
            "orange" => Color32::from_rgb(0xed, 0x5b, 0x00),
            "red" => Color32::from_rgb(0xe6, 0x2d, 0x42),
            "pink" => Color32::from_rgb(0xd5, 0x61, 0x99),
            "purple" => Color32::from_rgb(0x91, 0x41, 0xac),
            "slate" => Color32::from_rgb(0x6f, 0x83, 0x96),
            _ => return None,
        })
    }

    fn kde() -> Option<(Option<Color32>, Option<bool>)> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
        let text = std::fs::read_to_string(base.join("kdeglobals")).ok()?;
        let mut section = "";
        let mut accent = None;
        let mut window_bg = None;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                section = line;
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            match (section, k) {
                ("[General]", "AccentColor") => accent = parse_rgb(v),
                ("[Colors:Window]", "BackgroundNormal") => window_bg = parse_rgb(v),
                _ => {}
            }
        }
        let dark = window_bg.map(|c| (c.r() as u32 + c.g() as u32 + c.b() as u32) < 384);
        Some((accent, dark))
    }

    fn parse_rgb(v: &str) -> Option<Color32> {
        let p: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (p.len() >= 3).then(|| Color32::from_rgb(p[0], p[1], p[2]))
    }
}
