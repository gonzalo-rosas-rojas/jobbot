//! Discovery: fetch every source, drop what must not be applied to, score the rest and queue it.

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, TimeDelta, Utc};
use jobbot_domain::exclusion::check_exclusion;
use jobbot_domain::gate::{is_stale, stale_note};
use jobbot_domain::handoff::is_sent_by_hand;
use jobbot_domain::models::{FetchError, Job};
use jobbot_domain::rules::{Criteria, Exclusions};
use jobbot_domain::scoring::{ScoreResult, score_job};
use jobbot_domain::state::State;

use crate::apply::gate::is_stale_stored;
use crate::ports::Source;
use crate::ports::store::{INTENT_SUBMIT, INTERRUPTED_PREFIX, Store, StoreError};
use tracing::field::Empty;

// The region rules are pure and live in the domain crate.
pub use jobbot_domain::region::{
    in_extra_place, region_ok, region_ok_with, title_earns_extra_threshold,
};

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
    /// Of the `errors`, the parts of a source left alone because their host asked for a pause: the
    /// next run asks again once it is over.
    pub rate_limited: usize,
    pub errors: Vec<FetchError>,
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
#[tracing::instrument(
    skip_all,
    fields(
        sources = sources.len(),
        fetched = Empty,
        queued = Empty,
        errors = Empty,
        rate_limited = Empty,
    )
)]
pub async fn discover(
    sources: &[Box<dyn Source>],
    criteria: &Criteria,
    exclusions: &Exclusions,
    tracker: &dyn Store,
    now: DateTime<Utc>,
) -> anyhow::Result<DiscoveryReport> {
    let mut report = DiscoveryReport::default();
    // Keys of the postings the sources returned that were already known: a stored row with one of
    // these keys has the data its source reports today, which is what the region prune trusts.
    let mut refreshed_keys = HashSet::new();
    for source in sources {
        let result = source.fetch().await;
        report.rate_limited += result
            .errors
            .iter()
            .filter(|error| error.is_rate_limited())
            .count();
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
    crate::record_counts(&[
        ("fetched", report.fetched),
        ("queued", report.queued),
        ("errors", report.errors.len()),
        ("rate_limited", report.rate_limited),
    ]);
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
pub const MAY_HAVE_BEEN_SENT: [&str; 4] = [
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
#[tracing::instrument(skip_all, fields(stale = Empty, out_of_region = Empty))]
pub fn prune_ineligible(
    criteria: &Criteria,
    tracker: &dyn Store,
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
            Err(StoreError::IllegalTransition { .. }) => {}
            Err(other) => return Err(other.into()),
        }
    }
    crate::record_counts(&[
        ("stale", report.stale),
        ("out_of_region", report.out_of_region),
    ]);
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
    tracker: &dyn Store,
    now: DateTime<Utc>,
) -> anyhow::Result<Placement> {
    // A job in one of the extra places (Australia) gets the extra threshold and is never held back
    // for its pay: for those Gonza wants the offer, not a filter (2026-10-21), and an Australian
    // salary is not the dollar figure the floor was set for.
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
#[tracing::instrument(
    skip_all,
    fields(
        rescored = Empty,
        newly_queued = Empty,
        newly_flagged = Empty,
    )
)]
pub fn rescore(
    criteria: &Criteria,
    tracker: &dyn Store,
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
    crate::record_counts(&[
        ("rescored", report.rescored),
        ("newly_queued", report.newly_queued),
        ("newly_flagged", report.newly_flagged),
    ]);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use jobbot_domain::fixtures::make_job_with;

    use super::region_ok;

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
