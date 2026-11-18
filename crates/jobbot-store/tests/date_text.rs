//! The text of every date the store writes and reads, written out as literals.
//!
//! The tracker keeps instants as UTC text and compares them as text in SQL (`at >= ?1`,
//! `ORDER BY first_seen`, `MAX(e.at)`). So the exact characters of that text are part of the
//! behaviour: `2026-03-01T12:00:00Z`, with `Z`, with no fraction of a second. These tests read the
//! columns back with a second, plain SQLite connection and compare them with literals, so they
//! keep meaning the same whatever library turns an instant into text.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, TimeDelta, TimeZone, Utc};
use jobbot_domain::fixtures::make_job_with;
use jobbot_domain::models::Job;
use jobbot_domain::privacy::Pii;
use jobbot_domain::state::State;
use jobbot_domain::text::normalize;
use jobbot_store::prep::PrepStore;
use jobbot_store::tracker::{
    EmailAdded, EmailKind, NewEmail, NewTask, StoredPage, Tracker, UndoRefusal,
};
use rusqlite::Connection;

/// 2026-03-01 `h:mi:s` UTC plus `nanos` nanoseconds.
fn march_first(h: u32, mi: u32, s: u32, nanos: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 1, h, mi, s).unwrap() + TimeDelta::nanoseconds(nanos)
}

/// The fractions of a second an instant can arrive with: `Utc::now()` carries them on Windows.
const FRACTIONS: [(i64, &str); 4] = [
    (0, "whole"),
    (500_000_000, "milliseconds"),
    (123_456_000, "microseconds"),
    (123_456_789, "nanoseconds"),
];

/// A database file, the tracker over it and a plain connection to read what it wrote.
struct Db {
    _dir: tempfile::TempDir,
    path: PathBuf,
    tracker: Tracker,
}

impl Db {
    fn new() -> Db {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        let tracker = Tracker::open(&path).unwrap();
        Db {
            _dir: dir,
            path,
            tracker,
        }
    }

    /// The first row of `sql`, every column as text (`None` for NULL).
    fn row(&self, sql: &str) -> Vec<Option<String>> {
        let conn = Connection::open(&self.path).unwrap();
        conn.query_row(sql, [], |row| {
            (0..row.as_ref().column_count())
                .map(|i| row.get::<_, Option<String>>(i))
                .collect()
        })
        .unwrap()
    }

