//! Answer bank: the answers the user approved for the questions application forms ask.

use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

use crate::text::normalize;

/// One bank entry: every pattern is a normalized phrase; `value: None` means "ask Gonza";
/// `alternatives` are other option texts to try when the field is a drop-down and `value` is not
/// among its options.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerEntry {
    pub patterns: Vec<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
}

impl AnswerEntry {
    pub fn new(patterns: &[&str], value: Option<&str>) -> Self {
        AnswerEntry {
            patterns: patterns.iter().map(|p| p.to_string()).collect(),
            value: value.map(str::to_string),
            alternatives: Vec::new(),
        }
    }

    pub fn with_alternatives(mut self, alternatives: &[&str]) -> Self {
        self.alternatives = alternatives.iter().map(|a| a.to_string()).collect();
        self
    }

    /// The value followed by the alternatives, trimmed and without blanks. Empty when there is
    /// no usable value: an entry that only lists alternatives still means "ask Gonza".
    pub fn candidates(&self) -> Vec<String> {
        let Some(value) = self
            .value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Vec::new();
        };
        std::iter::once(value.to_string())
            .chain(
                self.alternatives
                    .iter()
                    .map(|alternative| alternative.trim().to_string())
                    .filter(|alternative| !alternative.is_empty()),
            )
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnswerBank {
    pub entries: Vec<AnswerEntry>,
    /// Phrases that allow a required checkbox to be ticked (`consent_checkboxes` in the file).
    pub consent_phrases: Vec<String>,
}

/// Labels that ask about the right to work, a visa, sponsorship or citizenship. For a job in a place
/// the user does not live in (Australia in the example profile) the contractor's answers are false:
/// "No sponsorship" for a role that needs the user on site, and "authorized to work" for a place
/// where the user holds no visa. A label that never names the country ("... in the country this
/// role is located") cannot be told apart by its words, so the job decides, not the label.
const WORK_RIGHTS_LABELS: &str = "authorized to work|authorised to work|legally authorized|legally authorised|work authorization|work authorisation|eligible to work|eligibility to work|right to work|legally entitled|legally permitted|work permit|work rights|working rights|visa|sponsorship|sponsor|sponsored|immigration|citizenship|citizen|permanent resident";

/// "Am I allowed to work there" labels, whose true answer for Australia is "No".
const AUSTRALIA_AUTHORIZED: &str = "authorized to work|authorised to work|legally authorized|legally authorised|eligible to work|eligibility to work|right to work|rights to work|legally entitled|legally permitted|entitled to work|permitted to work|allowed to work|work rights|working rights";

const SPONSORSHIP: &str = "sponsorship|sponsor|sponsored";

/// Words that turn a question around: "Can you work there WITHOUT sponsorship?" is also a "No".
const NEGATED_ASK: &str = "without|do not require|don't require|does not require|not require|not need|don't need|doesn't need|no need|no sponsorship|not requiring|not be required";

/// Labels about the country the user lives in keep the bank's own answers (authorized there, no
/// sponsorship needed): the Australian answers never take them.
const ASKS_ABOUT_RESIDENCE: &str = "!reside|residence|live in|living in|currently live|currently located|currently based|country you are in|country you are currently";

const AUSTRALIA_VISA_STATUS: &str = "Chilean citizen with no Australian work rights yet; I would need employer visa sponsorship. I am willing to relocate to Australia.";

impl AnswerBank {
    /// A copy of the bank for a job in Australia, for a user with no work rights there:
    /// "authorized to work" and "citizen or permanent resident" are "No", needing sponsorship or a
    /// visa is "Yes", and a visa-status question gets one honest sentence. A question that asks
    /// both at once, names Chile, or is about anything else to do with visas (a visa subclass,
    /// "have you ever been sponsored") is left to the user. A label about the country the user
    /// resides in keeps the bank's own answer. Everything else answers as before.
    pub fn with_australian_work_rights_answers(&self) -> AnswerBank {
        let residence = format!("&{ASKS_ABOUT_RESIDENCE}");
        let chile = format!("chile|chilean&{WORK_RIGHTS_LABELS}");
        let negated = format!("{AUSTRALIA_AUTHORIZED}|{SPONSORSHIP}&{NEGATED_ASK}{residence}");
        let both = format!("{AUSTRALIA_AUTHORIZED}&{SPONSORSHIP}{residence}");
        let no_rights = format!("{AUSTRALIA_AUTHORIZED}{residence}");
        let no_citizen = format!("citizen|permanent resident{residence}");
        let no_visa = format!(
            "valid visa|valid work visa|hold a visa|hold a work visa|have a visa|have a work visa|have a valid visa{residence}"
        );
        let needs_sponsor = format!("require|need|requiring|needing&{SPONSORSHIP}{residence}");
        let needs_visa = format!(
            "require a visa|require visa|need a visa|need visa|require a work visa|need a work visa|visa support|visa assistance|immigration support|immigration assistance{residence}"
        );
        // Whatever still names a visa, sponsorship or citizenship is left to the user, except the
        // residence labels and the citizenship FIELD (the bank answers "Chile" there).
        let rest = format!(
            "{WORK_RIGHTS_LABELS}{residence}&!country of citizenship|country of nationality|nationality"
        );
        let mut entries = vec![
            AnswerEntry::new(&[chile.as_str()], None),
            AnswerEntry::new(
                &[
                    "visa status|visa type|type of visa|current visa|immigration status|work authorization status|work authorisation status|work rights status|visa situation",
                ],
                Some(AUSTRALIA_VISA_STATUS),
            ),
            AnswerEntry::new(&[negated.as_str()], Some("No")),
            AnswerEntry::new(&[both.as_str()], None),
            AnswerEntry::new(
                &[no_rights.as_str(), no_citizen.as_str(), no_visa.as_str()],
                Some("No"),
            ),
            AnswerEntry::new(&[needs_sponsor.as_str(), needs_visa.as_str()], Some("Yes"))
                .with_alternatives(&[
                    "Yes, I will require sponsorship",
                    "Yes, I require sponsorship",
                    "Yes, now and in the future",
                ]),
            AnswerEntry::new(&[rest.as_str()], None),
        ];
        entries.extend(self.entries.iter().cloned());
        AnswerBank {
            entries,
            consent_phrases: self.consent_phrases.clone(),
        }
    }

    /// The first entry (in file order) with a pattern that appears as whole words in the label.
    /// A pattern that starts with `=` must match the whole label instead (`=name` takes "Name *"
    /// but not "Company name"), for the generic labels that would otherwise catch too much.
    /// Otherwise a pattern is one or more terms joined by `&` (every term must match), a term is
    /// one or more phrases joined by `|` (any phrase may match), and a phrase that ends with `$`
    /// must be the end of the label (`works in the country you reside in$` takes "Do you work in
    /// the country you reside in?" but not "... reside in and in the US?"). A term that starts
    /// with `!` must NOT match (`visa&!reside` takes "Do you need a visa?" but not "... a visa in
    /// the country you reside in?"); a pattern needs at least one term without the `!`.
    pub fn lookup(&self, label: &str) -> Option<&AnswerEntry> {
        // `normalize` drops "co" as a company suffix, which would turn "Co-applicant name" into
        // "applicant name": gluing a "co-" prefix first keeps it one word.
        let text = normalize(&label.to_lowercase().replace("co-", "co"));
        self.entries.iter().find(|entry| {
            entry
                .patterns
                .iter()
                .any(|pattern| pattern_matches(&text, pattern))
        })
    }

