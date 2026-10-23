//! The real process plumbing of `CliClaude`, exercised against `src/bin/fake_claude.rs`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use jobbot::claude::{
    ClaudeConfig, ClaudeError, ClaudeRequest, ClaudeRunner, CliClaude, PolicyWatch,
};

/// The stand-in in `mode`, working in `dir` with `dir/claude-config` as its own config folder.
fn fake(mode: &str, config: ClaudeConfig, dir: &Path) -> CliClaude {
    CliClaude::with_program(
        config,
        PathBuf::from(env!("CARGO_BIN_EXE_fake_claude")),
        vec![mode.to_string()],
        dir.to_path_buf(),
        dir.join("claude-config"),
    )
}

/// The refusal of a preflight, or a panic with whatever else came back.
fn account_message(err: ClaudeError) -> String {
    match err {
        ClaudeError::Account { message, .. } => message,
        other => panic!("{other:?}"),
    }
}

fn request(prompt: &str) -> ClaudeRequest {
    ClaudeRequest {
        stage: "test".into(),
        system: "sys".into(),
        prompt: prompt.into(),
        schema: None,
    }
}

#[tokio::test]
async fn the_prompt_goes_through_stdin_and_the_reply_is_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let prompt = "hola \"mundo\"\nlínea 2 ñ ⟦x⟧";
    let reply = fake("ok", ClaudeConfig::default(), dir.path())
        .run(&request(prompt))
        .await
        .unwrap();
    assert_eq!(reply.text, format!("echo:{prompt}"));
    assert!((reply.cost_usd - 0.01).abs() < 1e-9);
}

#[tokio::test]
async fn a_prompt_larger_than_the_pipe_buffer_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let prompt = "x".repeat(300_000);
    let reply = fake("len", ClaudeConfig::default(), dir.path())
        .run(&request(&prompt))
        .await
        .unwrap();
    assert_eq!(reply.text, "len:300000");
}

#[tokio::test]
async fn the_child_runs_in_the_work_dir_with_claude_md_loading_off() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("claude-cwd"); // does not exist yet: the runner creates it
    let reply = fake("env", ClaudeConfig::default(), &work)
        .run(&request("x"))
        .await
        .unwrap();
    let (flag, rest) = reply
        .text
        .strip_prefix("env=")
        .and_then(|rest| rest.split_once("|cwd="))
        .unwrap();
    let (cwd, _) = rest.split_once("|config_dir=").unwrap();
    assert_eq!(flag, "1");
    assert_eq!(
        std::fs::canonicalize(cwd).unwrap(),
        std::fs::canonicalize(&work).unwrap()
    );
}

// Windows only: there `set_var` is sound even with the other tests of this binary running (see
// its docs). Elsewhere the unit tests in `src/claude.rs` check the same rule via `get_envs`.
#[cfg(windows)]
#[tokio::test]
#[allow(unsafe_code)] // edition 2024: `set_var` is unsafe; the one justified use is below
async fn the_child_gets_the_own_config_folder_even_when_the_parent_has_another() {
    let dir = tempfile::tempdir().unwrap();
    let decoy = dir.path().join("someone-elses-dot-claude");
    // SAFETY: this test is built only on Windows, where std documents `set_var` as safe in a
    // multi-threaded program. The children never see the value: the runner removes the
    // variable from their environment and sets its own folder.
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &decoy) };
    let reply = fake("env", ClaudeConfig::default(), dir.path())
        .run(&request("x"))
        .await
        .unwrap();
    let (_, rest) = reply.text.split_once("|config_dir=").unwrap();
    let (config_dir, telemetry) = rest.split_once("|telemetry=").unwrap();
    assert_eq!(Path::new(config_dir), dir.path().join("claude-config"));
    assert_ne!(Path::new(config_dir), decoy);
    assert_eq!(telemetry, "1", "the child never phones home");
}