    /// The one column of `sql`, as text.
    fn column(&self, sql: &str) -> Vec<String> {
        let conn = Connection::open(&self.path).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn execute(&self, sql: &str, text: &str) {
        let conn = Connection::open(&self.path).unwrap();
        conn.execute(sql, [text]).unwrap();
    }

    /// Stores a posting seen at `seen` that says it was published at `posted`.
    fn add_job(
        &self,
        id: &str,
        company: &str,
        seen: DateTime<Utc>,
        posted: Option<DateTime<Utc>>,
    ) -> Job {
        let job = make_job_with(|j| {
            j.external_id = id.into();
            j.company = company.into();
            j.title = format!("Role {id}");
            j.posted_at = posted;
        });
        self.tracker
            .add_job(&job, 70.0, &BTreeMap::new(), seen)
            .unwrap();
        job
    }

    /// An application of `job` moved to `Submitted` at `at`.
    fn submit(&self, job: &Job, at: DateTime<Utc>) {
        self.tracker.create_application(&job.key(), at).unwrap();
        for state in [State::Prepared, State::Submitted] {
            self.tracker
                .transition(&job.key(), state, None, at)
                .unwrap();
        }
    }

    /// An application of `job` moved to `Submitted` by hand at `at`, so it can be taken back.
    fn mark_submitted(&self, job: &Job, at: DateTime<Utc>) {
        self.tracker.create_application(&job.key(), at).unwrap();
        self.tracker
            .transition(&job.key(), State::Prepared, None, at)
            .unwrap();
        self.tracker
            .mark_transition(&job.key(), State::Submitted, None, at)
            .unwrap();
    }
}

fn text(value: &str) -> Option<String> {
    Some(value.to_string())
}

// ---- what is written

#[test]
fn a_posting_stores_its_two_dates_as_z_text_with_no_fraction_whatever_the_fraction_was() {
    let db = Db::new();
    for (i, (nanos, _)) in FRACTIONS.iter().enumerate() {
        let when = march_first(12, 0, 0, *nanos);
        db.add_job(&i.to_string(), "Acme", when, Some(when));
    }
    assert_eq!(
        db.column("SELECT first_seen FROM jobs ORDER BY key"),
        vec!["2026-03-01T12:00:00Z"; 4]
    );
    assert_eq!(
        db.column("SELECT posted_at FROM jobs ORDER BY key"),
        vec!["2026-03-01T12:00:00Z"; 4]
    );
}

#[test]
fn a_fraction_is_cut_off_never_rounded_up_to_the_next_second() {
    let db = Db::new();
    let nearly_noon = march_first(11, 59, 59, 999_999_999);
    db.add_job("1", "Acme", nearly_noon, Some(nearly_noon));
    assert_eq!(
        db.row("SELECT first_seen, posted_at FROM jobs"),
        vec![text("2026-03-01T11:59:59Z"), text("2026-03-01T11:59:59Z")]
    );
}

#[test]
fn an_application_stores_every_stamp_it_has_as_z_text_with_no_fraction() {
    let db = Db::new();
    let when = march_first(12, 0, 0, 123_456_789);
    let job = db.add_job("1", "Acme", when, None);
    db.submit(&job, when);
    db.tracker.record_attempt(&job.key(), when).unwrap();
    db.tracker
        .set_snoozed_until(&job.key(), Some(march_first(13, 0, 0, 5)))
        .unwrap();
    assert_eq!(
        db.row("SELECT updated_at, submitted_at, attempted_at, snoozed_until FROM applications"),
        vec![
            text("2026-03-01T12:00:00Z"),
            text("2026-03-01T12:00:00Z"),
            text("2026-03-01T12:00:00Z"),
            text("2026-03-01T13:00:00Z"),
        ]
    );
    assert_eq!(
        db.column("SELECT at FROM events ORDER BY id"),
        vec!["2026-03-01T12:00:00Z"; 3]
    );
}

#[test]
fn mail_and_tasks_store_their_dates_as_z_text_with_no_fraction() {
    let db = Db::new();
    let when = march_first(12, 0, 0, 500_000_000);
    let email = NewEmail {
        message_id: "m1".into(),
        thread_id: "t1".into(),
        from_addr: Pii::new("Ana <ana@acme.example>".to_string()),
        subject: "Hola".into(),
        received_at: march_first(9, 30, 15, 250_000_000),
        job_key: None,
        kind: EmailKind::Recruiter,
        summary: "x".into(),
    };
    let task = NewTask {
        job_key: None,
        kind: "action_required".into(),
        text: "Responder".into(),
        due_at: Some(march_first(18, 0, 0, 999_000_000)),
        source_message_id: Some("m1".into()),
        link: None,
    };
    assert_eq!(
        db.tracker.add_email(&email, Some(&task), when).unwrap(),
        EmailAdded::Stored { task_created: true }
    );
    assert_eq!(
        db.row("SELECT received_at, imported_at FROM emails"),
        vec![text("2026-03-01T09:30:15Z"), text("2026-03-01T12:00:00Z")]
    );
    assert_eq!(
        db.row("SELECT due_at, created_at FROM gonza_tasks"),
        vec![text("2026-03-01T18:00:00Z"), text("2026-03-01T12:00:00Z")]
    );
}

/// The instant the bookkeeping tables below are written at: noon with microseconds.
fn noon_with_microseconds() -> DateTime<Utc> {
    march_first(12, 0, 0, 123_456_000)
}

#[test]
fn the_claude_ledger_stores_z_text_with_no_fraction() {
    let db = Db::new();
    db.tracker
        .record_claude_call("dossier", noon_with_microseconds(), 0.25, 10, 20, true)
        .unwrap();
    assert_eq!(
        db.column("SELECT at FROM claude_calls"),
        vec!["2026-03-01T12:00:00Z"]
    );
}

#[test]
fn the_submission_intents_store_z_text_with_no_fraction() {
    let db = Db::new();
    let when = noon_with_microseconds();
    let id = db
        .tracker
        .begin_intent("submit", "test:1", when)
        .unwrap()
        .unwrap();
    db.tracker
        .finish_intent(id, "submitted", march_first(12, 0, 7, 900_000_000))
        .unwrap();
    assert_eq!(
        db.row("SELECT started_at, finished_at FROM intents"),
        vec![text("2026-03-01T12:00:00Z"), text("2026-03-01T12:00:07Z")]
    );
}

#[test]
fn the_http_cooldowns_store_z_text_with_no_fraction() {
    let db = Db::new();
    db.tracker
        .rest_host("a.test:443", march_first(12, 5, 0, 1))
        .unwrap();
    assert_eq!(
        db.column("SELECT until FROM http_cooldowns"),
        vec!["2026-03-01T12:05:00Z"]
    );
}

#[test]
fn the_kept_http_pages_store_z_text_with_no_fraction_and_keep_the_hosts_last_modified() {
    let db = Db::new();
    let page = StoredPage {
        etag: Some("\"abc\"".into()),
        last_modified: Some("Sun, 01 Mar 2026 08:00:00 GMT".into()),
        body: "{}".into(),
    };
    db.tracker
        .store_page("https://a.test/feed", &page, noon_with_microseconds())
        .unwrap();
    assert_eq!(
        db.row("SELECT used_at, last_modified FROM http_pages"),
        vec![
            text("2026-03-01T12:00:00Z"),
            text("Sun, 01 Mar 2026 08:00:00 GMT"),
        ],
        "the page's own Last-Modified is kept as the host wrote it"
    );
}

#[test]
fn the_command_attempts_store_z_text_with_no_fraction() {
    let db = Db::new();
    db.tracker
        .begin_attempt("attempt-1", "mark", b"{}", noon_with_microseconds())
        .unwrap();
    assert_eq!(
        db.column("SELECT started_at FROM command_attempts"),
        vec!["2026-03-01T12:00:00Z"]
    );
}

#[test]
fn an_open_intent_hands_back_its_start_as_the_stored_text() {
    let db = Db::new();
    db.tracker
        .begin_intent("submit", "test:1", march_first(12, 0, 0, 750_000_000))
        .unwrap();
    let open = db.tracker.open_intents("submit").unwrap();
    assert_eq!(open[0].started_at, "2026-03-01T12:00:00Z");
}

/// The one interview question the drill tests grade.
fn drill_question() -> jobbot_domain::prep::Question {
    jobbot_domain::prep::Question {
        id: "q1".into(),
        topic: "sql".into(),
        question: "?".into(),
        answer: "!".into(),
    }
}

#[test]
fn the_interview_drill_stores_z_text_with_no_fraction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prep.db");
    let store = PrepStore::open(&path).unwrap();
    let question = drill_question();
    let when = march_first(12, 0, 0, 999_999_999);
    store
        .add_cards(std::slice::from_ref(&question), "greenhouse:a", when)
        .unwrap();
    store.grade(&question, 5, when).unwrap();
    let conn = Connection::open(&path).unwrap();
    let one = |sql: &str| -> String { conn.query_row(sql, [], |row| row.get(0)).unwrap() };
    assert_eq!(
        one("SELECT added_at FROM extra_cards"),
        "2026-03-01T12:00:00Z"
    );
    assert_eq!(one("SELECT at FROM reviews"), "2026-03-01T12:00:00Z");
    assert_eq!(
        one("SELECT due FROM cards"),
        "2026-03-02T12:00:00Z",
        "a first perfect answer comes back one day later"
    );
}

