//! The text of the dates the application writes for a person or for itself, and the rules of
//! Santiago's clock, written out as literals.
//!
//! Gonza reads Santiago's time (UTC-3 in summer, UTC-4 in winter; the clocks went back on
//! 2025-04-06 at 03:00 UTC and forward on 2025-09-07 at 04:00 UTC). The program writes its own
//! stamps in `meta` as RFC 3339 with a `+00:00` offset, which is not the form the database
//! columns use (`Z`). These tests fix both, so a change of library shows up here.

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::json;

use jobbot_app::actions::{earliest_date, latest_due, parse_snooze_until};
use jobbot_app::apply::results::{AttemptMode, LastResults, ResultLine, format_table};
use jobbot_app::claude::ClaudeGate;
use jobbot_app::inbox::{INBOX_IMPORTED_AT, import};
use jobbot_app::learn::dossier::{Dossier, render_markdown};
use jobbot_app::reporting::digest::build_digest;
use jobbot_app::reporting::status::{
    RunEnd, StepLog, describe_last_run, record_run, stamp_with_age,
};
use jobbot_app::reporting::today::build_today;
use jobbot_testkit::{FakeClaude, Tracker, make_job, utc_at as utc};

/// What Santiago's clock reads at `at`, as the reports write it.
fn santiago(at: DateTime<Utc>) -> String {
    let now = at + TimeDelta::minutes(5);
    let text = stamp_with_age(at, now, false);
    text.split(" (").next().unwrap().to_string()
}

// ---- Santiago's clock

#[test]
fn santiagos_clock_is_utc_minus_three_in_summer_and_minus_four_in_winter() {
    for (at, expected) in [
        (utc(2026, 1, 15, 12, 0, 0), "2026-01-15 09:00"),
        (utc(2026, 7, 15, 12, 0, 0), "2026-07-15 08:00"),
        (utc(2025, 7, 1, 12, 0, 0), "2025-07-01 08:00"),
        (utc(2025, 12, 31, 14, 0, 0), "2025-12-31 11:00"),
    ] {
        assert_eq!(santiago(at), expected, "{at:?}");
    }
}

#[test]
fn santiagos_day_starts_at_03_utc_in_summer_and_04_utc_in_winter() {
    for (at, expected) in [
        (utc(2026, 1, 15, 2, 59, 59), "2026-01-14 23:59"),
        (utc(2026, 1, 15, 3, 0, 0), "2026-01-15 00:00"),
        (utc(2026, 7, 15, 3, 59, 59), "2026-07-14 23:59"),
        (utc(2026, 7, 15, 4, 0, 0), "2026-07-15 00:00"),
    ] {
        assert_eq!(santiago(at), expected, "{at:?}");
    }
}

#[test]
fn the_hour_the_clocks_go_back_in_april_2025_is_read_twice() {
    // 2025-04-06 03:00 UTC: Santiago goes from 24:00 (UTC-3) back to 23:00 (UTC-4).
    for (at, expected) in [
        (utc(2025, 4, 6, 2, 59, 59), "2025-04-05 23:59"),
        (utc(2025, 4, 6, 3, 0, 0), "2025-04-05 23:00"),
        (utc(2025, 4, 6, 3, 59, 59), "2025-04-05 23:59"),
        (utc(2025, 4, 6, 4, 0, 0), "2025-04-06 00:00"),
    ] {
        assert_eq!(
            santiago(at),
            expected,
            "{at:?}: two instants an hour apart read the same"
        );
    }
}

#[test]
fn the_hour_the_clocks_go_forward_in_september_2025_never_shows() {
    // 2025-09-07 04:00 UTC: Santiago goes from 24:00 (UTC-4) straight to 01:00 (UTC-3).
    for (at, expected) in [
        (utc(2025, 9, 7, 3, 59, 59), "2025-09-06 23:59"),
        (utc(2025, 9, 7, 4, 0, 0), "2025-09-07 01:00"),
        (utc(2025, 9, 7, 4, 59, 59), "2025-09-07 01:59"),
        (utc(2025, 9, 7, 5, 0, 0), "2025-09-07 02:00"),
    ] {
        assert_eq!(santiago(at), expected, "{at:?}");
    }
}

