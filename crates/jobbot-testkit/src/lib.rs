//! jobbot's test support, for the tests of every crate above the domain and for the integration
//! tests under `tests/`: the shared fixtures (re-exported from the domain), a fake Claude, made-up
//! profile files (`profile`), and the real SQLite stores to open in memory. It is a
//! dev-dependency only; `tests/architecture.rs` keeps it out of anything that ships.

pub mod bench;
pub mod fuzz;
pub mod hostile;
pub mod profile;
pub mod spans;
pub mod synthetic;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};

use jobbot_app::paths::Paths;
use jobbot_app::ports::{AccountSeen, ClaudeError, ClaudeReply, ClaudeRequest, ClaudeRunner};

// The job builders, the fixed clock and the sample CV live in the pure domain crate.
pub use jobbot_domain::fixtures::{
    NOW, SAMPLE_BULLET_JOBS, SAMPLE_BULLET_MCP, SAMPLE_CV, make_job, make_job_with,
};

// The real stores, for tests that need SQLite behind the ports (open them with `in_memory`).
pub use jobbot_store::prep::PrepStore;
pub use jobbot_store::tracker::Tracker;

/// The UTC instant `year-month-day hour:minute:second` (no fraction; add a `TimeDelta` for one).
/// The one place the date tests of the crates that depend on this one build their input instants,
/// so a change of date library touches this function and not each test. Panics on a date that
/// does not exist.
pub fn utc_at(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("a UTC date and time that exists")
}

/// `Paths` rooted at `root` whose `profile/` is an empty folder that this call creates, so a test
/// writes into it the profile files it needs (and states them) instead of reading the user's own.
pub fn paths_with_empty_profile(root: &Path) -> Paths {
    let mut paths = Paths::new(root);
    paths.profile_dir = root.join("profile-under-test");
    std::fs::create_dir_all(&paths.profile_dir).expect("an empty profile folder under the root");
    paths
}

/// `Paths` rooted at the real project directory (where `profile/` lives): the workspace root, two
/// folders above this crate. Only for the tests that exist to check the files the repository
/// ships; a new test should build its own profile with `paths_with_empty_profile`.
pub fn real_paths() -> Paths {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    Paths::new(
        crate_dir
            .ancestors()
            .nth(2)
            .expect("crates/jobbot-testkit sits two levels down"),
    )
}

/// Stand-in for `claude -p` in tests: answers from a queue and remembers every request. When the
/// queue is empty it fails, so a test that makes an unexpected extra call notices.
pub struct FakeClaude {
    replies: Mutex<VecDeque<Result<ClaudeReply, ClaudeError>>>,
    asked: Mutex<Vec<ClaudeRequest>>,
    /// What `preflight` answers: a private login by default, or a refusal.
    account: Result<AccountSeen, ClaudeError>,
    /// How many times the login was closed (`logout`).
    logouts: AtomicUsize,
}

impl FakeClaude {
    pub fn new(replies: Vec<Result<ClaudeReply, ClaudeError>>) -> FakeClaude {
        FakeClaude {
            replies: Mutex::new(replies.into()),
            asked: Mutex::new(Vec::new()),
            account: Ok(FakeClaude::private_account()),
            logouts: AtomicUsize::new(0),
        }
    }

    /// How many times the login was closed.
    pub fn logouts(&self) -> usize {
        self.logouts.load(Ordering::SeqCst)
    }

    /// The same fake whose preflight refuses the login with `message` (so no call may follow).
    pub fn refusing_account(mut self, message: &str) -> FakeClaude {
        self.account = Err(ClaudeError::Account {
            message: message.to_string(),
            seen: Some(Box::new(FakeClaude::private_account())),
        });
        self
    }

    /// What a private `max` login looks like to the preflight (the address is made up).
    pub fn private_account() -> AccountSeen {
        AccountSeen {
            auth_method: "claude.ai".into(),
            api_provider: "firstParty".into(),
            subscription: "max".into(),
            email: "t***@example.com".into(),
            org_id: "org-test".into(),
            config_dir: "C:/fake/jobbot/claude-config".into(),
            cli_version: Some("2.1.284".into()),
            warnings: Vec::new(),
        }
    }

    /// The same fake whose private login passes with `warning` attached (a managed policy found).
    pub fn warning_about(mut self, warning: &str) -> FakeClaude {
        let mut seen = FakeClaude::private_account();
        seen.warnings.push(warning.to_string());
        self.account = Ok(seen);
        self
    }

