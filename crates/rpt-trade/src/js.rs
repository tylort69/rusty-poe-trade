//! Small helpers reproducing JavaScript number/string semantics that upstream
//! output depends on (JSON numbers, `Math.round`, `toFixed`, `encodeURIComponent`,
//! luxon relative dates).

use serde::{Deserialize, Serialize, Serializer};

/// A number that serializes like `JSON.stringify` would: integral values
/// without a fractional part (`5` instead of `5.0`).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Deserialize)]
#[serde(transparent)]
pub struct JsNum(pub f64);

impl Serialize for JsNum {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        if v.fract() == 0.0 && v.abs() < 9_007_199_254_740_992.0 {
            s.serialize_i64(v as i64)
        } else {
            s.serialize_f64(v)
        }
    }
}

impl From<f64> for JsNum {
    fn from(v: f64) -> Self {
        JsNum(v)
    }
}

/// `serde_json::Value` for a number, using JS integer formatting.
pub fn json_num(v: f64) -> serde_json::Value {
    serde_json::to_value(JsNum(v)).unwrap_or(serde_json::Value::Null)
}

/// JS `Math.round` (half rounds towards +Infinity).
pub fn math_round(v: f64) -> f64 {
    let f = v.floor();
    if v - f >= 0.5 { f + 1.0 } else { f }
}

/// JS `Number.prototype.toString` for finite numbers (common cases).
pub fn num_to_string(v: f64) -> String {
    if v == 0.0 {
        return "0".to_owned();
    }
    if v.is_nan() {
        return "NaN".to_owned();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    // Rust's `Display` for f64 prints the shortest round-trip representation,
    // same as JS for the magnitudes we deal with.
    format!("{v}")
}

/// JS `value.toFixed(digits)` (ties resolved to the larger candidate).
pub fn to_fixed(v: f64, digits: u32) -> String {
    // Rust formatting rounds the exact binary value correctly but resolves exact
    // ties to even; JS picks the larger candidate.
    let exact = format!("{:.80}", v);
    let tie = exact
        .split_once('.')
        .map(|(_, frac)| {
            let rest = &frac[digits as usize..];
            rest.starts_with('5') && rest[1..].bytes().all(|b| b == b'0')
        })
        .unwrap_or(false);
    let s = if tie {
        let p = 10f64.powi(digits as i32);
        format!("{:.*}", digits as usize, (v * p).ceil() / p)
    } else {
        format!("{:.*}", digits as usize, v)
    };
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
        s[1..].to_owned()
    } else {
        s
    }
}

/// JS `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let unreserved = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')');
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// luxon `DateTime.fromISO(iso).toRelative({ style: 'short' })` in English.
/// Returns an empty string when `iso` cannot be parsed (upstream `?? ''`).
pub fn relative_date_short(iso: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(date) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return String::new();
    };
    let date = date.with_timezone(&chrono::Utc);
    relative_from_millis((date - now).num_milliseconds())
}

fn relative_from_millis(diff_ms: i64) -> String {
    // luxon default units for toRelative: years, months, days, hours, minutes, seconds
    // (months/years computed calendar-wise there; we approximate with fixed lengths).
    const UNITS: &[(i64, &str, &str)] = &[
        (365 * 24 * 3600 * 1000, "yr.", "yr."),
        (30 * 24 * 3600 * 1000, "mo.", "mo."),
        (24 * 3600 * 1000, "day", "days"),
        (3600 * 1000, "hr.", "hr."),
        (60 * 1000, "min.", "min."),
        (1000, "sec.", "sec."),
    ];
    let past = diff_ms <= 0;
    let abs = diff_ms.unsigned_abs() as i64;
    let (count, one, many) = UNITS
        .iter()
        .find(|(ms, _, _)| abs >= *ms)
        .map(|(ms, one, many)| (abs / ms, *one, *many))
        .unwrap_or((0, "sec.", "sec."));
    let unit = if count == 1 { one } else { many };
    if past {
        format!("{count} {unit} ago")
    } else {
        format!("in {count} {unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(serde_json::to_string(&JsNum(5.0)).unwrap(), "5");
        assert_eq!(serde_json::to_string(&JsNum(5.5)).unwrap(), "5.5");
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(to_fixed(0.25, 1), "0.3");
        assert_eq!(to_fixed(1.04, 1), "1.0");
        assert_eq!(num_to_string(12.0), "12");
        assert_eq!(num_to_string(1.5), "1.5");
        assert_eq!(encode_uri_component("a b/{\"x\"}"), "a%20b%2F%7B%22x%22%7D");
    }

    #[test]
    fn relative() {
        assert_eq!(relative_from_millis(-5 * 60 * 1000 - 10), "5 min. ago");
        assert_eq!(relative_from_millis(-26 * 3600 * 1000), "1 day ago");
        assert_eq!(relative_from_millis(-3 * 24 * 3600 * 1000), "3 days ago");
        assert_eq!(relative_from_millis(0), "0 sec. ago");
    }
}
