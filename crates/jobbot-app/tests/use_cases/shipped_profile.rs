//! The tests that read the `profile/` folder to check the files the repository ships: that each
//! one parses, and the form it has, never a value the person wrote there. Other tests build their
//! own profile (`jobbot_testkit::profile`); the exception is the handful in `freelance` and
//! `dossier` whose input names an entry of the exclusion list that ships, which still read it
//! through `shipped_exclusions`.

use jobbot_app::freelance::load_freelance_config;
use jobbot_app::profile::claude::{ClaudeConfig, parse_claude_config};
use jobbot_app::profile::load_schedule;
use jobbot_testkit::real_paths;

#[test]
fn the_shipped_freelance_file_parses_and_is_deliberately_not_ready() {
    let shipped = load_freelance_config(&real_paths().profile_dir.join("freelance.toml")).unwrap();
    assert!(shipped.platforms.is_empty() && shipped.never_name.is_empty());
    assert!(shipped.check_ready("upwork").is_err());
}

#[test]
fn the_shipped_schedule_file_loads_and_a_missing_one_is_an_error() {
    let path = real_paths().profile_dir.join("schedule.toml");
    // Only that it loads: how many busy blocks it holds is the person's business, not the code's.
    load_schedule(&path).unwrap();
    assert!(load_schedule(&path.with_file_name("missing.toml")).is_err());
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
