//! The table of guards in `SECURITY.md`, held to the repository.
//!
//! Each row of "The guards, in short" says what is guarded, which file holds the guard and which
//! test keeps it honest. This test reads the table and fails, naming the row, when a guard file is
//! gone, when a test file is gone, or when a test file no longer holds a test function of that
//! name; and when the table has fewer than twelve rows, so it cannot be "fixed" by emptying it.
//! It does not judge whether a test is good: whoever adds a row reads the guard and the test.
//!
//! The checking rules are plain functions over strings, and their own tests feed them made-up
//! tables and files, so a rule cannot quietly stop biting.

use std::path::Path;

use regex::Regex;

/// The heading of the table, as `SECURITY.md` spells it.
const HEADING: &str = "## The guards, in short";

/// Fewer rows than this and the table no longer covers what a reviewer would check first.
const MIN_ROWS: usize = 12;

/// One row of the table.
#[derive(Debug, PartialEq)]
struct Claim {
    /// The first column, as written.
    claim: String,
    /// The paths in the second column.
    guards: Vec<String>,
    /// The entries in the third column, each `path::test_name`.
    tests: Vec<String>,
}

/// The text between `HEADING` and the next second-level heading.
fn section(markdown: &str) -> Option<&str> {
    let start = markdown.find(HEADING)? + HEADING.len();
    let rest = &markdown[start..];
    Some(rest.find("\n## ").map_or(rest, |end| &rest[..end]))
}

/// What sits between backticks in a cell.
fn spans(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(|span| span.trim().to_string())
        .collect()
}

/// The rows of the table in the guards section, without its header and its separator line.
fn claims(markdown: &str) -> Vec<Claim> {
    let Some(section) = section(markdown) else {
        return Vec::new();
    };
    section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            match cells.as_slice() {
                [claim, guards, tests] => Claim {
                    claim: claim.to_string(),
                    guards: spans(guards),
                    tests: spans(tests),
                },
                _ => Claim {
                    claim: line.to_string(),
                    guards: Vec::new(),
                    tests: Vec::new(),
                },
            }
        })
        .collect()
}

/// Whether `source` holds a function `name` whose attributes include a test attribute
/// (`#[test]` or `#[tokio::test]`, with or without `#[ignore]` beside it).
fn has_test(source: &str, name: &str) -> bool {
    let signature = Regex::new(&format!(r"\bfn\s+{}\s*[(<]", regex::escape(name))).unwrap();
    let lines: Vec<&str> = source.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| signature.is_match(line))
        .any(|(at, _)| {
            lines[..at]
                .iter()
                .rev()
                .map(|line| line.trim())
                .take_while(|line| line.starts_with("#[") || line.starts_with("//"))
                .any(|line| line == "#[test]" || line.starts_with("#[tokio::test"))
        })
}

/// Everything wrong with the table: one line per fault, each starting with the row's claim.
/// `read` gives the text of a repository file, or `None` when there is no such file.
fn problems(rows: &[Claim], read: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut found = Vec::new();
    if rows.len() < MIN_ROWS {
        found.push(format!(
            "the table has {} rows; it needs at least {MIN_ROWS}",
            rows.len()
        ));
    }
    for row in rows {
        let at = &row.claim;
        if row.guards.is_empty() {
            found.push(format!("{at}: no guard path in the second column"));
        }
        if row.tests.is_empty() {
            found.push(format!("{at}: no test in the third column"));
        }
        for guard in &row.guards {
            if read(guard).is_none() {
                found.push(format!("{at}: the guard file {guard} does not exist"));
            }
        }
        for test in &row.tests {
            let Some((path, name)) = test.rsplit_once("::") else {
                found.push(format!("{at}: `{test}` is not written path::test_name"));
                continue;
            };
            match read(path) {
                None => found.push(format!("{at}: the test file {path} does not exist")),
                Some(source) if !has_test(&source, name) => {
                    found.push(format!("{at}: {path} holds no test function named {name}"))
                }
                Some(_) => {}
            }
        }
    }
    found
}

#[test]
fn every_guard_in_the_security_policy_names_a_file_and_a_test_that_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy = std::fs::read_to_string(root.join("SECURITY.md")).expect("SECURITY.md is there");
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    let found = problems(&claims(&policy.replace("\r\n", "\n")), &read);
    assert!(
        found.is_empty(),
        "SECURITY.md promises what the repository does not hold:\n{}",
        found.join("\n")
    );
}

