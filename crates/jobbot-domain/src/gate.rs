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

/// The gate's reason for a host the bot never submits on (`MANUAL_ONLY_HOSTS`).
pub const REASON_MANUAL_ONLY: &str = "manual_only_platform";

/// The gate's reason for a page that is only a listing (`LISTING_ONLY_HOSTS`).
pub const REASON_LISTING_ONLY: &str = "listing_only";

/// The gate's reason for a posting older than the age limit.
pub const REASON_STALE: &str = "stale_posting";

/// The gate's reason for a posting outside the region (`region::region_ok_with`).
pub const REASON_OUT_OF_REGION: &str = "fuera_de_region";

/// The note of an application withdrawn for a region the job cannot be done from, by the prune or the gate. The
/// text before the colon is the rule.
pub const OUT_OF_REGION_NOTE: &str = "fuera_de_region: no se puede hacer desde Chile";

/// Whether a `needs_reason` is the one the region check parks an application with (`OUT_OF_REGION_NOTE`).
pub fn is_out_of_region_reason(reason: &str) -> bool {
    reason.starts_with(REASON_OUT_OF_REGION)
}

/// The note of the move that sends a parked out-of-region application back to the queue: Gonza looked at it and
/// chose to send it anyway. The event keeps it for good, so the region check no longer applies to that application
/// (`Store::region_override`). Only the region check: every other rule still does.
pub const REGION_OVERRIDE_NOTE: &str = "region_override: revisada por Gonza, se envía igual";

/// What an event note starts with when it is a region override (`REGION_OVERRIDE_NOTE`).
pub const REGION_OVERRIDE_PREFIX: &str = "region_override:";

#[derive(Debug, Clone, PartialEq)]
pub struct GateDecision {
    pub allowed: bool,
    /// Set when `allowed` is false: `stop_file`, `manual_only_platform`, `listing_only`,
    /// `excluded:<kind>` (the kind of rule, never its text: `domain`, `name` or `category`),
    /// `fuera_de_region`, `stale_posting`, `daily_cap`, `company_cooldown`, `warmup_hold` or
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
pub(crate) fn host_matches(url: &str, hosts: &[&str]) -> bool {
    let host = host_of(url);
    hosts
        .iter()
        .any(|blocked| host == *blocked || host.ends_with(&format!(".{blocked}")))
}

/// True when the URL's host is a manual-only platform or one of its subdomains.
pub fn is_manual_only(url: &str) -> bool {
    host_matches(url, MANUAL_ONLY_HOSTS)
}

/// True when the posting is on a manual-only platform by its own link or by its apply link: the
/// one rule the gate and discovery share for "Gonza applies to this one himself".
pub fn is_manual_only_posting(job: &Job) -> bool {
    is_manual_only(&job.apply_url) || is_manual_only(&job.url)
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
        "{REASON_STALE}: aviso de más de {} días",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_out_of_region_note_starts_with_its_reason() {
        assert!(OUT_OF_REGION_NOTE.starts_with(&format!("{REASON_OUT_OF_REGION}:")));
    }
}
