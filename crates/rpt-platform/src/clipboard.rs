//! Port of `shortcuts/HostClipboard.ts`.
//!
//! One `arboard::Clipboard` lives for the whole process (on X11 arboard serves
//! the selection from a background thread; dropping the last instance would
//! lose data we own). The X11 backend is used on Linux even in a Wayland
//! session on purpose: the game is an XWayland client and reads the X11
//! CLIPBOARD selection.

use std::time::{Duration, Instant};

pub(crate) const POLL_DELAY: Duration = Duration::from_millis(48);
pub(crate) const POLL_LIMIT: Duration = Duration::from_millis(500);
/// PoE must read the clipboard within this timeframe, after that we restore it.
pub(crate) const RESTORE_AFTER: Duration = Duration::from_millis(120);

const LANGUAGE_DETECTOR: &[(&str, &str)] = &[
    ("en", "Item Class: "),
    ("ru", "Класс предмета: "),
    ("fr", "Classe d'objet: "),
    ("de", "Gegenstandsklasse: "),
    ("pt", "Classe do Item: "),
    ("es", "Clase de objeto: "),
    ("th", "ชนิดไอเทม: "),
    ("ko", "아이템 종류: "),
    ("cmn-Hant", "物品種類: "),
    ("cmn-Hans", "物品类别: "),
];

/// Returns the detected language if `text` is a PoE item description.
pub fn is_poe_item(text: &str) -> Option<&'static str> {
    LANGUAGE_DETECTOR.iter().find(|(_, first)| text.starts_with(first)).map(|(l, _)| *l)
}

/// Placeholder written on Linux instead of an empty string (KDE "prevent empty
/// clipboard" and Proton 10+ workarounds from upstream).
pub(crate) fn force_empty_marker() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("__APT_FORCE_EMPTY_{ms}")
}

pub(crate) struct HostClipboard {
    cb: Option<arboard::Clipboard>,
    pub should_restore: bool,
    /// `isRestored == false` while a `restoreShortly` window is open.
    restore_pending: Option<(Instant, String)>,
}

impl HostClipboard {
    pub fn new() -> (Self, Option<String>) {
        match arboard::Clipboard::new() {
            Ok(cb) => (Self { cb: Some(cb), should_restore: false, restore_pending: None }, None),
            Err(e) => (
                Self { cb: None, should_restore: false, restore_pending: None },
                Some(format!("Clipboard unavailable: {e}")),
            ),
        }
    }

    pub fn read(&mut self) -> String {
        self.cb.as_mut().and_then(|c| c.get_text().ok()).unwrap_or_default()
    }

    pub fn write(&mut self, text: &str) {
        if let Some(c) = self.cb.as_mut()
            && let Err(e) = c.set_text(text.to_owned()) {
                tracing::warn!("clipboard write failed: {e}");
            }
    }

    /// First half of `readItemText`: clears the clipboard so a stale item is
    /// not mistaken for the new one. Returns the text to restore later.
    pub fn prepare_item_read(&mut self) -> String {
        let mut before = self.read();
        if is_poe_item(&before).is_some() {
            before.clear();
            if cfg!(target_os = "linux") {
                self.write(&force_empty_marker());
            } else {
                self.write("");
            }
        } else if cfg!(target_os = "linux") {
            self.write(&force_empty_marker());
        }
        before
    }

    /// Second half of `readItemText`: polls until an item appears or the
    /// timeout passes (blocking, at most ~500ms).
    pub fn poll_item(&mut self, text_before: &str) -> Option<String> {
        let start = Instant::now();
        loop {
            std::thread::sleep(POLL_DELAY);
            let after = self.read();
            if is_poe_item(&after).is_some() {
                if self.should_restore {
                    self.write(text_before);
                }
                return Some(after);
            }
            if start.elapsed() >= POLL_LIMIT {
                if self.should_restore {
                    self.write(text_before);
                }
                return None;
            }
        }
    }

    /// `restoreShortly`: returns `false` (and the callback must not run) while
    /// a previous restore window is still open — this doubles as throttling
    /// against "Too many actions" disconnects.
    pub fn begin_restore_shortly(&mut self) -> bool {
        if self.restore_pending.is_some() {
            return false;
        }
        let saved = self.read();
        self.restore_pending = Some((Instant::now() + RESTORE_AFTER, saved));
        true
    }

    pub fn restore_deadline(&self) -> Option<Instant> {
        self.restore_pending.as_ref().map(|(t, _)| *t)
    }

    pub fn on_timer(&mut self, now: Instant) {
        if let Some((t, _)) = &self.restore_pending
            && *t <= now {
                let (_, saved) = self.restore_pending.take().unwrap();
                if self.should_restore {
                    self.write(&saved);
                }
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_items() {
        assert_eq!(is_poe_item("Item Class: Rings\nRarity: Rare"), Some("en"));
        assert_eq!(is_poe_item("Класс предмета: Кольца"), Some("ru"));
        assert_eq!(is_poe_item("물품 아이템 종류: "), None);
        assert_eq!(is_poe_item("Rarity: Rare"), None);
        assert!(force_empty_marker().starts_with("__APT_FORCE_EMPTY_"));
    }
}
