//! The private list of terms an export must not contain. It lives outside the repository (the
//! export refuses a list inside it), because the list itself names what must stay private.
//!
//! One rule per line, matched without regard to case:
//!
//! - `REGEX`: forbidden in every exported file and path;
//! - `except FILE[,FILE...]: REGEX`: forbidden except in the listed files (paths from the root),
//!   for the author's name in the licence;
//! - `allow: TEXT`: a match (or an e-mail address) equal to this text is not a finding, for
//!   fictional values a rule would otherwise catch (a made-up phone number);
//! - `replace: OLD => NEW`: every `OLD`, exactly as written, becomes `NEW` in the exported text
//!   before the scan (a real name in a test becomes the example profile's); the longest `OLD` is
//!   replaced first;
//! - blank lines and lines starting with `#` are skipped.

use regex::{Regex, RegexBuilder};

/// One forbidden pattern and the files where it is allowed.
#[derive(Debug, Clone)]
pub struct Rule {
    pub pattern: Regex,
    pub except: Vec<String>,
}

/// A parsed terms file.
#[derive(Debug, Clone)]
pub struct Terms {
    pub rules: Vec<Rule>,
    /// Matches that are not findings, lowercased.
    pub allowed: Vec<String>,
    /// `(old, new)` pairs, the longest `old` first.
    pub replacements: Vec<(String, String)>,
    /// The addresses the copy is published at (`--public-home`), lowercased: a term inside one of
    /// them is not a finding. Never read from the terms file: whoever runs the export names them.
    pub public_homes: Vec<String>,
    /// One pattern for all of `public_homes`, built when one is added.
    pub public_home: Option<Regex>,
}

#[derive(Debug, thiserror::Error)]
pub enum TermsError {
    #[error("line {line}: {error}")]
    BadPattern { line: usize, error: regex::Error },
    #[error("line {line}: `except` needs `except FILE[,FILE...]: REGEX`")]
    BadExcept { line: usize },
    #[error("line {line}: `replace` needs `replace: OLD => NEW`")]
    BadReplace { line: usize },
    #[error("the terms file has no rule: an export scanned against nothing is not checked")]
    Empty,
}

impl Terms {
    /// Adds `account`'s home folder as a term: `C:\Users\<account>` on any drive with either slash,
    /// and `/home/<account>` or `/Users/<account>`, in any case.
    pub fn forbid_home_of(&mut self, account: &str) {
        let name = regex::escape(account);
        let pattern = format!(r"(?i)\b[a-z]:[\\/]+users[\\/]+{name}\b|/(?:home|users)/{name}\b");
        self.rules.push(Rule {
            pattern: Regex::new(&pattern).expect("an escaped name makes a valid pattern"),
            except: Vec::new(),
        });
    }

