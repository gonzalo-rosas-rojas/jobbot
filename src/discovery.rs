//! Discovery: fetch every source, drop what must not be applied to, score the rest and queue it.

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, TimeDelta, Utc};

use crate::exclusion::check_exclusion;
use crate::gate::{is_stale, is_stale_stored, stale_note};
use crate::handoff::is_sent_by_hand;
use crate::models::Job;
use crate::rules::{Criteria, Exclusions};
use crate::scoring::{ScoreResult, score_job};
use crate::sources::Source;
use crate::tracker::{INTENT_SUBMIT, INTERRUPTED_PREFIX, State, Tracker, TrackerError};

mod region;

pub use region::{in_extra_place, region_ok, region_ok_with, title_earns_extra_threshold};

/// The note stored with a below-floor job that goes to `needs_gonza`. The text after the colon is
/// shown to Gonza in the digest (Spanish): "learning or stability".
pub const BELOW_FLOOR_NOTE: &str = "below_floor_exception: aprendizaje o estabilidad";

/// What one discovery run did. `fetched` counts every job a source returned; the other counters
/// say what happened to it (a job lands in exactly one of them, except `errors`).
#[derive(Debug, Default)]
pub struct DiscoveryReport {
    pub fetched: usize,
    pub already_seen: usize,
    /// Of the `already_seen` postings, those whose stored link, date, location or remote flag the
    /// source now reports differently (and the refresh rewrote).
    pub refreshed: usize,
    pub excluded: usize,
    pub region_skipped: usize,
    /// Postings older than `max_posting_age_days`: not stored, because the gate would withdraw
    /// them at submission anyway.
    pub stale: usize,
    pub below_floor_flagged: usize,
    pub below_threshold: usize,
    pub queued: usize,
    /// Applications withdrawn after the loop because their posting is now too old.
    pub pruned_stale: usize,
    /// Applications withdrawn after the loop because their posting cannot be done from Chile.
    pub pruned_region: usize,
    pub errors: Vec<String>,
}

/// Runs every source and processes each job in this order: already stored, excluded, outside the
/// region, too old, scored. Excluded, region-skipped and stale jobs are not stored. Every other job
/// is stored with its score; a job at or above `apply_threshold` also gets an application: state
/// `Scored`, or `NeedsGonza` with `BELOW_FLOOR_NOTE` when its pay is below the floor (never
/// auto-applied).
///
/// A job already stored is not scored again, but its link, date, location and remote flag are
/// rewritten from the fetch (`Tracker::refresh_posting`): an earlier version of a source may have
/// read them wrongly. After the loop, `prune_ineligible` withdraws the unsent applications whose
/// posting is now stale or out of region.
pub async fn discover(
    sources: &[Box<dyn Source>],
    criteria: &Criteria,
    exclusions: &Exclusions,
    tracker: &Tracker,
    now: DateTime<Utc>,
) -> anyhow::Result<DiscoveryReport> {
    let mut report = DiscoveryReport::default();
    // Keys of the postings the sources returned that were already known: a stored row with one of
    // these keys has the data its source reports today, which is what the region prune trusts.
    let mut refreshed_keys = HashSet::new();
    for source in sources {
        let result = source.fetch().await;
        report.errors.extend(result.errors);
        for job in result.jobs {
            report.fetched += 1;
            // The same role posted again once the old posting is stale is a new posting.
            let repost_cutoff = now - TimeDelta::days(criteria.max_posting_age_days);
            if tracker.has_posting_since(&job, repost_cutoff)? {
                report.already_seen += 1;
                if tracker.refresh_posting(&job)? {
                    report.refreshed += 1;
                }
                refreshed_keys.insert(job.key());
                continue;
            }
            if check_exclusion(&job, exclusions).excluded {
                report.excluded += 1;
                continue;
            }
            if !region_ok_with(&job, &criteria.extra_open_places) {
                report.region_skipped += 1;
                continue;
            }
            // Not stored yet, so it has no first-seen day: only its own date can make it stale
            // (an undated posting starts counting from today).
            if is_stale(&job, criteria, now) {
                report.stale += 1;
                continue;
            }
            let scored = score_job(&job, criteria);
            tracker.add_job(&job, scored.total, &detail_with_floor(&scored), now)?;
            match place(&job, &scored, criteria, tracker, now)? {
                Placement::BelowThreshold => report.below_threshold += 1,
                Placement::Flagged => report.below_floor_flagged += 1,
                Placement::Queued => report.queued += 1,
            }
        }
    }
    let pruned = prune_ineligible(criteria, tracker, &refreshed_keys, now)?;
    report.pruned_stale = pruned.stale;
    report.pruned_region = pruned.out_of_region;
    Ok(report)
}

