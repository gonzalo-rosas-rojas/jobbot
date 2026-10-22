//! What Gonza reads after a wrong edit of a file in `profile/`: the name of the file and the cause.
//! These pin the whole message through the public API, so they hold however the loaders are
//! organized. Text we write is compared exactly; text a dependency writes (the TOML parser, the
//! time-zone table, the time parser) is only required to be there, once, after our own words.

use std::path::PathBuf;

use jobbot::rules::{load_criteria, load_exclusions};
use jobbot::window::load_schedule;

const CRITERIA: &str = r#"
floor_monthly_usd = 1
target_monthly_usd = [2, 3]
apply_threshold = 1
daily_cap = 1
daily_cap_until_reviewed = 1
company_cooldown_days = 1
first_n_reviewed = 1

[weights]
fit = 50
compensation = 10
stability = 10
learning = 10
interview_style = 10
other = 10
"#;

fn write(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, text).unwrap();
    path
}

/// The path of the file and the message as the CLI prints it: the whole chain.
fn criteria_error(text: &str) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "criteria.toml", text);
    let message = format!("{:#}", load_criteria(&path).unwrap_err());
    (path.display().to_string(), message)
}

fn schedule_error(text: &str) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "schedule.toml", text);
    let message = format!("{:#}", load_schedule(&path).unwrap_err());
    (path.display().to_string(), message)
}

fn busy_block(zone: &str, start: &str) -> String {
    format!("[[busy]]\nzone = \"{zone}\"\nstart = \"{start}\"\nend = \"17:00\"\ndays = [\"mon\"]\n")
}

/// What follows `prefix`, which must open the message: the cause that a dependency words.
fn cause_after<'a>(message: &'a str, prefix: &str) -> &'a str {
    let cause = message
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("{message:?} does not start with {prefix:?}"));
    assert!(!cause.is_empty(), "no cause after {prefix:?}");
    cause
}

#[test]
fn a_misspelled_key_in_criteria_names_the_file_and_the_key() {
    let (path, message) = criteria_error(&format!("daily_capp = 3\n{CRITERIA}"));
    let cause = cause_after(&message, &format!("in {path}: invalid criteria: "));
    assert!(cause.starts_with("TOML parse error"), "{message}");
    assert!(cause.contains("daily_capp"), "{message}");
    // The parser's message is part of ours; a `#[source]` on top would print it a second time.
    assert_eq!(message.matches("TOML parse error").count(), 1, "{message}");
}

#[test]
fn weights_that_do_not_add_up_say_by_how_much() {
    let (path, message) = criteria_error(&CRITERIA.replace("fit = 50", "fit = 45"));
    assert_eq!(
        message,
        format!("in {path}: weights must sum to 100, got 95")
    );
}

#[test]
fn a_target_at_or_below_the_floor_says_which_field_to_fix() {
    let (path, message) = criteria_error(&CRITERIA.replace("[2, 3]", "[1, 3]"));
    assert_eq!(
        message,
        format!("in {path}: target_monthly_usd must be an ascending range above floor_monthly_usd")
    );
}

#[test]
fn an_unknown_key_in_exclusions_names_the_file_and_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "exclusions.toml", "nmes = [\"Acme\"]\n");
    let message = format!("{:#}", load_exclusions(&path).unwrap_err());
    let cause = cause_after(
        &message,
        &format!("in {}: invalid exclusions: ", path.display()),
    );
    assert!(cause.starts_with("TOML parse error"), "{message}");
    assert!(cause.contains("nmes"), "{message}");
    assert_eq!(message.matches("TOML parse error").count(), 1, "{message}");
}

#[test]
fn a_schedule_without_blocks_refuses_to_run_around_the_clock() {
    let (path, message) = schedule_error("busy = []\n");
    assert_eq!(
        message,
        format!("in {path}: schedule has no [[busy]] blocks: refusing to run around the clock")
    );
}

#[test]
fn an_unknown_time_zone_is_named() {
    let (path, message) = schedule_error(&busy_block("Mars/Olympus", "09:00"));
    cause_after(
        &message,
        &format!("in {path}: unknown time zone \"Mars/Olympus\": "),
    );
}

#[test]
fn a_time_that_is_not_hh_mm_is_quoted() {
    let (path, message) = schedule_error(&busy_block("America/Santiago", "9am"));
    // Our words, then the parser's own reason after them (the chain, not just the top message).
    cause_after(
        &message,
        &format!("in {path}: bad time \"9am\", expected HH:MM: "),
    );
}