    /// True when a checkbox label says one of the consent phrases Gonza approved and NOTHING else
    /// that matters. Two layers, both must pass. First the deny list (`CONSENT_NEVER`): a box that
    /// also accepts arbitration, terms and conditions, marketing or a background check is never
    /// ticked. Then a whitelist: take the approved phrase out of the label and every word that is
    /// left must be a filler word (`CONSENT_FILLER`: "I have read and agree to the ..."), a word of
    /// a URL, or a company possessive ("Cloudflare's"). A word nobody listed means the box asks
    /// for something the bank never saw ("... and to AI-assisted screening", "... for this and
    /// other vacancies"), so it is not ticked and the form goes to Gonza.
    pub fn is_consent(&self, label: &str) -> bool {
        let text = normalize(&without_urls(label));
        let asks_for_more = CONSENT_NEVER
            .iter()
            .any(|word| contains_phrase(&text, &normalize(word)));
        !asks_for_more
            && self
                .consent_phrases
                .iter()
                .any(|phrase| says_only(&text, &normalize(phrase)))
    }
}

/// Words that add no commitment to a privacy consent box: the verbs of reading and agreeing, the
/// glue words, and the nouns of the privacy text itself. Anything else (an unknown noun, a verb of
/// sharing, storing, screening, signing, receiving) keeps the box unticked.
const CONSENT_FILLER: &[&str] = &[
    "i",
    "have",
    "has",
    "read",
    "and",
    "or",
    "understand",
    "acknowledge",
    "acknowledged",
    "agree",
    "accept",
    "consent",
    "to",
    "the",
    "of",
    "our",
    "my",
    "your",
    "you",
    "we",
    "a",
    "an",
    "as",
    "that",
    "this",
    "is",
    "are",
    "be",
    "being",
    "will",
    "by",
    "with",
    "in",
    "on",
    "for",
    "its",
    "their",
    "personal",
    "data",
    "information",
    "processed",
    "processing",
    "collected",
    "collection",
    "use",
    "used",
    "described",
    "applicable",
    "hereby",
    "candidate",
    "applicant",
    "recruitment",
    "application",
    "job",
    "careers",
    "hiring",
    "process",
    "purposes",
    "purpose",
    "regarding",
    "about",
    "please",
    "review",
    "checking",
    "clicking",
    "submitting",
    "box",
    "checkbox",
    "policy",
    "privacy",
    "notice",
    "statement",
    // "Point of Data Transfer: Acknowledge" is how Greenhouse captions a lone checkbox.
    "point",
];

/// Words that end in a possessive `s` in a label ("it's", "that's") and are not a company name.
const NOT_A_COMPANY: &[&str] = &[
    "it", "that", "there", "what", "who", "where", "here", "let", "he", "she", "how", "when", "why",
];

/// The label with every URL token removed: a link's words say nothing about what the box accepts.
fn without_urls(label: &str) -> String {
    label
        .split_whitespace()
        .filter(|token| {
            let token = token.trim_start_matches(['(', '[', '<', '{', '"', '\'']);
            let lowered = token.to_ascii_lowercase();
            let is_url = lowered.starts_with("http://")
                || lowered.starts_with("https://")
                || lowered.starts_with("www.")
                || lowered.contains("://")
                || (token.contains('.') && token.contains('/'));
            !is_url
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when the normalized `text` contains `phrase` and, once every occurrence of the phrase is
/// taken out, only consent filler words are left (see `AnswerBank::is_consent`).
fn says_only(text: &str, phrase: &str) -> bool {
    let phrase_words: Vec<&str> = phrase.split_whitespace().collect();
    let words: Vec<&str> = text.split_whitespace().collect();
    let n = phrase_words.len();
    if n == 0 {
        return false;
    }
    let (last, head) = phrase_words.split_last().expect("n > 0");
    let mut taken = vec![false; words.len()];
    let mut found = false;
    let mut i = 0;
    while i + n <= words.len() {
        let here = &words[i..i + n];
        let last_word = here[n - 1];
        // The phrase's last word may carry a plural `s`, as in `contains_phrase`.
        if &here[..n - 1] == head
            && (last_word == *last || last_word.strip_suffix('s') == Some(last))
        {
            taken[i..i + n].fill(true);
            found = true;
            i += n;
        } else {
            i += 1;
        }
    }
    if !found {
        return false;
    }
    let rest: Vec<&str> = words
        .iter()
        .zip(&taken)
        .filter(|(_, taken)| !**taken)
        .map(|(word, _)| *word)
        .collect();
    let mut k = 0;
    while k < rest.len() {
        let word = rest[k];
        if rest.get(k + 1) == Some(&"s") && !NOT_A_COMPANY.contains(&word) {
            k += 2; // "Cloudflare's": a company possessive
            continue;
        }
        let plural_of_filler = word.len() >= 4
            && word
                .strip_suffix('s')
                .is_some_and(|stem| CONSENT_FILLER.contains(&stem));
        if !CONSENT_FILLER.contains(&word) && !plural_of_filler {
            return false;
        }
        k += 1;
    }
    true
}

/// Phrases that mean a consent box is about more than the privacy consent Gonza authorized. One of
/// them anywhere in the label keeps the box unticked even when it also says "privacy policy"
/// (spec, sections 3 and 7: nothing legal is accepted, no background check is consented to). This
/// is a safety rule, so it lives here and not in the editable answers file; a box it blocks goes
/// to Gonza.
const CONSENT_NEVER: &[&str] = &[
    "arbitration",
    "terms and conditions",
    "terms of use",
    "terms of service",
    "terms of employment",
    "agreement",
    "contract",
    "waive",
    "waiver",
    "binding",
    "non compete",
    "noncompete",
    "confidential",
    "background",
    "marketing",
    "newsletter",
    "promotional",
    "alert",
    "sms",
    "text message",
    "contacted",
    "contact me",
    "future",
    "other opportunities",
    "other roles",
    "other positions",
    "talent community",
    "talent network",
    "talent pool",
    "third party",
    "third parties",
    "sell",
    "retain",
    "retaining",
    "retention",
];

/// Whether one bank pattern matches an already normalized label (see `AnswerBank::lookup`). A
/// term that is empty after normalizing never matches, so a stray `&` or `|` can only make a
/// pattern narrower, never wider.
fn pattern_matches(text: &str, pattern: &str) -> bool {
    if let Some(exact) = pattern.strip_prefix('=') {
        return text == normalize(exact);
    }
    let terms = || pattern.split('&').map(str::trim);
    // A pattern of only `!` terms would match every label that lacks them: it needs a plain term.
    terms().any(|term| !term.starts_with('!'))
        && terms().all(|term| {
            let (excluded, term) = match term.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, term),
            };
            let found = term
                .split('|')
                .any(|phrase| match phrase.trim().strip_suffix('$') {
                    Some(end) => ends_with_phrase(text, &normalize(end)),
                    None => contains_phrase(text, &normalize(phrase)),
                });
            found != excluded
        })
}

/// True when `text` is `phrase`, or ends with `phrase` as whole words. Both are already normalized.
fn ends_with_phrase(text: &str, phrase: &str) -> bool {
    !phrase.is_empty()
        && text.ends_with(phrase)
        && text[..text.len() - phrase.len()]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
}

/// True when `phrase` occurs in `text` delimited by non-alphanumerics, with an optional plural
/// `s` after it. Both strings are already normalized (lowercase ASCII letters, digits, spaces).
fn contains_phrase(text: &str, phrase: &str) -> bool {
    if phrase.is_empty() {
        return false;
    }
    let ends_word = |rest: &str| {
        rest.chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
    };
    text.match_indices(phrase).any(|(start, _)| {
        let starts_word = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        let after = &text[start + phrase.len()..];
        starts_word && (ends_word(after) || after.strip_prefix('s').is_some_and(ends_word))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBank {
    #[serde(default)]
    consent_checkboxes: Vec<String>,
    #[serde(default)]
    entry: Vec<AnswerEntry>,
}

/// Loads `profile/answers.toml`: an optional `consent_checkboxes` list and `[[entry]]` tables with
/// `patterns`, an optional `value` and optional `alternatives`. Unknown keys are errors.
pub fn load_answers(path: &Path) -> anyhow::Result<AnswerBank> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let raw: RawBank =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(AnswerBank {
        entries: raw.entry,
        consent_phrases: raw.consent_checkboxes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn shipped_bank() -> AnswerBank {
        let path = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .profile_dir
            .join("answers.toml");
        load_answers(&path).unwrap()
    }

    fn value_of(bank: &AnswerBank, label: &str) -> Option<String> {
        bank.lookup(label)
            .unwrap_or_else(|| panic!("no entry for {label}"))
            .value
            .clone()
    }

    #[test]
    fn lookup_matches_normalized_label() {
        let bank = shipped_bank();
        assert_eq!(value_of(&bank, "First Name *").as_deref(), Some("Jane"));
        assert_eq!(
            value_of(&bank, "Phone number").as_deref(),
            Some("+56 9 1234 5678")
        );
    }

    #[test]
    fn contact_current_employer_is_always_no() {
        let bank = shipped_bank();
        assert_eq!(
            value_of(&bank, "May we contact your current employer?").as_deref(),
            Some("No")
        );
    }

    #[test]
    fn unanswered_entries_have_no_value() {
        // On purpose: Gonza decides each of these himself, form by form.
        let bank = shipped_bank();
        assert_eq!(value_of(&bank, "GitHub URL"), None);
    }

    #[test]
    fn the_example_answers_are_in_the_bank() {
        let bank = shipped_bank();
        for (label, want) in [
            ("Email address", "jane.doe@example.com"),
            ("What are your salary expectations?", "USD 6,000 per month"),
            ("Desired annual salary (USD)", "72000"),
            ("Notice period", "Immediately"),
            (
                "LinkedIn profile",
                "https://www.linkedin.com/in/jane-doe-garcia/",
            ),
            (
                "Are you open to relocation?",
                "Prefer remote; open to discussing relocation depending on the role",
            ),
        ] {
            assert_eq!(value_of(&bank, label).as_deref(), Some(want), "{label}");
        }
    }

    #[test]
    fn only_privacy_consents_may_be_ticked() {
        let bank = shipped_bank();
        for label in [
            "I have read and agree to the Privacy Policy",
            "I acknowledge the Privacy Notice",
            "I consent to the processing of my personal data for this application",
        ] {
            assert!(bank.is_consent(label), "{label}");
        }
        for label in [
            "I agree to receive marketing emails",
            "I agree to resolve disputes by binding arbitration",
            "I authorize a background check",
        ] {
            assert!(!bank.is_consent(label), "{label}");
        }
    }

    #[test]
    fn a_bare_name_label_is_the_full_name_and_nothing_longer_is() {
        let bank = shipped_bank();
        for label in ["Name", "Name *", "Name*"] {
            assert_eq!(
                value_of(&bank, label).as_deref(),
                Some("Jane Doe Garcia"),
                "{label}"
            );
        }
        for label in ["Company name", "Name of your manager"] {
            assert_ne!(
                bank.lookup(label).and_then(|entry| entry.value.as_deref()),
                Some("Jane Doe Garcia"),
                "{label}"
            );
        }
    }

    #[test]
    fn an_equals_pattern_must_match_the_whole_label() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["=name"], Some("Jane"))],
            consent_phrases: vec![],
        };
        assert!(bank.lookup("Name *").is_some());
        assert!(bank.lookup("Company name").is_none());
        assert!(bank.lookup("Name of manager").is_none());
    }

    fn bank_of(entries: &[(&[&str], &str)]) -> AnswerBank {
        AnswerBank {
            entries: entries
                .iter()
                .map(|(patterns, value)| AnswerEntry::new(patterns, Some(value)))
                .collect(),
            consent_phrases: vec![],
        }
    }

    #[test]
    fn a_dollar_sign_pins_a_phrase_to_the_end_of_the_label() {
        let bank = bank_of(&[(&["reside in$"], "end")]);
        assert!(bank.lookup("Where do you reside in?*").is_some());
        assert!(bank.lookup("Reside in").is_some());
        assert!(
            bank.lookup("Where you reside in, and where else?")
                .is_none()
        );
        // Whole words: "preside in" is not "reside in".
        assert!(bank.lookup("Which court do you preside in").is_none());
        // Without the sign the phrase may sit anywhere (the old behavior).
        let anywhere = bank_of(&[(&["reside in"], "anywhere")]);
        assert!(
            anywhere
                .lookup("Where you reside in, and where else?")
                .is_some()
        );
    }

    #[test]
    fn terms_joined_by_and_all_must_match_and_pipes_offer_alternatives() {
        let bank = bank_of(&[(
            &["expected salary|salary expectation&annual|yearly"],
            "72000",
        )]);
        for label in [
            "Expected salary (annual)",
            "Salary expectation - Yearly (USD)",
            "What is your yearly salary expectations?",
        ] {
            assert!(bank.lookup(label).is_some(), "{label}");
        }
        for label in [
            "Expected salary",
            "Annual bonus",
            "Salary history (annual)",
            "Annualized expected salary",
        ] {
            assert!(bank.lookup(label).is_none(), "{label}");
        }
    }

    #[test]
    fn a_stray_operator_never_widens_a_pattern() {
        // An empty term (a trailing `&`, a leading `|`) matches nothing: the pattern cannot become
        // broader than what was written.
        let bank = bank_of(&[
            (&["salary&"], "a"),
            (&["|"], "b"),
            (&["$"], "c"),
            (&["&|$"], "d"),
        ]);
        assert!(bank.lookup("salary").is_none());
        assert!(bank.lookup("").is_none());
    }

    #[test]
    fn residence_worded_work_rights_questions_have_honest_answers_and_others_stay_open() {
        let bank = shipped_bank();
        assert_eq!(
            value_of(
                &bank,
                "Are you authorized to work in the country you currently reside in?*"
            )
            .as_deref(),
            Some("Yes")
        );
        assert_eq!(
            value_of(
                &bank,
                "Do you, now or in the future, require sponsorship to work in the country you currently reside in?*"
            )
            .as_deref(),
            Some("No")
        );
        // Worded around "where you are applying" the honest answer depends on the role: Gonza decides.
        assert_eq!(
            value_of(
                &bank,
                "Are you legally authorized to work in the country where you are applying?*"
            ),
            None
        );
        // The example user works as a contractor from Chile, so they are not authorized to work in
        // the US (and do not need to be) and never need a visa sponsored.
        for (label, want) in [
            ("Are you authorized to work in the US?", "No"),
            (
                "Will you now or in the future require visa sponsorship for employment?*",
                "No",
            ),
            ("Do you require visa sponsorship?", "No"),
        ] {
            assert_eq!(value_of(&bank, label).as_deref(), Some(want), "{label}");
        }
    }

    #[test]
    fn country_and_previous_employment_answers() {
        let bank = shipped_bank();
        assert_eq!(value_of(&bank, "Country*").as_deref(), Some("Chile"));
        for label in [
            "Have you ever worked at Acme?",
            "Have you ever, or are you currently working at Gusto or Symmetry in any capacity (ie: employee or contingent worker)?*",
        ] {
            assert_eq!(value_of(&bank, label).as_deref(), Some("No"), "{label}");
        }
        // Not about the target company, so the answer is not "No".
        assert!(bank.lookup("Have you ever worked remotely?").is_none());
    }

    #[test]
    fn the_example_bank_leaves_self_identification_to_the_user() {
        // Self-identification is personal: the example bank answers none of it, so each of these
        // questions comes to the user. Add a value in profile/answers.toml only for what you want answered.
        let bank = shipped_bank();
        for label in [
            "How would you describe your gender identity (please select one)?*",
            "I identify as transgender (please select one):*",
            "I identify my race/ethnicity as (mark all that apply):*",
            "I identify my sexual orientation as (please select one):*",
            "Are you Hispanic/Latino?",
            "What are your preferred gender pronouns?",
            "Are you a first-generation college student?",
            "Veteran Status (please select one):*",
            "I have a disability (please select one): *",
            "I identify as a first-generation professional (please select one):*",
            "Gender identity and sexual orientation",
        ] {
            assert!(
                bank.lookup(label)
                    .is_none_or(|entry| entry.candidates().is_empty()),
                "{label}"
            );
        }
    }

    #[test]
    fn unknown_label_returns_none() {
        assert!(shipped_bank().lookup("Favorite color").is_none());
    }

    #[test]
    fn patterns_match_whole_words_and_simple_plurals() {
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(&["city"], Some("Santiago")),
                AnswerEntry::new(&["phone"], Some("123")),
            ],
            consent_phrases: vec![],
        };
        assert!(bank.lookup("Capacity").is_none());
        assert!(bank.lookup("Which cities do you like").is_none());
        assert!(bank.lookup("Home city").is_some());
        assert!(bank.lookup("Phones").is_some());
        assert!(bank.lookup("Smartphone").is_none());
    }

