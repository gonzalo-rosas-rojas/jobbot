//! The `digest` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/digest.rs`).

use std::collections::BTreeMap;
use std::path::Path;

use chrono::TimeDelta;

use jobbot_app::reporting::digest::*;
use jobbot_domain::models::Job;
use jobbot_domain::state::State;
use jobbot_testkit::Tracker;
use jobbot_testkit::{NOW, make_job, make_job_with};

/// Adds a job, creates its application and walks it through `states`.
fn seed(t: &Tracker, job: &Job, states: &[State]) {
    t.add_job(job, 80.0, &BTreeMap::new(), *NOW).unwrap();
    t.create_application(&job.key(), *NOW).unwrap();
    for state in states {
        t.transition(&job.key(), *state, None, *NOW).unwrap();
    }
}
#[test]
fn digest_sections_attribution_and_privacy_of_rejections() {
    let t = Tracker::in_memory().unwrap();
    let new = make_job_with(|j| {
        j.source = "remoteok".into();
        j.external_id = "a".into();
        j.company = "NewCo".into();
        j.title = "New Role".into();
        j.url = "https://remoteok.com/remote-jobs/a".into();
    });
    let rejected = make_job_with(|j| {
        j.external_id = "b".into();
        j.company = "NoCo".into();
        j.title = "Rejected Role".into();
    });
    let interview = make_job_with(|j| {
        j.external_id = "c".into();
        j.company = "YesCo".into();
        j.title = "Interview Role".into();
    });
    let needy = make_job_with(|j| {
        j.external_id = "d".into();
        j.company = "NeedCo".into();
        j.title = "Needy Role".into();
    });
    seed(&t, &new, &[]);
    seed(
        &t,
        &rejected,
        &[State::Prepared, State::Submitted, State::Rejected],
    );
    seed(
        &t,
        &interview,
        &[State::Prepared, State::Submitted, State::Interview],
    );
    seed(&t, &needy, &[]);
    t.transition(&needy.key(), State::NeedsGonza, Some("falta email"), *NOW)
        .unwrap();

    let text = build_digest(&t, *NOW, 5).unwrap();
    assert!(text.contains("Resumen diario — 2026-10-01"), "{text}");
    assert!(
        text.contains("falta email") && text.contains("Interview Role"),
        "{text}"
    );
    // Attribution and the link back to the source.
    assert!(
        text.contains("remoteok: https://remoteok.com/remote-jobs/a"),
        "{text}"
    );
    // Rejections are logged, not pushed.
    assert!(!text.contains("Rejected Role"), "{text}");
    assert!(
        text.contains("- Enviadas: 2 · Respuestas: 2 · Entrevistas: 1\n"),
        "{text}"
    );
    assert!(
        text.contains("- De las enviadas esta semana ya respondieron: 2 (100%)\n"),
        "{text}"
    );
}
#[test]
fn an_application_that_replied_and_then_interviewed_counts_as_one_response() {
    let t = Tracker::in_memory().unwrap();
    seed(
        &t,
        &make_job(),
        &[
            State::Prepared,
            State::Submitted,
            State::Replied,
            State::Interview,
        ],
    );
    let text = build_digest(&t, *NOW, 5).unwrap();
    // Two events (Replied, Interview) on one application: one response, and never over 100 %.
    assert!(
        text.contains("- Enviadas: 1 · Respuestas: 1 · Entrevistas: 1\n"),
        "{text}"
    );
    assert!(
        text.contains("- De las enviadas esta semana ya respondieron: 1 (100%)\n"),
        "{text}"
    );
}
#[test]
fn a_reply_to_an_old_application_counts_as_a_response_but_not_in_the_weekly_rate() {
    let t = Tracker::in_memory().unwrap();
    let old = make_job_with(|j| j.external_id = "old".into());
    let fresh = make_job_with(|j| j.external_id = "fresh".into());
    let long_ago = *NOW - TimeDelta::days(30);
    // Sent a month ago, answered this week.
    t.add_job(&old, 80.0, &BTreeMap::new(), long_ago).unwrap();
    t.create_application(&old.key(), long_ago).unwrap();
    t.transition(&old.key(), State::Prepared, None, long_ago)
        .unwrap();
    t.transition(&old.key(), State::Submitted, None, long_ago)
        .unwrap();
    t.transition(&old.key(), State::Replied, None, *NOW)
        .unwrap();
    // Sent this week, still waiting.
    seed(&t, &fresh, &[State::Prepared, State::Submitted]);
    let text = build_digest(&t, *NOW, 5).unwrap();
    assert!(
        text.contains("- Enviadas: 1 · Respuestas: 1 · Entrevistas: 0\n"),
        "{text}"
    );
    assert!(
        text.contains("- De las enviadas esta semana ya respondieron: 0 (0%)\n"),
        "{text}"
    );
}
#[test]
fn digest_lists_applications_waiting_for_approval() {
    let t = Tracker::in_memory().unwrap();
    let job = make_job();
    seed(&t, &job, &[State::Prepared]);
    t.set_screenshot(&job.key(), Path::new("data/screenshots/x.png"))
        .unwrap();
    let text = build_digest(&t, *NOW, 5).unwrap();
    assert!(
        text.contains("Por aprobar (1)") && text.contains("jobbot approve test:1"),
        "{text}"
    );
}
#[test]
fn empty_digest_is_still_valid() {
    let text = build_digest(&Tracker::in_memory().unwrap(), *NOW, 5).unwrap();
    assert!(
        text.contains("Nada pendiente.") && text.contains("n/a"),
        "{text}"
    );
}
#[test]
fn top_section_respects_top_n() {
    let t = Tracker::in_memory().unwrap();
    for id in ["1", "2", "3"] {
        seed(&t, &make_job_with(|j| j.external_id = id.into()), &[]);
    }
    let text = build_digest(&t, *NOW, 2).unwrap();
    assert!(text.contains("Nuevas coincidencias (top 2)"), "{text}");
    assert_eq!(text.matches("- [80]").count(), 2, "{text}");
}
#[test]
fn the_digest_reports_claude_calls_by_stage_for_the_last_day() {
    let t = Tracker::in_memory().unwrap();
    t.record_claude_call("dossier", *NOW, 0.05, 1, 1, true)
        .unwrap();
    t.record_claude_call("dossier", *NOW - TimeDelta::hours(2), 0.07, 1, 1, true)
        .unwrap();
    t.record_claude_call("dossier", *NOW - TimeDelta::hours(30), 9.0, 1, 1, true)
        .unwrap();
    let text = build_digest(&t, *NOW, 5).unwrap();
    assert!(
        text.contains("## Claude (últimas 24 h)")
            && text.contains("- dossier: 2 llamadas · US$0.12"),
        "{text}"
    );
}
#[test]
fn the_digest_says_when_claude_was_not_used() {
    let text = build_digest(&Tracker::in_memory().unwrap(), *NOW, 5).unwrap();
    assert!(
        text.contains("## Claude (últimas 24 h)\n- Sin llamadas."),
        "{text}"
    );
}
