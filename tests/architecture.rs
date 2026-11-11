//! The architecture, enforced from the manifests instead of by convention.
//!
//! The workspace is split into layers, lowest first. A crate may depend on workspace crates of a
//! strictly lower layer only: never upward and never sideways, so two adapters (the database and
//! the job boards, say) can never call each other and every path between them goes through the
//! application. The lowest layer, the domain, is pure: its external dependencies are an explicit
//! allow-list. The application above it reaches the database, HTTP and the browser only through
//! its ports. Adding `tokio` to the domain, `rusqlite` to the application, or letting the database
//! crate use the HTTP crate, fails here and not in review.
//!
//! Tests may reach one more crate than production code: the test-support crate, which wires the
//! real adapters together for them. It sits above the adapters and is never a dependency of
//! shipped code.
//!
//! The root crate on top is the composition root: its command line parses the commands and wires
//! the adapters to the use cases, and its other modules only re-export the crates below, so no
//! logic settles in the one crate that sees everything.
//!
//! The rules are plain functions over a map of manifests, so the tests below can also feed them
//! made-up workspaces and prove that each rule really bites.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Workspace crates by layer, lowest first; crates in the same layer may not depend on each other.
/// Adding a crate means adding it here, deliberately.
const LAYERS: [&[&str]; 5] = [
    &["jobbot-domain"],
    &["jobbot-app"],
    &[
        "jobbot-browser",
        "jobbot-claude",
        "jobbot-panel",
        "jobbot-sources",
        "jobbot-store",
    ],
    &["jobbot-testkit"],
    // The command line, and the repository chores (which use no workspace crate).
    &["jobbot", "xtask"],
];

/// The test-support crate: any crate's tests may use it, no crate may ship with it.
const TEST_SUPPORT: &str = "jobbot-testkit";

/// The crates that must exist today (the extraction is done one crate at a time).
const MEMBERS: [&str; 10] = [
    "jobbot",
    "jobbot-app",
    "jobbot-browser",
    "jobbot-claude",
    "jobbot-domain",
    "jobbot-panel",
    "jobbot-sources",
    "jobbot-store",
    "jobbot-testkit",
    "xtask",
];

/// The pure core: no I/O, no async runtime, no database, no browser, no HTTP.
const DOMAIN: &str = "jobbot-domain";

/// The only external crates the domain may use (any kind of dependency, dev included).
const DOMAIN_ALLOWED: [&str; 12] = [
    "chrono",
    "chrono-tz",
    "html-escape",
    // Its property tests only (a dev-dependency).
    "proptest",
    "regex",
    "scraper",
    "serde",
    "serde_json",
    "thiserror",
    "toml",
    // Pure tables (TR39 confusables): exclusion matching folds look-alike letters.
    "unicode-security",
    "url",
];

/// The application layer: the use cases and the ports they need.
const APP: &str = "jobbot-app";

/// What the application may never use itself: the database, HTTP (client or server) and the
/// browser are adapters behind its ports. Its own files (the results log, the answer sheets, the
/// drafts) it may write with the standard library.
const APP_FORBIDDEN: [&str; 4] = ["playwright-rs", "reqwest", "rusqlite", "tiny_http"];

/// The root crate's command line: it parses the commands and wires adapters to use cases. Every
/// other module of the root only re-exports the crates below it.
const ROOT_COMMAND_LINE: [&str; 2] = ["cli.rs", "main.rs"];

/// Root functions outside the command line that only wire an adapter to another crate: the panel
/// gets its SQLite stores here because the panel crate may not depend on the store crate.
const COMPOSITION: [(&str, &str); 1] = [("panel.rs", "open")];

/// A crate's dependencies: those it ships with (normal and build) and those only its tests use.
#[derive(Debug, Default)]
struct Deps {
    normal: Vec<String>,
    dev: Vec<String>,
}

impl Deps {
    fn all(&self) -> impl Iterator<Item = &String> {
        self.normal.iter().chain(&self.dev)
    }
}

