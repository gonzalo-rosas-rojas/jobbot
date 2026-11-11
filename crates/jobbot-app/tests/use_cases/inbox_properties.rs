//! The e-mail import over generated files: every field missing, null, of the wrong type, dirty
//! (control and invisible characters), too long or badly dated. Whatever the file holds, the
//! import reports on every element, stores only clean text within its caps, and a second import of
//! the same file stores nothing new.

use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use serde_json::{Map, Value, json};

use jobbot_app::inbox::import;
use jobbot_app::ports::store::{EmailKind, EmailRow, TaskRow};
use jobbot_domain::models::Job;
use jobbot_domain::outbound::{plain_line, strip_invisible};
use jobbot_domain::state::State;
use jobbot_testkit::Tracker;
use jobbot_testkit::{NOW, make_job_with};

/// The caps of the stored texts (an id is never cut; a cut text ends with an ellipsis).
const ID_MAX: usize = 200;
const FROM_MAX: usize = 201;
const SUBJECT_MAX: usize = 301;
const SUMMARY_MAX: usize = 601;
const TASK_MAX: usize = 301;

/// Three sent applications: two at the same company, so a company name alone can be ambiguous.
fn jobs() -> Vec<Job> {
    [("1", "Acme AI"), ("2", "Acme AI"), ("3", "Beta Labs")]
        .into_iter()
        .map(|(id, company)| {
            make_job_with(|job| {
                job.external_id = id.into();
                job.company = company.into();
            })
        })
        .collect()
}

fn tracker_with_sent_applications() -> Tracker {
    let tracker = Tracker::in_memory().unwrap();
    let sent_at = *NOW - TimeDelta::days(3);
    for job in jobs() {
        tracker
            .add_job(&job, 80.0, &BTreeMap::new(), sent_at)
            .unwrap();
        tracker.create_application(&job.key(), sent_at).unwrap();
        for state in [State::Prepared, State::Submitted] {
            tracker
                .transition(&job.key(), state, None, sent_at)
                .unwrap();
        }
    }
    tracker
}

/// Text as other people's e-mails bring it: plain, full of control and invisible characters,
/// longer than any cap, or anything at all.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z0-9 ,.@<>]{0,40}",
        r"[\x00-\x1f\x7f\x{85}\x{ad}\x{200b}-\x{200f}\x{202a}-\x{202e}\x{2066}-\x{2069}\x{feff}\x{e0020}-\x{e007f} a-z]{0,60}",
        // Words with single spaces: still longer than every cap once whitespace is collapsed.
        "([a-z]{4,12} ){30,90}",
        any::<String>(),
    ]
}

/// A field that is usually text but may be missing, null or a number.
fn field<S: Strategy<Value = String> + 'static>(value: S) -> impl Strategy<Value = Option<Value>> {
    prop_oneof![
        1 => Just(None),
        1 => Just(Some(Value::Null)),
        1 => any::<i64>().prop_map(|n| Some(json!(n))),
        30 => value.prop_map(|text| Some(Value::String(text))),
    ]
}

/// A message id from a small pool, so a file repeats ids, sometimes with whitespace or an
/// invisible character around it that the import strips; or an empty, overlong or arbitrary one.
fn id() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => (0u8..6, prop_oneof![Just(""), Just(" "), Just("\u{200b}"), Just("\t")])
            .prop_map(|(n, pad)| format!("{pad}m{n}{pad}")),
        1 => Just(String::new()),
        1 => "[a-z]{201,210}",
        1 => text(),
    ]
}

fn date() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => (0i64..14_400).prop_map(|minutes| (*NOW - TimeDelta::minutes(minutes)).to_rfc3339()),
        1 => (2i64..4_000).prop_map(|days| (*NOW + TimeDelta::days(days)).to_rfc3339()),
        1 => Just("1999-12-31T23:59:59Z".to_string()),
        1 => text(),
    ]
}

fn kind() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => (select(EmailKind::ALL.to_vec()), any::<bool>()).prop_map(|(kind, shout)| {
            if shout {
                format!(" {} ", kind.as_str().to_uppercase())
            } else {
                kind.as_str().to_string()
            }
        }),
        1 => text(),
    ]
}

fn company() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("Acme AI".to_string()),
        Just("acme ai inc".to_string()),
        Just("Beta Labs".to_string()),
        text(),
    ]
}

fn job_key() -> impl Strategy<Value = String> {
    let keys: Vec<String> = jobs().iter().map(Job::key).collect();
    prop_oneof![3 => select(keys), 1 => text()]
}

fn task() -> impl Strategy<Value = Option<Value>> {
    prop_oneof![
        4 => Just(None),
        1 => Just(Some(Value::Null)),
        1 => text().prop_map(|text| Some(json!(text))),
        4 => (field(text()), field(date())).prop_map(|(text, due_at)| {
            let mut task = Map::new();
            text.map(|text| task.insert("text".to_string(), text));
            due_at.map(|due| task.insert("due_at".to_string(), due));
            Some(Value::Object(task))
        }),
    ]
}

