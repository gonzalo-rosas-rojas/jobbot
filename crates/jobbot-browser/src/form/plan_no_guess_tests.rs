//! The plan of a fill where the answer is not plain to see, and the plan does not guess: essay
//! boxes, bare yes/no answers, race lists that offer Hispanic or Latino and boxes the page ticked
//! itself.

use super::support_tests::{
    bank, checkbox_group, field, pay_and_start_bank, race_bank, radio, texts,
};
use super::*;
use jobbot_domain::answers::AnswerEntry;

const ESSAYS: [&str; 3] = [
    "Describe a project you are proud of and include your salary expectations",
    "Tell us why you are a good fit. Please also include your salary expectations and notice period.",
    "Additional Information (please include your location, notice period and salary expectations)",
];

#[test]
fn an_essay_box_never_gets_a_one_line_value_found_by_a_keyword() {
    // "salary" is in each question, but each one asks for far more than a salary: the bank's
    // "USD 6,000 per month" would answer only a corner of it. Required: Gonza answers.
    // Optional: left alone.
    for required in [true, false] {
        for label in ESSAYS {
            let fields = [field(0, "textarea", "", label, required)];
            let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
            assert!(plan.actions.is_empty(), "{label}: {:?}", plan.actions);
            let want: Vec<&str> = if required { vec![label] } else { vec![] };
            assert_eq!(plan.missing, want, "{label}");
        }
    }
}

#[test]
fn the_shipped_bank_answers_a_salary_text_box_by_its_whole_label_and_no_essay() {
    let bank = crate::test_support::load_bank(
        &crate::test_support::workspace_root()
            .join("profile")
            .join("answers.toml"),
    )
    .unwrap();
    for label in [
        "Salary expectations",
        "Compensation Expectations*",
        "What are your salary expectations?",
        "Expected salary",
        "Desired salary",
        "Salary requirements",
    ] {
        let fields = [field(0, "textarea", "", label, true)];
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert!(plan.missing.is_empty(), "{label}: {:?}", plan.missing);
        assert_eq!(
            plan.actions[0].value,
            FillValue::Text("USD 6,000 per month".into()),
            "{label}"
        );
    }
    for label in ESSAYS {
        let fields = [field(0, "textarea", "", label, true)];
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{label}: {:?}", plan.actions);
    }
}

#[test]
fn an_essay_box_is_answered_by_an_entry_written_for_its_whole_label() {
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(
            &["=compensation expectations"],
            Some("USD 6,000 per month"),
        )],
        consent_phrases: vec![],
    };
    let fields = [field(0, "textarea", "", "Compensation Expectations*", true)];
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert!(plan.missing.is_empty(), "{:?}", plan.missing);
    assert_eq!(
        plan.actions[0].value,
        FillValue::Text("USD 6,000 per month".into())
    );
    // The exact pattern does not stretch to a longer question that merely starts the same way.
    let longer = [field(
        0,
        "textarea",
        "",
        "Compensation Expectations and notice period",
        true,
    )];
    let plan = plan_fields(&longer, &bank, Path::new("cv.pdf"));
    assert!(plan.actions.is_empty());
    assert_eq!(
        plan.missing,
        vec!["Compensation Expectations and notice period"]
    );
}

#[test]
fn a_long_label_is_answered_in_a_one_line_box_only_by_its_exact_entry() {
    let twelve = ESSAYS[0];
    assert_eq!(twelve.split_whitespace().count(), 12);
    let short = "What are your salary expectations for this role in USD?";
    for label in [twelve, ESSAYS[1]] {
        let fields = [field(0, "input", "text", label, true)];
        let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{label}: {:?}", plan.actions);
        assert_eq!(plan.missing, vec![label], "{label}");
    }
    let fields = [field(0, "input", "text", short, true)];
    let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
    assert_eq!(plan.actions.len(), 1, "a short question is still answered");
    // A drop-down or a Yes/No group can only take one of its own options, so a long label
    // cannot make it carry extra claims: those are still answered.
    let long_yes_no =
        "Are you legally authorized to work in the country where you are applying today?";
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(&["legally authorized"], Some("Yes"))],
        consent_phrases: vec![],
    };
    let fields = [
        field(0, "select", "", long_yes_no, true),
        field(1, "input", "combobox", long_yes_no, true),
        radio(2, "auth", long_yes_no, "Yes", true),
        radio(3, "auth", long_yes_no, "No", true),
    ];
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    let answered: Vec<usize> = plan.actions.iter().map(|action| action.idx).collect();
    assert_eq!(answered, vec![0, 1, 2]);
}

