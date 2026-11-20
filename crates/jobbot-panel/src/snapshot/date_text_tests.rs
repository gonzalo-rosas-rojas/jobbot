//! The text of every date the snapshot hands to the page, written out as literals.
//!
//! The page reads `2026-03-01T12:00:00Z` (UTC, a `Z`, no fraction of a second) and compares those
//! strings itself, so the characters are part of the contract. Instants come in with fractions of a
//! second (`Utc::now()` has them) and with the offset the engine wrote them with; these tests fix
//! what comes out.

use chrono::{DateTime, TimeDelta, Utc};
use jobbot_domain::state::State;
use jobbot_testkit::{PrepStore, Tracker, make_job_with, utc_at};
use serde_json::Value;

use super::support_tests::{email, profile_with};
use super::{build_job_detail, build_snapshot};
use crate::Panel;
use crate::scheduler::{FixedTask, TaskAnswer};
use jobbot_app::paths::Paths;
use jobbot_app::ports::store::{EmailKind, NewTask};

/// 2026-03-01 `h:mi:s` UTC plus `nanos` nanoseconds.
fn at(h: u32, mi: u32, s: u32, nanos: i64) -> DateTime<Utc> {
    utc_at(2026, 3, 1, h, mi, s) + TimeDelta::nanoseconds(nanos)
}

/// A panel over fresh in-memory databases whose clock stands at `now`. Its profile is an empty
/// folder of its own, never the user's.
fn panel_at(now: DateTime<Utc>) -> (Panel, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut paths = Paths::new(dir.path());
    paths.profile_dir = profile_with(dir.path(), None, None);
    let panel = Panel::new(
        paths,
        Tracker::in_memory().unwrap(),
        PrepStore::in_memory().unwrap(),
        Box::new(move || now),
    );
    (panel, dir)
}

fn snapshot(panel: &Panel) -> Value {
    serde_json::to_value(build_snapshot(panel).unwrap()).unwrap()
}

/// The card of `key` wherever the board has it.
fn card<'a>(snapshot: &'a Value, key: &str) -> &'a Value {
    snapshot["board"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|column| column["cards"].as_array().unwrap())
        .find(|card| card["key"] == key)
        .unwrap_or_else(|| panic!("no card for {key}"))
}

#[test]
fn the_snapshot_says_when_it_was_made_in_utc_with_a_z_and_no_fraction() {
    for (now, text) in [
        (at(12, 0, 0, 0), "2026-03-01T12:00:00Z"),
        (at(12, 0, 0, 500_000_000), "2026-03-01T12:00:00Z"),
        (at(12, 0, 0, 123_456), "2026-03-01T12:00:00Z"),
        (at(12, 0, 0, 123_456_789), "2026-03-01T12:00:00Z"),
        (at(11, 59, 59, 999_999_999), "2026-03-01T11:59:59Z"),
    ] {
        let (panel, _dir) = panel_at(now);
        assert_eq!(snapshot(&panel)["generated_at"], text, "{now:?}");
    }
}

/// A panel whose clock stands at 15:30:00.75, and an application of `id` in it first seen at
/// 11:00:00.25 (published `posted`), created at that moment. Returns the application's key.
fn panel_with_application(
    id: &str,
    posted: Option<DateTime<Utc>>,
) -> (Panel, tempfile::TempDir, String) {
    let (panel, dir) = panel_at(at(15, 30, 0, 750_000_000));
    let job = make_job_with(|j| {
        j.external_id = id.into();
        j.company = format!("Co {id}");
        j.title = format!("Role {id}");
        j.posted_at = posted;
    });
    panel
        .tracker
        .add_job(&job, 70.0, &Default::default(), at(11, 0, 0, 250_000_000))
        .unwrap();
    panel
        .tracker
        .start_application(
            &job.key(),
            jobbot_domain::origin::Origin::Cold,
            at(11, 0, 0, 250_000_000),
        )
        .unwrap();
    let key = job.key();
    (panel, dir, key)
}

#[test]
fn a_card_waiting_for_gonza_shows_its_dates_in_utc_with_a_z_and_no_fraction() {
    // Posted the 27th of February at 08:15:30.5; waiting for Gonza since 12:00:00.9.
    let posted = utc_at(2026, 2, 27, 8, 15, 30) + TimeDelta::milliseconds(500);
    let (panel, _dir, key) = panel_with_application("waiting", Some(posted));
    for state in [State::Prepared, State::NeedsGonza] {
        panel
            .tracker
            .transition(&key, state, Some("falta email"), at(12, 0, 0, 900_000_000))
            .unwrap();
    }

    let snap = snapshot(&panel);

    let waiting = card(&snap, &key);
    assert_eq!(waiting["updated_at"], "2026-03-01T12:00:00Z");
    assert_eq!(waiting["waiting_since"], "2026-03-01T12:00:00Z");
    assert_eq!(waiting["first_seen"], "2026-03-01T11:00:00Z");
    assert_eq!(waiting["posted_at"], "2026-02-27T08:15:30Z");
    assert_eq!(
        snap["needs_gonza"]["cards"][0]["waiting_since"],
        "2026-03-01T12:00:00Z"
    );
}