#[test]
fn a_stamp_says_how_long_ago_in_minutes_hours_and_days() {
    let at = utc(2026, 1, 15, 12, 0, 0);
    for (later, expected) in [
        (TimeDelta::seconds(59), "2026-01-15 09:00 (hace un momento)"),
        (TimeDelta::seconds(60), "2026-01-15 09:00 (hace 1 min)"),
        (TimeDelta::minutes(59), "2026-01-15 09:00 (hace 59 min)"),
        (TimeDelta::minutes(60), "2026-01-15 09:00 (hace 1 h)"),
        (
            TimeDelta::hours(47) + TimeDelta::minutes(59),
            "2026-01-15 09:00 (hace 47 h)",
        ),
        (TimeDelta::hours(48), "2026-01-15 09:00 (hace 2 días)"),
        (TimeDelta::days(10), "2026-01-15 09:00 (hace 10 días)"),
    ] {
        assert_eq!(stamp_with_age(at, at + later, false), expected, "{later:?}");
    }
    assert_eq!(
        stamp_with_age(at, at + TimeDelta::hours(2), true),
        "2026-01-15 09:00 (hora de Santiago, hace 2 h)"
    );
    assert_eq!(
        stamp_with_age(
            at + TimeDelta::milliseconds(900),
            at + TimeDelta::seconds(61),
            false
        ),
        "2026-01-15 09:00 (hace 1 min)",
        "the age counts the real instants, the clock shows the minute"
    );
}

#[test]
fn the_title_of_the_digest_and_of_today_is_the_day_in_santiago() {
    let tracker = Tracker::in_memory().unwrap();
    for (now, day) in [
        (utc(2026, 1, 15, 2, 59, 59), "2026-01-14"),
        (utc(2026, 1, 15, 3, 0, 0), "2026-01-15"),
        (utc(2026, 7, 15, 3, 59, 59), "2026-07-14"),
        (utc(2026, 7, 15, 4, 0, 0), "2026-07-15"),
        (utc(2025, 4, 6, 3, 30, 0), "2025-04-05"),
        (utc(2025, 9, 7, 4, 0, 0), "2025-09-07"),
    ] {
        let digest = build_digest(&tracker, now, 5).unwrap();
        assert_eq!(
            digest.lines().next().unwrap(),
            format!("# Resumen diario — {day}"),
            "{now:?}"
        );
        let today = build_today(&tracker, now).unwrap();
        assert_eq!(
            today.lines().next().unwrap(),
            format!("Tu lista de hoy — {day} (hora de Santiago)"),
            "{now:?}"
        );
    }
}

#[test]
fn the_day_in_a_dossier_is_the_day_in_santiago() {
    let dossier = Dossier {
        summary: String::new(),
        questions: vec![],
        stories: vec![],
        dropped_stories: 0,
        gaps: vec![],
    };
    for (now, day) in [
        (utc(2026, 1, 15, 2, 59, 59), "2026-01-14"),
        (utc(2026, 1, 15, 3, 0, 0), "2026-01-15"),
        (utc(2026, 7, 15, 3, 59, 59), "2026-07-14"),
        (utc(2026, 7, 15, 4, 0, 0), "2026-07-15"),
    ] {
        let md = render_markdown(&make_job(), &dossier, now);
        assert!(
            md.contains(&format!("Generado el {day} (hora de Santiago) · aviso: ")),
            "{now:?}: {md}"
        );
    }
}

