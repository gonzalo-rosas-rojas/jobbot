//! What the tests of `snapshot` and its children share: a board column lookup, builders of tasks
//! and emails, postings first seen at a given moment, profile folders and the shipped criteria.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::*;
use jobbot_app::ports::store::{EmailKind, NewEmail, NewTask};
use jobbot_app::profile::load_criteria;
use jobbot_testkit::{NOW, make_job_with, real_paths};

pub(super) fn column<'a>(snap: &'a Snapshot, state: &str) -> &'a Column {
    snap.board.iter().find(|c| c.state == state).unwrap()
}

pub(super) fn task(panel: &Panel, text: &str, due: Option<DateTime<Utc>>, job_key: Option<&str>) {
    panel
        .tracker
        .add_task(
            &NewTask {
                job_key: job_key.map(str::to_string),
                kind: "action_required".into(),
                text: text.into(),
                due_at: due,
                source_message_id: None,
                link: None,
            },
            *NOW,
        )
        .unwrap();
}

pub(super) fn email(panel: &Panel, id: &str, kind: EmailKind, at: DateTime<Utc>, subject: &str) {
    panel
        .tracker
        .add_email(
            &NewEmail {
                message_id: id.into(),
                thread_id: format!("th-{id}"),
                from_addr: "Ana <ana@acme.example>".into(),
                subject: subject.into(),
                received_at: at,
                job_key: None,
                kind,
                summary: format!("Resumen {id}"),
            },
            None,
            *NOW,
        )
        .unwrap();
}

/// Stores job `id` as first seen at `seen`. With `walk`, an application is made at that
/// moment and walked through those states; `None` leaves the posting without one.
pub(super) fn seen_at(
    panel: &Panel,
    id: &str,
    score: f64,
    seen: DateTime<Utc>,
    walk: Option<&[State]>,
) -> String {
    let job = make_job_with(|j| {
        j.external_id = id.into();
        j.company = format!("Co {id}");
        j.title = format!("Role {id}");
    });
    panel
        .tracker
        .add_job(&job, score, &BTreeMap::new(), seen)
        .unwrap();
    if let Some(walk) = walk {
        panel.tracker.create_application(&job.key(), seen).unwrap();
        for state in walk {
            panel
                .tracker
                .transition(&job.key(), *state, None, seen)
                .unwrap();
        }
    }
    job.key()
}

/// A profile folder inside `dir` holding only the files given.
pub(super) fn profile_with(dir: &Path, criteria: Option<&str>, schedule: Option<&str>) -> PathBuf {
    let folder = dir.join("profile-under-test");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    if let Some(text) = criteria {
        std::fs::write(folder.join("criteria.toml"), text).unwrap();
    }
    if let Some(text) = schedule {
        std::fs::write(folder.join("schedule.toml"), text).unwrap();
    }
    folder
}

pub(super) fn shipped_criteria() -> jobbot_domain::rules::Criteria {
    load_criteria(&real_paths().profile_dir.join("criteria.toml")).unwrap()
}