    #[test]
    fn first_matching_entry_wins() {
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(&["current employer"], Some("Acme")),
                AnswerEntry::new(&["employer"], Some("Other")),
            ],
            consent_phrases: vec![],
        };
        assert_eq!(
            value_of(&bank, "Current employer name").as_deref(),
            Some("Acme")
        );
        assert_eq!(
            value_of(&bank, "Previous employer").as_deref(),
            Some("Other")
        );
    }

    #[test]
    fn alternatives_follow_the_value() {
        let bank = shipped_bank();
        let entry = bank.lookup("How did you hear about us?").unwrap();
        assert_eq!(
            entry.candidates(),
            vec![
                "Company careers page",
                "Company website",
                "Careers page",
                "Job board",
                "Other"
            ]
        );
        // Without a usable value there are no candidates, whatever the alternatives say.
        assert!(
            AnswerEntry::new(&["email"], None)
                .with_alternatives(&["x"])
                .candidates()
                .is_empty()
        );
        assert!(
            AnswerEntry::new(&["phone"], Some("  "))
                .candidates()
                .is_empty()
        );
    }

    #[test]
    fn consent_phrases_match_required_checkbox_labels() {
        let bank = AnswerBank {
            entries: vec![],
            consent_phrases: vec![
                "privacy policy".to_string(),
                "consent to the processing".to_string(),
            ],
        };
        assert!(bank.is_consent("I have read the Privacy Policy *"));
        assert!(bank.is_consent("I consent to the processing of my data"));
        assert!(!bank.is_consent("Subscribe to the newsletter"));
        assert!(!bank.is_consent("I agree to the privacy policies of our partners"));
        // The shipped file allows exactly the three privacy phrases Gonza authorized on 2026-10-04.
        assert_eq!(
            shipped_bank().consent_phrases,
            vec![
                "privacy policy",
                "privacy notice",
                "consent to the processing"
            ]
        );
    }

    #[test]
    fn load_answers_reads_alternatives_consent_phrases_and_rejects_typos() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("answers.toml");
        std::fs::write(
            &path,
            "consent_checkboxes = [\"privacy policy\"]\n\n[[entry]]\npatterns = [\"source\"]\nvalue = \"Web\"\nalternatives = [\"Other\"]\n",
        )
        .unwrap();
        let bank = load_answers(&path).unwrap();
        assert_eq!(bank.consent_phrases, vec!["privacy policy"]);
        assert_eq!(bank.entries[0].candidates(), vec!["Web", "Other"]);
        // A misspelled key is an error, not a setting that is silently ignored.
        std::fs::write(&path, "consent_checkboxs = []\n").unwrap();
        assert!(load_answers(&path).is_err());
    }

    #[test]
    fn every_shipped_entry_has_a_pattern() {
        let bank = shipped_bank();
        assert!(bank.entries.len() >= 15);
        assert!(bank.entries.iter().all(|entry| !entry.patterns.is_empty()));
    }

    #[test]
    fn load_answers_reads_entries_without_value_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("answers.toml");
        std::fs::write(&path, "[[entry]]\npatterns = [\"email\"]\n\n[[entry]]\npatterns = [\"city\"]\nvalue = \"Santiago\"\n").unwrap();
        let bank = load_answers(&path).unwrap();
        assert_eq!(bank.entries.len(), 2);
        assert_eq!(bank.entries[0].value, None);
        assert_eq!(bank.entries[1].value.as_deref(), Some("Santiago"));
    }
}

/// The adversarial audit: realistic form labels (every one seen on the sample forms and in the
/// dry-run reports, plus the usual Greenhouse, Lever, Ashby and Workable questions) with what the
/// shipped bank must do with each. `Some(answer)` is the exact approved answer (the first
/// candidate); `None` means the form must go to Gonza: no entry matches, or the entry that does
/// has no value. The `None` rows are the dangerous ones: each is a question some broad pattern
/// used to catch, or could catch, and answer wrongly.
#[cfg(test)]
mod audit {
    use super::*;
    use crate::paths::Paths;

