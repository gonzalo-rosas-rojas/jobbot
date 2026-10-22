//! The submission gate: every check that must pass before a form is submitted for real.

use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};

use crate::exclusion::check_exclusion;
use crate::models::Job;
use crate::rules::{Criteria, Exclusions};
use crate::text::{host_of, normalize};
use crate::tracker::{ApplicationRow, Tracker};

/// Hosts where the bot never submits: LinkedIn and the marketplaces with proposal auctions.
pub const MANUAL_ONLY_HOSTS: &[&str] = &[
    "linkedin.com",
    "upwork.com",
    "freelancer.com",
    "fiverr.com",
    "contra.com",
    "toptal.com",
];

/// Hosts whose job pages are listings, not forms: the apply button on the page leads elsewhere
/// (Remotive, RemoteOK, Himalayas, Jobicy, Hacker News, We Work Remotely, RemoteYeah, Working
/// Nomads, 4 Day Week, the python.org job board) or needs an account on their own site (Get on
/// Board), and the Workable (also its jobs.workable.com search), Recruitee, SmartRecruiters and
/// iCIMS (Atlassian's portal) forms are not driven yet, so the browser is never opened for them and
/// Gonza applies from the link in the digest.
pub const LISTING_ONLY_HOSTS: &[&str] = &[
    "remotive.com",
    "remoteok.com",
    "getonbrd.com",
    "himalayas.app",
    "jobicy.com",
    "news.ycombinator.com",
    "weworkremotely.com",
    "remoteyeah.com",
    "workingnomads.com",
    "4dayweek.io",
    "python.org",
    "workable.com",
    "recruitee.com",
    "smartrecruiters.com",
    "icims.com",
];

#[derive(Debug, Clone, PartialEq)]
pub struct GateDecision {
    pub allowed: bool,
    /// Set when `allowed` is false: `stop_file`, `manual_only_platform`, `listing_only`,
    /// `excluded:<why>`, `stale_posting`, `daily_cap`, `company_cooldown`, `warmup_hold` or
    /// `review_required`.
    pub reason: Option<String>,
}

impl GateDecision {
    fn allow() -> Self {
        GateDecision {
            allowed: true,
            reason: None,
        }
    }

