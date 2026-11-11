//! The `pipeline` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/pipeline.rs`).

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use jobbot_app::apply::cv::{CvError, Renderer};
use jobbot_app::apply::pipeline::*;
use jobbot_app::ports::{FillMode, FormFiller};
use jobbot_app::ports::{OpenChecker, OpenStatus};
use jobbot_domain::answers::AnswerBank;
use jobbot_domain::form::SubmitStatus;
use jobbot_domain::models::Job;
use jobbot_domain::rules::{Criteria, Exclusions};
use jobbot_domain::state::State;
use jobbot_domain::text::safe_key;
use jobbot_testkit::Tracker;
use jobbot_testkit::{NOW, make_job_with};

const MASTER: &str = "# JANE DOE
**Senior Full-Stack AI Engineer**

## SUMMARY
Builds agents.

## TECHNICAL SKILLS
**Languages:** TypeScript, Python

## PROFESSIONAL EXPERIENCE
**Role**
• Did things";
/// Test double for `Renderer`: records the markdown it is given and writes a fake PDF.
#[derive(Default)]
struct FakeRenderer {
    rendered: Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl Renderer for FakeRenderer {
    async fn render(&self, md_text: &str, out_path: &Path) -> anyhow::Result<PathBuf> {
        self.rendered.lock().unwrap().push(md_text.to_string());
        std::fs::write(out_path, b"%PDF-fake")?;
        Ok(out_path.to_path_buf())
    }
}
/// A temp folder plus an in-memory tracker.
struct Env {
    dir: tempfile::TempDir,
    tracker: Tracker,
}
impl Env {
    fn new() -> Env {
        Env {
            dir: tempfile::tempdir().unwrap(),
            tracker: Tracker::in_memory().unwrap(),
        }
    }

