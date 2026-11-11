//! The profile folder (`profile/*.toml`) read from disk: the answer bank, the criteria, the
//! exclusions, the execution window and the Claude settings. Parsing and the rules are the domain's;
//! this module only reads the files and says which one a mistake is in.

pub mod claude;

use std::path::Path;

use anyhow::{Context, Result};
use jobbot_domain::answers::{AnswerBank, parse_answers};
use jobbot_domain::rules::{Criteria, Exclusions, parse_criteria, parse_exclusions};
use jobbot_domain::window::{Schedule, parse_schedule};

/// Loads `profile/answers.toml` (see `parse_answers` for its shape).
pub fn load_answers(path: &Path) -> Result<AnswerBank> {
    let text = read(path)?;
    parse_answers(&text).with_context(|| format!("parsing {}", path.display()))
}

pub fn load_exclusions(path: &Path) -> Result<Exclusions> {
    let text = read(path)?;
    parse_exclusions(&text).with_context(|| format!("in {}", path.display()))
}

pub fn load_criteria(path: &Path) -> Result<Criteria> {
    let text = read(path)?;
    parse_criteria(&text).with_context(|| format!("in {}", path.display()))
}

pub fn load_schedule(path: &Path) -> Result<Schedule> {
    let text = read(path)?;
    parse_schedule(&text).with_context(|| format!("in {}", path.display()))
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("criteria.toml");
        let error = format!("{:#}", load_criteria(&missing).unwrap_err());
        assert!(error.starts_with("reading "), "{error}");
        assert!(error.contains("criteria.toml"), "{error}");
    }

    #[test]
    fn a_mistake_in_a_file_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schedule.toml");
        std::fs::write(&path, "busy = 3").unwrap();
        let error = format!("{:#}", load_schedule(&path).unwrap_err());
        assert!(error.starts_with("in "), "{error}");
        assert!(error.contains("schedule.toml"), "{error}");
    }
}