#[test]
fn a_snoozed_card_shows_when_it_comes_back_in_utc_with_a_z_and_no_fraction() {
    let (panel, _dir, key) = panel_with_application("snoozed", None);
    // Put off until the 3rd at 09:00:00.1.
    panel
        .tracker
        .set_snoozed_until(
            &key,
            Some(utc_at(2026, 3, 3, 9, 0, 0) + TimeDelta::milliseconds(100)),
        )
        .unwrap();

    let snap = snapshot(&panel);

    assert_eq!(card(&snap, &key)["snoozed_until"], "2026-03-03T09:00:00Z");
    assert_eq!(
        snap["snoozed"]["cards"][0]["snoozed_until"],
        "2026-03-03T09:00:00Z"
    );
}

#[test]
fn a_card_marked_by_hand_offers_its_undo_in_utc_ten_minutes_after_the_stored_second() {
    let (panel, _dir, key) = panel_with_application("marked", None);
    // A mark made by hand at 15:25:00.6, still inside its ten minutes (the clock is 15:30:00.75).
    panel
        .tracker
        .transition(&key, State::Prepared, None, at(11, 0, 0, 0))
        .unwrap();
    panel
        .tracker
        .mark_transition(&key, State::Withdrawn, None, at(15, 25, 0, 600_000_000))
        .unwrap();

    let snap = snapshot(&panel);
    let undo = &card(&snap, &key)["undo"];

    assert_eq!(undo["at"], "2026-03-01T15:25:00Z");
    assert_eq!(undo["until"], "2026-03-01T15:35:00Z");
}

#[test]
fn the_days_a_card_has_waited_are_whole_days_of_the_stored_seconds() {
    // Posted at 12:00:00.9 is stored as 12:00:00: a day later at 12:00:00.1 a whole day has gone.
    for (now, age_days) in [
        (at(11, 59, 59, 900_000_000) + TimeDelta::days(1), 0),
        (at(12, 0, 0, 100_000_000) + TimeDelta::days(1), 1),
        (at(12, 0, 0, 0) - TimeDelta::hours(1), 0),
    ] {
        let (panel, _dir) = panel_at(now);
        let job = make_job_with(|j| j.posted_at = Some(at(12, 0, 0, 900_000_000)));
        panel
            .tracker
            .add_job(&job, 70.0, &Default::default(), at(12, 0, 0, 900_000_000))
            .unwrap();
        panel
            .tracker
            .start_application(
                &job.key(),
                jobbot_domain::origin::Origin::Cold,
                at(12, 0, 0, 900_000_000),
            )
            .unwrap();
        let snap = snapshot(&panel);
        let found = card(&snap, &job.key());
        assert_eq!(found["age_days"], age_days, "{now:?}");
        assert_eq!(found["days_in_state"], age_days, "{now:?}");
    }
}