#[test]
fn the_results_table_shows_the_time_of_each_line_in_santiago_and_leaves_other_text_alone() {
    let line = |timestamp: &str| ResultLine {
        timestamp: timestamp.to_string(),
        key: "test:1".into(),
        source: "test".into(),
        company: "Acme".into(),
        title: "Role".into(),
        mode: AttemptMode::DryRun,
        state: "prepared".into(),
        fields_filled: Some(3),
        fields_left: vec![],
        screenshot: None,
        handoff_sheet: None,
    };
    let stamps = [
        ("2026-10-01T12:00:00Z", "2026-10-01 09:00"),
        ("2026-07-01T12:00:00Z", "2026-07-01 08:00"),
        ("2025-04-06T03:00:00Z", "2025-04-05 23:00"),
        ("2025-09-07T04:00:00Z", "2025-09-07 01:00"),
        ("2026-10-01T12:00:00+00:00", "2026-10-01 09:00"),
        ("2026-10-01T12:00:00.987Z", "2026-10-01 09:00"),
        ("2026-10-01T09:00:00-03:00", "2026-10-01 09:00"),
        ("ayer", "ayer"),
        ("2026-10-01", "2026-10-01"),
    ];
    let table = format_table(&LastResults {
        lines: stamps.iter().map(|(stamp, _)| line(stamp)).collect(),
        total: stamps.len(),
    });
    let rows: Vec<&str> = table.lines().skip(2).collect();
    assert_eq!(rows.len(), stamps.len(), "one row for each stamp:\n{table}");
    for ((stamp, expected), row) in stamps.iter().zip(rows) {
        // The columns are padded and told apart by two spaces; no date cell has two in a row.
        let cell = row.split("  ").next().unwrap();
        assert_eq!(cell, *expected, "{stamp}: the FECHA cell of {row:?}");
    }
}

// ---- the stamps the program writes for itself

/// The four fractions an instant arrives with, and what `to_rfc3339` writes for each.
const META_TEXTS: [(i64, &str); 6] = [
    (0, "2026-03-01T12:00:00+00:00"),
    (500_000_000, "2026-03-01T12:00:00.500+00:00"),
    (120_000_000, "2026-03-01T12:00:00.120+00:00"),
    (123_000, "2026-03-01T12:00:00.000123+00:00"),
    (123_450_000, "2026-03-01T12:00:00.123450+00:00"),
    (123_456_789, "2026-03-01T12:00:00.123456789+00:00"),
];

