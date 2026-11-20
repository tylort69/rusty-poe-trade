//! Map-run and session tracking from Client.txt events.
//!
//! Model:
//! - A **zone segment** runs from one `You have entered …` to the next. Its
//!   duration is added to the session's map / hideout / town / other time.
//! - A **map run** is one map instance, identified by the `Generating … seed`
//!   line. Entering the same instance again (no new `Generating` line, same
//!   area name) counts as a re-entry (portal). The run is completed when a
//!   different map instance is generated, the session goes idle, or the
//!   session is reset. Its duration is the time spent *inside* the map
//!   (hideout trips in between are not counted).
//! - A **session** ends after `idle_secs` without any timestamped log line,
//!   or on "Reset session". Time inside the idle gap is not counted.
//!
//! All times are log timestamps ([`LogSecs`]), never the wall clock, except
//! for the live "now" passed in by the UI.

use serde::{Deserialize, Serialize};

use super::parse::{AreaKind, LogEvent, LogSecs, classify, parse_line, log_timestamp};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MapRun {
    /// Id (start time) of the session this run belongs to.
    pub session: LogSecs,
    pub area_id: String,
    pub name: String,
    pub level: u32,
    pub seed: u64,
    pub started: LogSecs,
    /// Last moment we were inside the map.
    pub ended: LogSecs,
    /// Time spent inside the map instance.
    pub map_secs: i64,
    pub deaths: u32,
    /// Times the map was entered (first entry + portals).
    pub entries: u32,
}