#[test]
fn a_drill_card_comes_due_at_its_stored_second_not_before() {
    let store = PrepStore::in_memory().unwrap();
    let question = drill_question();
    let questions = std::slice::from_ref(&question);
    // Graded at 12:00:00.9 with a perfect answer: due one day later, stored as 2026-03-02T12:00:00Z.
    store
        .grade(&question, 5, march_first(12, 0, 0, 900_000_000))
        .unwrap();
    let due = |now: DateTime<Utc>| store.stats(questions, now).unwrap().due;
    assert_eq!(
        due(Utc.with_ymd_and_hms(2026, 3, 2, 11, 59, 59).unwrap()
            + TimeDelta::nanoseconds(999_999_999)),
        0
    );
    assert_eq!(due(Utc.with_ymd_and_hms(2026, 3, 2, 12, 0, 0).unwrap()), 1);
    assert_eq!(
        due(Utc.with_ymd_and_hms(2026, 3, 2, 12, 0, 0).unwrap() + TimeDelta::milliseconds(100)),
        1
    );
    assert!(
        store
            .next_question(
                questions,
                march_first(12, 0, 0, 0) + TimeDelta::hours(23),
                None
            )
            .unwrap()
            .is_none()
    );
}

// ---- what is read

/// Reads the application of `job` after its `updated_at` was replaced by `stored`, by hand. A
/// stored text that cannot be read comes back as the store's own error, not as "no application".
fn updated_at_read_from(stored: &str) -> jobbot_store::tracker::Result<DateTime<Utc>> {
    let db = Db::new();
    let job = db.add_job("1", "Acme", march_first(9, 0, 0, 0), None);
    db.tracker
        .create_application(&job.key(), march_first(9, 0, 0, 0))
        .unwrap();
    db.execute("UPDATE applications SET updated_at = ?1", stored);
    db.tracker
        .get_application(&job.key())
        .map(|row| row.expect("the application is there").updated_at)
}