fn relocation_options() -> Vec<String> {
    texts(&[
        "Yes",
        "No, but I am willing to relocate to the US",
        "No, and I am not willing to relocate",
    ])
}

#[test]
fn a_bare_yes_no_takes_a_qualified_option_only_when_it_is_the_only_one() {
    let options = relocation_options();
    assert_eq!(best_option(&options, "Yes"), Some(0));
    // Two options start with "No": the one Gonza would have to be speaking for is not guessed.
    assert_eq!(best_option(&options, "No"), None);
    // The exact option comes first wherever it is.
    let with_exact = texts(&["No, but I am willing to relocate", "No", "No, and I am not"]);
    assert_eq!(best_option(&with_exact, "No"), Some(1));
    // One qualified option is the only reading of the answer.
    let sponsor = texts(&["Yes, I need sponsorship", "No, I do not need sponsorship"]);
    assert_eq!(best_option(&sponsor, "No"), Some(1));
    assert_eq!(best_option(&sponsor, "Yes"), Some(0));
    // A longer answer is not a bare yes/no: the first prefix still wins.
    let thanks = texts(&["No, thanks a lot", "No, thanks anyway"]);
    assert_eq!(best_option(&thanks, "No, thanks"), Some(0));
}

#[test]
fn a_radio_group_does_not_guess_between_two_no_options() {
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(
            &["work location in the united states"],
            Some("No"),
        )],
        consent_phrases: vec![],
    };
    let question = "Is your work location in the United States";
    for required in [true, false] {
        let fields: Vec<FieldInfo> = relocation_options()
            .iter()
            .enumerate()
            .map(|(i, option)| radio(i, "us", question, option, required))
            .collect();
        let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        let want: Vec<&str> = if required { vec![question] } else { vec![] };
        assert_eq!(plan.missing, want);
    }
    // With a plain "No" in the group, that exact option is clicked.
    let fields = [
        radio(0, "us", question, "Yes", true),
        radio(1, "us", question, "No, but I would relocate", true),
        radio(2, "us", question, "No", true),
    ];
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].idx, 2);
}

#[test]
fn a_checkbox_group_does_not_guess_between_two_no_options_either() {
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(&["work location"], Some("No"))],
        consent_phrases: vec![],
    };
    let fields = checkbox_group(
        0,
        "Is your work location in the United States? (select one)",
        &[
            "Yes",
            "No, but I am willing to relocate",
            "No, and I am not willing to",
        ],
        true,
    );
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert_eq!(plan.missing.len(), 1);
}

fn race_options(options: &[&str], required: bool) -> Vec<FieldInfo> {
    options
        .iter()
        .enumerate()
        .map(|(i, option)| radio(i, "race", "Race", option, required))
        .collect()
}

#[test]
fn a_single_choice_race_list_with_a_hispanic_option_is_never_answered() {
    // In such a list "White" excludes "Hispanic or Latino": picking it answers the Hispanic
    // question, which Gonza keeps for himself. Whatever the exact wording of the options.
    let lists: [&[&str]; 3] = [
        &["Hispanic or Latino", "White", "Asian"],
        &["White", "Black or African American", "Hispanic/Latino"],
        &["White", "Latinx", "Decline to self-identify"],
    ];
    for list in lists {
        let required = plan_fields(&race_options(list, true), &race_bank(), Path::new("cv.pdf"));
        assert!(
            required.actions.is_empty(),
            "{list:?}: {:?}",
            required.actions
        );
        assert_eq!(required.missing, vec!["Race"], "{list:?}");
        let optional = plan_fields(
            &race_options(list, false),
            &race_bank(),
            Path::new("cv.pdf"),
        );
        assert!(optional.actions.is_empty(), "{list:?}");
        assert!(optional.missing.is_empty(), "{list:?}");
    }
    // A race list that does not mention Hispanic or Latino at all is answered as before.
    let plain = race_options(&["White", "Black or African American", "Asian"], true);
    let plan = plan_fields(&plain, &race_bank(), Path::new("cv.pdf"));
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].idx, 0);
}

