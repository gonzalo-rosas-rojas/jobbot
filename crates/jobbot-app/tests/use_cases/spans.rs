//! Every use case runs in a `tracing` span named after it, whose fields are counts and outcomes
//! (never a key, a name or a text). Nothing is printed unless the command line asks for it.

use std::collections::BTreeMap;
use std::path::Path;

use async_trait::async_trait;

use jobbot_app::actions::approve;
use jobbot_app::discovery::discover;
use jobbot_app::inbox::import;
use jobbot_app::ports::Source;
use jobbot_app::profile::{load_criteria, load_exclusions};
use jobbot_app::reporting::digest::build_digest;
use jobbot_domain::models::{FetchError, FetchResult};
use jobbot_testkit::spans::Spans;
use jobbot_testkit::{NOW, Tracker, make_job};

/// A source whose host asked for a pause.
struct Paused;

#[async_trait]
impl Source for Paused {
    fn name(&self) -> &str {
        "paused"
    }

    async fn fetch(&self) -> FetchResult {
        FetchResult {
            jobs: vec![],
            errors: vec![FetchError::rate_limited("paused", "api", 60, "slow down")],
        }
    }
}

fn field(spans: &Spans, span: &str, field: &str) -> Option<String> {
    spans.field(span, field)
}

fn some(value: &str) -> Option<String> {
    Some(value.to_string())
}

#[tokio::test]
async fn discovery_runs_in_a_span_that_counts_what_happened() {
    let paths = jobbot_testkit::real_paths();
    let criteria = load_criteria(&paths.profile_dir.join("criteria.toml")).unwrap();
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml")).unwrap();
    let tracker = Tracker::in_memory().unwrap();
    let sources: [Box<dyn Source>; 1] = [Box::new(Paused)];

    let spans = Spans::record();
    discover(&sources, &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert_eq!(field(&spans, "discover", "sources"), some("1"));
    assert_eq!(field(&spans, "discover", "fetched"), some("0"));
    assert_eq!(field(&spans, "discover", "queued"), some("0"));
    assert_eq!(field(&spans, "discover", "errors"), some("1"));
    assert_eq!(field(&spans, "discover", "rate_limited"), some("1"));
    assert!(
        spans.names().contains(&"prune_ineligible".to_string()),
        "the prune has its own span: {:?}",
        spans.names()
    );
}

#[test]
fn an_action_runs_in_its_span_with_its_outcome() {
    let tracker = Tracker::in_memory().unwrap();
    let job = make_job();
    tracker.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
    tracker.create_application(&job.key(), *NOW).unwrap();

    let spans = Spans::record();
    assert!(approve(&tracker, &job.key()).unwrap().ok);
    assert_eq!(field(&spans, "approve", "ok"), some("true"));
    assert!(!approve(&tracker, "nope:1").unwrap().ok);
    assert_eq!(field(&spans, "approve", "ok"), some("false"));
    assert!(
        spans
            .all()
            .iter()
            .all(|span| !span.fields.values().any(|value| value.contains(&job.key()))),
        "no span carries the key"
    );
}

#[test]
fn the_inbox_import_and_the_digest_run_in_their_spans() {
    let tracker = Tracker::in_memory().unwrap();
    let spans = Spans::record();
    let report = import(&tracker, &[], *NOW).unwrap();
    assert_eq!(field(&spans, "inbox_import", "items"), some("0"));
    assert_eq!(
        field(&spans, "inbox_import", "imported"),
        some(&report.imported.to_string())
    );
    build_digest(&tracker, *NOW, 5).unwrap();
    assert!(spans.names().contains(&"digest".to_string()));
}

/// Every use case of the application, by its file under `crates/jobbot-app/src`.
const USE_CASES: [(&str, &[&str]); 9] = [
    ("discovery.rs", &["discover", "prune_ineligible", "rescore"]),
    (
        "apply/pipeline.rs",
        &[
            "prepare_report_relocating",
            "refresh_cvs",
            "submit_all_while",
        ],
    ),
    ("inbox.rs", &["import"]),
    ("learn/dossier.rs", &["build_dossier"]),
    ("freelance.rs", &["draft_profile", "draft_proposal"]),
    ("reporting/digest.rs", &["build_digest"]),
    ("reporting/today.rs", &["build_today"]),
    ("reporting/status.rs", &["record_run"]),
    (
        "actions.rs",
        &[
            "approve",
            "resolve",
            "confirm",
            "snooze",
            "unsnooze",
            "mark",
            "record",
            "task_add",
            "task_done",
            "grade_card",
            "set_auto_approve_min",
            "stop",
            "resume",
        ],
    ),
];

#[test]
fn every_use_case_keeps_its_span() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut missing = Vec::new();
    for (file, names) in USE_CASES {
        let text = std::fs::read_to_string(src.join(file)).unwrap();
        for name in names {
            let at = [format!("pub fn {name}("), format!("pub async fn {name}(")]
                .iter()
                .filter_map(|signature| text.find(signature.as_str()))
                .min()
                .unwrap_or_else(|| panic!("{file} has no use case {name}"));
            // The item's attributes sit between the end of the item before it and its signature.
            let item_start = text[..at].rfind("\n}\n").map_or(0, |end| end + 3);
            if !text[item_start..at].contains("#[tracing::instrument") {
                missing.push(format!("{file}: {name}"));
            }
        }
    }
    assert!(missing.is_empty(), "use cases without a span: {missing:?}");
}
