//! Small helpers reproducing JavaScript number-parsing semantics used by the
//! upstream parser (`parseInt(s, 10)`, `parseFloat(s)`, `Number(s)`).
//! All of them return `NaN` where JS would.

fn trim_js_start(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// `parseInt(s, 10)`.
pub fn parse_int(s: &str) -> f64 {
    let s = trim_js_start(s);
    let (neg, rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let digits: &str = &rest[..rest.bytes().take_while(u8::is_ascii_digit).count()];
    if digits.is_empty() {
        return f64::NAN;
    }
    let v: f64 = digits.parse().unwrap_or(f64::NAN);
    if neg { -v } else { v }
}

/// `parseFloat(s)`.
pub fn parse_float(s: &str) -> f64 {
    let s = trim_js_start(s);
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'-' | b'+')) {
        i += 1;
    }
    if s[i..].starts_with("Infinity") {
        return if b.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut has_digits = i > int_start;
    if i < b.len() && b[i] == b'.' {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > frac_start || has_digits {
            has_digits = has_digits || j > frac_start;
            i = j;
        }
    }
    if !has_digits {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    s[..i].parse().unwrap_or(f64::NAN)
}

/// `Number(s)` for strings.
pub fn number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let lower = t.get(..2).map(str::to_ascii_lowercase);
    let radix = match lower.as_deref() {
        Some("0x") => 16,
        Some("0o") => 8,
        Some("0b") => 2,
        _ => 10,
    };
    if radix != 10 {
        return u64::from_str_radix(&t[2..], radix).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    // Rust accepts "inf"/"nan"/"infinity", JS does not.
    let valid = t.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !valid {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// Maps `NaN` to `None` (how this port represents a JS `NaN` field value).
pub fn non_nan(v: f64) -> Option<f64> {
    if v.is_nan() { None } else { Some(v) }
}

/// Converts a JS number to an unsigned integer field (`None` for NaN / negative / fractional / infinite).
pub fn to_u32(v: f64) -> Option<u32> {
    if v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v <= u32::MAX as f64 {
        Some(v as u32)
    } else {
        None
    }
}

/// `Math.round`.
pub fn math_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// UTF-16 length, i.e. JS `string.length`.
pub fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_numbers() {
        assert_eq!(parse_int("+20% (augmented)"), 20.0);
        assert_eq!(parse_int(" -5"), -5.0);
        assert!(parse_int("abc").is_nan());
        assert_eq!(parse_float("7.50%"), 7.5);
        assert_eq!(parse_float("1.20 (augmented)"), 1.2);
        assert_eq!(parse_float(".5"), 0.5);
        assert!(parse_float("x").is_nan());
        assert_eq!(number(" 84 "), 84.0);
        assert_eq!(number(""), 0.0);
        assert!(number("84a").is_nan());
        assert!(number("inf").is_nan());
        assert_eq!(number("0x10"), 16.0);
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(-2.5), -2.0);
    }
}
