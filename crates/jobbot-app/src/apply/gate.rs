//! The submission gate: every check that must pass before a form is submitted for real. The pure
//! rules (hosts, staleness, the decision type) are in `jobbot_domain::gate`, re-exported here.

use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use jobbot_domain::exclusion::check_exclusion;
use jobbot_domain::models::Job;
use jobbot_domain::rules::{Criteria, Exclusions};
use jobbot_domain::text::normalize;

use crate::ports::store::{ApplicationRow, Store};

pub use jobbot_domain::gate::*;

/// `is_stale_since` for a posting that is in the tracker: its first-seen day is read from there
/// (only when the posting has no date of its own). A posting that is not stored has no first
/// sighting yet, so it is as old as its own date says.
pub fn is_stale_stored(
    job: &Job,
    tracker: &dyn Store,
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
    tracker: &dyn Store,
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

/// Proof that the gate let one application through, and the only way to ask a `FormFiller` to
/// send a form (`FillMode::Live`). Its field is private to this module and only `check_gate`
/// makes one, so code that did not run the gate cannot send; the send it allows uses it up.
///
/// ```compile_fail,E0451
/// let approval = jobbot_app::apply::gate::GateApproval { _private: () };
/// ```
#[derive(Debug)]
pub struct GateApproval {
    _private: (),
}

/// What the gate said about one application, with the approval to send it when it let it through.
#[derive(Debug)]
pub struct Gated {
    pub decision: GateDecision,
    /// `Some` exactly when `decision.allowed`.
    pub approval: Option<GateApproval>,
}

/// Runs the checks in a fixed order and reports the first one that blocks; when none does, it
/// hands out the approval to send.
pub fn check_gate(
    job: &Job,
    app: &ApplicationRow,
    tracker: &dyn Store,
    criteria: &Criteria,
    exclusions: &Exclusions,
    stop_file: &Path,
    now: DateTime<Utc>,
) -> anyhow::Result<Gated> {
    let decision = decide(job, app, tracker, criteria, exclusions, stop_file, now)?;
    let approval = decision.allowed.then_some(GateApproval { _private: () });
    Ok(Gated { decision, approval })
}

fn decide(
    job: &Job,
    app: &ApplicationRow,
    tracker: &dyn Store,
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
    use super::*;

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
}
