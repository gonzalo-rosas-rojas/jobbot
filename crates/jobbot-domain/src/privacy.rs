//! Keeping a person's e-mail address out of logs, the database and the panel: the pattern of an
//! address in running text, the mask that leaves enough to recognise it (`g***@example.com`), and
//! `Pii`, the type of a field that holds one.

use std::fmt;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

/// A value that is personal data. It prints as `***` (`Pii(***)` under `{:?}`), so a log line, a
/// panic or the `Debug` of the struct that holds it never shows it; the value comes out only
/// through `expose` or `into_inner`, where the reader can see it leave. It reads and writes as the
/// value itself, so a file or a JSON document keeps its shape.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pii<T>(T);

impl<T> Pii<T> {
    pub fn new(value: T) -> Pii<T> {
        Pii(value)
    }

    pub fn expose(&self) -> &T {
        &self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Pii<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pii(***)")
    }
}

impl<T> fmt::Display for Pii<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl<T> From<T> for Pii<T> {
    fn from(value: T) -> Pii<T> {
        Pii(value)
    }
}

impl From<&str> for Pii<String> {
    fn from(value: &str) -> Pii<String> {
        Pii(value.to_string())
    }
}

/// What an e-mail address looks like in running text.
static ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)+").expect("fixed")
});

/// The pattern for an address inside a text, shared by the CLI's masking and the panel's filter.
pub fn address_pattern() -> &'static Regex {
    &ADDRESS
}

/// `text` with every whole address in it masked (`g***@example.com`): for what the CLI prints,
/// which is kept in `meta`, shown by `status` and written to logs.
pub fn mask_addresses(text: &str) -> String {
    ADDRESS
        .replace_all(text, |caps: &Captures| mask_email(&caps[0]))
        .into_owned()
}

/// `g***@example.com`: enough to recognise an address in a log, never the whole of it.
pub fn mask_email(email: &str) -> String {
    let email = email.trim();
    if email.is_empty() {
        return String::new();
    }
    let (local, domain) = email.split_once('@').unwrap_or((email, ""));
    let first: String = local.chars().take(1).collect();
    if domain.is_empty() {
        format!("{first}***")
    } else {
        format!("{first}***@{domain}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_email_is_masked_to_its_first_letter_and_domain() {
        assert_eq!(mask_email("tester@example.com"), "t***@example.com");
        assert_eq!(mask_email("  a@b.c "), "a***@b.c");
        assert_eq!(mask_email("noatsign"), "n***");
        assert_eq!(mask_email(""), "");
    }

    #[test]
    fn every_whole_address_in_a_text_is_masked_and_the_rest_kept() {
        assert_eq!(
            mask_addresses("login gonza.test@example.com ok, cc a@b.co"),
            "login g***@example.com ok, cc a***@b.co"
        );
        assert_eq!(mask_addresses("no address here"), "no address here");
    }

    #[test]
    fn a_pii_value_prints_as_stars_and_comes_out_only_through_expose() {
        let email = Pii::new("ana@mail.example".to_string());
        assert_eq!(format!("{email:?}"), "Pii(***)");
        assert_eq!(format!("{email}"), "***");
        assert_eq!(email.expose(), "ana@mail.example");
        assert_eq!(email.into_inner(), "ana@mail.example");
        let from_text: Pii<String> = "b@mail.example".into();
        assert_eq!(from_text, Pii::new("b@mail.example".to_string()));
    }

    #[test]
    fn a_pii_value_reads_and_writes_as_the_value_itself() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Holder {
            email: Pii<String>,
        }
        let read: Holder = toml::from_str("email = \"ana@mail.example\"\n").unwrap();
        assert_eq!(read.email.expose(), "ana@mail.example");
        assert_eq!(
            toml::to_string(&read).unwrap(),
            "email = \"ana@mail.example\"\n"
        );
    }
}
