//! The data the page draws, as plain serialisable structs. Built fresh from the tracker, the prep
//! store and a few small files on every request, so there is nothing to synchronise: building it
//! twice gives the same data, and whatever Gonza marks is in SQLite when the request returns.
//! Text from job boards (titles, descriptions, notes) is kept exactly as stored; the page draws it
//! with `textContent`.
//!
//! This file holds the whole snapshot and the detail of one application (`build_snapshot`,
//! `build_job_detail`) and the few helpers that several sections share. Each section lives in
//! a child:
//!
//! - `cards`: the cards, the board columns and the listings of the queues, and why a waiting card
//!   waits.
//! - `controls`: the state of the engine and of the Claude account.
//! - `metrics`: the weekly metrics, the funnel, the pipeline and the gate's trust numbers.
//! - `activity`: the half-hour strip of the last 24 hours, and what Claude cost in them.
//! - `today`: the open tasks and the recent emails of "Tu lista de hoy".
//! - `learn`: the study section for interviews.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;

use super::Panel;
use jobbot_app::inbox::LIVE_STATES;
use jobbot_app::learn::dossier::dossier_path;
use jobbot_app::ports::store::ApplicationRow;
use jobbot_domain::state::State;

mod activity;
mod cards;
mod controls;
mod learn;
mod metrics;
mod today;

pub use activity::{Activity, ClaudeSpend, Move, Slot, StageSpend};
use activity::{activity, claude_spend};
use cards::dossiers_dir;
pub use cards::{COLUMN_CAP, Card, Column, Listing, QUEUE_CAP, UndoCard, offer_label};
use cards::{awaits_approval, label, listing};
pub(super) use cards::{card, card_context};
use controls::controls;
pub use controls::{ClaudeAccount, Controls, ScheduledTask};
pub use learn::{GapView, Learn, WeakTopic};
use learn::{gap_view, learn};
pub use metrics::{Funnel, FunnelGroup, Metrics, Pipeline, Trust};
use metrics::{funnel, metrics, pipeline, trust};
use today::today;
pub use today::{Today, TodayEmail, TodayInbox, TodayTask};

const DESCRIPTION_CAP: usize = 8_000;
const DOSSIER_CAP: usize = 30_000;

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub generated_at: String,
    pub controls: Controls,
    /// The sections that are not on the way to applying (`metrics`, `funnel`, `activity`, `claude`,
    /// `learn`) are `null` when they could not be built: the page reads each of them as optional,
    /// and a failure in one must not take the queues and their buttons with it. Every other
    /// section is built with `?` and a failure in it fails the whole snapshot: `controls`,
    /// `pipeline`, `today`, the queues and the board, and also `trust`. `trust` is an `Option`
    /// for another reason: it is `null` when `criteria.toml` cannot be read, not when building it
    /// fails.
    pub metrics: Option<Metrics>,
    pub funnel: Option<Funnel>,
    pub pipeline: Pipeline,
    pub trust: Option<Trust>,
    pub activity: Option<Activity>,
    pub claude: Option<ClaudeSpend>,
    pub today: Today,
    /// Minus what is snoozed (see `snoozed`).
    pub needs_gonza: Listing,
    pub awaiting_approval: Listing,
    /// Every application Gonza put off and whose time has not come, soonest back first. They stay
    /// in `board`; they leave `needs_gonza`, `awaiting_approval` and the tasks of `today`.
    pub snoozed: Listing,
    pub board: Vec<Column>,
    pub learn: Option<Learn>,
}

#[derive(Debug, Serialize)]
pub struct EventView {
    pub at: String,
    pub from: Option<&'static str>,
    pub to: &'static str,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobDetail {
    pub card: Card,
    pub location: String,
    pub remote: bool,
    pub apply_url: String,
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    pub description: String,
    pub score_detail: BTreeMap<String, f64>,
    pub events: Vec<EventView>,
    pub dossier: Option<String>,
    pub gaps: Vec<GapView>,
    /// When the posting was first stored.
    pub first_seen: Option<String>,
    /// The date the posting says it was published, when it says one.
    pub posted_at: Option<String>,
}

fn iso(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max_chars).collect::<String>())
    }
}

/// How many applications `counts` (state name to count) holds in `state`.
fn count_of(counts: &BTreeMap<String, i64>, state: State) -> usize {
    counts
        .get(state.as_str())
        .map_or(0, |n| usize::try_from(*n).unwrap_or(0))
}