#[test]
fn a_mark_all_that_apply_race_group_still_ticks_only_white() {
    // Several boxes may be ticked: White alone says nothing about the Hispanic option beside it.
    let question = "I identify my race/ethnicity as (mark all that apply): *";
    let fields = checkbox_group(
        0,
        question,
        &[
            "Asian",
            "Hispanic or Latino",
            "White",
            "Decline to self-identify",
        ],
        true,
    );
    let bank = AnswerBank {
        entries: vec![AnswerEntry::new(&["race/ethnicity"], Some("White"))],
        consent_phrases: vec![],
    };
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert!(plan.missing.is_empty(), "{:?}", plan.missing);
    let ticked: Vec<usize> = plan.actions.iter().map(|action| action.idx).collect();
    assert_eq!(ticked, vec![2]);
}

#[test]
fn a_drop_down_list_is_never_answered_when_it_offers_hispanic_or_latino() {
    let with = texts(&["White", "Asian", "Hispanic or Latino"]);
    let without = texts(&["White", "Asian"]);
    let race = texts(&["White"]);
    assert_eq!(options_named(&with, &race, false), Vec::<usize>::new());
    assert_eq!(options_named(&without, &race, false), vec![0]);
    // A multi-select list takes the answer whatever else it offers.
    assert_eq!(options_named(&with, &race, true), vec![0]);
}

#[test]
fn not_hispanic_said_in_another_wording_is_refused() {
    for option in [
        "White (Other than Hispanic or Latino)",
        "White, excluding Hispanic or Latino",
        "White (No Hispanic origin)",
        "White except Hispanic",
        "White without Hispanic or Latino origin",
        "Caucasian (other than Latinx)",
        "Black or African American (Excluding Latina)",
    ] {
        assert!(claims_not_hispanic(option), "{option}");
    }
    for option in [
        "Hispanic or Latino",
        "White",
        "Other",
        "Other (please specify)",
        "No answer",
        "I do not wish to answer",
        "Prefer not to say",
        "Hispanic or Latino (any race)",
    ] {
        assert!(!claims_not_hispanic(option), "{option}");
    }
}

#[test]
fn a_pre_ticked_marketing_box_is_reported_instead_of_going_out() {
    let ticked = |idx: usize, label: &str, required: bool| FieldInfo {
        checked: true,
        ..field(idx, "input", "checkbox", label, required)
    };
    let fields = [
        ticked(0, "Yes, text me job alerts by SMS", false),
        ticked(1, "Subscribe to our newsletter", false),
        ticked(2, "I agree to binding arbitration *", true),
        // Already ticked and a consent Gonza allowed: nothing to report.
        ticked(3, "I have read and accept the Privacy Policy *", true),
        // Not ticked: an optional newsletter box is simply left alone.
        field(4, "input", "checkbox", "Send me marketing emails", false),
        // Ticked, but not about anything that signs him up: not reported.
        ticked(5, "Remember me on this device", false),
    ];
    let allowed = AnswerBank {
        consent_phrases: vec!["privacy policy".into()],
        ..bank()
    };
    let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
    let reported: Vec<&str> = plan
        .missing
        .iter()
        .map(|label| label.split(" (").next().unwrap())
        .collect();
    assert_eq!(
        reported,
        vec![
            "Yes, text me job alerts by SMS",
            "Subscribe to our newsletter",
            "I agree to binding arbitration *"
        ]
    );
    // It is flagged red on the page (the box is optional, so only this makes it show), and
    // nothing was planned for it: the filler never unticks.
    assert_eq!(controls_left(&fields, &plan, &[]), vec![0, 1, 2]);
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert_eq!(plan.kept, vec![3]);
}

#[test]
fn a_pre_ticked_option_of_a_group_that_signs_him_up_is_reported() {
    let mut fields = checkbox_group(
        0,
        "How may we contact you? (select all that apply)",
        &["Email", "SMS", "Phone"],
        false,
    );
    fields[1].checked = true;
    let plan = plan_fields(&fields, &AnswerBank::default(), Path::new("cv.pdf"));
    assert_eq!(plan.missing.len(), 1, "{:?}", plan.missing);
    assert!(plan.missing[0].starts_with("How may we contact you?"));
    assert_eq!(controls_left(&fields, &plan, &[]), vec![1]);
    // The same group with nothing ticked reports nothing.
    let untouched = checkbox_group(
        0,
        "How may we contact you? (select all that apply)",
        &["Email", "SMS", "Phone"],
        false,
    );
    let plan = plan_fields(&untouched, &AnswerBank::default(), Path::new("cv.pdf"));
    assert!(plan.missing.is_empty());
}
