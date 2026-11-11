//! The `claude` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/claude.rs`).

use chrono::TimeDelta;
use serde_json::Value;
use serde_json::json;

use jobbot_app::claude::*;
use jobbot_app::profile::claude::*;
use jobbot_testkit::Tracker;
use jobbot_testkit::{FakeClaude, NOW, real_paths};

fn request() -> ClaudeRequest {
    ClaudeRequest {
        stage: "test".into(),
        system: "You are terse.".into(),
        prompt: "SECRET PROMPT TEXT".into(),
        schema: None,
    }
}
#[test]
fn the_shipped_claude_toml_parses_to_the_defaults() {
    // The file as committed, not `load_claude_config`: that one also merges the developer's own
    // (git-ignored) `claude.local.toml`, which this test must not depend on.
    let text = std::fs::read_to_string(real_paths().profile_dir.join("claude.toml")).unwrap();
    assert_eq!(parse_claude_config(&text).unwrap(), ClaudeConfig::default());
}
#[test]
fn the_local_profile_files_are_kept_out_of_git() {
    // The address that `expected_email` holds must never reach a repository that may be published.
    let ignore = include_str!("../../../../.gitignore");
    assert!(
        ignore
            .lines()
            .any(|line| line.trim() == "profile/*.local.toml"),
        "add `profile/*.local.toml` to .gitignore"
    );
    let shipped = std::fs::read_to_string(real_paths().profile_dir.join("claude.toml")).unwrap();
    let committed = parse_claude_config(&shipped).unwrap();
    assert!(
        committed.expected_email.expose().is_empty() && committed.expected_org_id.is_empty(),
        "the committed claude.toml names no account: those two keys live in claude.local.toml"
    );
}
fn gate_config(daily: f64) -> ClaudeConfig {
    ClaudeConfig {
        daily_budget_usd: daily,
        ..ClaudeConfig::default()
    }
}
#[tokio::test]
async fn a_successful_call_is_recorded_with_its_cost_and_stage() {
    let tracker = Tracker::in_memory().unwrap();
    let fake = FakeClaude::new(vec![FakeClaude::structured(json!({"a": 1}), 0.04)]);
    let config = gate_config(1.0);
    let gate = ClaudeGate::new(&fake, &tracker, config.daily_budget_usd);
    let reply = gate.ask(&request(), *NOW).await.unwrap();
    assert_eq!(reply.structured, Some(json!({"a": 1})));
    let by_stage = tracker
        .claude_calls_by_stage(*NOW - TimeDelta::hours(1))
        .unwrap();
    assert_eq!(by_stage["test"].0, 1);
    assert!((by_stage["test"].1 - 0.04).abs() < 1e-9);
}
#[tokio::test]
async fn a_failed_call_is_recorded_and_a_reported_error_keeps_its_cost() {
    let tracker = Tracker::in_memory().unwrap();
    let fake = FakeClaude::new(vec![
        Err(ClaudeError::Timeout(300)),
        Err(ClaudeError::Reported {
            subtype: "error_max_turns".into(),
            message: "x".into(),
            cost_usd: 0.07,
        }),
    ]);
    let config = gate_config(1.0);
    let gate = ClaudeGate::new(&fake, &tracker, config.daily_budget_usd);
    assert!(gate.ask(&request(), *NOW).await.is_err());
    assert!(gate.ask(&request(), *NOW).await.is_err());
    let by_stage = tracker
        .claude_calls_by_stage(*NOW - TimeDelta::hours(1))
        .unwrap();
    assert_eq!(by_stage["test"].0, 2, "both failures are in the ledger");
    assert!((by_stage["test"].1 - 0.07).abs() < 1e-9);
}
#[tokio::test]
async fn a_call_that_was_cut_off_counts_as_the_most_it_could_have_cost() {
    let tracker = Tracker::in_memory().unwrap();
    let fake = FakeClaude::new(vec![
        Err(ClaudeError::Timeout(300)),
        Err(ClaudeError::Unreadable("not json".into())),
        Err(ClaudeError::Failed {
            code: 1,
            stderr: "boom".into(),
        }),
        Err(ClaudeError::Reported {
            subtype: "error_max_turns".into(),
            message: "x".into(),
            cost_usd: 0.07,
        }),
    ]);
    let config = gate_config(5.0);
    let gate = ClaudeGate::new(&fake, &tracker, config.daily_budget_usd)
        .with_per_call_cap(config.per_call_budget_usd);
    for _ in 0..4 {
        assert!(gate.ask(&request(), *NOW).await.is_err());
    }
    let spent = tracker
        .claude_spend_since(*NOW - TimeDelta::hours(1))
        .unwrap();
    // A process that exited with an error and no result is written as free: it most often never
    // reached the model, and charging the cap would spend the day's budget on a broken start.
    let expected = 2.0 * config.per_call_budget_usd + 0.07;
    assert!(
        (spent - expected).abs() < 1e-9,
        "the timeout and the unreadable result at the cap, the bare failure free, the reported cost as reported: {spent}"
    );
}
#[tokio::test]
async fn a_paid_answer_survives_a_ledger_that_cannot_be_written() {
    let tracker = Tracker::in_memory().unwrap();
    // A cost the ledger cannot store (NaN is NULL to SQLite, and the column is NOT NULL).
    let fake = FakeClaude::new(vec![FakeClaude::structured(json!({"a": 1}), f64::NAN)]);
    let gate = ClaudeGate::new(&fake, &tracker, 1.0);
    let reply = gate.ask(&request(), *NOW).await.unwrap();
    assert_eq!(reply.structured, Some(json!({"a": 1})));
}
#[tokio::test]
async fn the_daily_budget_stops_the_call_before_it_is_made() {
    let tracker = Tracker::in_memory().unwrap();
    let fake = FakeClaude::new(vec![
        FakeClaude::structured(json!({}), 0.10),
        FakeClaude::structured(json!({}), 0.10),
    ]);
    let config = gate_config(0.10);
    let gate = ClaudeGate::new(&fake, &tracker, config.daily_budget_usd);
    gate.ask(&request(), *NOW).await.unwrap(); // spends the whole budget
    let err = gate.ask(&request(), *NOW).await.unwrap_err();
    assert!(is_budget_error(&err), "{err:#}");
    assert!(err.to_string().contains("presupuesto diario"), "{err}");
    assert_eq!(
        fake.requests().len(),
        1,
        "the second call never reached Claude"
    );
}
#[tokio::test]
async fn spend_older_than_24_hours_does_not_count() {
    let tracker = Tracker::in_memory().unwrap();
    tracker
        .record_claude_call("test", *NOW - TimeDelta::hours(25), 5.0, 1, 1, true)
        .unwrap();
    let fake = FakeClaude::new(vec![FakeClaude::structured(json!({}), 0.01)]);
    let config = gate_config(1.0);
    let gate = ClaudeGate::new(&fake, &tracker, config.daily_budget_usd);
    gate.ask(&request(), *NOW).await.unwrap();
}
#[tokio::test]
async fn the_account_check_is_written_to_meta_whatever_the_verdict() {
    let tracker = Tracker::in_memory().unwrap();
    let config = gate_config(1.0);
    let fake = FakeClaude::new(vec![]);
    let gate = ClaudeGate::new(&fake, &tracker, config.daily_budget_usd);
    let seen = gate.check_account(*NOW).await.unwrap();
    assert_eq!(seen.auth_method, "claude.ai");
    assert_eq!(
        tracker
            .get_meta("claude_account_last_check")
            .unwrap()
            .as_deref(),
        Some(NOW.to_rfc3339().as_str())
    );
    let record: Value =
        serde_json::from_str(&tracker.get_meta("claude_account_last").unwrap().unwrap()).unwrap();
    assert_eq!(record["ok"], json!(true));
    assert_eq!(record["subscription"], json!("max"));
    assert_eq!(record["email"], json!("t***@example.com"));
    assert_eq!(record["message"], Value::Null);

    let refused = FakeClaude::new(vec![]).refusing_account("cuenta equivocada: prueba");
    let gate = ClaudeGate::new(&refused, &tracker, config.daily_budget_usd);
    let err = gate.check_account(*NOW).await.unwrap_err();
    assert!(is_account_error(&err), "{err:#}");
    let record: Value =
        serde_json::from_str(&tracker.get_meta("claude_account_last").unwrap().unwrap()).unwrap();
    assert_eq!(record["ok"], json!(false));
    assert_eq!(record["message"], json!("cuenta equivocada: prueba"));
    assert_eq!(
        record["auth_method"],
        json!("claude.ai"),
        "what was seen is kept"
    );
}
