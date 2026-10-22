//! What may leave this machine toward Claude, and how what comes back is cleaned. Everything the
//! program sends goes through `redact_contact` (and, for freelance, `strip_excluded_entries`);
//! everything it receives is run through `clean` before it is stored, and the file names made
//! from outside text come from `slug`.

use std::sync::LazyLock;

use regex::Regex;

use crate::exclusion::check_exclusion;
use crate::models::Job;
use crate::rules::Exclusions;

static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\w.+-]+@[\w-]+(\.[\w-]+)+").expect("valid regex"));
static PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\+?\d[\d\s().-]{7,}\d").expect("valid regex"));

/// A phone number has at least 9 digits; a date range like `2017-2020` has 8.
fn has_phone_number(line: &str) -> bool {
    PHONE
        .find_iter(line)
        .any(|m| m.as_str().chars().filter(|c| c.is_ascii_digit()).count() >= 9)
}

/// The CV without any line that holds an email address or a phone number (the contact line).
pub fn redact_contact(cv_md: &str) -> String {
    let text = cv_md.replace("\r\n", "\n");
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !EMAIL.is_match(line) && !has_phone_number(line))
        .collect();
    kept.join("\n") + "\n"
}

/// The CV without the blank-line-separated blocks that hit the exclusion list: the entry of the
/// current employer (an excluded name) or anything about an excluded category. Freelance prompts
/// are built from this text, so that employer never reaches a freelance profile.
pub fn strip_excluded_entries(cv_md: &str, exclusions: &Exclusions) -> String {
    let text = cv_md.replace("\r\n", "\n");
    let kept: Vec<String> = entry_blocks(&text)
        .into_iter()
        .map(|block| block.join("\n"))
        .filter(|block| {
            let probe = Job {
                description: block.clone(),
                ..Job::default()
            };
            !check_exclusion(&probe, exclusions).excluded
        })
        .collect();
    kept.join("\n\n")
}

/// A line of only spaces is blank: hand-edited CVs have them.
fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// A list item (`• x`, `- x`, `* x`), not a `**bold**` title.
fn is_bullet(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with('•') || line.starts_with("- ") || line.starts_with("* ")
}

/// The CV cut into entries: runs of non-blank lines. A block made only of bullets belongs to the
/// block above it (its title and company line), so a stray blank line inside an entry cannot leave
/// the bullets of an excluded entry behind.
fn entry_blocks(text: &str) -> Vec<Vec<&str>> {
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if is_blank(line) {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    let mut merged: Vec<Vec<&str>> = Vec::new();
    for block in blocks {
        if block.iter().all(|line| is_bullet(line))
            && let Some(previous) = merged.last_mut()
        {
            previous.extend(block);
        } else {
            merged.push(block);
        }
    }
    merged
}

/// Lowercase with every run of whitespace collapsed to one space.
pub fn squash(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A quoted CV line counts as evidence only when it really is in the CV (case, spacing and the
/// bullet mark ignored) and is long enough to be a specific claim, not a keyword.
pub fn evidence_in_cv(evidence: &str, cv_md: &str) -> bool {
    let wanted = squash(evidence.trim().trim_start_matches(['•', '-', '*']).trim());
    wanted.chars().count() >= 40 && squash(cv_md).contains(&wanted)
}

/// A file-name-safe version of `text`: lowercase ASCII letters and digits separated by single
/// dashes, at most 80 characters, never empty.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_was_dash = false;
        } else if !last_was_dash && !out.is_empty() {
            out.push('-');
            last_was_dash = true;
        }
    }
    let capped: String = out.chars().take(80).collect();
    let capped = capped.trim_end_matches('-');
    if capped.is_empty() {
        "item".to_string()
    } else {
        capped.to_string()
    }
}

/// Format characters that take no space but can hide text from a reader or a model: zero-width
/// characters, bidirectional controls and isolates, the byte-order mark, the soft hyphen and the
/// "tag" block used to smuggle ASCII. Not the whole Unicode `Cf` category, only these.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}'
        | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}'
        | '\u{E0001}' | '\u{E0020}'..='\u{E007F}')
}

/// `text` without the characters of `is_invisible`.
pub fn strip_invisible(text: &str) -> String {
    text.chars().filter(|c| !is_invisible(*c)).collect()
}

