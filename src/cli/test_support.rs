//! Helpers the tests of several `cli` modules share: a temporary project root, the fake sources,
//! renderer and form filler, and the call that runs one command and returns what it printed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};

use super::{Deps, run_with};
use crate::answers::AnswerBank;
use crate::claude::ClaudeRunner;
use crate::cv::Renderer;
use crate::dossier::dossier_path;
use crate::form::{FillMode, FormFiller, SubmitMode, SubmitResult, SubmitStatus};
use crate::models::{FetchResult, Job};
use crate::sources::Source;
use crate::testing::{FakeClaude, make_job_with};
use crate::tracker::{State, Tracker};

// Two instants chosen with the window tests (the same schedule file is used):
// Wednesday 2026-10-14 15:00 UTC is 12:00 in Santiago, a busy hour; 22:00 UTC is 19:00 there,
// so the execution window is open (it opens at 18:00).
pub(super) fn busy_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 14, 15, 0, 0).unwrap()
}

pub(super) fn open_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 14, 22, 0, 0).unwrap()
}

/// Copies the files of a `profile/` folder, except the developer's own overrides
/// (`claude.local.toml`): those belong to one machine and must never steer a test.
pub(super) fn copy_profile(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().ends_with(".local.toml") {
            continue;
        }
        std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

/// A temporary project root with a copy of the real `profile/` folder.
pub(super) fn temp_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_profile(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("profile"),
        &dir.path().join("profile"),
    );
    dir
}

pub(super) struct FakeSource(pub(super) Vec<Job>);

#[async_trait::async_trait]
impl Source for FakeSource {
    fn name(&self) -> &str {
        "fake"
    }
    async fn fetch(&self) -> FetchResult {
        FetchResult {
            jobs: self.0.clone(),
            errors: vec![],
        }
    }
}

pub(super) struct FakeRenderer;

#[async_trait::async_trait]
impl Renderer for FakeRenderer {
    async fn render(&self, md_text: &str, out_path: &Path) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(out_path.parent().unwrap())?;
        std::fs::write(out_path, md_text)?;
        Ok(out_path.to_path_buf())
    }
}

/// Records the `SubmitMode` of every call so tests can check what the CLI asked for.
pub(super) struct FakeFiller(pub(super) Arc<Mutex<Vec<SubmitMode>>>);

#[async_trait::async_trait]
impl FormFiller for FakeFiller {
    async fn submit(
        &self,
        _job: &Job,
        _cv: &Path,
        _bank: &AnswerBank,
        mode: FillMode,
        _shots: &Path,
    ) -> anyhow::Result<SubmitResult> {
        let mode = mode.submit_mode();
        self.0.lock().unwrap().push(mode);
        let status = if mode == SubmitMode::DryRun {
            SubmitStatus::DryRun
        } else {
            SubmitStatus::Submitted
        };
        Ok(SubmitResult {
            status,
            reason: None,
            screenshot: None,
        })
    }
}

pub(super) fn fake_deps(jobs: Vec<Job>) -> (Deps, Arc<Mutex<Vec<SubmitMode>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let deps = Deps {
        sources: Some(vec![Box::new(FakeSource(jobs))]),
        renderer: Box::new(FakeRenderer),
        filler: Some(Box::new(FakeFiller(calls.clone()))),
        inspector: None,
        open_checker: None,
        claude: None,
    };
    (deps, calls)
}

/// Runs `jobbot <parts...>` and returns the exit code and everything it printed.
pub(super) async fn cli(
    root: &Path,
    now: DateTime<Utc>,
    deps: Deps,
    parts: &[&str],
) -> (i32, String) {
    let mut args = vec!["jobbot".to_string()];
    args.extend(parts.iter().map(|p| p.to_string()));
    let mut out = Vec::new();
    let code = run_with(args, root, now, deps, &mut out).await;
    (code, String::from_utf8(out).unwrap())
}

/// Same, for commands that never need the fakes.
pub(super) async fn plain(root: &Path, now: DateTime<Utc>, parts: &[&str]) -> (i32, String) {
    cli(root, now, fake_deps(vec![]).0, parts).await
}