#[test]
fn stored_text_in_any_rfc_3339_shape_reads_back_as_the_same_instant() {
    for (stored, nanos) in [
        ("2026-03-01T12:00:00Z", 0),
        ("2026-03-01T12:00:00+00:00", 0),
        ("2026-03-01T12:00:00.500Z", 500_000_000),
        ("2026-03-01T12:00:00.5+00:00", 500_000_000),
        ("2026-03-01T12:00:00.123456Z", 123_456_000),
        ("2026-03-01T12:00:00.123456789+00:00", 123_456_789),
        ("2026-03-01T08:00:00-04:00", 0),
        ("2026-03-01T15:30:00+03:30", 0),
        ("2026-03-01T12:00:00z", 0),
        ("2026-03-01t12:00:00Z", 0),
        ("2026-03-01 12:00:00Z", 0),
    ] {
        assert_eq!(
            updated_at_read_from(stored).unwrap(),
            march_first(12, 0, 0, nanos),
            "{stored}"
        );
    }
}

#[test]
fn stored_text_that_is_not_rfc_3339_is_an_error_not_a_guess() {
    for stored in [
        "",
        "yesterday",
        "2026-03-01",
        "2026-03-01 12:00:00",
        "2026-03-01T12:00:00",
        "2026-03-01T12:00Z",
        "2026-03-01T12:00:00+0000",
        "2026-03-01T12:00:00 UTC",
        "1772366400",
    ] {
        assert!(updated_at_read_from(stored).is_err(), "{stored:?}");
    }
}

// ---- what the SQL compares

#[test]
fn a_since_inside_the_same_second_counts_the_events_of_that_second() {
    let db = Db::new();
    let job = db.add_job("1", "Acme", march_first(11, 0, 0, 0), None);
    db.submit(&job, march_first(12, 0, 0, 900_000_000));
    let states = [State::Submitted];
    // Both ends are cut to the second before they are compared as text.
    for since in [
        march_first(12, 0, 0, 100_000_000),
        march_first(12, 0, 0, 900_000_000),
    ] {
        assert_eq!(db.tracker.count_events(&states, since).unwrap(), 1);
        assert_eq!(db.tracker.jobs_reaching(&states, since).unwrap(), 1);
        assert_eq!(db.tracker.submitted_since(since).unwrap(), 1);
        assert_eq!(db.tracker.clicked_since(since).unwrap(), 1);
        assert_eq!(
            db.tracker.submitted_since_reaching(since, &states).unwrap(),
            1
        );
    }
    let next_second = march_first(12, 0, 1, 0);
    assert_eq!(db.tracker.count_events(&states, next_second).unwrap(), 0);
    assert_eq!(db.tracker.jobs_reaching(&states, next_second).unwrap(), 0);
    assert_eq!(db.tracker.submitted_since(next_second).unwrap(), 0);
    assert_eq!(db.tracker.clicked_since(next_second).unwrap(), 0);
    assert_eq!(
        db.tracker
            .submitted_since_reaching(next_second, &states)
            .unwrap(),
        0
    );
}