#[test]
fn the_detail_of_an_application_lists_its_history_with_z_text_and_no_fraction() {
    let (panel, _dir) = panel_at(at(15, 0, 0, 0));
    let job = make_job_with(|j| j.external_id = "1".into());
    panel
        .tracker
        .add_job(&job, 70.0, &Default::default(), at(9, 0, 0, 0))
        .unwrap();
    panel
        .tracker
        .start_application(
            &job.key(),
            jobbot_domain::origin::Origin::Cold,
            at(9, 0, 0, 5),
        )
        .unwrap();
    panel
        .tracker
        .transition(
            &job.key(),
            State::Prepared,
            None,
            at(10, 30, 0, 999_999_999),
        )
        .unwrap();
    let detail =
        serde_json::to_value(build_job_detail(&panel, &job.key()).unwrap().unwrap()).unwrap();
    let times: Vec<&str> = detail["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["at"].as_str().unwrap())
        .collect();
    assert_eq!(times, ["2026-03-01T09:00:00Z", "2026-03-01T10:30:00Z"]);
    assert_eq!(detail["first_seen"], "2026-03-01T09:00:00Z");
}

#[test]
fn today_lists_the_dates_of_tasks_and_mail_in_utc_with_a_z_and_no_fraction() {
    let (panel, _dir) = panel_at(at(12, 0, 0, 0));
    let task = NewTask {
        job_key: None,
        kind: "manual".into(),
        text: "Llamar".into(),
        due_at: Some(at(18, 0, 0, 999_999_999)),
        source_message_id: None,
        link: None,
    };
    panel
        .tracker
        .add_task(&task, at(9, 15, 0, 400_000_000))
        .unwrap();
    email(
        &panel,
        "m1",
        EmailKind::Recruiter,
        at(10, 45, 30, 800_000_000),
        "Hola",
    );

    let today = &snapshot(&panel)["today"];

    assert_eq!(today["tasks"][0]["due_at"], "2026-03-01T18:00:00Z");
    assert_eq!(today["tasks"][0]["created_at"], "2026-03-01T09:15:00Z");
    assert_eq!(today["emails"][0]["received_at"], "2026-03-01T10:45:30Z");
}

#[test]
fn the_time_of_the_last_mail_import_reaches_the_page_exactly_as_the_importer_wrote_it() {
    // `meta.inbox_imported_at` is not parsed on the way: the page gets the stored text, so the
    // importer's own format (an offset, a fraction) is visible there.
    let (panel, _dir) = panel_at(at(12, 0, 0, 0));
    for stored in [
        "2026-03-01T11:00:00+00:00",
        "2026-03-01T11:00:00.123456789+00:00",
        "2026-03-01T11:00:00Z",
    ] {
        panel
            .tracker
            .set_meta(jobbot_app::inbox::INBOX_IMPORTED_AT, stored)
            .unwrap();
        assert_eq!(snapshot(&panel)["today"]["inbox"]["imported_at"], stored);
    }
}

#[test]
fn the_last_run_is_read_in_any_rfc_3339_shape_and_shown_in_utc_with_a_z_and_no_fraction() {
    let (panel, _dir) = panel_at(at(12, 0, 0, 0));
    for stored in [
        "2026-03-01T08:30:15+00:00",
        "2026-03-01T08:30:15.987654321+00:00",
        "2026-03-01T08:30:15Z",
        "2026-03-01T05:30:15-03:00",
        "2026-03-01T11:30:15+03:00",
    ] {
        panel.tracker.set_meta("last_run_at", stored).unwrap();
        assert_eq!(
            snapshot(&panel)["controls"]["last_run_at"],
            "2026-03-01T08:30:15Z",
            "{stored}"
        );
    }
    // Discovery's own stamp counts when no run has written one, and a stamp that is no date is ignored.
    panel.tracker.set_meta("last_run_at", "ayer").unwrap();
    panel
        .tracker
        .set_meta("last_discover_at", "2026-03-01T07:00:00.5+00:00")
        .unwrap();
    assert_eq!(
        snapshot(&panel)["controls"]["last_run_at"],
        "2026-03-01T07:00:00Z"
    );
}

#[test]
fn the_next_window_is_santiagos_clock_in_summer_time_and_in_winter_time() {
    let schedule = r#"
        [[busy]]
        zone = "America/Santiago"
        start = "09:00"
        end = "17:00"
        days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
    "#;
    // (now, the first free minute as Santiago shows it)
    let cases = [
        // Summer (UTC-3): 12:00 in Santiago, busy until 17:00 = 20:00Z.
        (at(15, 0, 0, 0), "2026-03-01 17:00"),
        // Winter (UTC-4): 11:00 in Santiago, busy until 17:00 = 21:00Z.
        (utc_at(2026, 6, 1, 15, 0, 0), "2026-06-01 17:00"),
        // The Saturday before the clocks go forward in 2025 (September 6 at 24:00): 10:00 local.
        (utc_at(2025, 9, 6, 14, 0, 0), "2025-09-06 17:00"),
        // The Sunday after the change: 09:00 local at UTC-3 is 12:00Z.
        (utc_at(2025, 9, 7, 12, 0, 0), "2025-09-07 17:00"),
    ];
    for (now, expected) in cases {
        let (mut panel, dir) = panel_at(now);
        panel.paths.profile_dir = profile_with(dir.path(), None, Some(schedule));
        let controls = &snapshot(&panel)["controls"];
        assert_eq!(controls["window_open"], false, "{now:?}");
        assert_eq!(controls["next_window"], expected, "{now:?}");
    }
}

#[test]
fn the_next_run_of_the_scheduled_task_is_shown_in_utc_with_a_z_and_no_fraction() {
    let (panel, _dir) = panel_at(at(12, 0, 0, 0));
    let panel = panel.with_task_query(FixedTask(Some(TaskAnswer::Registered {
        next_run_at: Some(at(19, 0, 0, 123_456_789)),
    })));
    panel.tasks.settle();
    assert_eq!(
        snapshot(&panel)["controls"]["scheduled_task"]["next_run_at"],
        "2026-03-01T19:00:00Z"
    );
}

#[test]
fn the_activity_is_cut_in_half_hours_of_the_utc_clock_whatever_fraction_now_has() {
    let (panel, _dir) = panel_at(at(12, 29, 59, 999_999_999));
    let key = {
        let job = make_job_with(|j| j.external_id = "1".into());
        panel
            .tracker
            .add_job(&job, 70.0, &Default::default(), at(12, 10, 0, 500_000_000))
            .unwrap();
        panel
            .tracker
            .start_application(
                &job.key(),
                jobbot_domain::origin::Origin::Cold,
                at(12, 10, 0, 500_000_000),
            )
            .unwrap();
        job.key()
    };
    let activity = &snapshot(&panel)["activity"];
    assert_eq!(activity["slots"][47]["at"], "2026-03-01T12:00:00Z");
    assert_eq!(activity["slots"][47]["seen"], 1);
    assert_eq!(activity["slots"][0]["at"], "2026-02-28T12:30:00Z");
    assert_eq!(activity["from"], "2026-02-28T12:30:00Z");
    assert_eq!(activity["to"], "2026-03-01T12:30:00Z");
    assert_eq!(activity["moves"][0]["at"], "2026-03-01T12:10:00Z");
    assert_eq!(activity["moves"][0]["key"], key);
}