#[test]
fn the_end_of_a_run_is_written_with_a_plus_zero_offset_and_the_digits_of_its_fraction() {
    for (nanos, expected) in META_TEXTS {
        let tracker = Tracker::in_memory().unwrap();
        let finished = utc(2026, 3, 1, 12, 0, 0) + TimeDelta::nanoseconds(nanos);
        record_run(&tracker, finished, &RunEnd::Done, &mut StepLog::default()).unwrap();
        assert_eq!(
            tracker.get_meta("last_run_at").unwrap().as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn the_import_of_the_mailbox_leaves_its_time_with_a_plus_zero_offset_and_the_digits_of_its_fraction()
 {
    for (nanos, expected) in META_TEXTS {
        let tracker = Tracker::in_memory().unwrap();
        let now = utc(2026, 3, 1, 12, 0, 0) + TimeDelta::nanoseconds(nanos);
        import(&tracker, &[], now).unwrap();
        assert_eq!(
            tracker.get_meta(INBOX_IMPORTED_AT).unwrap().as_deref(),
            Some(expected)
        );
    }
}

#[tokio::test]
async fn the_account_check_writes_its_time_the_same_way_in_both_places() {
    for (nanos, expected) in META_TEXTS {
        let tracker = Tracker::in_memory().unwrap();
        let fake = FakeClaude::new(vec![]);
        let gate = ClaudeGate::new(&fake, &tracker, 1.0);
        let now = utc(2026, 3, 1, 12, 0, 0) + TimeDelta::nanoseconds(nanos);
        gate.check_account(now).await.unwrap();
        assert_eq!(
            tracker
                .get_meta("claude_account_last_check")
                .unwrap()
                .as_deref(),
            Some(expected)
        );
        let record: serde_json::Value =
            serde_json::from_str(&tracker.get_meta("claude_account_last").unwrap().unwrap())
                .unwrap();
        assert_eq!(record["checked_at"], json!(expected));
    }
}

#[test]
fn the_last_run_reads_back_from_any_rfc_3339_shape_and_an_unreadable_one_is_said() {
    for stored in [
        "2026-03-01T12:00:00+00:00",
        "2026-03-01T12:00:00Z",
        "2026-03-01T12:00:00.123456789+00:00",
        "2026-03-01T09:00:00-03:00",
        "2026-03-01T15:30:00+03:30",
    ] {
        let tracker = Tracker::in_memory().unwrap();
        tracker.set_meta("last_run_at", stored).unwrap();
        tracker.set_meta("last_run_result", "ok").unwrap();
        // One second past two hours, so the fraction of the stored stamp does not change the age.
        let line = describe_last_run(&tracker, utc(2026, 3, 1, 14, 0, 1)).unwrap();
        assert!(
            line.starts_with("Última corrida: 2026-03-01 09:00 (hora de Santiago, hace 2 h): ok"),
            "{stored}: {line}"
        );
    }
    for unreadable in [
        "2026-03-01",
        "2026-03-01T12:00:00",
        "ayer",
        "1772366400",
        "",
    ] {
        let tracker = Tracker::in_memory().unwrap();
        tracker.set_meta("last_run_at", unreadable).unwrap();
        assert_eq!(
            describe_last_run(&tracker, utc(2026, 3, 1, 14, 0, 0)).unwrap(),
            format!("Última corrida: fecha ilegible ({unreadable})")
        );
    }
}

// ---- the rules that take a date

#[test]
fn a_snooze_to_monday_is_midnight_at_the_start_of_the_next_monday_in_santiago() {
    let until = |now: DateTime<Utc>| parse_snooze_until("monday", now);
    for (now, expected) in [
        // Summer, UTC-3: Thursday 2026-01-15 -> Monday the 19th at 00:00 local = 03:00Z.
        (utc(2026, 1, 15, 12, 0, 0), utc(2026, 1, 19, 3, 0, 0)),
        // Winter, UTC-4: Thursday 2026-07-16 -> Monday the 20th = 04:00Z.
        (utc(2026, 7, 16, 12, 0, 0), utc(2026, 7, 20, 4, 0, 0)),
        // The Monday after the clocks went back (Saturday night 2025-04-05): UTC-4 already.
        (utc(2025, 4, 2, 12, 0, 0), utc(2025, 4, 7, 4, 0, 0)),
        // The Monday after the clocks went forward (Saturday night 2025-09-06): UTC-3 already.
        (utc(2025, 9, 3, 12, 0, 0), utc(2025, 9, 8, 3, 0, 0)),
        // Sunday evening in Santiago is still Sunday there: Monday starts in a few hours.
        (utc(2025, 7, 6, 23, 0, 0), utc(2025, 7, 7, 4, 0, 0)),
        (utc(2025, 7, 7, 3, 59, 59), utc(2025, 7, 7, 4, 0, 0)),
        // At Monday 00:00 in Santiago it is Monday: the next one is a week away.
        (utc(2025, 7, 7, 4, 0, 0), utc(2025, 7, 14, 4, 0, 0)),
        // 02:59:59Z on a Monday in summer is still Sunday night in Santiago; 03:00:00Z is Monday.
        (utc(2026, 1, 19, 2, 59, 59), utc(2026, 1, 19, 3, 0, 0)),
        (utc(2026, 1, 19, 3, 0, 0), utc(2026, 1, 26, 3, 0, 0)),
    ] {
        assert_eq!(until(now), Some(expected), "{now:?}");
    }
}

#[test]
fn a_snooze_in_days_adds_whole_days_of_24_hours_whatever_the_clocks_do() {
    // Across the change of April 2025 the snooze still ends 24 hours per day later, not at the same
    // hour of the local clock.
    let now = utc(2025, 4, 5, 12, 0, 0);
    assert_eq!(
        parse_snooze_until("1d", now),
        Some(utc(2025, 4, 6, 12, 0, 0))
    );
    assert_eq!(
        parse_snooze_until("3d", now),
        Some(utc(2025, 4, 8, 12, 0, 0))
    );
    assert_eq!(
        parse_snooze_until("365d", now),
        Some(utc(2026, 4, 5, 12, 0, 0))
    );
}

#[test]
fn a_snooze_given_as_a_moment_is_read_in_any_rfc_3339_shape() {
    let now = utc(2026, 3, 1, 12, 0, 0);
    let wanted = utc(2026, 3, 9, 15, 0, 0);
    for text in [
        "2026-03-09T15:00:00Z",
        "2026-03-09T15:00:00+00:00",
        "2026-03-09T12:00:00-03:00",
        " 2026-03-09T15:00:00Z ",
    ] {
        assert_eq!(parse_snooze_until(text, now), Some(wanted), "{text:?}");
    }
    assert_eq!(
        parse_snooze_until("2026-03-09T15:00:00.5Z", now),
        Some(wanted + TimeDelta::milliseconds(500))
    );
    for text in [
        "2026-03-09",
        "2026-03-09T15:00:00",
        "tomorrow",
        "Monday 9am",
    ] {
        assert_eq!(parse_snooze_until(text, now), None, "{text:?}");
    }
}

#[test]
fn a_mail_dated_in_the_future_is_refused_naming_the_moment_as_utc_with_a_plus_zero_offset() {
    let tracker = Tracker::in_memory().unwrap();
    let now = utc(2026, 3, 1, 12, 0, 0);
    let mail = |received_at: &str| {
        json!({
            "message_id": "m1",
            "thread_id": "th-m1",
            "from": "Ana <ana@acme.example>",
            "subject": "Hola",
            "received_at": received_at,
            "kind": "other",
            "summary": "x",
        })
    };
    for (received_at, shown) in [
        ("2026-03-05T12:00:00Z", "2026-03-05T12:00:00+00:00"),
        ("2026-03-05T09:00:00-03:00", "2026-03-05T12:00:00+00:00"),
        ("2026-03-05T12:00:00.5Z", "2026-03-05T12:00:00.500+00:00"),
    ] {
        let report = import(&tracker, &[mail(received_at)], now).unwrap();
        assert_eq!(report.rejected, 1, "{received_at}");
        let said = report.lines.join("\n");
        assert!(
            said.contains(&format!("«received_at» está en el futuro ({shown})")),
            "{received_at}: {said}"
        );
    }
}

#[test]
fn the_oldest_and_the_latest_date_a_task_may_have() {
    assert_eq!(earliest_date(), utc(2000, 1, 1, 0, 0, 0));
    // Five years (60 months) from now, to the same day and time.
    assert_eq!(
        latest_due(utc(2026, 3, 1, 12, 0, 0)),
        utc(2031, 3, 1, 12, 0, 0)
    );
    assert_eq!(
        latest_due(utc(2026, 1, 31, 23, 59, 59)),
        utc(2031, 1, 31, 23, 59, 59)
    );
    // A day that the month five years on does not have falls on that month's last day.
    assert_eq!(
        latest_due(utc(2028, 2, 29, 8, 0, 0)),
        utc(2033, 2, 28, 8, 0, 0)
    );
    assert_eq!(
        latest_due(utc(2024, 2, 29, 8, 0, 0)),
        utc(2029, 2, 28, 8, 0, 0)
    );
    assert_eq!(
        latest_due(utc(2026, 8, 31, 0, 0, 0)),
        utc(2031, 8, 31, 0, 0, 0)
    );
    assert_eq!(
        latest_due(utc(2020, 2, 29, 0, 0, 0)),
        utc(2025, 2, 28, 0, 0, 0)
    );
}