impl MapRun {
    pub fn portals(&self) -> u32 {
        self.entries.saturating_sub(1)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionTotals {
    /// Session start (log time); also the session id.
    pub id: LogSecs,
    /// Last log activity.
    pub last: LogSecs,
    pub maps: u32,
    pub map_secs: i64,
    pub hideout_secs: i64,
    pub town_secs: i64,
    pub other_secs: i64,
    pub deaths: u32,
}

impl SessionTotals {
    pub fn duration(&self) -> i64 {
        (self.last - self.id).max(0)
    }
    fn add(&mut self, kind: AreaKind, d: i64) {
        match kind {
            AreaKind::Map => self.map_secs += d,
            AreaKind::Hideout => self.hideout_secs += d,
            AreaKind::Town => self.town_secs += d,
            AreaKind::Other => self.other_secs += d,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub kind: AreaKind,
    pub name: String,
    pub since: LogSecs,
}

/// What the tracker wants persisted.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    RunFinished(MapRun),
    /// Latest snapshot of a session (append-only; last one wins).
    Session(SessionTotals),
}

#[derive(Debug, Clone)]
pub struct Tracker {
    pub session: SessionTotals,
    /// Completed runs of the current session, oldest first.
    pub runs: Vec<MapRun>,
    pub current: Option<MapRun>,
    pub zone: Option<Zone>,
    pending: Option<(u32, String, u64)>,
    last_event: Option<LogSecs>,
    pub idle_secs: i64,
    /// Character whose deaths count. Empty → every `has been slain` counts.
    pub char_name: String,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new(30 * 60)
    }
}

impl Tracker {
    pub fn new(idle_secs: i64) -> Self {
        Self {
            session: SessionTotals::default(),
            runs: Vec::new(),
            current: None,
            zone: None,
            pending: None,
            last_event: None,
            idle_secs,
            char_name: String::new(),
        }
    }

    /// Continue a persisted session (after an app restart).
    pub fn restore(&mut self, session: SessionTotals, runs: Vec<MapRun>) {
        self.last_event = Some(session.last);
        self.session = session;
        self.runs = runs;
    }

    pub fn started(&self) -> bool {
        self.session.id != 0
    }

    /// Feed one raw Client.txt line.
    pub fn feed(&mut self, line: &str) -> Vec<Output> {
        match parse_line(line) {
            Some((ts, ev)) => self.apply(ts, Some(ev)),
            None => match log_timestamp(line) {
                Some(ts) => self.apply(ts, None),
                None => Vec::new(),
            },
        }
    }

    /// Apply an event at log time `ts`. `None` = activity only.
    pub fn apply(&mut self, ts: LogSecs, ev: Option<LogEvent>) -> Vec<Output> {
        let mut out = Vec::new();
        if let Some(last) = self.last_event {
            if ts - last > self.idle_secs {
                // idle: close everything at the last activity, start fresh at `ts`
                self.end_session(last, &mut out);
                if let Some(z) = self.zone.as_mut() {
                    z.since = ts;
                }
            }
        }
        if !self.started() {
            self.session.id = ts;
        }
        self.last_event = Some(ts.max(self.last_event.unwrap_or(ts)));
        self.session.last = self.session.last.max(ts);

        match ev {
            None | Some(LogEvent::Joined { .. } | LogEvent::Left { .. } | LogEvent::TradeAccepted | LogEvent::TradeCancelled) => {}
            Some(LogEvent::Generating { level, area_id, seed }) => {
                if classify(Some(&area_id), "", Some(level)) == AreaKind::Map
                    && self.current.as_ref().is_none_or(|c| c.seed != seed)
                {
                    self.finish_run(&mut out);
                    self.current = Some(MapRun {
                        session: self.session.id,
                        area_id: area_id.clone(),
                        level,
                        seed,
                        started: ts,
                        ended: ts,
                        ..Default::default()
                    });
                }
                self.pending = Some((level, area_id, seed));
            }
            Some(LogEvent::Entered { area }) => {
                self.close_segment(ts);
                let kind = match self.pending.take() {
                    Some((level, id, seed)) => {
                        let kind = classify(Some(&id), &area, Some(level));
                        if kind == AreaKind::Map {
                            if let Some(c) = self.current.as_mut().filter(|c| c.seed == seed) {
                                c.entries += 1;
                                c.name = area.clone();
                            }
                        }
                        kind
                    }
                    None => match self.current.as_mut() {
                        Some(c) if c.name == area => {
                            c.entries += 1;
                            AreaKind::Map
                        }
                        _ => classify(None, &area, None),
                    },
                };
                self.zone = Some(Zone { kind, name: area, since: ts });
            }
            Some(LogEvent::Slain { name }) => {
                if self.char_name.trim().is_empty() || name == self.char_name.trim() {
                    self.session.deaths += 1;
                    if self.in_map() {
                        if let Some(c) = self.current.as_mut() {
                            c.deaths += 1;
                        }
                    }
                }
            }
        }
        out
    }

    pub fn in_map(&self) -> bool {
        self.zone.as_ref().is_some_and(|z| z.kind == AreaKind::Map) && self.current.is_some()
    }

    fn close_segment(&mut self, ts: LogSecs) {
        let in_map = self.in_map();
        if let Some(z) = self.zone.as_mut() {
            let d = (ts - z.since).max(0);
            self.session.add(z.kind, d);
            if in_map {
                if let Some(c) = self.current.as_mut() {
                    c.map_secs += d;
                    c.ended = ts;
                }
            }
            z.since = ts;
        }
    }

    fn finish_run(&mut self, out: &mut Vec<Output>) {
        if let Some(run) = self.current.take() {
            if run.entries > 0 {
                self.session.maps += 1;
                self.runs.push(run.clone());
                out.push(Output::RunFinished(run));
                out.push(Output::Session(self.session.clone()));
            }
        }
    }

    fn end_session(&mut self, at: LogSecs, out: &mut Vec<Output>) {
        self.close_segment(at);
        self.finish_run(out);
        if self.started() && (self.session.maps > 0 || self.session.map_secs > 0) {
            out.push(Output::Session(self.session.clone()));
        }
        self.session = SessionTotals::default();
        self.runs.clear();
        self.pending = None;
        // a map left open across the reset no longer counts as "inside a run"
        if let Some(z) = self.zone.as_mut().filter(|z| z.kind == AreaKind::Map) {
            z.kind = AreaKind::Other;
        }
    }

    /// "Reset session" button: close the session at `now` and start a new one.
    pub fn reset_session(&mut self, now: LogSecs) -> Vec<Output> {
        let mut out = Vec::new();
        let at = self.effective_now(now);
        self.end_session(at, &mut out);
        self.session.id = at;
        self.session.last = at;
        self.last_event = Some(at);
        if let Some(z) = self.zone.as_mut() {
            z.since = at;
        }
        out
    }

    /// `now`, but not past the idle cut-off (an idle session stops counting).
    pub fn effective_now(&self, now: LogSecs) -> LogSecs {
        match self.last_event {
            Some(last) => now.clamp(last, last + self.idle_secs),
            None => now,
        }
    }

    /// Session totals including the still-open zone segment.
    pub fn live_totals(&self, now: LogSecs) -> SessionTotals {
        let mut t = self.session.clone();
        let now = self.effective_now(now);
        if let Some(z) = &self.zone {
            t.add(z.kind, (now - z.since).max(0));
        }
        t.last = t.last.max(now);
        t
    }

    /// Time inside the current map, including the open segment.
    pub fn current_elapsed(&self, now: LogSecs) -> i64 {
        let Some(c) = &self.current else { return 0 };
        let open = match &self.zone {
            Some(z) if self.in_map() => (self.effective_now(now) - z.since).max(0),
            _ => 0,
        };
        c.map_secs + open
    }
}

/// Aggregates shown in the session panel.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionStats {
    pub maps: u32,
    pub maps_per_hour: f64,
    pub avg_map_secs: i64,
    pub map_secs: i64,
    pub hideout_secs: i64,
    pub town_secs: i64,
    pub other_secs: i64,
    pub deaths: u32,
    pub duration: i64,
}

pub fn stats(t: &Tracker, now: LogSecs) -> SessionStats {
    let live = t.live_totals(now);
    let maps = t.runs.len() as u32;
    let duration = live.duration();
    let avg = if maps > 0 { t.runs.iter().map(|r| r.map_secs).sum::<i64>() / maps as i64 } else { 0 };
    SessionStats {
        maps,
        maps_per_hour: if duration >= 60 { maps as f64 * 3600.0 / duration as f64 } else { 0.0 },
        avg_map_secs: avg,
        map_secs: live.map_secs,
        hideout_secs: live.hideout_secs,
        town_secs: live.town_secs,
        other_secs: live.other_secs,
        deaths: live.deaths,
        duration,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a Client.txt line at `12:00:00 + secs`.
    fn l(secs: i64, msg: &str) -> String {
        let t = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap().and_hms_opt(12, 0, 0).unwrap() + chrono::Duration::seconds(secs);
        format!("{} 123456789 cffb0719 [INFO Client 1234] {msg}", t.format("%Y/%m/%d %H:%M:%S"))
    }
    fn gen_line(secs: i64, id: &str, seed: u64) -> String {
        l(secs, &format!("Generating level 83 area \"{id}\" with seed {seed}"))
    }

    fn feed(t: &mut Tracker, lines: &[String]) -> Vec<Output> {
        lines.iter().flat_map(|x| t.feed(x)).collect()
    }

    #[test]
    fn aggregates_a_session() {
        let mut t = Tracker::new(30 * 60);
        t.char_name = "Me".into();
        let out = feed(
            &mut t,
            &[
                gen_line(0, "HideoutFelled", 1),
                l(0, ": You have entered Felled Hideout."),
                // map 1: 5 min inside, one portal back to hideout (60 s), one death
                gen_line(60, "MapWorldsStrand", 100),
                l(60, ": You have entered Strand."),
                l(120, ": Me has been slain."),
                l(121, ": Someone has been slain."),
                gen_line(180, "HideoutFelled", 2),
                l(180, ": You have entered Felled Hideout."),
                l(240, ": You have entered Strand."),
                l(480, ": You have entered Felled Hideout."),
                // map 2: 4 min
                gen_line(540, "MapWorldsBeach", 200),
                l(540, ": You have entered Beach."),
                l(780, ": You have entered Felled Hideout."),
            ],
        );
        let runs: Vec<_> = out.iter().filter_map(|o| if let Output::RunFinished(r) = o { Some(r) } else { None }).collect();
        assert_eq!(runs.len(), 1, "map 1 completes when map 2 is generated");
        let m1 = runs[0];
        assert_eq!((m1.name.as_str(), m1.level, m1.map_secs, m1.deaths, m1.entries, m1.portals()), ("Strand", 83, 360, 1, 2, 1));
        assert_eq!(m1.started, t.session.id + 60);

        let cur = t.current.as_ref().unwrap();
        assert_eq!((cur.name.as_str(), cur.map_secs), ("Beach", 240));
        assert!(!t.in_map());
        assert_eq!(t.current_elapsed(t.session.last + 100), 240, "not ticking while in hideout");

        let s = stats(&t, t.session.last);
        assert_eq!(s.maps, 1);
        assert_eq!(s.deaths, 1, "only our character's deaths");
        assert_eq!(s.map_secs, 600);
        assert_eq!(s.hideout_secs, 60 + 60 + 60);
        assert_eq!(s.avg_map_secs, 360);
        assert_eq!(s.duration, 780);
        assert!((s.maps_per_hour - 3600.0 / 780.0).abs() < 1e-9);
    }

    #[test]
    fn live_elapsed_and_idle_split() {
        let mut t = Tracker::new(30 * 60);
        feed(&mut t, &[gen_line(0, "MapWorldsStrand", 7), l(0, ": You have entered Strand.")]);
        assert!(t.in_map());
        let start = t.session.id;
        assert_eq!(t.current_elapsed(start + 90), 90);
        // never counts more than the idle window past the last line
        assert_eq!(t.current_elapsed(start + 10_000), 30 * 60);

        // 2 h later: new session; the open map is finished at the last activity
        let out = feed(&mut t, &[l(7200, ": You have entered Felled Hideout.")]);
        assert!(matches!(&out[0], Output::RunFinished(r) if r.map_secs == 0 && r.entries == 1));
        assert!(out.iter().any(|o| matches!(o, Output::Session(s) if s.id == start && s.maps == 1)));
        assert_eq!(t.session.id, start + 7200);
        assert!(t.runs.is_empty());
    }

    #[test]
    fn reset_session_finishes_current_map() {
        let mut t = Tracker::new(30 * 60);
        feed(&mut t, &[gen_line(0, "MapWorldsStrand", 7), l(0, ": You have entered Strand."), l(100, ": You have entered Felled Hideout.")]);
        let start = t.session.id;
        let out = t.reset_session(start + 130);
        assert!(matches!(&out[0], Output::RunFinished(r) if r.map_secs == 100));
        assert_eq!(t.session.id, start + 130);
        assert_eq!(stats(&t, start + 130).maps, 0);
        // the hideout segment continues into the new session
        assert_eq!(stats(&t, start + 190).hideout_secs, 60);
    }

    #[test]
    fn deaths_without_character_name_count_all() {
        let mut t = Tracker::new(1800);
        feed(&mut t, &[gen_line(0, "MapWorldsStrand", 7), l(0, ": You have entered Strand."), l(5, ": Anyone has been slain.")]);
        assert_eq!(t.current.as_ref().unwrap().deaths, 1);
    }
}
