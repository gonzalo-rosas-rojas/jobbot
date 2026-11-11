//! What an export must not carry: the private terms of the terms file, secrets in the shapes the
//! common providers give them, and any e-mail address outside the example domains. Every text file
//! is read line by line, and every path is checked as a line of its own (line 0).

use std::fs;
use std::io;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::terms::Terms;

/// What kind of rule a finding broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Term,
    Secret,
    Email,
    /// A binary file of a kind not on `BINARY_KINDS`: its content cannot be scanned.
    Binary,
}

/// One match: the file (from the export's root, with `/`), the line (0 for the path itself) and
/// the text that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    pub line: usize,
    pub kind: Kind,
    pub matched: String,
}

/// Keys and tokens in the shapes their providers issue them, and private key blocks.
static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"sk-[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}",
        r"|AKIA[0-9A-Z]{16}|xox[abposr]-[A-Za-z0-9-]{10,}",
        r"|-----BEGIN (RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY",
        r#"|(?i:api[_-]?key|secret|token)\s*[:=]\s*["'][A-Za-z0-9/+_-]{16,}["']"#,
    ))
    .expect("the secret pattern compiles")
});

static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
        .expect("the e-mail pattern compiles")
});

/// The domains an address in public text may use: the reserved example domains (RFC 2606) and
/// GitHub's no-reply addresses.
const EXAMPLE_DOMAINS: [&str; 4] = [
    "example.com",
    "example.org",
    "example.net",
    "users.noreply.github.com",
];

/// The top-level names reserved for examples and tests (RFC 2606, RFC 6761): no real mailbox lives
/// under them.
const RESERVED_TLDS: [&str; 4] = [".example", ".invalid", ".localhost", ".test"];

/// True for an address at an example domain, a subdomain of one, or under a reserved name.
fn is_example_address(address: &str) -> bool {
    let Some((_, domain)) = address.rsplit_once('@') else {
        return false;
    };
    let domain = domain.to_lowercase();
    RESERVED_TLDS.iter().any(|tld| domain.ends_with(tld))
        || EXAMPLE_DOMAINS
            .iter()
            .any(|example| domain == *example || domain.ends_with(&format!(".{example}")))
}

/// The findings in one file's text; `path` is the file's path from the export's root.
pub fn scan_text(path: &str, text: &str, terms: &Terms) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut found = |line: usize, kind: Kind, matched: &str| {
        findings.push(Finding {
            path: path.to_string(),
            line,
            kind,
            matched: matched.to_string(),
        });
    };
    let lines = std::iter::once((0, path)).chain(text.lines().enumerate().map(|(i, l)| (i + 1, l)));
    for (number, line) in lines {
        // The address the copy is published at is public by definition: a term inside it is not a
        // finding (the same term anywhere else on the line still is).
        let without_homes = terms.without_public_homes(line);
        for rule in &terms.rules {
            if rule.except.iter().any(|file| file == path) {
                continue;
            }
            for matched in rule.pattern.find_iter(&without_homes) {
                if !terms.is_allowed(matched.as_str()) {
                    found(number, Kind::Term, matched.as_str());
                }
            }
        }
        for matched in SECRET.find_iter(line) {
            found(number, Kind::Secret, matched.as_str());
        }
        for matched in EMAIL.find_iter(line) {
            if !is_example_address(matched.as_str()) && !terms.is_allowed(matched.as_str()) {
                found(number, Kind::Email, matched.as_str());
            }
        }
    }
    findings
}

/// The binary files a copy may carry, by extension: images and fonts.
const BINARY_KINDS: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "webp", "ico", "woff", "woff2", "ttf", "otf",
];