fn item() -> impl Strategy<Value = Value> {
    let fields = (
        field(id()),
        field(id()),
        field(text()),
        field(text()),
        field(date()),
        field(kind()),
        field(text()),
        field(company()),
        field(job_key()),
        task(),
    );
    prop_oneof![
        1 => text().prop_map(Value::String),
        12 => fields.prop_map(
            |(message_id, thread_id, from, subject, received_at, kind, summary, company, job_key, task)| {
                let mut item = Map::new();
                for (name, value) in [
                    ("message_id", message_id),
                    ("thread_id", thread_id),
                    ("from", from),
                    ("subject", subject),
                    ("received_at", received_at),
                    ("kind", kind),
                    ("summary", summary),
                    ("company", company),
                    ("job_key", job_key),
                    ("task", task),
                ] {
                    if let Some(value) = value {
                        item.insert(name.to_string(), value);
                    }
                }
                Value::Object(item)
            },
        ),
    ]
}

/// One line with nothing hidden in it: no control or invisible character, single spaces, trimmed.
fn tidy(text: &str) -> bool {
    !text.chars().any(char::is_control)
        && strip_invisible(text) == text
        && text == text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn short(text: &str, max: usize) -> bool {
    tidy(text) && text.chars().count() <= max
}

/// The stored e-mails of the ids the file names, in the file's order.
fn stored(tracker: &Tracker, items: &[Value]) -> Vec<EmailRow> {
    let mut rows = Vec::new();
    for item in items {
        if let Some(Value::String(raw)) = item.get("message_id")
            && let Some(row) = tracker.get_email(&plain_line(raw)).unwrap()
            && !rows.contains(&row)
        {
            rows.push(row);
        }
    }
    rows
}

/// `rows` without their postings: the one thing a second import may add (a message stored
/// without a posting gets one once it resolves).
fn unlinked_emails(rows: &[EmailRow]) -> Vec<EmailRow> {
    rows.iter()
        .map(|row| EmailRow {
            job_key: None,
            company: None,
            ..row.clone()
        })
        .collect()
}

fn unlinked_tasks(rows: &[TaskRow]) -> Vec<TaskRow> {
    rows.iter()
        .map(|row| TaskRow {
            job_key: None,
            ..row.clone()
        })
        .collect()
}

fn received_in_range(at: DateTime<Utc>) -> bool {
    at >= jobbot_app::actions::earliest_date() && at <= *NOW + TimeDelta::days(1)
}

proptest! {
    #[test]
    fn any_file_is_reported_element_by_element_and_stored_clean(items in vec(item(), 0..12)) {
        let tracker = tracker_with_sent_applications();

        let report = import(&tracker, &items, *NOW).unwrap();

        prop_assert_eq!(
            report.imported + report.skipped + report.rejected + report.linked,
            items.len(),
            "every element is counted once: {:?}", report
        );
        prop_assert_eq!(report.lines.len(), items.len());
        for line in &report.lines {
            prop_assert!(tidy(line), "{:?}", line);
        }
        for row in stored(&tracker, &items) {
            prop_assert!(short(&row.message_id, ID_MAX) && !row.message_id.is_empty(), "{:?}", row);
            prop_assert!(short(&row.thread_id, ID_MAX) && !row.thread_id.is_empty(), "{:?}", row);
            prop_assert!(short(row.from_addr.expose(), FROM_MAX) && !row.from_addr.expose().is_empty(), "{:?}", row);
            prop_assert!(short(&row.subject, SUBJECT_MAX), "{:?}", row);
            prop_assert!(short(&row.summary, SUMMARY_MAX), "{:?}", row);
            prop_assert!(received_in_range(row.received_at), "{:?}", row);
        }
        for task in tracker.open_tasks().unwrap() {
            prop_assert!(short(&task.text, TASK_MAX) && !task.text.is_empty(), "{:?}", task);
        }
    }

    #[test]
    fn a_second_import_of_the_same_file_stores_nothing_new(items in vec(item(), 0..12)) {
        let tracker = tracker_with_sent_applications();
        let first = import(&tracker, &items, *NOW).unwrap();
        let emails = stored(&tracker, &items);
        let tasks = tracker.open_tasks().unwrap();

        let second = import(&tracker, &items, *NOW).unwrap();

        prop_assert_eq!(second.imported, 0, "{:?}", second);
        prop_assert_eq!(second.tasks_created, 0, "{:?}", second);
        prop_assert_eq!(second.rejected, first.rejected);
        let again = stored(&tracker, &items);
        prop_assert_eq!(unlinked_emails(&again), unlinked_emails(&emails));
        for (before, after) in emails.iter().zip(&again) {
            if before.job_key.is_some() {
                prop_assert_eq!(&after.job_key, &before.job_key, "a posting is never changed");
            }
        }
        prop_assert_eq!(unlinked_tasks(&tracker.open_tasks().unwrap()), unlinked_tasks(&tasks));
    }
}