    type Table = &'static [(&'static str, Option<&'static str>)];

    const NAME: &str = "Jane Doe Garcia";
    const LEGAL: &str = "Jane Isabel Doe Garcia";
    const EMAIL: &str = "jane.doe@example.com";
    const PHONE: &str = "+56 9 1234 5678";
    const SANTIAGO: &str = "Santiago, Chile";
    const PAY_MONTH: &str = "USD 6,000 per month";
    const PAY_YEAR: &str = "72000";
    const LINKEDIN: &str = "https://www.linkedin.com/in/jane-doe-garcia/";
    const IMMEDIATELY: &str = "Immediately";
    const COMPANY: &str = "Harbor Light Shipping and Logistics Center LLC (independent contractor)";
    const ROLE: &str = "Freelance Automation Contractor";
    // The candidate is between roles: a label that asks for the CURRENT job gets the truth and,
    // in brackets, the most recent one; a label that asks for the most recent one gets the name.
    const NO_CURRENT_COMPANY: &str = "Between roles (most recent: Harbor Light Shipping and Logistics Center LLC, independent contractor)";
    const NO_CURRENT_ROLE: &str = "Between roles (most recent: Freelance Automation Contractor)";
    const RELOCATION: &str = "Prefer remote; open to discussing relocation depending on the role";

    fn shipped_bank() -> AnswerBank {
        let path = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .profile_dir
            .join("answers.toml");
        load_answers(&path).unwrap()
    }

    /// The text the filler would type or pick first, or None when the form goes to Gonza.
    fn answer_of(bank: &AnswerBank, label: &str) -> Option<String> {
        bank.lookup(label)
            .and_then(|entry| entry.candidates().into_iter().next())
    }

    /// Every row whose answer differs from the table, so one run shows all the damage.
    fn check(table: Table) {
        let bank = shipped_bank();
        let wrong: Vec<String> = table
            .iter()
            .filter_map(|(label, want)| {
                let got = answer_of(&bank, label);
                (got.as_deref() != *want)
                    .then(|| format!("  {label:?}: want {want:?}, got {got:?}"))
            })
            .collect();
        assert!(wrong.is_empty(), "wrong answers:\n{}", wrong.join("\n"));
    }

    // The CV says "English (C1, Professional Working Proficiency)" and he works daily in English
    // with a US company: the bank answers "Advanced". It answers about the person's own level only.
    const ENGLISH: Table = &[
        ("English level", Some("Advanced")),
        ("What is your level of English?", Some("Advanced")),
        ("English proficiency *", Some("Advanced")),
        ("Level of English (spoken and written)", Some("Advanced")),
        ("English language proficiency", Some("Advanced")),
        ("Are you fluent in English?", Some("Yes")),
        ("Do you speak English fluently?", Some("Yes")),
        // Not his level of English: another language, a native-speaker claim, English as a topic.
        ("Are you a native English speaker?", None),
        ("Spanish level", None),
        ("Years of experience writing English documentation", None),
        ("Describe a time you explained something in English", None),
    ];

    #[test]
    fn english_level_comes_from_the_cv() {
        check(ENGLISH);
    }

    const NAMES: Table = &[
        ("First Name *", Some("Jane")),
        ("First name", Some("Jane")),
        ("Given name", Some("Jane")),
        ("Preferred First Name*", Some("Jane")),
        ("Preferred Name (if applicable)", Some("Jane")),
        ("Last Name *", Some("Doe Garcia")),
        ("Surname", Some("Doe Garcia")),
        ("Family name", Some("Doe Garcia")),
        ("Full name", Some(NAME)),
        ("Full Name*", Some(NAME)),
        ("Name *", Some(NAME)),
        ("Name*", Some(NAME)),
        ("Your name", Some(NAME)),
        ("First and last name", Some(NAME)),
        ("Name (first and last)", Some(NAME)),
        ("First name and last name", Some(NAME)),
        ("Legal Name", Some(LEGAL)),
        ("Full legal name", Some(LEGAL)),
        (
            "Please enter your legal name exactly as shown on your government-issued ID.",
            Some(LEGAL),
        ),
        ("Full name as it appears on your passport", Some(LEGAL)),
        ("Middle name", Some("Isabel")),
        ("Middle initial", Some("I")),
        ("Your first name", Some("Jane")),
        ("What is your preferred name?", Some("Jane")),
        ("Full Name (as on your national ID)", Some(LEGAL)),
        // An ID number is not a name, even when the label says "as it appears on your ID".
        ("ID number as it appears on your government ID", None),
        ("Passport number", None),
        // Someone else's name, or a name that is not today's: never Gonza's.
        ("Full name of your university", None),
        ("First and last day you are available", None),
        ("Name of your referee", None),
        ("Hiring manager name", None),
        ("Name of reference 1", None),
        ("Company name", None),
        ("Name of your manager", None),
        ("Manager's full name", None),
        ("Reference name", None),
        ("Reference first name", None),
        ("Reference's last name", None),
        ("Emergency contact name", None),
        ("Recruiter name", None),
        ("Referred by", None),
        ("Spouse's first name", None),
        ("Maiden name", None),
        ("Previous last name", None),
        ("Former surname", None),
        ("Co-applicant name", None),
        ("Co-applicant's name", None),
        ("Referring employee first name", None),
        ("Referring employee last name", None),
        ("Friend's first name", None),
        ("Child's first name", None),
        ("Partner's first name", None),
        ("Sibling's last name", None),
        ("Landlord's full name", None),
        ("Employer's name", None),
        ("Mentor's name", None),
        ("Preferred name", Some("Jane")),
        ("Last name", Some("Doe Garcia")),
        // The possessive of the applicant himself is his own name; another owner's is not.
        ("Applicant's name", Some("Jane Doe Garcia")),
        ("Candidate's full name", Some("Jane Doe Garcia")),
        ("Applicant's first name", Some("Jane")),
        ("Candidate's last name", Some("Doe Garcia")),
        ("Applicant's manager name", None),
        // Signing is never done by the bot, whatever the box asks for.
        ("Type your full name to sign electronically", None),
        ("Signature", None),
    ];

    const CONTACT: Table = &[
        ("Email*", Some(EMAIL)),
        ("Email address", Some(EMAIL)),
        ("E-mail", Some(EMAIL)),
        ("Confirm your email", Some(EMAIL)),
        ("Personal email", Some(EMAIL)),
        ("Phone*", Some(PHONE)),
        ("Phone Number*", Some(PHONE)),
        ("Mobile phone number", Some(PHONE)),
        ("Telephone", Some(PHONE)),
        ("Cell phone", Some(PHONE)),
        ("Contact number", Some(PHONE)),
        ("Phone (with country code)", Some(PHONE)),
        ("Mobile", Some(PHONE)),
        ("Telephone number (with country code)", Some(PHONE)),
        ("Alternate email", None),
        ("Whatsapp number", None),
        ("Contact number of your referee", None),
        ("Contact number of your previous employer", None),
        ("Work email", None),
        ("Work phone", None),
        ("Home phone", None),
        ("Company email", None),
        ("Employer phone number", None),
        ("Reference email", None),
        ("Emergency contact phone number", None),
        ("Emergency contact number", None),
        ("Manager's email address", None),
        ("Supervisor phone", None),
        ("Phone number of your current employer", None),
        ("Email address of your current employer", None),
        ("Country code", None),
        ("Phone country code", None),
        ("Phone type", None),
        ("Phone extension", None),
        // The apostrophe of a possessive is a space after normalizing ("employer s"), so the
        // phone, email or name of ANOTHER person or company must be caught by its owner too.
        ("Employer's phone number", None),
        ("Previous employer's contact number", None),
        ("Previous employer's email address", None),
        ("Employer's email address", None),
        ("Current employer's phone number", None),
        ("Company's phone number", None),
        ("Contact phone number for emergencies", None),
        ("Mentor's email address", None),
        ("Landlord's contact number", None),
        ("Alternate phone number", None),
        ("Alternative email address", None),
        ("Secondary email address", None),
        ("Backup phone number", None),
        ("Additional phone number", None),
        ("Other phone number", None),
        ("Second email", None),
        ("Phone number type", None),
        ("Phone number extension", None),
        ("Phone number country code", None),
        ("Telephone type", None),
        ("Mobile number type", None),
        ("Type of phone", None),
        ("Phone ext.", None),
        // Plain contact fields, however they are phrased, are still Gonza's own.
        ("Phone", Some(PHONE)),
        ("Phone number", Some(PHONE)),
        ("Phone number (with country code)", Some(PHONE)),
        ("Your phone number", Some(PHONE)),
        ("What is your phone number?", Some(PHONE)),
        ("Applicant's phone", Some(PHONE)),
        ("Candidate's phone number", Some(PHONE)),
        ("Applicant's email address", Some(EMAIL)),
        ("Candidate's email", Some(EMAIL)),
        ("Email", Some(EMAIL)),
        ("Email address", Some(EMAIL)),
        ("Please enter your email address", Some(EMAIL)),
        ("Do you have experience with mobile development?", None),
        ("Are you available for a phone screen?", None),
        ("Can we contact you by email or phone?", None),
        ("Email me about similar jobs", None),
    ];