/// For every workspace crate, the names of the crates it depends on.
type Manifests = BTreeMap<String, Deps>;

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
            let mut deps = Deps::default();
            for dep in package["dependencies"].as_array().expect("dependencies") {
                let dep_name = dep["name"].as_str().expect("dependency name").to_string();
                match dep["kind"].as_str() {
                    Some("dev") => deps.dev.push(dep_name),
                    _ => deps.normal.push(dep_name),
                }
            }
            (name, deps)
        })
        .collect()
}

/// Every workspace crate must sit in `layers` and may depend on strictly lower layers only. Its
/// tests may also use `test_support`, and nothing else above or beside it.
fn layer_violations(manifests: &Manifests, layers: &[&[&str]], test_support: &str) -> Vec<String> {
    let rank = |name: &str| layers.iter().position(|layer| layer.contains(&name));
    let mut found = Vec::new();
    for (name, deps) in manifests {
        let Some(mine) = rank(name) else {
            found.push(format!("{name} is not in the layer table"));
            continue;
        };
        let tests = deps.dev.iter().filter(|dep| dep.as_str() != test_support);
        for (dep, who) in deps
            .normal
            .iter()
            .map(|dep| (dep, format!("{name} depends")))
            .chain(tests.map(|dep| (dep, format!("the tests of {name} depend"))))
        {
            match rank(dep) {
                Some(theirs) if theirs == mine => found.push(format!(
                    "{who} on {dep}, which sits in the same layer ({mine}); \
                     crates of one layer talk through a lower one"
                )),
                Some(theirs) if theirs > mine => found.push(format!(
                    "{who} on {dep} (layer {theirs}) from layer {mine}; dependencies must point down"
                )),
                _ => {}
            }
        }
    }
    found
}

/// Crates that ship with `test_support` (a normal dependency, not a dev one).
fn test_support_violations(manifests: &Manifests, test_support: &str) -> Vec<String> {
    manifests
        .iter()
        .filter(|(_, deps)| deps.normal.iter().any(|dep| dep == test_support))
        .map(|(name, _)| {
            format!("{name} depends on {test_support} outside its tests: test support never ships")
        })
        .collect()
}

/// The crates in `expected` that the workspace does not have.
fn missing_members(manifests: &Manifests, expected: &[&str]) -> Vec<String> {
    expected
        .iter()
        .filter(|name| !manifests.contains_key(**name))
        .map(|name| format!("{name} is not a member of the workspace"))
        .collect()
}

/// The domain depends on nothing outside `allowed` (and on no other workspace crate).
fn domain_violations(manifests: &Manifests, domain: &str, allowed: &[&str]) -> Vec<String> {
    let Some(deps) = manifests.get(domain) else {
        return vec![format!("{domain} is not a member of the workspace")];
    };
    deps.all()
        .filter(|dep| !allowed.contains(&dep.as_str()))
        .map(|dep| format!("{domain} depends on {dep}, which is not in its allow-list"))
        .collect()
}

/// The dependencies of `krate` that are in `forbidden`.
fn forbidden_dependencies(manifests: &Manifests, krate: &str, forbidden: &[&str]) -> Vec<String> {
    let Some(deps) = manifests.get(krate) else {
        return vec![format!("{krate} is not a member of the workspace")];
    };
    deps.all()
        .filter(|dep| forbidden.contains(&dep.as_str()))
        .map(|dep| format!("{krate} depends on {dep}: the outside world is reached through a port"))
        .collect()
}

