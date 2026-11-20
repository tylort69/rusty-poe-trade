//! Bulk-sell outputs: trade-chat "WTS" lines and in-game price notes.

use rpt_trade::prices::{PriceCurrency, auto_currency, display_rounding};

/// Conservative per-message length for PoE chat.
pub const CHAT_LINE_MAX: usize = 255;

#[derive(Debug, Clone, PartialEq)]
pub struct BulkEntry {
    pub name: String,
    pub qty: u32,
    /// Asking price per unit, already scaled by the chosen % of poe.ninja.
    pub unit_chaos: f64,
}

/// `display_rounding` without the hair spaces used for on-screen numbers.
fn plain(value: f64, fraction: bool) -> String {
    display_rounding(value, fraction).replace('\u{200A}', "")
}

/// Short chat price: `180c`, `1.5div`.
pub fn chat_price(chaos: f64, divine_chaos: Option<f64>) -> String {
    let v = auto_currency(divine_chaos, chaos);
    match v.currency {
        PriceCurrency::Div => format!("{}div", plain(v.min, false)),
        PriceCurrency::Chaos => format!("{}c", plain(v.min, true)),
    }
}

/// Split entries into chat messages of at most `max` characters, each starting
/// with `WTS <league>: ` and listing `12x Divine Orb 180c ea` items.
pub fn wts_lines(league: &str, entries: &[BulkEntry], divine_chaos: Option<f64>, max: usize) -> Vec<String> {
    let prefix = format!("WTS {league}: ");
    let mut lines = Vec::new();
    let mut cur = String::new();
    for e in entries {
        let part = if e.qty > 1 {
            format!("{}x {} {} ea", e.qty, e.name, chat_price(e.unit_chaos, divine_chaos))
        } else {
            format!("{} {}", e.name, chat_price(e.unit_chaos, divine_chaos))
        };
        if !cur.is_empty() && cur.chars().count() + 2 + part.chars().count() > max {
            lines.push(std::mem::take(&mut cur));
        }
        if cur.is_empty() {
            cur = format!("{prefix}{part}");
        } else {
            cur.push_str(", ");
            cur.push_str(&part);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// In-game premium-tab note, e.g. `~price 162 chaos` / `~b/o 1.5 divine`.
/// Uses divines from ~1 div up (same threshold as the price check).
pub fn price_note(unit_chaos: f64, divine_chaos: Option<f64>, buyout: bool) -> String {
    let kw = if buyout { "~b/o" } else { "~price" };
    let v = auto_currency(divine_chaos, unit_chaos);
    match v.currency {
        PriceCurrency::Div => format!("{kw} {} divine", plain(v.min, false)),
        PriceCurrency::Chaos => format!("{kw} {} chaos", plain(v.min, true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, qty: u32, unit: f64) -> BulkEntry {
        BulkEntry { name: name.into(), qty, unit_chaos: unit }
    }

    #[test]
    fn splits_under_chat_limit() {
        let entries: Vec<BulkEntry> = (0..30).map(|i| e(&format!("Gilded Scarab No{i}"), 12, 25.0)).collect();
        let lines = wts_lines("Standard", &entries, Some(200.0), CHAT_LINE_MAX);
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(l.starts_with("WTS Standard: "), "{l}");
            assert!(l.chars().count() <= CHAT_LINE_MAX, "{} chars", l.len());
        }
        // nothing lost or duplicated
        let joined = lines.iter().map(|l| l.trim_start_matches("WTS Standard: ")).collect::<Vec<_>>().join(", ");
        assert_eq!(joined.matches("12x Gilded Scarab No").count(), 30);
        assert!(joined.contains("12x Gilded Scarab No7 25c ea"));
    }

    #[test]
    fn single_line_format() {
        let lines = wts_lines("Settlers", &[e("Divine Orb", 12, 180.0), e("Mageblood", 1, 30_000.0), e("Orb of Alteration", 300, 0.25)], Some(200.0), CHAT_LINE_MAX);
        assert_eq!(lines, vec!["WTS Settlers: 12x Divine Orb 180c ea, Mageblood 150div, 300x Orb of Alteration 1/4c ea"]);
        assert!(wts_lines("Standard", &[], None, CHAT_LINE_MAX).is_empty());
    }

    #[test]
    fn note_formatting() {
        let rate = Some(200.0);
        assert_eq!(price_note(162.0, rate, false), "~price 162 chaos");
        assert_eq!(price_note(300.0, rate, true), "~b/o 1.5 divine");
        assert_eq!(price_note(195.0, rate, false), "~price 1 divine");
        assert_eq!(price_note(4.44, rate, false), "~price 4.4 chaos");
        assert_eq!(price_note(0.5, rate, false), "~price 1/2 chaos");
        assert_eq!(price_note(2400.0, rate, true), "~b/o 12 divine");
        // without an exchange rate everything stays in chaos
        assert_eq!(price_note(2400.0, None, false), "~price 2400 chaos");
    }
}
