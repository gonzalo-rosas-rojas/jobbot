//! The `window` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/window.rs`).

use jobbot_app::profile::load_schedule;

#[test]
fn load_schedule_reads_the_profile_file() {
    let path = jobbot_testkit::real_paths()
        .root
        .join("profile")
        .join("schedule.toml");
    let s = load_schedule(&path).unwrap();
    assert_eq!(s.busy.len(), 2);
    assert!(load_schedule(&path.with_file_name("missing.toml")).is_err());
}
