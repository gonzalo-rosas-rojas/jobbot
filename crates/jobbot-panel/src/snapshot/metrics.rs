//! The numbers of the page: the weekly metrics, the funnel of stages, the pipeline of the
//! postings seen and the gate's trust counts.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use serde::Serialize;

use super::cards::{file_on_disk, has_screenshot_on_disk};
use crate::Panel;
use jobbot_domain::rules::Criteria;
use jobbot_domain::state::{ANSWERED_STATES, State};

/// Whole scores 0 to 100 inclusive: one bucket each in the pipeline's score histogram.
const SCORE_LEVELS: usize = 101;

/// The weekly pipeline looks at the postings first seen in the last 7 days.
const PIPELINE_DAYS: i64 = 7;

#[derive(Debug, Serialize, PartialEq)]
pub struct Metrics {
    pub sent_week: i64,
    pub responses_week: i64,
    pub response_rate: Option<f64>,
    pub tests_week: i64,
    pub interviews_week: i64,
    pub interviews_total: i64,
    pub offers_total: i64,
    pub interview_to_offer: Option<f64>,
}

/// How far the applications have got, all time. Each application counts once per stage it ever
/// reached: it is the line of stations of the "Procesos" tab, not today's board. The technical
/// test is optional, so `interviews` can exceed `tests`; only `answered` bounds the three stages
/// after it.
#[derive(Debug, Serialize, PartialEq)]
pub struct Funnel {
    pub sent: i64,
    /// Any answer at all: a reply, a test, an interview, an offer or a rejection.
    pub answered: i64,
    pub tests: i64,
    pub interviews: i64,
    pub offers: i64,
    /// The same stages per source the posting came from, with the base `n` of each. Counts only:
    /// with a handful of sends per source a rate would be noise, so the page shows "2 of 11".
    pub by_source: BTreeMap<String, SourceFunnel>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SourceFunnel {
    /// Applications of the source, sent or not.
    pub n: i64,
    pub sent: i64,
    pub answered: i64,
    pub tests: i64,
    pub interviews: i64,
    pub offers: i64,
}

/// How far the postings first seen in the last `days` days got. Each count is "reached at least
/// this layer", so `sent <= complete <= with_cv <= above <= seen`.
#[derive(Debug, Serialize, PartialEq)]
pub struct Pipeline {
    pub days: i64,
    /// `apply_threshold` of `profile/criteria.toml`; `None` when it cannot be read.
    pub threshold: Option<f64>,
    pub seen: i64,
    /// With an application, whatever its state (withdrawn included).
    pub above: i64,
    /// With a CV on disk, or further along.
    pub with_cv: i64,
    /// Sent, or prepared with its dry-run screenshot on disk.
    pub complete: i64,
    pub sent: i64,
    /// 101 counts: how many of the `seen` have each score, rounded and held to 0..=100.
    pub score_counts: Vec<u32>,
}

/// How much room the engine has to send, as the gate counts it. `None` in the snapshot when
/// `profile/criteria.toml` cannot be read.
#[derive(Debug, Serialize, PartialEq)]
pub struct Trust {
    /// Submissions Gonza approved before they went out.
    pub reviewed: i64,
    pub first_n: u32,
    /// Fewer than `first_n` reviewed: the lower cap applies and each form needs his approval.
    pub review_phase: bool,
    /// Clicks of the last 24 hours, confirmed or not.
    pub sent_24h: i64,
    /// The daily cap in force now.
    pub cap: u32,
    /// The daily cap once the review phase is over.
    pub cap_after: u32,
}

/// The start of "all time": before any posting this program has stored, for the weekly numbers
/// that also count everything (`metrics`) and for the funnel (`funnel`).
fn all_time_from() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap()
}