/// Text from a web page made safe to store and print on one line: invisible format characters
/// removed, every control character (escape sequences, bells, newlines) turned into a space, runs of
/// whitespace collapsed, trimmed.
pub fn plain_line(text: &str) -> String {
    let spaced: String = strip_invisible(text)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text from Claude made safe to store: control characters other than newlines and invisible
/// format characters removed, trimmed, and cut to `max_chars` characters (with an ellipsis when cut).
pub fn clean(text: &str, max_chars: usize) -> String {
    let kept: String = text
        .chars()
        .filter(|c| (!c.is_control() || *c == '\n') && !is_invisible(*c))
        .collect();
    let kept = kept.trim();
    if kept.chars().count() <= max_chars {
        kept.to_string()
    } else {
        let cut: String = kept.chars().take(max_chars).collect();
        format!("{}…", cut.trim_end())
    }
}

/// FNV-1a over the bytes of `text`. Hand-written because the result becomes part of file names and
/// card ids stored on disk, so it must never change with a Rust or crate version.
fn fnv1a32(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// A file-name-safe name for a job key that stays unique: the readable `slug` plus eight hex
/// characters of the hash of the whole key. `slug` alone maps `a/b` and `a-b` to the same text
/// and cuts long keys at 80 characters, so two jobs could otherwise share one dossier.
pub fn key_slug(key: &str) -> String {
    format!("{}-{:08x}", slug(key), fnv1a32(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exclusion::check_exclusion;
    use crate::fixtures::SAMPLE_CV;
    use crate::models::Job;
    use crate::shipped;

    fn exclusions() -> Exclusions {
        shipped::exclusions()
    }

    #[test]
    fn the_contact_line_is_removed_and_the_rest_stays() {
        let out = redact_contact(SAMPLE_CV);
        assert!(!out.contains('@') && !out.contains("+56"), "{out}");
        assert!(out.contains("## PROFESSIONAL EXPERIENCE") && out.contains("stackwave.io"));
        assert_eq!(out.lines().count(), SAMPLE_CV.lines().count() - 1);
    }

    #[test]
    fn a_date_range_with_hyphens_is_not_a_phone_number() {
        let cv = "• Worked 2017-2020 on things\n+1 (555) 123-4567\nwrite me at x@y.com\n• Kept 100 ops/sec";
        let out = redact_contact(cv);
        assert!(
            out.contains("2017-2020") && out.contains("100 ops/sec"),
            "{out}"
        );
        assert!(!out.contains("555") && !out.contains("x@y.com"), "{out}");
    }

    #[test]
    fn windows_line_endings_are_handled() {
        let crlf = SAMPLE_CV.replace('\n', "\r\n");
        assert_eq!(redact_contact(&crlf), redact_contact(SAMPLE_CV));
        let stripped = strip_excluded_entries(&crlf, &exclusions());
        assert!(!stripped.contains("Harbor Light"), "{stripped}");
    }

    #[test]
    fn the_current_employer_entry_is_removed_with_its_title_and_bullets() {
        let out = strip_excluded_entries(SAMPLE_CV, &exclusions());
        assert!(!out.contains("Harbor Light"), "{out}");
        assert!(!out.contains("Fractional Operations Contractor"), "{out}");
        assert!(!out.contains("multi-office logistics operation"), "{out}");
        for kept in [
            "stackwave.io",
            "## KEY PROJECTS",
            "## TECHNICAL SKILLS",
            "## PROFESSIONAL EXPERIENCE",
        ] {
            assert!(out.contains(kept), "{kept} was lost: {out}");
        }
    }

    #[test]
    fn the_real_cv_master_is_clean_after_both_filters() {
        // A property of the real file, so it keeps holding when Gonza edits the CV.
        let cv = shipped::cv_master();
        let out = strip_excluded_entries(&redact_contact(&cv), &exclusions());
        assert!(!out.contains('@'), "an email address is left in the text");
        let probe = Job {
            description: out.clone(),
            ..Job::default()
        };
        assert!(
            !check_exclusion(&probe, &exclusions()).excluded,
            "the exclusion list still hits"
        );
    }

    #[test]
    fn a_blank_line_inside_the_employer_entry_does_not_leave_its_bullets_behind() {
        // Hand-edited CVs drift: here a blank line separates the company line from its bullet.
        let cv = SAMPLE_CV.replace("Jun 2026 – Present\n•", "Jun 2026 – Present\n\n•");
        assert_ne!(
            cv, SAMPLE_CV,
            "the fixture must really contain the blank line"
        );
        let out = strip_excluded_entries(&cv, &exclusions());
        assert!(!out.contains("Harbor Light"), "{out}");
        assert!(!out.contains("multi-office logistics operation"), "{out}");
        assert!(
            out.contains("stackwave.io") && out.contains("Local LLM Lab"),
            "{out}"
        );
    }

    #[test]
    fn a_blank_line_with_spaces_in_it_still_separates_entries() {
        let cv = SAMPLE_CV.replace("\n\n", "\n \n");
        let out = strip_excluded_entries(&cv, &exclusions());
        assert!(!out.contains("Harbor Light"), "{out}");
        assert!(
            out.contains("stackwave.io") && out.contains("BullMQ"),
            "the next entry must not be swallowed with the employer's: {out}"
        );
    }

    #[test]
    fn nothing_of_the_employer_entry_of_the_real_cv_survives_whatever_its_layout() {
        // Structural property of the real file (nothing is printed): for every line that hits the
        // exclusion list, the title line above it (a `**bold**` line) and every line below it up to
        // the next heading belong to that entry and must be gone.
        let cv = shipped::cv_master().replace("\r\n", "\n");
        let rules = exclusions();
        let lines: Vec<&str> = cv.lines().collect();
        let hits_the_list = |line: &str| {
            let probe = Job {
                description: line.to_string(),
                ..Job::default()
            };
            check_exclusion(&probe, &rules).excluded
        };
        let is_heading = |line: &str| line.starts_with("**") || line.starts_with('#');
        let out = strip_excluded_entries(&redact_contact(&cv), &rules);
        let kept: Vec<&str> = out.lines().map(str::trim).collect();
        for (i, line) in lines.iter().enumerate() {
            if !hits_the_list(line) {
                continue;
            }
            let mut entry: Vec<&str> = vec![line];
            if let Some(title) = lines[..i].iter().rev().find(|l| !l.trim().is_empty())
                && title.starts_with("**")
                && !title.contains(':')
            {
                entry.push(title);
            }
            entry.extend(
                lines[i + 1..]
                    .iter()
                    .take_while(|l| !is_heading(l))
                    .filter(|l| !l.trim().is_empty()),
            );
            for text in entry {
                assert!(
                    !kept.contains(&text.trim()),
                    "a line of an excluded entry is still in the text sent to Claude (line {})",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn evidence_must_be_a_real_cv_line() {
        let real = "•  Built an MCP (Model Context Protocol) server with DUAL transport (stdio + HTTP/SSE)\n connecting 21 AI tools";
        assert!(
            evidence_in_cv(real, SAMPLE_CV),
            "case, spacing and the bullet are ignored"
        );
        assert!(!evidence_in_cv(
            "Led a team of forty engineers shipping Kubernetes operators at Google",
            SAMPLE_CV
        ));
        assert!(
            !evidence_in_cv("Built an MCP server", SAMPLE_CV),
            "too short to count as evidence"
        );
        assert!(!evidence_in_cv("", SAMPLE_CV));
    }

    #[test]
    fn a_slug_is_a_safe_file_name() {
        assert_eq!(slug("greenhouse:anthropic/123"), "greenhouse-anthropic-123");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug(""), "item");
        assert_eq!(slug("¿¡?!"), "item");
        let long = slug(&"a".repeat(200));
        assert_eq!(long.len(), 80);
        for hostile in ["a/b", "a\\b", "a:b", "..", "a b.c", "CON.txt"] {
            let s = slug(hostile);
            assert!(
                s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{s}"
            );
            assert!(!s.starts_with('-') && !s.ends_with('-'), "{s}");
        }
    }

    #[test]
    fn clean_removes_control_characters_and_caps_the_length() {
        assert_eq!(
            clean("  a\u{7}b\u{1b}[31m c \r\n d  ", 100),
            "ab[31m c \n d"
        );
        let capped = clean(&"é".repeat(50), 10);
        assert_eq!(capped.chars().count(), 11, "10 characters and the ellipsis");
        assert!(capped.ends_with('…'));
        assert_eq!(clean("short", 10), "short");
    }

    #[test]
    fn clean_strips_characters_that_hide_text() {
        // zero-width space and joiner, left-to-right and right-to-left overrides, isolates,
        // byte-order mark, soft hyphen, and a "tag" character (ASCII smuggling).
        let sneaky = "a\u{200b}b\u{200d}c\u{202e}d\u{2066}e\u{feff}f\u{ad}g\u{e0041}h";
        assert_eq!(clean(sneaky, 100), "abcdefgh");
        // Visible non-ASCII text, combining marks and emoji without a joiner are left alone.
        assert_eq!(
            clean("canción e\u{301} ñandú 🙂", 100),
            "canción e\u{301} ñandú 🙂"
        );
    }

    #[test]
    fn keys_that_slug_alike_get_different_names() {
        assert_eq!(slug("greenhouse:a/b"), slug("greenhouse:a-b"));
        assert_ne!(key_slug("greenhouse:a/b"), key_slug("greenhouse:a-b"));
        let long_a = format!("{}x", "a".repeat(100));
        let long_b = format!("{}y", "a".repeat(100));
        assert_eq!(
            slug(&long_a),
            slug(&long_b),
            "the slug is cut at 80 characters"
        );
        assert_ne!(key_slug(&long_a), key_slug(&long_b));
    }

    #[test]
    fn a_key_slug_is_stable_and_safe_in_a_file_name() {
        // FNV-1a test vectors: a dossier's name must never change between runs or versions.
        assert_eq!(fnv1a32(""), 0x811c_9dc5);
        assert_eq!(fnv1a32("a"), 0xe40c_292c);
        assert_eq!(fnv1a32("foobar"), 0xbf9c_f968);
        assert_eq!(
            key_slug("test:1"),
            format!("test-1-{:08x}", fnv1a32("test:1"))
        );
        for hostile in ["", "../../etc/passwd", "a\\b", "CON", "ñandú", "a b.c"] {
            let name = key_slug(hostile);
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{name}"
            );
            assert!(!name.starts_with('-'), "{name}");
            assert!(name.len() <= 89, "{name}");
        }
    }

    #[test]
    fn squash_ignores_case_and_spacing() {
        assert_eq!(squash("  Hello \n  WORLD\t!"), "hello world !");
    }
}
