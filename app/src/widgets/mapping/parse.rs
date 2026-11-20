//! Client.txt line parsing shared by the mapping and whisper widgets.
//!
//! A line looks like
//! `2024/01/01 12:00:00 123456789 cffb0719 [INFO Client 1234] : You have entered Strand.`
//! The leading timestamp is local game-client time. We keep it as a naive
//! date-time and convert it to "log seconds" (`and_utc().timestamp()`), so
//! re-reading the same log always yields the same numbers.

use std::sync::OnceLock;

use chrono::NaiveDateTime;
use regex::Regex;

/// Seconds since the epoch of the (naive, local) log timestamp.
pub type LogSecs = i64;

pub fn to_secs(t: NaiveDateTime) -> LogSecs {
    t.and_utc().timestamp()
}

/// Wall-clock "now" in the same naive-local scale as log timestamps.
pub fn now_secs() -> LogSecs {
    to_secs(chrono::Local::now().naive_local())
}

/// Leading `YYYY/MM/DD HH:MM:SS` of a Client.txt line.
pub fn log_timestamp(line: &str) -> Option<LogSecs> {
    let head = line.get(..19)?;
    NaiveDateTime::parse_from_str(head, "%Y/%m/%d %H:%M:%S").ok().map(to_secs)
}

/// Text after the `[LEVEL Client N] ` tag.
pub fn message(line: &str) -> Option<&str> {
    let i = line.find("] ")?;
    // the tag must be the bracketed `[... Client ...]` part, not something inside chat
    line[..i].contains('[').then(|| &line[i + 2..])
}

/// Events the widgets care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    /// `Generating level 83 area "MapWorldsStrand" with seed 1234`
    Generating { level: u32, area_id: String, seed: u64 },
    /// `: You have entered Strand.`
    Entered { area: String },
    /// `: <name> has been slain.`
    Slain { name: String },
    /// `: <name> has joined the area.`
    Joined { name: String },
    /// `: <name> has left the area.`
    Left { name: String },
    /// `: Trade accepted.`
    TradeAccepted,
    /// `: Trade cancelled.`
    TradeCancelled,
}