/// The text of a UTF-16 file that starts with its byte-order mark (PowerShell and Notepad write
/// them); `None` for anything else.
fn utf16_text(bytes: &[u8]) -> Option<String> {
    let (little_endian, rest) = match bytes {
        [0xFF, 0xFE, rest @ ..] => (true, rest),
        [0xFE, 0xFF, rest @ ..] => (false, rest),
        _ => return None,
    };
    let units: Vec<u16> = rest
        .chunks_exact(2)
        .map(|pair| {
            let pair = [pair[0], pair[1]];
            if little_endian {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        })
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// The findings in every file under `root` (`.git` skipped). UTF-16 text is read as text. Any
/// other file with a NUL byte is binary: its path is checked, and a binary of a kind not on
/// `BINARY_KINDS` is a finding of its own, because its content cannot be checked.
pub fn scan_tree(root: &Path, terms: &Terms) -> io::Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                if entry.file_name() != ".git" {
                    folders.push(path);
                }
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&path)?;
            if let Some(text) = utf16_text(&bytes) {
                findings.extend(scan_text(&relative, &text, terms));
            } else if bytes.contains(&0) {
                findings.extend(scan_text(&relative, "", terms));
                let listed = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        BINARY_KINDS.contains(&extension.to_ascii_lowercase().as_str())
                    });
                if !listed {
                    findings.push(Finding {
                        path: relative.clone(),
                        line: 0,
                        kind: Kind::Binary,
                        matched: "a binary file of an unlisted kind".into(),
                    });
                }
            } else {
                findings.extend(scan_text(
                    &relative,
                    &String::from_utf8_lossy(&bytes),
                    terms,
                ));
            }
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(text: &str) -> Terms {
        Terms::parse(text).unwrap()
    }

    fn kinds(findings: &[Finding]) -> Vec<(usize, Kind, String)> {
        findings
            .iter()
            .map(|finding| (finding.line, finding.kind, finding.matched.clone()))
            .collect()
    }

    #[test]
    fn a_term_is_found_on_its_line_whatever_its_case() {
        let found = scan_text(
            "src/a.rs",
            "one\nThe Secret Name is here\n",
            &terms("secret name"),
        );
        assert_eq!(kinds(&found), vec![(2, Kind::Term, "Secret Name".into())]);
        assert_eq!(found[0].path, "src/a.rs");
    }

    #[test]
    fn a_term_in_a_path_is_found_on_line_zero() {
        let found = scan_text("docs/secret-name.md", "nothing\n", &terms("secret-name"));
        assert_eq!(kinds(&found), vec![(0, Kind::Term, "secret-name".into())]);
    }

    #[test]
    fn an_excepted_file_may_hold_its_term_and_an_allowed_text_is_not_a_finding() {
        let rules = terms("except LICENSE: jane roe\n\\+00 \\d{4}\nallow: +00 1234\n");
        assert!(scan_text("LICENSE", "Copyright Jane Roe\n", &rules).is_empty());
        assert_eq!(scan_text("README.md", "by Jane Roe\n", &rules).len(), 1);
        assert!(scan_text("src/a.rs", "call +00 1234\n", &rules).is_empty());
        assert_eq!(scan_text("src/a.rs", "call +00 9876\n", &rules).len(), 1);
    }

    #[test]
    fn a_term_inside_the_address_the_copy_is_published_at_is_not_a_finding() {
        let plain = terms(
            "jane|doe
",
        );
        let mut published = plain.clone();
        published.allow_public_home("https://github.com/jane-doe/");
        published.allow_public_home("https://jane-doe.github.io/");
        let text = "see https://github.com/Jane-Doe/tool and https://jane-doe.github.io/tool-site/
                    written by Jane, at https://github.com/jane-doe/tool
                    https://github.com/jane-doe-other/tool
";
        assert_eq!(
            kinds(&scan_text("docs/a.md", text, &published)),
            vec![
                (2, Kind::Term, "Jane".to_string()),
                (3, Kind::Term, "jane".to_string()),
                (3, Kind::Term, "doe".to_string()),
            ],
            "only the exact published address is let through: the bare name and a look-alike              account are still findings"
        );
        // Without the option nothing changes.
        assert_eq!(scan_text("docs/a.md", text, &plain).len(), 9);
    }

    #[test]
    fn secrets_are_found_in_their_provider_shapes() {
        // Built at run time, so this file never holds a secret-shaped string itself.
        let token = format!("gh{}_{}", "p", "a1".repeat(12));
        let key = format!("-----BEGIN {}PRIVATE KEY-----", "RSA ");
        let aws = format!("AKIA{}", "ABCDEFGHIJKLMNOP");
        let assigned = format!("api_key = \"{}\"", "Zx9".repeat(6));
        for text in [token, key, aws, assigned] {
            let found = scan_text("src/a.rs", &text, &terms("unrelated"));
            assert_eq!(found.len(), 1, "{text}");
            assert_eq!(found[0].kind, Kind::Secret);
        }
        assert!(
            scan_text(
                "src/a.rs",
                "let key = read_key(path);\n",
                &terms("unrelated")
            )
            .is_empty()
        );
    }

    #[test]
    fn only_addresses_at_example_domains_pass() {
        let rules = terms("unrelated");
        for ok in [
            "jane@example.com",
            "hr@jobs.example.org",
            "a@b.example",
            "x@mail.invalid",
            "x@sub.test",
            "123+jane@users.noreply.github.com",
        ] {
            assert!(scan_text("src/a.rs", ok, &rules).is_empty(), "{ok}");
        }
        // Built at run time, so this file holds no address outside the example domains itself.
        for bad in [
            format!("jane@{}", "mail.org"),
            format!("hr@example.com.{}", "evil.org"),
            format!("x@not{}", "example.com"),
            format!("x@{}", "testing.org"),
        ] {
            let found = scan_text("src/a.rs", &bad, &rules);
            assert_eq!(kinds(&found), vec![(1, Kind::Email, bad.clone())], "{bad}");
        }
        let other = format!("other@{}", "mail.org");
        let allowed = terms(&format!("unrelated\nallow: {other}\n"));
        assert!(scan_text("src/a.rs", &other, &allowed).is_empty());
    }

    #[test]
    fn a_utf16_file_is_read_as_text_and_an_unlisted_binary_is_a_finding() {
        let dir = tempfile::tempdir().unwrap();
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in "fine\nsecret name\n".encode_utf16() {
            utf16.extend(unit.to_le_bytes());
        }
        fs::write(dir.path().join("notes.txt"), utf16).unwrap();
        fs::write(dir.path().join("cache.db"), b"SQLite\0rows").unwrap();
        fs::write(dir.path().join("font.woff2"), b"wOF2\0glyphs").unwrap();
        let mut found = scan_tree(dir.path(), &terms("secret name")).unwrap();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        let found: Vec<(&str, usize, Kind)> = found
            .iter()
            .map(|f| (f.path.as_str(), f.line, f.kind))
            .collect();
        assert_eq!(
            found,
            vec![("cache.db", 0, Kind::Binary), ("notes.txt", 2, Kind::Term)]
        );
    }

    #[test]
    fn a_tree_is_scanned_file_by_file_and_binary_files_by_path_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "fine\nsecret name\n").unwrap();
        fs::write(dir.path().join("logo.png"), b"\x89PNG\0secret name").unwrap();
        fs::write(dir.path().join(".git/config"), "secret name").unwrap();
        let mut found = scan_tree(dir.path(), &terms("secret name")).unwrap();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].path.as_str(), found[0].line), ("src/a.rs", 2));
    }
}
