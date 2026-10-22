//! Review of the questions of many jobs before any form is opened.
//!
//! The questions of each job come from [`crate::questions`]; the answer bank says which of them
//! the form filler can already answer. What is left is grouped by question, across jobs, so a
//! question that thirty jobs ask is answered once instead of thirty times. The rules are the
//! planner's own (`form::plan_fields`): the option matching, the `;` list of a multi-select, the
//! consent rule for checkbox groups, the essay rule and the Hispanic rule come from `crate::form`,
//! not from a private copy, but are applied to the schema: nothing here opens a browser or touches
//! a form.

use std::sync::LazyLock;

use regex::Regex;

use crate::answers::{AnswerBank, AnswerEntry};
use crate::form::{answer_entry, is_essay_box, options_named, single_checkbox_label};
use crate::questions::{FieldKind, Question, Section};

/// The same test `form::plan_fields` uses to tell a resume input from any other file.
static RESUME_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)r[eé]sum[eé]|\bcv\b|curr[ií]culum").expect("valid resume regex")
});

static COVER_LETTER_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)cover\s+letter|carta\s+de\s+presentaci[oó]n").expect("valid cover regex")
});

/// How many option labels a group keeps as a sample.
pub const MAX_OPTIONS: usize = 12;

/// Why the bank leaves a question open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankSays {
    /// An entry matches the label but has no value: Gonza decides this one himself.
    Silent,
    /// An entry has an answer, but none of its candidates is one of the form's options.
    NoMatchingOption,
    /// No entry matches the label.
    Unknown,
    /// An entry has an answer, but the question is an essay (a textarea, or a long label): only
    /// an entry written for its whole label (an `=` pattern) answers one, so a keyword does not.
    Essay,
}

/// One question that at least one job asks and nothing answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnansweredGroup {
    /// [`crate::questions::normalize_label`] of the question: the grouping key, with `kind`.
    pub normalized_label: String,
    /// The label as the first job words it.
    pub sample_label: String,
    pub kind: FieldKind,
    /// True when any of the jobs requires it.
    pub required: bool,
    /// Up to [`MAX_OPTIONS`] option labels, from the first job that asks it.
    pub options: Vec<String>,
    /// How many options that first job has.
    pub option_count: usize,
    /// The keys of the jobs that ask it and are left without an answer, in input order.
    pub offers: Vec<String>,
    /// What the bank says about the first of those jobs.
    pub bank_says: BankSays,
}

/// The outcome of reviewing a batch of jobs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Review {
    /// Required first, then the most asked, then by label.
    pub groups: Vec<UnansweredGroup>,
    /// Questions the bank (or the CV, or a consent phrase) already answers.
    pub answered: usize,
    /// EEOC and demographic questions with a decline option that the review set apart (left out of
    /// `groups`) because `ReviewOptions::eeo_decline` is on. Informational only: these would be
    /// covered IF a decline policy existed. The filler does not decline today (it never picks a
    /// decline option), so on a real form each of them is still open.
    pub eeo_by_decline: usize,
    /// Hidden fields and optional files that nobody answers.
    pub ignored: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewOptions {
    /// Count the EEOC and demographic questions that have a decline option apart
    /// (`Review::eeo_by_decline`) instead of listing them, as if a decline policy existed. It does
    /// not: the filler never declines, so turn this off to see those questions as open.
    pub eeo_decline: bool,
}

impl Default for ReviewOptions {
    fn default() -> Self {
        Self { eeo_decline: true }
    }
}

/// [`group_unanswered_with`] with the default options.
pub fn group_unanswered(offers: &[(String, Vec<Question>)], bank: &AnswerBank) -> Review {
    group_unanswered_with(offers, bank, ReviewOptions::default())
}