pub(super) fn tracker_of(root: &Path) -> Tracker {
    std::fs::create_dir_all(root.join("data")).unwrap();
    Tracker::open(&root.join("data").join("jobbot.db")).unwrap()
}

/// Puts one job in the tracker and walks it through `states`.
pub(super) fn seed(root: &Path, job: &Job, states: &[State]) {
    let t = tracker_of(root);
    t.add_job(job, 80.0, &BTreeMap::new(), open_now()).unwrap();
    t.create_application(&job.key(), open_now()).unwrap();
    for state in states {
        t.transition(&job.key(), *state, Some("falta email"), open_now())
            .unwrap();
    }
}

/// A prepared application with a CV file, ready for `submit`.
pub(super) fn seed_prepared(root: &Path, job: &Job) {
    seed(root, job, &[State::Prepared]);
    let cv = root.join("data").join("cv.pdf");
    std::fs::write(&cv, "pdf").unwrap();
    tracker_of(root).set_cv_path(&job.key(), &cv).unwrap();
}

pub(super) fn write_cv_master(root: &Path, text: &str) {
    std::fs::create_dir_all(root.join("cv")).unwrap();
    std::fs::write(root.join("cv").join("cv-master.md"), text).unwrap();
}

/// `deps` with a fake Claude whose handle the test keeps, to read the requests afterwards.
pub(super) fn with_claude(mut deps: Deps, fake: &Arc<FakeClaude>) -> Deps {
    let runner: Arc<dyn ClaudeRunner> = fake.clone();
    deps.claude = Some(runner);
    deps
}

pub(super) fn write_claude_toml(root: &Path, text: &str) {
    std::fs::write(root.join("profile").join("claude.toml"), text).unwrap();
}

pub(super) fn seed_replied(root: &Path, job: &Job) {
    seed(
        root,
        job,
        &[State::Prepared, State::Submitted, State::Replied],
    );
}

pub(super) fn dossier_file(root: &Path, key: &str) -> PathBuf {
    dossier_path(&root.join("data").join("dossiers"), key)
}

/// Writes `value` as the import file of the inbox and returns its path.
pub(super) fn write_inbox_file(root: &Path, value: &serde_json::Value) -> PathBuf {
    let file = root.join("correos.json");
    std::fs::write(&file, value.to_string()).unwrap();
    file
}

/// A valid message received two hours before `open_now()`.
pub(super) fn inbox_msg(id: &str, kind: &str) -> serde_json::Value {
    serde_json::json!({
        "message_id": id,
        "thread_id": format!("th-{id}"),
        "from": "Ana <ana@acme.example>",
        "subject": format!("Asunto {id}"),
        "received_at": "2026-10-14T20:00:00Z",
        "kind": kind,
        "summary": format!("Resumen {id}"),
    })
}

const ASHBY_APPLY: &str = "https://jobs.ashbyhq.com/acme/123/application";

pub(super) fn ashby_job() -> Job {
    make_job_with(|j| j.apply_url = ASHBY_APPLY.into())
}

/// Posting number `n` of one same company: what the per-company cap counts.
pub(super) fn acme(n: u32) -> Job {
    make_job_with(|j| j.external_id = n.to_string())
}

/// A posting that scores above the shipped threshold, so a discovery queues it.
pub(super) fn strong_job() -> Job {
    make_job_with(|j| {
        j.description = "Build LLM agents with python, typescript, react, rag and mcp. \
                         Applied AI and generative AI work on agentic workflows, with an \
                         engineering culture and open source time. \
                         Full-time position with mentorship and a take-home exercise."
            .into();
    })
}

mod tests {
    use super::copy_profile;

    #[test]
    fn the_test_projects_do_not_copy_the_developers_local_profile_files() {
        let from = tempfile::tempdir().unwrap();
        std::fs::write(from.path().join("claude.toml"), "model = \"m\"\n").unwrap();
        std::fs::write(
            from.path().join("claude.local.toml"),
            "expected_email = \"x\"\n",
        )
        .unwrap();
        let to = tempfile::tempdir().unwrap();

        copy_profile(from.path(), &to.path().join("profile"));

        let mut copied: Vec<String> = std::fs::read_dir(to.path().join("profile"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        copied.sort();
        assert_eq!(copied, vec!["claude.toml".to_string()]);
    }
}
