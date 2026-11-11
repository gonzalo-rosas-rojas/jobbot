//! The published text names no plan, phase, task or design document a reader cannot open.
//!
//! Comments and documents are written while a piece of work is planned, and it is easy to leave the
//! plan's name in them: "(plan 1b-3)", "after phase 2", "spec section 5". In a public copy those
//! names point at nothing. This test reads the code, the scripts and the documents that are
//! published, and fails, naming the file and the line, when one of those names is in them.
//!
//! The rules are a function over a line, and their own tests feed it examples, so a rule cannot
//! quietly stop biting.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

/// Folders and files that are read, relative to the repository root.
const ROOTS: [&str; 17] = [
    "ARCHITECTURE.md",
    "AI-USE.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "benches",
    "crates",
    "docs/adr",
    "docs/engineering",
    "docs/public",
    "examples",
    "fuzz",
    "scripts",
    "src",
    "tests",
    "xtask",
    "deny.toml",
    "Cargo.toml",
];

/// The kinds of file that are read: text, not fixtures or binaries.
const EXTENSIONS: [&str; 8] = ["rs", "toml", "md", "js", "mjs", "ps1", "yml", "cmd"];

/// Folders that are never read: build output, installed packages, fuzz corpora.
const SKIPPED_FOLDERS: [&str; 5] = ["target", "node_modules", "corpus", "artifacts", "fixtures"];

/// The names of work-in-progress that mean nothing to a reader of the published copy.
static PATTERNS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("a plan by number", r"(?i)\bplan\s+\d[0-9a-z-]*"),
        ("a plan by letter", r"\bPlan\s+[A-Z]\b"),
        ("a phase by number", r"(?i)\b(phase|fase)s?\s+\d"),
        ("a task by number", r"\bTask\s+\d+\b"),
        (
            "a design spec by section",
            r"(?i)\bspec(\s+section|\s+§|'s\s+ceiling|\s+[A-Z]\d+\b)",
        ),
        ("a development branch", r"backend/excelencia|design/turno"),
    ]
    .into_iter()
    .map(|(what, pattern)| (what, Regex::new(pattern).unwrap()))
    .collect()
});

/// What a line names, if it names any of the above.
fn traces(line: &str) -> Vec<&'static str> {
    PATTERNS
        .iter()
        .filter(|(_, pattern)| pattern.is_match(line))
        .map(|(what, _)| *what)
        .collect()
}

fn files(path: &Path, found: &mut Vec<PathBuf>) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    // Files named after the private profile never leave the repository and are not read here.
    if name.starts_with("profile_audit") || SKIPPED_FOLDERS.contains(&name) {
        return;
    }
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries {
            files(&entry.expect("an entry").path(), found);
        }
    } else if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXTENSIONS.contains(&ext))
    {
        found.push(path.to_path_buf());
    }
}

#[test]
fn the_published_text_names_no_plan_phase_task_or_spec() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let this_file = root.join("tests").join("no_process_names.rs");
    let mut all = Vec::new();
    for name in ROOTS {
        files(&root.join(name), &mut all);
    }
    assert!(all.len() > 100, "read only {} files", all.len());
    let mut found = Vec::new();
    for path in all.into_iter().filter(|path| *path != this_file) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let shown = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        for (at, line) in text.lines().enumerate() {
            for what in traces(line) {
                found.push(format!("{shown}:{} names {what}: {}", at + 1, line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[cfg(test)]
mod rules {
    use super::traces;

    #[test]
    fn each_kind_of_name_is_found() {
        for (line, what) in [
            ("/// Records an answer (plan 1b-3).", "a plan by number"),
            ("# plan 1a used t1-t13", "a plan by number"),
            ("// Plan B applies here", "a plan by letter"),
            ("## After phase 2", "a phase by number"),
            ("Phases 3 to 6 are done", "a phase by number"),
            ("la fase 4 no existe", "a phase by number"),
            ("shows the source name (Task 12)", "a task by number"),
            ("(spec section 5, item 3)", "a design spec by section"),
            ("the spec's ceiling for its p95", "a design spec by section"),
            ("(spec A15): never ran", "a design spec by section"),
            ("branch `backend/excelencia`", "a development branch"),
        ] {
            assert_eq!(traces(line), [what], "{line}");
        }
    }

    #[test]
    fn ordinary_words_are_not_names() {
        for line in [
            "let plan = plan_fields(&fields, &bank, cv);",
            "the plan for the controls that were there",
            "allowed_subscriptions must name at least one plan",
            "the review phase exists to look at forms the bot filled",
            "a RecordSpec and a TaskSpec",
            "Ask the task 3 times",
            "the specification of the fields",
        ] {
            assert!(traces(line).is_empty(), "{line}");
        }
    }
}
