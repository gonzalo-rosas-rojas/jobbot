//! The submission gate's pure rules: which hosts the bot never submits on, which pages are only
//! listings, when a posting is stale, and the decision type. The checks that read the database
//! (caps, cool-downs, the warm-up hold) are in the application's `gate` module.

use chrono::{DateTime, TimeDelta, Utc};

use crate::models::Job;
use crate::rules::Criteria;
use crate::text::host_of;

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
    pub fn allow() -> Self {
        GateDecision {
            allowed: true,
            reason: None,
        }
    }

    pub fn block(reason: impl Into<String>) -> Self {
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