/// What a root module named `file` defines at its top level besides re-exports, module
/// declarations and its tests: functions, types, impls, constants and inline modules. The source is
/// rustfmt's, so a top-level item starts at column 0 and what is inside `mod tests` is indented.
/// `composition` lists the wiring functions a file may hold.
fn root_logic(file: &str, text: &str, composition: &[(&str, &str)]) -> Vec<String> {
    const KINDS: [&str; 10] = [
        "fn", "struct", "enum", "impl", "const", "static", "trait", "type", "union", "mod",
    ];
    let mut found = Vec::new();
    let mut after_cfg_test = false;
    for line in text.lines() {
        if line.starts_with(char::is_whitespace) || line.is_empty() || line.starts_with("//") {
            continue;
        }
        let mut rest = line;
        for prefix in ["pub(crate) ", "pub ", "async ", "unsafe "] {
            rest = rest.strip_prefix(prefix).unwrap_or(rest);
        }
        let mut words = rest.split(|c: char| !(c.is_alphanumeric() || c == '_'));
        let kind = words.next().unwrap_or_default();
        let name = words.find(|word| !word.is_empty()).unwrap_or_default();
        let test_only = std::mem::replace(&mut after_cfg_test, line == "#[cfg(test)]");
        let declaration = kind == "mod" && rest.trim_end().ends_with(';');
        let wiring = kind == "fn" && composition.contains(&(file, name));
        if KINDS.contains(&kind) && !test_only && !declaration && !wiring {
            found.push(format!(
                "{file} defines `{kind} {name}`: the root only re-exports and wires"
            ));
        }
    }
    found
}

#[test]
fn the_workspace_obeys_the_layer_rule() {
    let found = layer_violations(&manifests(), &LAYERS, TEST_SUPPORT);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn every_extracted_crate_is_a_member() {
    let found = missing_members(&manifests(), &MEMBERS);
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
            let normal = deps.iter().map(|dep| dep.to_string()).collect();
            (
                name.to_string(),
                Deps {
                    normal,
                    dev: Vec::new(),
                },
            )
        })
        .collect()
}

/// `manifests` with `dev` added as dependencies of the tests of `name`.
fn with_dev(mut manifests: Manifests, name: &str, dev: &[&str]) -> Manifests {
    let deps = manifests
        .get_mut(name)
        .expect("a crate of the made-up workspace");
    deps.dev.extend(dev.iter().map(|dep| dep.to_string()));
    manifests
}

#[test]
fn an_upward_dependency_is_reported() {
    let made_up = workspace(&[("low", &["high"]), ("high", &[])]);
    let found = layer_violations(&made_up, &[&["low"], &["high"]], "kit");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("low") && found[0].contains("high"),
        "{found:?}"
    );
}

#[test]
fn a_dependency_on_itself_is_reported() {
    let made_up = workspace(&[("low", &["low"])]);
    assert_eq!(layer_violations(&made_up, &[&["low"]], "kit").len(), 1);
}

#[test]
fn two_adapters_of_one_layer_may_not_depend_on_each_other() {
    let made_up = workspace(&[("core", &[]), ("db", &["core", "web"]), ("web", &["core"])]);
    let found = layer_violations(&made_up, &[&["core"], &["db", "web"]], "kit");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("db") && found[0].contains("web") && found[0].contains("same layer"),
        "{found:?}"
    );
}

#[test]
fn a_crate_missing_from_the_layer_table_is_reported() {
    let made_up = workspace(&[("stranger", &[])]);
    let found = layer_violations(&made_up, &[&["low"]], "kit");
    assert_eq!(found, ["stranger is not in the layer table"]);
}

#[test]
fn downward_dependencies_are_fine() {
    let made_up = workspace(&[
        ("low", &["serde"]),
        ("mid", &["low"]),
        ("side", &["low"]),
        ("high", &["low", "mid", "side", "tokio"]),
    ]);
    assert!(layer_violations(&made_up, &[&["low"], &["mid", "side"], &["high"]], "kit").is_empty());
}

