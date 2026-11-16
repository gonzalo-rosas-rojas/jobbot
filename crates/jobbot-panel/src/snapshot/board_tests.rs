//! Tests of the board, of the two queues of what needs Gonza and of the snoozing that takes cards
//! off them: each builds the whole snapshot and reads its lists.

use std::path::Path;

use chrono::TimeDelta;

use super::support_tests::{column, task};
use super::*;
use crate::fixtures::{add, panel};
use jobbot_testkit::NOW;

#[test]
fn a_capture_that_is_no_longer_on_disk_offers_no_approval() {
    let (panel, dir) = panel();
    let key = add(&panel, "1", 70.0, &[State::Prepared]);
    panel
        .tracker
        .set_screenshot(&key, &dir.path().join("gone.png"))
        .unwrap();
    let snap = build_snapshot(&panel).unwrap();
    assert_eq!(snap.awaiting_approval.total, 0, "nothing to review");
    let card = &column(&snap, "prepared").cards[0];
    assert!(
        !card.has_screenshot && card.actions.is_empty(),
        "{:?}",
        card.actions
    );
    // The file comes back (or is saved again): now there is something to approve.
    std::fs::write(dir.path().join("gone.png"), b"png").unwrap();
    let snap = build_snapshot(&panel).unwrap();
    assert_eq!(snap.awaiting_approval.total, 1);
    assert_eq!(column(&snap, "prepared").cards[0].actions, vec!["approve"]);
}

#[test]
fn the_queue_lists_what_needs_gonza_and_what_awaits_his_approval() {
    let (panel, dir) = panel();
    let needy = add(&panel, "1", 80.0, &[State::NeedsGonza]);
    let review = add(&panel, "2", 70.0, &[State::Prepared]);
    let shot = dir.path().join("shot.png");
    std::fs::write(&shot, b"png").unwrap();
    panel.tracker.set_screenshot(&review, &shot).unwrap();

    let snap = build_snapshot(&panel).unwrap();
    let card = &snap.needs_gonza.cards[0];
    assert_eq!(card.key, needy);
    assert_eq!(card.reason.as_deref(), Some("falta email"));
    assert_eq!(card.actions, vec!["resolve", "confirm"]);
    let waiting = &snap.awaiting_approval.cards[0];
    assert_eq!(waiting.key, review);
    assert_eq!(waiting.actions, vec!["approve"]);
    assert!(waiting.has_screenshot && !waiting.approved);

    panel.tracker.approve(&review).unwrap();
    let snap = build_snapshot(&panel).unwrap();
    assert_eq!(
        snap.awaiting_approval.total, 0,
        "approved: no longer waiting"
    );
    assert!(column(&snap, "prepared").cards[0].actions.is_empty());
}

#[test]
fn a_long_queue_is_capped_and_sorted_by_score() {
    let (panel, _dir) = panel();
    for n in 1..=60 {
        add(&panel, &n.to_string(), f64::from(n), &[]);
        add(&panel, &format!("n{n}"), f64::from(n), &[State::NeedsGonza]);
    }
    let snap = build_snapshot(&panel).unwrap();
    let scored = column(&snap, "scored");
    assert_eq!(scored.total, 60);
    assert_eq!(scored.cards.len(), COLUMN_CAP);
    assert_eq!(scored.cards[0].score, 60);
    assert!(scored.cards.windows(2).all(|w| w[0].score >= w[1].score));
    assert_eq!(snap.needs_gonza.total, 60);
    assert_eq!(snap.needs_gonza.cards.len(), QUEUE_CAP);
}

#[test]
fn a_snoozed_application_past_its_columns_cards_is_still_listed_as_snoozed() {
    let (panel, _dir) = panel();
    for n in 1..=30 {
        add(&panel, &n.to_string(), f64::from(50 + n), &[]);
    }
    let last = add(&panel, "low", 1.0, &[]);
    panel
        .tracker
        .set_snoozed_until(&last, Some(*NOW + TimeDelta::days(2)))
        .unwrap();
    let snap = build_snapshot(&panel).unwrap();
    let scored = column(&snap, "scored");
    assert_eq!((scored.total, scored.cards.len()), (31, COLUMN_CAP));
    assert!(scored.cards.iter().all(|c| c.key != last), "past the cap");
    let snoozed: Vec<&str> = snap.snoozed.cards.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(snoozed, [last.as_str()]);
}

#[test]
fn every_application_in_progress_is_listed_however_many_there_are() {
    let (panel, _dir) = panel();
    for n in 1..=COLUMN_CAP + 15 {
        add(
            &panel,
            &n.to_string(),
            f64::from(n as u32),
            &[State::Prepared, State::Submitted],
        );
    }
    let snap = build_snapshot(&panel).unwrap();
    let sent = column(&snap, "submitted");
    assert_eq!(sent.total, COLUMN_CAP + 15);
    assert_eq!(
        sent.cards.len(),
        COLUMN_CAP + 15,
        "the Procesos tab draws one row per live application: none may be cut"
    );
}

#[test]
fn a_snoozed_application_leaves_the_queues_and_today_until_its_time() {
    let (panel, _dir) = panel();
    let t = &panel.tracker;
    let waiting = add(&panel, "1", 80.0, &[State::NeedsGonza]);
    let other = add(&panel, "2", 70.0, &[State::NeedsGonza]);
    let proposed = add(&panel, "3", 90.0, &[State::Prepared]);
    t.set_screenshot(&proposed, Path::new("shot.png")).unwrap();
    task(&panel, "Postular", None, Some(&waiting));
    task(&panel, "Otra", None, Some(&other));
    task(&panel, "Suelta", None, None);
    t.set_snoozed_until(&waiting, Some(*NOW + TimeDelta::days(1)))
        .unwrap();
    t.set_snoozed_until(&proposed, Some(*NOW + TimeDelta::days(3)))
        .unwrap();
    // A snooze whose time has passed is over, even if nobody cleared it.
    t.set_snoozed_until(&other, Some(*NOW - TimeDelta::hours(1)))
        .unwrap();

    let snap = build_snapshot(&panel).unwrap();

    assert_eq!(snap.needs_gonza.total, 1);
    assert_eq!(snap.needs_gonza.cards[0].key, other);
    assert!(snap.needs_gonza.cards[0].snoozed_until.is_none());
    assert_eq!(snap.awaiting_approval.total, 0);
    let snoozed: Vec<(&str, Option<&str>)> = snap
        .snoozed
        .cards
        .iter()
        .map(|c| (c.key.as_str(), c.snoozed_until.as_deref()))
        .collect();
    assert_eq!(
        snoozed,
        [
            (waiting.as_str(), Some("2026-10-02T12:00:00Z")),
            (proposed.as_str(), Some("2026-10-04T12:00:00Z")),
        ],
        "soonest back first"
    );
    assert_eq!(snap.snoozed.total, 2);
    assert_eq!(
        column(&snap, "needs_gonza").total,
        2,
        "the board keeps everything"
    );
    let texts: Vec<&str> = snap.today.tasks.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(
        texts,
        ["Otra", "Suelta"],
        "a task about a put-off application waits with it"
    );
    assert_eq!(snap.today.tasks_total, 2);

    t.set_snoozed_until(&waiting, None).unwrap();
    let snap = build_snapshot(&panel).unwrap();
    assert_eq!(snap.needs_gonza.total, 2);
    assert_eq!(snap.snoozed.total, 1);
    assert_eq!(snap.today.tasks_total, 3);
}