    const LOCATION: Table = &[
        ("Country*", Some("Chile")),
        ("Country", Some("Chile")),
        ("Country of residence", Some("Chile")),
        ("In which country do you currently reside?", Some("Chile")),
        ("Which country are you based in?", Some("Chile")),
        ("Location*", Some(SANTIAGO)),
        ("Location (City)*", Some(SANTIAGO)),
        ("City", Some(SANTIAGO)),
        ("Current City", Some(SANTIAGO)),
        ("Where are you currently located?*", Some(SANTIAGO)),
        ("Current location", Some(SANTIAGO)),
        ("Where are you based?", Some(SANTIAGO)),
        (
            "Where are you currently located (city, country)?",
            Some(SANTIAGO),
        ),
        ("Where do you live?", Some(SANTIAGO)),
        ("Preferred work location", None),
        ("Are you located in Chile?", None),
        ("Is your work location in the United States*", Some("No")),
        ("Are you currently located in the US?", Some("No")),
        ("Do you currently reside in the United States?", Some("No")),
        ("Is your current location in the United States?", Some("No")),
        ("Are you willing to relocate?", Some(RELOCATION)),
        ("Are you open to relocation?", Some(RELOCATION)),
        (
            "If not currently in the Bay Area, are you willing to relocate near our Foster City, CA Office?*",
            Some(RELOCATION),
        ),
        (
            "Are you willing to relocate from your current location?",
            Some(RELOCATION),
        ),
        (
            "Please select the country where you currently reside.",
            Some("Chile"),
        ),
        ("What is your current country of residence?", Some("Chile")),
        (
            "Please choose the country in which you are located.",
            Some("Chile"),
        ),
        (
            "Please indicate your current city, state, and country.",
            Some(SANTIAGO),
        ),
        // A country or city asked about something else is not the field.
        (
            "Do you have family in the country where you currently reside?",
            None,
        ),
        ("Please select the country where the job is located.", None),
        ("Please select the country where you are applying.", None),
        ("Please select the country of your citizenship.", None),
        ("Are you open to relocating?", Some(RELOCATION)),
        (
            "Would you be open to relocating to Austin?",
            Some(RELOCATION),
        ),
        // Australia (an extra place in the example profile): relocating there is "Yes". Whether the
        // user is authorized to work there, needs sponsorship or wants relocation help depends on
        // the posting: the user's call.
        ("Are you willing to relocate to Australia?", Some("Yes")),
        ("Are you willing to relocate to Sydney?", Some("Yes")),
        (
            "Would you be open to relocating to Melbourne, Australia?",
            Some("Yes"),
        ),
        (
            "Will you now or in the future require visa sponsorship to work in Australia?",
            None,
        ),
        ("Do you require sponsorship to work in Australia?", None),
        ("Are you legally authorized to work in Australia?", None),
        ("Do you have the right to work in Australia?", None),
        ("Are you an Australian citizen or permanent resident?", None),
        ("Do you require relocation assistance to Sydney?", None),
        // With no country named, a sponsorship question is still the contractor's "No".
        (
            "Will you now or in the future require visa sponsorship?",
            Some("No"),
        ),
        ("What is your willingness to relocate?", Some(RELOCATION)),
        // A relocation WILLINGNESS question gets the prose; a field that merely mentions relocating
        // (an address, a location plus a question, an instruction to type a word) gets nothing.
        (
            r#"What is the address from which you plan on working? If you would need to relocate, please type "relocating"."#,
            None,
        ),
        (
            "What is your current location and are you open to relocate?",
            None,
        ),
        ("Do you plan to relocate?", None),
        (
            "If you would need to relocate, please describe your timeline",
            None,
        ),
        ("Have you relocated in the last five years?", None),
        ("Which city would you relocate from?", None),
        // "country" and "city" appear inside questions that are not a country or city field.
        ("Country where you are applying", None),
        ("In which country are you applying?", None),
        ("Country of birth", None),
        ("Country of citizenship", Some("Chile")),
        ("City of birth", None),
        ("Emergency contact city", None),
        ("Why do you want to work in the city of Austin?", None),
        ("Which city would you prefer to work from?", None),
        ("Foster City, CA HQ in person days", None),
        ("Are you willing to be located in the United States?", None),
        ("Do you require relocation assistance?", None),
        ("State / Province", None),
        ("Address", None),
    ];

    const WORK_RIGHTS: Table = &[
        (
            "Are you authorized to work in the country you currently reside in?*",
            Some("Yes"),
        ),
        (
            "Do you, now or in the future, require sponsorship to work in the country you currently reside in?*",
            Some("No"),
        ),
        // The same words, wider: more than the residence country is asked, so the residence-only
        // answer would be a half truth. An authorization question goes to Gonza; a sponsorship
        // one is "No" whichever country is named (he works as a contractor from Chile).
        (
            "Are you authorized to work in the country where you currently reside and in the country in which this position is located?",
            None,
        ),
        (
            "Are you authorized to work in your country of residence and the United States?",
            None,
        ),
        (
            "Are you legally authorized to work in the country in which you currently reside and in which you are applying?",
            None,
        ),
        (
            "Are you legally authorized to work in the country where you currently reside or where the position is based?",
            None,
        ),
        (
            "Do you require sponsorship to work in the country you currently reside in, or will you require sponsorship for the country of this role?",
            Some("No"),
        ),
        (
            "Will you require sponsorship to work in the country where you currently reside and in the US?",
            Some("No"),
        ),
        (
            "Do you require visa sponsorship to work in the country you reside in or where this job is located?",
            Some("No"),
        ),
        (
            "Are you authorized to work in the United States? Are you authorized to work in the country you currently reside in?",
            None,
        ),
        (
            "Are you authorized to work in the country you currently reside in, and do you require sponsorship?",
            None,
        ),
        (
            "Do you require sponsorship to work in the country you reside in, or in the country where this role is located?",
            Some("No"),
        ),
        // The single-country residence wording stays answered, whatever comes before it.
        (
            "Are you legally authorized to work in the country in which you currently reside?",
            Some("Yes"),
        ),
        (
            "Are you legally authorized to work in your country of residence?*",
            Some("Yes"),
        ),
        (
            "Will you require sponsorship to work in the country where you currently reside?",
            Some("No"),
        ),
        (
            "Do you require visa sponsorship to work in the country you reside in?",
            Some("No"),
        ),
        (
            "Are you authorized to work in the country where the job is located?*",
            None,
        ),
        (
            "Are you legally authorized to work in the country where you are applying?*",
            None,
        ),
        (
            "Are you legally authorized to work in the country in which you are applying?*",
            None,
        ),
        (
            "Do you now or will you in the future need sponsorship for employment visa status in the country in which you are applying?*",
            Some("No"),
        ),
        (
            "Will you need sponsorship now or in the future? *",
            Some("No"),
        ),
        (
            "Will you now or in the future require visa sponsorship for employment?*",
            Some("No"),
        ),
        (
            "Will you now or in the future require sponsorship for employment visa status in this country?*",
            Some("No"),
        ),
        (
            "Are you legally authorized to work in the United States?*",
            Some("No"),
        ),
        (
            "Will you now, or in the future, require sponsorship for employment visa status (e.g. H-1B visa status)?*",
            Some("No"),
        ),
        ("Do you require visa sponsorship?", Some("No")),
        ("Do you need a visa to work in Chile?", None),
        ("Are you a citizen of Chile?", None),
        ("What is your work authorization status?", None),
        ("Are you eligible to work in the EU?", None),
        (
            "Are you currently located in the U.S. and legally authorized to work?",
            None,
        ),
        ("Are you a U.S. citizen?", None),
        ("Do you have a valid work permit?", None),
        ("Do you have the right to work in the UK?", None),
        ("Do you hold an active security clearance?", None),
        ("Have you ever been convicted of a felony?", None),
        ("Are you willing to undergo a background check?", None),
        (
            "Do you have a non-compete agreement with your current employer?",
            None,
        ),
        (
            "Are you subject to U.S. export controls or sanctions?",
            None,
        ),
        (
            "Citizen or permanent resident of Cuba, Iran, North Korea, or Syria",
            None,
        ),
        (
            "Please confirm whether any of the below applies to you. Select all that apply. Note: This information will only be used to ensure compliance with U.S. sanctions and export controls.",
            None,
        ),
        ("Applicant Arbitration Agreement Acknowledgement", None),
        (
            "Do you agree to binding arbitration of employment disputes?",
            None,
        ),
    ];