#[tokio::test]
async fn a_private_login_in_the_own_folder_passes_the_preflight_once_per_process() {
    let dir = tempfile::tempdir().unwrap();
    let claude = fake("ok", ClaudeConfig::default(), dir.path());
    let seen = claude.preflight().await.unwrap();
    assert_eq!(
        (
            seen.auth_method.as_str(),
            seen.api_provider.as_str(),
            seen.subscription.as_str(),
            seen.email.as_str(),
            seen.org_id.as_str(),
            seen.cli_version.as_deref()
        ),
        (
            "claude.ai",
            "firstParty",
            "max",
            "t***@example.com",
            "org-test",
            Some("2.1.284")
        )
    );
    assert!(
        seen.config_dir.ends_with("/claude-config"),
        "{}",
        seen.config_dir
    );
    // Two calls later, `auth status` was still asked only once.
    claude.run(&request("a")).await.unwrap();
    claude.run(&request("b")).await.unwrap();
    let calls = std::fs::read_to_string(dir.path().join("auth-status-calls")).unwrap();
    assert_eq!(calls.lines().count(), 1, "{calls:?}");
}

#[tokio::test]
async fn without_a_session_no_prompt_goes_out() {
    let dir = tempfile::tempdir().unwrap();
    let err = fake("auth-none", ClaudeConfig::default(), dir.path())
        .run(&request("x"))
        .await
        .unwrap_err();
    let message = account_message(err);
    assert!(
        message.contains("no tiene sesión") && message.contains("jobbot claude-login"),
        "{message}"
    );
    assert!(
        !dir.path().join("p-was-called").exists(),
        "the -p call must never have been made"
    );
}

#[tokio::test]
async fn an_api_key_a_team_plan_or_another_folder_are_refused() {
    for (mode, expect) in [
        ("auth-api-key", "es api_key"),
        ("auth-team", "es team"),
        ("auth-other-dir", "CLAUDE_CONFIG_DIR"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let err = fake(mode, ClaudeConfig::default(), dir.path())
            .run(&request("x"))
            .await
            .unwrap_err();
        let message = account_message(err);
        assert!(message.contains(expect), "{mode}: {message}");
        assert!(!dir.path().join("p-was-called").exists(), "{mode}");
    }
}

#[tokio::test]
async fn another_address_is_refused_and_never_printed_in_full() {
    let dir = tempfile::tempdir().unwrap();
    let config = ClaudeConfig {
        expected_email: "tester@example.com".to_string(),
        ..ClaudeConfig::default()
    };
    // The same expectation lets the right address through.
    fake("ok", config.clone(), dir.path())
        .preflight()
        .await
        .unwrap();
    let err = fake("auth-other-email", config, dir.path())
        .run(&request("x"))
        .await
        .unwrap_err();
    let message = account_message(err);
    assert!(message.contains("cuenta equivocada"), "{message}");
    assert!(
        message.contains("s***@corp.example") && !message.contains("someone.else@"),
        "{message}"
    );
}

#[tokio::test]
async fn a_managed_policy_is_a_warning_on_a_login_that_passes_and_on_one_that_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    // A folder this test made stands for C:\Program Files; the registry answers "yes" from memory.
    let program_files = dir.path().join("Program Files");
    std::fs::create_dir_all(program_files.join("ClaudeCode")).unwrap();
    let watch =
        || PolicyWatch::injected(Some(program_files.clone()), |key| key.starts_with("HKLM"));

    let claude = fake("ok", ClaudeConfig::default(), dir.path()).with_policy_watch(watch());
    let seen = claude.preflight().await.unwrap();
    assert_eq!(seen.auth_method, "claude.ai", "the verdict is untouched");
    assert_eq!(seen.warnings.len(), 2, "{:?}", seen.warnings);
    assert!(
        seen.warnings.iter().any(|w| w.contains("HKLM")),
        "{:?}",
        seen.warnings
    );
    assert!(
        seen.warnings.iter().any(|w| w.contains("ClaudeCode")),
        "{:?}",
        seen.warnings
    );

    // The same traces ride along with a refusal, where they may explain it.
    let refused = fake("auth-team", ClaudeConfig::default(), dir.path()).with_policy_watch(watch());
    match refused.preflight().await.unwrap_err() {
        ClaudeError::Account { message, seen } => {
            assert!(message.contains("es team"), "{message}");
            assert_eq!(seen.unwrap().warnings.len(), 2);
        }
        other => panic!("{other:?}"),
    }

    // Without a watch (what every test runner starts with) nothing is looked at.
    let plain = fake("ok", ClaudeConfig::default(), dir.path());
    assert!(plain.preflight().await.unwrap().warnings.is_empty());
}

