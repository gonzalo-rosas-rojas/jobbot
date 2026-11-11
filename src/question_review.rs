//! Moved to `jobbot_app::apply::question_review`. Re-exported here so every `crate::question_review::...` path
//! keeps working.
//! The tests left here read real forms through the sources adapter's parser.

pub use jobbot_app::apply::question_review::*;

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::answers::{AnswerBank, AnswerEntry, load_answers};
    use crate::paths::Paths;
    use crate::questions::{FieldKind, Question};
    use crate::questions::{QuestionOption, Section, parse_questions};

    fn fixture(name: &str) -> Vec<Question> {
        let text = match name {
            "databricks" => {
                include_str!("../crates/jobbot-sources/tests/fixtures/questions/databricks.json")
            }
            _ => include_str!("../crates/jobbot-sources/tests/fixtures/questions/datadog.json"),
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