    fn block(reason: impl Into<String>) -> Self {
        GateDecision {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

/// True when the URL's host is one of `hosts` or a subdomain of one.
fn host_matches(url: &str, hosts: &[&str]) -> bool {
    let host = host_of(url);
    hosts
        .iter()
        .any(|blocked| host == *blocked || host.ends_with(&format!(".{blocked}")))
}

/// True when the URL's host is a manual-only platform or one of its subdomains.
pub fn is_manual_only(url: &str) -> bool {
    host_matches(url, MANUAL_ONLY_HOSTS)
}

/// True when the URL points at a listing site whose apply button leads elsewhere.
pub fn is_listing_only(url: &str) -> bool {
    host_matches(url, LISTING_ONLY_HOSTS)
}

/// True when the posting is older than `criteria.max_posting_age_days` by its own date. A posting
/// without a date is never stale here: for the age of a posting we have seen before, use
/// `is_stale_stored`. Discovery uses this one on a posting it has not stored yet, so an old
/// posting is not queued just to be withdrawn.
pub fn is_stale(job: &Job, criteria: &Criteria, now: DateTime<Utc>) -> bool {
    is_stale_since(job, None, criteria, now)
}

/// The note of an application withdrawn because its posting is stale (Spanish, shown in the
/// digest and the panel): the gate's reason, then the age limit. The submission flow and the
/// discovery prune both write exactly this text.
pub fn stale_note(criteria: &Criteria) -> String {
    format!(
        "stale_posting: aviso de más de {} días",
        criteria.max_posting_age_days
    )
}

/// Like `is_stale`, but a posting without a date counts from `first_seen`, the day we first saw
/// it: a posting can arrive without a date (an item whose date field is missing or unparseable
/// from a source that usually gives one, e.g. Remotive, Workable, Greenhouse), and without a
/// fallback such a posting would never be withdrawn. The posting's own date wins when
/// there is one; with neither date the posting is not stale.
pub fn is_stale_since(
    job: &Job,
    first_seen: Option<DateTime<Utc>>,
    criteria: &Criteria,
    now: DateTime<Utc>,
) -> bool {
    job.posted_at
        .or(first_seen)
        .is_some_and(|since| now - since > TimeDelta::days(criteria.max_posting_age_days))
}

/// `is_stale_since` for a posting that is in the tracker: its first-seen day is read from there
/// (only when the posting has no date of its own). A posting that is not stored has no first
/// sighting yet, so it is as old as its own date says.
pub fn is_stale_stored(
    job: &Job,
    tracker: &Tracker,
    criteria: &Criteria,
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    let first_seen = if job.posted_at.is_none() {
        tracker.job_first_seen(&job.key())?
    } else {
        None
    };
    Ok(is_stale_since(job, first_seen, criteria, now))
}

fn in_warmup_hold(
    job: &Job,
    tracker: &Tracker,
    criteria: &Criteria,
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    let company = normalize(&job.company);
    if !criteria
        .priority_companies
        .iter()
        .any(|name| normalize(name) == company)
    {
        return Ok(false);
    }
    let release = tracker
        .first_seen_at()?
        .map(|start| start + TimeDelta::days(criteria.warmup_days));
    Ok(release.is_some_and(|release| now < release))
}

/// Runs the checks in a fixed order and reports the first one that blocks.
pub fn check_gate(
    job: &Job,
    app: &ApplicationRow,
    tracker: &Tracker,
    criteria: &Criteria,
    exclusions: &Exclusions,
    stop_file: &Path,
    now: DateTime<Utc>,
) -> anyhow::Result<GateDecision> {
    if stop_file.exists() {
        return Ok(GateDecision::block("stop_file"));
    }
    if is_manual_only(&job.apply_url) || is_manual_only(&job.url) {
        return Ok(GateDecision::block("manual_only_platform"));
    }
    if is_listing_only(&job.apply_url) {
        return Ok(GateDecision::block("listing_only"));
    }
    let exclusion = check_exclusion(job, exclusions);
    if exclusion.excluded {
        return Ok(GateDecision::block(format!(
            "excluded:{}",
            exclusion.reason.unwrap_or_default()
        )));
    }
    // An old posting is probably filled, and applying would also burn the company cooldown.
    if is_stale_stored(job, tracker, criteria, now)? {
        return Ok(GateDecision::block("stale_posting"));
    }
    // Until the first N submissions are approved the lower daily cap applies. The cap counts a
    // rolling 24 hours: the evening run window crosses midnight in UTC, Santiago and New York
    // alike, so a calendar day would reset it in the middle of a night.
    let in_review_phase =
        tracker.approved_submitted_total()? < i64::from(criteria.first_n_reviewed);
    let cap = if in_review_phase {
        criteria.daily_cap_until_reviewed
    } else {
        criteria.daily_cap
    };
    // Clicks the page did not confirm count too: any of them may have gone through.
    if tracker.clicked_since(now - TimeDelta::hours(24))? >= i64::from(cap) {
        return Ok(GateDecision::block("daily_cap"));
    }
    if tracker.company_in_cooldown(
        &normalize(&job.company),
        criteria.company_cooldown_days,
        now,
    )? {
        return Ok(GateDecision::block("company_cooldown"));
    }
    if in_warmup_hold(job, tracker, criteria, now)? {
        return Ok(GateDecision::block("warmup_hold"));
    }
    if in_review_phase && !app.approved {
        return Ok(GateDecision::block("review_required"));
    }
    Ok(GateDecision::allow())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use chrono::TimeDelta;

    use super::*;
    use crate::paths::Paths;
    use crate::rules::{load_criteria, load_exclusions};
    use crate::testing::{NOW, make_job_with};
    use crate::tracker::State;

    fn criteria() -> Criteria {
        let path = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .profile_dir
            .join("criteria.toml");
        load_criteria(&path).unwrap()
    }

    fn exclusions() -> Exclusions {
        let path = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .profile_dir
            .join("exclusions.toml");
        load_exclusions(&path).unwrap()
    }

    /// Adds the job, moves it to `prepared`, and optionally approves and submits it.
    fn prepared(tracker: &Tracker, job: &Job, approved: bool, submit: bool) -> ApplicationRow {
        let key = job.key();
        tracker.add_job(job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        tracker.create_application(&key, *NOW).unwrap();
        tracker
            .transition(&key, State::Prepared, None, *NOW)
            .unwrap();
        if approved {
            tracker.approve(&key).unwrap();
        }
        if submit {
            tracker
                .transition(&key, State::Submitted, None, *NOW)
                .unwrap();
        }
        tracker.get_application(&key).unwrap().unwrap()
    }

    fn decide(
        tracker: &Tracker,
        job: &Job,
        criteria: &Criteria,
        stop_file: &Path,
        now: DateTime<Utc>,
    ) -> GateDecision {
        let row = tracker.get_application(&job.key()).unwrap().unwrap();
        check_gate(job, &row, tracker, criteria, &exclusions(), stop_file, now).unwrap()
    }

    fn no_stop_file() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let stop = dir.path().join("STOP");
        (dir, stop)
    }

    #[test]
    fn review_required_until_approved() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|_| {});
        prepared(&tracker, &job, false, false);
        let decision = decide(&tracker, &job, &criteria(), &stop, *NOW);
        assert!(!decision.allowed);
        assert_eq!(decision.reason.as_deref(), Some("review_required"));
        tracker.approve(&job.key()).unwrap();
        assert!(decide(&tracker, &job, &criteria(), &stop, *NOW).allowed);
    }

    #[test]
    fn stop_file_wins_over_everything() {
        let (_dir, stop) = no_stop_file();
        std::fs::write(&stop, "x").unwrap();
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| j.apply_url = "https://www.linkedin.com/jobs/view/1".into());
        prepared(&tracker, &job, true, false);
        let decision = decide(&tracker, &job, &criteria(), &stop, *NOW);
        assert_eq!(decision.reason.as_deref(), Some("stop_file"));
    }