#[cfg(test)]
mod rules {
    use super::*;

    const TABLE: &str = "\
# Security

## The guards, in short

| guard | held in | kept honest by |
|---|---|---|
| Links are https only | `src/links.rs` | `tests/links.rs::https_only` |
| Sending waits for the window | `src/window.rs`, `src/cli.rs` | `tests/links.rs::in_the_window`, `tests/links.rs::async_one` |

## Supported versions
";

    const FILES: [(&str, &str); 4] = [
        ("src/links.rs", "pub fn check() {}\n"),
        ("src/window.rs", "pub fn open() {}\n"),
        ("src/cli.rs", "pub fn run() {}\n"),
        (
            "tests/links.rs",
            "#[test]\nfn https_only() {}\n\n/// Slow.\n#[test]\n#[ignore = \"needs chromium\"]\nfn in_the_window() {}\n\n#[tokio::test(flavor = \"multi_thread\")]\nasync fn async_one() {}\n",
        ),
    ];

    fn read(path: &str) -> Option<String> {
        FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, text)| text.to_string())
    }

    fn faults(table: &str) -> Vec<String> {
        problems(&claims(table), &read)
            .into_iter()
            .filter(|fault| !fault.starts_with("the table has"))
            .collect()
    }

    #[test]
    fn a_table_read_back_gives_its_rows_with_their_paths_and_tests() {
        let rows = claims(TABLE);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].claim, "Links are https only");
        assert_eq!(rows[1].guards, ["src/window.rs", "src/cli.rs"]);
        assert_eq!(
            rows[1].tests,
            ["tests/links.rs::in_the_window", "tests/links.rs::async_one"]
        );
        assert!(faults(TABLE).is_empty(), "{:?}", faults(TABLE));
    }

    #[test]
    fn a_missing_guard_file_is_named_with_its_row() {
        let table = TABLE.replace("`src/links.rs`", "`src/gone.rs`");
        assert_eq!(
            faults(&table),
            ["Links are https only: the guard file src/gone.rs does not exist"]
        );
    }

    #[test]
    fn a_missing_test_file_is_named_with_its_row() {
        let table = TABLE.replace("tests/links.rs::https_only", "tests/gone.rs::https_only");
        assert_eq!(
            faults(&table),
            ["Links are https only: the test file tests/gone.rs does not exist"]
        );
    }

    #[test]
    fn a_test_name_the_file_does_not_hold_is_named_with_its_row() {
        let table = TABLE.replace("::https_only", "::http_too");
        assert_eq!(
            faults(&table),
            ["Links are https only: tests/links.rs holds no test function named http_too"]
        );
    }

    #[test]
    fn a_function_that_is_not_a_test_does_not_count() {
        assert!(has_test("#[test]\nfn a() {}\n", "a"));
        assert!(!has_test("fn a() {}\n", "a"));
        assert!(!has_test("#[test]\nfn b() {}\n\nfn a() {}\n", "a"));
        assert!(!has_test("#[test]\nfn ab() {}\n", "a"));
        assert!(!has_test("// fn a() {}\n#[test]\nfn b() {}\n", "a"));
        assert!(has_test("#[tokio::test]\nasync fn a() {}\n", "a"));
    }

    #[test]
    fn a_row_with_no_test_or_no_guard_is_a_fault() {
        let table = TABLE.replace("`tests/links.rs::https_only`", "nothing yet");
        assert_eq!(
            faults(&table),
            ["Links are https only: no test in the third column"]
        );
        let table = TABLE.replace("`src/links.rs`", "");
        assert_eq!(
            faults(&table),
            ["Links are https only: no guard path in the second column"]
        );
    }

    #[test]
    fn a_test_written_without_its_file_is_a_fault() {
        let table = TABLE.replace("tests/links.rs::https_only", "https_only");
        assert_eq!(
            faults(&table),
            ["Links are https only: `https_only` is not written path::test_name"]
        );
    }

    #[test]
    fn a_table_with_fewer_than_twelve_rows_fails_and_so_does_a_missing_one() {
        let found = problems(&claims(TABLE), &read);
        assert_eq!(found, ["the table has 2 rows; it needs at least 12"]);
        let found = problems(&claims("# Security\n\nNo table here.\n"), &read);
        assert_eq!(found, ["the table has 0 rows; it needs at least 12"]);
    }
}