/// A snapshot section that is allowed to be missing: on failure it is `None` (`null` on the page)
/// and the reason goes to the log, so the rest of the snapshot still comes out.
fn optional<T>(section: &str, built: Result<T>) -> Option<T> {
    match built {
        Ok(value) => Some(value),
        Err(err) => {
            tracing::warn!(section, error = %format!("{err:#}"), "snapshot section unavailable, sent as null");
            None
        }
    }
}

pub fn build_snapshot(panel: &Panel) -> Result<Snapshot> {
    let now = panel.now();
    let criteria = panel.criteria();
    let ctx = card_context(panel, now, criteria.as_ref())?;
    let counts = panel.tracker.counts()?;
    let mut board = Vec::new();
    // Gonza's two queues are made from the rows their columns read.
    let mut needs = Vec::new();
    let mut prepared = Vec::new();
    for state in State::ALL {
        // The Procesos tab draws one row per application in progress, so those columns are never
        // cut, and the two queues need every row of theirs. Any other column reads only the cards
        // it shows and takes its total from the counts.
        let live = LIVE_STATES.contains(&state);
        let cap = if live { usize::MAX } else { COLUMN_CAP };
        let shown = if live || matches!(state, State::NeedsGonza | State::Prepared) {
            let rows = panel.tracker.list_by_state(state)?;
            let shown = listing(&rows, cap, &ctx);
            match state {
                State::NeedsGonza => needs = rows,
                State::Prepared => prepared = rows,
                _ => {}
            }
            shown
        } else {
            let rows = panel.tracker.first_by_state(state, cap)?;
            Listing {
                total: count_of(&counts, state),
                cards: rows.iter().map(|r| card(r, &ctx)).collect(),
            }
        };
        board.push(Column {
            state: state.as_str(),
            label: label(state),
            total: shown.total,
            cards: shown.cards,
        });
    }
    // What Gonza put off, from every column: the board keeps it, the queues and today's tasks skip it.
    let mut snoozed = panel.tracker.still_snoozed(now)?;
    snoozed.sort_by_key(|r| (r.snoozed_until, r.job_key.clone()));
    let snoozed_keys: BTreeSet<String> = snoozed.iter().map(|r| r.job_key.clone()).collect();
    let needs: Vec<ApplicationRow> = needs
        .into_iter()
        .filter(|r| !snoozed_keys.contains(&r.job_key))
        .collect();
    let awaiting: Vec<ApplicationRow> = prepared
        .into_iter()
        .filter(awaits_approval)
        .filter(|r| !snoozed_keys.contains(&r.job_key))
        .collect();
    Ok(Snapshot {
        generated_at: iso(now),
        controls: controls(panel, now)?,
        metrics: optional("metrics", metrics(panel, now)),
        funnel: optional("funnel", funnel(panel)),
        pipeline: pipeline(panel, now, criteria.as_ref())?,
        trust: trust(panel, now, criteria.as_ref())?,
        activity: optional("activity", activity(panel, now)),
        claude: optional("claude", claude_spend(panel, now)),
        today: today(panel, now, &snoozed_keys)?,
        needs_gonza: listing(&needs, QUEUE_CAP, &ctx),
        awaiting_approval: listing(&awaiting, QUEUE_CAP, &ctx),
        snoozed: listing(&snoozed, QUEUE_CAP, &ctx),
        board,
        learn: optional("learn", learn(panel, now)),
    })
}

/// Everything about one application for the detail drawer; `None` when there is none.
pub fn build_job_detail(panel: &Panel, key: &str) -> Result<Option<JobDetail>> {
    let Some(row) = panel.tracker.get_application(key)? else {
        return Ok(None);
    };
    let Some(job) = panel.tracker.get_job(key)? else {
        return Ok(None);
    };
    let criteria = panel.criteria();
    let ctx = card_context(panel, panel.now(), criteria.as_ref())?;
    let events = panel
        .tracker
        .events_for(key)?
        .into_iter()
        .map(|e| EventView {
            at: iso(e.at),
            from: e.from_state.map(|s| s.as_str()),
            to: e.to_state.as_str(),
            note: e.note,
        })
        .collect();
    // A dossier that does not exist is the normal case; one that exists and cannot be read
    // (permissions, invalid UTF-8) still reaches the page as "none", but leaves a trace, because
    // its card says `has_dossier`.
    let dossier = match std::fs::read_to_string(dossier_path(&dossiers_dir(panel), key)) {
        Ok(text) => Some(truncate(&text, DOSSIER_CAP)),
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(job = key, error = %err, "dossier exists but could not be read, sent as none");
            }
            None
        }
    };
    let gaps = panel
        .store
        .gap_rows()?
        .into_iter()
        .filter(|gap| gap.source == key)
        .map(|gap| gap_view(panel, gap))
        .collect();
    Ok(Some(JobDetail {
        card: card(&row, &ctx),
        location: job.location,
        remote: job.remote,
        apply_url: job.apply_url,
        salary_min: job.salary_min_monthly_usd,
        salary_max: job.salary_max_monthly_usd,
        description: truncate(&job.description, DESCRIPTION_CAP),
        score_detail: panel.tracker.score_detail(key)?,
        events,
        dossier,
        gaps,
        first_seen: panel.tracker.job_first_seen(key)?.map(iso),
        posted_at: job.posted_at.map(iso),
    }))
}

