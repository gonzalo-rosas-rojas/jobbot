//! Execution window: nothing automatic runs during the busy hours of `profile/schedule.toml`.
//!
//! Busy hours are the union of the blocks listed in `profile/schedule.toml`. Every block is
//! written in the local time of its own zone, so daylight saving changes are handled by
//! `chrono-tz` and the overlap between New York and Santiago moves during the year.

use std::str::FromStr;

use chrono::{DateTime, Datelike, DurationRound, NaiveTime, TimeDelta, Utc, Weekday};
use chrono_tz::Tz;
use serde::Deserialize;

/// How far `next_window_start` looks ahead. A weekly schedule repeats every 7 days, so
/// 8 days is enough to find an open minute unless the schedule is busy around the clock.
const LOOKAHEAD_DAYS: i64 = 8;

/// One recurring busy block, expressed in the local time of `zone`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusyBlock {
    pub zone: Tz,
    /// First busy minute (inclusive).
    pub start: NaiveTime,
    /// First free minute (exclusive).
    pub end: NaiveTime,
    /// Local weekdays of `zone` on which the block applies.
    pub days: Vec<Weekday>,
}

impl BusyBlock {
    /// True when `now`, seen from this block's time zone, falls inside the block.
    fn covers(&self, now: DateTime<Utc>) -> bool {
        // UTC -> local time is always unambiguous, even on a DST change day.
        let local = now.with_timezone(&self.zone);
        let time = local.time();
        self.days.contains(&local.weekday()) && self.start <= time && time < self.end
    }
}

/// The parsed `profile/schedule.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub busy: Vec<BusyBlock>,
}

#[derive(Deserialize)]
struct RawSchedule {
    busy: Vec<RawBlock>,
}

#[derive(Deserialize)]
struct RawBlock {
    zone: String,
    start: String,
    end: String,
    days: Vec<String>,
}

/// Why a schedule file was refused. The messages are what Gonza reads when he edits
/// `profile/schedule.toml` wrongly, so each names the thing to fix.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    // The TOML message is part of ours (not a `#[source]`): printing the whole chain would say it twice.
    #[error("invalid schedule TOML: {0}")]
    Toml(toml::de::Error),
    #[error("schedule has no [[busy]] blocks: refusing to run around the clock")]
    NoBlocks,
    #[error("unknown time zone {zone:?}: {reason}")]
    UnknownZone { zone: String, reason: String },
    // Unlike the TOML and time-zone texts, the parser's reason is not in ours: it follows ours as
    // the source, so the whole chain (`{:#}`) ends with it, as it always did.
    #[error("bad time {text:?}, expected HH:MM")]
    BadClock {
        text: String,
        #[source]
        source: chrono::ParseError,
    },
    #[error("block {zone} {start}-{end}: end must be after start")]
    EndNotAfterStart {
        zone: String,
        start: String,
        end: String,
    },
    #[error("unknown weekday {day:?}, use mon..sun")]
    UnknownWeekday { day: String },
    #[error("block {zone} {start}-{end} lists no days")]
    NoDays {
        zone: String,
        start: String,
        end: String,
    },
}

fn parse_clock(text: &str) -> Result<NaiveTime, ScheduleError> {
    NaiveTime::parse_from_str(text, "%H:%M").map_err(|source| ScheduleError::BadClock {
        text: text.to_string(),
        source,
    })
}