#[test]
fn a_crate_that_was_not_extracted_yet_is_reported() {
    let made_up = workspace(&[("core", &[])]);
    assert_eq!(
        missing_members(&made_up, &["core", "db"]),
        ["db is not a member of the workspace"]
    );
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

#[test]
fn an_application_layer_that_opens_the_database_itself_is_reported() {
    let made_up = workspace(&[("app", &["core", "rusqlite", "serde"]), ("core", &[])]);
    let found = forbidden_dependencies(&made_up, "app", &["rusqlite", "reqwest"]);
    assert_eq!(
        found,
        ["app depends on rusqlite: the outside world is reached through a port"]
    );
}

#[test]
fn the_application_reaches_the_outside_world_only_through_its_ports() {
    let found = forbidden_dependencies(&manifests(), APP, &APP_FORBIDDEN);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn an_adapter_may_use_the_test_support_crate_in_its_tests() {
    let made_up = with_dev(
        workspace(&[
            ("core", &[]),
            ("web", &["core"]),
            ("kit", &["core", "db"]),
            ("db", &["core"]),
        ]),
        "web",
        &["kit"],
    );
    let layers: [&[&str]; 3] = [&["core"], &["db", "web"], &["kit"]];
    assert!(layer_violations(&made_up, &layers, "kit").is_empty());
}

#[test]
fn a_test_that_reaches_into_another_adapter_is_reported() {
    let made_up = with_dev(
        workspace(&[("core", &[]), ("db", &["core"]), ("web", &["core"])]),
        "web",
        &["db"],
    );
    let found = layer_violations(&made_up, &[&["core"], &["db", "web"]], "kit");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("web") && found[0].contains("db") && found[0].contains("tests"),
        "{found:?}"
    );
}

#[test]
fn production_code_that_depends_on_the_test_support_crate_is_reported() {
    let made_up = workspace(&[("core", &[]), ("kit", &["core"]), ("app", &["core", "kit"])]);
    assert_eq!(
        test_support_violations(&made_up, "kit"),
        ["app depends on kit outside its tests: test support never ships"]
    );
}

#[test]
fn nothing_ships_the_test_support_crate() {
    let found = test_support_violations(&manifests(), TEST_SUPPORT);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn logic_in_a_root_module_is_reported() {
    let source =
        "//! A facade.\n\npub use x::*;\n\npub fn decide(score: u8) -> bool {\n    score > 50\n}\n";
    assert_eq!(
        root_logic("pipeline.rs", source, &[]),
        ["pipeline.rs defines `fn decide`: the root only re-exports and wires"]
    );
}

#[test]
fn re_exports_declarations_and_tests_are_not_logic() {
    let source = "//! A facade.\n\npub use a::*;\npub use b::{\n    C,\n    D,\n};\npub mod e;\n\n\
                  #[cfg(test)]\nmod tests {\n    use super::*;\n\n    fn helper() -> u8 {\n        1\n    }\n}\n";
    assert!(root_logic("x.rs", source, &[]).is_empty());
}

#[test]
fn an_inline_module_outside_the_tests_is_reported() {
    let source = "pub mod hidden {\n    pub fn decide() {}\n}\n";
    assert_eq!(
        root_logic("x.rs", source, &[]),
        ["x.rs defines `mod hidden`: the root only re-exports and wires"]
    );
}

#[test]
fn wiring_on_the_composition_list_is_allowed_where_it_is_listed() {
    let source =
        "pub use p::*;\n\npub fn open(paths: Paths) -> Panel {\n    Panel::new(paths)\n}\n";
    let composition = [("panel.rs", "open")];
    assert!(root_logic("panel.rs", source, &composition).is_empty());
    assert_eq!(
        root_logic("other.rs", source, &composition),
        ["other.rs defines `fn open`: the root only re-exports and wires"]
    );
}

#[test]
fn the_root_crate_only_parses_commands_re_exports_and_wires() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&src).expect("src/") {
        let path = entry.expect("an entry of src/").path();
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        if !file.ends_with(".rs") || ROOT_COMMAND_LINE.contains(&file.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("a root module");
        found.extend(root_logic(&file, &text.replace("\r\n", "\n"), &COMPOSITION));
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// How `unsafe` is kept out. The workspace table sets `unsafe_code` to `deny` (or `forbid`) once;
/// each crate of the workspace takes that table with `[lints] workspace = true`, so none can
/// loosen it by omission.
fn unsafe_lint_problems(root: &toml::Table, crates: &BTreeMap<String, toml::Table>) -> Vec<String> {
    let level = root
        .get("workspace")
        .and_then(|workspace| workspace.get("lints"))
        .and_then(|lints| lints.get("rust"))
        .and_then(|rust| rust.get("unsafe_code"))
        .and_then(toml::Value::as_str);
    let mut found = Vec::new();
    if !matches!(level, Some("deny" | "forbid")) {
        found.push(format!(
            "[workspace.lints.rust] sets unsafe_code to {level:?}, not \"deny\" or \"forbid\""
        ));
    }
    for (name, manifest) in crates {
        let takes = manifest
            .get("lints")
            .and_then(|lints| lints.get("workspace"))
            .and_then(toml::Value::as_bool);
        if takes != Some(true) {
            found.push(format!(
                "{name} does not take the workspace lints (`[lints] workspace = true`)"
            ));
        }
    }
    found
}

/// The lines of `text` that allow `unsafe_code` for an item or a file.
fn unsafe_allowances(file: &str, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim_start();
            (line.starts_with("#[allow(") || line.starts_with("#![allow("))
                && line.contains("unsafe_code")
        })
        .map(|(at, _)| format!("{file}:{} allows unsafe_code", at + 1))
        .collect()
}

fn rust_files(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn unsafe_is_denied_once_and_every_crate_takes_that_setting() {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |path: &Path| -> toml::Table {
        std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .parse()
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    };
    let root = read(&root_dir.join("Cargo.toml"));
    let crates: BTreeMap<String, toml::Table> = root["workspace"]["members"]
        .as_array()
        .expect("the workspace lists its members")
        .iter()
        .map(|member| {
            let member = member.as_str().expect("a member is a path");
            (
                member.to_string(),
                read(&root_dir.join(member).join("Cargo.toml")),
            )
        })
        .collect();
    let mut found = unsafe_lint_problems(&root, &crates);
    // No shipped source file opts back in; the one test that sets an environment variable
    // (unsafe since edition 2024) lives in `tests/` and carries its own SAFETY note.
    let mut files = Vec::new();
    for dir in ["src", "benches", "xtask/src"] {
        rust_files(&root_dir.join(dir), &mut files);
    }
    for member in crates.keys().filter(|member| member.starts_with("crates/")) {
        rust_files(&root_dir.join(member).join("src"), &mut files);
    }
    for path in files {
        let text = std::fs::read_to_string(&path).expect("a source file");
        let name = path
            .strip_prefix(root_dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        found.extend(unsafe_allowances(&name, &text));
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_workspace_that_allows_unsafe_or_a_crate_that_skips_its_lints_is_reported() {
    let table = |text: &str| text.parse::<toml::Table>().unwrap();
    let denied = table("[workspace.lints.rust]\nunsafe_code = \"deny\"\n");
    let takes = BTreeMap::from([("a".to_string(), table("[lints]\nworkspace = true\n"))]);
    assert!(unsafe_lint_problems(&denied, &takes).is_empty());

    let allowed = table("[workspace.lints.rust]\nunsafe_code = \"allow\"\n");
    assert_eq!(
        unsafe_lint_problems(&allowed, &takes),
        ["[workspace.lints.rust] sets unsafe_code to Some(\"allow\"), not \"deny\" or \"forbid\""]
    );
    assert_eq!(unsafe_lint_problems(&table(""), &takes).len(), 1);

    let skips = BTreeMap::from([
        ("a".to_string(), table("[package]\nname = \"a\"\n")),
        ("b".to_string(), table("[lints]\nworkspace = false\n")),
    ]);
    assert_eq!(
        unsafe_lint_problems(&denied, &skips),
        [
            "a does not take the workspace lints (`[lints] workspace = true`)",
            "b does not take the workspace lints (`[lints] workspace = true`)"
        ]
    );
}

#[test]
fn a_source_line_that_allows_unsafe_is_reported_and_a_mention_is_not() {
    assert_eq!(
        unsafe_allowances("a.rs", "fn f() {}\n    #[allow(unsafe_code)]\nfn g() {}\n"),
        ["a.rs:2 allows unsafe_code"]
    );
    assert_eq!(
        unsafe_allowances("b.rs", "#![allow(dead_code, unsafe_code)]\n"),
        ["b.rs:1 allows unsafe_code"]
    );
    assert!(
        unsafe_allowances("c.rs", "// unsafe_code is denied\n#[allow(dead_code)]\n").is_empty()
    );
}