/// What a prune pass withdrew, counted by the rule that decided each application.
#[derive(Debug, Default, PartialEq)]
pub struct PruneReport {
    pub stale: usize,
    pub out_of_region: usize,
}

/// The note of an application withdrawn for a region the job cannot be done from. The text before
/// the colon is the rule that decided it; the stale note is the submission gate's own
/// (`gate::stale_note`, `stale_posting: aviso de más de N días`).
const OUT_OF_REGION_NOTE: &str = "fuera_de_region: no se puede hacer desde Chile";

/// Reasons under which a form may have gone out although the application never reached
/// `Submitted`: the page did not confirm a click, or Gonza finished an assisted form himself and
/// closed it, or the run that clicked it died before recording the result. Withdrawing such an
/// application would hide a real submission. The forms Gonza sends by hand
/// (`handoff::is_sent_by_hand`) are protected the same way: he may have sent one and not yet run
/// `confirm`, and withdrawn is terminal.
///
/// The panel sorts a waiting card by the same prefixes (`panel::snapshot::reason_kind`), so it can
/// tell Gonza to look before he applies again.
pub(crate) const MAY_HAVE_BEEN_SENT: [&str; 4] = [
    "unconfirmed_submit",
    INTERRUPTED_PREFIX,
    "assist_closed",
    "assist_timeout",
];

fn may_have_been_sent(reason: &str) -> bool {
    is_sent_by_hand(reason)
        || MAY_HAVE_BEEN_SENT
            .iter()
            .any(|prefix| reason.starts_with(prefix))
}

/// Withdraws the applications that were never sent and whose posting is not eligible any more.
/// Withdrawal is terminal, so the pass only acts on what is clear:
///
/// * Only `Scored`, `Prepared` and `NeedsGonza` applications with no submission and no unconfirmed
///   click are looked at (`Tracker::unsent_applications`), minus the `NeedsGonza` ones whose reason
///   says the form may have gone out, and minus any application with a submit intent still open
///   (a run is clicking it or died clicking it). Everything submitted, answered, finished or
///   withdrawn stays.
/// * A stale posting (the gate's `is_stale_stored`: the posting's date, or for a source that
///   publishes none, the day it was first seen) is withdrawn with `stale_posting`. The date needs
///   no fresh fetch: the submission gate would withdraw the same row anyway.
/// * A posting outside the region (`region_ok`) is withdrawn with `fuera_de_region`, but only when
///   its key is in `refreshed`: the source returned it in this run, so its location and remote
///   flag are what the source says now. A row the sources no longer return keeps the location it
///   was stored with, which an older version of a source may have read badly.
/// * Eligibility beats the reason an application waits: a `NeedsGonza` application that waits for
///   Gonza for another reason (missing answers, below the pay floor) is withdrawn too.
///
/// A second pass finds nothing new: withdrawn applications are not looked at again.
pub fn prune_ineligible(
    criteria: &Criteria,
    tracker: &Tracker,
    refreshed: &HashSet<String>,
    now: DateTime<Utc>,
) -> anyhow::Result<PruneReport> {
    let mut report = PruneReport::default();
    // A submit intent still open means a run is clicking that form right now, or died doing it:
    // the application may have gone out although nothing says so yet (`recover_open_intents` runs
    // at the start of the next submit run). It stays as it is until the intent is closed.
    let clicking: HashSet<String> = tracker
        .open_intents(INTENT_SUBMIT)?
        .into_iter()
        .map(|intent| intent.job_key)
        .collect();
    for row in tracker.unsent_applications()? {
        if clicking.contains(&row.job_key)
            || (row.state == State::NeedsGonza
                && row.needs_reason.as_deref().is_some_and(may_have_been_sent))
        {
            continue;
        }
        let Some(job) = tracker.get_job(&row.job_key)? else {
            continue;
        };
        let (note, counter) = if is_stale_stored(&job, tracker, criteria, now)? {
            (stale_note(criteria), &mut report.stale)
        } else if refreshed.contains(&row.job_key)
            && !region_ok_with(&job, &criteria.extra_open_places)
        {
            (OUT_OF_REGION_NOTE.to_string(), &mut report.out_of_region)
        } else {
            continue;
        };
        match tracker.transition(&row.job_key, State::Withdrawn, Some(&note), now) {
            Ok(()) => *counter += 1,
            // The panel or `mark` moved it since the list was read: that decision stands.
            Err(TrackerError::IllegalTransition { .. }) => {}
            Err(other) => return Err(other.into()),
        }
    }
    Ok(report)
}

