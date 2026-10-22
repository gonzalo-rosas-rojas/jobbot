//! The shipped `profile/answers.toml` against question shapes read from real Greenhouse boards
//! (`tests/fixtures/questions`): what the bank answers and what it leaves for Gonza.

use jobbot::answers::load_answers;
use jobbot::question_review::group_unanswered;
use jobbot::questions::{FieldKind, Question, QuestionOption, Section};

fn select(label: &str, options: &[&str]) -> Question {
    Question {
        section: Section::Application,
        label: label.to_string(),
        description: String::new(),
        field: "question_1".to_string(),
        kind: FieldKind::SingleSelect,
        required: true,
        options: options
            .iter()
            .enumerate()
            .map(|(i, label)| QuestionOption {
                label: label.to_string(),
                value: i.to_string(),
                free_form: false,
                decline: false,
            })
            .collect(),
    }
}

fn text(label: &str, required: bool) -> Question {
    Question {
        section: Section::Application,
        label: label.to_string(),
        description: String::new(),
        field: "question_2".to_string(),
        kind: FieldKind::Text,
        required,
        options: Vec::new(),
    }
}

fn unanswered_labels(questions: Vec<Question>) -> Vec<String> {
    let bank = load_answers(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("profile/answers.toml"),
    )
    .expect("the shipped bank loads");
    group_unanswered(&[("greenhouse:acme/1".to_string(), questions)], &bank)
        .groups
        .into_iter()
        .map(|group| group.sample_label)
        .collect()
}

#[test]
fn a_compound_residence_question_reaches_gonza_and_the_single_country_one_does_not() {
    let yes_no = ["Yes", "No"];
    let single = select(
        "Are you authorized to work in the country you currently reside in?",
        &yes_no,
    );
    let compound_authorization = "Are you authorized to work in the country where you currently reside and in the country in which this position is located?";
    let compound_sponsorship = "Do you require sponsorship to work in the country you currently reside in, or will you require sponsorship for the country of this role?";
    // Authorization in two countries has no one-word answer; sponsorship is "No" whichever
    // countries are named (Gonza works as a contractor from Chile, decided on 2026-10-04).
    let got = unanswered_labels(vec![
        single,
        select(compound_authorization, &yes_no),
        select(compound_sponsorship, &yes_no),
    ]);
    assert_eq!(got, vec![compound_authorization.to_string()]);
}

#[test]
fn a_field_that_only_mentions_relocating_is_not_given_the_relocation_prose() {
    let address = r#"What is the address from which you plan on working? If you would need to relocate, please type "relocating"."#;
    let both = "What is your current location and are you open to relocate?";
    let willing = text("Are you willing to relocate?", false);
    let mut got = unanswered_labels(vec![text(address, false), text(both, true), willing]);
    got.sort();
    let mut want = vec![address.to_string(), both.to_string()];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn greenhouse_residence_and_employer_labels_are_answered() {
    let open = unanswered_labels(vec![
        text(
            "Please select the country where you currently reside.",
            true,
        ),
        text("What is your current country of residence?", true),
        text(
            "Please indicate your current city, state, and country.",
            true,
        ),
        text("Who is your current or previous employer?", true),
        text("What is your current or previous job title?", true),
        select(
            "Voluntary Self-Identification of Gender",
            &["Male", "Female"],
        ),
        select(
            "Have you previously been employed by Coinbase in any capacity?",
            &["Yes", "No"],
        ),
        select("Have you ever worked at GitHub?", &["Yes", "No"]),
        select("Have you ever worked at Google?", &["Yes", "No"]),
        select(
            "Have you ever worked at Google, Meta or Microsoft?",
            &["Yes", "No"],
        ),
    ]);
    // Self-identification is left to the user in the example bank.
    assert_eq!(open, ["Voluntary Self-Identification of Gender"]);
}

#[test]
fn a_class_of_employers_is_left_to_gonza() {
    let asked = [
        "Have you ever worked for any government or public sector organization?",
        "Have you ever worked for one of our competitors?",
    ];
    let mut got = unanswered_labels(
        asked
            .iter()
            .map(|label| select(label, &["Yes", "No"]))
            .collect(),
    );
    got.sort();
    let mut want: Vec<String> = asked.iter().map(|label| label.to_string()).collect();
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn a_never_worked_here_option_answers_the_previous_employment_question() {
    // Robinhood's real wording: the options are sentences, not Yes/No.
    let robinhood = select(
        "Have you ever worked for Robinhood or any of its subsidiaries?",
        &[
            "I have never worked at Robinhood",
            "I currently work at Robinhood",
            "I have previously worked at Robinhood",
        ],
    );
    let plain = select("Have you ever worked for Acme before?", &["Yes", "No"]);
    assert_eq!(
        unanswered_labels(vec![robinhood, plain]),
        Vec::<String>::new()
    );
}