    #[test]
    fn manual_only_platform() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| j.apply_url = "https://www.linkedin.com/jobs/view/1".into());
        prepared(&tracker, &job, true, false);
        let decision = decide(&tracker, &job, &criteria(), &stop, *NOW);
        assert_eq!(decision.reason.as_deref(), Some("manual_only_platform"));
    }

    #[test]
    fn manual_only_matches_subdomains_but_not_lookalikes() {
        assert!(is_manual_only("https://www.upwork.com/jobs/1"));
        assert!(is_manual_only("https://contra.com/opportunity/1"));
        assert!(!is_manual_only("https://notlinkedin.com/jobs/1"));
        assert!(!is_manual_only(
            "https://example.com/apply?ref=linkedin.com"
        ));
        assert!(!is_manual_only(""));
    }

    #[test]
    fn listing_only_sites_go_to_gonza_without_a_browser() {
        assert!(is_listing_only(
            "https://remotive.com/remote-jobs/software-dev/x-123"
        ));
        assert!(!is_listing_only("https://jobs.lever.co/acme/1/apply"));
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| {
            j.apply_url = "https://remotive.com/remote-jobs/software-dev/x-123".into()
        });
        prepared(&tracker, &job, true, false);
        assert_eq!(
            decide(&tracker, &job, &criteria(), &stop, *NOW)
                .reason
                .as_deref(),
            Some("listing_only")
        );
    }

    #[test]
    fn aggregators_and_ats_pages_the_filler_cannot_drive_yet_are_listing_only() {
        // Himalayas, Jobicy and Hacker News only link to the job; the Workable, Recruitee and
        // SmartRecruiters forms are not supported by the filler yet. Recruitee posts live on
        // `{company}.recruitee.com`.
        for url in [
            "https://himalayas.app/companies/acme/jobs/senior-engineer",
            "https://jobicy.com/jobs/12345-senior-engineer",
            "https://news.ycombinator.com/item?id=45123456",
            "https://apply.workable.com/huggingface/j/ABC123/",
            "https://bunq.recruitee.com/o/senior-engineer",
            "https://jobs.smartrecruiters.com/BoschGroup/743999999999999-senior-engineer",
            "https://careers-apac-atlassian.icims.com/jobs/25538/backend-software-engineer/job?mode=apply",
        ] {
            assert!(is_listing_only(url), "{url} must be listing-only");
        }
        assert!(!is_listing_only("https://notworkable.com/jobs/1"));
        assert!(!is_listing_only("https://noticims.com/jobs/1"));
    }

    #[test]
    fn aggregators_whose_apply_button_leads_to_the_company_are_listing_only() {
        // We Work Remotely, RemoteYeah, Working Nomads, 4 Day Week and the python.org job board
        // only link to the job; Workable's own search lives on jobs.workable.com.
        for url in [
            "https://weworkremotely.com/remote-jobs/stripe-abuse-research-engineer",
            "https://remoteyeah.com/jobs/remote-staff-software-engineer-cision-6",
            "https://www.workingnomads.com/job/go/1878435/",
            "https://4dayweek.io/job/full-stack-engineer-finance-at-ledgebrook-f2cc121f",
            "https://www.python.org/jobs/8139/",
            "https://jobs.workable.com/view/6toeS2gWbTnuz4KdnKCtRP/remote-software-engineer-in-chile-at-enroute",
        ] {
            assert!(is_listing_only(url), "{url} must be listing-only");
        }
        assert!(!is_listing_only("https://notpython.org/jobs/1"));
        assert!(!is_listing_only(
            "https://example.com/apply?ref=weworkremotely.com"
        ));
    }

    #[test]
    fn job_boards_whose_apply_button_needs_their_own_account_are_listing_only() {
        // Get on Board applications happen inside an account on their site, and RemoteOK's apply
        // link is its own redirect page: the form filler must never open either (it once scanned
        // the Get on Board search box, "Search jobs: MySQL, Service Designer, Vue...").
        for url in [
            "https://www.getonbrd.com/jobs/senior-full-stack-engineer-worthy-remote",
            "https://getonbrd.com/jobs/programming/some-job",
            "https://remoteOK.com/remote-jobs/remote-junior-crypto-analyst-1137460",
            "https://remoteok.com/l/1137460",
        ] {
            assert!(is_listing_only(url), "{url} must be listing-only");
        }
        // Lookalikes and real ATS hosts stay open to the filler.
        assert!(!is_listing_only("https://notgetonbrd.com/jobs/1"));
        assert!(!is_listing_only(
            "https://job-boards.greenhouse.io/acme/jobs/1"
        ));
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| {
            j.apply_url =
                "https://www.getonbrd.com/jobs/senior-full-stack-engineer-worthy-remote".into()
        });
        prepared(&tracker, &job, true, false);
        assert_eq!(
            decide(&tracker, &job, &criteria(), &stop, *NOW)
                .reason
                .as_deref(),
            Some("listing_only")
        );
    }

    #[test]
    fn stale_posting_is_blocked() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let crit = Criteria {
            first_n_reviewed: 0,
            max_posting_age_days: 45,
            ..criteria()
        };
        let stale = make_job_with(|j| j.posted_at = Some(*NOW - TimeDelta::days(60)));
        prepared(&tracker, &stale, true, false);
        assert_eq!(
            decide(&tracker, &stale, &crit, &stop, *NOW)
                .reason
                .as_deref(),
            Some("stale_posting")
        );
        // A recent posting, or one without a date that was first seen just now, passes.
        let fresh = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Fresh Co".into();
            j.posted_at = Some(*NOW - TimeDelta::days(10));
        });
        prepared(&tracker, &fresh, true, false);
        assert!(decide(&tracker, &fresh, &crit, &stop, *NOW).allowed);
        let undated = make_job_with(|j| {
            j.external_id = "3".into();
            j.company = "Undated Co".into();
            j.posted_at = None;
        });
        prepared(&tracker, &undated, true, false);
        assert!(decide(&tracker, &undated, &crit, &stop, *NOW).allowed);
    }

    /// Criteria for the staleness tests: nothing else in the gate can block the posting.
    fn stale_criteria() -> Criteria {
        Criteria {
            first_n_reviewed: 0,
            max_posting_age_days: 45,
            ..criteria()
        }
    }

    #[test]
    fn the_stale_note_names_the_gate_reason_and_the_age_limit() {
        let note = |days| {
            stale_note(&Criteria {
                max_posting_age_days: days,
                ..criteria()
            })
        };

        // The digest and the panel show this text, and the tests of other modules look for the
        // reason in front of the colon.
        assert_eq!(note(45), "stale_posting: aviso de más de 45 días");
        assert_eq!(note(30), "stale_posting: aviso de más de 30 días");
        assert!(note(45).starts_with("stale_posting:"));
    }

    #[test]
    fn an_undated_posting_counts_from_the_day_it_was_first_seen() {
        let crit = stale_criteria();
        let undated = make_job_with(|j| j.posted_at = None);
        let seen = |days_ago: i64| Some(*NOW - TimeDelta::days(days_ago));

        assert!(is_stale_since(&undated, seen(46), &crit, *NOW));
        assert!(!is_stale_since(&undated, seen(45), &crit, *NOW));
        assert!(!is_stale_since(&undated, seen(10), &crit, *NOW));
        // Never seen and never dated: nothing says it is old.
        assert!(!is_stale_since(&undated, None, &crit, *NOW));
        // The old entry point only knows the posting's own date.
        assert!(!is_stale(&undated, &crit, *NOW));
    }

    #[test]
    fn the_posting_date_wins_over_the_first_seen_day() {
        let crit = stale_criteria();
        let recent = make_job_with(|j| j.posted_at = Some(*NOW - TimeDelta::days(10)));
        let old = make_job_with(|j| j.posted_at = Some(*NOW - TimeDelta::days(60)));
        let long_ago = Some(*NOW - TimeDelta::days(200));
        let just_now = Some(*NOW);

        // Seen long ago but posted recently (the source re-listed it): not stale.
        assert!(!is_stale_since(&recent, long_ago, &crit, *NOW));
        // Posted long ago but first seen today (we only just started reading the board): stale.
        assert!(is_stale_since(&old, just_now, &crit, *NOW));
        assert!(is_stale(&old, &crit, *NOW));
    }

    #[test]
    fn a_stored_posting_is_aged_from_the_tracker_and_an_unstored_one_from_its_date() {
        let crit = stale_criteria();
        let tracker = Tracker::in_memory().unwrap();
        let undated = make_job_with(|j| j.posted_at = None);
        let later = |days: i64| *NOW + TimeDelta::days(days);

        // Not stored: no first sighting, so an undated posting is not stale however late it is.
        assert!(!is_stale_stored(&undated, &tracker, &crit, later(400)).unwrap());

        tracker
            .add_job(&undated, 70.0, &BTreeMap::new(), *NOW)
            .unwrap();
        assert!(!is_stale_stored(&undated, &tracker, &crit, later(45)).unwrap());
        assert!(is_stale_stored(&undated, &tracker, &crit, later(46)).unwrap());

        // A dated posting is aged by its date even when the tracker saw it much later.
        let dated = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Dated Co".into();
            j.posted_at = Some(*NOW - TimeDelta::days(60));
        });
        tracker
            .add_job(&dated, 70.0, &BTreeMap::new(), *NOW)
            .unwrap();
        assert!(is_stale_stored(&dated, &tracker, &crit, *NOW).unwrap());
    }

    #[test]
    fn an_undated_posting_is_blocked_once_it_has_been_known_for_over_45_days() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let crit = stale_criteria();
        let undated = make_job_with(|j| j.posted_at = None);
        prepared(&tracker, &undated, true, false);

        let at = |days: i64| *NOW + TimeDelta::days(days);
        assert!(decide(&tracker, &undated, &crit, &stop, at(44)).allowed);
        assert_eq!(
            decide(&tracker, &undated, &crit, &stop, at(46))
                .reason
                .as_deref(),
            Some("stale_posting")
        );
    }

    /// An approved application submitted `hours` ago, at its own company.
    fn submitted_hours_ago(tracker: &Tracker, id: &str, hours: i64) {
        let job = make_job_with(|j| {
            j.external_id = id.into();
            j.company = format!("Co{id}");
        });
        tracker.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        tracker.create_application(&job.key(), *NOW).unwrap();
        tracker
            .transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        tracker.approve(&job.key()).unwrap();
        tracker
            .transition(
                &job.key(),
                State::Submitted,
                None,
                *NOW - TimeDelta::hours(hours),
            )
            .unwrap();
    }

    #[test]
    fn daily_cap_counts_the_last_24_hours_not_the_calendar_day() {
        let (_dir, stop) = no_stop_file();
        let crit = Criteria {
            first_n_reviewed: 1,
            daily_cap: 1,
            ..criteria()
        };
        let next = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "B".into();
        });
        // NOW is 12:00 UTC: a submission 13 hours ago fell on the previous UTC day (and the
        // previous Santiago day), yet it is inside the rolling window and fills the cap of 1.
        let tracker = Tracker::in_memory().unwrap();
        submitted_hours_ago(&tracker, "1", 13);
        prepared(&tracker, &next, true, false);
        assert_eq!(
            decide(&tracker, &next, &crit, &stop, *NOW)
                .reason
                .as_deref(),
            Some("daily_cap")
        );
        // 25 hours ago is outside the window: the cap is free again.
        let tracker = Tracker::in_memory().unwrap();
        submitted_hours_ago(&tracker, "1", 25);
        prepared(&tracker, &next, true, false);
        assert!(decide(&tracker, &next, &crit, &stop, *NOW).allowed);
    }

    #[test]
    fn exclusion_is_rechecked() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| j.company = "Tidewater Systems".into());
        prepared(&tracker, &job, true, false);
        let decision = decide(&tracker, &job, &criteria(), &stop, *NOW);
        assert!(decision.reason.unwrap().starts_with("excluded:"));
    }

    #[test]
    fn daily_cap() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let first = make_job_with(|j| {
            j.external_id = "1".into();
            j.company = "A".into();
            j.title = "One".into();
        });
        prepared(&tracker, &first, true, true);
        let second = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "B".into();
            j.title = "Two".into();
        });
        prepared(&tracker, &second, true, false);
        // One approved submission already counts as "reviewed", so the full cap applies.
        let crit = Criteria {
            first_n_reviewed: 1,
            daily_cap: 1,
            ..criteria()
        };
        let decision = decide(&tracker, &second, &crit, &stop, *NOW);
        assert_eq!(decision.reason.as_deref(), Some("daily_cap"));
    }

    #[test]
    fn lower_cap_applies_until_the_first_submissions_are_approved() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let first = make_job_with(|j| {
            j.external_id = "1".into();
            j.company = "A".into();
        });
        prepared(&tracker, &first, true, true);
        let second = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "B".into();
        });
        prepared(&tracker, &second, true, false);
        let reviewing = Criteria {
            first_n_reviewed: 5,
            daily_cap_until_reviewed: 1,
            daily_cap: 25,
            ..criteria()
        };
        assert_eq!(
            decide(&tracker, &second, &reviewing, &stop, *NOW)
                .reason
                .as_deref(),
            Some("daily_cap")
        );
        let reviewed = Criteria {
            first_n_reviewed: 1,
            ..reviewing
        };
        assert!(decide(&tracker, &second, &reviewed, &stop, *NOW).allowed);
    }

    #[test]
    fn company_cooldown() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let first = make_job_with(|j| {
            j.external_id = "1".into();
            j.title = "Role A".into();
        });
        prepared(&tracker, &first, true, true);
        let second = make_job_with(|j| {
            j.external_id = "2".into();
            j.title = "Role B".into();
        });
        prepared(&tracker, &second, true, false);
        let decision = decide(&tracker, &second, &criteria(), &stop, *NOW);
        assert_eq!(decision.reason.as_deref(), Some("company_cooldown"));
    }

    #[test]
    fn warmup_hold_then_release() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|_| {});
        prepared(&tracker, &job, true, false);
        let crit = Criteria {
            priority_companies: vec!["Acme AI".to_string()],
            warmup_days: 14,
            ..criteria()
        };
        assert_eq!(
            decide(&tracker, &job, &crit, &stop, *NOW).reason.as_deref(),
            Some("warmup_hold")
        );
        let later = *NOW + TimeDelta::days(15);
        assert!(decide(&tracker, &job, &crit, &stop, later).allowed);
    }

    #[test]
    fn a_manual_only_listing_link_is_caught_even_when_the_apply_link_is_not() {
        let (_dir, stop) = no_stop_file();
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| j.url = "https://www.linkedin.com/jobs/view/1".into());
        prepared(&tracker, &job, true, false);
        let decision = decide(&tracker, &job, &criteria(), &stop, *NOW);
        assert_eq!(decision.reason.as_deref(), Some("manual_only_platform"));
    }
}