    /// A reply with `value` as its structured output, costing `cost` dollars.
    pub fn structured(value: serde_json::Value, cost: f64) -> Result<ClaudeReply, ClaudeError> {
        Ok(ClaudeReply {
            text: value.to_string(),
            structured: Some(value),
            cost_usd: cost,
            input_tokens: 100,
            output_tokens: 50,
            turns: 2,
        })
    }

    /// A plain-text reply costing `cost` dollars.
    pub fn text(text: &str, cost: f64) -> Result<ClaudeReply, ClaudeError> {
        Ok(ClaudeReply {
            text: text.to_string(),
            structured: None,
            cost_usd: cost,
            input_tokens: 100,
            output_tokens: 5,
            turns: 1,
        })
    }

    pub fn requests(&self) -> Vec<ClaudeRequest> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait]
impl ClaudeRunner for FakeClaude {
    async fn run(&self, req: &ClaudeRequest) -> Result<ClaudeReply, ClaudeError> {
        self.asked.lock().unwrap().push(req.clone());
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(ClaudeError::Io("FakeClaude: no reply queued".to_string())))
    }

    async fn preflight(&self) -> Result<AccountSeen, ClaudeError> {
        self.account.clone()
    }

    async fn logout(&self) -> Result<(), ClaudeError> {
        self.logouts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// What Claude would answer for the dossier of `SAMPLE_CV`: two stories whose evidence is a real
/// CV line (one with a bullet mark in front), one invented story, one empty question, and two
/// gaps (one of them without a topic).
pub fn sample_dossier_json() -> serde_json::Value {
    serde_json::json!({
        "company_summary": "Acme builds AI agents for support teams.",
        "likely_questions": [
            {"question": "How would you design a tool-calling agent?", "why": "The posting asks for agents."},
            {"question": "", "why": "an empty question is dropped"}
        ],
        "stories": [
            {"title": "MCP server", "evidence": SAMPLE_BULLET_MCP, "story": "I built it end to end."},
            {"title": "Invented", "evidence": "Led a team of forty engineers shipping Kubernetes operators at Google", "story": "Never happened."},
            {"title": "Job system", "evidence": format!("• {SAMPLE_BULLET_JOBS}"), "story": "Queues with retries."}
        ],
        "gaps": [
            {"topic": "Kubernetes Operators", "question": "What is a reconcile loop?", "answer": "A control loop that drives the actual state to the desired one."},
            {"topic": "", "question": "A gap without a topic still becomes a card", "answer": "Under the topic gap."}
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_at_is_the_utc_instant_of_the_fields() {
        // 2026-03-01T12:00:00Z is 1_772_366_400 seconds after 1970-01-01T00:00:00Z.
        assert_eq!(utc_at(2026, 3, 1, 12, 0, 0).timestamp(), 1_772_366_400);
        assert_eq!(utc_at(2026, 3, 1, 12, 0, 1).timestamp(), 1_772_366_401);
        assert_eq!(utc_at(1970, 1, 1, 0, 0, 0).timestamp(), 0);
    }

    #[test]
    fn real_paths_point_at_the_project_root() {
        let paths = real_paths();
        assert!(paths.root.join("Cargo.toml").is_file());
        assert!(
            paths.profile_dir.join("answers.toml").is_file(),
            "{paths:?}"
        );
    }

    #[tokio::test]
    async fn fake_claude_answers_in_order_and_remembers_requests() {
        let fake = FakeClaude::new(vec![
            FakeClaude::text("first", 0.01),
            FakeClaude::text("second", 0.02),
        ]);
        let req = |prompt: &str| ClaudeRequest {
            stage: "t".into(),
            system: "s".into(),
            prompt: prompt.into(),
            schema: None,
        };
        assert_eq!(fake.run(&req("a")).await.unwrap().text, "first");
        assert_eq!(fake.run(&req("b")).await.unwrap().text, "second");
        assert_eq!(fake.requests().len(), 2);
        assert_eq!(fake.requests()[1].prompt, "b");
        assert!(matches!(fake.run(&req("c")).await, Err(ClaudeError::Io(_))));
    }

    #[tokio::test]
    async fn fake_claude_passes_the_preflight_unless_told_to_refuse() {
        let fake = FakeClaude::new(vec![]);
        assert_eq!(fake.preflight().await.unwrap().subscription, "max");
        let refused = FakeClaude::new(vec![]).refusing_account("sin sesión");
        let err = refused.preflight().await.unwrap_err();
        assert!(matches!(err, ClaudeError::Account { .. }), "{err:?}");
        assert_eq!(err.to_string(), "sin sesión");
    }
}