#[tokio::test]
async fn login_points_the_cli_at_the_own_folder_and_creates_it() {
    let dir = tempfile::tempdir().unwrap();
    let claude = fake("ok", ClaudeConfig::default(), dir.path());
    assert!(!claude.config_dir().exists());
    claude.login().await.unwrap();
    assert!(claude.config_dir().join("fake-login.txt").is_file());
    assert!(claude.preflight().await.is_ok());
}

#[tokio::test]
async fn login_asks_for_the_subscription_and_for_the_expected_address() {
    let dir = tempfile::tempdir().unwrap();
    let config = ClaudeConfig {
        expected_email: "me@example.com".into(),
        ..ClaudeConfig::default()
    };
    let claude = fake("ok", config, dir.path());
    claude.login().await.unwrap();
    let asked = std::fs::read_to_string(claude.config_dir().join("fake-login-args.txt")).unwrap();
    assert_eq!(asked, "auth|login|--claudeai|--email|me@example.com");

    let free = tempfile::tempdir().unwrap();
    let claude = fake("ok", ClaudeConfig::default(), free.path());
    claude.login().await.unwrap();
    let asked = std::fs::read_to_string(claude.config_dir().join("fake-login-args.txt")).unwrap();
    assert_eq!(asked, "auth|login|--claudeai");
}

#[tokio::test]
async fn logout_closes_the_login_of_the_own_folder_from_the_work_folder_and_forgets_the_check() {
    let dir = tempfile::tempdir().unwrap();
    let claude = fake("ok", ClaudeConfig::default(), dir.path());
    claude.login().await.unwrap();
    assert!(claude.preflight().await.is_ok());
    assert!(claude.config_dir().join("fake-login.txt").is_file());

    claude.logout().await.unwrap();

    assert!(
        !claude.config_dir().join("fake-login.txt").exists(),
        "the credentials are gone"
    );
    // The child ran with the own folder and in the work folder, like every other child.
    let seen = std::fs::read_to_string(claude.config_dir().join("fake-logout.txt")).unwrap();
    let (cwd, config_dir) = seen.split_once('|').unwrap();
    assert!(
        same_folder(cwd, dir.path()) && same_folder(config_dir, claude.config_dir()),
        "{seen}"
    );
    // The answer of the first preflight is not reused: the next one asks again.
    let _ = claude.preflight().await;
    let calls = std::fs::read_to_string(dir.path().join("auth-status-calls")).unwrap();
    assert_eq!(calls.lines().count(), 2, "{calls}");
}

#[tokio::test]
async fn a_logout_that_fails_says_so_and_masks_the_address_it_printed() {
    let dir = tempfile::tempdir().unwrap();
    let err = fake("logout-fails", ClaudeConfig::default(), dir.path())
        .logout()
        .await
        .unwrap_err();
    match err {
        ClaudeError::Failed { code, stderr } => {
            assert_eq!(code, 1);
            assert!(stderr.contains("p***@corp.example"), "{stderr}");
            assert!(!stderr.contains("persona@"), "{stderr}");
        }
        other => panic!("{other:?}"),
    }
}

/// The same folder, however the slashes and the case were written.
fn same_folder(text: &str, path: &Path) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(text) == norm(&path.to_string_lossy())
}

#[tokio::test]
async fn the_command_line_carries_the_flags_and_never_the_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let reply = fake("args", ClaudeConfig::default(), dir.path())
        .run(&request("SECRET PROMPT"))
        .await
        .unwrap();
    assert!(
        reply.text.contains("--setting-sources|project"),
        "{}",
        reply.text
    );
    assert!(reply.text.contains("--tools||--model"), "{}", reply.text);
    assert!(!reply.text.contains("SECRET PROMPT"));
    assert!(!reply.text.contains("--bare"));
}