#[cfg(test)]
mod board_tests;

#[cfg(test)]
mod contract_tests;

#[cfg(test)]
mod date_text_tests;

#[cfg(test)]
mod support_tests;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeDelta;

    use super::contract_tests::missing_contract_paths;
    use super::support_tests::{column, email, sample_criteria, task, use_sample_criteria};
    use super::*;
    use crate::fixtures::{add, panel};
    use jobbot_app::ports::store::{EmailKind, GapRow, TopicProgress};
    use jobbot_domain::prep::{Question, load_questions};
    use jobbot_testkit::{NOW, Tracker, make_job_with, paths_with_empty_profile};

    #[test]
    fn an_empty_database_gives_an_empty_but_complete_snapshot() {
        let (mut panel, dir) = panel();
        use_sample_criteria(&mut panel, dir.path());
        let snap = build_snapshot(&panel).unwrap();
        assert_eq!(snap.board.len(), 12);
        assert!(
            snap.board
                .iter()
                .all(|c| c.total == 0 && c.cards.is_empty())
        );
        assert_eq!(snap.needs_gonza.total, 0);
        assert_eq!(snap.awaiting_approval.total, 0);
        assert_eq!(
            snap.metrics,
            Some(Metrics {
                sent_week: 0,
                responses_week: 0,
                response_rate: None,
                tests_week: 0,
                interviews_week: 0,
                interviews_total: 0,
                offers_total: 0,
                interview_to_offer: None
            })
        );
        assert_eq!(
            snap.funnel,
            Some(Funnel {
                sent: 0,
                answered: 0,
                tests: 0,
                interviews: 0,
                offers: 0,
                by_source: BTreeMap::new(),
                by_origin: BTreeMap::new(),
                by_region: BTreeMap::new(),
            })
        );
        assert_eq!(snap.snoozed.total, 0);
        assert_eq!(snap.controls.engine, "never");
        assert!(snap.controls.last_run_at.is_none() && snap.controls.claude_account.is_none());
        let bank = load_questions().unwrap().len();
        let learn = snap.learn.as_ref().unwrap();
        assert_eq!((learn.total, learn.seen, learn.due), (bank, 0, bank));
        assert!(learn.gaps.is_empty() && learn.weak_topics.is_empty());
        assert!(!snap.controls.stopped);
        assert_eq!(snap.claude.as_ref().unwrap().spent_24h, 0.0);
        assert!(snap.claude.as_ref().unwrap().by_stage.is_empty());
        assert_eq!((snap.today.tasks_total, snap.today.emails_total), (0, 0));
        assert!(snap.today.tasks.is_empty() && snap.today.emails.is_empty());
        let criteria = sample_criteria();
        let pipeline = &snap.pipeline;
        assert_eq!(
            (pipeline.days, pipeline.threshold),
            (7, Some(criteria.apply_threshold))
        );
        assert_eq!(
            (
                pipeline.seen,
                pipeline.above,
                pipeline.with_cv,
                pipeline.complete,
                pipeline.sent
            ),
            (0, 0, 0, 0, 0)
        );
        assert_eq!(pipeline.score_counts, vec![0; 101]);
        assert_eq!(
            snap.trust,
            Some(Trust {
                reviewed: 0,
                first_n: criteria.first_n_reviewed,
                review_phase: true,
                sent_24h: 0,
                cap: criteria.daily_cap_until_reviewed,
                cap_after: criteria.daily_cap,
            })
        );
        let activity = snap.activity.as_ref().unwrap();
        assert_eq!(activity.slots.len(), 48);
        assert!(activity.slots.iter().all(|s| s.seen == 0));
        assert!(activity.moves.is_empty());
        assert_eq!(activity.moves_total, 0);
        assert!(activity.moves_by_state.is_empty());
        let json = serde_json::to_value(&snap.activity).unwrap();
        assert_eq!(
            json["moves_by_state"],
            serde_json::json!({}),
            "an object even when empty, so the page can always index it"
        );
    }

    /// A study store that fails every call, as `prep.db` does when its file is damaged.
    struct BrokenStudy;

    fn broken() -> jobbot_app::ports::store::StoreError {
        jobbot_app::ports::store::StoreError::Backend("prep.db is damaged".into())
    }

    impl jobbot_app::ports::store::LearnRepo for BrokenStudy {
        fn add_cards(
            &self,
            _: &[Question],
            _: &str,
            _: DateTime<Utc>,
        ) -> jobbot_app::ports::store::StoreResult<usize> {
            Err(broken())
        }
        fn all_questions(&self) -> jobbot_app::ports::store::StoreResult<Vec<Question>> {
            Err(broken())
        }
        fn next_question<'a>(
            &self,
            _: &'a [Question],
            _: DateTime<Utc>,
            _: Option<&str>,
        ) -> jobbot_app::ports::store::StoreResult<Option<&'a Question>> {
            Err(broken())
        }
        fn grade(
            &self,
            _: &Question,
            _: u8,
            _: DateTime<Utc>,
        ) -> jobbot_app::ports::store::StoreResult<()> {
            Err(broken())
        }
        fn weak_topics(
            &self,
            _: usize,
            _: usize,
        ) -> jobbot_app::ports::store::StoreResult<Vec<(String, f64)>> {
            Err(broken())
        }
        fn stats(
            &self,
            _: &[Question],
            _: DateTime<Utc>,
        ) -> jobbot_app::ports::store::StoreResult<jobbot_app::ports::store::PrepStats> {
            Err(broken())
        }
        fn topic_progress(
            &self,
            _: &[Question],
            _: DateTime<Utc>,
        ) -> jobbot_app::ports::store::StoreResult<Vec<TopicProgress>> {
            Err(broken())
        }
        fn gap_rows(&self) -> jobbot_app::ports::store::StoreResult<Vec<GapRow>> {
            Err(broken())
        }
    }

    #[test]
    fn a_study_store_that_fails_costs_the_snapshot_its_learn_section_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_with_empty_profile(dir.path());
        let panel = Panel::new(
            paths,
            Tracker::in_memory().unwrap(),
            BrokenStudy,
            Box::new(|| *NOW),
        );
        let key = add(&panel, "1", 80.0, &[State::NeedsGonza]);
        add(&panel, "2", 80.0, &[State::Prepared]);
        let snap = build_snapshot(&panel).expect("the snapshot still comes out");
        assert!(snap.learn.is_none());
        assert_eq!(snap.needs_gonza.total, 1);
        assert_eq!(snap.needs_gonza.cards[0].key, key);
        assert_eq!(column(&snap, "prepared").total, 1);
        let json = serde_json::to_value(&snap).unwrap();
        assert!(json["learn"].is_null());
        assert!(json["board"].is_array() && json["controls"].is_object());
        // Still the snapshot the page was promised: the key is there, null, and the rest is whole.
        assert_eq!(missing_contract_paths(&json, false), Vec::<String>::new());
        // A section that is not there at all is another matter.
        let mut dropped = json.clone();
        dropped.as_object_mut().unwrap().remove("learn");
        assert_eq!(missing_contract_paths(&dropped, false), vec!["learn"]);
    }

    #[test]
    fn building_the_snapshot_twice_gives_the_same_data() {
        let (panel, _dir) = panel();
        add(&panel, "1", 80.0, &[State::NeedsGonza]);
        add(
            &panel,
            "2",
            70.0,
            &[State::Prepared, State::Submitted, State::Replied],
        );
        panel
            .store
            .add_cards(
                &[Question {
                    id: "gap-x-1".into(),
                    topic: "kubernetes".into(),
                    question: "q".into(),
                    answer: "a".into(),
                }],
                "test:2",
                *NOW,
            )
            .unwrap();
        task(&panel, "Responder", Some(*NOW + TimeDelta::hours(2)), None);
        email(&panel, "m1", EmailKind::Recruiter, *NOW, "Hola");
        let first = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();
        let second = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();
        assert_eq!(
            first, second,
            "reading the panel changes nothing and duplicates nothing"
        );
    }

    #[test]
    fn job_detail_has_the_history_the_dossier_and_the_gaps_of_that_job() {
        let (panel, dir) = panel();
        let key = add(
            &panel,
            "1",
            80.0,
            &[State::Prepared, State::Submitted, State::Replied],
        );
        let dossiers = dir.path().join("data").join("dossiers");
        std::fs::create_dir_all(&dossiers).unwrap();
        std::fs::write(
            jobbot_app::learn::dossier::dossier_path(&dossiers, &key),
            "# Dossier\nhola",
        )
        .unwrap();
        panel
            .store
            .add_cards(
                &[Question {
                    id: "gap-x-1".into(),
                    topic: "kubernetes".into(),
                    question: "q".into(),
                    answer: "a".into(),
                }],
                &key,
                *NOW,
            )
            .unwrap();
        let detail = build_job_detail(&panel, &key).unwrap().unwrap();
        assert_eq!(detail.card.state, "replied");
        assert!(detail.card.has_dossier);
        let path: Vec<&str> = detail.events.iter().map(|e| e.to).collect();
        assert_eq!(path, vec!["scored", "prepared", "submitted", "replied"]);
        assert_eq!(detail.events[0].from, None);
        assert_eq!(detail.dossier.as_deref(), Some("# Dossier\nhola"));
        assert_eq!(detail.gaps.len(), 1);
        assert_eq!(detail.gaps[0].offer, "Co 1 — Role 1");
        assert!(build_job_detail(&panel, "nope:1").unwrap().is_none());
    }

    #[test]
    fn a_dossier_that_cannot_be_read_is_sent_as_none_while_its_card_still_has_one() {
        let (panel, dir) = panel();
        let key = add(&panel, "1", 80.0, &[State::Prepared]);
        let dossiers = dir.path().join("data").join("dossiers");
        std::fs::create_dir_all(&dossiers).unwrap();
        // Not valid UTF-8: the file is there, reading it as text fails.
        std::fs::write(
            jobbot_app::learn::dossier::dossier_path(&dossiers, &key),
            [0xff, 0xfe, 0xfd],
        )
        .unwrap();
        let detail = build_job_detail(&panel, &key).unwrap().unwrap();
        assert!(detail.card.has_dossier);
        assert_eq!(detail.dossier, None);
    }

    #[test]
    fn a_huge_description_is_cut_and_hostile_text_is_kept_as_plain_data() {
        let (panel, _dir) = panel();
        let hostile = "</script><img src=x onerror=alert(1)>";
        let job = make_job_with(|j| {
            j.title = hostile.into();
            j.description = "x".repeat(50_000);
        });
        panel
            .tracker
            .add_job(&job, 80.0, &BTreeMap::new(), *NOW)
            .unwrap();
        panel.tracker.create_application(&job.key(), *NOW).unwrap();
        let detail = build_job_detail(&panel, &job.key()).unwrap().unwrap();
        assert_eq!(
            detail.description.chars().count(),
            8_000 + 1,
            "cut at the cap the page counts on (the literal on purpose), plus the ellipsis"
        );
        assert_eq!(
            detail.card.title, hostile,
            "the data layer keeps text as it came; escaping is the renderer's job"
        );
    }

    #[test]
    fn job_detail_says_when_the_posting_was_first_seen_and_when_it_was_posted() {
        let (panel, _dir) = panel();
        let dated = make_job_with(|j| {
            j.external_id = "dated".into();
            j.posted_at = Some(*NOW - TimeDelta::days(3));
        });
        let undated = make_job_with(|j| j.external_id = "undated".into());
        for (job, seen) in [
            (&dated, *NOW - TimeDelta::days(1)),
            (&undated, *NOW - TimeDelta::hours(5)),
        ] {
            panel
                .tracker
                .add_job(job, 80.0, &BTreeMap::new(), seen)
                .unwrap();
            panel.tracker.create_application(&job.key(), seen).unwrap();
        }

        let dated_detail = build_job_detail(&panel, &dated.key()).unwrap().unwrap();
        let undated_detail = build_job_detail(&panel, &undated.key()).unwrap().unwrap();

        assert_eq!(
            dated_detail.first_seen.as_deref(),
            Some("2026-09-30T12:00:00Z")
        );
        assert_eq!(
            dated_detail.posted_at.as_deref(),
            Some("2026-09-28T12:00:00Z")
        );
        assert_eq!(
            undated_detail.first_seen.as_deref(),
            Some("2026-10-01T07:00:00Z")
        );
        assert_eq!(undated_detail.posted_at, None);
    }
}