    fn cv_master(&self, text: &str) -> PathBuf {
        let path = self.dir.path().join("cv.md");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn out_dir(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn scored(&self, job: &Job, score: f64) {
        self.tracker
            .add_job(job, score, &BTreeMap::new(), *NOW)
            .unwrap();
        self.tracker.create_application(&job.key(), *NOW).unwrap();
    }

    fn state(&self, key: &str) -> State {
        self.tracker.get_application(key).unwrap().unwrap().state
    }
}
#[tokio::test]
async fn prepare_all_tailors_renders_and_moves_to_prepared() {
    let env = Env::new();
    let master = env.cv_master(MASTER);
    let job = make_job_with(|j| j.description = "We need python.".into());
    env.scored(&job, 70.0);
    let renderer = FakeRenderer::default();
    let keys = prepare_all(&env.tracker, &master, &env.out_dir(), &renderer, *NOW)
        .await
        .unwrap();
    assert_eq!(keys, vec![job.key()]);
    let row = env.tracker.get_application(&job.key()).unwrap().unwrap();
    assert_eq!(row.state, State::Prepared);
    assert!(Path::new(row.cv_path.as_deref().unwrap()).exists());
    let rendered = renderer.rendered.lock().unwrap();
    assert!(rendered[0].contains("**Languages:** Python, TypeScript"));
    assert!(rendered[0].contains("Relevant to this role: Python."));
}
const MASTER_WITH_CONTACT: &str = "# JANE DOE
me@example.com • +56 9 0000 0000 • Santiago, Chile • Remote

## SUMMARY
Builds agents. Seeking remote roles in applied AI.

## TECHNICAL SKILLS
**Languages:** TypeScript, Python
";
fn australia_and_latam() -> (Job, Job) {
    let sydney = make_job_with(|j| {
        j.external_id = "sydney".into();
        j.company = "Sydney Co".into();
        j.location = "Sydney, Australia".into();
        j.remote = false;
        j.description = "We need python.".into();
    });
    let latam = make_job_with(|j| {
        j.external_id = "latam".into();
        j.company = "Latam Co".into();
        j.location = "Remote - Latin America".into();
        j.remote = true;
        j.description = "We need python.".into();
    });
    (sydney, latam)
}
#[tokio::test]
async fn a_job_in_the_relocation_place_gets_the_relocated_cv_and_others_the_master() {
    let env = Env::new();
    let master = env.cv_master(MASTER_WITH_CONTACT);
    let (sydney, latam) = australia_and_latam();
    env.scored(&sydney, 80.0);
    env.scored(&latam, 70.0);
    let places = vec!["australia".to_string(), "sydney".to_string()];
    let relocation = Relocation {
        places: &places,
        to: Some("Australia"),
    };
    let renderer = FakeRenderer::default();

    let report = prepare_report_relocating(
        &env.tracker,
        &master,
        &env.out_dir(),
        &renderer,
        relocation,
        *NOW,
    )
    .await
    .unwrap();

    assert_eq!(report.prepared, vec![sydney.key(), latam.key()]);
    let rendered = renderer.rendered.lock().unwrap();
    assert!(
        rendered[0].contains("Santiago, Chile • Open to relocating to Australia"),
        "{}",
        rendered[0]
    );
    assert!(
        rendered[0].contains("Seeking roles in applied AI in Australia, on site or remote."),
        "{}",
        rendered[0]
    );
    assert!(
        rendered[1].contains("Santiago, Chile • Remote"),
        "{}",
        rendered[1]
    );
    assert!(rendered[1].contains("Seeking remote roles in applied AI."));
    // Without a place to move to, every job gets the master.
    let plain = Relocation {
        places: &places,
        to: None,
    };
    assert_eq!(
        plain.master_for(MASTER_WITH_CONTACT, &sydney),
        MASTER_WITH_CONTACT
    );
}
#[tokio::test]
async fn refresh_renders_waiting_cvs_again_keeps_the_old_ones_and_never_touches_sent_ones() {
    let env = Env::new();
    let master = env.cv_master(MASTER);
    let (sydney, latam) = australia_and_latam();
    let sent = make_job_with(|j| {
        j.external_id = "sent".into();
        j.company = "Sent Co".into();
    });
    for (job, score) in [(&sydney, 80.0), (&latam, 70.0), (&sent, 60.0)] {
        env.scored(job, score);
    }
    let renderer = FakeRenderer::default();
    prepare_all(&env.tracker, &master, &env.out_dir(), &renderer, *NOW)
        .await
        .unwrap();
    env.tracker
        .transition(&latam.key(), State::NeedsGonza, Some("listing_only"), *NOW)
        .unwrap();
    env.tracker
        .transition(&sent.key(), State::Submitted, None, *NOW)
        .unwrap();
    let sent_pdf = env
        .tracker
        .get_application(&sent.key())
        .unwrap()
        .unwrap()
        .cv_path
        .unwrap();
    std::fs::write(&sent_pdf, b"%PDF-what-was-sent").unwrap();
    // The master changes (a new line in the summary), then the CVs are refreshed.
    let master = env.cv_master(MASTER_WITH_CONTACT);
    let places = vec!["australia".to_string()];
    let renderer = FakeRenderer::default();

    let refreshed = refresh_cvs(
        &env.tracker,
        &master,
        &renderer,
        Relocation {
            places: &places,
            to: Some("Australia"),
        },
        *NOW,
    )
    .await
    .unwrap();

    assert!(refreshed.contains(&sydney.key()), "{refreshed:?}");
    assert!(refreshed.contains(&latam.key()), "{refreshed:?}");
    assert!(
        !refreshed.contains(&sent.key()),
        "a sent CV is the record of what went out"
    );
    assert_eq!(std::fs::read(&sent_pdf).unwrap(), b"%PDF-what-was-sent");
    let rendered = renderer.rendered.lock().unwrap();
    assert!(
        rendered
            .iter()
            .any(|md| md.contains("Open to relocating to Australia"))
    );
    assert!(rendered.iter().all(|md| md.contains("me@example.com")));
    // Each replaced PDF was kept first.
    let kept: Vec<_> = std::fs::read_dir(env.out_dir().join(REPLACED_CV_DIR))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(kept.len(), refreshed.len(), "{kept:?}");
    assert!(
        kept.iter()
            .all(|name| name.ends_with(".pdf") && name.contains('T'))
    );
    // No state moved.
    assert_eq!(env.state(&sydney.key()), State::Prepared);
    assert_eq!(env.state(&latam.key()), State::NeedsGonza);
}
#[tokio::test]
async fn placeholder_cv_leaves_state_untouched() {
    let env = Env::new();
    let master = env.cv_master(&format!("{MASTER}\n⟦GitHub URL⟧"));
    let job = make_job_with(|_| {});
    env.scored(&job, 70.0);
    let renderer = FakeRenderer::default();
    let err = prepare_all(&env.tracker, &master, &env.out_dir(), &renderer, *NOW)
        .await
        .unwrap_err();
    assert!(matches!(err, CvError::NotReady));
    assert_eq!(env.state(&job.key()), State::Scored);
    assert!(renderer.rendered.lock().unwrap().is_empty());
    assert!(!env.out_dir().exists());
}
#[tokio::test]
async fn only_scored_jobs_are_prepared_and_a_second_run_finds_nothing() {
    let env = Env::new();
    let master = env.cv_master(MASTER);
    let first = make_job_with(|j| j.external_id = "1".into());
    let second = make_job_with(|j| j.external_id = "2".into());
    env.scored(&first, 80.0);
    env.scored(&second, 70.0);
    env.tracker
        .transition(&second.key(), State::NeedsGonza, Some("falta email"), *NOW)
        .unwrap();
    let renderer = FakeRenderer::default();
    let keys = prepare_all(&env.tracker, &master, &env.out_dir(), &renderer, *NOW)
        .await
        .unwrap();
    assert_eq!(keys, vec![first.key()]);
    assert_eq!(env.state(&second.key()), State::NeedsGonza);
    let again = prepare_all(&env.tracker, &master, &env.out_dir(), &renderer, *NOW)
        .await
        .unwrap();
    assert!(again.is_empty());
    assert_eq!(renderer.rendered.lock().unwrap().len(), 1);
}
// ---- submit_all ----

use chrono::TimeDelta;
use jobbot_app::profile::{load_criteria, load_exclusions};
use jobbot_domain::form::{SubmitMode, SubmitResult};

/// Test double for `FormFiller`: records `(job key, mode)` for every call and answers with a
/// fixed reply, or with `DryRun`/`Submitted` according to the mode when none is set.
struct FakeFiller {
    calls: Mutex<Vec<(String, SubmitMode)>>,
    reply: Option<(SubmitStatus, Option<String>)>,
}
impl FakeFiller {
    fn new() -> FakeFiller {
        FakeFiller {
            calls: Mutex::new(Vec::new()),
            reply: None,
        }
    }

    fn replying(status: SubmitStatus, reason: &str) -> FakeFiller {
        FakeFiller {
            calls: Mutex::new(Vec::new()),
            reply: Some((status, Some(reason.to_string()))),
        }
    }

    fn calls(&self) -> Vec<(String, SubmitMode)> {
        self.calls.lock().unwrap().clone()
    }
}
#[async_trait::async_trait]
impl FormFiller for FakeFiller {
    async fn submit(
        &self,
        job: &Job,
        _cv_pdf: &Path,
        _bank: &AnswerBank,
        mode: FillMode,
        shots_dir: &Path,
    ) -> anyhow::Result<SubmitResult> {
        let mode = mode.submit_mode();
        self.calls.lock().unwrap().push((job.key(), mode));
        let screenshot = Some(shots_dir.join(format!("{}.png", safe_key(&job.key()))));
        let (status, reason) = match &self.reply {
            Some((status, reason)) => (*status, reason.clone()),
            None if mode == SubmitMode::DryRun => (SubmitStatus::DryRun, None),
            None => (SubmitStatus::Submitted, None),
        };
        Ok(SubmitResult {
            status,
            reason,
            screenshot,
        })
    }
}
fn criteria() -> Criteria {
    let path = jobbot_testkit::real_paths()
        .profile_dir
        .join("criteria.toml");
    load_criteria(&path).unwrap()
}
fn exclusions() -> Exclusions {
    let path = jobbot_testkit::real_paths()
        .profile_dir
        .join("exclusions.toml");
    load_exclusions(&path).unwrap()
}
/// Job number `n`: key `test:n`, its own company, so the 90-day company cooldown never applies.
fn numbered(n: u32) -> Job {
    make_job_with(|j| {
        j.external_id = n.to_string();
        j.company = format!("Co{n}");
        j.title = format!("Role {n}");
    })
}
impl Env {
    fn prepared(&self, job: &Job, score: f64) {
        self.scored(job, score);
        self.tracker
            .set_cv_path(&job.key(), Path::new("cv.pdf"))
            .unwrap();
        self.tracker
            .transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
    }

    fn stop_file(&self) -> PathBuf {
        self.dir.path().join("STOP")
    }

    async fn run(
        &self,
        filler: &FakeFiller,
        crit: &Criteria,
        mode: SubmitMode,
    ) -> Vec<SubmitOutcome> {
        self.run_with(filler, None, crit, mode).await.unwrap()
    }

    /// `run` with an inspector for the handoff forms, returning the error instead of panicking.
    async fn run_with(
        &self,
        filler: &FakeFiller,
        inspector: Option<&dyn FormInspector>,
        crit: &Criteria,
        mode: SubmitMode,
    ) -> anyhow::Result<Vec<SubmitOutcome>> {
        submit_all_while(
            &self.tracker,
            crit,
            &exclusions(),
            &AnswerBank::default(),
            filler,
            inspector,
            None,
            mode,
            self.dir.path(),
            &self.stop_file(),
            *NOW,
            &|| true,
        )
        .await
    }

    /// `run` with a checker that says whether each posting is still open.
    async fn run_checked(
        &self,
        filler: &FakeFiller,
        checker: &dyn OpenChecker,
        crit: &Criteria,
        mode: SubmitMode,
    ) -> Vec<SubmitOutcome> {
        submit_all_while(
            &self.tracker,
            crit,
            &exclusions(),
            &AnswerBank::default(),
            filler,
            None,
            Some(checker),
            mode,
            self.dir.path(),
            &self.stop_file(),
            *NOW,
            &|| true,
        )
        .await
        .unwrap()
    }

    fn handoff_sheet(&self, key: &str) -> String {
        let path = self
            .dir
            .path()
            .join("handoff")
            .join(format!("{}.md", safe_key(key)));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("no sheet at {}: {err}", path.display()))
    }
}
#[tokio::test]
async fn without_approvals_everything_is_a_dry_run() {
    let env = Env::new();
    for (n, score) in [(1, 90.0), (2, 80.0), (3, 70.0)] {
        env.prepared(&numbered(n), score);
    }
    let filler = FakeFiller::new();
    let outcomes = env.run(&filler, &criteria(), SubmitMode::Live).await;
    let statuses: Vec<&str> = outcomes.iter().map(|o| o.status.as_str()).collect();
    assert_eq!(statuses, vec!["dry_run"; 3]);
    assert!(
        filler
            .calls()
            .iter()
            .all(|(_, mode)| *mode == SubmitMode::DryRun)
    );
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    assert_eq!(row.state, State::Prepared);
    assert!(row.screenshot_path.is_some());
    // A second run does not repeat the dry runs.
    assert!(
        env.run(&filler, &criteria(), SubmitMode::Live)
            .await
            .is_empty()
    );
    assert_eq!(filler.calls().len(), 3);
}
#[tokio::test]
async fn approved_application_is_submitted_and_others_stay_in_review() {
    let env = Env::new();
    for (n, score) in [(1, 90.0), (2, 80.0)] {
        env.prepared(&numbered(n), score);
    }
    env.tracker.approve("test:1").unwrap();
    let filler = FakeFiller::new();
    env.run(&filler, &criteria(), SubmitMode::Live).await;
    assert_eq!(
        filler.calls(),
        vec![
            ("test:1".to_string(), SubmitMode::Live),
            ("test:2".to_string(), SubmitMode::DryRun)
        ]
    );
    assert_eq!(env.state("test:1"), State::Submitted);
    assert_eq!(env.state("test:2"), State::Prepared);
}
#[tokio::test]
async fn a_results_log_that_cannot_be_written_does_not_lose_the_report_of_what_was_sent() {
    let env = Env::new();
    for (n, score) in [(1, 90.0), (2, 80.0)] {
        env.prepared(&numbered(n), score);
        env.tracker.approve(&numbered(n).key()).unwrap();
    }
    // A folder where the log file should be: every append to it fails.
    std::fs::create_dir(results_path(env.dir.path())).unwrap();
    let filler = FakeFiller::new();
    let outcomes = env.run(&filler, &criteria(), SubmitMode::Live).await;
    let sent: Vec<(&str, &str)> = outcomes
        .iter()
        .map(|o| (o.job_key.as_str(), o.status.as_str()))
        .collect();
    assert_eq!(
        sent,
        [("test:1", "submitted"), ("test:2", "submitted")],
        "both sends are reported although their log lines could not be written"
    );
    assert_eq!(env.state("test:1"), State::Submitted);
    assert_eq!(env.state("test:2"), State::Submitted);
}
#[tokio::test]
async fn approved_job_is_submitted_after_its_review_dry_run() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::new();
    env.run(&filler, &criteria(), SubmitMode::Live).await;
    env.tracker.approve("test:1").unwrap();
    let outcomes = env.run(&filler, &criteria(), SubmitMode::Live).await;
    assert_eq!(
        filler.calls(),
        vec![
            ("test:1".to_string(), SubmitMode::DryRun),
            ("test:1".to_string(), SubmitMode::Live)
        ]
    );
    assert_eq!(outcomes[0].status, "submitted");
    assert_eq!(env.state("test:1"), State::Submitted);
}
#[tokio::test]
async fn after_first_n_approved_the_rest_go_automatically() {
    let env = Env::new();
    for (n, score) in [(1, 90.0), (2, 80.0), (3, 70.0)] {
        env.prepared(&numbered(n), score);
    }
    env.tracker.approve("test:1").unwrap();
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 1,
        ..criteria()
    };
    env.run(&filler, &crit, SubmitMode::Live).await;
    let modes: Vec<SubmitMode> = filler.calls().iter().map(|(_, mode)| *mode).collect();
    assert_eq!(modes, vec![SubmitMode::Live; 3]);
    for key in ["test:1", "test:2", "test:3"] {
        assert_eq!(env.state(key), State::Submitted, "{key}");
    }
}
#[tokio::test]
async fn dry_run_mode_never_submits_even_when_the_gate_allows() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::DryRun).await;
    assert_eq!(
        filler.calls(),
        vec![("test:1".to_string(), SubmitMode::DryRun)]
    );
    assert_eq!(outcomes[0].status, "dry_run");
    assert_eq!(env.state("test:1"), State::Prepared);
    assert!(env.run(&filler, &crit, SubmitMode::DryRun).await.is_empty());
    assert_eq!(filler.calls().len(), 1);
}
#[tokio::test]
async fn daily_cap_stops_the_run() {
    let env = Env::new();
    for (n, score) in [(1, 90.0), (2, 80.0)] {
        env.prepared(&numbered(n), score);
    }
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        daily_cap: 1,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert_eq!(
        filler.calls(),
        vec![("test:1".to_string(), SubmitMode::Live)]
    );
    assert_eq!(outcomes.len(), 1);
    assert_eq!(env.state("test:2"), State::Prepared);
}
#[tokio::test]
async fn after_the_daily_cap_a_listing_still_goes_to_gonza() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    env.prepared(&numbered(2), 80.0);
    let listing = make_job_with(|j| {
        j.external_id = "listing".into();
        j.company = "Listing Co".into();
        j.apply_url = "https://remotive.com/remote-jobs/software-dev/x-1".into();
    });
    env.prepared(&listing, 70.0);
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        daily_cap: 1,
        ..criteria()
    };

    env.run(&filler, &crit, SubmitMode::Live).await;

    assert_eq!(
        filler.calls().len(),
        1,
        "the cap still stops the second form"
    );
    assert_eq!(env.state("test:2"), State::Prepared);
    assert_eq!(env.state(&listing.key()), State::NeedsGonza);
}
#[tokio::test]
async fn stop_file_blocks_everything() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    std::fs::write(env.stop_file(), "x").unwrap();
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    assert!(env.run(&filler, &crit, SubmitMode::Live).await.is_empty());
    assert!(filler.calls().is_empty());
    assert_eq!(env.state("test:1"), State::Prepared);
}
#[tokio::test]
async fn manual_only_platform_goes_to_gonza() {
    let env = Env::new();
    let job = make_job_with(|j| j.apply_url = "https://www.linkedin.com/jobs/view/1".into());
    env.prepared(&job, 90.0);
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert!(filler.calls().is_empty());
    assert_eq!(outcomes[0].status, "blocked");
    assert_eq!(outcomes[0].reason.as_deref(), Some("manual_only_platform"));
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    assert_eq!(row.state, State::NeedsGonza);
    assert!(
        row.needs_reason
            .unwrap()
            .starts_with("manual_only_platform")
    );
}
#[tokio::test]
async fn excluded_company_is_withdrawn() {
    let env = Env::new();
    let job = make_job_with(|j| j.company = "Tidewater Systems".into());
    env.prepared(&job, 90.0);
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert!(filler.calls().is_empty());
    assert_eq!(outcomes[0].status, "blocked");
    assert!(
        outcomes[0]
            .reason
            .as_deref()
            .unwrap()
            .starts_with("excluded:")
    );
    assert_eq!(env.state("test:1"), State::Withdrawn);
}
#[tokio::test]
async fn company_cooldown_is_reported_and_the_application_stays_prepared() {
    let env = Env::new();
    let earlier = make_job_with(|j| j.external_id = "1".into());
    let later = make_job_with(|j| j.external_id = "2".into());
    env.prepared(&earlier, 90.0);
    env.tracker.approve(&earlier.key()).unwrap();
    env.tracker
        .transition(&earlier.key(), State::Submitted, None, *NOW)
        .unwrap();
    env.prepared(&later, 80.0);
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert!(filler.calls().is_empty());
    assert_eq!(
        outcomes,
        vec![SubmitOutcome {
            job_key: "test:2".to_string(),
            status: "blocked".to_string(),
            reason: Some("company_cooldown".to_string()),
        }]
    );
    assert_eq!(env.state("test:2"), State::Prepared);
}
#[tokio::test]
async fn filler_needs_gonza_is_recorded() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::replying(SubmitStatus::NeedsGonza, "faltan respuestas: Email");
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert_eq!(outcomes[0].status, "needs_gonza");
    assert_eq!(
        outcomes[0].reason.as_deref(),
        Some("faltan respuestas: Email")
    );
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    assert_eq!(row.state, State::NeedsGonza);
    assert_eq!(
        row.needs_reason.as_deref(),
        Some("faltan respuestas: Email")
    );
}
#[tokio::test]
async fn a_dry_run_that_lacks_answers_keeps_the_screenshot_of_the_partly_filled_form() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "faltan respuestas: Why are you interested in this role? *",
    );
    let outcomes = env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    assert_eq!(
        filler.calls(),
        vec![("test:1".to_string(), SubmitMode::DryRun)]
    );
    assert_eq!(outcomes[0].status, "needs_gonza");
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    assert_eq!(row.state, State::NeedsGonza);
    assert_eq!(
        row.needs_reason.as_deref(),
        Some("faltan respuestas: Why are you interested in this role? *")
    );
    assert!(
        row.screenshot_path.is_some(),
        "the screenshot shows what is done and what is left"
    );
    // It is only a dry run: nothing was clicked, so nothing counts toward the cap or the
    // company cooldown, and the application is not approved.
    assert!(!row.approved);
    let day_ago = *NOW - TimeDelta::hours(24);
    assert_eq!(env.tracker.clicked_since(day_ago).unwrap(), 0);
}
#[tokio::test]
async fn a_skipped_assist_form_does_not_lock_the_company() {
    let env = Env::new();
    let first = make_job_with(|j| j.external_id = "1".into());
    let second = make_job_with(|j| j.external_id = "2".into());
    env.prepared(&first, 90.0);
    env.prepared(&second, 80.0);
    // Gonza closed the tab of the first form instead of answering it.
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "assist_closed: cerraste la pestaña. Quedaba pendiente: faltan respuestas: Email *",
    );
    let outcomes = env.run(&filler, &criteria(), SubmitMode::Assist).await;
    // The run moves on to the second posting of the same company, and both wait for him.
    assert_eq!(
        filler.calls(),
        vec![
            ("test:1".to_string(), SubmitMode::Assist),
            ("test:2".to_string(), SubmitMode::Assist)
        ]
    );
    assert_eq!(outcomes.len(), 2);
    for key in ["test:1", "test:2"] {
        let row = env.tracker.get_application(key).unwrap().unwrap();
        assert_eq!(row.state, State::NeedsGonza, "{key}");
        assert!(!row.approved, "{key}");
    }
    let day_ago = *NOW - TimeDelta::hours(24);
    assert_eq!(env.tracker.clicked_since(day_ago).unwrap(), 0);
}
#[tokio::test]
async fn unconfirmed_submit_is_recorded_as_needs_gonza_and_never_retried() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "unconfirmed_submit: Phone number is invalid.",
    );
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert_eq!(outcomes[0].status, "needs_gonza");
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    assert_eq!(row.state, State::NeedsGonza);
    assert_eq!(
        row.needs_reason.as_deref(),
        Some("unconfirmed_submit: Phone number is invalid.")
    );
    assert!(
        row.screenshot_path.is_some(),
        "the -after screenshot is Gonza's evidence"
    );
    // The program never clicks the same form twice: a second run finds nothing prepared.
    assert!(env.run(&filler, &crit, SubmitMode::Live).await.is_empty());
    assert_eq!(filler.calls().len(), 1);
}
/// A click call that failed (the page went away with the click, or it timed out) leaves the form
/// possibly sent: the reason `unconfirmed_submit: ...` seals the attempt, so the daily cap and the
/// company's rest count it and the withdrawal pass leaves the application alone.
#[tokio::test]
async fn a_click_that_returned_an_error_seals_the_attempt() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "unconfirmed_submit: el clic de envío devolvió un error: Timeout 10000ms exceeded.",
    );
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let day_ago = *NOW - TimeDelta::hours(24);
    assert_eq!(env.tracker.clicked_since(day_ago).unwrap(), 0);
    env.run(&filler, &crit, SubmitMode::Live).await;
    assert_eq!(env.state("test:1"), State::NeedsGonza);
    assert_eq!(env.tracker.clicked_since(day_ago).unwrap(), 1);
    assert!(
        jobbot_app::discovery::MAY_HAVE_BEEN_SENT
            .iter()
            .any(|prefix| {
                "unconfirmed_submit: el clic de envío devolvió un error".starts_with(prefix)
            })
    );
}
/// A filler whose form Gonza sends from his own browser and confirms (`jobbot confirm`, from a
/// second handle on the same database file) before he closes the assisted tab.
struct ConfirmedElsewhere {
    gonza: Tracker,
}
#[async_trait::async_trait]
impl FormFiller for ConfirmedElsewhere {
    async fn submit(
        &self,
        job: &Job,
        _cv_pdf: &Path,
        _bank: &AnswerBank,
        _mode: FillMode,
        _shots_dir: &Path,
    ) -> anyhow::Result<SubmitResult> {
        self.gonza
            .transition(&job.key(), State::Submitted, None, *NOW)
            .unwrap();
        Ok(SubmitResult {
            status: SubmitStatus::NeedsGonza,
            reason: Some("assist_closed: cerraste la pestaña sin que la página confirmara".into()),
            screenshot: None,
        })
    }
}
#[tokio::test]
async fn an_assisted_form_confirmed_from_elsewhere_stays_submitted_when_its_tab_closes() {
    use jobbot_app::ports::store::INTENT_SUBMIT;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let tracker = Tracker::open(&db).unwrap();
    let filler = ConfirmedElsewhere {
        gonza: Tracker::open(&db).unwrap(),
    };
    let job = numbered(1);
    tracker.add_job(&job, 90.0, &BTreeMap::new(), *NOW).unwrap();
    tracker.create_application(&job.key(), *NOW).unwrap();
    tracker
        .set_cv_path(&job.key(), Path::new("cv.pdf"))
        .unwrap();
    tracker
        .transition(&job.key(), State::Prepared, None, *NOW)
        .unwrap();
    let outcomes = submit_all_while(
        &tracker,
        &criteria(),
        &exclusions(),
        &AnswerBank::default(),
        &filler,
        None,
        None,
        SubmitMode::Assist,
        dir.path(),
        &dir.path().join("STOP"),
        *NOW,
        &|| true,
    )
    .await
    .expect("a tab closed after `jobbot confirm` must not abort the run");
    assert_eq!(outcomes[0].status, "submitted");
    let row = tracker.get_application(&job.key()).unwrap().unwrap();
    assert_eq!(row.state, State::Submitted);
    assert!(tracker.open_intents(INTENT_SUBMIT).unwrap().is_empty());
}
/// A filler that looks, from a second handle on the same database file, at which submit intents
/// are open at the instant the program would click.
struct IntentProbe {
    watcher: Tracker,
    open_at_click: Mutex<Vec<Vec<String>>>,
}
#[async_trait::async_trait]
impl FormFiller for IntentProbe {
    async fn submit(
        &self,
        job: &Job,
        _cv_pdf: &Path,
        _bank: &AnswerBank,
        _mode: FillMode,
        shots_dir: &Path,
    ) -> anyhow::Result<SubmitResult> {
        let open = self
            .watcher
            .open_intents(jobbot_app::ports::store::INTENT_SUBMIT)?
            .into_iter()
            .map(|intent| intent.job_key)
            .collect();
        self.open_at_click.lock().unwrap().push(open);
        Ok(SubmitResult {
            status: SubmitStatus::Submitted,
            reason: None,
            screenshot: Some(shots_dir.join(format!("{}.png", safe_key(&job.key())))),
        })
    }
}
#[tokio::test]
async fn a_live_click_is_preceded_by_a_durable_intent_and_closes_it() {
    use jobbot_app::ports::store::INTENT_SUBMIT;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let tracker = Tracker::open(&db).unwrap();
    let probe = IntentProbe {
        watcher: Tracker::open(&db).unwrap(),
        open_at_click: Mutex::new(Vec::new()),
    };
    let job = numbered(1);
    tracker.add_job(&job, 90.0, &BTreeMap::new(), *NOW).unwrap();
    tracker.create_application(&job.key(), *NOW).unwrap();
    tracker
        .set_cv_path(&job.key(), Path::new("cv.pdf"))
        .unwrap();
    tracker
        .transition(&job.key(), State::Prepared, None, *NOW)
        .unwrap();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = submit_all_while(
        &tracker,
        &crit,
        &exclusions(),
        &AnswerBank::default(),
        &probe,
        None,
        None,
        SubmitMode::Live,
        dir.path(),
        &dir.path().join("STOP"),
        *NOW,
        &|| true,
    )
    .await
    .unwrap();
    assert_eq!(outcomes[0].status, "submitted");
    assert_eq!(
        *probe.open_at_click.lock().unwrap(),
        vec![vec!["test:1".to_string()]],
        "the intent must be on disk when the form is clicked"
    );
    assert!(
        tracker.open_intents(INTENT_SUBMIT).unwrap().is_empty(),
        "closed once the result is known"
    );
}
#[tokio::test]
async fn an_interrupted_click_is_handed_to_gonza_and_never_repeated() {
    use jobbot_app::ports::store::{INTENT_SUBMIT, INTERRUPTED_PREFIX};
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    // The previous run died after writing the intent and before it recorded the result.
    env.tracker
        .begin_intent(INTENT_SUBMIT, "test:1", *NOW)
        .unwrap()
        .unwrap();
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert_eq!(outcomes[0].status, "needs_gonza");
    assert!(
        filler.calls().is_empty(),
        "the form must not be clicked again"
    );
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    assert_eq!(row.state, State::NeedsGonza);
    assert!(
        row.needs_reason
            .as_deref()
            .unwrap()
            .starts_with(INTERRUPTED_PREFIX)
    );
    assert!(env.tracker.open_intents(INTENT_SUBMIT).unwrap().is_empty());
    assert!(env.run(&filler, &crit, SubmitMode::Live).await.is_empty());
    assert!(filler.calls().is_empty());
}
#[tokio::test]
async fn a_dry_run_writes_no_intent() {
    use jobbot_app::ports::store::INTENT_SUBMIT;
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::new();
    env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    assert_eq!(filler.calls().len(), 1);
    assert!(env.tracker.open_intents(INTENT_SUBMIT).unwrap().is_empty());
    assert_eq!(
        env.tracker.count_intents().unwrap(),
        0,
        "no row at all, not even a closed one: a dry run never clicks"
    );
}
#[tokio::test]
async fn assist_mode_submits_unreviewed_applications_and_counts_them_as_approved() {
    let env = Env::new();
    for (n, score) in [(1, 90.0), (2, 80.0)] {
        env.prepared(&numbered(n), score);
    }
    // Nothing approved and the review phase still open: Live would only dry-run these.
    let filler = FakeFiller::new();
    let outcomes = env.run(&filler, &criteria(), SubmitMode::Assist).await;
    assert_eq!(
        filler.calls(),
        vec![
            ("test:1".to_string(), SubmitMode::Assist),
            ("test:2".to_string(), SubmitMode::Assist)
        ]
    );
    let statuses: Vec<&str> = outcomes.iter().map(|o| o.status.as_str()).collect();
    assert_eq!(statuses, vec!["submitted"; 2]);
    for key in ["test:1", "test:2"] {
        let row = env.tracker.get_application(key).unwrap().unwrap();
        assert_eq!(row.state, State::Submitted, "{key}");
        assert!(
            row.approved,
            "{key}: Gonza's click in the browser is the review"
        );
    }
    assert_eq!(env.tracker.approved_submitted_total().unwrap(), 2);
}
#[tokio::test]
async fn review_dry_runs_are_capped_per_run() {
    let env = Env::new();
    for n in 1..=7 {
        env.prepared(&numbered(n), 100.0 - f64::from(n) * 10.0);
    }
    let filler = FakeFiller::new();
    let crit = Criteria {
        review_dry_runs_per_run: 2,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    // Only the two best get a screenshot this run; the rest wait, without an outcome.
    assert_eq!(
        filler.calls(),
        vec![
            ("test:1".to_string(), SubmitMode::DryRun),
            ("test:2".to_string(), SubmitMode::DryRun)
        ]
    );
    assert_eq!(outcomes.len(), 2);
    // The next run takes the next two.
    env.run(&filler, &crit, SubmitMode::Live).await;
    let keys: Vec<String> = filler.calls().into_iter().map(|(key, _)| key).collect();
    assert_eq!(keys, vec!["test:1", "test:2", "test:3", "test:4"]);
}
#[tokio::test]
async fn missing_cv_goes_to_gonza_instead_of_aborting() {
    let env = Env::new();
    // `resolve` on a below-floor exception lands here: prepared, approved, but never rendered.
    let no_cv = numbered(1);
    env.scored(&no_cv, 90.0);
    env.tracker
        .transition(&no_cv.key(), State::Prepared, None, *NOW)
        .unwrap();
    env.prepared(&numbered(2), 80.0);
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert_eq!(outcomes[0].status, "needs_gonza");
    assert!(
        outcomes[0].reason.as_deref().unwrap().starts_with("sin CV"),
        "{:?}",
        outcomes[0].reason
    );
    assert_eq!(env.state("test:1"), State::NeedsGonza);
    // The run went on with the next application.
    assert_eq!(
        filler.calls(),
        vec![("test:2".to_string(), SubmitMode::Live)]
    );
    assert_eq!(env.state("test:2"), State::Submitted);
}
#[tokio::test]
async fn stale_posting_is_withdrawn() {
    let env = Env::new();
    let old = make_job_with(|j| j.posted_at = Some(*NOW - TimeDelta::days(60)));
    env.prepared(&old, 90.0);
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert!(filler.calls().is_empty());
    assert_eq!(outcomes[0].status, "blocked");
    assert_eq!(outcomes[0].reason.as_deref(), Some("stale_posting"));
    assert_eq!(env.state("test:1"), State::Withdrawn);
}
/// Answers each posting with what the test says (open when it says nothing) and remembers who
/// it was asked about.
struct FakeChecker {
    answers: Vec<(String, OpenStatus)>,
    asked: Mutex<Vec<String>>,
}
impl FakeChecker {
    fn saying(answers: &[(&str, OpenStatus)]) -> FakeChecker {
        FakeChecker {
            answers: answers
                .iter()
                .map(|(key, status)| (key.to_string(), *status))
                .collect(),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}
#[async_trait::async_trait]
impl OpenChecker for FakeChecker {
    async fn check(&self, job: &Job) -> OpenStatus {
        self.asked.lock().unwrap().push(job.key());
        self.answers
            .iter()
            .find(|(key, _)| *key == job.key())
            .map_or(OpenStatus::Open, |(_, status)| *status)
    }
}
#[tokio::test]
async fn a_posting_that_has_closed_is_withdrawn_before_its_form_is_opened() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::new();
    let checker = FakeChecker::saying(&[("test:1", OpenStatus::Closed)]);
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };

    let outcomes = env
        .run_checked(&filler, &checker, &crit, SubmitMode::Live)
        .await;

    assert!(filler.calls().is_empty(), "no form is opened");
    assert_eq!(outcomes[0].status, "blocked");
    assert_eq!(outcomes[0].reason.as_deref(), Some("posting_closed"));
    assert_eq!(env.state("test:1"), State::Withdrawn);
}
#[tokio::test]
async fn a_closed_posting_does_not_use_up_a_dry_run_slot() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    env.prepared(&numbered(2), 80.0);
    let filler = FakeFiller::new();
    let checker = FakeChecker::saying(&[("test:1", OpenStatus::Closed)]);
    let crit = Criteria {
        review_dry_runs_per_run: 1,
        ..criteria()
    };

    env.run_checked(&filler, &checker, &crit, SubmitMode::DryRun)
        .await;

    assert_eq!(
        filler.calls(),
        vec![("test:2".to_string(), SubmitMode::DryRun)],
        "the one slot goes to the posting that is still open"
    );
}
#[tokio::test]
async fn an_open_or_unknown_answer_goes_on_to_the_form() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    env.prepared(&numbered(2), 80.0);
    let filler = FakeFiller::new();
    let checker = FakeChecker::saying(&[("test:1", OpenStatus::Unknown)]);

    env.run_checked(&filler, &checker, &criteria(), SubmitMode::DryRun)
        .await;

    assert_eq!(filler.calls().len(), 2);
    assert_eq!(env.state("test:1"), State::Prepared);
    assert_eq!(env.state("test:2"), State::Prepared);
}
#[tokio::test]
async fn the_source_is_not_asked_about_what_the_gate_already_refused() {
    let env = Env::new();
    let old = make_job_with(|j| j.posted_at = Some(*NOW - TimeDelta::days(60)));
    env.prepared(&old, 90.0);
    let filler = FakeFiller::new();
    let checker = FakeChecker::saying(&[]);

    let outcomes = env
        .run_checked(&filler, &checker, &criteria(), SubmitMode::DryRun)
        .await;

    assert_eq!(outcomes[0].reason.as_deref(), Some("stale_posting"));
    assert!(checker.asked().is_empty(), "no request for a refused row");
}
#[tokio::test]
async fn listing_only_site_goes_to_gonza() {
    let env = Env::new();
    let job =
        make_job_with(|j| j.apply_url = "https://remotive.com/remote-jobs/software-dev/x-1".into());
    env.prepared(&job, 90.0);
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    assert!(filler.calls().is_empty());
    assert_eq!(outcomes[0].reason.as_deref(), Some("listing_only"));
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    assert_eq!(row.state, State::NeedsGonza);
    assert!(row.needs_reason.unwrap().starts_with("listing_only"));
}
// ---- Final review fixes ----