#[test]
fn the_moves_between_two_instants_include_both_ends_to_the_second() {
    let db = Db::new();
    let job = db.add_job("1", "Acme", march_first(11, 0, 0, 0), None);
    db.submit(&job, march_first(12, 0, 0, 900_000_000));
    // The event is at 12:00:00.9; the span ends at 12:00:00.1: the same second, so it is inside.
    let (total, moves) = db
        .tracker
        .moves_between(
            march_first(12, 0, 0, 0),
            march_first(12, 0, 0, 100_000_000),
            10,
        )
        .unwrap();
    assert_eq!(
        total, 3,
        "scored, prepared and submitted all happened at 12:00:00"
    );
    assert_eq!(moves.len(), 3);
    assert_eq!(
        moves[0].at,
        march_first(12, 0, 0, 0),
        "the time read back has no fraction"
    );
    let (before, _) = db
        .tracker
        .moves_between(
            march_first(11, 59, 0, 0),
            march_first(11, 59, 59, 999_999_999),
            10,
        )
        .unwrap();
    assert_eq!(before, 0);
    let counts = db
        .tracker
        .move_counts_between(
            march_first(12, 0, 0, 900_000_000),
            march_first(12, 0, 0, 900_000_000),
        )
        .unwrap();
    assert_eq!(counts.iter().map(|(_, n)| n).sum::<i64>(), 3);
}

#[test]
fn a_mark_in_the_same_second_as_the_since_is_too_old_to_undo_and_not_offered() {
    let db = Db::new();
    let job = db.add_job("1", "Acme", march_first(11, 0, 0, 0), None);
    db.mark_submitted(&job, march_first(12, 0, 0, 900_000_000));
    let same_second = march_first(12, 0, 0, 100_000_000);
    assert!(db.tracker.undoable_marks(same_second).unwrap().is_empty());
    assert_eq!(
        db.tracker
            .undo_mark(&job.key(), same_second)
            .unwrap()
            .unwrap_err(),
        UndoRefusal::TooOld,
        "`at <= since` compares the two texts, and they are the same second"
    );
    let second_before = march_first(11, 59, 59, 999_999_999);
    let offered = db.tracker.undoable_marks(second_before).unwrap();
    assert_eq!(offered[&job.key()].at, march_first(12, 0, 0, 0));
    let taken_back = db
        .tracker
        .undo_mark(&job.key(), second_before)
        .unwrap()
        .unwrap();
    assert_eq!(
        (taken_back.from, taken_back.to),
        (State::Prepared, State::Submitted)
    );
}

