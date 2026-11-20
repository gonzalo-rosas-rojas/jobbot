//! Helpers the tests of several tracker modules share.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use jobbot_domain::fixtures::{NOW, make_job_with};
use jobbot_domain::models::Job;

use super::{EmailKind, NewEmail, NewTask, State, Tracker};

/// Stores `job` and walks its application to `Prepared`.
fn prepare(t: &Tracker, job: &Job) {
    t.add_job(job, 70.0, &BTreeMap::new(), *NOW).unwrap();
    t.start_cold_for_tests(&job.key(), *NOW).unwrap();
    t.transition(&job.key(), State::Prepared, None, *NOW)
        .unwrap();
}

/// Stores `job` and submits it without Gonza's approval.
pub(super) fn submit(t: &Tracker, job: &Job) {
    prepare(t, job);
    t.transition(&job.key(), State::Submitted, None, *NOW)
        .unwrap();
}

/// Stores `job`, has Gonza approve it and submits it.
pub(super) fn submit_approved(t: &Tracker, job: &Job) {
    prepare(t, job);
    t.approve(&job.key()).unwrap();
    t.transition(&job.key(), State::Submitted, None, *NOW)
        .unwrap();
}

pub(super) fn state_of(t: &Tracker, key: &str) -> State {
    t.get_application(key).unwrap().unwrap().state
}

pub(super) fn tracker_with_prepared(ids: &[&str]) -> Tracker {
    let tracker = Tracker::in_memory().unwrap();
    for id in ids {
        let job = make_job_with(|j| {
            j.external_id = (*id).into();
            j.company = format!("Co{id}");
            j.title = format!("Role {id}");
        });
        tracker
            .add_job(&job, 70.0, &Default::default(), *NOW)
            .unwrap();
        tracker.start_cold_for_tests(&job.key(), *NOW).unwrap();
        tracker
            .transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
    }
    tracker
}

/// Three stored, queued postings scored 50, 90 and 70, all added at `now`.
pub(super) fn three_scored(t: &Tracker, now: DateTime<Utc>) {
    for (i, score) in [50.0, 90.0, 70.0].into_iter().enumerate() {
        let job = make_job_with(|j| {
            j.external_id = i.to_string();
            j.company = format!("Co{i}");
            j.title = format!("Role {i}");
        });
        t.add_job(&job, score, &BTreeMap::new(), now).unwrap();
        t.start_cold_for_tests(&job.key(), now).unwrap();
    }
}

pub(super) fn new_email(id: &str, kind: EmailKind, at: DateTime<Utc>) -> NewEmail {
    NewEmail {
        message_id: id.into(),
        thread_id: format!("th-{id}"),
        from_addr: "Ana <ana@acme.example>".into(),
        subject: format!("Asunto {id}"),
        received_at: at,
        job_key: None,
        kind,
        summary: format!("Resumen {id}"),
    }
}

pub(super) fn new_task(text: &str, due: Option<DateTime<Utc>>, source: Option<&str>) -> NewTask {
    NewTask {
        job_key: None,
        kind: "action_required".into(),
        text: text.into(),
        due_at: due,
        source_message_id: source.map(str::to_string),
        link: None,
    }
}

/// Stores job `id` as first seen at `seen`; with `walk`, also its application at `seen`.
pub(super) fn stored_at(
    t: &Tracker,
    id: &str,
    score: f64,
    seen: DateTime<Utc>,
    walk: &[State],
) -> String {
    let job = make_job_with(|j| {
        j.external_id = id.into();
        j.company = format!("Co {id}");
        j.title = format!("Role {id}");
    });
    t.add_job(&job, score, &BTreeMap::new(), seen).unwrap();
    if !walk.is_empty() {
        t.start_cold_for_tests(&job.key(), seen).unwrap();
        for state in walk {
            t.transition(&job.key(), *state, None, seen).unwrap();
        }
    }
    job.key()
}