    const PAY: Table = &[
        ("What are your salary expectations?", Some(PAY_MONTH)),
        ("Compensation Expectations*", Some(PAY_MONTH)),
        ("What is your desired salary range?*", Some(PAY_MONTH)),
        ("Expected salary", Some(PAY_MONTH)),
        ("Salary requirements", Some(PAY_MONTH)),
        ("Expected monthly salary (USD)", Some(PAY_MONTH)),
        (
            "What are your current salary expectations?",
            Some(PAY_MONTH),
        ),
        ("Desired annual salary (USD)", Some(PAY_YEAR)),
        ("What is your expected base salary?", Some(PAY_MONTH)),
        ("Expected CTC", None),
        ("Salary expectations (hourly)", None),
        ("Salary expectations (CAD)", None),
        // A field in thousands wants 72 (a year) or 6 (a month), not 72000: the answer would be
        // wrong by a factor of 1000. Without a period it is the year.
        ("Expected annual salary (in thousands of USD)", Some("72")),
        ("Expected annual salary (USD, in thousands)", Some("72")),
        ("Expected annual salary (k USD)", Some("72")),
        ("Expected annual salary ($K)", Some("72")),
        ("Expected annual salary ($000s)", Some("72")),
        ("Expected annual salary (in 000s)", Some("72")),
        ("Expected annual salary (in k)", Some("72")),
        ("Expected salary (in thousands)", Some("72")),
        ("Salary expectations (USD thousands)", Some("72")),
        ("Expected monthly salary (in thousands of USD)", Some("6")),
        ("Desired salary per month (in 000s)", Some("6")),
        // ...but only the expectation: what he earns now, a bare "salary" and two figures at once
        // stay unanswered.
        ("Current salary (in thousands)", None),
        ("Salary (in thousands of USD)", None),
        ("Current and expected salary (in thousands)", None),
        // Annual wording that is not the exact adjacent phrase still asks for the annual figure.
        ("Salary expectation - Annual (USD)", Some(PAY_YEAR)),
        ("Expected Salary (USD / year)", Some(PAY_YEAR)),
        (
            "What is your expected salary in USD (annual)?",
            Some(PAY_YEAR),
        ),
        ("Expected salary per year", Some(PAY_YEAR)),
        ("Expected compensation (annually)", Some(PAY_YEAR)),
        ("Yearly salary expectations (USD)", Some(PAY_YEAR)),
        // An annual or yearly question the annual answer cannot take is never given the monthly
        // text, and neither is a question that is not a figure at all.
        ("Expected salary / year", None),
        ("Salary expectation - Annual (CLP)", None),
        ("What are your current salary expectations (annual)?", None),
        ("Do you meet the salary requirements?", None),
        (
            "Is the posted salary range in line with your salary expectations?",
            None,
        ),
        (
            "Are you comfortable with the salary range for this role?",
            None,
        ),
        (
            "Are you okay with the compensation for this position?",
            None,
        ),
        // Two figures are asked and one of them is what he earns now: the expectation alone
        // would assert a current pay he never gave.
        ("Please state your current and expected salary", None),
        ("Current and expected salary", None),
        ("Current / expected salary", None),
        ("Current vs expected compensation", None),
        ("Salary history and desired salary", None),
        // The expectation must never stand in for what the user earns now, or for another currency
        // or pay period.
        ("Current salary", None),
        ("What is your current annual salary?", None),
        ("Current annual compensation", None),
        ("What is your current compensation?", None),
        ("What is your current base salary?", None),
        ("What was your previous salary?", None),
        ("Salary history", None),
        ("How much do you currently earn?", None),
        ("Hourly rate", None),
        ("What is your expected hourly rate?", None),
        ("Daily rate", None),
        ("Desired annual salary (CLP)", None),
        ("Expected salary (EUR)", None),
        // Ambiguous between current and expected, or not a pay question at all.
        ("Annual salary", None),
        ("Salary", None),
        ("Minimum salary you would accept", None),
        ("Salary currency", None),
    ];

    const START: Table = &[
        ("Notice period", Some(IMMEDIATELY)),
        ("Notice period (weeks)", Some(IMMEDIATELY)),
        ("How soon can you start?", Some(IMMEDIATELY)),
        ("Earliest start date", Some(IMMEDIATELY)),
        ("Desired start date", Some(IMMEDIATELY)),
        ("Available start date", Some(IMMEDIATELY)),
        ("When can you start a new role?*", Some(IMMEDIATELY)),
        ("When are you available to start?", Some(IMMEDIATELY)),
        ("Are you available to start immediately?", Some(IMMEDIATELY)),
        ("Earliest available start date", Some(IMMEDIATELY)),
        ("When would you be available to start?", Some(IMMEDIATELY)),
        ("What is your notice period?", Some(IMMEDIATELY)),
        // A bare "start date" is also the start of a past job; the bare date picker label is
        // the placeholder; "availability" is also interview availability or hours per week.
        ("Start date", None),
        ("Are you currently serving a notice period?", None),
        ("Which hours are you available from?", None),
        ("Start date of your most recent job", None),
        ("End date", None),
        ("Pick date...", None),
        ("Availability", None),
        ("Interview availability", None),
        ("What is your availability for an interview?", None),
        ("Please share your availability for a call", None),
        ("Weekly availability (hours per week)", None),
        (
            "Are you available for a technical interview next week?",
            None,
        ),
        ("Date of birth", None),
    ];

    const EMPLOYMENT: Table = &[
        ("Current Company*", Some(NO_CURRENT_COMPANY)),
        ("Current employer", Some(NO_CURRENT_COMPANY)),
        ("Current company (if applicable)", Some(NO_CURRENT_COMPANY)),
        ("Most recent company", Some(COMPANY)),
        ("Current or most recent employer", Some(COMPANY)),
        ("Current Role*", Some(NO_CURRENT_ROLE)),
        ("Current job title", Some(NO_CURRENT_ROLE)),
        ("Current position", Some(NO_CURRENT_ROLE)),
        ("Current Role/Title", Some(NO_CURRENT_ROLE)),
        ("Most recent job title", Some(ROLE)),
        ("May we contact your current employer?", Some("No")),
        (
            "Do you give us permission to contact your current employer?",
            Some("No"),
        ),
        (
            "Do you currently or have you previously worked for Databricks in the past?*",
            Some("No"),
        ),
        (
            "Have you ever, or are you currently working at Gusto or Symmetry in any capacity (ie: employee or contingent worker)?*",
            Some("No"),
        ),
        ("Have you ever worked at Acme?", Some("No")),
        (
            "Have you ever worked for or been employed by Acme?",
            Some("No"),
        ),
        ("Have you ever been employed by OpenAI?", Some("No")),
        (
            "Are you a current or former employee of Notion?",
            Some("No"),
        ),
        ("Who is your current or previous employer?", Some(COMPANY)),
        ("What is your current or previous job title?", Some(ROLE)),
        (
            "Have you previously been employed by Coinbase in any capacity?",
            Some("No"),
        ),
        // A company name that is also a site (GitHub, GitLab) is the company here, not a profile.
        (
            "Have you previously worked at or consulted for GitLab?",
            Some("No"),
        ),
        ("Have you ever worked at GitHub?", Some("No")),
        // "Ever worked at/for" a class of employers, a list of them, or a company Gonza did work
        // for (the CV: stackwave.io, Bluecrate, Pinewood Foods, Meridian Freight, Brightpath, Harbor Light): the
        // bank cannot tell which company is meant, and "No" would be a guess.
        (
            "Have you ever worked for any government or public sector organization?",
            None,
        ),
        ("Have you ever worked for one of our competitors?", None),
        // A named big-tech company is just a company: Gonza has never worked at any of them.
        (
            "Have you ever worked at Google, Meta or Microsoft?",
            Some("No"),
        ),
        (
            "Have you ever worked at Stripe, Plaid or Google?",
            Some("No"),
        ),
        ("Have you ever worked at Google?", Some("No")),
        ("Have you ever worked for Amazon?", Some("No")),
        // The same company words outside an employment question are not caught by that guard.
        (
            "How did you hear about this job? LinkedIn or Google",
            Some("Company careers page"),
        ),
        (
            "Have you ever worked for any federal, state, or local government?",
            None,
        ),
        (
            "Have you ever been employed by any government agency?",
            None,
        ),
        (
            "Have you ever worked for another company in our industry?",
            None,
        ),
        (
            "Have you previously worked for any of our competitors?",
            None,
        ),
        ("Have you previously been employed by a competitor?", None),
        ("Have you ever worked at Brightpath?", None),
        (
            "Have you ever worked for Harbor Light Shipping and Logistics Center?",
            None,
        ),
        ("Have you ever worked at Bluecrate?", None),
        ("Have you ever worked for stackwave.io?", None),
        ("Have you ever worked at Pinewood Foods?", None),
        ("Have you ever worked for Meridian Freight?", None),
        ("Are you a current or former government employee?", None),
        // The standard shapes about the company applied to keep their answer.
        (
            "Have you ever worked for Acme or any of its subsidiaries?",
            Some("No"),
        ),
        ("Have you ever worked for Stripe?", Some("No")),
        // "current employer" in a question that is not asking for its name.
        (
            "Does your current employer know that you are applying?",
            None,
        ),
        ("Is your current employer a competitor?", None),
        ("Years of experience at your current employer", None),
        ("Describe your current role and responsibilities", None),
        ("Job title", None),
        ("Employer name", None),
        ("Previous employer", None),
        ("Are you currently employed?", None),
        // "ever worked at/for" in questions that are not about the target company.
        ("Have you ever worked remotely?", None),
        ("Have you ever worked for a Fortune 500 company?", None),
        ("Have you worked at Acme before?", None),
        ("Where do you currently work?", None),
        ("Current organization", None),
        ("Have you ever worked for a startup?", None),
        (
            "Have you ever worked at a company with more than 500 employees?",
            None,
        ),
        ("Have you previously worked for the U.S. government?", None),
        ("Have you ever been employed by a competitor?", None),
        ("Are you a current employee of a competitor?", None),
        ("Have you applied to this company before?", None),
        ("Do you have any relatives who work at Acme?", None),
        ("Were you referred by a current employee?", None),
    ];

    const EDUCATION: Table = &[
        ("School*", Some("Example Professional Institute")),
        ("School name", Some("Example Professional Institute")),
        ("Degree*", Some("Ingeniero en Informática")),
        ("Discipline*", Some("Computer Engineering")),
        ("Field of study", Some("Computer Engineering")),
        ("Highest level of education", None),
        ("Do you have a Bachelor's degree?", None),
        ("Graduation year", None),
        ("GPA", None),
    ];

