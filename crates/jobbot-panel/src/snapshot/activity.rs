//! The last 24 hours in half-hour slots with the state changes inside them, and what Claude cost
//! in that time.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use serde::Serialize;

use super::iso;
use crate::Panel;
use jobbot_app::profile::claude::load_claude_config;
use jobbot_app::profile::load_schedule;
use jobbot_domain::window::in_run_window;

/// How long one slot of the night strip is.
const SLOT_MINUTES: i64 = 30;
/// The night strip holds this many slots (24 hours), the last one holding "now".
const SLOT_COUNT: usize = 48;
/// The strip's list of state changes shows the newest ones, up to this many.
const MOVES_CAP: usize = 60;

/// One half hour of the night strip.
#[derive(Debug, Serialize)]
pub struct Slot {
    /// Start of the slot.
    pub at: String,
    /// The run window is open at the start of the slot; `None` when `schedule.toml` cannot be read.
    pub open: Option<bool>,
    /// Postings first seen inside the slot.
    pub seen: i64,
}

/// A state change in the strip's span, with the posting it happened to.
#[derive(Debug, Serialize)]
pub struct Move {
    pub at: String,
    pub key: String,
    pub company: String,
    pub title: String,
    pub to: &'static str,
}

/// The last 24 hours in half hours: what was found and what changed state. `from` is the start of
/// the first slot and `to` the end of the last, the one that holds "now".
#[derive(Debug, Serialize)]
pub struct Activity {
    pub from: String,
    pub to: String,
    pub slot_minutes: i64,
    pub slots: Vec<Slot>,
    /// Newest first, cut at `MOVES_CAP`; `moves_total` says how many there are.
    pub moves: Vec<Move>,
    pub moves_total: i64,
    /// Every state change of the span by the state it went into, not cut at `MOVES_CAP`: the
    /// counts add up to `moves_total`. Only the states that had a change appear.
    pub moves_by_state: BTreeMap<&'static str, i64>,
}

#[derive(Debug, Serialize)]
pub struct StageSpend {
    pub stage: String,
    pub calls: i64,
    pub dollars: f64,
}

#[derive(Debug, Serialize)]
pub struct ClaudeSpend {
    pub spent_24h: f64,
    /// `None` when `profile/claude.toml`, or the local file laid over it, cannot be read.
    pub daily_budget: Option<f64>,
    pub by_stage: Vec<StageSpend>,
}

/// The last 24 hours in half-hour slots aligned to the clock (UTC, whose half hours are also
/// Santiago's): the last slot is the one that holds `now`.
pub(super) fn activity(panel: &Panel, now: DateTime<Utc>) -> Result<Activity> {
    let last = now.duration_trunc(TimeDelta::minutes(SLOT_MINUTES))?;
    let first = last - TimeDelta::minutes(SLOT_MINUTES * (SLOT_COUNT as i64 - 1));
    let end = last + TimeDelta::minutes(SLOT_MINUTES);
    let mut seen = vec![0i64; SLOT_COUNT];
    for at in panel.tracker.first_seen_since(first)? {
        let slot = usize::try_from((at - first).num_seconds() / (SLOT_MINUTES * 60));
        if let Some(count) = slot.ok().and_then(|n| seen.get_mut(n)) {
            *count += 1;
        }
    }
    let schedule = load_schedule(&panel.paths.profile_dir.join("schedule.toml")).ok();
    let slots = seen
        .into_iter()
        .enumerate()
        .map(|(n, seen)| {
            let start = first + TimeDelta::minutes(SLOT_MINUTES * n as i64);
            Slot {
                at: iso(start),
                open: schedule.as_ref().map(|s| in_run_window(s, start)),
                seen,
            }
        })
        .collect();
    let (moves_total, moves) = panel.tracker.moves_between(first, now, MOVES_CAP)?;
    let moves_by_state = panel
        .tracker
        .move_counts_between(first, now)?
        .into_iter()
        .map(|(state, n)| (state.as_str(), n))
        .collect();
    Ok(Activity {
        from: iso(first),
        to: iso(end),
        slot_minutes: SLOT_MINUTES,
        slots,
        moves: moves
            .into_iter()
            .map(|m| Move {
                at: iso(m.at),
                key: m.job_key,
                company: m.company,
                title: m.title,
                to: m.to_state.as_str(),
            })
            .collect(),
        moves_total,
        moves_by_state,
    })
}