#[test]
fn a_snooze_ending_inside_the_second_of_now_is_already_over() {
    let db = Db::new();
    let job = db.add_job("1", "Acme", march_first(11, 0, 0, 0), None);
    db.tracker
        .create_application(&job.key(), march_first(11, 0, 0, 0))
        .unwrap();
    db.tracker
        .set_snoozed_until(&job.key(), Some(march_first(12, 0, 0, 900_000_000)))
        .unwrap();
    // `snoozed_until > now` compares text: 12:00:00Z is not after 12:00:00Z.
    assert!(
        db.tracker
            .still_snoozed(march_first(12, 0, 0, 100_000_000))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.tracker
            .still_snoozed(march_first(11, 59, 59, 999_999_999))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn the_company_cooldown_edge_is_decided_by_whole_seconds() {
    let db = Db::new();
    let job = db.add_job("1", "Acme AI", march_first(11, 0, 0, 0), None);
    db.submit(&job, march_first(12, 0, 0, 900_000_000));
    let company = normalize("Acme AI");
    // Thirty days after the submission, to the second, it still counts; a second later it does not.
    let day_30 = march_first(12, 0, 0, 100_000_000) + TimeDelta::days(30);
    assert!(
        db.tracker
            .company_in_cooldown(&company, 30, day_30)
            .unwrap()
    );
    assert!(
        !db.tracker
            .company_in_cooldown(&company, 30, day_30 + TimeDelta::seconds(1))
            .unwrap()
    );
}

#[test]
fn the_postings_since_a_date_come_oldest_first_by_the_stored_second() {
    let db = Db::new();
    db.add_job("c", "Acme", march_first(12, 0, 2, 500_000_000), None);
    db.add_job("b", "Acme", march_first(12, 0, 1, 100_000_000), None);
    db.add_job("a", "Acme", march_first(12, 0, 0, 900_000_000), None);
    let seen = db
        .tracker
        .first_seen_since(march_first(12, 0, 0, 100_000_000))
        .unwrap();
    assert_eq!(
        seen,
        vec![
            march_first(12, 0, 0, 0),
            march_first(12, 0, 1, 0),
            march_first(12, 0, 2, 0)
        ]
    );
    assert_eq!(
        db.tracker.first_seen_at().unwrap(),
        Some(march_first(12, 0, 0, 0))
    );
    assert!(
        db.tracker
            .first_seen_since(march_first(12, 0, 3, 0))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.tracker
            .cohort_since(march_first(12, 0, 1, 0))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn postings_stored_in_the_same_second_come_in_the_cohort_by_key_not_by_their_fraction_or_insertion()
{
    let db = Db::new();
    // `test:z` is inserted first and seen 0.8 s earlier than `test:y`, but both are stored as 12:00:00.
    for (id, score, nanos) in [("z", 10.0, 100_000_000), ("y", 20.0, 900_000_000)] {
        let job = make_job_with(|j| {
            j.external_id = id.into();
            j.company = "Acme".into();
            j.title = format!("Role {id}");
        });
        db.tracker
            .add_job(&job, score, &BTreeMap::new(), march_first(12, 0, 0, nanos))
            .unwrap();
    }
    assert_eq!(
        db.column("SELECT first_seen FROM jobs ORDER BY key"),
        ["2026-03-01T12:00:00Z", "2026-03-01T12:00:00Z"]
    );
    let scores: Vec<f64> = db
        .tracker
        .cohort_since(march_first(12, 0, 0, 0))
        .unwrap()
        .into_iter()
        .map(|row| row.score)
        .collect();
    assert_eq!(scores, [20.0, 10.0], "`test:y` before `test:z`");
}

#[test]
fn a_repost_is_live_when_its_date_is_in_the_same_second_as_the_cutoff() {
    let db = Db::new();
    let posted = march_first(12, 0, 0, 900_000_000);
    db.add_job("1", "Acme", march_first(11, 0, 0, 0), Some(posted));
    let repost = make_job_with(|j| {
        j.external_id = "2".into();
        j.company = "Acme".into();
        j.title = "Role 1".into();
    });
    assert!(
        db.tracker
            .has_posting_since(&repost, march_first(12, 0, 0, 100_000_000))
            .unwrap()
    );
    assert!(
        !db.tracker
            .has_posting_since(&repost, march_first(12, 0, 1, 0))
            .unwrap()
    );
}

#[test]
fn a_refreshed_date_in_the_same_second_is_no_change_and_keeps_the_approval() {
    let db = Db::new();
    let job = db.add_job(
        "1",
        "Acme",
        march_first(11, 0, 0, 0),
        Some(march_first(10, 0, 0, 0)),
    );
    let same_second = Job {
        posted_at: Some(march_first(10, 0, 0, 750_000_000)),
        ..job.clone()
    };
    assert!(!db.tracker.refresh_posting(&same_second).unwrap());
    let next_second = Job {
        posted_at: Some(march_first(10, 0, 1, 0)),
        ..job
    };
    assert!(db.tracker.refresh_posting(&next_second).unwrap());
    assert_eq!(
        db.column("SELECT posted_at FROM jobs"),
        vec!["2026-03-01T10:00:01Z"]
    );
}

#[test]
fn mail_since_a_date_comes_newest_first_cut_to_the_second() {
    let db = Db::new();
    for (id, at) in [
        ("old", march_first(9, 59, 59, 999_999_999)),
        ("a", march_first(10, 0, 0, 900_000_000)),
        ("b", march_first(10, 0, 0, 100_000_000)),
        ("c", march_first(10, 0, 5, 0)),
    ] {
        let email = NewEmail {
            message_id: id.into(),
            thread_id: format!("t-{id}"),
            from_addr: Pii::new("Ana <ana@acme.example>".to_string()),
            subject: id.into(),
            received_at: at,
            job_key: None,
            kind: EmailKind::Recruiter,
            summary: String::new(),
        };
        db.tracker
            .add_email(&email, None, march_first(12, 0, 0, 0))
            .unwrap();
    }
    let ids: Vec<String> = db
        .tracker
        .recent_emails(march_first(10, 0, 0, 500_000_000))
        .unwrap()
        .into_iter()
        .map(|email| email.message_id)
        .collect();
    // `a` and `b` are the same second, so they tie and come in message-id order, not by their real order.
    assert_eq!(ids, ["c", "a", "b"]);
}

#[test]
fn open_tasks_come_by_the_due_second_then_by_id_and_the_undated_last() {
    let db = Db::new();
    for (name, due) in [
        ("late", Some(march_first(12, 0, 2, 500_000_000))),
        ("first-id", Some(march_first(12, 0, 0, 900_000_000))),
        ("second-id", Some(march_first(12, 0, 0, 100_000_000))),
        ("undated", None),
    ] {
        let task = NewTask {
            job_key: None,
            kind: "manual".into(),
            text: name.into(),
            due_at: due,
            source_message_id: None,
            link: None,
        };
        db.tracker
            .add_manual_task(&task, march_first(9, 0, 0, 0))
            .unwrap();
    }
    let order: Vec<String> = db
        .tracker
        .open_tasks()
        .unwrap()
        .into_iter()
        .map(|t| t.text)
        .collect();
    assert_eq!(order, ["first-id", "second-id", "late", "undated"]);
}

#[test]
fn the_spend_since_a_date_counts_the_calls_of_that_second() {
    let db = Db::new();
    db.tracker
        .record_claude_call(
            "dossier",
            march_first(12, 0, 0, 900_000_000),
            0.5,
            1,
            1,
            true,
        )
        .unwrap();
    db.tracker
        .record_claude_call("cv", march_first(11, 59, 59, 900_000_000), 0.25, 1, 1, true)
        .unwrap();
    let since = march_first(12, 0, 0, 100_000_000);
    assert!((db.tracker.claude_spend_since(since).unwrap() - 0.5).abs() < 1e-9);
    let by_stage = db.tracker.claude_calls_by_stage(since).unwrap();
    assert_eq!(
        by_stage.keys().map(String::as_str).collect::<Vec<_>>(),
        ["dossier"]
    );
}

#[test]
fn a_host_rest_ending_inside_the_second_of_now_is_forgotten() {
    let db = Db::new();
    db.tracker
        .rest_host("a.test:443", march_first(12, 0, 0, 900_000_000))
        .unwrap();
    db.tracker
        .rest_host("b.test:443", march_first(12, 0, 1, 0))
        .unwrap();
    // `until <= now` is read as text: 12:00:00Z <= 12:00:00Z, though the rest had 0.8 s left.
    let resting = db
        .tracker
        .resting_hosts(march_first(12, 0, 0, 100_000_000))
        .unwrap();
    assert_eq!(
        resting,
        vec![("b.test:443".to_string(), march_first(12, 0, 1, 0))]
    );
}

#[test]
fn a_kept_page_and_an_attempt_are_dropped_by_the_whole_second_of_their_age() {
    let db = Db::new();
    let page = StoredPage {
        etag: None,
        last_modified: None,
        body: "x".into(),
    };
    let start = march_first(12, 0, 0, 900_000_000);
    db.tracker
        .store_page("https://a.test/old", &page, start)
        .unwrap();
    db.tracker
        .begin_attempt("old", "mark", b"{}", start)
        .unwrap();
    // Exactly 30 days (pages) and 7 days (attempts) later to the second: `<` is not met, they stay.
    db.tracker
        .store_page(
            "https://a.test/new",
            &page,
            start + TimeDelta::days(30) - TimeDelta::milliseconds(800),
        )
        .unwrap();
    db.tracker
        .begin_attempt(
            "new",
            "mark",
            b"{}",
            start + TimeDelta::days(7) - TimeDelta::milliseconds(800),
        )
        .unwrap();
    assert_eq!(
        db.column("SELECT url FROM http_pages ORDER BY url"),
        ["https://a.test/new", "https://a.test/old"]
    );
    assert_eq!(
        db.column("SELECT id FROM command_attempts ORDER BY id"),
        ["new", "old"]
    );
    // A second more and the old ones go.
    db.tracker
        .store_page(
            "https://a.test/newer",
            &page,
            start + TimeDelta::days(30) + TimeDelta::seconds(1),
        )
        .unwrap();
    db.tracker
        .begin_attempt(
            "newer",
            "mark",
            b"{}",
            start + TimeDelta::days(7) + TimeDelta::seconds(1),
        )
        .unwrap();
    assert_eq!(
        db.column("SELECT url FROM http_pages ORDER BY url"),
        ["https://a.test/new", "https://a.test/newer"]
    );
    assert_eq!(
        db.column("SELECT id FROM command_attempts ORDER BY id"),
        ["new", "newer"]
    );
}

#[test]
fn the_submissions_of_a_utc_day_are_counted_by_their_utc_date() {
    let db = Db::new();
    let late = db.add_job("1", "Acme", march_first(0, 0, 0, 0), None);
    db.submit(&late, march_first(23, 59, 59, 999_999_999));
    let early = db.add_job("2", "Beta", march_first(0, 0, 0, 0), None);
    db.submit(&early, march_first(0, 0, 0, 0) + TimeDelta::days(1));
    let first = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let second = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    assert_eq!(db.tracker.submitted_on(first).unwrap(), 1);
    assert_eq!(db.tracker.submitted_on(second).unwrap(), 1);
    assert_eq!(
        db.tracker
            .submitted_on(NaiveDate::from_ymd_opt(2026, 3, 3).unwrap())
            .unwrap(),
        0
    );
}

// ---- what the comparison as text would do with other shapes of text

#[test]
fn sql_text_order_puts_a_fraction_before_the_whole_second() {
    // A known hazard, not a contract. Nothing writes such a row today (`to_text` cuts the fraction)
    // and this test writes the row by hand, so it does not watch the writer. It documents what the
    // SQL would do if two shapes of text lived together: `.` sorts before `Z`, so 12:00:00.500Z
    // sorts BEFORE 12:00:00Z although it is later. That is why the formatter must keep writing a
    // single shape; it does not say this order is right.
    let db = Db::new();
    let first = db.add_job("a", "Acme", march_first(12, 0, 0, 0), None);
    db.add_job("b", "Acme", march_first(12, 0, 5, 0), None);
    db.execute(
        "UPDATE jobs SET first_seen = ?1 WHERE key = 'test:b'",
        "2026-03-01T12:00:00.500Z",
    );
    let seen = db
        .tracker
        .first_seen_since(march_first(11, 0, 0, 0))
        .unwrap();
    assert_eq!(
        seen,
        vec![march_first(12, 0, 0, 500_000_000), march_first(12, 0, 0, 0)]
    );
    assert_eq!(first.key(), "test:a");
}

#[test]
fn sql_text_order_drops_a_plus_zero_offset_from_a_since_filter() {
    // A known hazard, not a contract; the row is written by hand, so the writer is not watched.
    // `+` sorts before `Z`: a row written as ...+00:00 is "before" a cutoff written as ...Z for the
    // very same instant, so `>=` leaves it out. That is why the formatter must keep writing a
    // single shape; it does not say this filter is right.
    let db = Db::new();
    let job = db.add_job("a", "Acme", march_first(12, 0, 0, 0), None);
    db.execute(
        "UPDATE jobs SET first_seen = ?1",
        "2026-03-01T12:00:00+00:00",
    );
    assert!(
        db.tracker
            .first_seen_since(march_first(12, 0, 0, 0))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.tracker.job_first_seen(&job.key()).unwrap(),
        Some(march_first(12, 0, 0, 0)),
        "read one by one, the same text is the same instant"
    );
}