    const SELF_ID: Table = &[
        ("Gender", None),
        (
            "How would you describe your gender identity (please select one)?*",
            None,
        ),
        ("What is your gender?", None),
        ("Please select your gender", None),
        ("Gender Identity", None),
        ("I identify as transgender (please select one):*", None),
        // The example bank answers no self-identification question: each one comes to the user.
        ("Race", None),
        ("Race/Ethnicity", None),
        (
            "I identify my race/ethnicity as (mark all that apply):*",
            None,
        ),
        ("Ethnicity", None),
        ("Veteran Status (please select one):*", None),
        ("Veteran Status", None),
        ("Are you a protected veteran?", None),
        ("Are you a veteran?", None),
        ("Are you a protected veteran or disabled veteran?", None),
        ("I have a disability (please select one): *", None),
        ("Please indicate if you have a disability", None),
        ("Disability Status", None),
        ("Do you have a disability?", None),
        ("What are your preferred gender pronouns?", None),
        ("Pronouns", None),
        ("Preferred pronouns (he/him, she/her, they/them)", None),
        (
            "I identify my sexual orientation as (please select one):*",
            None,
        ),
        ("Are you Hispanic/Latino?", None),
        ("Do you consider yourself Hispanic or Latino?", None),
        ("Are you a first-generation college student?", None),
        ("Do you identify as LGBTQ+?", None),
        // Two questions in one label, and a question that is not his own identity, stay with him.
        ("Gender identity and sexual orientation", None),
        ("Sexual orientation and gender", None),
        (
            "I identify as a first-generation professional (please select one):*",
            None,
        ),
        ("Are you an ally of the LGBTQ+ community?", None),
        ("Marital status", None),
        ("Religion", None),
        ("Religious affiliation", None),
        ("Nationality", Some("Chilean")),
        ("Citizenship", Some("Chile")),
        ("Country of citizenship", Some("Chile")),
        // A citizenship question about a particular country, or its status, is a legal one.
        ("Are you a US citizen?", None),
        ("Citizenship status", None),
        ("Age", None),
        ("Voluntary Self-Identification of Gender", None),
        ("Voluntary Self-Identification of Disability", None),
        ("VeteranStatus", None),
        ("DisabilityStatus", None),
        ("Are you an individual with a disability?", None),
        ("Are you a person with a disability?", None),
        ("Do you identify as transgender?", None),
        ("Are you transgender?", None),
        ("What is your ethnicity?", None),
        ("Racial or ethnic background", None),
        // The same words in questions about the work, a relative, a team or a group: not Gonza's
        // own self-identification, so never a "No" or a "White".
        (
            "Do you have experience building accessible products for people with disabilities?",
            None,
        ),
        (
            "Do you have experience designing for people with disabilities?",
            None,
        ),
        ("Are you a caregiver for someone with a disability?", None),
        ("Do you have a child with a disability?", None),
        (
            "Do you belong to an underrepresented racial or ethnic group?",
            None,
        ),
        (
            "Do you identify as a member of an underrepresented gender identity group?",
            None,
        ),
        (
            "Do you identify as a member of an underrepresented ethnic background?",
            None,
        ),
        ("Do you support people who identify as transgender?", None),
        (
            "Are you comfortable working with transgender colleagues?",
            None,
        ),
        ("Do you have experience analyzing ethnicity data?", None),
        ("Do you have experience debugging race conditions?", None),
        ("Explain a race condition you have fixed", None),
        ("Do you have a family member who is a veteran?", None),
        (
            "Do you have experience with disability benefits administration?",
            None,
        ),
        ("Do you have a family member with a disability?", None),
        (
            "Do you have any disabilities that would require accommodation?",
            None,
        ),
        (
            "Do you require reasonable accommodation to complete the application process?",
            None,
        ),
    ];

    const OTHER: Table = &[
        (
            "How did you hear about this opportunity? (select all that apply)*",
            Some("Company careers page"),
        ),
        ("How did you hear about us?", Some("Company careers page")),
        (
            "Where did you hear about this job?",
            Some("Company careers page"),
        ),
        ("Are you at least 18 years of age?*", Some("Yes")),
        ("Are you 18 years of age or older?", Some("Yes")),
        (
            "Do you have at least 18 months of experience with Kubernetes?",
            None,
        ),
        (
            r#"Zip Code / Postal Code (Non-U.S. based candidates, please enter "00000")*"#,
            Some("00000"),
        ),
        ("Zip Code", None),
        ("Postal code", None),
        ("ZIP / Postal Code", None),
        ("Postal code (use 12345 if outside the US)", None),
        ("Street address", None),
        ("Resume/CV *", None),
        ("Cover Letter", None),
        ("Additional Information", None),
        ("LinkedIn Profile*", Some(LINKEDIN)),
        ("LinkedIn", Some(LINKEDIN)),
        ("GitHub URL", None),
        ("LinkedIn Profile URL", Some(LINKEDIN)),
        ("Please provide your LinkedIn", Some(LINKEDIN)),
        ("Link to your GitHub profile", None),
        ("GitHub username", None),
        ("GitLab profile", None),
        ("Portfolio URL", None),
        ("Website", None),
        ("Personal website", None),
        ("Replit Profile URL", None),
        (
            "Why are you interested in this role? Please reference one specific area from the job description (e.g. agent evaluation, debugging tools, or natural-language automation design) and explain why it appeals to you.*",
            None,
        ),
        ("What excites you about Replit?*", None),
        (
            "If you want to share something you built with Replit please share below.",
            None,
        ),
        (
            "How many years of relevant professional experience do you have?*",
            None,
        ),
        (
            "Are you able to work from our Foster City, CA HQ 3 days per week?*",
            None,
        ),
        (
            "Are you able to work from our US office three days per week?*",
            None,
        ),
        ("Tell us about a project you are proud of", None),
        ("Anything else you'd like us to know?", None),
        ("Do you have experience with Python?", None),
        ("Currency", None),
    ];

    const ALL: [Table; 10] = [
        NAMES,
        CONTACT,
        LOCATION,
        WORK_RIGHTS,
        PAY,
        START,
        EMPLOYMENT,
        EDUCATION,
        SELF_ID,
        OTHER,
    ];

    #[test]
    fn names_never_become_someone_elses_or_a_signature() {
        check(NAMES);
    }

    #[test]
    fn email_and_phone_answer_only_gonzas_own_contact_fields() {
        check(CONTACT);
    }

    #[test]
    fn country_and_city_answer_only_country_and_city_fields() {
        check(LOCATION);
    }

    #[test]
    fn work_rights_and_legal_questions_stay_with_gonza() {
        check(WORK_RIGHTS);
    }

    #[test]
    fn the_salary_expectation_never_answers_current_pay() {
        check(PAY);
    }

    #[test]
    fn start_and_notice_answers_skip_dates_of_past_jobs_and_interview_availability() {
        check(START);
    }

    #[test]
    fn employment_answers_are_for_the_target_company_and_the_current_job_only() {
        check(EMPLOYMENT);
    }

    #[test]
    fn education_is_answered_by_exact_field_name_only() {
        check(EDUCATION);
    }

    #[test]
    fn self_identification_answers_do_not_leak_into_other_questions() {
        check(SELF_ID);
    }

    #[test]
    fn the_remaining_forms_labels() {
        check(OTHER);
    }

    #[test]
    fn the_audit_covers_at_least_120_labels_and_never_contradicts_itself() {
        let total: usize = ALL.iter().map(|table| table.len()).sum();
        assert!(total >= 120, "only {total} labels");
        let mut seen = std::collections::HashMap::new();
        for table in ALL {
            for (label, want) in table {
                if let Some(previous) = seen.insert(*label, *want) {
                    assert_eq!(previous, *want, "{label} is listed twice");
                }
            }
        }
    }

    #[test]
    fn the_education_options_are_honest_and_ordered_from_exact_to_vaguest() {
        let bank = shipped_bank();
        assert_eq!(
            bank.lookup("School*").unwrap().candidates(),
            vec![
                "Example Professional Institute",
                "Example Institute",
                "Example",
                "Other"
            ]
        );
        assert_eq!(
            bank.lookup("Degree*").unwrap().candidates(),
            vec!["Ingeniero en Informática", "Engineer's Degree", "Other"]
        );
        // The example CV has no Bachelor's, Master's or computer science degree: none is claimed.
        for label in ["School*", "Degree*", "Discipline*"] {
            for candidate in bank.lookup(label).unwrap().candidates() {
                let lowered = candidate.to_lowercase();
                assert!(!lowered.contains("bachelor"), "{label}: {candidate}");
                assert!(
                    !lowered.contains("computer science"),
                    "{label}: {candidate}"
                );
                assert!(!lowered.contains("master"), "{label}: {candidate}");
            }
        }
    }

    /// Boxes whose whole text is the privacy consent Gonza authorized: these are ticked.
    const CONSENT_TICK: &[&str] = &[
        "I have read and agree to the Privacy Policy",
        "I have read and agree to the Privacy Policy *",
        "I have read the Privacy Policy *",
        "I agree to the privacy policy",
        "I accept the Privacy Policy",
        "I have read and accept the Privacy Policy *",
        "I acknowledge the Privacy Notice *",
        "I acknowledge that I have read the Candidate Privacy Notice",
        "Please review and acknowledge Cloudflare's Candidate Privacy Policy (https://www.cloudflare.com/candidate-privacy-notice/)",
        "Please review and acknowledge Cloudflare\u{2019}s Candidate Privacy Policy",
        "I consent to the processing of my personal data for this application",
        "I consent to the processing of my personal data for recruitment purposes",
        "I consent to the processing of my personal data for the purposes of this recruitment process",
        "By checking this box, I acknowledge the Privacy Notice",
        "By submitting this application, I acknowledge that I have read the Privacy Notice",
        "Applicant Privacy Notice: Acknowledge",
        "I understand that my personal data will be processed as described in the Candidate Privacy Notice",
        "I have read the Applicant Privacy Policy (https://boards.greenhouse.io/privacy) and agree",
        "I have read and understand Acme's Privacy Statement and Privacy Policy",
    ];