    /// Reads a terms file's text.
    pub fn parse(text: &str) -> Result<Terms, TermsError> {
        let mut rules = Vec::new();
        let mut allowed = Vec::new();
        let mut replacements = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let entry = raw.trim();
            if entry.is_empty() || entry.starts_with('#') {
                continue;
            }
            if let Some(text) = entry.strip_prefix("allow:") {
                allowed.push(text.trim().to_lowercase());
                continue;
            }
            if let Some(pair) = entry.strip_prefix("replace:") {
                let (old, new) = pair
                    .split_once(" => ")
                    .ok_or(TermsError::BadReplace { line })?;
                let old = old.trim();
                if old.is_empty() {
                    return Err(TermsError::BadReplace { line });
                }
                replacements.push((old.to_string(), new.trim().to_string()));
                continue;
            }
            let (except, pattern) = match entry.strip_prefix("except ") {
                Some(rest) => {
                    let (files, pattern) = rest
                        .split_once(": ")
                        .ok_or(TermsError::BadExcept { line })?;
                    let files: Vec<String> = files
                        .split(',')
                        .map(|file| file.trim().to_string())
                        .filter(|file| !file.is_empty())
                        .collect();
                    if files.is_empty() || pattern.trim().is_empty() {
                        return Err(TermsError::BadExcept { line });
                    }
                    (files, pattern.trim())
                }
                None => (Vec::new(), entry),
            };
            let pattern = RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .map_err(|error| TermsError::BadPattern { line, error })?;
            rules.push(Rule { pattern, except });
        }
        if rules.is_empty() {
            return Err(TermsError::Empty);
        }
        replacements.sort_by_key(|(old, _): &(String, String)| std::cmp::Reverse(old.len()));
        Ok(Terms {
            rules,
            allowed,
            replacements,
            public_homes: Vec::new(),
            public_home: None,
        })
    }

    /// True when `matched` is one of the allowed texts.
    /// Lets `url` through the term rules: the address the copy is published at.
    pub fn allow_public_home(&mut self, url: &str) {
        self.public_homes.push(url.to_lowercase());
        let any = self
            .public_homes
            .iter()
            .map(|home| regex::escape(home))
            .collect::<Vec<_>>()
            .join("|");
        self.public_home = Some(
            RegexBuilder::new(&any)
                .case_insensitive(true)
                .build()
                .expect("escaped addresses make a valid pattern"),
        );
    }

    /// `line` with every published address blanked out (same length, so nothing else moves),
    /// whatever its case.
    pub fn without_public_homes<'a>(&self, line: &'a str) -> std::borrow::Cow<'a, str> {
        match &self.public_home {
            Some(pattern) => pattern.replace_all(line, |found: &regex::Captures<'_>| {
                " ".repeat(found[0].len())
            }),
            None => std::borrow::Cow::Borrowed(line),
        }
    }

    pub fn is_allowed(&self, matched: &str) -> bool {
        self.allowed.contains(&matched.to_lowercase())
    }

    /// `text` with every replacement made, the longest first.
    pub fn scrub(&self, text: &str) -> String {
        let mut text = text.to_string();
        for (old, new) in &self.replacements {
            if text.contains(old.as_str()) {
                text = text.replace(old.as_str(), new);
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rules_exceptions_and_allowed_texts_and_skips_comments() {
        let terms = Terms::parse(
            "# private\n\nsecret[ -]name\nexcept LICENSE, README.md: jane roe\nallow: +00 0000\n",
        )
        .unwrap();
        assert_eq!(terms.rules.len(), 2);
        assert!(terms.rules[0].pattern.is_match("a SECRET-NAME here"));
        assert!(terms.rules[0].except.is_empty());
        assert_eq!(terms.rules[1].except, vec!["LICENSE", "README.md"]);
        assert!(terms.rules[1].pattern.is_match("Jane Roe"));
        assert!(terms.is_allowed("+00 0000"));
        assert!(!terms.is_allowed("+00 0001"));
    }

    #[test]
    fn a_list_without_rules_or_with_a_broken_line_is_refused() {
        assert!(matches!(
            Terms::parse("# only a comment\nallow: x\n"),
            Err(TermsError::Empty)
        ));
        assert!(matches!(
            Terms::parse("ok\n(unclosed\n"),
            Err(TermsError::BadPattern { line: 2, .. })
        ));
        assert!(matches!(
            Terms::parse("except LICENSE jane\n"),
            Err(TermsError::BadExcept { line: 1 })
        ));
        assert!(matches!(
            Terms::parse("except : jane\n"),
            Err(TermsError::BadExcept { line: 1 })
        ));
        assert!(matches!(
            Terms::parse("ok\nreplace: Secret Name\n"),
            Err(TermsError::BadReplace { line: 2 })
        ));
        assert!(matches!(
            Terms::parse("ok\nreplace:  => Jane\n"),
            Err(TermsError::BadReplace { line: 2 })
        ));
    }

    #[test]
    fn replacements_apply_longest_first_and_exactly_as_written() {
        let terms = Terms::parse(concat!(
            "secret\n",
            "replace: Secret Name => Jane Doe\n",
            "replace: Secret Name and Sons => Acme\n",
            "replace: secret-name.com => acme.example\n",
        ))
        .unwrap();
        assert_eq!(
            terms.scrub(
                "Secret Name and Sons hired Secret Name at secret-name.com; SECRET NAME stays"
            ),
            "Acme hired Jane Doe at acme.example; SECRET NAME stays"
        );
        assert_eq!(terms.scrub("nothing to change"), "nothing to change");
    }
}