#[tokio::test]
async fn an_unconfirmed_click_holds_back_the_next_posting_at_the_same_company() {
    let env = Env::new();
    let first = make_job_with(|j| j.external_id = "1".into());
    let second = make_job_with(|j| j.external_id = "2".into());
    env.prepared(&first, 90.0);
    env.prepared(&second, 80.0);
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "unconfirmed_submit: sin confirmación visible",
    );
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    let outcomes = env.run(&filler, &crit, SubmitMode::Live).await;
    // The first click may well have gone through; a second application to the same company
    // minutes later would break the 90-day rule even if Gonza later confirms the first one.
    assert_eq!(
        filler.calls(),
        vec![("test:1".to_string(), SubmitMode::Live)]
    );
    assert_eq!(
        outcomes[1],
        SubmitOutcome {
            job_key: "test:2".to_string(),
            status: "blocked".to_string(),
            reason: Some("company_cooldown".to_string()),
        }
    );
    assert_eq!(env.state("test:2"), State::Prepared);
}
#[tokio::test]
async fn unconfirmed_clicks_count_toward_the_daily_cap() {
    let env = Env::new();
    for n in 1..=3 {
        env.prepared(&numbered(n), 100.0 - f64::from(n) * 10.0);
    }
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "unconfirmed_submit: Phone number is invalid.",
    );
    let crit = Criteria {
        first_n_reviewed: 0,
        daily_cap: 2,
        ..criteria()
    };
    env.run(&filler, &crit, SubmitMode::Live).await;
    assert_eq!(filler.calls().len(), 2, "the third click is over the cap");
    assert_eq!(env.state("test:3"), State::Prepared);
}
#[tokio::test]
async fn an_assist_timeout_is_not_counted_as_a_click() {
    // Guard for the other side of the rule: only the program's own unconfirmed Live clicks
    // count. A form Gonza never clicked (assist_timeout) must not lock the company for 90 days.
    let env = Env::new();
    let first = make_job_with(|j| j.external_id = "1".into());
    let second = make_job_with(|j| j.external_id = "2".into());
    env.prepared(&first, 90.0);
    env.prepared(&second, 80.0);
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "assist_timeout: sin confirmación en 10 minutos",
    );
    env.run(&filler, &criteria(), SubmitMode::Assist).await;
    assert_eq!(
        filler.calls(),
        vec![
            ("test:1".to_string(), SubmitMode::Assist),
            ("test:2".to_string(), SubmitMode::Assist)
        ]
    );
}
#[tokio::test]
async fn submit_stops_when_the_execution_window_closes_mid_run() {
    let env = Env::new();
    for n in 1..=3 {
        env.prepared(&numbered(n), 100.0 - f64::from(n) * 10.0);
    }
    let filler = FakeFiller::new();
    let crit = Criteria {
        first_n_reviewed: 0,
        ..criteria()
    };
    // The window is open for the first application and closes before the second.
    let asked = std::sync::atomic::AtomicUsize::new(0);
    let still_open = || asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 1;
    let outcomes = submit_all_while(
        &env.tracker,
        &crit,
        &exclusions(),
        &AnswerBank::default(),
        &filler,
        None,
        None,
        SubmitMode::Live,
        env.dir.path(),
        &env.stop_file(),
        *NOW,
        &still_open,
    )
    .await
    .unwrap();
    assert_eq!(
        filler.calls(),
        vec![("test:1".to_string(), SubmitMode::Live)]
    );
    assert_eq!(outcomes.len(), 1);
    assert_eq!(env.state("test:2"), State::Prepared);
    assert_eq!(env.state("test:3"), State::Prepared);
}
// ---- handoff hosts (Ashby): the bot fills, Gonza sends ----

