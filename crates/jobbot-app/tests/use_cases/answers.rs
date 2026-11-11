//! The `answers` use cases against the real SQLite stores and the fake Claude: the matching
//! rules of the answer bank on made-up banks (moved from the root crate's `src/answers.rs`). What
//! the shipped bank itself must answer is audited in `tests/profile_audit.rs`.

use jobbot_app::profile::load_answers;
use jobbot_domain::answers::*;

fn value_of(bank: &AnswerBank, label: &str) -> Option<String> {
    bank.lookup(label)
        .unwrap_or_else(|| panic!("no entry for {label}"))
        .value
        .clone()
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
    let entry = AnswerEntry::new(&["hear about"], Some("Company careers page"))
        .with_alternatives(&["Job board", "Other"]);
    assert_eq!(
        entry.candidates(),
        vec!["Company careers page", "Job board", "Other"]
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
fn a_job_in_australia_keeps_the_banks_answers_for_the_country_the_candidate_lives_in() {
    let bank = bank_of(&[
        (&["authorized to work|authorised to work"], "Yes"),
        (&["sponsorship"], "No"),
    ])
    .with_australian_work_rights_answers();
    for (label, want) in [
        ("Are you legally authorized to work in Australia?", "No"),
        (
            "Are you legally authorized to work in the country you reside in?",
            "Yes",
        ),
        (
            "Are you authorised to work in the country you are currently located in?",
            "Yes",
        ),
        (
            "Will you require sponsorship to remain in Australia?",
            "Yes",
        ),
        (
            "Will you require sponsorship to remain in your current location?",
            "No",
        ),
    ] {
        assert_eq!(value_of(&bank, label).as_deref(), Some(want), "{label}");
    }
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
