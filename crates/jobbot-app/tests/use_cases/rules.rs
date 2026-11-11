//! The `rules` use cases (moved from the root crate's `src/rules.rs`). What the shipped criteria
//! and exclusions must say is audited in `tests/profile_audit.rs`.

use std::path::Path;

use jobbot_app::profile::load_criteria;

#[test]
fn missing_file_reports_its_path() {
    let err = load_criteria(Path::new("no/such/criteria.toml")).unwrap_err();
    assert!(err.to_string().contains("criteria.toml"), "{err}");
}