use jobbot_app::actions;
use jobbot_app::apply::handoff::FormInspector;
use jobbot_domain::handoff::{FieldReport, FormInspection, SheetAnswer, SheetField};

const ASHBY_APPLY: &str = "https://jobs.ashbyhq.com/acme/123/application";
fn ashby_job(n: u32) -> Job {
    make_job_with(|j| {
        j.external_id = n.to_string();
        j.company = format!("Co{n}");
        j.title = format!("Role {n}");
        j.apply_url = ASHBY_APPLY.into();
    })
}
/// A criteria file where nothing but the handoff rule stands between an approved application
/// and a real click.
fn open_gate() -> Criteria {
    Criteria {
        first_n_reviewed: 0,
        ..criteria()
    }
}
/// Test double for `FormInspector`: remembers the job keys and answers with a fixed inspection.
struct FakeInspector {
    calls: Mutex<Vec<String>>,
    reply: FormInspection,
}
impl FakeInspector {
    fn new(status: SubmitStatus, reason: Option<&str>, report: FieldReport) -> FakeInspector {
        FakeInspector {
            calls: Mutex::new(Vec::new()),
            reply: FormInspection {
                result: SubmitResult {
                    status,
                    reason: reason.map(str::to_string),
                    screenshot: Some(PathBuf::from("shots/inspected.png")),
                },
                report,
            },
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}
#[async_trait::async_trait]
impl FormInspector for FakeInspector {
    async fn inspect(
        &self,
        job: &Job,
        _cv_pdf: &Path,
        _bank: &AnswerBank,
        _shots_dir: &Path,
    ) -> anyhow::Result<FormInspection> {
        self.calls.lock().unwrap().push(job.key());
        Ok(self.reply.clone())
    }
}
fn answered(label: &str, text: &str) -> SheetField {
    SheetField {
        label: label.into(),
        required: true,
        answer: SheetAnswer::Text(text.into()),
        options: vec![],
    }
}
#[tokio::test]
async fn a_handoff_form_is_never_assisted_or_sent_whatever_the_mode() {
    for mode in [SubmitMode::DryRun, SubmitMode::Assist, SubmitMode::Live] {
        for approved in [false, true] {
            let env = Env::new();
            env.prepared(&ashby_job(1), 90.0);
            if approved {
                env.tracker.approve("test:1").unwrap();
            }
            let filler = FakeFiller::new();
            let outcomes = env.run(&filler, &open_gate(), mode).await;
            // Only ever a dry run, even where Assist or an approved Live would have clicked.
            assert_eq!(
                filler.calls(),
                vec![("test:1".to_string(), SubmitMode::DryRun)],
                "{mode:?} approved={approved}"
            );
            assert_eq!(outcomes[0].status, "needs_gonza");
            let row = env.tracker.get_application("test:1").unwrap().unwrap();
            assert_eq!(row.state, State::NeedsGonza, "{mode:?} approved={approved}");
            assert!(row.submitted_at.is_none());
            assert_eq!(row.approved, approved, "the bot approves nothing");
            let day_ago = *NOW - TimeDelta::hours(24);
            assert_eq!(env.tracker.clicked_since(day_ago).unwrap(), 0);
        }
    }
}
#[tokio::test]
async fn a_handoff_application_goes_to_needs_gonza_with_the_sheet_path_in_the_reason() {
    let env = Env::new();
    env.prepared(&ashby_job(1), 90.0);
    let filler = FakeFiller::new();
    env.run(&filler, &open_gate(), SubmitMode::Live).await;
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    let reason = row.needs_reason.unwrap();
    assert!(reason.starts_with("handoff:"), "{reason}");
    let sheet = env.dir.path().join("handoff").join("test_1.md");
    assert!(
        reason.contains(&sheet.display().to_string()),
        "{reason} should name {}",
        sheet.display()
    );
    assert!(sheet.is_file());
    assert!(
        row.screenshot_path.is_some(),
        "the dry run's screenshot is kept"
    );
    // The way out is the existing confirm: no new state.
    let outcome = actions::confirm(&env.tracker, "test:1", *NOW).unwrap();
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(env.state("test:1"), State::Submitted);
}
#[tokio::test]
async fn the_sheet_names_the_job_the_link_the_cv_and_what_the_bank_could_not_answer() {
    let env = Env::new();
    env.prepared(&ashby_job(1), 90.0);
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "faltan respuestas: Why are you interested in this role? *",
    );
    let outcomes = env.run(&filler, &open_gate(), SubmitMode::DryRun).await;
    assert_eq!(outcomes[0].status, "needs_gonza");
    assert!(
        outcomes[0]
            .reason
            .as_deref()
            .unwrap()
            .starts_with("handoff:")
    );
    let sheet = env.handoff_sheet("test:1");
    for expected in [
        "# Hoja de respuestas: Co1 — Role 1",
        ASHBY_APPLY,
        "cv.pdf",
        "Why are you interested in this role? *",
        "⚠ OBLIGATORIA, contéstala tú",
        "jobbot confirm test:1",
    ] {
        assert!(
            sheet.contains(expected),
            "missing {expected:?} in:\n{sheet}"
        );
    }
}
#[tokio::test]
async fn an_inspector_lists_every_field_and_the_filler_is_left_alone() {
    let env = Env::new();
    env.prepared(&ashby_job(1), 90.0);
    let report = FieldReport::Full(vec![
        answered("Name", "Jane Doe"),
        SheetField::missing("Why this role?"),
        SheetField {
            label: "LinkedIn".into(),
            required: false,
            answer: SheetAnswer::Skipped,
            options: vec![],
        },
    ]);
    let inspector = FakeInspector::new(
        SubmitStatus::NeedsGonza,
        Some("faltan respuestas: Why this role?"),
        report,
    );
    let filler = FakeFiller::new();
    let outcomes = env
        .run_with(&filler, Some(&inspector), &open_gate(), SubmitMode::Assist)
        .await
        .unwrap();
    assert_eq!(inspector.calls(), vec!["test:1"]);
    assert!(filler.calls().is_empty(), "the inspector did the dry run");
    assert_eq!(outcomes[0].status, "needs_gonza");
    let sheet = env.handoff_sheet("test:1");
    for expected in [
        "### 1. Name (obligatoria)",
        "Jane Doe",
        "### 2. Why this role? (obligatoria)",
        "⚠ OBLIGATORIA, contéstala tú",
        "### 3. LinkedIn (opcional)",
        "(opcional, sin respuesta)",
    ] {
        assert!(
            sheet.contains(expected),
            "missing {expected:?} in:\n{sheet}"
        );
    }
    let row = env.tracker.get_application("test:1").unwrap().unwrap();
    assert_eq!(row.screenshot_path.as_deref(), Some("shots/inspected.png"));
}
#[tokio::test]
async fn a_form_the_bot_could_not_read_still_gets_a_sheet() {
    let env = Env::new();
    env.prepared(&ashby_job(1), 90.0);
    let inspector = FakeInspector::new(
        SubmitStatus::NeedsGonza,
        Some("page_error: HTTP 403"),
        FieldReport::Unreadable,
    );
    let outcomes = env
        .run_with(
            &FakeFiller::new(),
            Some(&inspector),
            &open_gate(),
            SubmitMode::Live,
        )
        .await
        .unwrap();
    let reason = outcomes[0].reason.as_deref().unwrap();
    assert!(reason.starts_with("handoff:"), "{reason}");
    assert!(reason.contains("page_error: HTTP 403"), "{reason}");
    let sheet = env.handoff_sheet("test:1");
    assert!(sheet.contains("no pudo leer"), "{sheet}");
    assert!(sheet.contains(ASHBY_APPLY));
    assert_eq!(env.state("test:1"), State::NeedsGonza);
}
#[tokio::test]
async fn a_handoff_host_in_the_listing_link_counts_too() {
    let env = Env::new();
    let job = make_job_with(|j| {
        j.url = "https://jobs.ashbyhq.com/acme/123".into();
        j.apply_url = "https://acme.example.com/careers/123/apply".into();
    });
    env.prepared(&job, 90.0);
    let filler = FakeFiller::new();
    env.run(&filler, &open_gate(), SubmitMode::Live).await;
    assert_eq!(
        filler.calls(),
        vec![("test:1".to_string(), SubmitMode::DryRun)]
    );
    assert_eq!(env.state("test:1"), State::NeedsGonza);
}
#[tokio::test]
async fn other_hosts_are_not_handed_off() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::new();
    env.run(&filler, &open_gate(), SubmitMode::Live).await;
    assert_eq!(
        filler.calls(),
        vec![("test:1".to_string(), SubmitMode::Live)]
    );
    assert!(!env.dir.path().join("handoff").exists());
}
#[tokio::test]
async fn the_gate_still_decides_before_a_handoff() {
    let env = Env::new();
    // Same company as an application already sent: the company cooldown holds it back.
    let sent = make_job_with(|j| j.external_id = "1".into());
    env.prepared(&sent, 90.0);
    env.tracker.approve(&sent.key()).unwrap();
    env.tracker
        .transition(&sent.key(), State::Submitted, None, *NOW)
        .unwrap();
    let later = make_job_with(|j| {
        j.external_id = "2".into();
        j.apply_url = ASHBY_APPLY.into();
    });
    env.prepared(&later, 80.0);
    let filler = FakeFiller::new();
    let outcomes = env.run(&filler, &open_gate(), SubmitMode::Live).await;
    assert!(filler.calls().is_empty());
    assert_eq!(outcomes[0].status, "blocked");
    assert_eq!(outcomes[0].reason.as_deref(), Some("company_cooldown"));
    assert_eq!(env.state("test:2"), State::Prepared);
    assert!(!env.dir.path().join("handoff").exists());
}
#[tokio::test]
async fn a_handoff_that_fails_to_open_goes_to_gonza_and_the_run_goes_on() {
    struct Broken;
    #[async_trait::async_trait]
    impl FormInspector for Broken {
        async fn inspect(
            &self,
            _job: &Job,
            _cv_pdf: &Path,
            _bank: &AnswerBank,
            _shots_dir: &Path,
        ) -> anyhow::Result<FormInspection> {
            anyhow::bail!("the driver died")
        }
    }
    let env = Env::new();
    env.prepared(&ashby_job(1), 90.0);
    env.prepared(&numbered(2), 80.0);
    let filler = FakeFiller::new();
    let outcomes = env
        .run_with(&filler, Some(&Broken), &open_gate(), SubmitMode::Live)
        .await
        .unwrap();
    assert_eq!(env.state("test:1"), State::NeedsGonza);
    assert_eq!(outcomes[0].status, "needs_gonza");
    assert!(
        outcomes[0]
            .reason
            .as_deref()
            .unwrap()
            .starts_with("page_error: the driver died"),
        "{:?}",
        outcomes[0].reason
    );
    assert_eq!(
        filler.calls().len(),
        1,
        "the next application is still looked at"
    );
}
/// Fails for the keys it is told to, answers a dry run for the others.
struct FailingFiller {
    failing: Vec<String>,
    calls: Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl FormFiller for FailingFiller {
    async fn submit(
        &self,
        job: &Job,
        _cv_pdf: &Path,
        _bank: &AnswerBank,
        _mode: FillMode,
        _shots_dir: &Path,
    ) -> anyhow::Result<SubmitResult> {
        self.calls.lock().unwrap().push(job.key());
        if self.failing.contains(&job.key()) {
            anyhow::bail!("Execution context was destroyed\nCall log: ...");
        }
        Ok(SubmitResult {
            status: SubmitStatus::DryRun,
            reason: None,
            screenshot: None,
        })
    }
}
impl FailingFiller {
    fn failing_on(keys: &[&str]) -> FailingFiller {
        FailingFiller {
            failing: keys.iter().map(|key| key.to_string()).collect(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}
/// `run_with` for any filler (the helper takes the fake one).
async fn run_any(
    env: &Env,
    filler: &dyn FormFiller,
    crit: &Criteria,
) -> anyhow::Result<Vec<SubmitOutcome>> {
    submit_all_while(
        &env.tracker,
        crit,
        &exclusions(),
        &AnswerBank::default(),
        filler,
        None,
        None,
        SubmitMode::DryRun,
        env.dir.path(),
        &env.stop_file(),
        *NOW,
        &|| true,
    )
    .await
}
#[tokio::test]
async fn a_page_that_errors_goes_to_gonza_and_the_run_goes_on() {
    let env = Env::new();
    for n in 1..=3 {
        env.prepared(&numbered(n), 100.0 - f64::from(n) * 10.0);
    }
    let filler = FailingFiller::failing_on(&["test:1"]);

    let outcomes = run_any(&env, &filler, &criteria()).await.unwrap();

    assert_eq!(filler.calls(), vec!["test:1", "test:2", "test:3"]);
    assert_eq!(env.state("test:1"), State::NeedsGonza);
    assert_eq!(outcomes[0].status, "needs_gonza");
    // The first line of the error only: the call log is noise for the tracker.
    assert_eq!(
        outcomes[0].reason.as_deref(),
        Some("page_error: Execution context was destroyed")
    );
    assert_eq!(outcomes[1].status, "dry_run");
    assert_eq!(outcomes[2].status, "dry_run");
}
#[tokio::test]
async fn three_errors_in_a_row_mean_the_browser_is_gone_and_stop_the_run() {
    let env = Env::new();
    for n in 1..=5 {
        env.prepared(&numbered(n), 100.0 - f64::from(n) * 10.0);
    }
    let keys: Vec<String> = (1..=5).map(|n| format!("test:{n}")).collect();
    let filler = FailingFiller {
        failing: keys.clone(),
        calls: Mutex::new(Vec::new()),
    };

    let outcomes = run_any(&env, &filler, &criteria()).await.unwrap();

    assert_eq!(filler.calls().len(), 3, "the run stops at the third error");
    assert_eq!(env.state("test:3"), State::NeedsGonza);
    assert_eq!(
        env.state("test:4"),
        State::Prepared,
        "left for the next run"
    );
    let last = outcomes.last().unwrap();
    assert_eq!(last.status, "stopped");
    assert!(
        last.reason
            .as_deref()
            .unwrap()
            .contains("3 errores seguidos"),
        "{:?}",
        last.reason
    );
}
#[tokio::test]
async fn an_error_between_good_pages_does_not_count_as_a_streak() {
    let env = Env::new();
    for n in 1..=6 {
        env.prepared(&numbered(n), 100.0 - f64::from(n) * 10.0);
    }
    // Errors on 1, 3 and 5 are never three in a row.
    let filler = FailingFiller::failing_on(&["test:1", "test:3", "test:5"]);
    let crit = Criteria {
        review_dry_runs_per_run: 10,
        ..criteria()
    };

    let outcomes = run_any(&env, &filler, &crit).await.unwrap();

    assert_eq!(filler.calls().len(), 6);
    assert!(outcomes.iter().all(|o| o.status != "stopped"));
}
#[tokio::test]
async fn handoff_forms_count_toward_the_dry_runs_of_one_run() {
    let env = Env::new();
    for n in 1..=3 {
        env.prepared(&ashby_job(n), 100.0 - f64::from(n) * 10.0);
    }
    let crit = Criteria {
        review_dry_runs_per_run: 2,
        ..open_gate()
    };
    let outcomes = env.run(&FakeFiller::new(), &crit, SubmitMode::Live).await;
    assert_eq!(outcomes.len(), 2);
    assert_eq!(env.state("test:1"), State::NeedsGonza);
    assert_eq!(env.state("test:2"), State::NeedsGonza);
    assert_eq!(env.state("test:3"), State::Prepared, "next run");
}
// ---- the per-company cap ----

/// Job number `n` at the company named `company`.
fn at(company: &str, n: u32) -> Job {
    make_job_with(|j| {
        j.external_id = n.to_string();
        j.company = company.into();
        j.title = format!("Role {n}");
    })
}
/// Moves an application through the given states, in order, 200 days ago: far enough back
/// that the 90-day company cooldown of the gate stays out of what these tests are about.
fn walk(env: &Env, key: &str, states: &[State]) {
    let long_ago = *NOW - TimeDelta::days(200);
    for state in states {
        env.tracker.transition(key, *state, None, long_ago).unwrap();
    }
}
#[tokio::test]
async fn three_prepared_applications_at_one_company_only_the_best_two_proceed() {
    let env = Env::new();
    for (n, score) in [(1, 90.0), (2, 80.0), (3, 70.0)] {
        env.prepared(&at("Acme", n), score);
    }
    let filler = FakeFiller::new();
    let outcomes = env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    assert_eq!(
        filler.calls(),
        vec![
            ("test:1".to_string(), SubmitMode::DryRun),
            ("test:2".to_string(), SubmitMode::DryRun)
        ]
    );
    assert_eq!(outcomes.len(), 3);
    assert_eq!(outcomes[2].job_key, "test:3");
    assert_eq!(outcomes[2].status, "blocked");
    let reason = outcomes[2].reason.as_deref().unwrap();
    assert!(reason.starts_with("tope por empresa"), "{reason}");
    assert!(
        reason.contains("Acme") && reason.contains("test:3"),
        "{reason}"
    );
    // The skipped one stays exactly as it was.
    let row = env.tracker.get_application("test:3").unwrap().unwrap();
    assert_eq!(row.state, State::Prepared);
    assert!(row.screenshot_path.is_none());
    // And no line in the results log: nothing was opened for it.
    assert!(env.results().iter().all(|line| line.key != "test:3"));
}
#[tokio::test]
async fn a_different_company_is_unaffected_by_the_cap() {
    let env = Env::new();
    for (n, score) in [(1, 90.0), (2, 80.0), (3, 70.0)] {
        env.prepared(&at("Acme", n), score);
    }
    env.prepared(&at("Globex", 4), 60.0);
    let filler = FakeFiller::new();
    env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    let keys: Vec<String> = filler.calls().into_iter().map(|(key, _)| key).collect();
    assert_eq!(keys, vec!["test:1", "test:2", "test:4"]);
}
#[tokio::test]
async fn submitted_applications_and_beyond_always_count_and_are_never_skipped() {
    let env = Env::new();
    // One application at Acme is already at the interview stage; two more are prepared.
    env.prepared(&at("Acme", 1), 50.0);
    walk(&env, "test:1", &[State::Submitted, State::Interview]);
    env.prepared(&at("Acme", 2), 90.0);
    env.prepared(&at("Acme", 3), 80.0);
    let filler = FakeFiller::new();
    let outcomes = env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    // The interview holds one of the two places, so only the best prepared one gets the other.
    assert_eq!(
        filler.calls(),
        vec![("test:2".to_string(), SubmitMode::DryRun)]
    );
    assert_eq!(outcomes[1].job_key, "test:3");
    assert!(
        outcomes[1]
            .reason
            .as_deref()
            .unwrap()
            .starts_with("tope por empresa")
    );
    assert_eq!(
        env.state("test:1"),
        State::Interview,
        "a sent application is never touched"
    );
}
#[tokio::test]
async fn two_sent_applications_leave_no_room_for_a_third() {
    let env = Env::new();
    env.prepared(&at("Acme", 1), 50.0);
    walk(&env, "test:1", &[State::Submitted]);
    env.prepared(&at("Acme", 2), 50.0);
    walk(&env, "test:2", &[State::Submitted, State::Replied]);
    env.prepared(&at("Acme", 3), 99.0);
    let filler = FakeFiller::new();
    let outcomes = env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    assert!(filler.calls().is_empty(), "{outcomes:?}");
    assert!(
        outcomes[0]
            .reason
            .as_deref()
            .unwrap()
            .starts_with("tope por empresa")
    );
    assert_eq!(env.state("test:3"), State::Prepared);
}
#[tokio::test]
async fn a_finished_application_stops_counting() {
    let env = Env::new();
    // Rejected and withdrawn are over; ghosted is not in flight either.
    env.prepared(&at("Acme", 1), 50.0);
    walk(&env, "test:1", &[State::Submitted, State::Rejected]);
    env.prepared(&at("Acme", 2), 50.0);
    walk(&env, "test:2", &[State::Withdrawn]);
    env.prepared(&at("Acme", 3), 99.0);
    env.prepared(&at("Acme", 4), 98.0);
    env.prepared(&at("Acme", 5), 97.0);
    let filler = FakeFiller::new();
    env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    let calls: Vec<String> = filler.calls().into_iter().map(|(key, _)| key).collect();
    assert_eq!(calls, vec!["test:3", "test:4"]);
    assert_eq!(env.state("test:5"), State::Prepared);
}
#[tokio::test]
async fn an_application_waiting_for_gonza_holds_a_place_by_its_score() {
    let env = Env::new();
    env.prepared(&at("Acme", 1), 85.0);
    walk(&env, "test:1", &[State::NeedsGonza]);
    env.prepared(&at("Acme", 2), 90.0);
    env.prepared(&at("Acme", 3), 70.0);
    let filler = FakeFiller::new();
    env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    // 90 and 85 hold the two places: the 70 is left out even though 85 is not being sent.
    let calls: Vec<String> = filler.calls().into_iter().map(|(key, _)| key).collect();
    assert_eq!(calls, vec!["test:2"]);
    assert_eq!(env.state("test:3"), State::Prepared);
    assert_eq!(env.state("test:1"), State::NeedsGonza);
}
#[tokio::test]
async fn the_company_is_its_name_lowercased_and_trimmed() {
    let env = Env::new();
    env.prepared(&at("Acme AI", 1), 90.0);
    env.prepared(&at("  acme ai ", 2), 80.0);
    env.prepared(&at("ACME AI", 3), 70.0);
    // A name that only looks alike is another company.
    env.prepared(&at("Acme AI Labs", 4), 60.0);
    let filler = FakeFiller::new();
    env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    let calls: Vec<String> = filler.calls().into_iter().map(|(key, _)| key).collect();
    assert_eq!(calls, vec!["test:1", "test:2", "test:4"]);
}
#[tokio::test]
async fn a_blank_company_name_is_never_capped() {
    let env = Env::new();
    for n in 1..=3 {
        env.prepared(&at("  ", n), 100.0 - f64::from(n));
    }
    let filler = FakeFiller::new();
    let crit = Criteria {
        review_dry_runs_per_run: 10,
        ..criteria()
    };
    env.run(&filler, &crit, SubmitMode::DryRun).await;
    assert_eq!(filler.calls().len(), 3);
}
#[tokio::test]
async fn prepare_skips_the_lowest_scored_extras_of_a_company() {
    let env = Env::new();
    let master = env.cv_master(MASTER);
    for (n, score) in [(1, 80.0), (2, 70.0), (3, 60.0)] {
        env.scored(&at("Acme", n), score);
    }
    env.scored(&at("Globex", 4), 50.0);
    let renderer = FakeRenderer::default();
    let report = prepare_report(&env.tracker, &master, &env.out_dir(), &renderer, *NOW)
        .await
        .unwrap();
    assert_eq!(report.prepared, vec!["test:1", "test:2", "test:4"]);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].job_key, "test:3");
    let line = report.skipped[0].line();
    assert!(line.starts_with("tope por empresa"), "{line}");
    assert!(line.contains("Acme") && line.contains("test:3"), "{line}");
    // The skipped one stays scored and gets no CV; the next run finds the same.
    assert_eq!(env.state("test:3"), State::Scored);
    assert_eq!(renderer.rendered.lock().unwrap().len(), 3);
    let again = prepare_report(&env.tracker, &master, &env.out_dir(), &renderer, *NOW)
        .await
        .unwrap();
    assert!(again.prepared.is_empty());
    assert_eq!(again.skipped.len(), 1);
}
#[tokio::test]
async fn prepare_counts_what_is_already_in_flight_at_the_company() {
    let env = Env::new();
    let master = env.cv_master(MASTER);
    // One application at Acme is already sent and one waits for Gonza: both hold a place.
    env.prepared(&at("Acme", 1), 50.0);
    walk(&env, "test:1", &[State::Submitted]);
    env.prepared(&at("Acme", 2), 50.0);
    walk(&env, "test:2", &[State::NeedsGonza]);
    env.scored(&at("Acme", 3), 99.0);
    let report = prepare_report(
        &env.tracker,
        &master,
        &env.out_dir(),
        &FakeRenderer::default(),
        *NOW,
    )
    .await
    .unwrap();
    assert!(report.prepared.is_empty());
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(env.state("test:3"), State::Scored);
}
/// A posting below the pay floor that discovery set aside for Gonza to decide on: it is not an
/// application in flight and holds no place at its company.
fn flag_below_floor(env: &Env, job: &Job, score: f64) {
    env.scored(job, score);
    env.tracker
        .transition(
            &job.key(),
            State::NeedsGonza,
            Some(jobbot_app::discovery::BELOW_FLOOR_NOTE),
            *NOW - TimeDelta::days(200),
        )
        .unwrap();
}
#[tokio::test]
async fn a_posting_set_aside_below_the_floor_holds_no_place_at_its_company() {
    let env = Env::new();
    let master = env.cv_master(MASTER);
    flag_below_floor(&env, &at("Acme", 1), 40.0);
    env.scored(&at("Acme", 2), 80.0);
    env.scored(&at("Acme", 3), 70.0);

    let report = prepare_report(
        &env.tracker,
        &master,
        &env.out_dir(),
        &FakeRenderer::default(),
        *NOW,
    )
    .await
    .unwrap();

    assert_eq!(report.prepared, vec!["test:2", "test:3"]);
    assert!(report.skipped.is_empty());
}
#[tokio::test]
async fn submit_does_not_let_a_posting_set_aside_below_the_floor_take_a_place() {
    let env = Env::new();
    flag_below_floor(&env, &at("Acme", 1), 95.0);
    env.prepared(&at("Acme", 2), 80.0);
    env.prepared(&at("Acme", 3), 70.0);
    let filler = FakeFiller::new();

    let outcomes = env.run(&filler, &criteria(), SubmitMode::DryRun).await;

    assert_eq!(
        outcomes
            .iter()
            .filter(|o| o.status == "dry_run")
            .map(|o| o.job_key.as_str())
            .collect::<Vec<_>>(),
        vec!["test:2", "test:3"],
        "{outcomes:?}"
    );
}
#[tokio::test]
async fn prepare_all_applies_the_cap_too() {
    let env = Env::new();
    let master = env.cv_master(MASTER);
    for (n, score) in [(1, 80.0), (2, 70.0), (3, 60.0)] {
        env.scored(&at("Acme", n), score);
    }
    let keys = prepare_all(
        &env.tracker,
        &master,
        &env.out_dir(),
        &FakeRenderer::default(),
        *NOW,
    )
    .await
    .unwrap();
    assert_eq!(keys, vec!["test:1", "test:2"]);
}
// ---- the results log ----

use jobbot_app::apply::results::{AttemptMode, ResultLine, read_last, results_path};
impl Env {
    fn results(&self) -> Vec<ResultLine> {
        read_last(&results_path(self.dir.path()), usize::MAX)
            .unwrap()
            .lines
    }
}
#[tokio::test]
async fn a_dry_run_leaves_one_line_in_the_results_log() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    // Unreviewed and under Live: the gate turns it into a dry run, and the log says so.
    env.run(&FakeFiller::new(), &criteria(), SubmitMode::Live)
        .await;
    let lines = env.results();
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(line.timestamp, "2026-10-01T12:00:00Z");
    assert_eq!(
        (line.key.as_str(), line.source.as_str()),
        ("test:1", "test")
    );
    assert_eq!(
        (line.company.as_str(), line.title.as_str()),
        ("Co1", "Role 1")
    );
    assert_eq!(line.mode, AttemptMode::DryRun);
    assert_eq!(line.state, "prepared");
    assert!(line.screenshot.as_deref().unwrap().ends_with("test_1.png"));
    assert_eq!(line.handoff_sheet, None);
    assert!(line.fields_left.is_empty());
    assert_eq!(line.fields_filled, None, "the filler does not count them");
}
#[tokio::test]
async fn a_live_send_is_logged_as_submitted() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    env.run(&FakeFiller::new(), &open_gate(), SubmitMode::Live)
        .await;
    let lines = env.results();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].mode, AttemptMode::Live);
    assert_eq!(lines[0].state, "submitted");
}
#[tokio::test]
async fn an_assisted_form_is_logged_with_what_was_left_pending() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "assist_closed: cerraste la pestaña. Quedaba pendiente: faltan respuestas: Email *; Phone *",
    );
    env.run(&filler, &criteria(), SubmitMode::Assist).await;
    let lines = env.results();
    assert_eq!(lines[0].mode, AttemptMode::Assist);
    assert_eq!(lines[0].state, "needs_gonza");
    assert_eq!(lines[0].fields_left, vec!["Email *", "Phone *"]);
}
#[tokio::test]
async fn a_dry_run_that_lacks_answers_logs_which_ones() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::replying(
        SubmitStatus::NeedsGonza,
        "faltan respuestas: Why are you interested in this role? * — opción no encontrada: Country = Atlantis",
    );
    env.run(&filler, &criteria(), SubmitMode::DryRun).await;
    let lines = env.results();
    assert_eq!(lines[0].mode, AttemptMode::DryRun);
    assert_eq!(lines[0].state, "needs_gonza");
    assert_eq!(
        lines[0].fields_left,
        vec!["Why are you interested in this role? *"]
    );
}
#[tokio::test]
async fn a_handoff_is_logged_with_its_sheet_and_the_counts_of_the_inspector() {
    let env = Env::new();
    env.prepared(&ashby_job(1), 90.0);
    let report = FieldReport::Full(vec![
        answered("Name", "Jane Doe"),
        answered("Email", "search@example.com"),
        SheetField::missing("Why this role?"),
    ]);
    let inspector = FakeInspector::new(
        SubmitStatus::NeedsGonza,
        Some("faltan respuestas: Why this role?"),
        report,
    );
    env.run_with(
        &FakeFiller::new(),
        Some(&inspector),
        &open_gate(),
        SubmitMode::Assist,
    )
    .await
    .unwrap();
    let lines = env.results();
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(line.mode, AttemptMode::Handoff);
    assert_eq!(line.state, "needs_gonza");
    assert_eq!(line.fields_filled, Some(2));
    assert_eq!(line.fields_left, vec!["Why this role?"]);
    assert_eq!(line.screenshot.as_deref(), Some("shots/inspected.png"));
    let sheet = env.dir.path().join("handoff").join("test_1.md");
    assert_eq!(
        line.handoff_sheet.as_deref(),
        Some(sheet.to_string_lossy().as_ref())
    );
}
#[tokio::test]
async fn what_the_gate_refuses_is_not_an_attempt() {
    let env = Env::new();
    // A manual-only platform, a cooldown, a missing CV and a STOP: no form was opened.
    let linkedin = make_job_with(|j| {
        j.external_id = "1".into();
        j.apply_url = "https://www.linkedin.com/jobs/view/1".into();
    });
    env.prepared(&linkedin, 90.0);
    let no_cv = numbered(2);
    env.scored(&no_cv, 80.0);
    env.tracker
        .transition(&no_cv.key(), State::Prepared, None, *NOW)
        .unwrap();
    env.run(&FakeFiller::new(), &open_gate(), SubmitMode::Live)
        .await;
    assert_eq!(env.state("test:1"), State::NeedsGonza);
    assert_eq!(env.state("test:2"), State::NeedsGonza);
    assert!(env.results().is_empty());
    assert!(!results_path(env.dir.path()).exists());
    // And a STOP file stops everything before any line.
    env.prepared(&numbered(3), 70.0);
    std::fs::write(env.stop_file(), "x").unwrap();
    env.run(&FakeFiller::new(), &open_gate(), SubmitMode::Live)
        .await;
    assert!(env.results().is_empty());
}
#[tokio::test]
async fn lines_accumulate_across_runs() {
    let env = Env::new();
    env.prepared(&numbered(1), 90.0);
    let filler = FakeFiller::new();
    env.run(&filler, &criteria(), SubmitMode::Live).await;
    env.tracker.approve("test:1").unwrap();
    env.run(&filler, &criteria(), SubmitMode::Live).await;
    let modes: Vec<AttemptMode> = env.results().iter().map(|l| l.mode).collect();
    assert_eq!(modes, vec![AttemptMode::DryRun, AttemptMode::Live]);
}
#[tokio::test]
async fn a_handoff_application_without_a_cv_waits_for_gonza_without_a_sheet() {
    let env = Env::new();
    let job = ashby_job(1);
    env.scored(&job, 90.0);
    env.tracker
        .transition(&job.key(), State::Prepared, None, *NOW)
        .unwrap();
    let outcomes = env
        .run(&FakeFiller::new(), &open_gate(), SubmitMode::Live)
        .await;
    assert!(outcomes[0].reason.as_deref().unwrap().starts_with("sin CV"));
    assert_eq!(env.state("test:1"), State::NeedsGonza);
}
#[test]
fn a_job_in_australia_gets_the_australian_work_rights_answers() {
    use jobbot_domain::answers::AnswerEntry;
    let bank = AnswerBank {
        entries: vec![
            AnswerEntry::new(&["sponsorship&visa|employment|work|require"], Some("No")),
            AnswerEntry::new(&["=name"], Some("Jane")),
        ],
        ..AnswerBank::default()
    };
    // A label that never names Australia: only the job can say what it means.
    let label = "Will you now or in the future require visa sponsorship to work in the country this role is located?";
    let answered =
        |bank: &AnswerBank, label: &str| bank.lookup(label).and_then(|entry| entry.value.clone());
    let australia = Criteria {
        extra_open_places: vec!["australia".into(), "sydney".into()],
        ..criteria()
    };
    let in_sydney = make_job_with(|job| {
        job.location = "Sydney, Australia".into();
        job.remote = false;
    });
    let from_chile = make_job_with(|job| {
        job.location = "Remote - Latin America".into();
        job.remote = true;
    });

    let sydney_bank = bank_for_job(&bank, &in_sydney, &australia);
    assert_eq!(answered(&sydney_bank, label), Some("Yes".into()));
    assert_eq!(answered(&sydney_bank, "Name"), Some("Jane".into()));
    // A job open to Chile keeps the contractor's answers, and so does a run without extras.
    let chile_bank = bank_for_job(&bank, &from_chile, &australia);
    assert_eq!(answered(&chile_bank, label), Some("No".into()));
    let without_extras = Criteria {
        extra_open_places: vec![],
        ..criteria()
    };
    let unconfigured = bank_for_job(&bank, &in_sydney, &without_extras);
    assert_eq!(answered(&unconfigured, label), Some("No".into()));
}
