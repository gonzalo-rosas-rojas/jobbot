//! The profile files that ship with the repository parse and say what the search relies on. The
//! loaders live in their crates; the files belong to the application, so the check lives here.

use std::path::Path;

use jobbot::sources::load_sources_config;

#[test]
fn shipped_sources_toml_parses() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("profile")
        .join("sources.toml");

    let cfg = load_sources_config(&path).unwrap();

    assert!(cfg.remoteok && cfg.remotive);
    assert!(cfg.remoteyeah && cfg.pythonjobs && cfg.atlassian);
    assert!(cfg.weworkremotely.is_some() && cfg.workingnomads.is_some());
    assert_eq!(
        cfg.workable_search
            .as_ref()
            .map(|search| search.location.as_str()),
        Some("Chile")
    );
    assert!(cfg.fourdayweek.is_some());
    assert!(
        cfg.greenhouse.len() >= 30,
        "the seed list is wide on purpose (audit finding A1)"
    );
}