/// Groups what the bank cannot answer. `offers` pairs a job key (`greenhouse:board/id`) with its
/// questions.
pub fn group_unanswered_with(
    offers: &[(String, Vec<Question>)],
    bank: &AnswerBank,
    options: ReviewOptions,
) -> Review {
    let mut review = Review::default();
    for (key, questions) in offers {
        for question in questions {
            match classify(question, bank, options) {
                Verdict::Answered => review.answered += 1,
                Verdict::Ignored => review.ignored += 1,
                Verdict::EeoDecline => review.eeo_by_decline += 1,
                Verdict::Open(says) => add_to_groups(&mut review.groups, key, question, says),
            }
        }
    }
    review.groups.sort_by(|a, b| {
        (!a.required)
            .cmp(&!b.required)
            .then(b.offers.len().cmp(&a.offers.len()))
            .then_with(|| a.normalized_label.cmp(&b.normalized_label))
    });
    review
}

/// What happens to one field.
enum Verdict {
    /// The bank, the CV or a consent phrase answers it.
    Answered,
    /// Nobody answers it and nobody needs to: a hidden field or an optional file.
    Ignored,
    /// Would be covered if a decline policy existed (informational: the filler does not decline).
    EeoDecline,
    /// Nothing answers it.
    Open(BankSays),
}

fn classify(question: &Question, bank: &AnswerBank, options: ReviewOptions) -> Verdict {
    if !question.is_visible() {
        return Verdict::Ignored;
    }
    let label = question.label.as_str();
    let is_eeo = matches!(question.section, Section::Eeoc | Section::Demographic);
    if is_eeo && options.eeo_decline && question.decline_option().is_some() {
        return Verdict::EeoDecline;
    }
    let is_cover_letter =
        question.field.starts_with("cover_letter") || COVER_LETTER_LABEL.is_match(label);
    match question.kind {
        FieldKind::File => {
            if question.field == "resume" || RESUME_LABEL.is_match(label) {
                Verdict::Answered
            } else if question.required {
                Verdict::Open(BankSays::Unknown)
            } else {
                Verdict::Ignored
            }
        }
        // The text box next to the resume upload: the CV answers it too.
        _ if question.field == "resume_text" => Verdict::Answered,
        _ if is_cover_letter && !question.required => Verdict::Ignored,
        // A multi-select with at most one option is a lone checkbox on the page, and the planner
        // ticks a lone checkbox only for a consent phrase: the bank's answers play no part.
        FieldKind::MultiSelect if question.options.len() <= 1 => {
            if is_consent(question, bank) {
                Verdict::Answered
            } else {
                Verdict::Open(silent_or_unknown(bank.lookup(label)))
            }
        }
        FieldKind::SingleSelect | FieldKind::MultiSelect => {
            let entry = bank.lookup(label);
            let candidates = entry.map(AnswerEntry::candidates).unwrap_or_default();
            let labels: Vec<String> = question
                .options
                .iter()
                .map(|option| option.label.clone())
                .collect();
            let several = question.kind == FieldKind::MultiSelect;
            if candidates.is_empty() {
                Verdict::Open(silent_or_unknown(entry))
            } else if labels.is_empty() || !options_named(&labels, &candidates, several).is_empty()
            {
                Verdict::Answered
            } else {
                Verdict::Open(BankSays::NoMatchingOption)
            }
        }
        _ => {
            let tag = if question.kind == FieldKind::Textarea {
                "textarea"
            } else {
                "input"
            };
            let found = bank.lookup(label);
            let entry = answer_entry(bank, label, is_essay_box(tag, "", label));
            let has_answer = |entry: Option<&AnswerEntry>| {
                entry.is_some_and(|entry| !entry.candidates().is_empty())
            };
            if has_answer(entry) {
                Verdict::Answered
            } else if has_answer(found) {
                // The bank has a value, but a keyword does not answer an essay.
                Verdict::Open(BankSays::Essay)
            } else {
                Verdict::Open(silent_or_unknown(found))
            }
        }
    }
}

fn silent_or_unknown(entry: Option<&AnswerEntry>) -> BankSays {
    if entry.is_some() {
        BankSays::Silent
    } else {
        BankSays::Unknown
    }
}

