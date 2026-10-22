//! The files shipped in `profile/` and `cv/`, for the tests that must run against the real rules
//! (a careless edit of `profile/exclusions.toml` should break a test, not a night of applications).
//! Test-only: the domain itself never reads a file.

use std::path::PathBuf;

use crate::rules::{Criteria, Exclusions, parse_criteria, parse_exclusions};

/// The repository root: two levels above this crate's manifest.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// `profile/exclusions.toml`, parsed.
pub fn exclusions() -> Exclusions {
    parse_exclusions(&read("profile/exclusions.toml")).unwrap()
}

/// `profile/criteria.toml`, parsed and validated.
pub fn criteria() -> Criteria {
    parse_criteria(&read("profile/criteria.toml")).unwrap()
}

/// `cv/cv-master.md`, as written.
pub fn cv_master() -> String {
    read("cv/cv-master.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_files_are_found_and_parse() {
        assert!(repo_root().join("Cargo.toml").is_file());
        assert!(!exclusions().names.is_empty());
        assert_eq!(criteria().weights.total(), 100.0);
        assert!(cv_master().contains('#'));
    }
}
