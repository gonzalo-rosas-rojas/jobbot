//! Review of the questions of many jobs before any form is opened.
//!
//! The questions of each job come from [`crate::questions`]; the answer bank says which of them
//! the form filler can already answer. What is left is grouped by question, across jobs, so a
//! question that thirty jobs ask is answered once instead of thirty times. The rules are the
//! planner's own (`form::plan_fields`): the option matching, the `;` list of a multi-select, the
//! consent rule for checkbox groups, the essay rule, the resume label and the Hispanic rule come
//! from `crate::form`, not from a private copy, but are applied to the schema: nothing here opens a browser or touches
//! a form.

use std::sync::LazyLock;

use jobbot_domain::answers::{AnswerBank, AnswerEntry};
use jobbot_domain::form::{
    answer_entry, is_essay_box, is_resume_label, options_named, single_checkbox_label,
};
use jobbot_domain::questions::{FieldKind, Question, Section};
use regex::Regex;

static COVER_LETTER_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)cover\s+letter|carta\s+de\s+presentaci[oó]n").expect("valid cover regex")
});

/// The field name of the resume upload, and of the text box next to it.
const RESUME_FIELD: &str = "resume";
const RESUME_TEXT_FIELD: &str = "resume_text";
/// The field names of a cover letter start with this.
const COVER_LETTER_FIELD_PREFIX: &str = "cover_letter";
/// The `kind` `is_essay_box` is asked about: a schema has none (it is the page scan's `combobox`), only a tag and a label.
const NO_INPUT_TYPE: &str = "";

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
    /// [`jobbot_domain::questions::normalize_label`] of the question: the grouping key, with `kind`.
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewOptions {
    /// Count the EEOC and demographic questions that have a decline option apart
    /// (`Review::eeo_by_decline`) instead of listing them, as if a decline policy existed. It does
    /// not: the filler never declines, so this is off by default and those questions are listed as
    /// open, which is what they are on a real form.
    pub eeo_decline: bool,
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
        question.field.starts_with(COVER_LETTER_FIELD_PREFIX) || COVER_LETTER_LABEL.is_match(label);
    match question.kind {
        FieldKind::File => {
            if question.field == RESUME_FIELD || is_resume_label(label) {
                Verdict::Answered
            } else if question.required {
                Verdict::Open(BankSays::Unknown)
            } else {
                Verdict::Ignored
            }
        }
        // The text box next to the resume upload: the CV answers it too.
        _ if question.field == RESUME_TEXT_FIELD => Verdict::Answered,
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
            let entry = answer_entry(bank, label, is_essay_box(tag, NO_INPUT_TYPE, label));
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
    use jobbot_domain::answers::{AnswerBank, AnswerEntry};
    use jobbot_domain::questions::{QuestionOption, Section};

    use super::*;

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

    /// A bank whose only answer is the race `White`, written for the whole label.
    fn race_bank() -> AnswerBank {
        AnswerBank {
            entries: vec![AnswerEntry::new(&["=race"], Some("White"))],
            consent_phrases: Vec::new(),
        }
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
    fn an_option_that_says_not_of_hispanic_origin_never_answers_the_race_question() {
        let bank = race_bank();
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

        let review = group_unanswered_with(
            &one(vec![gender]),
            &bank(),
            ReviewOptions { eeo_decline: true },
        );

        assert_eq!(review.eeo_by_decline, 0);
        assert_eq!(review.groups.len(), 1);
    }

    fn gender_with_a_decline_option() -> Question {
        let mut gender = question(
            "Gender",
            FieldKind::SingleSelect,
            true,
            &["Female", "Male", "Decline to self-identify"],
        );
        gender.section = Section::Eeoc;
        gender.options[2].decline = true;
        gender
    }

    #[test]
    fn by_default_an_eeo_question_with_a_decline_option_is_open_because_the_filler_never_declines()
    {
        let review = group_unanswered(&one(vec![gender_with_a_decline_option()]), &bank());

        assert_eq!(review.eeo_by_decline, 0);
        assert_eq!(review.groups.len(), 1);
    }

    #[test]
    fn with_the_decline_policy_on_that_question_is_counted_apart() {
        let review = group_unanswered_with(
            &one(vec![gender_with_a_decline_option()]),
            &bank(),
            ReviewOptions { eeo_decline: true },
        );

        assert_eq!(review.eeo_by_decline, 1);
        assert!(review.groups.is_empty());
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
    fn an_essay_the_bank_does_not_know_stays_open() {
        let review = group_unanswered(
            &one(vec![question(
                "Why are you interested in Acme?",
                FieldKind::Textarea,
                true,
                &[],
            )]),
            &bank(),
        );

        assert_eq!(review.groups.len(), 1);
        assert_eq!(review.groups[0].bank_says, BankSays::Unknown);
        assert_eq!(review.groups[0].kind, FieldKind::Textarea);
    }

    #[test]
    fn a_select_without_options_takes_the_bank_value() {
        let review = group_unanswered(
            &one(vec![question(
                "How did you hear about us?",
                FieldKind::SingleSelect,
                true,
                &[],
            )]),
            &bank(),
        );

        assert!(review.groups.is_empty());
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
    fn a_race_list_with_a_hispanic_option_is_answered_only_when_it_is_multi_select() {
        let bank = race_bank();
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
}