#[tokio::test]
async fn a_failing_child_reports_its_exit_code_and_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let err = fake("fail", ClaudeConfig::default(), dir.path())
        .run(&request("x"))
        .await
        .unwrap_err();
    match err {
        ClaudeError::Failed { code, stderr } => {
            assert_eq!(code, 1);
            assert!(stderr.contains("not logged in"), "{stderr}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_reported_error_keeps_its_cost_even_when_the_exit_code_is_not_zero() {
    let dir = tempfile::tempdir().unwrap();
    let err = fake("reported", ClaudeConfig::default(), dir.path())
        .run(&request("x"))
        .await
        .unwrap_err();
    assert!(matches!(err, ClaudeError::Reported { .. }), "{err:?}");
    assert!((err.cost_usd() - 0.02).abs() < 1e-9);
}

#[tokio::test]
async fn output_that_is_not_json_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let err = fake("garbage", ClaudeConfig::default(), dir.path())
        .run(&request("x"))
        .await
        .unwrap_err();
    assert!(matches!(err, ClaudeError::Unreadable(_)), "{err:?}");
}

#[tokio::test]
async fn a_whole_address_in_what_auth_status_prints_is_masked_before_it_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let err = fake("auth-noisy", ClaudeConfig::default(), dir.path())
        .preflight()
        .await
        .unwrap_err();
    let message = account_message(err);
    assert!(
        !message.contains("persona@corp.example"),
        "the refusal ends up in meta and in `status`: {message}"
    );
    assert!(message.contains("p***@corp.example"), "{message}");
    assert!(message.contains("no se pudo leer la sesión"), "{message}");
}

#[tokio::test]
async fn a_slow_child_is_given_up_on_at_the_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let config = ClaudeConfig {
        timeout_secs: 1, // the struct field is not validated; only the file loader is
        ..ClaudeConfig::default()
    };
    let started = Instant::now();
    let err = fake("slow", config, dir.path())
        .run(&request("x"))
        .await
        .unwrap_err();
    assert!(matches!(err, ClaudeError::Timeout(1)), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[tokio::test]
async fn a_missing_program_is_reported_as_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let missing = CliClaude::with_program(
        ClaudeConfig::default(),
        dir.path().join("no-such-claude.exe"),
        vec![],
        dir.path().to_path_buf(),
        dir.path().join("claude-config"),
    );
    let err = missing.run(&request("x")).await.unwrap_err();
    assert!(matches!(err, ClaudeError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn the_version_is_the_first_word_of_dash_dash_version() {
    let dir = tempfile::tempdir().unwrap();
    let claude = fake("version", ClaudeConfig::default(), dir.path());
    assert_eq!(claude.version().await.as_deref(), Some("2.1.284"));
}

/// Needs a logged-in `claude` on this PC and costs about US$0.004. Re-checks the facts the design
/// rests on: the prompt on stdin, the JSON shape, and that CLAUDE.md is not loaded.
/// Run with `cargo test --test claude_cli -- --ignored`.
#[tokio::test]
#[ignore = "needs a logged-in claude"]
async fn real_claude_answers_through_the_runner() {
    let dir = tempfile::tempdir().unwrap();
    let config = ClaudeConfig {
        model: "claude-haiku-4-5-20251001".to_string(),
        ..ClaudeConfig::default()
    };
    let claude = CliClaude::locate(config, dir.path().to_path_buf()).unwrap();
    let reply = claude
        .run(&ClaudeRequest {
            stage: "smoke".into(),
            system: "Answer in one word.".into(),
            prompt: "Reply with exactly the word OK and nothing else.".into(),
            schema: None,
        })
        .await
        .unwrap();
    assert_eq!(reply.text.trim(), "OK");
    assert!(
        reply.cost_usd > 0.0 && reply.cost_usd < 0.05,
        "{}",
        reply.cost_usd
    );
}
