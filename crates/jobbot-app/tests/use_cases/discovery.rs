//! The `discovery` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/discovery.rs`).

use async_trait::async_trait;
use chrono::TimeDelta;

use jobbot_app::discovery::*;
use jobbot_app::ports::Source;
use jobbot_app::ports::store::INTENT_SUBMIT;
use jobbot_app::profile::{load_criteria, load_exclusions};
use jobbot_domain::models::{FetchError, FetchResult, Job};
use jobbot_domain::rules::{Criteria, Exclusions};
use jobbot_domain::state::State;
use jobbot_testkit::Tracker;
use jobbot_testkit::{NOW, make_job_with};

const GOOD_DESC: &str = "Build LLM agents with python, typescript, react, rag and mcp. \
                             Applied AI and generative AI work on agentic workflows, with an \
                             engineering culture and open source time. \
                             Full-time position with mentorship and a take-home exercise.";
/// A source that returns a fixed result, so discovery is tested without any network.
struct FakeSource {
    jobs: Vec<Job>,
    errors: Vec<FetchError>,
}
#[async_trait]
impl Source for FakeSource {
    fn name(&self) -> &str {
        "fake"
    }

    async fn fetch(&self) -> FetchResult {
        FetchResult {
            jobs: self.jobs.clone(),
            errors: self.errors.clone(),
        }
    }
}
fn fake(jobs: Vec<Job>, errors: &[&str]) -> Box<dyn Source> {
    Box::new(FakeSource {
        jobs,
        errors: errors.iter().map(|line| failed(line)).collect(),
    })
}
/// A failure from its report line, `<source>:<slug>: <detail>`.
fn failed(line: &str) -> FetchError {
    let (place, detail) = line.split_once(": ").unwrap();
    let (source, slug) = place.split_once(':').unwrap();
    FetchError::failed(source, slug, detail)
}
fn lines(errors: &[FetchError]) -> Vec<String> {
    errors.iter().map(ToString::to_string).collect()
}
fn rules() -> (Criteria, Exclusions) {
    let paths = jobbot_testkit::real_paths();
    (
        load_criteria(&paths.profile_dir.join("criteria.toml")).unwrap(),
        load_exclusions(&paths.profile_dir.join("exclusions.toml")).unwrap(),
    )
}
fn good(id: &str, company: &str) -> Job {
    make_job_with(|job| {
        job.external_id = id.into();
        job.company = company.into();
        job.title = "Senior AI Engineer".into();
        job.location = "Remote - Latin America".into();
        job.description = GOOD_DESC.into();
        job.salary_min_monthly_usd = Some(5000.0);
        job.salary_max_monthly_usd = Some(7000.0);
    })
}
/// The shipped rules with the numbers of the extra-place tests pinned: Australia open, a
/// threshold nothing in Chile reaches and one that everything in Australia reaches.
fn australia_rules() -> (Criteria, Exclusions) {
    let (criteria, exclusions) = rules();
    (
        Criteria {
            apply_threshold: 99.0,
            extra_open_places: vec!["australia".into()],
            extra_open_apply_threshold: Some(0.0),
            ..criteria
        },
        exclusions,
    )
}
#[tokio::test]
async fn a_job_in_an_extra_place_gets_the_extra_threshold_and_is_not_held_for_pay() {
    let (criteria, exclusions) = australia_rules();
    let tracker = Tracker::in_memory().unwrap();
    let mut sydney = good("1", "Sydney Co");
    sydney.location = "Sydney, Australia".into();
    sydney.remote = false;
    // A pay under the floor: for a job in Chile this would flag it for Gonza.
    sydney.salary_min_monthly_usd = Some(1000.0);
    sydney.salary_max_monthly_usd = Some(1500.0);
    let chile = good("2", "Chile Co");
    let mut london = good("3", "London Co");
    london.location = "London".into();
    london.remote = false;

    let report = discover(
        &[fake(vec![sydney, chile, london], &[])],
        &criteria,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();

    // Only the Sydney job is queued, and as a normal queued job, not as a below-floor one.
    assert_eq!(report.region_skipped, 1, "London stays out of the region");
    assert_eq!(state_and_note(&tracker, "test:1"), (State::Scored, None));
    assert!(tracker.get_application("test:2").unwrap().is_none());
}
#[tokio::test]
async fn the_extra_threshold_is_for_titles_with_a_technical_word() {
    // The first Australian run queued sales and legal jobs at the extra threshold too.
    let (criteria, exclusions) = australia_rules();
    let criteria = Criteria {
        extra_open_title_words: vec!["engineer".into(), "developer".into(), "architect".into()],
        ..criteria
    };
    let tracker = Tracker::in_memory().unwrap();
    let in_sydney = |id: &str, title: &str| {
        let mut job = good(id, &format!("Co {id}"));
        job.title = title.into();
        job.location = "Sydney, Australia".into();
        job.remote = false;
        job
    };
    let jobs = vec![
        in_sydney("1", "Senior Software Engineer"),
        in_sydney("2", "Solutions Architect"),
        in_sydney("3", "Senior Account Executive"),
        in_sydney("4", "People Legal Counsel"),
    ];

    discover(&[fake(jobs, &[])], &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert!(tracker.get_application("test:1").unwrap().is_some());
    assert!(tracker.get_application("test:2").unwrap().is_some());
    assert!(tracker.get_application("test:3").unwrap().is_none());
    assert!(tracker.get_application("test:4").unwrap().is_none());
}
#[tokio::test]
async fn a_title_word_with_a_bang_keeps_its_titles_from_the_extra_threshold() {
    // "Senior Contract Solutions Lawyer" and "Product Marketing Manager - AB Platform" held a
    // technical word and were queued at the extra threshold (2026-10-22).
    let (criteria, exclusions) = australia_rules();
    let criteria = Criteria {
        extra_open_title_words: vec![
            "solutions".into(),
            "platform".into(),
            "!lawyer".into(),
            "! Marketing".into(),
        ],
        ..criteria
    };
    let tracker = Tracker::in_memory().unwrap();
    let in_sydney = |id: &str, title: &str| {
        let mut job = good(id, &format!("Co {id}"));
        job.title = title.into();
        job.location = "Sydney, Australia".into();
        job.remote = false;
        job
    };
    let jobs = vec![
        in_sydney("1", "Solutions Architect"),
        in_sydney("2", "Senior Contract Solutions Lawyer, APAC"),
        in_sydney("3", "Product Marketing Manager - AB Platform"),
        in_sydney("4", "Senior Engineer - Platform"),
    ];

    discover(&[fake(jobs, &[])], &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert!(tracker.get_application("test:1").unwrap().is_some());
    assert!(tracker.get_application("test:2").unwrap().is_none());
    assert!(tracker.get_application("test:3").unwrap().is_none());
    assert!(tracker.get_application("test:4").unwrap().is_some());
}
#[tokio::test]
async fn only_bang_words_keep_every_other_title_at_the_extra_threshold() {
    let (criteria, exclusions) = australia_rules();
    let criteria = Criteria {
        extra_open_title_words: vec!["!lawyer".into()],
        ..criteria
    };
    let tracker = Tracker::in_memory().unwrap();
    let in_sydney = |id: &str, title: &str| {
        let mut job = good(id, &format!("Co {id}"));
        job.title = title.into();
        job.location = "Sydney, Australia".into();
        job.remote = false;
        job
    };
    let jobs = vec![
        in_sydney("1", "Senior Account Executive"),
        in_sydney("2", "Employment Lawyer"),
    ];

    discover(&[fake(jobs, &[])], &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert!(tracker.get_application("test:1").unwrap().is_some());
    assert!(tracker.get_application("test:2").unwrap().is_none());
}
#[tokio::test]
async fn without_the_extra_threshold_an_australian_job_needs_the_normal_one() {
    let (criteria, exclusions) = australia_rules();
    let criteria = Criteria {
        extra_open_apply_threshold: None,
        ..criteria
    };
    let tracker = Tracker::in_memory().unwrap();
    let mut sydney = good("1", "Sydney Co");
    sydney.location = "Sydney, Australia".into();
    sydney.remote = false;

    discover(
        &[fake(vec![sydney], &[])],
        &criteria,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();

    assert!(tracker.get_job("test:1").unwrap().is_some(), "stored");
    assert!(tracker.get_application("test:1").unwrap().is_none());
}
fn jobs() -> Vec<Job> {
    let mut us_only = good("3", "UsOnly Co");
    us_only.location = "Remote - USA".into();
    let sales = make_job_with(|job| {
        job.external_id = "4".into();
        job.company = "Sales Co".into();
        job.title = "Account Executive".into();
        job.location = "Remote".into();
        job.description = "sales".into();
    });
    let mut low_pay = good("5", "LowPay Inc");
    low_pay.title = "AI Engineer".into();
    low_pay.salary_min_monthly_usd = Some(1500.0);
    low_pay.salary_max_monthly_usd = Some(2200.0);
    vec![
        good("1", "Acme AI"),
        good("2", "Tidewater Systems"),
        us_only,
        sales,
        low_pay,
        good("6", "Acme AI"),
    ]
}
#[tokio::test]
async fn discover_counts_and_states() {
    let (criteria, exclusions) = rules();
    let tracker = Tracker::in_memory().unwrap();

    let report = discover(&[fake(jobs(), &[])], &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert_eq!(
        (
            report.fetched,
            report.already_seen,
            report.excluded,
            report.region_skipped
        ),
        (6, 1, 1, 1)
    );
    assert_eq!(
        (
            report.below_threshold,
            report.below_floor_flagged,
            report.queued
        ),
        (1, 1, 1)
    );
    assert_eq!(
        tracker.get_application("test:1").unwrap().unwrap().state,
        State::Scored
    );
    let flagged = tracker.get_application("test:5").unwrap().unwrap();
    assert_eq!(flagged.state, State::NeedsGonza);
    assert!(
        flagged
            .needs_reason
            .unwrap()
            .starts_with("below_floor_exception")
    );
    // Excluded and region-skipped jobs are not stored at all; a below-threshold job is stored but gets no application.
    for key in ["test:2", "test:3"] {
        assert!(
            tracker.get_job(key).unwrap().is_none(),
            "{key} must not be stored"
        );
        assert!(tracker.get_application(key).unwrap().is_none());
    }
    assert!(tracker.get_job("test:4").unwrap().is_some());
    assert!(tracker.get_application("test:4").unwrap().is_none());
    // A below-floor job never reaches the auto-apply queue.
    let queued: Vec<String> = tracker
        .list_by_state(State::Scored)
        .unwrap()
        .into_iter()
        .map(|row| row.job_key)
        .collect();
    assert_eq!(queued, ["test:1"]);
}
#[tokio::test]
async fn a_posting_seen_before_gets_its_current_apply_link() {
    let (criteria, exclusions) = rules();
    let tracker = Tracker::in_memory().unwrap();
    let first = good("1", "Acme AI");
    discover(
        &[fake(vec![first.clone()], &[])],
        &criteria,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();
    let mut moved = first.clone();
    moved.apply_url = "https://job-boards.greenhouse.io/acme/jobs/1".into();

    let report = discover(
        &[fake(vec![moved.clone()], &[])],
        &criteria,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();

    assert_eq!(report.already_seen, 1);
    assert_eq!(
        tracker.get_job("test:1").unwrap().unwrap().apply_url,
        moved.apply_url
    );
}
/// The rules a discovery run uses in the refresh and prune tests: the shipped ones with the
/// 45-day limit pinned.
fn pruning_rules() -> (Criteria, Exclusions) {
    let (criteria, exclusions) = rules();
    (
        Criteria {
            max_posting_age_days: 45,
            ..criteria
        },
        exclusions,
    )
}
/// An in-memory tracker where one discovery run has already stored (and queued) `jobs`.
async fn stored_by_discovery(jobs: Vec<Job>) -> Tracker {
    let (criteria, exclusions) = pruning_rules();
    let tracker = Tracker::in_memory().unwrap();
    discover(&[fake(jobs, &[])], &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();
    tracker
}
/// Another discovery run over a tracker, with the sources returning `jobs` now.
async fn discover_again(tracker: &Tracker, jobs: Vec<Job>) -> DiscoveryReport {
    let (criteria, exclusions) = pruning_rules();
    discover(&[fake(jobs, &[])], &criteria, &exclusions, tracker, *NOW)
        .await
        .unwrap()
}
/// The state and the note of an application.
fn state_and_note(tracker: &Tracker, key: &str) -> (State, Option<String>) {
    let row = tracker.get_application(key).unwrap().unwrap();
    (row.state, row.note)
}
const STALE_NOTE: &str = "stale_posting: aviso de más de 45 días";
const REGION_NOTE: &str = "fuera_de_region: no se puede hacer desde Chile";
/// The posting as a source that dated it wrongly reported it when it was stored: three days old.
fn recently_dated(id: &str, company: &str) -> Job {
    let mut job = good(id, company);
    job.posted_at = Some(*NOW - TimeDelta::days(3));
    job
}
#[tokio::test]
async fn a_stored_posting_that_the_source_now_dates_as_too_old_is_withdrawn() {
    let first = recently_dated("1", "Acme AI");
    let tracker = stored_by_discovery(vec![first.clone()]).await;
    assert_eq!(state_and_note(&tracker, "test:1").0, State::Scored);
    // Greenhouse used to report `updated_at`; `first_published` says it is 80 days old.
    let mut fresh = first.clone();
    fresh.posted_at = Some(*NOW - TimeDelta::days(80));

    let report = discover_again(&tracker, vec![fresh.clone()]).await;

    assert_eq!(
        (
            report.already_seen,
            report.refreshed,
            report.pruned_stale,
            report.pruned_region
        ),
        (1, 1, 1, 0)
    );
    assert_eq!(
        state_and_note(&tracker, "test:1"),
        (State::Withdrawn, Some(STALE_NOTE.to_string()))
    );
    assert_eq!(
        tracker.get_job("test:1").unwrap().unwrap().posted_at,
        fresh.posted_at
    );
}
#[tokio::test]
async fn a_stored_posting_that_the_source_now_places_in_a_us_city_is_withdrawn() {
    let first = good("1", "Acme AI");
    let tracker = stored_by_discovery(vec![first.clone()]).await;
    let mut fresh = first.clone();
    fresh.location = "San Francisco, CA".into();

    let report = discover_again(&tracker, vec![fresh.clone()]).await;

    assert_eq!(
        (report.refreshed, report.pruned_stale, report.pruned_region),
        (1, 0, 1)
    );
    assert_eq!(
        state_and_note(&tracker, "test:1"),
        (State::Withdrawn, Some(REGION_NOTE.to_string()))
    );
    // The refreshed location is what is stored now, so the digest shows the truth.
    assert_eq!(
        tracker.get_job("test:1").unwrap().unwrap().location,
        "San Francisco, CA"
    );
}
#[tokio::test]
async fn the_prune_follows_region_ok_on_the_fresh_fields() {
    let cases = [
        // (location, remote now, still queued?)
        ("Austin, TX", true, false),
        // Ashby's hybrid role that `isRemote` once called remote.
        ("Remote", false, false),
        ("Santiago, Chile", false, true),
        ("Remote - Latin America; Toronto, Canada", true, true),
        ("Remote", true, true),
    ];
    let jobs: Vec<Job> = (0..cases.len())
        .map(|i| good(&i.to_string(), &format!("Company {i}")))
        .collect();
    let tracker = stored_by_discovery(jobs.clone()).await;
    let fresh: Vec<Job> = jobs
        .iter()
        .zip(cases)
        .map(|(job, (location, remote, _))| Job {
            location: location.into(),
            remote,
            ..job.clone()
        })
        .collect();

    discover_again(&tracker, fresh).await;

    for (i, (location, remote, queued)) in cases.into_iter().enumerate() {
        let state = state_and_note(&tracker, &format!("test:{i}")).0;
        let expected = if queued {
            State::Scored
        } else {
            State::Withdrawn
        };
        assert_eq!(state, expected, "{location:?}, remote {remote}");
    }
}
#[tokio::test]
async fn a_healthy_posting_stays_in_the_queue_and_its_fresh_fields_are_stored() {
    let first = recently_dated("1", "Acme AI");
    let tracker = stored_by_discovery(vec![first.clone()]).await;
    // A little older, a new link, nothing wrong with it; and a fetch with no date at all must
    // not blank the stored one.
    let mut fresh = first.clone();
    fresh.posted_at = Some(*NOW - TimeDelta::days(20));
    fresh.apply_url = "https://job-boards.greenhouse.io/acme/jobs/1".into();
    let undated = Job {
        posted_at: None,
        ..fresh.clone()
    };

    let report = discover_again(&tracker, vec![fresh.clone()]).await;
    assert_eq!(
        (report.refreshed, report.pruned_stale, report.pruned_region),
        (1, 0, 0)
    );
    assert_eq!(state_and_note(&tracker, "test:1").0, State::Scored);
    let report = discover_again(&tracker, vec![undated]).await;

    assert_eq!(report.refreshed, 0);
    assert_eq!(state_and_note(&tracker, "test:1").0, State::Scored);
    let stored = tracker.get_job("test:1").unwrap().unwrap();
    assert_eq!(stored.posted_at, fresh.posted_at);
    assert_eq!(stored.apply_url, fresh.apply_url);
}
/// Walks an application through `path` (the states after `Scored`).
fn walk(tracker: &Tracker, key: &str, path: &[State]) {
    for state in path {
        tracker.transition(key, *state, None, *NOW).unwrap();
    }
}
#[tokio::test]
async fn an_application_that_was_sent_or_is_finished_is_never_withdrawn() {
    use State::*;
    let paths: [(State, &[State]); 9] = [
        (Submitted, &[Prepared, Submitted]),
        (Replied, &[Prepared, Submitted, Replied]),
        (TechnicalTest, &[Prepared, Submitted, TechnicalTest]),
        (Interview, &[Prepared, Submitted, Interview]),
        (Offer, &[Prepared, Submitted, Interview, Offer]),
        (Accepted, &[Prepared, Submitted, Interview, Offer, Accepted]),
        (Rejected, &[Prepared, Submitted, Rejected]),
        (Ghosted, &[Prepared, Submitted, Ghosted]),
        (Withdrawn, &[Withdrawn]),
    ];
    let jobs: Vec<Job> = (0..paths.len())
        .map(|i| recently_dated(&i.to_string(), &format!("Company {i}")))
        .collect();
    let tracker = stored_by_discovery(jobs.clone()).await;
    for (job, (_, path)) in jobs.iter().zip(paths) {
        walk(&tracker, &job.key(), path);
    }
    // Every posting is now both too old and in a US city.
    let fresh: Vec<Job> = jobs
        .iter()
        .map(|job| Job {
            location: "San Francisco, CA".into(),
            posted_at: Some(*NOW - TimeDelta::days(90)),
            ..job.clone()
        })
        .collect();

    let report = discover_again(&tracker, fresh).await;

    assert_eq!((report.pruned_stale, report.pruned_region), (0, 0));
    for (job, (state, _)) in jobs.iter().zip(paths) {
        assert_eq!(
            state_and_note(&tracker, &job.key()).0,
            state,
            "{}",
            job.key()
        );
    }
    // The Withdrawn one kept the note it had, too.
    assert_eq!(state_and_note(&tracker, "test:8").1, None);
}
#[tokio::test]
async fn a_needs_gonza_application_is_withdrawn_when_its_posting_is_ineligible() {
    let waiting = good("1", "Waiting Co");
    let below_floor = {
        let mut job = good("2", "LowPay Inc");
        job.salary_min_monthly_usd = Some(1500.0);
        job.salary_max_monthly_usd = Some(2200.0);
        job
    };
    let healthy = good("3", "Healthy Co");
    let tracker =
        stored_by_discovery(vec![waiting.clone(), below_floor.clone(), healthy.clone()]).await;
    tracker
        .transition(
            "test:1",
            State::NeedsGonza,
            Some("faltan respuestas: Email *"),
            *NOW,
        )
        .unwrap();
    tracker
        .transition("test:3", State::NeedsGonza, Some("captcha"), *NOW)
        .unwrap();
    assert_eq!(state_and_note(&tracker, "test:2").0, State::NeedsGonza);
    let in_us = |job: &Job| Job {
        location: "Remote - USA".into(),
        ..job.clone()
    };

    let report = discover_again(
        &tracker,
        vec![in_us(&waiting), in_us(&below_floor), healthy.clone()],
    )
    .await;

    assert_eq!(report.pruned_region, 2);
    for key in ["test:1", "test:2"] {
        let row = tracker.get_application(key).unwrap().unwrap();
        assert_eq!(row.state, State::Withdrawn, "{key}");
        assert_eq!(row.note.as_deref(), Some(REGION_NOTE), "{key}");
        assert_eq!(row.needs_reason, None, "{key}");
    }
    // Waiting for Gonza for its own reason, and still eligible: left alone.
    let row = tracker.get_application("test:3").unwrap().unwrap();
    assert_eq!(row.state, State::NeedsGonza);
    assert_eq!(row.needs_reason.as_deref(), Some("captcha"));
}
#[tokio::test]
async fn a_form_that_may_have_gone_out_is_never_withdrawn() {
    let jobs: Vec<Job> = (1..=3)
        .map(|i| good(&i.to_string(), &format!("Company {i}")))
        .collect();
    let tracker = stored_by_discovery(jobs.clone()).await;
    // The program clicked and the page did not confirm; Gonza closed or left the assisted
    // form that he may have sent himself.
    tracker
        .transition(
            "test:1",
            State::NeedsGonza,
            Some("unconfirmed_submit: x"),
            *NOW,
        )
        .unwrap();
    tracker.record_attempt("test:1", *NOW).unwrap();
    tracker
        .transition(
            "test:2",
            State::NeedsGonza,
            Some("assist_closed: cerraste"),
            *NOW,
        )
        .unwrap();
    tracker
        .transition(
            "test:3",
            State::NeedsGonza,
            Some("assist_timeout: sin"),
            *NOW,
        )
        .unwrap();
    let fresh: Vec<Job> = jobs
        .iter()
        .map(|job| Job {
            location: "San Francisco, CA".into(),
            posted_at: Some(*NOW - TimeDelta::days(90)),
            ..job.clone()
        })
        .collect();

    let report = discover_again(&tracker, fresh).await;

    assert_eq!((report.pruned_stale, report.pruned_region), (0, 0));
    for key in ["test:1", "test:2", "test:3"] {
        assert_eq!(state_and_note(&tracker, key).0, State::NeedsGonza, "{key}");
    }
}
#[tokio::test]
async fn an_interrupted_submit_is_never_withdrawn() {
    let jobs: Vec<Job> = vec![good("1", "Company 1")];
    let tracker = stored_by_discovery(jobs.clone()).await;
    tracker
        .transition(
            "test:1",
            State::NeedsGonza,
            Some("interrupted_submit: la corrida se cortó"),
            *NOW,
        )
        .unwrap();
    let fresh: Vec<Job> = jobs
        .iter()
        .map(|job| Job {
            location: "San Francisco, CA".into(),
            posted_at: Some(*NOW - TimeDelta::days(90)),
            ..job.clone()
        })
        .collect();

    let report = discover_again(&tracker, fresh).await;

    assert_eq!((report.pruned_stale, report.pruned_region), (0, 0));
    assert_eq!(state_and_note(&tracker, "test:1").0, State::NeedsGonza);
}
#[tokio::test]
async fn a_form_with_an_open_submit_intent_is_never_withdrawn() {
    // A run died (or is still running) between the intent and the end of its click: the
    // application is still `Prepared` and has no attempt stamp yet, but it may have gone out.
    let jobs: Vec<Job> = vec![good("1", "Company 1")];
    let tracker = stored_by_discovery(jobs.clone()).await;
    tracker
        .transition("test:1", State::Prepared, None, *NOW)
        .unwrap();
    let intent = tracker
        .begin_intent(INTENT_SUBMIT, "test:1", *NOW)
        .unwrap()
        .unwrap();
    let fresh: Vec<Job> = jobs
        .iter()
        .map(|job| Job {
            location: "San Francisco, CA".into(),
            posted_at: Some(*NOW - TimeDelta::days(90)),
            ..job.clone()
        })
        .collect();

    let report = discover_again(&tracker, fresh.clone()).await;

    assert_eq!((report.pruned_stale, report.pruned_region), (0, 0));
    assert_eq!(state_and_note(&tracker, "test:1").0, State::Prepared);

    // Once the intent is closed the application is an ordinary one again.
    tracker.finish_intent(intent, "refused", *NOW).unwrap();
    let report = discover_again(&tracker, fresh).await;
    assert_eq!(report.pruned_stale + report.pruned_region, 1);
    assert_eq!(state_and_note(&tracker, "test:1").0, State::Withdrawn);
}
#[tokio::test]
async fn a_form_waiting_for_gonza_to_send_by_hand_is_never_withdrawn() {
    // Gonza may have sent it from his own browser and not yet run `confirm`; withdrawn is
    // terminal, so `confirm` would then be refused and the real send would be lost.
    let reasons = [
        "handoff: envía tú este formulario. Hoja de respuestas: data/handoff/x.md",
        "listing_only: postular a mano desde el enlace del aviso",
        "manual_only_platform: postular a mano desde el enlace del aviso",
    ];
    let jobs: Vec<Job> = (1..=3)
        .map(|i| good(&i.to_string(), &format!("Company {i}")))
        .collect();
    let tracker = stored_by_discovery(jobs.clone()).await;
    for (i, reason) in reasons.iter().enumerate() {
        tracker
            .transition(
                &format!("test:{}", i + 1),
                State::NeedsGonza,
                Some(reason),
                *NOW,
            )
            .unwrap();
    }
    // Both stale and out of region now: every rule would withdraw them.
    let fresh: Vec<Job> = jobs
        .iter()
        .map(|job| Job {
            location: "San Francisco, CA".into(),
            posted_at: Some(*NOW - TimeDelta::days(90)),
            ..job.clone()
        })
        .collect();

    let report = discover_again(&tracker, fresh).await;

    assert_eq!((report.pruned_stale, report.pruned_region), (0, 0));
    for key in ["test:1", "test:2", "test:3"] {
        assert_eq!(state_and_note(&tracker, key).0, State::NeedsGonza, "{key}");
    }
}
#[tokio::test]
async fn a_role_posted_again_after_the_old_one_went_stale_is_a_new_posting() {
    let (criteria, exclusions) = pruning_rules();
    let tracker = Tracker::in_memory().unwrap();
    let old = Job {
        posted_at: Some(*NOW - TimeDelta::days(60)),
        ..good("1", "Acme AI")
    };
    tracker
        .add_job(
            &old,
            80.0,
            &std::collections::BTreeMap::new(),
            *NOW - TimeDelta::days(60),
        )
        .unwrap();
    // Same company and title, a new id and a fresh date: the company reposted the role.
    let repost = Job {
        posted_at: Some(*NOW - TimeDelta::days(2)),
        ..good("2", "Acme AI")
    };

    let report = discover(
        &[fake(vec![repost], &[])],
        &criteria,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();

    assert_eq!(report.already_seen, 0);
    assert!(tracker.get_job("test:2").unwrap().is_some(), "stored");
}
#[tokio::test]
async fn a_role_posted_again_while_the_old_one_is_live_stays_one_posting() {
    let (criteria, exclusions) = pruning_rules();
    let tracker = Tracker::in_memory().unwrap();
    let old = Job {
        posted_at: Some(*NOW - TimeDelta::days(10)),
        ..good("1", "Acme AI")
    };
    tracker
        .add_job(
            &old,
            80.0,
            &std::collections::BTreeMap::new(),
            *NOW - TimeDelta::days(10),
        )
        .unwrap();
    let repost = Job {
        posted_at: Some(*NOW - TimeDelta::days(2)),
        ..good("2", "Acme AI")
    };

    let report = discover(
        &[fake(vec![repost], &[])],
        &criteria,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();

    assert_eq!(report.already_seen, 1);
    assert!(tracker.get_job("test:2").unwrap().is_none());
}
#[tokio::test]
async fn a_second_run_changes_nothing() {
    let stale = recently_dated("1", "Stale Co");
    let abroad = good("2", "Abroad Co");
    let fine = good("3", "Fine Co");
    let tracker = stored_by_discovery(vec![stale.clone(), abroad.clone(), fine.clone()]).await;
    let fresh = vec![
        Job {
            posted_at: Some(*NOW - TimeDelta::days(90)),
            ..stale
        },
        Job {
            location: "Remote - USA".into(),
            ..abroad
        },
        fine,
    ];
    let first = discover_again(&tracker, fresh.clone()).await;
    assert_eq!(
        (first.refreshed, first.pruned_stale, first.pruned_region),
        (2, 1, 1)
    );
    let history = |key: &str| tracker.events_for(key).unwrap().len();
    let before = ["test:1", "test:2", "test:3"].map(history);

    let again = discover_again(&tracker, fresh).await;

    assert_eq!(
        (
            again.already_seen,
            again.refreshed,
            again.pruned_stale,
            again.pruned_region
        ),
        (3, 0, 0, 0)
    );
    assert_eq!(["test:1", "test:2", "test:3"].map(history), before);
    assert_eq!(tracker.counts().unwrap().get("scored"), Some(&1));
}
#[tokio::test]
async fn the_refresh_keeps_the_score_and_the_pay_it_was_stored_with() {
    let first = good("1", "Acme AI");
    let tracker = stored_by_discovery(vec![first.clone()]).await;
    let score = tracker.get_application("test:1").unwrap().unwrap().score;
    let detail = tracker.score_detail("test:1").unwrap();
    assert!(score >= 60.0);
    let fresh = Job {
        location: "Remote - Chile".into(),
        posted_at: Some(*NOW - TimeDelta::days(5)),
        description: "A different text with no keywords at all.".into(),
        salary_min_monthly_usd: Some(100.0),
        salary_max_monthly_usd: Some(200.0),
        ..first.clone()
    };

    let report = discover_again(&tracker, vec![fresh]).await;

    assert_eq!(report.refreshed, 1);
    assert_eq!(
        tracker.get_application("test:1").unwrap().unwrap().score,
        score
    );
    assert_eq!(tracker.score_detail("test:1").unwrap(), detail);
    let stored = tracker.get_job("test:1").unwrap().unwrap();
    assert_eq!(stored.salary_min_monthly_usd, first.salary_min_monthly_usd);
    assert_eq!(stored.salary_max_monthly_usd, first.salary_max_monthly_usd);
    assert_eq!(stored.description, first.description);
    assert_eq!(state_and_note(&tracker, "test:1").0, State::Scored);
}
/// Stores a posting and queues it directly, as an older version of the program left it.
fn left_in_the_queue(tracker: &Tracker, job: &Job) {
    tracker
        .add_job(job, 70.0, &Default::default(), *NOW)
        .unwrap();
    tracker.create_application(&job.key(), *NOW).unwrap();
}
#[tokio::test]
async fn a_posting_the_source_no_longer_returns_is_pruned_when_stale_but_not_for_its_region() {
    let tracker = Tracker::in_memory().unwrap();
    let mut old = good("1", "Old Co");
    old.posted_at = Some(*NOW - TimeDelta::days(60));
    let mut abroad = good("2", "Abroad Co");
    abroad.location = "San Francisco, CA".into();
    left_in_the_queue(&tracker, &old);
    left_in_the_queue(&tracker, &abroad);

    let report = discover_again(&tracker, vec![]).await;

    // The date is the gate's own rule on the stored date, so it needs no fresh fetch. The
    // location may be an older version's reading, so only a fresh fetch can close it.
    assert_eq!((report.pruned_stale, report.pruned_region), (1, 0));
    assert_eq!(state_and_note(&tracker, "test:1").0, State::Withdrawn);
    assert_eq!(state_and_note(&tracker, "test:2").0, State::Scored);
}
/// Stores a posting as first seen `days` days before `NOW` and queues it, as the discovery run
/// of that day left it.
fn first_seen_days_ago(tracker: &Tracker, job: &Job, days: i64) {
    let then = *NOW - TimeDelta::days(days);
    tracker
        .add_job(job, 70.0, &Default::default(), then)
        .unwrap();
    tracker.create_application(&job.key(), then).unwrap();
}
#[tokio::test]
async fn an_undated_posting_is_pruned_once_it_has_been_known_for_over_45_days() {
    let tracker = Tracker::in_memory().unwrap();
    // These postings carry no date (a source whose date field is missing or unparseable).
    let known_long = good("1", "Known Long Co");
    let known_a_while = good("2", "Known A While Co");
    let dated_recent = recently_dated("3", "Dated Recent Co");
    first_seen_days_ago(&tracker, &known_long, 46);
    first_seen_days_ago(&tracker, &known_a_while, 45);
    // First seen long ago, but the source now dates it three days ago: its date wins.
    first_seen_days_ago(&tracker, &dated_recent, 200);

    let report = discover_again(&tracker, vec![]).await;

    assert_eq!((report.pruned_stale, report.pruned_region), (1, 0));
    assert_eq!(
        state_and_note(&tracker, "test:1"),
        (State::Withdrawn, Some(STALE_NOTE.to_string()))
    );
    assert_eq!(state_and_note(&tracker, "test:2").0, State::Scored);
    assert_eq!(state_and_note(&tracker, "test:3").0, State::Scored);
}
#[tokio::test]
async fn rescore_does_not_queue_an_undated_posting_known_for_over_45_days() {
    let (criteria, exclusions) = pruning_rules();
    let tracker = Tracker::in_memory().unwrap();
    let strict = Criteria {
        apply_threshold: 99.0,
        ..criteria.clone()
    };
    let old = good("1", "Old Co");
    let new = good("2", "New Co");
    for (job, days_ago) in [(&old, 60), (&new, 10)] {
        discover(
            &[fake(vec![job.clone()], &[])],
            &strict,
            &exclusions,
            &tracker,
            *NOW - TimeDelta::days(days_ago),
        )
        .await
        .unwrap();
    }

    let report = rescore(&criteria, &tracker, *NOW).unwrap();

    assert_eq!((report.rescored, report.newly_queued), (1, 1));
    assert!(tracker.get_application("test:1").unwrap().is_none());
    assert!(tracker.get_application("test:2").unwrap().is_some());
}
#[tokio::test]
async fn a_fetch_that_lost_the_location_does_not_close_the_posting() {
    let first = good("1", "Acme AI");
    let tracker = stored_by_discovery(vec![first.clone()]).await;
    // A fetch without a location says nothing about it: the stored one stays.
    let blank = Job {
        location: String::new(),
        ..first
    };

    let report = discover_again(&tracker, vec![blank]).await;

    assert_eq!((report.refreshed, report.pruned_region), (0, 0));
    assert_eq!(state_and_note(&tracker, "test:1").0, State::Scored);
}
#[tokio::test]
async fn rescore_does_not_queue_a_stored_posting_that_is_out_of_region() {
    let (criteria, exclusions) = pruning_rules();
    let tracker = Tracker::in_memory().unwrap();
    let strict = Criteria {
        apply_threshold: 99.0,
        ..criteria.clone()
    };
    let first = good("1", "Acme AI");
    discover(
        &[fake(vec![first.clone()], &[])],
        &strict,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();
    // The next fetch finds it placed in a US city; the strict threshold still keeps it unqueued.
    let fresh = Job {
        location: "Austin, TX".into(),
        ..first
    };
    discover(
        &[fake(vec![fresh], &[])],
        &strict,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();

    // Gonza puts the threshold back: a posting nobody could do from Chile is not queued.
    assert_eq!(
        rescore(&criteria, &tracker, *NOW).unwrap(),
        RescoreReport::default()
    );
    assert!(tracker.get_application("test:1").unwrap().is_none());
}
#[tokio::test]
async fn postings_older_than_the_limit_are_counted_and_not_stored() {
    let (criteria, exclusions) = rules();
    let criteria = Criteria {
        max_posting_age_days: 45,
        ..criteria
    };
    let tracker = Tracker::in_memory().unwrap();
    let mut old = good("1", "Old Co");
    old.posted_at = Some(*NOW - TimeDelta::days(60));
    let mut fresh = good("2", "Fresh Co");
    fresh.posted_at = Some(*NOW - TimeDelta::days(10));
    // A posting without a date that is seen for the first time is not stale: it counts from
    // today (the prune withdraws it 45 days from now if it is still unsent).
    let undated = good("3", "Undated Co");

    let report = discover(
        &[fake(vec![old, fresh, undated], &[])],
        &criteria,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();

    assert_eq!((report.fetched, report.stale, report.queued), (3, 1, 2));
    assert!(tracker.get_job("test:1").unwrap().is_none());
    assert!(tracker.get_application("test:2").unwrap().is_some());
    assert!(tracker.get_application("test:3").unwrap().is_some());
}
#[tokio::test]
async fn rescore_does_not_queue_a_posting_that_has_become_too_old() {
    let (criteria, exclusions) = rules();
    let criteria = Criteria {
        max_posting_age_days: 45,
        ..criteria
    };
    let tracker = Tracker::in_memory().unwrap();
    let strict = Criteria {
        apply_threshold: 99.0,
        ..criteria.clone()
    };
    let mut job = good("1", "Acme AI");
    job.posted_at = Some(*NOW - TimeDelta::days(10));
    discover(
        &[fake(vec![job], &[])],
        &strict,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();

    // Sixty days later the same posting is 70 days old: it must not enter the queue now.
    let later = *NOW + TimeDelta::days(60);
    assert_eq!(
        rescore(&criteria, &tracker, later).unwrap(),
        RescoreReport::default()
    );
    assert!(tracker.get_application("test:1").unwrap().is_none());
}
#[tokio::test]
async fn second_run_is_idempotent() {
    let (criteria, exclusions) = rules();
    let tracker = Tracker::in_memory().unwrap();
    discover(&[fake(jobs(), &[])], &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    let again = discover(&[fake(jobs(), &[])], &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert_eq!(again.queued, 0);
    assert_eq!(again.below_floor_flagged, 0);
}
#[tokio::test]
async fn source_errors_are_reported() {
    let (criteria, exclusions) = rules();
    let tracker = Tracker::in_memory().unwrap();
    let sources = [
        fake(vec![], &["fake:x: boom"]),
        fake(jobs()[..1].to_vec(), &[]),
    ];

    let report = discover(&sources, &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert_eq!(lines(&report.errors), ["fake:x: boom"]);
    assert_eq!(report.queued, 1);
    assert_eq!(report.rate_limited, 0);
}
#[tokio::test]
async fn the_queries_a_host_asked_to_pause_are_counted_apart() {
    let (criteria, exclusions) = rules();
    let tracker = Tracker::in_memory().unwrap();
    let limited = FetchError::rate_limited(
        "fake",
        "rust",
        120,
        "a.test:443 asked us to slow down; not asking again for 120 s",
    );
    let sources: [Box<dyn Source>; 1] = [Box::new(FakeSource {
        jobs: vec![],
        errors: vec![limited.clone(), failed("fake:x: boom"), limited],
    })];

    let report = discover(&sources, &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert_eq!(report.rate_limited, 2);
    assert_eq!(report.errors.len(), 3, "every failure is still listed");
}
#[tokio::test]
async fn jobs_of_a_source_that_also_reports_errors_are_still_processed() {
    let (criteria, exclusions) = rules();
    let tracker = Tracker::in_memory().unwrap();
    let sources = [fake(jobs()[..1].to_vec(), &["fake:y: partial"])];

    let report = discover(&sources, &criteria, &exclusions, &tracker, *NOW)
        .await
        .unwrap();

    assert_eq!(lines(&report.errors), ["fake:y: partial"]);
    assert_eq!((report.fetched, report.queued), (1, 1));
}
#[tokio::test]
async fn rescore_requeues_stored_jobs_after_a_threshold_change() {
    let (criteria, exclusions) = rules();
    let tracker = Tracker::in_memory().unwrap();
    // Discovered under a threshold nothing reaches: stored, no application.
    let strict = Criteria {
        apply_threshold: 99.0,
        ..criteria.clone()
    };
    let report = discover(
        &[fake(jobs()[..1].to_vec(), &[])],
        &strict,
        &exclusions,
        &tracker,
        *NOW,
    )
    .await
    .unwrap();
    assert_eq!((report.below_threshold, report.queued), (1, 0));
    assert!(tracker.get_application("test:1").unwrap().is_none());

    // Gonza puts the threshold back: the stored posting is scored again and queued.
    let report = rescore(&criteria, &tracker, *NOW).unwrap();
    assert_eq!(
        report,
        RescoreReport {
            rescored: 1,
            newly_queued: 1,
            newly_flagged: 0
        }
    );
    assert_eq!(
        tracker.get_application("test:1").unwrap().unwrap().state,
        State::Scored
    );
    assert!(tracker.get_application("test:1").unwrap().unwrap().score >= 60.0);

    // Nothing is left without an application, so a second pass does nothing.
    assert_eq!(
        rescore(&criteria, &tracker, *NOW).unwrap(),
        RescoreReport::default()
    );
}