fn generating_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"^Generating level (\d+) area "([^"]+)" with seed (\d+)"#).unwrap())
}

/// Parse one raw Client.txt line. Returns the log timestamp and the event.
///
/// System messages are English only: upstream ships no translations of the
/// area / party strings, and these lines are written in the game client's language.
pub fn parse_line(line: &str) -> Option<(LogSecs, LogEvent)> {
    let ts = log_timestamp(line)?;
    let msg = message(line)?;
    if let Some(c) = generating_re().captures(msg) {
        return Some((
            ts,
            LogEvent::Generating {
                level: c[1].parse().ok()?,
                area_id: c[2].to_owned(),
                seed: c[3].parse().ok()?,
            },
        ));
    }
    let body = msg.strip_prefix(": ")?.trim_end();
    let ev = if let Some(a) = body.strip_prefix("You have entered ").and_then(|s| s.strip_suffix('.')) {
        LogEvent::Entered { area: a.to_owned() }
    } else if let Some(n) = body.strip_suffix(" has been slain.") {
        LogEvent::Slain { name: n.to_owned() }
    } else if let Some(n) = body.strip_suffix(" has joined the area.") {
        LogEvent::Joined { name: n.to_owned() }
    } else if let Some(n) = body.strip_suffix(" has left the area.") {
        LogEvent::Left { name: n.to_owned() }
    } else if body == "Trade accepted." {
        LogEvent::TradeAccepted
    } else if body == "Trade cancelled." {
        LogEvent::TradeCancelled
    } else {
        return None;
    };
    // Player names never contain spaces; this rejects e.g. "Your party has been slain."
    match &ev {
        LogEvent::Slain { name } | LogEvent::Joined { name } | LogEvent::Left { name } if name.contains(' ') || name.is_empty() => None,
        _ => Some((ts, ev)),
    }
}

// ---------------------------------------------------------------------------
// Area classification

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AreaKind {
    Map,
    Hideout,
    Town,
    Other,
}

/// English town / safe-zone names (campaign towns, Oriath, league hubs).
const TOWNS: &[&str] = &[
    "Lioneye's Watch",
    "The Forest Encampment",
    "The Sarn Encampment",
    "Highgate",
    "Overseer's Tower",
    "The Bridge Encampment",
    "Oriath Docks",
    "Oriath",
    "Karui Shores",
    "The Rogue Harbour",
    "Kingsmarch",
    "The Menagerie",
    "Aspirants' Plaza",
    "Azurite Mine",
];

/// Classify an area by its internal id (from `Generating …`) and/or its display name.
///
/// Heuristics (documented, intentionally simple):
/// - hideout: id starts with `Hideout`, or the name contains "Hideout";
/// - town: id ends with `_town` (campaign `1_1_town` … `2_10_town`) or is a known
///   hub (`HeistHub`, `Menagerie…`), or the name is in [`TOWNS`];
/// - map: id starts with `Map` (`MapWorlds…`, `MapAtlas…`, `MapUnique…`) but is not a
///   side area (`…SideArea…`, labyrinth trials); with no id, a level ≥ 68 non-town
///   non-hideout area also counts as a map.
pub fn classify(area_id: Option<&str>, name: &str, level: Option<u32>) -> AreaKind {
    let id = area_id.unwrap_or("");
    if id.starts_with("Hideout") || name.contains("Hideout") {
        return AreaKind::Hideout;
    }
    if id.ends_with("_town") || id == "HeistHub" || id.starts_with("Menagerie") || TOWNS.contains(&name) {
        return AreaKind::Town;
    }
    if !id.is_empty() {
        let side = id.contains("SideArea") || id.contains("Labyrinth") || id.contains("Trial");
        return if id.starts_with("Map") && !side { AreaKind::Map } else { AreaKind::Other };
    }
    if level.is_some_and(|l| l >= 68) { AreaKind::Map } else { AreaKind::Other }
}

/// Atlas tier from the area level (T1 = 68 … T16 = 83, T17 maps are level 84).
pub fn tier(level: u32) -> Option<u32> {
    (68..=90).contains(&level).then(|| (level - 67).min(17))
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "2024/01/01 12:00:00 123456789 cffb0719 [INFO Client 1234] ";

    #[test]
    fn timestamps() {
        let a = log_timestamp(&format!("{P}: x")).unwrap();
        let b = log_timestamp("2024/01/01 12:01:30 123456789 cffb0719 [INFO Client 1234] : x").unwrap();
        assert_eq!(b - a, 90);
        assert!(log_timestamp("garbage").is_none());
    }

    #[test]
    fn parses_every_line_type() {
        let g = "2024/01/01 12:00:00 123456789 1186a0e0 [DEBUG Client 1234] Generating level 83 area \"MapWorldsStrand\" with seed 3076395869";
        assert_eq!(parse_line(g).unwrap().1, LogEvent::Generating { level: 83, area_id: "MapWorldsStrand".into(), seed: 3076395869 });
        assert_eq!(parse_line(&format!("{P}: You have entered Strand.")).unwrap().1, LogEvent::Entered { area: "Strand".into() });
        assert_eq!(
            parse_line(&format!("{P}: You have entered Lioneye's Watch.")).unwrap().1,
            LogEvent::Entered { area: "Lioneye's Watch".into() }
        );
        assert_eq!(parse_line(&format!("{P}: MyChar_Name has been slain.")).unwrap().1, LogEvent::Slain { name: "MyChar_Name".into() });
        assert_eq!(parse_line(&format!("{P}: Buyer has joined the area.")).unwrap().1, LogEvent::Joined { name: "Buyer".into() });
        assert_eq!(parse_line(&format!("{P}: Buyer has left the area.")).unwrap().1, LogEvent::Left { name: "Buyer".into() });
        assert_eq!(parse_line(&format!("{P}: Trade accepted.")).unwrap().1, LogEvent::TradeAccepted);
        assert_eq!(parse_line(&format!("{P}: Trade cancelled.")).unwrap().1, LogEvent::TradeCancelled);
        // chat that merely contains the words is not a system message
        assert!(parse_line(&format!("{P}#Spammer: Trade accepted.")).is_none());
        assert!(parse_line(&format!("{P}@From Joker: You have entered Strand.")).is_none());
        assert!(parse_line(&format!("{P}: Your whole party has been slain.")).is_none());
        assert!(parse_line(&format!("{P}: AFK mode is now ON.")).is_none());
    }

    #[test]
    fn area_classification() {
        assert_eq!(classify(Some("MapWorldsStrand"), "Strand", Some(83)), AreaKind::Map);
        assert_eq!(classify(Some("MapAtlasUniqueX"), "X", Some(78)), AreaKind::Map);
        assert_eq!(classify(Some("HideoutFelled"), "Felled Hideout", Some(1)), AreaKind::Hideout);
        assert_eq!(classify(None, "Celestial Hideout", None), AreaKind::Hideout);
        assert_eq!(classify(Some("1_1_town"), "Lioneye's Watch", Some(1)), AreaKind::Town);
        assert_eq!(classify(None, "Karui Shores", None), AreaKind::Town);
        assert_eq!(classify(Some("1_1_2"), "The Coast", Some(2)), AreaKind::Other);
        assert_eq!(classify(Some("MapSideArea_Vaal"), "Vaal Temple", Some(83)), AreaKind::Other);
        assert_eq!(tier(83), Some(16));
        assert_eq!(tier(68), Some(1));
        assert_eq!(tier(45), None);
    }
}
