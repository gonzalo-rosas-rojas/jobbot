//! Public test helpers, used by unit tests and by integration tests under `tests/`.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::claude::{AccountSeen, ClaudeError, ClaudeReply, ClaudeRequest, ClaudeRunner};
use crate::paths::Paths;

// The job builders, the fixed clock and the sample CV live in the pure domain crate; they are
// re-exported here so every existing `crate::testing::...` path keeps working.
pub use jobbot_domain::fixtures::{
    NOW, SAMPLE_BULLET_JOBS, SAMPLE_BULLET_MCP, SAMPLE_CV, make_job, make_job_with,
};

/// `Paths` rooted at the real project directory (where `profile/` lives).
pub fn real_paths() -> Paths {
    Paths::new(env!("CARGO_MANIFEST_DIR"))
}

/// Stand-in for `claude -p` in tests: answers from a queue and remembers every request. When the
/// queue is empty it fails, so a test that makes an unexpected extra call notices.
pub struct FakeClaude {
    replies: Mutex<VecDeque<Result<ClaudeReply, ClaudeError>>>,
    asked: Mutex<Vec<ClaudeRequest>>,
    /// What `preflight` answers: a private login by default, or a refusal.
    account: Result<AccountSeen, ClaudeError>,
    /// How many times the login was closed (`logout`).
    logouts: std::sync::atomic::AtomicUsize,
}

impl FakeClaude {
    pub fn new(replies: Vec<Result<ClaudeReply, ClaudeError>>) -> FakeClaude {
        FakeClaude {
            replies: Mutex::new(replies.into()),
            asked: Mutex::new(Vec::new()),
            account: Ok(FakeClaude::private_account()),
            logouts: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// How many times the login was closed.
    pub fn logouts(&self) -> usize {
        self.logouts.load(std::sync::atomic::Ordering::SeqCst)
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
        self.logouts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
    fn real_paths_point_at_the_project_root() {
        let paths = real_paths();
        assert!(paths.root.join("Cargo.toml").is_file());
        assert!(paths.profile_dir.ends_with("profile"));
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