    /// Boxes that also accept, certify, authorize or sign up for something more. Every one of them
    /// carries an approved phrase and must still go to Gonza.
    const CONSENT_SKIP: &[&str] = &[
        "I have read the Privacy Policy and agree to the Terms",
        "I have read the Privacy Policy and certify that my answers are true",
        "I understand and agree to the Privacy Policy. I understand that false statements may result in termination.",
        "I consent to the processing of my personal data for this and other vacancies",
        "I consent to the processing of my personal data for this and similar roles",
        "I consent to the processing of my personal data and to it being stored for 24 months",
        "I consent to the processing of my personal data and to AI-assisted screening",
        "I agree to the use of AI tools in evaluating my application. I have read the Privacy Policy",
        "I consent to the processing of my personal data and to the sharing of it with Acme group companies",
        "I consent to the processing of my personal data and to the sharing of it with Acme affiliates",
        "I consent to the processing of my personal data and to the sharing of it with partners",
        "I consent to the processing of my personal data and to the sharing of it with subsidiaries",
        "I consent to the processing of my personal data and to the sharing of it with service providers",
        "I consent to the processing of my personal data and to its transfer outside Chile and the EU",
        "I consent to the processing of my personal data, including my biometric data",
        "I consent to the processing of my sensitive personal data, including health data",
        "I accept the Privacy Policy and the Data Processing Addendum",
        "I accept the Privacy Policy and reference checks",
        "I consent to the processing of my personal data, including a credit check",
        "I authorize Acme to verify my employment history. I have read the Privacy Policy",
        "I have read the Privacy Policy and agree to electronic signature",
        "I consent to being recorded during interviews. I have read the Privacy Policy",
        "I have read the Privacy Policy and understand that this does not guarantee employment and that employment is at will",
        "I agree to the privacy policy and consent to receive communications",
        "I agree to the privacy policy and consent to receive updates",
        "I agree to the privacy policy and consent to receive notifications",
        "I have read the Privacy Policy and electronically sign this application",
        "I have read the Privacy Policy and agree to receive email updates about job openings",
        "I have read the Privacy Policy and consent to the transfer of my data outside my country",
        "I have read the Privacy Policy and consent to AI-assisted screening and automated decision making",
        "I have read the Privacy Policy and consent to being added to our talent pipeline",
        "I have read the Privacy Policy and consent to calls from recruiters",
        "I have read the Privacy Policy and opt in to Whatsapp notifications",
        // Words the allow list has never seen: an unknown word is never ticked.
        "I have read the Privacy Policy and consent to a drug test",
        "I have read the Privacy Policy and the Cookie Policy",
        "I have read the Privacy Policy and Acme's Code of Conduct",
        "I consent to the processing of my personal data by Acme's clients",
        "I acknowledge the Privacy Policy and that Acme may contact my previous employers",
        "I consent to the processing of my personal data. I also certify that I am over 18",
        // A company name the bank does not know is an unknown word too: Gonza ticks it himself.
        "I have read the Acme Privacy Policy",
    ];

    #[test]
    fn a_consent_box_is_ticked_only_when_the_privacy_phrase_is_all_it_says() {
        let bank = shipped_bank();
        let wrongly_ticked: Vec<String> = CONSENT_SKIP
            .iter()
            .filter(|label| bank.is_consent(label))
            .map(|label| format!("  {label:?}"))
            .collect();
        let wrongly_skipped: Vec<String> = CONSENT_TICK
            .iter()
            .filter(|label| !bank.is_consent(label))
            .map(|label| format!("  {label:?}"))
            .collect();
        assert!(
            wrongly_ticked.is_empty() && wrongly_skipped.is_empty(),
            "ticked but must go to Gonza:\n{}\nskipped but plain privacy consent:\n{}",
            wrongly_ticked.join("\n"),
            wrongly_skipped.join("\n")
        );
    }

    #[test]
    fn a_consent_box_that_also_accepts_something_else_is_never_ticked() {
        let bank = shipped_bank();
        for label in [
            "I have read and agree to the Privacy Policy",
            "I acknowledge the Privacy Notice *",
            "I consent to the processing of my personal data for this application",
        ] {
            assert!(bank.is_consent(label), "{label}");
        }
        for label in [
            "I have read the Privacy Policy and agree to the Applicant Arbitration Agreement",
            "I agree to the Terms and Conditions and the Privacy Policy",
            "I accept the Privacy Policy and consent to receive marketing emails",
            "I consent to the processing of my data and to a background check",
            "I agree to the Privacy Policy and to be contacted about future opportunities",
            "I agree to the privacy policy and waive my right to a jury trial",
            "I consent to the processing of my personal data and to receive SMS messages",
        ] {
            assert!(!bank.is_consent(label), "{label}");
        }
    }

    #[test]
    fn a_term_with_a_bang_must_not_match_and_a_pattern_needs_a_plain_term() {
        assert!(pattern_matches("do you need a visa", "visa&!reside"));
        assert!(!pattern_matches(
            "do you need a visa in the country you reside in",
            "visa&!reside"
        ));
        assert!(!pattern_matches(
            "do you need a visa in the country you reside in",
            "visa&!reside|residence"
        ));
        assert!(
            !pattern_matches("anything at all", "!reside"),
            "only `!` terms would match every label that lacks them"
        );
        assert!(!pattern_matches("anything at all", "!"));
    }

    /// The answers for a job in Australia, for a user with no work rights there: not authorized,
    /// sponsorship needed. A label about the country the user resides in keeps the bank's own
    /// answer, and whatever else touches a visa or sponsorship goes to the user.
    const AUSTRALIA_WORK_RIGHTS: Table = &[
        (
            "Will you now or in the future require visa sponsorship to work in the country this role is located?",
            Some("Yes"),
        ),
        (
            "Will you now or in the future require visa sponsorship to work in Australia?",
            Some("Yes"),
        ),
        (
            "Do you require sponsorship to work in Australia?",
            Some("Yes"),
        ),
        ("Will you require sponsorship?*", Some("Yes")),
        ("Do you need a visa to work in Australia?", Some("Yes")),
        ("Will you require visa support?", Some("Yes")),
        (
            "Are you legally authorized to work in Australia?",
            Some("No"),
        ),
        ("Do you have the right to work in Australia?", Some("No")),
        ("Are you eligible to work in Australia?*", Some("No")),
        (
            "Are you an Australian citizen or permanent resident?",
            Some("No"),
        ),
        ("Do you hold a valid visa to work in Australia?", Some("No")),
        // The same words turned around.
        (
            "Do you have the right to work in Australia without sponsorship?",
            Some("No"),
        ),
        (
            "Are you able to work in Australia without visa sponsorship?",
            Some("No"),
        ),
        (
            "I do not require sponsorship to work in Australia",
            Some("No"),
        ),
        ("I don't need visa sponsorship", Some("No")),
        // One honest sentence for a status question.
        ("What is your visa status?", Some(AUSTRALIA_VISA_STATUS)),
        (
            "Current work authorization status",
            Some(AUSTRALIA_VISA_STATUS),
        ),
        // Left to the user: two questions in one, another country, anything else about visas.
        (
            "Are you legally authorized to work in Australia, or will you require sponsorship?",
            None,
        ),
        ("Are you legally authorized to work in Chile?", None),
        ("Are you a Chilean citizen?", None),
        ("Have you ever been sponsored by an employer?", None),
        ("Which visa subclass do you hold?", None),
        ("Please select the country of your citizenship.", None),
        ("Do you require relocation assistance?", None),
        // The country the user lives in, and what the bank already knew.
        (
            "Are you authorized to work in the country you currently reside in?*",
            Some("Yes"),
        ),
        (
            "Do you, now or in the future, require sponsorship to work in the country you currently reside in?*",
            Some("No"),
        ),
        ("Country of citizenship", Some("Chile")),
        ("Are you willing to relocate to Australia?", Some("Yes")),
    ];

    #[test]
    fn a_job_in_australia_is_answered_with_the_facts() {
        let bank = shipped_bank().with_australian_work_rights_answers();
        let wrong: Vec<String> = AUSTRALIA_WORK_RIGHTS
            .iter()
            .filter_map(|(label, want)| {
                let got = answer_of(&bank, label);
                (got.as_deref() != *want)
                    .then(|| format!("  {label:?}: want {want:?}, got {got:?}"))
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "wrong answers:
{}",
            wrong.join(
                "
"
            )
        );
    }

    #[test]
    fn the_australian_answers_never_touch_the_plain_bank() {
        let plain = shipped_bank();

        assert_eq!(
            answer_of(
                &plain,
                "Will you now or in the future require visa sponsorship?"
            ),
            Some("No".to_string()),
            "a remote job from Chile keeps the contractor's answer"
        );
        assert_eq!(
            plain.entries.len() + 7,
            plain.with_australian_work_rights_answers().entries.len()
        );
    }
}