/// Where a scored job ends up: stored only, queued, or flagged as a below-floor exception.
enum Placement {
    BelowThreshold,
    Queued,
    Flagged,
}

/// The score detail plus the `below_floor` marker the panel reads.
fn detail_with_floor(scored: &ScoreResult) -> BTreeMap<String, f64> {
    let mut detail = scored.detail.clone();
    detail.insert(
        "below_floor".to_string(),
        if scored.below_floor { 1.0 } else { 0.0 },
    );
    detail
}

/// Opens the application of a job that reaches the threshold: `Scored`, or `NeedsGonza` with
/// `BELOW_FLOOR_NOTE` when its pay is below the floor. Shared by `discover` and `rescore`.
fn place(
    job: &Job,
    scored: &ScoreResult,
    criteria: &Criteria,
    tracker: &Tracker,
    now: DateTime<Utc>,
) -> anyhow::Result<Placement> {
    // A job in one of the extra places (Australia) gets the extra threshold and is never held back
    // for its pay: an Australian salary is not the dollar figure the floor was set for.
    let in_extra_place = in_extra_place(job, &criteria.extra_open_places)
        && title_earns_extra_threshold(job, &criteria.extra_open_title_words);
    let threshold = match criteria.extra_open_apply_threshold {
        Some(extra) if in_extra_place => extra,
        _ => criteria.apply_threshold,
    };
    if scored.total < threshold {
        return Ok(Placement::BelowThreshold);
    }
    tracker.create_application(&job.key(), now)?;
    if scored.below_floor && !in_extra_place {
        tracker.transition(&job.key(), State::NeedsGonza, Some(BELOW_FLOOR_NOTE), now)?;
        Ok(Placement::Flagged)
    } else {
        Ok(Placement::Queued)
    }
}

/// What `rescore` did with the stored postings that had no application.
#[derive(Debug, Default, PartialEq)]
pub struct RescoreReport {
    pub rescored: usize,
    pub newly_queued: usize,
    pub newly_flagged: usize,
}