pub(super) fn claude_spend(panel: &Panel, now: DateTime<Utc>) -> Result<ClaudeSpend> {
    let since = now - TimeDelta::hours(24);
    let daily_budget = load_claude_config(&panel.paths.profile_dir.join("claude.toml"))
        .ok()
        .map(|config| config.daily_budget_usd);
    let by_stage = panel
        .tracker
        .claude_calls_by_stage(since)?
        .into_iter()
        .map(|(stage, (calls, dollars))| StageSpend {
            stage,
            calls,
            dollars,
        })
        .collect();
    Ok(ClaudeSpend {
        spent_24h: panel.tracker.claude_spend_since(since)?,
        daily_budget,
        by_stage,
    })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Timelike};

    use super::*;
    use crate::fixtures::panel;
    use crate::snapshot::build_snapshot;
    use crate::snapshot::support_tests::{profile_with, seen_at};
    use jobbot_domain::state::State;
    use jobbot_testkit::{NOW, PrepStore, Tracker, paths_with_empty_profile};

    #[test]
    fn claude_spend_covers_the_last_24_hours_only() {
        let (panel, _dir) = panel();
        let t = &panel.tracker;
        t.record_claude_call("dossier", *NOW - TimeDelta::hours(1), 0.25, 10, 5, true)
            .unwrap();
        t.record_claude_call("dossier", *NOW - TimeDelta::hours(30), 9.0, 1, 1, true)
            .unwrap();
        let claude = build_snapshot(&panel).unwrap().claude.unwrap();
        assert!((claude.spent_24h - 0.25).abs() < 1e-9);
        assert_eq!(claude.by_stage.len(), 1);
        assert_eq!(
            (claude.by_stage[0].stage.as_str(), claude.by_stage[0].calls),
            ("dossier", 1)
        );
    }

    /// A panel over fresh databases whose clock stands at `at`.
    fn panel_at(at: DateTime<Utc>) -> (Panel, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_with_empty_profile(dir.path());
        let panel = Panel::new(
            paths,
            Tracker::in_memory().unwrap(),
            PrepStore::in_memory().unwrap(),
            Box::new(move || at),
        );
        (panel, dir)
    }

    #[test]
    fn the_activity_is_48_half_hour_slots_ending_with_the_one_that_holds_now() {
        let at = |h, m, s| Utc.with_ymd_and_hms(2026, 10, 1, h, m, s).unwrap();
        // (clock, start of the last slot)
        let cases = [
            (at(12, 0, 0), "2026-10-01T12:00:00Z"),
            (at(12, 17, 45), "2026-10-01T12:00:00Z"),
            (at(12, 29, 59), "2026-10-01T12:00:00Z"),
            (at(12, 30, 0), "2026-10-01T12:30:00Z"),
            (at(0, 5, 0), "2026-10-01T00:00:00Z"),
        ];
        for (now, last) in cases {
            let (panel, _dir) = panel_at(now);
            let activity = build_snapshot(&panel).unwrap().activity.unwrap();

            assert_eq!(activity.slot_minutes, 30);
            assert_eq!(activity.slots.len(), 48);
            assert_eq!(activity.slots[47].at, last, "clock at {now}");
            let starts: Vec<DateTime<Utc>> = activity
                .slots
                .iter()
                .map(|s| {
                    DateTime::parse_from_rfc3339(&s.at)
                        .unwrap()
                        .with_timezone(&Utc)
                })
                .collect();
            assert!(
                starts
                    .windows(2)
                    .all(|w| w[1] - w[0] == TimeDelta::minutes(30)),
                "a slot every half hour, oldest first"
            );
            assert_eq!(activity.from, activity.slots[0].at);
            let end = starts[47] + TimeDelta::minutes(30);
            assert_eq!(activity.to, iso(end));
            assert!(starts[47] <= now && now < end, "the last slot holds now");
            assert!(
                starts
                    .iter()
                    .all(|s| s.minute() % 30 == 0 && s.second() == 0),
                "slots are aligned to the clock"
            );
        }
    }

    #[test]
    fn each_slot_counts_the_postings_first_seen_inside_it() {
        let (panel, _dir) = panel();
        let slot = TimeDelta::minutes(30);
        // The span is 2026-09-30T12:30Z to 2026-10-01T12:30Z; slot 10 starts at 17:30Z.
        let from = *NOW - slot * 47;
        let tenth = from + slot * 10;
        seen_at(&panel, "first", 50.0, from, None);
        seen_at(&panel, "before", 50.0, from - TimeDelta::seconds(1), None);
        seen_at(&panel, "ten-a", 50.0, tenth, None);
        seen_at(
            &panel,
            "ten-b",
            50.0,
            tenth + slot - TimeDelta::seconds(1),
            None,
        );
        seen_at(&panel, "now", 50.0, *NOW, None);
        seen_at(
            &panel,
            "end",
            50.0,
            *NOW + slot - TimeDelta::seconds(1),
            None,
        );
        seen_at(&panel, "past-end", 50.0, *NOW + slot, None);

        let activity = build_snapshot(&panel).unwrap().activity.unwrap();

        assert_eq!(activity.from, "2026-09-30T12:30:00Z");
        assert_eq!(activity.to, "2026-10-01T12:30:00Z");
        let mut expected = vec![0i64; 48];
        expected[0] = 1;
        expected[10] = 2;
        expected[47] = 2;
        let seen: Vec<i64> = activity.slots.iter().map(|s| s.seen).collect();
        assert_eq!(seen, expected);
        assert_eq!(activity.slots[10].at, "2026-09-30T17:30:00Z");
    }

    #[test]
    fn a_slot_is_open_when_the_schedule_has_no_busy_block_on_it() {
        let (mut panel, dir) = panel();
        // Busy 13:00 to 14:00 UTC every day: of the 48 slots only 13:00 and 13:30 of the 30th.
        let schedule = "[[busy]]\nzone = \"UTC\"\nstart = \"13:00\"\nend = \"14:00\"\n\
                        days = [\"mon\", \"tue\", \"wed\", \"thu\", \"fri\", \"sat\", \"sun\"]\n";
        panel.paths.profile_dir = profile_with(dir.path(), None, Some(schedule));

        let activity = build_snapshot(&panel).unwrap().activity.unwrap();

        let closed: Vec<&str> = activity
            .slots
            .iter()
            .filter(|s| s.open == Some(false))
            .map(|s| s.at.as_str())
            .collect();
        assert_eq!(closed, ["2026-09-30T13:00:00Z", "2026-09-30T13:30:00Z"]);
        assert_eq!(
            activity
                .slots
                .iter()
                .filter(|s| s.open == Some(true))
                .count(),
            46
        );
    }

    #[test]
    fn a_schedule_that_cannot_be_read_leaves_every_slot_without_an_answer() {
        let (mut panel, dir) = panel();
        seen_at(&panel, "1", 50.0, *NOW, None);
        for schedule in [None, Some("this is not a schedule")] {
            panel.paths.profile_dir = profile_with(dir.path(), None, schedule);
            let activity = build_snapshot(&panel).unwrap().activity.unwrap();
            assert_eq!(activity.slots.len(), 48);
            assert!(activity.slots.iter().all(|s| s.open.is_none()));
            assert_eq!(activity.slots[47].seen, 1, "the counts do not depend on it");
        }
    }

    #[test]
    fn the_moves_are_the_state_changes_of_the_span_newest_first() {
        let (panel, _dir) = panel();
        let from = *NOW - TimeDelta::minutes(47 * 30);
        let second = TimeDelta::seconds(1);
        seen_at(&panel, "before", 50.0, from - second, Some(&[]));
        let first = seen_at(&panel, "first", 50.0, from, Some(&[]));
        let mid = seen_at(&panel, "mid", 50.0, *NOW - TimeDelta::hours(2), Some(&[]));
        panel
            .tracker
            .transition(&mid, State::Prepared, None, *NOW - TimeDelta::hours(1))
            .unwrap();
        let last = seen_at(&panel, "last", 50.0, *NOW, Some(&[]));
        seen_at(&panel, "after", 50.0, *NOW + second, Some(&[]));

        let activity = build_snapshot(&panel).unwrap().activity.unwrap();

        assert_eq!(activity.moves_total, 4);
        assert_eq!(
            serde_json::to_value(&activity.moves_by_state).unwrap(),
            serde_json::json!({"prepared": 1, "scored": 3})
        );
        let seen: Vec<(&str, &str, &str, &str, &str)> = activity
            .moves
            .iter()
            .map(|m| {
                (
                    m.at.as_str(),
                    m.key.as_str(),
                    m.company.as_str(),
                    m.title.as_str(),
                    m.to,
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                (
                    "2026-10-01T12:00:00Z",
                    last.as_str(),
                    "Co last",
                    "Role last",
                    "scored"
                ),
                (
                    "2026-10-01T11:00:00Z",
                    mid.as_str(),
                    "Co mid",
                    "Role mid",
                    "prepared"
                ),
                (
                    "2026-10-01T10:00:00Z",
                    mid.as_str(),
                    "Co mid",
                    "Role mid",
                    "scored"
                ),
                (
                    "2026-09-30T12:30:00Z",
                    first.as_str(),
                    "Co first",
                    "Role first",
                    "scored"
                ),
            ]
        );
    }

    #[test]
    fn a_busy_day_shows_the_newest_60_moves_and_counts_all_of_them() {
        let (panel, _dir) = panel();
        let keys: Vec<String> = (0..65)
            .map(|n| {
                seen_at(
                    &panel,
                    &n.to_string(),
                    50.0,
                    *NOW - TimeDelta::minutes(n),
                    Some(&[]),
                )
            })
            .collect();

        let activity = build_snapshot(&panel).unwrap().activity.unwrap();

        assert_eq!(activity.moves_total, 65);
        assert_eq!(activity.moves.len(), 60);
        assert_eq!(activity.moves[0].key, keys[0]);
        assert_eq!(activity.moves[59].key, keys[59]);
        assert!(activity.moves.windows(2).all(|w| w[0].at > w[1].at));
    }

    #[test]
    fn the_moves_by_state_count_what_the_cut_list_leaves_out() {
        let (panel, _dir) = panel();
        // 65 postings scored in the last hour, and 10 of them prepared right after: 75 moves, of
        // which the list keeps the newest 60 but the counts keep all.
        for n in 0..65 {
            let key = seen_at(
                &panel,
                &n.to_string(),
                50.0,
                *NOW - TimeDelta::minutes(n + 1),
                Some(&[]),
            );
            if n < 10 {
                panel
                    .tracker
                    .transition(&key, State::Prepared, None, *NOW - TimeDelta::seconds(n))
                    .unwrap();
            }
        }

        let activity = build_snapshot(&panel).unwrap().activity.unwrap();

        assert_eq!(activity.moves_total, 75);
        assert_eq!(activity.moves.len(), 60);
        let by_state = serde_json::to_value(&activity.moves_by_state).unwrap();
        assert_eq!(by_state, serde_json::json!({"prepared": 10, "scored": 65}));
        let sum: i64 = activity.moves_by_state.values().sum();
        assert_eq!(sum, activity.moves_total, "the counts add up to the total");
    }
}
