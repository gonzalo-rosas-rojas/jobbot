//! Execution window: nothing automatic runs while Gonza is at work.
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
    // Truncating to a minute only fails for a time outside chrono's range, which a real clock
    // never gives; a panic here would be worse than the fallback. Without the cut the answer
    // carries the seconds of `now` instead of landing on the exact minute edge, which for a time
    // that cannot be cut is all the precision there is to keep.
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

    /// `utc_s` on a whole minute: the form the older tests below use.
    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        utc_s(y, m, d, h, min, 0)
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

    /// The one place this module's tests build an instant (the crate cannot use `jobbot_testkit`,
    /// which depends on it).
    fn utc_s(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, s).unwrap()
    }

    /// A schedule whose blocks are given as `(zone, start, end)` and apply every day.
    fn every_day(blocks: &[(&str, &str, &str)]) -> Schedule {
        let all = "[\"mon\",\"tue\",\"wed\",\"thu\",\"fri\",\"sat\",\"sun\"]";
        let text: String = blocks
            .iter()
            .map(|(zone, start, end)| block(zone, start, end, all))
            .collect();
        parse_schedule(&text).unwrap()
    }

    #[test]
    fn a_block_includes_its_first_minute_and_leaves_out_its_last_to_the_second() {
        let s = every_day(&[("America/Santiago", "09:00", "17:00")]);
        // Summer, UTC-3: 09:00 local is 12:00Z.
        let at = |h, mi, sec, ms| utc_s(2026, 3, 2, h, mi, sec) + TimeDelta::milliseconds(ms);
        assert!(
            in_run_window(&s, at(11, 59, 59, 999)),
            "08:59:59.999 is free"
        );
        assert!(!in_run_window(&s, at(12, 0, 0, 0)), "09:00:00 is busy");
        assert!(
            !in_run_window(&s, at(19, 59, 59, 999)),
            "16:59:59.999 is busy"
        );
        assert!(in_run_window(&s, at(20, 0, 0, 0)), "17:00:00 is free");
        // Winter, UTC-4: the same local hours are one hour later in UTC.
        let winter = |h, mi, sec| utc_s(2026, 7, 2, h, mi, sec);
        assert!(in_run_window(&s, winter(12, 59, 59)));
        assert!(!in_run_window(&s, winter(13, 0, 0)));
        assert!(!in_run_window(&s, winter(20, 59, 59)));
        assert!(in_run_window(&s, winter(21, 0, 0)));
    }

    #[test]
    fn a_block_in_the_hour_the_clocks_go_back_covers_both_times_that_hour_happens() {
        // 2025-04-06 03:00Z: Santiago goes from 24:00 back to 23:00, so 23:30 happens twice.
        let s = every_day(&[("America/Santiago", "23:30", "23:45")]);
        assert!(
            !in_run_window(&s, utc_s(2025, 4, 6, 2, 30, 0)),
            "23:30 the first time"
        );
        assert!(in_run_window(&s, utc_s(2025, 4, 6, 2, 45, 0)));
        assert!(
            !in_run_window(&s, utc_s(2025, 4, 6, 3, 30, 0)),
            "23:30 the second time"
        );
        assert!(in_run_window(&s, utc_s(2025, 4, 6, 3, 45, 0)));
    }

    #[test]
    fn a_block_in_the_hour_the_clocks_go_forward_covers_only_the_part_that_exists() {
        // 2025-09-07 04:00Z: Santiago goes from 24:00 to 01:00, so 00:00-00:59 never happens.
        let s = every_day(&[("America/Santiago", "00:30", "01:30")]);
        assert!(
            in_run_window(&s, utc_s(2025, 9, 7, 3, 59, 59)),
            "23:59:59 on the 6th"
        );
        assert!(
            !in_run_window(&s, utc_s(2025, 9, 7, 4, 0, 0)),
            "01:00, the first minute there is"
        );
        assert!(!in_run_window(&s, utc_s(2025, 9, 7, 4, 29, 59)));
        assert!(
            in_run_window(&s, utc_s(2025, 9, 7, 4, 30, 0)),
            "01:30 ends the block"
        );
    }

    #[test]
    fn the_day_of_a_block_is_the_weekday_in_its_own_zone() {
        let monday_only =
            parse_schedule(&block("America/Santiago", "00:00", "02:00", "[\"mon\"]")).unwrap();
        // Monday 2026-03-02 00:00 in Santiago (UTC-3) is 03:00Z; Sunday night UTC is not Monday there.
        assert!(in_run_window(&monday_only, utc_s(2026, 3, 2, 2, 59, 59)));
        assert!(!in_run_window(&monday_only, utc_s(2026, 3, 2, 3, 0, 0)));
        assert!(!in_run_window(&monday_only, utc_s(2026, 3, 2, 4, 59, 59)));
        assert!(in_run_window(&monday_only, utc_s(2026, 3, 2, 5, 0, 0)));
        // A block in Auckland (UTC+13 in March): Monday there starts on Sunday in UTC.
        let auckland =
            parse_schedule(&block("Pacific/Auckland", "00:00", "01:00", "[\"mon\"]")).unwrap();
        assert!(!in_run_window(&auckland, utc_s(2026, 3, 1, 11, 30, 0)));
        assert!(in_run_window(&auckland, utc_s(2026, 3, 1, 10, 59, 59)));
    }

    #[test]
    fn the_next_window_lands_on_the_exact_minute_edge_and_an_open_instant_comes_back_whole() {
        let s = every_day(&[("America/Santiago", "09:00", "17:00")]);
        // 12:00 local in summer: the block ends at 17:00 local = 20:00Z whatever the seconds and fractions.
        for now in [
            utc_s(2026, 3, 1, 15, 0, 0),
            utc_s(2026, 3, 1, 15, 0, 0) + TimeDelta::milliseconds(700),
            utc_s(2026, 3, 1, 19, 59, 59) + TimeDelta::milliseconds(999),
        ] {
            assert_eq!(
                next_window_start(&s, now),
                utc_s(2026, 3, 1, 20, 0, 0),
                "{now:?}"
            );
        }
        // Winter: 17:00 local is 21:00Z.
        assert_eq!(
            next_window_start(&s, utc_s(2026, 7, 1, 15, 0, 7)),
            utc_s(2026, 7, 1, 21, 0, 0)
        );
        // An open moment comes back as it is, fraction included.
        let open = utc_s(2026, 3, 1, 22, 0, 0) + TimeDelta::nanoseconds(123_456_789);
        assert_eq!(next_window_start(&s, open), open);
    }

    #[test]
    fn a_schedule_busy_the_whole_day_in_two_zones_has_no_window_and_answers_eight_days_ahead() {
        // Santiago is free at 23:59 local and Kolkata at 23:59 local, never at the same moment.
        let s = every_day(&[
            ("America/Santiago", "00:00", "23:59"),
            ("Asia/Kolkata", "00:00", "23:59"),
        ]);
        // 2026-03-01 15:00Z is 12:00 in Santiago and 20:30 in Kolkata: busy in both.
        let now = utc_s(2026, 3, 1, 15, 0, 0) + TimeDelta::milliseconds(500);
        assert!(!in_run_window(&s, now));
        assert_eq!(
            next_window_start(&s, now),
            utc_s(2026, 3, 9, 15, 0, 0) + TimeDelta::milliseconds(500),
            "eight days ahead, with the fraction of now"
        );
    }

    #[test]
    fn a_clock_is_hour_and_minute_with_or_without_the_leading_zeros_and_may_start_with_blanks() {
        let start_of = |text: &str| {
            parse_schedule(&block("America/Santiago", text, "23:59", "[\"mon\"]"))
                .map(|s| s.busy[0].start.to_string())
                .map_err(|e| e.to_string())
        };
        assert_eq!(start_of("09:00"), Ok("09:00:00".to_string()));
        assert_eq!(start_of("9:00"), Ok("09:00:00".to_string()));
        assert_eq!(start_of("00:00"), Ok("00:00:00".to_string()));
        let last_minute = parse_schedule(&block("America/Santiago", "00:00", "23:59", "[\"mon\"]"));
        assert_eq!(last_minute.unwrap().busy[0].end.to_string(), "23:59:00");
        assert_eq!(
            start_of("9:0"),
            Ok("09:00:00".to_string()),
            "the minute may lose its zero too"
        );
        assert_eq!(
            start_of(" 09:00"),
            Ok("09:00:00".to_string()),
            "blanks before the hour are skipped"
        );
        for bad in [
            "24:00", "09:60", "9am", "09:00:00", "09", "", "09.00", "09:00 ",
        ] {
            assert!(start_of(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            start_of("9am"),
            Err("bad time \"9am\", expected HH:MM".to_string())
        );
    }

    #[test]
    fn a_refused_clock_says_its_own_message_and_carries_the_parsers_error_as_its_cause() {
        use std::error::Error;
        // The parser's own wording belongs to the library and is not asserted here.
        for (bad, message) in [
            ("9am", "bad time \"9am\", expected HH:MM"),
            ("09", "bad time \"09\", expected HH:MM"),
            ("25:00", "bad time \"25:00\", expected HH:MM"),
        ] {
            let err =
                parse_schedule(&block("America/Santiago", bad, "17:00", "[\"mon\"]")).unwrap_err();
            assert_eq!(err.to_string(), message);
            let cause = err.source().expect("the parser's error follows ours");
            assert!(!cause.to_string().is_empty(), "{bad:?}");
        }
    }

    #[test]
    fn weekdays_are_read_by_three_letters_or_the_whole_name_in_any_case() {
        let days_of = |names: &str| {
            parse_schedule(&block("America/Santiago", "09:00", "17:00", names))
                .map(|s| s.busy[0].days.clone())
                .map_err(|e| e.to_string())
        };
        assert_eq!(
            days_of("[\"mon\",\"Tue\",\"WED\",\"thu\",\"Friday\",\"saturday\",\"SUNDAY\"]"),
            Ok(vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
                Weekday::Sat,
                Weekday::Sun,
            ])
        );
        for bad in ["mo", "tues", "thur", "lunes", "1", " mon", ""] {
            assert!(
                days_of(&format!("[\"{bad}\"]")).is_err(),
                "{bad:?} is not a weekday"
            );
        }
        assert_eq!(
            days_of("[\"funday\"]"),
            Err("unknown weekday \"funday\", use mon..sun".to_string())
        );
    }

    #[test]
    fn a_valid_schedule_file_is_read() {
        let ok = r#"
            [[busy]]
            zone = "America/Santiago"
            start = "08:30"
            end = "12:00"
            days = ["Mon", "friday"]
        "#;
        let parsed = parse_schedule(ok).unwrap();
        assert_eq!(parsed.busy[0].days, vec![Weekday::Mon, Weekday::Fri]);
    }

    /// The message with which `parse_schedule` refuses `text`.
    fn bad(text: &str) -> String {
        parse_schedule(text).unwrap_err().to_string()
    }

    /// A schedule of one `[[busy]]` block.
    fn block(zone: &str, start: &str, end: &str, days: &str) -> String {
        format!(
            "[[busy]]\nzone = \"{zone}\"\nstart = \"{start}\"\nend = \"{end}\"\ndays = {days}\n"
        )
    }

    #[test]
    fn a_schedule_that_is_not_toml_is_refused() {
        assert!(bad("").contains("invalid schedule TOML"));
    }

    #[test]
    fn a_schedule_without_blocks_is_refused() {
        assert!(bad("busy = []").contains("no [[busy]] blocks"));
    }

    #[test]
    fn an_unknown_time_zone_is_refused() {
        assert!(
            bad(&block("Mars/Olympus", "09:00", "17:00", "[\"mon\"]"))
                .contains("unknown time zone")
        );
    }

    #[test]
    fn a_time_not_written_as_hh_mm_is_refused() {
        assert!(
            bad(&block("America/Santiago", "9am", "17:00", "[\"mon\"]")).contains("expected HH:MM")
        );
    }

    #[test]
    fn a_block_that_does_not_end_after_it_starts_is_refused() {
        assert!(
            bad(&block("America/Santiago", "17:00", "09:00", "[\"mon\"]"))
                .contains("end must be after start")
        );
    }

    #[test]
    fn a_block_without_days_is_refused() {
        assert!(bad(&block("America/Santiago", "09:00", "17:00", "[]")).contains("lists no days"));
    }

    #[test]
    fn an_unknown_weekday_is_refused() {
        assert!(
            bad(&block("America/Santiago", "09:00", "17:00", "[\"funday\"]"))
                .contains("unknown weekday")
        );
    }
}