/// Scores again every stored posting without an application, with the criteria as they are now,
/// and opens the applications that reach the threshold. `discover` never re-scores a stored
/// posting, so this is how a changed `criteria.toml` reaches what is already in the database.
///
/// It does not prune: a posting without an application has nothing to withdraw, and the prune
/// needs a fresh fetch, which `discover --rescore` makes right after (its `discover` prunes the
/// queue). It only keeps from queueing what that prune would withdraw at once.
pub fn rescore(
    criteria: &Criteria,
    tracker: &Tracker,
    now: DateTime<Utc>,
) -> anyhow::Result<RescoreReport> {
    let mut report = RescoreReport::default();
    for job in tracker.jobs_without_application()? {
        // Too old by now, or not doable from Chile: queueing it would only get it withdrawn. It
        // stays stored, so a later refresh that fixes its data lets a later rescore queue it.
        if is_stale_stored(&job, tracker, criteria, now)?
            || !region_ok_with(&job, &criteria.extra_open_places)
        {
            continue;
        }
        let scored = score_job(&job, criteria);
        tracker.set_score(&job.key(), scored.total, &detail_with_floor(&scored))?;
        report.rescored += 1;
        match place(&job, &scored, criteria, tracker, now)? {
            Placement::BelowThreshold => {}
            Placement::Flagged => report.newly_flagged += 1,
            Placement::Queued => report.newly_queued += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::{DiscoveryReport, RescoreReport, discover, region_ok, rescore};
    use crate::models::{FetchResult, Job};
    use crate::paths::Paths;
    use crate::rules::{Criteria, Exclusions, load_criteria, load_exclusions};
    use crate::sources::Source;
    use crate::testing::{NOW, make_job_with};
    use crate::tracker::{INTENT_SUBMIT, State, Tracker};
    use async_trait::async_trait;
    use chrono::TimeDelta;

    const GOOD_DESC: &str = "Build LLM agents with python, typescript, react, rag and mcp. \
                             Applied AI and generative AI work on agentic workflows, with an \
                             engineering culture and open source time. \
                             Full-time position with mentorship and a take-home exercise.";

    /// A source that returns a fixed result, so discovery is tested without any network.
    struct FakeSource {
        jobs: Vec<Job>,
        errors: Vec<String>,
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
            errors: errors.iter().map(|error| error.to_string()).collect(),
        })
    }

    fn rules() -> (Criteria, Exclusions) {
        let paths = Paths::new(env!("CARGO_MANIFEST_DIR"));
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

        assert_eq!(report.errors, ["fake:x: boom"]);
        assert_eq!(report.queued, 1);
    }

    #[tokio::test]
    async fn jobs_of_a_source_that_also_reports_errors_are_still_processed() {
        let (criteria, exclusions) = rules();
        let tracker = Tracker::in_memory().unwrap();
        let sources = [fake(jobs()[..1].to_vec(), &["fake:y: partial"])];

        let report = discover(&sources, &criteria, &exclusions, &tracker, *NOW)
            .await
            .unwrap();

        assert_eq!(report.errors, ["fake:y: partial"]);
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

    #[test]
    fn region_ok_table() {
        let cases = [
            ("Remote - USA", true, "", false),
            ("Remote", true, "", true),
            ("Remote - Worldwide", true, "", true),
            ("Santiago, Chile", false, "", true),
            ("Bogotá, Colombia", false, "", false),
            (
                "Remote",
                true,
                "Candidates must be located in the United States.",
                false,
            ),
            ("Remote (US)", true, "", false),
            // Austin is a US city now (this row once checked that `us` is not read inside
            // "austin"); "Mauritius" keeps that check.
            ("Remote - Austin", true, "", false),
            ("Remote - Mauritius", true, "", true),
            ("Remote - US, Canada or LATAM", true, "", true),
            // Remotive-style locations: a continent other than the Americas closes the job.
            ("Europe", true, "", false),
            ("UK", true, "", false),
            ("EMEA", true, "", false),
            ("Worldwide", true, "", true),
            ("LATAM", true, "", true),
            ("Remote - Europe or LATAM", true, "", true),
        ];
        for (location, remote, description, expected) in cases {
            let job = make_job_with(|job| {
                job.location = location.into();
                job.remote = remote;
                job.description = description.into();
            });
            assert_eq!(
                region_ok(&job),
                expected,
                "location {location:?}, remote {remote}, description {description:?}"
            );
        }
    }

    #[test]
    fn region_ok_empty_fields() {
        let job = make_job_with(|job| {
            job.location = String::new();
            job.description = String::new();
            job.remote = true;
        });
        assert!(region_ok(&job));
    }
}
