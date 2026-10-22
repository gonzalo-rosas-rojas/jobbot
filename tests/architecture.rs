//! The architecture, enforced from the manifests instead of by convention.
//!
//! The workspace is split into layers, lowest first. A crate may depend on workspace crates of a
//! lower layer only, and the lowest layer, the domain, is pure: its external dependencies are an
//! explicit allow-list. Adding `tokio` to the domain, or making it depend on the application,
//! fails here and not in review.
//!
//! The rules are plain functions over a map of manifests, so the tests below can also feed them
//! made-up workspaces and prove that each rule really bites.

use std::collections::BTreeMap;
use std::process::Command;

use serde_json::Value;

/// Workspace crates, lowest layer first. Adding a crate means adding it here, deliberately.
const LAYERS: [&str; 2] = ["jobbot-domain", "jobbot"];

/// The pure core: no I/O, no async runtime, no database, no browser, no HTTP.
const DOMAIN: &str = "jobbot-domain";

/// The only external crates the domain may use (any kind of dependency, dev included).
const DOMAIN_ALLOWED: [&str; 9] = [
    "chrono",
    "chrono-tz",
    "html-escape",
    "regex",
    "scraper",
    "serde",
    "thiserror",
    "toml",
    "url",
];

/// For every workspace crate, the names of the crates it depends on.
type Manifests = BTreeMap<String, Vec<String>>;

fn manifests() -> Manifests {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--locked",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata starts");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let meta: Value = serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");
    let packages = meta["packages"].as_array().expect("a list of packages");
    packages
        .iter()
        .map(|package| {
            let name = package["name"].as_str().expect("package name").to_string();
            let deps = package["dependencies"].as_array().expect("dependencies");
            let deps = deps
                .iter()
                .map(|dep| dep["name"].as_str().expect("dependency name").to_string())
                .collect();
            (name, deps)
        })
        .collect()
}

/// Every workspace crate must sit in `LAYERS` and may depend on lower layers only.
fn layer_violations(manifests: &Manifests, layers: &[&str]) -> Vec<String> {
    let rank = |name: &str| layers.iter().position(|layer| *layer == name);
    let mut found = Vec::new();
    for (name, deps) in manifests {
        let Some(mine) = rank(name) else {
            found.push(format!("{name} is not in the layer table"));
            continue;
        };
        for dep in deps {
            if let Some(theirs) = rank(dep)
                && theirs >= mine
            {
                found.push(format!(
                    "{name} (layer {mine}) depends on {dep} (layer {theirs}); dependencies must point down"
                ));
            }
        }
    }
    found
}

/// The domain depends on nothing outside `allowed` (and on no other workspace crate).
fn domain_violations(manifests: &Manifests, domain: &str, allowed: &[&str]) -> Vec<String> {
    let Some(deps) = manifests.get(domain) else {
        return vec![format!("{domain} is not a member of the workspace")];
    };
    deps.iter()
        .filter(|dep| !allowed.contains(&dep.as_str()))
        .map(|dep| format!("{domain} depends on {dep}, which is not in its allow-list"))
        .collect()
}

#[test]
fn the_workspace_obeys_the_layer_rule() {
    let found = layer_violations(&manifests(), &LAYERS);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_domain_exists_and_stays_pure() {
    let found = domain_violations(&manifests(), DOMAIN, &DOMAIN_ALLOWED);
    assert!(found.is_empty(), "{found:#?}");
}

fn workspace(crates: &[(&str, &[&str])]) -> Manifests {
    crates
        .iter()
        .map(|(name, deps)| {
            (
                name.to_string(),
                deps.iter().map(|dep| dep.to_string()).collect(),
            )
        })
        .collect()
}

#[test]
fn an_upward_dependency_is_reported() {
    let made_up = workspace(&[("low", &["high"]), ("high", &[])]);
    let found = layer_violations(&made_up, &["low", "high"]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("low") && found[0].contains("high"),
        "{found:?}"
    );
}

#[test]
fn a_dependency_on_the_same_layer_is_reported() {
    let made_up = workspace(&[("low", &["low"])]);
    assert_eq!(layer_violations(&made_up, &["low"]).len(), 1);
}

#[test]
fn a_crate_missing_from_the_layer_table_is_reported() {
    let made_up = workspace(&[("stranger", &[])]);
    let found = layer_violations(&made_up, &["low"]);
    assert_eq!(found, ["stranger is not in the layer table"]);
}

#[test]
fn downward_dependencies_are_fine() {
    let made_up = workspace(&[("low", &["serde"]), ("high", &["low", "tokio"])]);
    assert!(layer_violations(&made_up, &["low", "high"]).is_empty());
}

#[test]
fn a_domain_with_an_async_runtime_is_reported() {
    let made_up = workspace(&[("core", &["chrono", "tokio"])]);
    let found = domain_violations(&made_up, "core", &["chrono"]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("tokio"), "{found:?}");
}

#[test]
fn a_domain_that_reaches_up_into_the_application_is_reported() {
    let made_up = workspace(&[("core", &["app"]), ("app", &["core"])]);
    assert_eq!(domain_violations(&made_up, "core", &[]).len(), 1);
}

#[test]
fn a_missing_domain_is_reported() {
    let made_up = workspace(&[("app", &[])]);
    let found = domain_violations(&made_up, "core", &["chrono"]);
    assert_eq!(found, ["core is not a member of the workspace"]);
}

#[test]
fn the_domain_may_use_exactly_its_allow_list() {
    let made_up = workspace(&[("core", &["chrono", "url"])]);
    assert!(domain_violations(&made_up, "core", &["chrono", "url", "toml"]).is_empty());
}