fn parse_block(raw: RawBlock) -> Result<BusyBlock, ScheduleError> {
    let zone = Tz::from_str(&raw.zone).map_err(|e| ScheduleError::UnknownZone {
        zone: raw.zone.clone(),
        reason: e.to_string(),
    })?;
    let start = parse_clock(&raw.start)?;
    let end = parse_clock(&raw.end)?;
    if end <= start {
        return Err(ScheduleError::EndNotAfterStart {
            zone: raw.zone,
            start: raw.start,
            end: raw.end,
        });
    }
    let days = raw
        .days
        .iter()
        .map(|day| {
            day.parse::<Weekday>()
                .map_err(|_| ScheduleError::UnknownWeekday { day: day.clone() })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if days.is_empty() {
        return Err(ScheduleError::NoDays {
            zone: raw.zone,
            start: raw.start,
            end: raw.end,
        });
    }
    Ok(BusyBlock {
        zone,
        start,
        end,
        days,
    })
}

/// Parses the text of a schedule file. The application's `load_schedule` reads the file and calls
/// this, so the rules can be tested with plain strings.
pub fn parse_schedule(text: &str) -> Result<Schedule, ScheduleError> {
    let raw: RawSchedule = toml::from_str(text).map_err(ScheduleError::Toml)?;
    if raw.busy.is_empty() {
        return Err(ScheduleError::NoBlocks);
    }
    let busy = raw
        .busy
        .into_iter()
        .map(parse_block)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Schedule { busy })
}

/// True when no busy block covers `now`.
pub fn in_run_window(schedule: &Schedule, now: DateTime<Utc>) -> bool {
    !schedule.busy.iter().any(|block| block.covers(now))
}

/// `now` itself when the window is already open; otherwise the first whole minute after `now`
/// at which no block covers. Block edges fall on whole minutes, so this is exactly the moment
/// the last covering block ends. A schedule that is busy around the clock has no next window:
/// the function then returns the end of its 8-day lookahead.
pub fn next_window_start(schedule: &Schedule, now: DateTime<Utc>) -> DateTime<Utc> {
    if in_run_window(schedule, now) {
        return now;
    }
    let minute = TimeDelta::minutes(1);
    let mut t = now.duration_trunc(minute).unwrap_or(now) + minute;
    let limit = now + TimeDelta::days(LOOKAHEAD_DAYS);
    while t < limit {
        if in_run_window(schedule, t) {
            return t;
        }
        t += minute;
    }
    limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// The shipped file, so a careless edit of `profile/schedule.toml` breaks these tests.
    fn schedule() -> Schedule {
        parse_schedule(include_str!("../../../profile/schedule.toml")).unwrap()
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    #[test]
    fn window_busy_during_ny_hours() {
        let s = schedule();
        // Wed 2026-10-14, 16:30 in New York (EDT, UTC-4) but already 17:30 in Santiago:
        // only the New York block covers this instant.
        assert!(!in_run_window(&s, utc(2026, 10, 14, 20, 30)));
        assert!(!in_run_window(&s, utc(2026, 10, 14, 20, 59)));
        // 17:00 in New York: the block ends (end is exclusive), so the window opens.
        assert!(in_run_window(&s, utc(2026, 10, 14, 21, 0)));
    }

    #[test]
    fn window_busy_during_santiago_hours_before_ny_opens() {
        let s = schedule();
        // Wed 2026-10-14, 09:00 in Santiago (UTC-3) is still 08:00 in New York (UTC-4).
        assert!(!in_run_window(&s, utc(2026, 10, 14, 12, 0)));
        // One minute earlier it is 08:59 in Santiago and 07:59 in New York: free.
        assert!(in_run_window(&s, utc(2026, 10, 14, 11, 59)));
    }

    #[test]
    fn window_open_in_the_evening() {
        let s = schedule();
        // 18:00 in New York, 19:00 in Santiago.
        assert!(in_run_window(&s, utc(2026, 10, 14, 22, 0)));
        // 23:00 in New York, 00:00 (Thursday) in Santiago.
        assert!(in_run_window(&s, utc(2026, 10, 15, 3, 0)));
    }

    #[test]
    fn window_weekend_is_open() {
        let s = schedule();
        // Friday evening: 17:00 in New York, 18:00 in Santiago.
        assert!(in_run_window(&s, utc(2026, 10, 16, 21, 0)));
        // Saturday and Sunday at midday local time.
        assert!(in_run_window(&s, utc(2026, 10, 17, 15, 0)));
        assert!(in_run_window(&s, utc(2026, 10, 18, 15, 0)));
        // Saturday 21:00 in New York is already Sunday 01:00 in UTC; still the weekend.
        assert!(in_run_window(&s, utc(2026, 10, 18, 1, 0)));
        // Monday 09:00 in Santiago: the work week starts again.
        assert!(!in_run_window(&s, utc(2026, 10, 19, 12, 0)));
    }

    #[test]
    fn window_dst_us_fall_back_shifts_the_overlap() {
        let s = schedule();
        // US clocks go back on Sunday 2026-11-01; Chile stays at UTC-3.
        // The same UTC moment (21:30), a Friday before and a Monday after the change:
        let before = utc(2026, 10, 30, 21, 30); // New York 17:30 EDT, Santiago 18:30: free
        let after = utc(2026, 11, 2, 21, 30); // New York 16:30 EST, Santiago 18:30: busy
        assert!(in_run_window(&s, before));
        assert!(!in_run_window(&s, after));
        // The start of the busy day does not move: Santiago opens it at 12:00 UTC both times.
        assert!(!in_run_window(&s, utc(2026, 10, 30, 12, 0)));
        assert!(!in_run_window(&s, utc(2026, 11, 2, 12, 0)));
        // After the change New York's 09:00 is 14:00 UTC, so the union runs until 22:00 UTC.
        assert!(!in_run_window(&s, utc(2026, 11, 2, 21, 59)));
        assert!(in_run_window(&s, utc(2026, 11, 2, 22, 0)));
        // The change day itself is a Sunday: open, also around the repeated 01:30 in New York
        // (05:30 UTC is 01:30 EDT, 06:30 UTC is 01:30 EST) and at midday.
        assert!(in_run_window(&s, utc(2026, 11, 1, 5, 30)));
        assert!(in_run_window(&s, utc(2026, 11, 1, 6, 30)));
        assert!(in_run_window(&s, utc(2026, 11, 1, 17, 0)));
    }

    #[test]
    fn window_dst_chile_changes_the_offset() {
        let s = schedule();
        // Chile ends daylight saving on Saturday 2027-04-03; New York is still on EDT.
        // The same UTC moment (12:30), a Friday before and a Monday after the change:
        let before = utc(2027, 3, 26, 12, 30); // Santiago 09:30 (UTC-3): busy
        let after = utc(2027, 4, 5, 12, 30); // Santiago 08:30 (UTC-4), New York 08:30: free
        assert!(!in_run_window(&s, before));
        assert!(in_run_window(&s, after));
        // After the change both zones open at 13:00 UTC.
        assert!(!in_run_window(&s, utc(2027, 4, 5, 13, 0)));
        // The change itself happens at midnight between Saturday and Sunday: both days are open.
        assert!(in_run_window(&s, utc(2027, 4, 3, 15, 0)));
        assert!(in_run_window(&s, utc(2027, 4, 4, 3, 30)));
        assert!(in_run_window(&s, utc(2027, 4, 4, 4, 30)));
    }

    #[test]
    fn next_window_start_returns_now_when_open() {
        let s = schedule();
        let now = utc(2026, 10, 14, 22, 0);
        assert_eq!(next_window_start(&s, now), now);
        // Also when `now` has seconds: an open moment is returned untouched.
        let odd = now + TimeDelta::seconds(17);
        assert_eq!(next_window_start(&s, odd), odd);
    }

    #[test]
    fn next_window_start_skips_busy_block() {
        let s = schedule();
        // Wed 2026-10-14 15:00 UTC: busy. Santiago's block ends at 20:00 UTC but New York's
        // keeps going until 17:00 EDT = 21:00 UTC, so the window opens at 21:00 UTC.
        let now = utc(2026, 10, 14, 15, 0);
        let next = next_window_start(&s, now);
        assert_eq!(next, utc(2026, 10, 14, 21, 0));
        assert!(in_run_window(&s, next));
        assert!(!in_run_window(&s, next - TimeDelta::minutes(1)));
        // From a busy moment with seconds, the answer is still the exact block edge.
        let with_seconds = now + TimeDelta::seconds(41);
        assert_eq!(next_window_start(&s, with_seconds), next);
        // Friday: the next window is Friday evening, not Monday.
        assert_eq!(
            next_window_start(&s, utc(2026, 10, 16, 15, 0)),
            utc(2026, 10, 16, 21, 0)
        );
    }

    #[test]
    fn next_window_start_waits_for_the_later_block_end() {
        let s = schedule();
        // Wed 2026-12-16: Santiago (UTC-3) is free from 20:00 UTC, New York (UTC-5) only from 22:00 UTC.
        assert_eq!(
            next_window_start(&s, utc(2026, 12, 16, 15, 0)),
            utc(2026, 12, 16, 22, 0)
        );
    }

    #[test]
    fn schedule_file_is_validated() {
        let ok = r#"
            [[busy]]
            zone = "America/Santiago"
            start = "08:30"
            end = "12:00"
            days = ["Mon", "friday"]
        "#;
        let parsed = parse_schedule(ok).unwrap();
        assert_eq!(parsed.busy[0].days, vec![Weekday::Mon, Weekday::Fri]);

        let bad = |text: &str| parse_schedule(text).unwrap_err().to_string();
        let block = |zone: &str, start: &str, end: &str, days: &str| {
            format!(
                "[[busy]]\nzone = \"{zone}\"\nstart = \"{start}\"\nend = \"{end}\"\ndays = {days}\n"
            )
        };
        assert!(bad("").contains("invalid schedule TOML"));
        assert!(bad("busy = []").contains("no [[busy]] blocks"));
        assert!(
            bad(&block("Mars/Olympus", "09:00", "17:00", "[\"mon\"]"))
                .contains("unknown time zone")
        );
        assert!(
            bad(&block("America/Santiago", "9am", "17:00", "[\"mon\"]")).contains("expected HH:MM")
        );
        assert!(
            bad(&block("America/Santiago", "17:00", "09:00", "[\"mon\"]"))
                .contains("end must be after start")
        );
        assert!(bad(&block("America/Santiago", "09:00", "17:00", "[]")).contains("lists no days"));
        assert!(
            bad(&block("America/Santiago", "09:00", "17:00", "[\"funday\"]"))
                .contains("unknown weekday")
        );
    }
}