fn ratio(part: i64, whole: i64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

pub(super) fn metrics(panel: &Panel, now: DateTime<Utc>) -> Result<Metrics> {
    let t = &panel.tracker;
    let week = now - TimeDelta::days(7);
    let ever = all_time_from();
    let sent_week = t.submitted_since(week)?;
    let responses_week = t.jobs_reaching(&ANSWERED_STATES, week)?;
    // The rate compares like with like: of what was sent this week, how much got an answer.
    let sent_week_answered = t.submitted_since_reaching(week, &ANSWERED_STATES)?;
    let interviews_total = t.jobs_reaching(&[State::Interview], ever)?;
    let offers_total = t.jobs_reaching(&[State::Offer], ever)?;
    let interview_then_offer = t.jobs_reaching_both(State::Interview, State::Offer)?;
    Ok(Metrics {
        sent_week,
        responses_week,
        response_rate: ratio(sent_week_answered, sent_week),
        tests_week: t.jobs_reaching(&[State::TechnicalTest], week)?,
        interviews_week: t.jobs_reaching(&[State::Interview], week)?,
        interviews_total,
        offers_total,
        interview_to_offer: ratio(interview_then_offer, interviews_total),
    })
}

pub(super) fn funnel(panel: &Panel) -> Result<Funnel> {
    let t = &panel.tracker;
    let ever = all_time_from();
    Ok(Funnel {
        sent: t.submitted_since(ever)?,
        answered: t.jobs_reaching(&ANSWERED_STATES, ever)?,
        tests: t.jobs_reaching(&[State::TechnicalTest], ever)?,
        interviews: t.jobs_reaching(&[State::Interview], ever)?,
        offers: t.jobs_reaching(&[State::Offer], ever)?,
        by_source: t
            .funnel_by_source()?
            .into_iter()
            .map(|row| {
                (
                    row.group,
                    SourceFunnel {
                        n: row.n,
                        sent: row.sent,
                        answered: row.answered,
                        tests: row.tests,
                        interviews: row.interviews,
                        offers: row.offers,
                    },
                )
            })
            .collect(),
    })
}

/// The weekly pipeline. The layers nest by construction: only an application can have a CV, a
/// screenshot or a submission, so the counts only ever shrink from `seen` to `sent`.
pub(super) fn pipeline(
    panel: &Panel,
    now: DateTime<Utc>,
    criteria: Option<&Criteria>,
) -> Result<Pipeline> {
    let mut layers = [0i64; 5];
    let mut score_counts = vec![0u32; SCORE_LEVELS];
    for row in panel
        .tracker
        .cohort_since(now - TimeDelta::days(PIPELINE_DAYS))?
    {
        let sent = row.submitted_at.is_some();
        // Unlike the approval queue, this does not ask whether it was approved.
        let complete = sent
            || (row.state == Some(State::Prepared)
                && has_screenshot_on_disk(row.screenshot_path.as_deref()));
        let with_cv = complete || file_on_disk(row.cv_path.as_deref());
        let above = row.state.is_some();
        for (count, reached) in layers
            .iter_mut()
            .zip([true, above, with_cv, complete, sent])
        {
            *count += i64::from(reached);
        }
        // A score is held to 0..=100 on the page's axis; NaN lands on 0.
        score_counts[row.score.round().clamp(0.0, (SCORE_LEVELS - 1) as f64) as usize] += 1;
    }
    let [seen, above, with_cv, complete, sent] = layers;
    Ok(Pipeline {
        days: PIPELINE_DAYS,
        threshold: criteria.map(|c| c.apply_threshold),
        seen,
        above,
        with_cv,
        complete,
        sent,
        score_counts,
    })
}

/// The gate's own numbers: reviewed sends, clicks of the last 24 hours and the cap in force.
pub(super) fn trust(
    panel: &Panel,
    now: DateTime<Utc>,
    criteria: Option<&Criteria>,
) -> Result<Option<Trust>> {
    let Some(criteria) = criteria else {
        return Ok(None);
    };
    let reviewed = panel.tracker.approved_submitted_total()?;
    let review_phase = reviewed < i64::from(criteria.first_n_reviewed);
    Ok(Some(Trust {
        reviewed,
        first_n: criteria.first_n_reviewed,
        review_phase,
        sent_24h: panel.tracker.clicked_since(now - TimeDelta::hours(24))?,
        cap: if review_phase {
            criteria.daily_cap_until_reviewed
        } else {
            criteria.daily_cap
        },
        cap_after: criteria.daily_cap,
    }))
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures::{add, panel};
    use crate::snapshot::build_snapshot;
    use crate::snapshot::support_tests::{
        SAMPLE_CRITERIA, profile_with, sample_criteria, seen_at, use_sample_criteria,
    };
    use jobbot_testkit::profile::with_line;
    use jobbot_testkit::{NOW, make_job_with};

    #[test]
    fn the_funnel_counts_each_application_once_per_stage_it_ever_reached() {
        let (panel, _dir) = panel();
        let sent = [State::Prepared, State::Submitted];
        add(&panel, "1", 80.0, &sent);
        add(
            &panel,
            "2",
            80.0,
            &[sent.as_slice(), &[State::Replied]].concat(),
        );
        // Went on to a test and an interview: it also counts as answered and nowhere twice.
        add(
            &panel,
            "3",
            80.0,
            &[
                sent.as_slice(),
                &[State::Replied, State::TechnicalTest, State::Interview],
            ]
            .concat(),
        );
        add(
            &panel,
            "4",
            80.0,
            &[
                sent.as_slice(),
                &[State::Replied, State::Interview, State::Offer],
            ]
            .concat(),
        );
        // A rejection is an answer too.
        add(
            &panel,
            "5",
            80.0,
            &[sent.as_slice(), &[State::Rejected]].concat(),
        );
        // Silence is not an answer, and an application never sent is not in the funnel.
        add(
            &panel,
            "6",
            80.0,
            &[sent.as_slice(), &[State::Ghosted]].concat(),
        );
        add(&panel, "7", 80.0, &[State::NeedsGonza]);

        let funnel = build_snapshot(&panel).unwrap().funnel.unwrap();

        assert_eq!(
            (
                funnel.sent,
                funnel.answered,
                funnel.tests,
                funnel.interviews,
                funnel.offers
            ),
            (6, 4, 1, 2, 1)
        );
        // Every fixture posting comes from the one source: its line is the whole funnel plus the
        // unsent one in its base.
        assert_eq!(
            funnel.by_source.get("test"),
            Some(&SourceFunnel {
                n: 7,
                sent: 6,
                answered: 4,
                tests: 1,
                interviews: 2,
                offers: 1,
            })
        );
    }

    #[test]
    fn the_funnel_is_split_by_source_with_a_base_for_each() {
        let (panel, _dir) = panel();
        let walk = |source: &str, id: &str, states: &[State]| {
            let job = make_job_with(|j| {
                j.source = source.into();
                j.external_id = id.into();
            });
            panel
                .tracker
                .add_job(&job, 80.0, &BTreeMap::new(), *NOW)
                .unwrap();
            panel.tracker.create_application(&job.key(), *NOW).unwrap();
            for state in states {
                panel
                    .tracker
                    .transition(&job.key(), *state, None, *NOW)
                    .unwrap();
            }
        };
        walk(
            "ashby",
            "1",
            &[State::Prepared, State::Submitted, State::Rejected],
        );
        walk("ashby", "2", &[State::NeedsGonza]);
        walk(
            "lever",
            "3",
            &[
                State::Prepared,
                State::Submitted,
                State::Replied,
                State::Interview,
            ],
        );

        let json = serde_json::to_value(build_snapshot(&panel).unwrap().funnel.unwrap()).unwrap();

        assert_eq!(json["sent"], 2);
        assert_eq!(
            json["by_source"]["ashby"],
            serde_json::json!({ "n": 2, "sent": 1, "answered": 1, "tests": 0, "interviews": 0, "offers": 0 })
        );
        assert_eq!(
            json["by_source"]["lever"],
            serde_json::json!({ "n": 1, "sent": 1, "answered": 1, "tests": 0, "interviews": 1, "offers": 0 })
        );
        assert_eq!(json["by_source"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn metrics_count_each_application_once_and_split_week_from_all_time() {
        let (panel, _dir) = panel();
        let t = &panel.tracker;
        add(
            &panel,
            "a",
            80.0,
            &[
                State::Prepared,
                State::Submitted,
                State::Interview,
                State::Interview,
                State::Offer,
            ],
        );
        add(
            &panel,
            "b",
            80.0,
            &[State::Prepared, State::Submitted, State::Replied],
        );
        add(
            &panel,
            "c",
            80.0,
            &[State::Prepared, State::Submitted, State::TechnicalTest],
        );
        // An application that went to interview ten days ago: it counts for all time, not for the week.
        let old = *NOW - TimeDelta::days(10);
        let d = add(&panel, "d", 80.0, &[]);
        for state in [State::Prepared, State::Submitted, State::Interview] {
            t.transition(&d, state, None, old).unwrap();
        }
        let m = build_snapshot(&panel).unwrap().metrics.unwrap();
        assert_eq!(m.sent_week, 3);
        assert_eq!(m.responses_week, 3);
        assert_eq!(m.response_rate, Some(1.0));
        assert_eq!(m.tests_week, 1);
        assert_eq!(
            m.interviews_week, 1,
            "a's second round does not count twice"
        );
        assert_eq!(m.interviews_total, 2);
        assert_eq!(m.offers_total, 1);
        assert_eq!(m.interview_to_offer, Some(0.5));
    }

    #[test]
    fn response_rate_only_counts_answers_to_applications_sent_in_the_window() {
        let (panel, _dir) = panel();
        let t = &panel.tracker;
        for i in 0..10 {
            add(
                &panel,
                &format!("new{i}"),
                80.0,
                &[State::Prepared, State::Submitted],
            );
        }
        // Three sent twenty days ago that are rejected now: answers this week, but not to this
        // week's sends.
        let old = *NOW - TimeDelta::days(20);
        for i in 0..3 {
            let key = add(&panel, &format!("old{i}"), 80.0, &[]);
            for state in [State::Prepared, State::Submitted] {
                t.transition(&key, state, None, old).unwrap();
            }
            t.transition(&key, State::Rejected, None, *NOW).unwrap();
        }
        let m = build_snapshot(&panel).unwrap().metrics.unwrap();
        assert_eq!(m.sent_week, 10);
        assert_eq!(m.responses_week, 3);
        assert_eq!(m.response_rate, Some(0.0));
    }

    /// `SAMPLE_CRITERIA` with `first_n_reviewed` changed.
    fn criteria_with_first_n(first_n: u32) -> String {
        with_line(
            SAMPLE_CRITERIA,
            "first_n_reviewed",
            &format!("first_n_reviewed = {first_n}"),
        )
    }

    #[test]
    fn the_pipeline_counts_the_week_by_the_furthest_layer_each_posting_reached() {
        let (panel, dir) = panel();
        let t = &panel.tracker;
        let file = |name: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"x").unwrap();
            path
        };
        let gone = dir.path().join("gone.png");
        let fresh = *NOW - TimeDelta::days(1);

        // No application: only seen.
        seen_at(&panel, "below", 30.0, fresh, None);
        // An application and nothing else yet, and one that was withdrawn: both are above.
        seen_at(&panel, "plain", 65.0, fresh, Some(&[]));
        seen_at(&panel, "withdrawn", 65.0, fresh, Some(&[State::Withdrawn]));
        // Paths that point at files that are gone prove nothing.
        let lost = seen_at(&panel, "lost", 65.0, fresh, Some(&[State::Prepared]));
        t.set_cv_path(&lost, &gone).unwrap();
        t.set_screenshot(&lost, &gone).unwrap();
        // A CV on disk, and nothing more.
        let cv = seen_at(&panel, "cv", 65.0, fresh, Some(&[State::Prepared]));
        t.set_cv_path(&cv, &file("cv.pdf")).unwrap();
        // Prepared with its screenshot: complete, and with a CV by the same token.
        let done = seen_at(&panel, "done", 65.0, fresh, Some(&[State::Prepared]));
        t.set_screenshot(&done, &file("shot.png")).unwrap();
        // The screenshot of a form that then needed Gonza is not a finished form; its CV counts.
        let needy = seen_at(&panel, "needy", 65.0, fresh, Some(&[State::NeedsGonza]));
        t.set_screenshot(&needy, &file("needy.png")).unwrap();
        t.set_cv_path(&needy, &file("needy.pdf")).unwrap();
        // Sent, with nothing left on disk: it counts in every layer.
        seen_at(
            &panel,
            "sent",
            65.0,
            fresh,
            Some(&[State::Prepared, State::Submitted]),
        );
        // First seen exactly seven days ago counts; a second earlier does not, however far it got.
        seen_at(&panel, "edge", 65.0, *NOW - TimeDelta::days(7), None);
        seen_at(
            &panel,
            "old",
            90.0,
            *NOW - TimeDelta::days(7) - TimeDelta::seconds(1),
            Some(&[State::Prepared, State::Submitted]),
        );

        let p = build_snapshot(&panel).unwrap().pipeline;

        assert_eq!(p.days, 7);
        assert_eq!(
            (p.seen, p.above, p.with_cv, p.complete, p.sent),
            (9, 7, 4, 2, 1)
        );
        assert!(p.sent <= p.complete && p.complete <= p.with_cv);
        assert!(p.with_cv <= p.above && p.above <= p.seen);
        assert_eq!(
            p.score_counts.iter().map(|&n| i64::from(n)).sum::<i64>(),
            p.seen
        );
    }

    #[test]
    fn an_approved_form_with_its_screenshot_is_still_a_complete_one_in_the_pipeline() {
        // The approval queue stops listing it, the pipeline keeps counting the form as finished.
        let (panel, dir) = panel();
        let fresh = *NOW - TimeDelta::days(1);
        let key = seen_at(&panel, "done", 65.0, fresh, Some(&[State::Prepared]));
        let shot = dir.path().join("shot.png");
        std::fs::write(&shot, b"x").unwrap();
        panel.tracker.set_screenshot(&key, &shot).unwrap();
        panel.tracker.approve(&key).unwrap();

        let snap = build_snapshot(&panel).unwrap();

        assert_eq!(snap.awaiting_approval.total, 0);
        assert_eq!(snap.pipeline.complete, 1);
    }

    #[test]
    fn score_counts_round_each_score_and_clamp_it_to_0_to_100() {
        let (panel, _dir) = panel();
        let fresh = *NOW - TimeDelta::hours(1);
        for (n, score) in [0.0, 0.4, -5.0, 0.5, 59.5, 60.2, 100.0, 100.4, 250.0]
            .into_iter()
            .enumerate()
        {
            seen_at(&panel, &n.to_string(), score, fresh, None);
        }
        // A posting from before the week is not counted.
        seen_at(&panel, "old", 77.0, *NOW - TimeDelta::days(8), None);

        let counts = build_snapshot(&panel).unwrap().pipeline.score_counts;

        assert_eq!(counts.len(), 101);
        let mut expected = vec![0u32; 101];
        expected[0] = 3;
        expected[1] = 1;
        expected[60] = 2;
        expected[100] = 3;
        assert_eq!(counts, expected);
    }

    #[test]
    fn the_threshold_comes_from_criteria_toml_and_is_none_when_it_cannot_be_read() {
        let (mut panel, dir) = panel();
        seen_at(&panel, "1", 70.0, *NOW, Some(&[]));
        use_sample_criteria(&mut panel, dir.path());
        let snap = build_snapshot(&panel).unwrap();
        assert_eq!(
            snap.pipeline.threshold,
            Some(sample_criteria().apply_threshold)
        );

        // No criteria.toml at all, and one that does not parse: same answer, the counts stay.
        for criteria in [None, Some("floor_monthly_usd = ")] {
            panel.paths.profile_dir = profile_with(dir.path(), criteria, None);
            let snap = build_snapshot(&panel).unwrap();
            assert_eq!(snap.pipeline.threshold, None);
            assert!(snap.trust.is_none());
            assert_eq!((snap.pipeline.seen, snap.pipeline.above), (1, 1));
        }
    }

    #[test]
    fn the_panel_rereads_criteria_toml_on_every_call_and_gives_none_for_a_missing_or_broken_file() {
        let (mut panel, dir) = panel();
        use_sample_criteria(&mut panel, dir.path());
        assert_eq!(
            panel.criteria().map(|c| c.apply_threshold),
            Some(sample_criteria().apply_threshold)
        );
        for criteria in [None, Some("floor_monthly_usd = ")] {
            panel.paths.profile_dir = profile_with(dir.path(), criteria, None);
            assert!(panel.criteria().is_none());
        }
    }

    #[test]
    fn trust_counts_the_reviewed_sends_and_the_cap_in_force() {
        let (mut panel, dir) = panel();
        let criteria = sample_criteria();
        assert_ne!(
            criteria.daily_cap, criteria.daily_cap_until_reviewed,
            "the test needs two different caps to tell them apart"
        );
        panel.paths.profile_dir = profile_with(dir.path(), Some(&criteria_with_first_n(2)), None);
        let send = |id: &str, approve: bool| {
            let key = add(&panel, id, 80.0, &[State::Prepared]);
            if approve {
                panel.tracker.approve(&key).unwrap();
            }
            panel
                .tracker
                .transition(&key, State::Submitted, None, *NOW)
                .unwrap();
        };

        let trust = build_snapshot(&panel).unwrap().trust.unwrap();
        assert_eq!(
            trust,
            Trust {
                reviewed: 0,
                first_n: 2,
                review_phase: true,
                sent_24h: 0,
                cap: criteria.daily_cap_until_reviewed,
                cap_after: criteria.daily_cap,
            }
        );

        // One approved send, and one nobody reviewed: only the first counts as reviewed.
        send("a", true);
        send("b", false);
        let trust = build_snapshot(&panel).unwrap().trust.unwrap();
        assert_eq!((trust.reviewed, trust.review_phase), (1, true));
        assert_eq!(trust.sent_24h, 2);
        assert_eq!(trust.cap, criteria.daily_cap_until_reviewed);

        // The second approved send ends the review phase: the larger cap is in force now.
        send("c", true);
        let trust = build_snapshot(&panel).unwrap().trust.unwrap();
        assert_eq!(
            (trust.reviewed, trust.first_n, trust.review_phase),
            (2, 2, false)
        );
        assert_eq!(trust.sent_24h, 3);
        assert_eq!(trust.cap, criteria.daily_cap);
        assert_eq!(trust.cap_after, criteria.daily_cap);
    }

    #[test]
    fn sent_24h_counts_what_the_gate_counts_as_a_click() {
        let (mut panel, dir) = panel();
        use_sample_criteria(&mut panel, dir.path());
        let hour = TimeDelta::hours(1);
        let send = |id: &str, at: DateTime<Utc>| {
            let key = add(&panel, id, 80.0, &[State::Prepared]);
            panel
                .tracker
                .transition(&key, State::Submitted, None, at)
                .unwrap();
        };
        send("recent", *NOW - hour);
        send("edge", *NOW - hour * 24);
        send("old", *NOW - hour * 25);
        // A click the page did not confirm may have gone out: it counts. A prepared form does not.
        let clicked = add(&panel, "clicked", 80.0, &[State::Prepared]);
        panel
            .tracker
            .record_attempt(&clicked, *NOW - hour * 2)
            .unwrap();
        add(&panel, "idle", 80.0, &[State::Prepared]);

        let trust = build_snapshot(&panel).unwrap().trust.unwrap();

        assert_eq!(trust.sent_24h, 3);
    }
}