/// The planner's consent rule for a checkbox that stands alone: it is ticked only for a consent
/// phrase Gonza approved, judged on the text the page scan gives it (the question and its one
/// option together, so a refusal word in either half blocks it). A group with several options is
/// never ticked for consent: the question says nothing about it.
fn is_consent(question: &Question, bank: &AnswerBank) -> bool {
    match question.options.as_slice() {
        [] => bank.is_consent(&question.label),
        [only] => bank.is_consent(&single_checkbox_label(&question.label, &only.label)),
        _ => false,
    }
}

fn add_to_groups(
    groups: &mut Vec<UnansweredGroup>,
    key: &str,
    question: &Question,
    says: BankSays,
) {
    let normalized = question.normalized_label();
    if let Some(group) = groups
        .iter_mut()
        .find(|group| group.normalized_label == normalized && group.kind == question.kind)
    {
        group.required |= question.required;
        if !group.offers.iter().any(|offer| offer == key) {
            group.offers.push(key.to_string());
        }
        return;
    }
    groups.push(UnansweredGroup {
        normalized_label: normalized,
        sample_label: question.label.clone(),
        kind: question.kind.clone(),
        required: question.required,
        options: question
            .options
            .iter()
            .take(MAX_OPTIONS)
            .map(|option| option.label.clone())
            .collect(),
        option_count: question.options.len(),
        offers: vec![key.to_string()],
        bank_says: says,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answers::{AnswerBank, AnswerEntry, load_answers};
    use crate::paths::Paths;
    use crate::questions::{QuestionOption, Section, parse_questions};
    use serde_json::Value;

    fn fixture(name: &str) -> Vec<Question> {
        let text = match name {
            "databricks" => include_str!("../tests/fixtures/questions/databricks.json"),
            _ => include_str!("../tests/fixtures/questions/datadog.json"),
        };
        parse_questions(&serde_json::from_str::<Value>(text).unwrap())
    }

    fn offers(names: &[&str]) -> Vec<(String, Vec<Question>)> {
        names
            .iter()
            .map(|name| (format!("greenhouse:{name}"), fixture(name)))
            .collect()
    }

    /// A small bank with known answers so the tests do not depend on `profile/answers.toml`.
    fn bank() -> AnswerBank {
        AnswerBank {
            entries: vec![
                AnswerEntry::new(&["sponsorship"], None),
                AnswerEntry::new(&["linkedin"], None),
                AnswerEntry::new(&["legally authorized to work"], Some("Yes")),
                AnswerEntry::new(
                    &["legally authorised to work"],
                    Some("Yes, no restriction."),
                ),
                AnswerEntry::new(&["how did you hear"], Some("Carrier pigeon")),
                AnswerEntry::new(&["first name"], Some("Jane")),
                AnswerEntry::new(&["last name"], Some("Doe")),
                AnswerEntry::new(&["=email"], Some("me@example.com")),
                AnswerEntry::new(&["=phone"], Some("+56 9 0000 0000")),
            ],
            consent_phrases: vec!["consent to the processing".to_string()],
        }
    }

    fn group<'a>(review: &'a Review, label_start: &str) -> &'a UnansweredGroup {
        review
            .groups
            .iter()
            .find(|group| group.normalized_label.starts_with(label_start))
            .unwrap_or_else(|| panic!("no group starting with {label_start:?}"))
    }

    fn question(label: &str, kind: FieldKind, required: bool, options: &[&str]) -> Question {
        Question {
            section: Section::Application,
            label: label.to_string(),
            description: String::new(),
            field: "question_1".to_string(),
            kind,
            required,
            options: options
                .iter()
                .map(|label| QuestionOption {
                    label: label.to_string(),
                    value: label.to_string(),
                    free_form: false,
                    decline: false,
                })
                .collect(),
        }
    }

    fn one(questions: Vec<Question>) -> Vec<(String, Vec<Question>)> {
        vec![("greenhouse:acme/1".to_string(), questions)]
    }

    #[test]
    fn a_real_job_leaves_only_what_the_bank_cannot_answer() {
        let review = group_unanswered(&offers(&["databricks"]), &bank());

        // 24 fields: 8 answered, 4 ignored, 3 EEOC, 9 groups.
        assert_eq!(
            review.answered, 8,
            "name, email, phone, resume x2, authorization"
        );
        assert_eq!(
            review.ignored, 4,
            "two hidden coordinates and the optional cover letter x2"
        );
        assert_eq!(review.eeo_by_decline, 3);
        assert_eq!(review.groups.len(), 9);
        assert!(review.groups.iter().all(|group| group.required));
        assert_eq!(group(&review, "linkedin").bank_says, BankSays::Silent);
        assert_eq!(
            group(&review, "do you now or will you").bank_says,
            BankSays::Silent
        );
        assert_eq!(
            group(&review, "current company").bank_says,
            BankSays::Unknown
        );
        assert_eq!(group(&review, "location").kind, FieldKind::Text);
    }

    #[test]
    fn nothing_the_bank_answers_is_listed_and_every_field_is_counted_once() {
        let jobs = offers(&["databricks", "datadog"]);
        let total: usize = jobs.iter().map(|(_, questions)| questions.len()).sum();

        let review = group_unanswered(&jobs, &bank());

        let listed: usize = review.groups.iter().map(|group| group.offers.len()).sum();
        assert_eq!(
            review.answered + review.ignored + review.eeo_by_decline + listed,
            total
        );
        for group in &review.groups {
            let candidates = bank()
                .lookup(&group.sample_label)
                .map(AnswerEntry::candidates)
                .unwrap_or_default();
            assert!(
                candidates.is_empty() || group.bank_says == BankSays::NoMatchingOption,
                "{}",
                group.sample_label
            );
        }
    }

    #[test]
    fn the_same_question_in_two_jobs_is_one_group_with_both_offers() {
        let review = group_unanswered(&offers(&["databricks", "datadog"]), &bank());

        let linkedin = group(&review, "linkedin");
        assert_eq!(
            linkedin.offers,
            ["greenhouse:databricks", "greenhouse:datadog"]
        );
        assert!(
            linkedin.required,
            "databricks requires it, datadog does not"
        );
        assert_eq!(linkedin.bank_says, BankSays::Silent);
        assert_eq!(review.groups[0].normalized_label, "linkedin profile");
    }

    #[test]
    fn groups_come_required_first_then_most_asked_then_by_label() {
        let review = group_unanswered(&offers(&["databricks", "datadog"]), &bank());

        let keys: Vec<(bool, std::cmp::Reverse<usize>, &str)> = review
            .groups
            .iter()
            .map(|group| {
                (
                    !group.required,
                    std::cmp::Reverse(group.offers.len()),
                    group.normalized_label.as_str(),
                )
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert!(
            review.groups.iter().any(|group| !group.required),
            "datadog's website is optional"
        );
        assert!(!group(&review, "website").required);
    }

    #[test]
    fn a_bank_answer_that_is_not_among_the_options_is_not_enough() {
        let review = group_unanswered(&offers(&["datadog"]), &bank());

        let heard = group(&review, "how did you hear");
        assert_eq!(heard.bank_says, BankSays::NoMatchingOption);
        assert_eq!(heard.kind, FieldKind::SingleSelect);
        assert_eq!(heard.option_count, 35);
        assert_eq!(heard.options.len(), MAX_OPTIONS);
        // The authorization question has the bank's answer as its first option: answered.
        assert!(!review.groups.iter().any(|group| {
            group
                .normalized_label
                .starts_with("are you legally authorised")
        }));
    }

    #[test]
    fn demographic_questions_with_a_decline_option_are_counted_apart() {
        let review = group_unanswered(&offers(&["datadog"]), &bank());

        assert_eq!(review.eeo_by_decline, 1);
        assert!(
            !review
                .groups
                .iter()
                .any(|group| group.normalized_label.contains("self-identification"))
        );
    }

    #[test]
    fn without_the_decline_policy_the_eeoc_questions_are_listed() {
        let off = ReviewOptions { eeo_decline: false };

        let review = group_unanswered_with(&offers(&["databricks"]), &bank(), off);

        assert_eq!(review.eeo_by_decline, 0);
        assert_eq!(review.groups.len(), 12);
        assert!(!group(&review, "race").required);
    }

    #[test]
    fn an_option_that_says_not_of_hispanic_origin_never_answers_the_race_question() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["=race"], Some("White"))],
            consent_phrases: Vec::new(),
        };
        let race = question(
            "Race",
            FieldKind::SingleSelect,
            true,
            &["White (not of Hispanic origin)", "Hispanic or Latino"],
        );

        let review = group_unanswered(&one(vec![race]), &bank);

        assert_eq!(review.answered, 0);
        assert_eq!(review.groups.len(), 1);
    }

    #[test]
    fn an_eeo_question_without_a_decline_option_is_listed_even_with_the_policy() {
        let mut gender = question("Gender", FieldKind::SingleSelect, true, &["Female", "Male"]);
        gender.section = Section::Eeoc;

        let review = group_unanswered(&one(vec![gender]), &bank());

        assert_eq!(review.eeo_by_decline, 0);
        assert_eq!(review.groups.len(), 1);
    }

    #[test]
    fn the_resume_is_answered_by_the_cv_and_other_files_are_not() {
        let review = group_unanswered(
            &one(vec![
                question("Resume/CV", FieldKind::File, true, &[]),
                Question {
                    field: "resume_text".to_string(),
                    ..question("Resume/CV", FieldKind::Textarea, true, &[])
                },
                question("Cover Letter", FieldKind::File, false, &[]),
                question("Cover Letter", FieldKind::Textarea, false, &[]),
                question("Portfolio PDF", FieldKind::File, true, &[]),
                question("Writing sample", FieldKind::File, false, &[]),
            ]),
            &bank(),
        );

        assert_eq!(review.answered, 2);
        assert_eq!(
            review.ignored, 3,
            "optional cover letter x2 and the optional sample"
        );
        assert_eq!(review.groups.len(), 1);
        assert_eq!(review.groups[0].sample_label, "Portfolio PDF");
        assert_eq!(review.groups[0].kind, FieldKind::File);
    }

    #[test]
    fn a_required_cover_letter_is_listed() {
        let review = group_unanswered(
            &one(vec![question("Cover Letter", FieldKind::File, true, &[])]),
            &bank(),
        );

        assert_eq!(review.groups.len(), 1);
        assert_eq!(review.ignored, 0);
    }

    #[test]
    fn hidden_fields_are_ignored() {
        let review = group_unanswered(
            &one(vec![question("Latitude", FieldKind::Hidden, true, &[])]),
            &bank(),
        );

        assert_eq!(review.ignored, 1);
        assert!(review.groups.is_empty());
    }

    #[test]
    fn an_essay_the_bank_does_not_know_is_unknown_and_a_select_without_options_trusts_the_bank() {
        let review = group_unanswered(
            &one(vec![
                question(
                    "Why are you interested in Acme?",
                    FieldKind::Textarea,
                    true,
                    &[],
                ),
                question(
                    "How did you hear about us?",
                    FieldKind::SingleSelect,
                    true,
                    &[],
                ),
            ]),
            &bank(),
        );

        assert_eq!(review.groups.len(), 1);
        assert_eq!(review.groups[0].bank_says, BankSays::Unknown);
        assert_eq!(review.groups[0].kind, FieldKind::Textarea);
        assert_eq!(
            review.answered, 1,
            "no options to check: the bank value is taken"
        );
    }

    #[test]
    fn a_multi_select_is_answered_by_a_consent_phrase_or_a_matching_option() {
        let review = group_unanswered(
            &one(vec![
                question(
                    "I consent to the processing of my personal data",
                    FieldKind::MultiSelect,
                    true,
                    &["I agree"],
                ),
                question(
                    "I agree to marketing emails and I consent to the processing of my data",
                    FieldKind::MultiSelect,
                    true,
                    &["I agree"],
                ),
            ]),
            &bank(),
        );

        assert_eq!(review.answered, 1);
        assert_eq!(review.groups.len(), 1);
        assert!(review.groups[0].sample_label.contains("marketing"));
    }

    #[test]
    fn grouping_ignores_case_spacing_and_trailing_marks_but_not_the_kind() {
        let a = vec![question("Current  Company *", FieldKind::Text, true, &[])];
        let b = vec![question("current company:", FieldKind::Text, true, &[])];
        let c = vec![question("Current company", FieldKind::Textarea, true, &[])];
        let jobs = vec![
            ("greenhouse:a/1".to_string(), a),
            ("greenhouse:b/2".to_string(), b),
            ("greenhouse:c/3".to_string(), c),
        ];

        let review = group_unanswered(&jobs, &bank());

        assert_eq!(review.groups.len(), 2);
        assert_eq!(
            review.groups[0].offers,
            ["greenhouse:a/1", "greenhouse:b/2"]
        );
        assert_eq!(review.groups[0].sample_label, "Current  Company *");
        assert_eq!(review.groups[0].normalized_label, "current company");
    }

    #[test]
    fn a_job_that_asks_one_question_twice_is_listed_once() {
        let twice = vec![
            question("Current Company", FieldKind::Text, true, &[]),
            question("Current Company", FieldKind::Text, true, &[]),
        ];

        let review = group_unanswered(&one(twice), &bank());

        assert_eq!(review.groups.len(), 1);
        assert_eq!(review.groups[0].offers, ["greenhouse:acme/1"]);
    }

    #[test]
    fn an_empty_batch_is_an_empty_review() {
        assert_eq!(group_unanswered(&[], &bank()), Review::default());
        assert_eq!(
            group_unanswered(&[("greenhouse:a/1".to_string(), vec![])], &bank()),
            Review::default()
        );
    }

    fn review_of(questions: Vec<Question>, bank: &AnswerBank) -> Review {
        group_unanswered(&one(questions), bank)
    }

    #[test]
    fn a_place_matches_the_option_that_names_all_its_parts_like_the_filler_does() {
        // The filler reads "Santiago, Chile" as "Santiago, Metropolitana, Chile"; the review
        // used to call that a bank answer with no matching option.
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(
                &["=location city"],
                Some("Santiago, Chile"),
            )],
            consent_phrases: Vec::new(),
        };
        let city =
            |options: &[&str]| question("Location City", FieldKind::SingleSelect, true, options);
        let review = review_of(vec![city(&["Santiago, Metropolitana, Chile"])], &bank);
        assert_eq!((review.answered, review.groups.len()), (1, 0));
        let review = review_of(vec![city(&["Santiago, Dominican Republic"])], &bank);
        assert_eq!(review.groups.len(), 1);
        assert_eq!(review.groups[0].bank_says, BankSays::NoMatchingOption);
    }

    #[test]
    fn a_multi_select_answer_is_a_list_separated_by_semicolons() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["which languages"], Some("Python; Rust"))],
            consent_phrases: Vec::new(),
        };
        let languages = |options: &[&str]| {
            question(
                "Which languages do you use?",
                FieldKind::MultiSelect,
                true,
                options,
            )
        };
        let review = review_of(vec![languages(&["Go", "Rust"])], &bank);
        assert_eq!(review.answered, 1, "Rust is one of the listed options");
        let review = review_of(vec![languages(&["Go", "Java"])], &bank);
        assert_eq!(review.groups[0].bank_says, BankSays::NoMatchingOption);
    }

    #[test]
    fn a_consent_phrase_in_a_group_of_several_options_answers_nothing() {
        // The planner ticks a group only from a bank answer: for a group with several options the
        // allow list plays no part, whether the phrase is in the question or in an option.
        let bank = bank();
        let by_question = question(
            "Review our consent to the processing notice, then choose how we may reach you",
            FieldKind::MultiSelect,
            true,
            &["Email", "SMS"],
        );
        let by_option = question(
            "How may we reach you?",
            FieldKind::MultiSelect,
            true,
            &["Email", "I consent to the processing of my data"],
        );
        let review = review_of(vec![by_question, by_option], &bank);
        assert_eq!(review.answered, 0);
        assert_eq!(review.groups.len(), 2);
        // One lone option is judged together with its question, as the page scan labels it.
        let lone = question(
            "Applicant consent to the processing notice",
            FieldKind::MultiSelect,
            true,
            &["Acknowledge"],
        );
        let review = review_of(vec![lone], &bank);
        assert_eq!(review.answered, 1);
    }

    #[test]
    fn a_single_choice_list_with_a_hispanic_option_is_never_answered_by_a_race() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["=race"], Some("White"))],
            consent_phrases: Vec::new(),
        };
        let options = ["White", "Asian", "Hispanic or Latino"];
        let single = question("Race", FieldKind::SingleSelect, true, &options);
        let review = review_of(vec![single], &bank);
        assert_eq!(review.answered, 0);
        assert_eq!(review.groups.len(), 1);
        // Mark-all-that-apply: White alone is ticked, so the question is answered.
        let multi = question("Race", FieldKind::MultiSelect, true, &options);
        let review = review_of(vec![multi], &bank);
        assert_eq!(review.answered, 1);
    }

    #[test]
    fn a_bare_no_does_not_guess_between_two_qualified_options() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["work location"], Some("No"))],
            consent_phrases: Vec::new(),
        };
        let ask = |options: &[&str]| {
            question(
                "Is your work location in the US",
                FieldKind::SingleSelect,
                true,
                options,
            )
        };
        let review = review_of(
            vec![ask(&[
                "Yes",
                "No, but I would relocate",
                "No, and I would not",
            ])],
            &bank,
        );
        assert_eq!(review.groups[0].bank_says, BankSays::NoMatchingOption);
        let review = review_of(vec![ask(&["Yes", "No, I would not relocate"])], &bank);
        assert_eq!(review.answered, 1);
    }

    #[test]
    fn an_essay_is_answered_only_by_an_entry_written_for_its_whole_label() {
        let bank = AnswerBank {
            entries: vec![
                AnswerEntry::new(&["salary"], Some("USD 6,000 per month")),
                AnswerEntry::new(&["=compensation expectations"], Some("USD 6,000 per month")),
            ],
            consent_phrases: Vec::new(),
        };
        let essay = question(
            "Describe a project you are proud of and include your salary expectations",
            FieldKind::Textarea,
            true,
            &[],
        );
        let exact = question("Compensation Expectations", FieldKind::Textarea, true, &[]);
        let short = question("Salary expectations?", FieldKind::Text, true, &[]);
        let review = review_of(vec![essay, exact, short], &bank);
        assert_eq!(review.answered, 2);
        assert_eq!(review.groups.len(), 1);
        assert_eq!(review.groups[0].bank_says, BankSays::Essay);
    }

    #[test]
    fn the_shipped_bank_reviews_the_real_jobs_without_listing_what_it_answers() {
        let path = Paths::new(env!("CARGO_MANIFEST_DIR"))
            .profile_dir
            .join("answers.toml");
        let shipped = load_answers(&path).unwrap();

        let review = group_unanswered(&offers(&["databricks", "datadog"]), &shipped);

        assert!(review.answered > 0);
        for group in &review.groups {
            let candidates = shipped
                .lookup(&group.sample_label)
                .map(AnswerEntry::candidates)
                .unwrap_or_default();
            assert!(
                candidates.is_empty() || group.bank_says == BankSays::NoMatchingOption,
                "{}",
                group.sample_label
            );
        }
    }
}
